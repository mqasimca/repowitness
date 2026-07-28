#[test]
fn process_mutation_lease_prevents_competing_recovery_and_releases_on_shutdown() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (first, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("first owner should start");
    let repository = snapshot_identity().repository();
    first
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let ready = first
        .stage(
            0,
            snapshot_identity(),
            prepared("owned"),
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("first owner should retain its ready generation");

    let competing_deadline = Instant::now()
        .checked_add(Duration::from_millis(30))
        .expect("competing deadline should be representable");
    let competing_result = OwnedSqliteIndex::start(
        &directory.0.join(".").join("index.sqlite3"),
        456,
        competing_deadline,
    );
    assert!(matches!(
        competing_result,
        Err(SqliteStoreError::DeadlineExceeded)
    ));
    first
        .activate(ready, 0, deadline())
        .expect("competing startup must not invalidate ready work");
    first.shutdown(deadline()).expect("first owner should stop");

    let (replacement, startup) = OwnedSqliteIndex::start(&database, 789, deadline())
        .expect("the lease should release with its owner");
    assert_eq!(startup.recovered_generations(), 0);
    assert_eq!(
        replacement
            .active_generation(repository, deadline())
            .expect("active generation should survive owner replacement"),
        Some(ready)
    );
    replacement
        .shutdown(deadline())
        .expect("replacement owner should stop");
}

#[test]
fn unavailable_mutation_lease_fails_before_database_creation() {
    let directory = TempDirectory::new();
    let database = directory.database();
    fs::create_dir(mutation_lease_path(&database))
        .expect("fixture should make the lease path unopenable");

    let result = OwnedSqliteIndex::start(&database, 123, deadline());
    assert!(matches!(
        result,
        Err(SqliteStoreError::MutationLeaseUnavailable)
    ));
    assert!(!database.exists());
}

#[test]
fn stale_and_cancelled_candidates_never_replace_active() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let repository = snapshot_identity().repository();
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let first = store
        .stage(
            0,
            snapshot_identity(),
            prepared("v1"),
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("first generation should stage");
    store
        .activate(first, 0, deadline())
        .expect("first generation should activate");

    let cancelled = Arc::new(AtomicBool::new(true));
    assert_eq!(
        store
            .stage(
                0,
                snapshot_identity(),
                prepared("cancelled"),
                GenerationCoverage::new(2, 0, 0, 0),
                cancelled,
                deadline(),
            )
            .expect_err("cancelled work should fail"),
        SqliteStoreError::Cancelled
    );
    store
        .advance_source_epoch(repository, 0, 1, deadline())
        .expect("source epoch should advance");
    assert_eq!(
        store
            .activate(first, 0, deadline())
            .expect_err("stale activation should fail"),
        SqliteStoreError::StaleSourceEpoch
    );
    assert_eq!(
        store
            .active_generation(repository, deadline())
            .expect("previous generation should remain active"),
        Some(first)
    );
    store.shutdown(deadline()).expect("worker should stop");
}

#[test]
fn restart_marks_ready_generation_failed_and_removes_scoped_rows() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let repository = snapshot_identity().repository();
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let ready = store
        .stage(
            0,
            snapshot_identity(),
            prepared("unpublished"),
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("candidate should become ready");
    store.shutdown(deadline()).expect("worker should stop");

    let (store, startup) = OwnedSqliteIndex::start(&directory.database(), 456, deadline())
        .expect("owned store should recover");
    assert_eq!(startup.recovered_generations(), 1);
    assert_eq!(
        store
            .active_generation(repository, deadline())
            .expect("workspace should remain readable"),
        None
    );
    store.shutdown(deadline()).expect("worker should stop");

    let connection =
        Connection::open(directory.database()).expect("database should reopen for inspection");
    let state: String = connection
        .query_row(
            "SELECT lifecycle_state FROM index_generations WHERE generation_id = ?1",
            [ready.get()],
            |row| row.get(0),
        )
        .expect("recovered generation should remain auditable");
    let scoped_rows: i64 = connection
        .query_row(
            "SELECT
                    (SELECT count(*) FROM generation_files WHERE generation_id = ?1) +
                    (SELECT count(*) FROM generation_facts WHERE generation_id = ?1) +
                    (SELECT count(*) FROM generation_search WHERE generation_id = ?1)",
            [ready.get()],
            |row| row.get(0),
        )
        .expect("scoped rows should be inspectable");
    assert_eq!(state, "failed");
    assert_eq!(scoped_rows, 0);
}

fn insert_incomplete_generation_fixture(database: &Path, generation_count: usize) {
    let mut connection =
        Connection::open(database).expect("fixture database should reopen for insertion");
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("fixture transaction should start");
    transaction
        .execute(
            "INSERT INTO workspaces(
                    workspace_id, repository_identity, source_epoch
                 ) VALUES (1, zeroblob(32), 0)",
            [],
        )
        .expect("fixture workspace should be inserted");
    transaction
        .execute(
            "INSERT INTO source_snapshots(
                    snapshot_digest, lifecycle_state, repository_identity,
                    git_state_digest, worktree_state_digest, configuration_digest,
                    producer_manifest_digest, analysis_schema_digest,
                    canonicalization_version, manifest_digest, file_count,
                    total_source_bytes, total_syntax_error_nodes
                 ) VALUES (
                    zeroblob(32), 'complete', zeroblob(32), zeroblob(32),
                    zeroblob(32), zeroblob(32), zeroblob(32), zeroblob(32),
                    1, zeroblob(32), 0, 0, 0
                 )",
            [],
        )
        .expect("fixture snapshot should be inserted");
    for generation_id in 1..=generation_count {
        let generation_id =
            i64::try_from(generation_id).expect("fixture generation should fit in SQLite");
        transaction
            .execute(
                "INSERT INTO index_generations(
                        generation_id, workspace_id, source_epoch,
                        snapshot_digest, lifecycle_state
                     ) VALUES (?1, 1, 0, zeroblob(32), 'discovered')",
                params![generation_id],
            )
            .expect("fixture generation should be inserted");
    }
    transaction.commit().expect("fixture rows should commit");
}

#[test]
fn startup_recovery_limit_fails_without_partially_changing_generations() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (store, _) = OwnedSqliteIndex::start(&database, 123, deadline())
        .expect("owned store should initialize the schema");
    store.shutdown(deadline()).expect("worker should stop");

    insert_incomplete_generation_fixture(&database, MAX_STARTUP_RECOVERY_GENERATIONS + 1);

    let error = match OwnedSqliteIndex::start(&database, 456, deadline()) {
        Ok(_) => panic!("over-limit recovery should fail"),
        Err(error) => error,
    };
    assert_eq!(error, SqliteStoreError::RecoveryGenerationLimitExceeded);

    let connection =
        Connection::open(&database).expect("fixture database should reopen for validation");
    let discovered: i64 = connection
        .query_row(
            "SELECT count(*) FROM index_generations
                 WHERE lifecycle_state = 'discovered'",
            [],
            |row| row.get(0),
        )
        .expect("fixture generations should remain queryable");
    assert_eq!(
        discovered,
        i64::try_from(MAX_STARTUP_RECOVERY_GENERATIONS + 1)
            .expect("fixture count should fit in SQLite")
    );
    connection
        .execute(
            "DELETE FROM index_generations
                 WHERE generation_id = ?1",
            params![
                i64::try_from(MAX_STARTUP_RECOVERY_GENERATIONS + 1)
                    .expect("fixture generation should fit in SQLite")
            ],
        )
        .expect("one fixture generation should be removed");
    drop(connection);

    let (store, startup) = OwnedSqliteIndex::start(&database, 789, deadline())
        .expect("the inclusive recovery limit should succeed");
    assert_eq!(
        startup.recovered_generations(),
        u64::try_from(MAX_STARTUP_RECOVERY_GENERATIONS)
            .expect("fixture count should fit in the report")
    );
    store.shutdown(deadline()).expect("worker should stop");

    let connection =
        Connection::open(&database).expect("recovered database should reopen for validation");
    let failed: i64 = connection
        .query_row(
            "SELECT count(*) FROM index_generations
                 WHERE lifecycle_state = 'failed'",
            [],
            |row| row.get(0),
        )
        .expect("recovered generations should remain queryable");
    assert_eq!(
        failed,
        i64::try_from(MAX_STARTUP_RECOVERY_GENERATIONS)
            .expect("fixture count should fit in SQLite")
    );
}

#[test]
fn restart_removes_incomplete_snapshot_and_artifact_staging() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    store.shutdown(deadline()).expect("worker should stop");
    let connection =
        Connection::open(directory.database()).expect("database should reopen for fixture");
    connection
        .execute(
            "INSERT INTO source_snapshots(
                    snapshot_digest, lifecycle_state, repository_identity, git_state_digest,
                    worktree_state_digest, configuration_digest, producer_manifest_digest,
                    analysis_schema_digest, canonicalization_version, manifest_digest,
                    file_count, total_source_bytes, total_syntax_error_nodes
                 ) VALUES (
                    zeroblob(32), 'staging', zeroblob(32), zeroblob(32),
                    zeroblob(32), zeroblob(32), zeroblob(32), zeroblob(32),
                    1, zeroblob(32), 0, 0, 0
                 )",
            [],
        )
        .expect("incomplete snapshot fixture should insert");
    connection
        .execute(
            "INSERT INTO analysis_artifacts(
                    artifact_digest, lifecycle_state, source_content_digest,
                    producer_manifest_digest, configuration_digest, analysis_schema_digest,
                    canonicalization_version, fact_count, visited_nodes, syntax_error_nodes,
                    known_parser_limitation_nodes
                 ) VALUES (
                    zeroblob(32), 'staging', zeroblob(32), zeroblob(32),
                    zeroblob(32), zeroblob(32), 1, 0, 0, 0, 0
                 )",
            [],
        )
        .expect("incomplete artifact fixture should insert");
    drop(connection);

    let (store, startup) = OwnedSqliteIndex::start(&directory.database(), 456, deadline())
        .expect("owned store should recover");
    assert_eq!(startup.recovered_generations(), 0);
    store.shutdown(deadline()).expect("worker should stop");
    let connection =
        Connection::open(directory.database()).expect("database should reopen for inspection");
    let staging: i64 = connection
        .query_row(
            "SELECT
                    (SELECT count(*) FROM source_snapshots WHERE lifecycle_state = 'staging') +
                    (SELECT count(*) FROM analysis_artifacts WHERE lifecycle_state = 'staging')",
            [],
            |row| row.get(0),
        )
        .expect("staging counts should be readable");
    assert_eq!(staging, 0);
}
