fn retention_history(
    store: &OwnedSqliteIndex,
    repository: RepositoryIdentityDigest,
    count: u8,
    salt_base: u8,
) -> Vec<GenerationId> {
    store
        .register_workspace(repository, 0, deadline())
        .expect("retention workspace should register");
    let mut generations = Vec::new();
    for ordinal in 0..count {
        let epoch = u64::from(ordinal);
        if ordinal != 0 {
            store
                .advance_source_epoch(repository, epoch - 1, epoch, deadline())
                .expect("retention source epoch should advance");
        }
        let suffix = format!("retention_{salt_base}_{ordinal}");
        let generation = store
            .stage(
                epoch,
                workspace_snapshot_identity(repository, salt_base.wrapping_add(ordinal)),
                prepared(&suffix),
                GenerationCoverage::new(2, 0, 0, 0),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("retention generation should stage");
        store
            .activate(generation, epoch, deadline())
            .expect("retention generation should activate");
        generations.push(generation);
    }
    generations
}

fn retention_policy(
    retained_floor: u16,
    limits: RetentionLimits,
    pins: RetentionPins,
) -> GenerationRetentionPolicy {
    GenerationRetentionPolicy::try_new(retained_floor, limits, pins)
        .expect("fixture retention policy should validate")
}

fn plan_retention(
    store: &OwnedSqliteIndex,
    policy: GenerationRetentionPolicy,
) -> super::RetentionPlan {
    store
        .plan_generation_retention(RetentionPlanRequest::new(
            policy,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("retention plan should complete")
}

struct RetentionMemoryOccurrence {
    workspace: i64,
    snapshot: Vec<u8>,
    path: Vec<u8>,
    content: Vec<u8>,
    artifact: Vec<u8>,
    fact_ordinal: i64,
    kind: String,
    name: String,
    qualified_name: String,
    name_start: i64,
    name_end: i64,
    declaration_start: i64,
    declaration_end: i64,
}

fn retention_memory_occurrence(
    connection: &Connection,
    generation: GenerationId,
) -> RetentionMemoryOccurrence {
    connection
        .query_row(
            "SELECT generation.workspace_id, generation.snapshot_digest,
                    file.repository_path, file.content_digest,
                    file.artifact_digest, fact.ordinal, fact.kind, fact.name,
                    fact.qualified_name, fact.name_start, fact.name_end,
                    fact.declaration_start, fact.declaration_end
             FROM index_generations AS generation
             JOIN generation_files AS file
               ON file.generation_id = generation.generation_id
             JOIN artifact_facts AS fact
               ON fact.artifact_digest = file.artifact_digest
             WHERE generation.generation_id = ?1
               AND NOT EXISTS (
                   SELECT 1 FROM generation_files AS other
                   WHERE other.artifact_digest = file.artifact_digest
                     AND other.generation_id != generation.generation_id
               )
             ORDER BY file.ordinal, fact.ordinal
             LIMIT 1",
            [generation.get()],
            |row| {
                Ok(RetentionMemoryOccurrence {
                    workspace: row.get(0)?,
                    snapshot: row.get(1)?,
                    path: row.get(2)?,
                    content: row.get(3)?,
                    artifact: row.get(4)?,
                    fact_ordinal: row.get(5)?,
                    kind: row.get(6)?,
                    name: row.get(7)?,
                    qualified_name: row.get(8)?,
                    name_start: row.get(9)?,
                    name_end: row.get(10)?,
                    declaration_start: row.get(11)?,
                    declaration_end: row.get(12)?,
                })
            },
        )
        .expect("retention memory occurrence should be readable")
}

fn insert_retention_memory_evidence(
    transaction: &rusqlite::Transaction<'_>,
    record_id: &[u8; 16],
    revision: &[u8; 32],
    source: &RetentionMemoryOccurrence,
) {
    transaction
        .execute(
            "INSERT INTO memory_evidence(
                workspace_id, record_id, revision_digest, ordinal,
                evidence_kind, source_snapshot_digest, repository_path,
                content_digest, artifact_digest, fact_ordinal, symbol_kind,
                name, qualified_name, name_start, name_length,
                declaration_start, declaration_length, declaration_digest,
                producer_id, producer_version
             ) VALUES (
                ?1, ?2, ?3, 0, 'rust_symbol', ?4, ?5, ?6, ?7, ?8, ?9,
                ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                'repowitness.rust.syntax', 'phase0-rust-syntax-v1'
             )",
            params![
                source.workspace,
                record_id.as_slice(),
                revision.as_slice(),
                source.snapshot.as_slice(),
                source.path.as_slice(),
                source.content.as_slice(),
                source.artifact.as_slice(),
                source.fact_ordinal,
                source.kind,
                source.name,
                source.qualified_name,
                source.name_start,
                source.name_end - source.name_start,
                source.declaration_start,
                source.declaration_end - source.declaration_start,
                [0xE3_u8; 32].as_slice(),
            ],
        )
        .expect("memory evidence root should insert");
}

fn insert_retention_memory_version(
    transaction: &rusqlite::Transaction<'_>,
    record_id: &[u8; 16],
    revision: &[u8; 32],
    source: &RetentionMemoryOccurrence,
) {
    transaction
        .execute(
            "INSERT INTO memory_versions(
                workspace_id, record_id, revision_digest, schema_version,
                canonical_json, kind, title, body, subject_evidence,
                provenance_origin, authored_actor_kind, authored_actor_id,
                authored_assurance, authored_lifecycle, validity_kind,
                validity_source_snapshot, tombstone
             ) VALUES (
                ?1, ?2, ?3, 1, X'7B7D', 'decision', 'Retain evidence',
                'Referenced evidence remains reproducible.', 0, 'human',
                'local_asserted', 'retention-test', 'locally_approved',
                'active', 'worktree', ?4, 0
             )",
            params![
                source.workspace,
                record_id.as_slice(),
                revision.as_slice(),
                source.snapshot.as_slice(),
            ],
        )
        .expect("memory version root should insert");
}

fn insert_retention_memory_audit(
    transaction: &rusqlite::Transaction<'_>,
    record_id: &[u8; 16],
    revision: &[u8; 32],
    source: &RetentionMemoryOccurrence,
) {
    transaction
        .execute(
            "INSERT INTO memory_audit(
                workspace_id, record_id, revision_digest, operation,
                trusted_actor_kind, trusted_actor_id, recorded_at_unix_ms,
                source_kind, source_format, source_revision,
                display_revision, presentation_digest
             ) VALUES (
                ?1, ?2, ?3, 'locally_approved', 'local_asserted',
                'retention-test', 1, 'worktree', 'source_snapshot', ?4,
                1, zeroblob(32)
             )",
            params![
                source.workspace,
                record_id.as_slice(),
                revision.as_slice(),
                source.snapshot.as_slice(),
            ],
        )
        .expect("append-only memory audit root should insert");
}

fn insert_retention_correspondence_audit(
    transaction: &rusqlite::Transaction<'_>,
    record_id: &[u8; 16],
    revision: &[u8; 32],
    source: &RetentionMemoryOccurrence,
    target: &RetentionMemoryOccurrence,
) {
    transaction
        .execute(
            "INSERT INTO memory_correspondence_audit(
                workspace_id, record_id, revision_digest, evidence_ordinal,
                operation, source_snapshot_digest, source_repository_path,
                source_artifact_digest, source_fact_ordinal,
                target_snapshot_digest, target_repository_path,
                target_artifact_digest, target_fact_ordinal,
                method_id, method_version, trusted_actor_kind,
                trusted_actor_id, recorded_at_unix_ms
             ) VALUES (
                ?1, ?2, ?3, 0, 'manual_link', ?4, ?5, ?6, ?7,
                ?8, ?9, ?10, ?11, 'manual-review', 1,
                'local_asserted', 'retention-test', 2
             )",
            params![
                source.workspace,
                record_id.as_slice(),
                revision.as_slice(),
                source.snapshot.as_slice(),
                source.path.as_slice(),
                source.artifact.as_slice(),
                source.fact_ordinal,
                target.snapshot.as_slice(),
                target.path.as_slice(),
                target.artifact.as_slice(),
                target.fact_ordinal,
            ],
        )
        .expect("correspondence audit roots should insert");
}

fn insert_retention_memory_roots(
    connection: &mut Connection,
    source: &RetentionMemoryOccurrence,
    target: &RetentionMemoryOccurrence,
) {
    let record_id = [0xE1_u8; 16];
    let revision = [0xE2_u8; 32];
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("memory-root transaction should begin");
    insert_retention_memory_evidence(&transaction, &record_id, &revision, source);
    insert_retention_memory_version(&transaction, &record_id, &revision, source);
    insert_retention_memory_audit(&transaction, &record_id, &revision, source);
    insert_retention_correspondence_audit(&transaction, &record_id, &revision, source, target);
    transaction
        .commit()
        .expect("memory roots should commit atomically");
}

fn retention_storage_counts(database: &Path) -> (i64, i64, i64, i64, i64, i64, i64) {
    let connection = Connection::open(database).expect("retention database should be readable");
    let counts = connection
        .query_row(
            "SELECT
                (SELECT count(*) FROM index_generations),
                (SELECT count(*) FROM source_snapshots),
                (SELECT count(*) FROM workspace_views),
                (SELECT count(*) FROM source_slot_generation_receipts),
                (SELECT count(*) FROM analysis_artifacts),
                (SELECT count(*) FROM retention_collection_audit),
                (SELECT count(*) FROM retention_generation_garbage)",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .expect("retention counts should be readable");
    let foreign_key_failures: i64 = connection
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .expect("foreign keys should remain valid");
    assert_eq!(foreign_key_failures, 0);
    counts
}

fn assert_collected_store_reopens(
    directory: &TempDirectory,
    repository: RepositoryIdentityDigest,
    expected_generation: GenerationId,
) {
    let (reopened, startup) = OwnedSqliteIndex::start(&directory.database(), 456, deadline())
        .expect("collected store should reopen");
    assert_eq!(startup.recovered_generations(), 0);
    assert_eq!(
        reopened
            .active_generation(repository, deadline())
            .expect("active generation should survive reopen"),
        Some(expected_generation)
    );
    reopened
        .shutdown(deadline())
        .expect("reopened store should stop");
}

#[test]
fn retention_plan_apply_is_bounded_atomic_and_idempotent() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0xD1; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("retention store should start");
    let generations = retention_history(&store, repository, 5, 0x20);
    let policy = retention_policy(2, RetentionLimits::default(), RetentionPins::default());
    let plan = plan_retention(&store, policy.clone());
    let repeated_plan = plan_retention(&store, policy.clone());

    assert_eq!(repeated_plan, plan);
    assert_eq!(plan.candidate_generations(), &generations[..2]);
    assert!(!plan.more_work());
    assert!(plan.estimated_rows() > 0);
    assert!(plan.estimated_bytes() >= plan.estimated_rows());

    let outcome = store
        .apply_generation_retention(RetentionApplyRequest::new(
            policy.clone(),
            plan.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("retention apply should commit");
    assert_eq!(outcome.generation_count(), 2);
    assert_eq!(outcome.workspace_view_count(), 2);
    assert_eq!(outcome.source_slot_receipt_count(), 2);
    assert_eq!(outcome.snapshot_count(), 2);
    assert_eq!(outcome.artifact_count(), 2);
    assert!(outcome.deleted_rows() > 0);

    let replay = store
        .apply_generation_retention(RetentionApplyRequest::new(
            policy.clone(),
            plan.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("an exact apply replay should be idempotent");
    assert_eq!(replay, outcome);
    assert_eq!(
        store
            .active_generation(repository, deadline())
            .expect("active generation should remain readable"),
        generations.last().copied()
    );

    let no_op = plan_retention(&store, policy.clone());
    assert!(no_op.candidate_generations().is_empty());
    let no_op_outcome = store
        .apply_generation_retention(RetentionApplyRequest::new(
            policy,
            no_op.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("empty retention plan should record a no-op");
    assert_eq!(no_op_outcome.generation_count(), 0);
    assert_eq!(no_op_outcome.deleted_rows(), 0);
    assert_ne!(no_op_outcome.collection_id(), outcome.collection_id());

    assert_eq!(
        retention_storage_counts(&directory.database()),
        (3, 3, 3, 3, 4, 2, 0)
    );
    store.shutdown(deadline()).expect("store should stop");
    assert_collected_store_reopens(
        &directory,
        repository,
        *generations.last().expect("active generation"),
    );
}

#[test]
fn retention_rejects_a_stale_plan_without_deleting_anything() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0xD2; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("retention store should start");
    let mut generations = retention_history(&store, repository, 3, 0x30);
    let policy = retention_policy(1, RetentionLimits::default(), RetentionPins::default());
    let plan = plan_retention(&store, policy.clone());
    assert_eq!(plan.candidate_generations(), &generations[..1]);

    store
        .advance_source_epoch(repository, 2, 3, deadline())
        .expect("source epoch should advance after planning");
    let newest = store
        .stage(
            3,
            workspace_snapshot_identity(repository, 0x34),
            prepared("retention-stale-successor"),
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("successor should stage");
    store
        .activate(newest, 3, deadline())
        .expect("successor should activate");
    generations.push(newest);

    assert_eq!(
        store.apply_generation_retention(RetentionApplyRequest::new(
            policy,
            plan.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )),
        Err(SqliteStoreError::RetentionPlanStale)
    );
    let connection =
        Connection::open(directory.database()).expect("stale-plan database should open");
    let generation_count: i64 = connection
        .query_row("SELECT count(*) FROM index_generations", [], |row| {
            row.get(0)
        })
        .expect("generation count should be readable");
    let audit_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM retention_collection_audit",
            [],
            |row| row.get(0),
        )
        .expect("audit count should be readable");
    assert_eq!(generation_count, 4);
    assert_eq!(audit_count, 0);
    drop(connection);
    store.shutdown(deadline()).expect("store should stop");
}

#[test]
fn all_generation_and_immutable_view_pin_kinds_fail_closed() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0xD3; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("retention store should start");
    let generations = retention_history(&store, repository, 5, 0x40);
    let connection =
        Connection::open(directory.database()).expect("pin fixture database should open");
    let pinned_view = connection
        .query_row(
            "SELECT workspace_view_id FROM workspace_view_members
             WHERE generation_id = ?1",
            [generations[2].get()],
            |row| row.get::<_, i64>(0),
        )
        .expect("historical view should be readable");
    drop(connection);
    let pins = RetentionPins::try_new(
        vec![generations[0]],
        vec![generations[1]],
        vec![WorkspaceViewId::from_database(pinned_view)],
    )
    .expect("bounded pins should validate");
    let policy = retention_policy(1, RetentionLimits::default(), pins);
    let plan = plan_retention(&store, policy);
    assert!(plan.candidate_generations().is_empty());

    let missing_view = WorkspaceViewId::from_database(i64::MAX);
    let invalid_pins = RetentionPins::try_new(Vec::new(), Vec::new(), vec![missing_view])
        .expect("syntactically valid missing view should construct");
    let invalid_policy = retention_policy(1, RetentionLimits::default(), invalid_pins);
    assert_eq!(
        store.plan_generation_retention(RetentionPlanRequest::new(
            invalid_policy,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )),
        Err(SqliteStoreError::RetentionPinUnavailable)
    );
    store.shutdown(deadline()).expect("store should stop");
}

#[test]
fn retention_batches_two_workspaces_in_canonical_source_slot_order() {
    let directory = TempDirectory::new();
    let first_repository = RepositoryIdentityDigest::new([0xD4; 32]);
    let second_repository = RepositoryIdentityDigest::new([0xD5; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("retention store should start");
    let first = retention_history(&store, first_repository, 3, 0x50);
    let second = retention_history(&store, second_repository, 3, 0x60);
    let policy = retention_policy(1, RetentionLimits::default(), RetentionPins::default());
    let plan = plan_retention(&store, policy.clone());
    let actual = plan
        .candidate_generations()
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let expected = [first[0], second[0]]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(actual, expected);

    let outcome = store
        .apply_generation_retention(RetentionApplyRequest::new(
            policy,
            plan.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("multi-workspace collection should commit");
    assert_eq!(outcome.generation_count(), 2);
    assert_eq!(
        store
            .active_generation(first_repository, deadline())
            .expect("first active generation should remain"),
        first.last().copied()
    );
    assert_eq!(
        store
            .active_generation(second_repository, deadline())
            .expect("second active generation should remain"),
        second.last().copied()
    );
    store.shutdown(deadline()).expect("store should stop");
}

#[test]
fn memory_evidence_and_append_only_audits_root_source_generations() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0xD8; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("retention store should start");
    let generations = retention_history(&store, repository, 5, 0x90);
    store
        .shutdown(deadline())
        .expect("store should stop for authoritative root injection");

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
    let policy = retention_policy(1, RetentionLimits::default(), RetentionPins::default());
    let plan = plan_retention(&reopened, policy);
    assert_eq!(plan.candidate_generations(), &generations[2..3]);
    reopened
        .shutdown(deadline())
        .expect("rooted store should stop");
}

#[test]
fn retention_limits_cancellation_and_deadline_are_enforced_before_sweep() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0xD6; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("retention store should start");
    let generations = retention_history(&store, repository, 4, 0x70);
    let one_candidate = RetentionLimits::try_new(1, 1_000_000, 512 * 1024 * 1024)
        .expect("one-candidate limits should validate");
    let bounded_policy = retention_policy(1, one_candidate, RetentionPins::default());
    let bounded = plan_retention(&store, bounded_policy.clone());
    assert_eq!(bounded.candidate_generations(), &generations[..1]);
    assert!(bounded.root_count() > 0);
    assert!(bounded.unresolved_count() > 0 || bounded.unresolved_truncated());
    assert!(bounded.logical_work_rows() <= bounded_policy.limits().max_rows());
    assert!(bounded.more_work());
    assert_eq!(
        store.apply_generation_retention(RetentionApplyRequest::new(
            bounded_policy.clone(),
            bounded.plan_digest(),
            Arc::new(AtomicBool::new(true)),
            deadline(),
        )),
        Err(SqliteStoreError::Cancelled)
    );
    assert_eq!(
        store.apply_generation_retention(RetentionApplyRequest::new(
            bounded_policy,
            bounded.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            Instant::now(),
        )),
        Err(SqliteStoreError::DeadlineExceeded)
    );

    let tiny_policy = retention_policy(
        1,
        RetentionLimits::try_new(1, 1, 1).expect("tiny positive limits should validate"),
        RetentionPins::default(),
    );
    assert_eq!(
        store.plan_generation_retention(RetentionPlanRequest::new(
            tiny_policy,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )),
        Err(SqliteStoreError::RetentionLimitExceeded)
    );
    let cancelled = Arc::new(AtomicBool::new(true));
    assert_eq!(
        store.plan_generation_retention(RetentionPlanRequest::new(
            GenerationRetentionPolicy::default(),
            cancelled,
            deadline(),
        )),
        Err(SqliteStoreError::Cancelled)
    );
    assert_eq!(
        store.plan_generation_retention(RetentionPlanRequest::new(
            GenerationRetentionPolicy::default(),
            Arc::new(AtomicBool::new(false)),
            Instant::now(),
        )),
        Err(SqliteStoreError::DeadlineExceeded)
    );
    store.shutdown(deadline()).expect("store should stop");
}
