fn commit_failure_writer(database: &Path) -> (OwnedSqliteIndex, Arc<AtomicBool>) {
    let (store, _, fail_next_commit) =
        OwnedSqliteIndex::start_with_commit_failure_control(database, 123, deadline())
            .expect("fault-controlled writer should start");
    (store, fail_next_commit)
}

fn arm_commit_failure(control: &AtomicBool) {
    assert!(
        !control.swap(true, Ordering::AcqRel),
        "a prior commit failure must not remain armed"
    );
}

fn database_count(database: &Path, sql: &str) -> i64 {
    Connection::open(database)
        .expect("database should reopen")
        .query_row(sql, [], |row| row.get(0))
        .expect("fixture count should be readable")
}

#[test]
fn workspace_registration_commit_failure_is_outcome_unknown_and_rolled_back() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x91; 32]);
    let (store, fail_next_commit) = commit_failure_writer(&database);

    arm_commit_failure(&fail_next_commit);
    assert_eq!(
        store.register_workspace(repository, 0, deadline()),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    store.shutdown(deadline()).expect("writer should stop");

    assert_eq!(
        database_count(&database, "SELECT count(*) FROM workspaces"),
        0
    );
    let (reopened, _) =
        OwnedSqliteIndex::start(&database, 124, deadline()).expect("store should reopen");
    reopened
        .register_workspace(repository, 0, deadline())
        .expect("reconciled registration should succeed");
    reopened.shutdown(deadline()).expect("writer should stop");
}

#[test]
fn workspace_ensure_commit_failure_is_outcome_unknown_and_rolled_back() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x90; 32]);
    let (store, fail_next_commit) = commit_failure_writer(&database);

    arm_commit_failure(&fail_next_commit);
    assert_eq!(
        store.ensure_workspace(repository, 0, deadline()),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    store.shutdown(deadline()).expect("writer should stop");
    assert_eq!(
        database_count(&database, "SELECT count(*) FROM workspaces"),
        0
    );
}

#[test]
fn source_epoch_commit_failures_are_outcome_unknown_and_rolled_back() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x92; 32]);
    let connected = ConnectedWorkspaceId::new([0x93; 32]);
    let source_slot = SourceSlotId::new([0x94; 32]);
    let (store, fail_next_commit) = commit_failure_writer(&database);
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");

    arm_commit_failure(&fail_next_commit);
    assert_eq!(
        store.advance_source_epoch(repository, 0, 1, deadline()),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    assert_eq!(persisted_workspace_epoch(&database), 0);
    store
        .shutdown(deadline())
        .expect("fenced writer should stop before reconciliation");

    let (store, fail_next_commit) = commit_failure_writer(&database);
    connected_single_slot(&store, repository, connected, source_slot);
    arm_commit_failure(&fail_next_commit);
    assert_eq!(
        store.reserve_source_slot_epoch(
            connected,
            source_slot,
            SourceSlotEpoch::INITIAL,
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    let state = store
        .source_slot_state(connected, source_slot, workspace_control(), deadline())
        .expect("rolled-back source-slot state should load");
    assert_eq!(state.current_epoch(), SourceSlotEpoch::INITIAL);
    store.shutdown(deadline()).expect("writer should stop");
}

#[test]
fn generation_staging_commit_failure_is_outcome_unknown_and_recoverable() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = snapshot_identity().repository();
    let (store, fail_next_commit) = commit_failure_writer(&database);
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");

    arm_commit_failure(&fail_next_commit);
    assert_eq!(
        store.stage(
            0,
            snapshot_identity(),
            prepared("commit_failure_stage"),
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    store.shutdown(deadline()).expect("writer should stop");
    assert_eq!(
        database_count(&database, "SELECT count(*) FROM source_manifest_entries"),
        0
    );

    let (reopened, _) =
        OwnedSqliteIndex::start(&database, 124, deadline()).expect("store should reopen");
    reopened
        .stage(
            0,
            snapshot_identity(),
            prepared("commit_failure_stage"),
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("reconciled staging should succeed");
    reopened.shutdown(deadline()).expect("writer should stop");
}

#[test]
fn generation_activation_commit_failure_is_outcome_unknown_and_rolled_back() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = snapshot_identity().repository();
    let (store, fail_next_commit) = commit_failure_writer(&database);
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let generation = store
        .stage(
            0,
            snapshot_identity(),
            prepared("commit-failure-activation"),
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("generation should stage");

    arm_commit_failure(&fail_next_commit);
    assert_eq!(
        store.activate(generation, 0, deadline()),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    assert_eq!(
        store
            .active_generation(repository, deadline())
            .expect("active generation should remain readable"),
        None
    );
    store.shutdown(deadline()).expect("writer should stop");
}

#[test]
fn connected_workspace_commit_failures_are_outcome_unknown_and_rolled_back() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x95; 32]);
    let connected = ConnectedWorkspaceId::new([0x96; 32]);
    let source_slot = SourceSlotId::new([0x97; 32]);
    let (store, fail_next_commit) = commit_failure_writer(&database);
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");

    arm_commit_failure(&fail_next_commit);
    assert_eq!(
        store.connect_workspace(
            connected,
            vec![WorkspaceSourceSlot::new(source_slot, repository)],
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    store.shutdown(deadline()).expect("writer should stop");
    assert_eq!(
        database_count(&database, "SELECT count(*) FROM connected_workspaces"),
        1,
        "single-repository registration should leave only its default workspace"
    );
    assert_eq!(
        database_count(
            &database,
            "SELECT count(*) FROM connected_workspaces
             WHERE connected_workspace_id = x'9696969696969696969696969696969696969696969696969696969696969696'"
        ),
        0
    );
}

#[test]
fn source_slot_completion_commit_failure_is_outcome_unknown_and_rolled_back() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x98; 32]);
    let connected = ConnectedWorkspaceId::new([0x99; 32]);
    let source_slot = SourceSlotId::new([0x9A; 32]);
    let (store, fail_next_commit) = commit_failure_writer(&database);
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let generation =
        stage_workspace_generation(&store, repository, 0x9B, "commit-failure-completion");
    connected_single_slot(&store, repository, connected, source_slot);

    arm_commit_failure(&fail_next_commit);
    assert_eq!(
        store.complete_source_slot_epoch(
            connected,
            source_slot,
            SourceSlotEpoch::INITIAL,
            generation,
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    let state = store
        .source_slot_state(connected, source_slot, workspace_control(), deadline())
        .expect("rolled-back source-slot state should load");
    assert_eq!(state.current_completion(), None);
    store.shutdown(deadline()).expect("writer should stop");
}

#[test]
fn workspace_view_commit_failure_is_outcome_unknown_and_rolled_back() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x9C; 32]);
    let connected = ConnectedWorkspaceId::new([0x9D; 32]);
    let source_slot = SourceSlotId::new([0x9E; 32]);
    let (store, fail_next_commit) = commit_failure_writer(&database);
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let generation = stage_workspace_generation(&store, repository, 0x9F, "commit-failure-view");
    connected_single_slot(&store, repository, connected, source_slot);
    complete_slot(&store, connected, source_slot, generation);

    arm_commit_failure(&fail_next_commit);
    assert_eq!(
        store.publish_workspace_view(
            connected,
            vec![WorkspaceViewMember::new(source_slot, generation)],
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    assert_eq!(
        store
            .active_workspace_view(connected, workspace_control(), deadline())
            .expect("active view query should succeed"),
        None
    );
    store.shutdown(deadline()).expect("writer should stop");
}

#[test]
fn graph_staging_commit_failure_is_outcome_unknown() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0xA0; 32]);
    let (store, fail_next_commit) = commit_failure_writer(&database);
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let (generation, graph) = graph_candidate(&store, repository, "commit_failure");

    arm_commit_failure(&fail_next_commit);
    assert_eq!(
        store.stage_rust_graph(
            generation,
            graph,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    store.shutdown(deadline()).expect("writer should stop");
    assert_eq!(
        database_count(
            &database,
            "SELECT count(*) FROM analysis_artifacts
             WHERE lifecycle_state = 'staging' AND language = 'rust'"
        ),
        0
    );
}

#[test]
fn projection_rebuild_commit_failure_is_not_masked_by_progress_cleanup() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (store, fail_next_commit) = commit_failure_writer(&database);

    arm_commit_failure(&fail_next_commit);
    assert_eq!(
        store.rebuild_search_projection(
            ProjectionRebuildLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    store
        .shutdown(deadline())
        .expect("fenced writer should stop before reconciliation");
    let (store, _) =
        OwnedSqliteIndex::start(&database, 456, deadline()).expect("store should reopen");
    store
        .rebuild_search_projection(
            ProjectionRebuildLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("a rebuild after reconciliation should succeed");
    store.shutdown(deadline()).expect("writer should stop");
}

#[test]
fn retention_apply_commit_failure_is_outcome_unknown_and_rolled_back() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0xA1; 32]);
    let (store, fail_next_commit) = commit_failure_writer(&database);
    let generations = retention_history(&store, repository, 3, 0xA2);
    let policy = retention_policy(1, RetentionLimits::default(), RetentionPins::default());
    let plan = plan_retention(&store, policy.clone());

    arm_commit_failure(&fail_next_commit);
    assert_eq!(
        store.apply_generation_retention(RetentionApplyRequest::new(
            policy,
            plan.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    assert_eq!(
        store
            .active_generation(repository, deadline())
            .expect("active generation should remain readable"),
        generations.last().copied()
    );
    assert_eq!(
        database_count(&database, "SELECT count(*) FROM retention_collection_audit"),
        0
    );
    store.shutdown(deadline()).expect("writer should stop");
}

#[test]
fn committed_retention_replay_returns_receipt_without_consuming_mutation_fault() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0xA3; 32]);
    let (store, fail_next_commit) = commit_failure_writer(&database);
    retention_history(&store, repository, 3, 0xA4);
    let policy = retention_policy(1, RetentionLimits::default(), RetentionPins::default());
    let plan = plan_retention(&store, policy.clone());
    let expected = store
        .apply_generation_retention(RetentionApplyRequest::new(
            policy.clone(),
            plan.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("initial retention apply should commit");

    arm_commit_failure(&fail_next_commit);
    let replayed = store
        .apply_generation_retention(RetentionApplyRequest::new(
            policy,
            plan.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("committed replay should return its authoritative receipt");
    assert_eq!(replayed, expected);
    assert!(
        fail_next_commit.swap(false, Ordering::AcqRel),
        "read-only replay must not consume the next mutation fault"
    );
    store.shutdown(deadline()).expect("writer should stop");
}
