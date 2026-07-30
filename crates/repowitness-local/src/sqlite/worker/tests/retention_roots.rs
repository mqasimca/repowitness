fn persisted_retention_root_count(connection: &Connection, retained_floor: u16) -> u64 {
    let count: i64 = connection
        .query_row(
            "WITH ranked AS (
                 SELECT slot.source_slot_id, generation.generation_id,
                        row_number() OVER (
                            PARTITION BY slot.source_slot_id
                            ORDER BY generation.source_epoch DESC,
                                     generation.generation_id DESC
                        ) AS retained_rank
                 FROM workspace_source_slots AS slot
                 JOIN index_generations AS generation
                   ON generation.workspace_id = slot.generation_workspace_id
                  AND generation.lifecycle_state = 'retained'
             )
             SELECT
                 (SELECT count(*) FROM workspaces)
                 + (SELECT count(*) FROM workspace_source_slots)
                 + (SELECT count(*) FROM active_workspace_views)
                 + (
                     SELECT count(*)
                     FROM source_slot_generation_receipts AS receipt
                     JOIN workspace_source_slots AS slot
                       ON slot.connected_workspace_id = receipt.connected_workspace_id
                      AND slot.source_slot_id = receipt.source_slot_id
                      AND slot.source_epoch = receipt.source_epoch
                 )
                 + (SELECT count(*) FROM ranked WHERE retained_rank <= ?1)
                 + (
                     SELECT count(*) FROM generation_graph_sources
                     WHERE source_generation_id != generation_id
                 )
                 + (SELECT count(*) FROM memory_projection_generations)
                 + (SELECT count(*) FROM memory_projection_generations)
                 + (
                     SELECT count(*) FROM memory_versions
                     WHERE validity_source_snapshot IS NOT NULL
                 )
                 + (SELECT count(*) FROM memory_evidence)
                 + (SELECT count(*) FROM memory_evidence)
                 + (
                     SELECT count(*) FROM memory_audit
                     WHERE source_format = 'source_snapshot'
                 )
                 + (SELECT count(*) FROM memory_correspondence_audit)
                 + (SELECT count(*) FROM memory_correspondence_audit)
                 + (SELECT count(*) FROM memory_correspondence_audit)
                 + (SELECT count(*) FROM memory_correspondence_audit)
                 + (
                     SELECT count(*) FROM memory_projection_evidence
                     WHERE target_snapshot_digest IS NOT NULL
                 )
                 + (
                     SELECT count(*) FROM memory_projection_evidence
                     WHERE target_artifact_digest IS NOT NULL
                 )
                 + (SELECT count(*) FROM memory_projection_candidates)
                 + (SELECT count(*) FROM memory_projection_candidates)",
            [i64::from(retained_floor)],
            |row| row.get(0),
        )
        .expect("aggregate persisted root count should be readable");
    u64::try_from(count).expect("aggregate persisted root count should be nonnegative")
}

#[test]
fn floor_and_memory_journal_relations_are_counted_hashed_and_bounded_as_roots() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0xDC; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("retention store should start");
    let generations = retention_history(&store, repository, 6, 0xC0);

    let floor_one = plan_retention(
        &store,
        retention_policy(1, RetentionLimits::default(), RetentionPins::default()),
    );
    let floor_all_policy =
        retention_policy(5, RetentionLimits::default(), RetentionPins::default());
    let before_memory = plan_retention(&store, floor_all_policy.clone());
    assert_eq!(
        before_memory.root_count(),
        floor_one.root_count() + 4,
        "each additional per-slot retained-floor relation is one root"
    );
    assert!(before_memory.candidate_generations().is_empty());
    store
        .shutdown(deadline())
        .expect("store should stop for root injection");

    let mut connection =
        Connection::open(directory.database()).expect("memory-root database should open");
    connection
        .pragma_update(None, "foreign_keys", true)
        .expect("foreign keys should enable");
    let source = retention_memory_occurrence(&connection, generations[0]);
    let target = retention_memory_occurrence(&connection, generations[1]);
    insert_retention_memory_roots(&mut connection, &source, &target);
    drop(connection);

    let (reopened, _) = OwnedSqliteIndex::start(&directory.database(), 456, deadline())
        .expect("rooted store should reopen");
    let after_memory = plan_retention(&reopened, floor_all_policy);
    let connection =
        Connection::open(directory.database()).expect("rooted database should be readable");
    assert_eq!(
        after_memory.root_count(),
        persisted_retention_root_count(&connection, 5)
    );
    assert_eq!(
        after_memory.root_count(),
        before_memory.root_count() + 8,
        "one evidence, version, audit, and correspondence row contributes eight root relations"
    );
    assert_eq!(
        after_memory.logical_work_rows(),
        before_memory.logical_work_rows() + 8
    );
    assert_ne!(after_memory.plan_digest(), before_memory.plan_digest());
    drop(connection);

    let exact_rows = after_memory.logical_work_rows();
    let exact_policy = retention_policy(
        5,
        RetentionLimits::try_new(64, exact_rows, 512 * 1024 * 1024)
            .expect("exact memory-root limit should validate"),
        RetentionPins::default(),
    );
    assert_eq!(
        plan_retention(&reopened, exact_policy).logical_work_rows(),
        exact_rows
    );
    let one_under_policy = retention_policy(
        5,
        RetentionLimits::try_new(64, exact_rows - 1, 512 * 1024 * 1024)
            .expect("one-under memory-root limit should validate"),
        RetentionPins::default(),
    );
    assert_eq!(
        reopened.plan_generation_retention(RetentionPlanRequest::new(
            one_under_policy,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )),
        Err(SqliteStoreError::RetentionLimitExceeded)
    );
    reopened.shutdown(deadline()).expect("store should stop");
}

#[allow(
    clippy::too_many_lines,
    reason = "one state sequence proves the graph root's digest, shared budget, exclusion, and safe-sweep invariants"
)]
#[test]
fn cross_generation_graph_source_is_hashed_bounded_excluded_and_apply_safe() {
    let directory = TempDirectory::new();
    let first_repository = RepositoryIdentityDigest::new([0xA1; 32]);
    let second_repository = RepositoryIdentityDigest::new([0xA2; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("retention store should start");
    retention_history(&store, first_repository, 3, 0xA0);
    let second_generations = retention_history(&store, second_repository, 4, 0xB0);
    let rooted_generation = second_generations[0];
    let connected = ConnectedWorkspaceId::new([0xA3; 32]);
    let first_slot = SourceSlotId::new([0xA4; 32]);
    let second_slot = SourceSlotId::new([0xA5; 32]);
    store
        .connect_workspace(
            connected,
            vec![
                WorkspaceSourceSlot::new(first_slot, first_repository),
                WorkspaceSourceSlot::new(second_slot, second_repository),
            ],
            workspace_control(),
            deadline(),
        )
        .expect("graph source workspace should connect");
    store
        .advance_source_epoch(first_repository, 2, 3, deadline())
        .expect("graph owner source epoch should advance");
    let graph_owner = store
        .stage(
            3,
            workspace_snapshot_identity(first_repository, 0xC0),
            prepared("cross_generation_graph_owner"),
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("graph owner generation should stage");

    let no_candidate_policy =
        retention_policy(4, RetentionLimits::default(), RetentionPins::default());
    let before_graph = plan_retention(&store, no_candidate_policy.clone());
    assert!(before_graph.candidate_generations().is_empty());
    let collectible_policy =
        retention_policy(1, RetentionLimits::default(), RetentionPins::default());
    let collectible_before = plan_retention(&store, collectible_policy.clone());
    assert!(
        collectible_before
            .candidate_generations()
            .contains(&rooted_generation),
        "the old source generation should be eligible before the graph references it"
    );

    let resolution_cancelled = AtomicBool::new(false);
    let empty_resolution = repowitness_analysis::resolve_rust_graph_sites(
        &[],
        &[],
        repowitness_analysis::RustGraphResolutionLimits::DEFAULT,
        repowitness_analysis::RustGraphResolutionControl::new(
            &resolution_cancelled,
            deadline(),
        ),
    )
    .expect("empty graph should resolve categorically");
    let graph = crate::sqlite::prepare_rust_graph_generation(
        connected,
        vec![
            crate::sqlite::RustGraphSource::new(first_slot, graph_owner),
            crate::sqlite::RustGraphSource::new(second_slot, rooted_generation),
        ],
        Vec::new(),
        Vec::new(),
        empty_resolution,
        crate::sqlite::RustGraphPreparationControl::new(&resolution_cancelled, deadline()),
    )
    .expect("cross-generation graph should prepare");
    store
        .stage_rust_graph(
            graph_owner,
            graph,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("cross-generation graph should stage");

    let after_graph = plan_retention(&store, no_candidate_policy);
    assert!(after_graph.candidate_generations().is_empty());
    assert_eq!(after_graph.root_count(), before_graph.root_count() + 1);
    assert_eq!(
        after_graph.logical_work_rows(),
        before_graph.logical_work_rows() + 1,
        "the graph-source root must consume the shared logical-row budget"
    );
    assert_ne!(after_graph.plan_digest(), before_graph.plan_digest());
    let connection =
        Connection::open(directory.database()).expect("graph-root database should be readable");
    assert_eq!(
        after_graph.root_count(),
        persisted_retention_root_count(&connection, 4)
    );
    drop(connection);

    let exact_rows = after_graph.logical_work_rows();
    let exact_policy = retention_policy(
        4,
        RetentionLimits::try_new(64, exact_rows, 512 * 1024 * 1024)
            .expect("exact graph-root limit should validate"),
        RetentionPins::default(),
    );
    assert_eq!(
        plan_retention(&store, exact_policy).logical_work_rows(),
        exact_rows
    );
    let one_under_policy = retention_policy(
        4,
        RetentionLimits::try_new(64, exact_rows - 1, 512 * 1024 * 1024)
            .expect("one-under graph-root limit should validate"),
        RetentionPins::default(),
    );
    assert_eq!(
        store.plan_generation_retention(RetentionPlanRequest::new(
            one_under_policy,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )),
        Err(SqliteStoreError::RetentionLimitExceeded)
    );

    let collectible_after = plan_retention(&store, collectible_policy.clone());
    assert!(
        !collectible_after
            .candidate_generations()
            .contains(&rooted_generation),
        "a cross-generation graph source must not be a retention candidate"
    );
    store
        .activate(graph_owner, 3, deadline())
        .expect("complete graph owner should activate");
    let apply_plan = plan_retention(&store, collectible_policy.clone());
    assert!(!apply_plan.candidate_generations().is_empty());
    assert!(
        !apply_plan
            .candidate_generations()
            .contains(&rooted_generation)
    );
    assert!(
        !apply_plan
            .candidate_generations()
            .contains(&graph_owner)
    );
    let expected_deleted = u64::try_from(apply_plan.candidate_generations().len())
        .expect("bounded candidate count should fit");
    let outcome = store
        .apply_generation_retention(RetentionApplyRequest::new(
            collectible_policy,
            apply_plan.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("unrelated retention candidates should collect safely");
    assert_eq!(outcome.generation_count(), expected_deleted);

    let connection =
        Connection::open(directory.database()).expect("collected graph-root database should open");
    let protected: (String, i64) = connection
        .query_row(
            "SELECT generation.lifecycle_state,
                    (SELECT count(*) FROM generation_graph_sources
                     WHERE generation_id = ?1 AND source_generation_id = ?2)
             FROM index_generations AS generation
             WHERE generation.generation_id = ?2",
            params![graph_owner.get(), rooted_generation.get()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("cross-generation graph source should remain protected");
    assert_eq!(protected, ("retained".to_owned(), 1));
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("foreign-key check should complete"),
        0
    );
    drop(connection);
    assert_eq!(
        store
            .active_generation(first_repository, deadline())
            .expect("graph owner should remain active"),
        Some(graph_owner)
    );
    store.shutdown(deadline()).expect("store should stop");
}

#[test]
fn current_projection_evidence_and_candidates_are_counted_and_hashed_as_roots() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (store, ambiguous, repository) = prepared_memory_projection_fixture(&database);
    let policy = retention_policy(1, RetentionLimits::default(), RetentionPins::default());
    let before_projection = plan_retention(&store, policy.clone());
    let before_connection =
        Connection::open(&database).expect("pre-projection database should be readable");
    assert_eq!(
        before_projection.root_count(),
        persisted_retention_root_count(&before_connection, 1)
    );
    drop(before_connection);

    store
        .publish_memory_projection(
            ambiguous,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("ambiguous projection should publish");
    let after_ambiguous = plan_retention(&store, policy.clone());
    let ambiguous_connection =
        Connection::open(&database).expect("ambiguous projection should be readable");
    assert_eq!(
        after_ambiguous.root_count(),
        persisted_retention_root_count(&ambiguous_connection, 1)
    );
    assert_eq!(
        after_ambiguous.root_count(),
        before_projection.root_count() + 4,
        "the projection generation and ambiguous candidate each contribute two root relations"
    );
    drop(ambiguous_connection);

    let exact = exact_memory_projection(&store, repository);
    store
        .publish_memory_projection(exact, Arc::new(AtomicBool::new(false)), deadline())
        .expect("exact projection should publish");
    let after_exact = plan_retention(&store, policy);
    let exact_connection =
        Connection::open(&database).expect("exact projection should be readable");
    assert_eq!(
        after_exact.root_count(),
        persisted_retention_root_count(&exact_connection, 1)
    );
    assert_eq!(
        after_exact.root_count(),
        after_ambiguous.root_count() + 4,
        "the second projection generation and its exact evidence each contribute two root relations"
    );
    assert_eq!(
        after_exact.logical_work_rows(),
        before_projection.logical_work_rows() + 8
    );
    assert_ne!(after_exact.plan_digest(), before_projection.plan_digest());
    drop(exact_connection);
    store.shutdown(deadline()).expect("store should stop");
}

fn exact_memory_projection(
    store: &OwnedSqliteIndex,
    repository: RepositoryIdentityDigest,
) -> PreparedMemoryProjection {
    let (record, revision, _) = memory_input(COMMIT_MEMORY_YAML);
    let journal = store
        .load_memory_journal(
            repository,
            MemoryProjectionLoadLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("journal should load");
    let MemoryEvidence::RustSymbol(evidence) = &record.evidence()[0];
    let candidates = store
        .load_rust_memory_candidates(
            journal.source(),
            evidence.clone(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("candidate set should load")
        .into_candidates();
    assert_eq!(candidates.len(), 1);
    let evidence_result = PreparedProjectionEvidence::resolved(
        ProjectionEvidenceOutcome::Exact,
        ProjectionEvidenceAssurance::Automatic,
        ProjectionOccurrence::from_candidate(&candidates[0]),
        1,
    )
    .expect("exact evidence should validate");
    let decision = evaluate_memory_projection(
        &record,
        Some(MemoryProjectValidity::Valid),
        &[MemoryEvidenceOutcome::Exact],
    )
    .expect("exact projection decision should validate");
    PreparedMemoryProjection::try_new(
        journal.source(),
        MemoryRevalidationTarget::worktree(
            journal.source().snapshot(),
            Some(MemoryCommitId::Sha1([0x11; 20])),
        ),
        vec![PreparedProjectionRecord {
            record_id: record.header().record_id(),
            kind: PreparedProjectionRecordKind::Evaluated {
                revision,
                decision,
                evidence: vec![evidence_result],
            },
        }],
        0,
        0,
        MemoryProjectionResultLimits::default(),
    )
    .expect("exact projection should prepare")
}
