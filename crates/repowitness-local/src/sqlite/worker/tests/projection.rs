#[test]
fn corrupted_complete_artifact_is_never_reused_or_activated() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = snapshot_identity().repository();
    let (store, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("owned store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let active = store
        .stage(
            0,
            snapshot_identity(),
            prepared("v1"),
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("baseline generation should stage");
    store
        .activate(active, 0, deadline())
        .expect("baseline generation should activate");
    store.shutdown(deadline()).expect("writer should stop");

    let raw = Connection::open(&database).expect("fixture database should open");
    raw.execute("DROP TRIGGER artifact_facts_no_update", [])
        .expect("fixture should remove the immutable-row guard");
    assert_eq!(
        raw.execute(
            "UPDATE artifact_facts SET name = 'tampered'
                 WHERE artifact_digest = (
                    SELECT artifact_digest FROM artifact_facts
                    ORDER BY artifact_digest LIMIT 1
                 ) AND ordinal = 0",
            [],
        )
        .expect("fixture should corrupt one complete artifact"),
        1
    );
    drop(raw);

    let (store, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("store should reopen");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should remain registered");
    assert_eq!(
        store
            .stage(
                0,
                snapshot_identity(),
                prepared("v1"),
                GenerationCoverage::new(2, 0, 0, 0),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect_err("tampered immutable facts must fail reuse"),
        SqliteStoreError::IntegrityCheckFailed
    );
    assert_eq!(
        store
            .active_generation(repository, deadline())
            .expect("active generation should remain readable"),
        Some(active)
    );
    store.shutdown(deadline()).expect("writer should stop");
}

#[test]
#[allow(
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    reason = "one end-to-end test keeps projection damage, failed rebuilds, pinned reads, and both slot switches in their required order"
)]
fn projection_rebuild_is_bounded_atomic_repeatable_and_recovers_a_missing_slot() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (store, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("owned store should start");
    let repository = snapshot_identity().repository();
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let prepared = prepared_many(300);
    let expected_rows = prepared.total_facts();
    assert!(expected_rows > 256);
    let generation = store
        .stage(
            0,
            snapshot_identity(),
            prepared,
            GenerationCoverage::new(1, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("generation should stage");
    store
        .activate(generation, 0, deadline())
        .expect("generation should activate");
    let reader = OwnedSqliteReader::start(&database, deadline()).expect("reader should start");
    let search = || {
        reader
            .search(
                repository,
                "symbol_0299",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("active projection should remain searchable")
    };
    let baseline = search();
    assert_eq!(baseline.hits().len(), 1);

    let raw = Connection::open(&database).expect("fixture connection should open");
    raw.execute(
        "DELETE FROM generation_search
             WHERE generation_id = ?1 AND name = 'symbol_0299'",
        [generation.get()],
    )
    .expect("fixture should remove one projected fact");
    assert!(
        reader
            .search(
                repository,
                "symbol_0299",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("damaged projection should still be queryable")
            .hits()
            .is_empty()
    );

    let too_small = ProjectionRebuildLimits::try_new(expected_rows - 1)
        .expect("fixture row limit should be valid");
    assert_eq!(
        store
            .rebuild_search_projection(too_small, Arc::new(AtomicBool::new(false)), deadline(),)
            .expect_err("row-limited rebuild should fail closed"),
        SqliteStoreError::ProjectionRebuildRowLimitExceeded
    );
    assert!(
        reader
            .search(
                repository,
                "symbol_0299",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("failed rebuild must not switch projections")
            .hits()
            .is_empty()
    );

    assert_eq!(
        store
            .rebuild_search_projection(
                ProjectionRebuildLimits::default(),
                Arc::new(AtomicBool::new(true)),
                deadline(),
            )
            .expect_err("pre-cancelled rebuild should fail"),
        SqliteStoreError::Cancelled
    );
    let mut pinned_connection =
        Connection::open(&database).expect("pinned fixture connection should open");
    let pinned = pinned_connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .expect("pinned read should begin");
    let pinned_before: i64 = pinned
        .query_row(
            "SELECT active_slot FROM search_projection_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .expect("pinned slot should be readable");
    assert_eq!(pinned_before, 0);

    let first = store
        .rebuild_search_projection(
            ProjectionRebuildLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("projection should rebuild");
    assert_eq!(first.previous_slot(), 0);
    assert_eq!(first.active_slot(), 1);
    assert_eq!(first.rebuilt_rows(), expected_rows);
    assert_eq!(first.write_batches(), expected_rows.div_ceil(256));
    let pinned_after: i64 = pinned
        .query_row(
            "SELECT active_slot FROM search_projection_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .expect("pinned read should keep its original slot");
    assert_eq!(pinned_after, 0);
    let published_slot: i64 = raw
        .query_row(
            "SELECT active_slot FROM search_projection_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .expect("new read should observe the published slot");
    assert_eq!(published_slot, 1);
    assert_eq!(search(), baseline);

    raw.execute_batch("DROP TABLE generation_search")
        .expect("fixture should remove the inactive projection table");
    let second = store
        .rebuild_search_projection(
            ProjectionRebuildLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("missing inactive table should be recreated");
    assert_eq!(second.previous_slot(), 1);
    assert_eq!(second.active_slot(), 0);
    assert_eq!(second.rebuilt_rows(), expected_rows);
    let pinned_rows: i64 = pinned
        .query_row(
            "SELECT count(*) FROM generation_search WHERE generation_id = ?1",
            [generation.get()],
            |row| row.get(0),
        )
        .expect("old read should retain the dropped slot snapshot");
    assert_eq!(
        pinned_rows,
        i64::try_from(expected_rows - 1).expect("fixture row count should fit")
    );
    pinned.commit().expect("pinned read should commit");
    let republished_slot: i64 = pinned_connection
        .query_row(
            "SELECT active_slot FROM search_projection_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .expect("new read should observe the second published slot");
    assert_eq!(republished_slot, 0);
    assert_eq!(search(), baseline);

    let damaged_blocks = raw
        .execute(
            "UPDATE generation_search_rebuild_data SET block = X'00'
                 WHERE id = (SELECT min(id) FROM generation_search_rebuild_data)",
            [],
        )
        .expect("fixture should damage the inactive FTS index");
    assert_eq!(damaged_blocks, 1);
    let third = store
        .rebuild_search_projection(
            ProjectionRebuildLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("damaged inactive FTS internals should be recreated");
    assert_eq!(third.previous_slot(), 0);
    assert_eq!(third.active_slot(), 1);
    assert_eq!(third.rebuilt_rows(), expected_rows);
    assert_eq!(search(), baseline);

    drop(raw);
    let backup_path = directory.0.join("projection-backup.sqlite3");
    create_online_backup(
        &database,
        &backup_path,
        BackupLimits::default(),
        Arc::new(AtomicBool::new(false)),
        deadline(),
    )
    .expect("rebuilt projection should back up");
    let backup_reader =
        OwnedSqliteReader::start(&backup_path, deadline()).expect("backup reader should start");
    let backup_results = backup_reader
        .search(
            repository,
            "symbol_0299",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("backed-up active slot should be searchable");
    assert_eq!(backup_results, baseline);
    backup_reader
        .shutdown(deadline())
        .expect("backup reader should stop");
    reader.shutdown(deadline()).expect("reader should stop");
    store.shutdown(deadline()).expect("writer should stop");

    let (restarted, startup) =
        OwnedSqliteIndex::start(&database, 456, deadline()).expect("rebuilt store should restart");
    assert_eq!(startup.recovered_generations(), 0);
    let restarted_reader =
        OwnedSqliteReader::start(&database, deadline()).expect("restarted reader should start");
    let restarted_results = restarted_reader
        .search(
            repository,
            "symbol_0299",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("restarted active slot should be searchable");
    assert_eq!(restarted_results, baseline);
    restarted_reader
        .shutdown(deadline())
        .expect("restarted reader should stop");
    restarted
        .shutdown(deadline())
        .expect("restarted writer should stop");
}

#[test]
fn projection_rebuild_limits_are_explicit_and_inclusive() {
    assert_eq!(
        ProjectionRebuildLimits::try_new(0),
        Err(SqliteStoreError::InvalidProjectionRebuildLimits)
    );
    assert_eq!(
        ProjectionRebuildLimits::try_new(100_000_001),
        Err(SqliteStoreError::InvalidProjectionRebuildLimits)
    );
    assert_eq!(
        ProjectionRebuildLimits::try_new(100_000_000)
            .expect("hard ceiling should be inclusive")
            .max_rows(),
        100_000_000
    );
    assert_eq!(ProjectionRebuildLimits::default().max_rows(), 5_000_000);
}
