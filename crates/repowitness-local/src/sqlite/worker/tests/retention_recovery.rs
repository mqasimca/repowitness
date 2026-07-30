#[test]
fn apply_rejects_preexisting_garbage_marks_without_deleting_anything() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0xD6; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("retention store should start");
    let generations = retention_history(&store, repository, 5, 0x81);
    let policy = retention_policy(2, RetentionLimits::default(), RetentionPins::default());
    let plan = plan_retention(&store, policy.clone());
    assert_eq!(
        plan.candidate_generations(),
        &generations[..2],
        "fixture should leave one retained non-candidate available for fault injection"
    );

    let connection =
        Connection::open(directory.database()).expect("fault fixture database should open");
    connection
        .execute_batch("PRAGMA foreign_keys = ON; BEGIN IMMEDIATE;")
        .expect("fault transaction should begin");
    connection
        .execute(
            "INSERT INTO retention_generation_garbage(
                generation_id, plan_digest, lifecycle_state
             ) VALUES (?1, ?2, 'garbage')",
            params![generations[2].get(), [0xAA_u8; 32].as_slice()],
        )
        .expect("stale retained-generation mark should be injectable");
    connection
        .execute_batch("COMMIT")
        .expect("fault mark should persist");
    drop(connection);

    assert_eq!(
        store.apply_generation_retention(RetentionApplyRequest::new(
            policy,
            plan.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )),
        Err(SqliteStoreError::IntegrityCheckFailed),
        "a stale mark must block the entire sweep"
    );

    let connection =
        Connection::open(directory.database()).expect("failed-closed database should open");
    let state: (i64, i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT count(*) FROM index_generations),
                (SELECT count(*) FROM retention_generation_garbage),
                (SELECT count(*) FROM retention_collection_audit)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("failed-closed retention state should be readable");
    assert_eq!(state, (5, 1, 0));
    drop(connection);
    store.shutdown(deadline()).expect("store should stop");
}

#[test]
fn startup_revokes_stale_garbage_marks_and_preserves_roots() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0xD7; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("retention store should start");
    let generations = retention_history(&store, repository, 4, 0x80);
    store
        .shutdown(deadline())
        .expect("store should stop before fault injection");

    let connection =
        Connection::open(directory.database()).expect("fault fixture database should open");
    connection
        .execute_batch("PRAGMA foreign_keys = ON; BEGIN IMMEDIATE;")
        .expect("fault transaction should begin");
    connection
        .execute(
            "INSERT INTO retention_generation_garbage(
                generation_id, plan_digest, lifecycle_state
             ) VALUES (?1, ?2, 'garbage')",
            params![generations[0].get(), [0xAB_u8; 32].as_slice()],
        )
        .expect("stale retained-generation mark should be injectable");
    connection
        .execute_batch("COMMIT")
        .expect("fault mark should persist like an interrupted implementation");
    assert!(
        connection
            .execute(
                "INSERT INTO retention_generation_garbage(
                    generation_id, plan_digest, lifecycle_state
                 ) VALUES (?1, ?2, 'garbage')",
                params![
                    generations.last().expect("active generation").get(),
                    [0xCD_u8; 32].as_slice()
                ],
            )
            .is_err(),
        "schema backstop must reject an active-generation mark"
    );
    drop(connection);

    let (reopened, startup) = OwnedSqliteIndex::start(&directory.database(), 456, deadline())
        .expect("startup should safely revoke stale marks");
    assert_eq!(startup.recovered_generations(), 0);
    let connection =
        Connection::open(directory.database()).expect("recovered database should open");
    let state: (i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT count(*) FROM retention_generation_garbage),
                (SELECT count(*) FROM index_generations
                 WHERE generation_id = ?1)",
            [generations[0].get()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("recovered mark state should be readable");
    assert_eq!(state, (0, 1));
    drop(connection);
    reopened
        .shutdown(deadline())
        .expect("reopened store should stop");
}

#[test]
fn dependent_delete_failure_rolls_back_rows_marks_audit_and_plan_state() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0xD9; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("retention store should start");
    let generations = retention_history(&store, repository, 4, 0xD0);
    let policy = retention_policy(1, RetentionLimits::default(), RetentionPins::default());
    let plan = plan_retention(&store, policy.clone());
    assert_eq!(plan.candidate_generations(), &generations[..2]);

    let connection =
        Connection::open(directory.database()).expect("fault fixture database should open");
    connection
        .execute_batch(
            "CREATE TRIGGER fail_retention_dependent_delete
             BEFORE DELETE ON generation_files BEGIN
                 SELECT RAISE(ABORT, 'injected dependent-delete failure');
             END;",
        )
        .expect("dependent-delete fault should install");
    let before = rollback_sensitive_retention_counts(&connection);
    drop(connection);

    assert_eq!(
        store.apply_generation_retention(RetentionApplyRequest::new(
            policy.clone(),
            plan.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )),
        Err(SqliteStoreError::IntegrityCheckFailed)
    );

    let connection =
        Connection::open(directory.database()).expect("rolled-back database should open");
    assert_eq!(rollback_sensitive_retention_counts(&connection), before);
    assert_eq!(
        connection
            .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
            .expect("quick check should complete"),
        "ok"
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("foreign-key check should complete"),
        0
    );
    connection
        .execute_batch("DROP TRIGGER fail_retention_dependent_delete")
        .expect("dependent-delete fault should be removable");
    drop(connection);

    assert_eq!(
        plan_retention(&store, policy.clone()),
        plan,
        "rollback must preserve the exact deterministic plan"
    );
    let outcome = store
        .apply_generation_retention(RetentionApplyRequest::new(
            policy,
            plan.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("the same plan should commit after removing the fault");
    assert_eq!(outcome.generation_count(), 2);
    store.shutdown(deadline()).expect("store should stop");
}

fn rollback_sensitive_retention_counts(connection: &Connection) -> Vec<i64> {
    [
        "index_generations",
        "workspace_views",
        "workspace_view_members",
        "source_slot_generation_receipts",
        "generation_search",
        "generation_search_rebuild",
        "generation_facts",
        "generation_files",
        "source_snapshots",
        "source_manifest_entries",
        "analysis_artifacts",
        "artifact_facts",
        "retention_collection_audit",
        "retention_generation_garbage",
        "retention_snapshot_garbage",
        "retention_artifact_garbage",
        "retention_workspace_view_garbage",
        "retention_source_slot_receipt_garbage",
    ]
    .into_iter()
    .map(|relation| {
        connection
            .query_row(&format!("SELECT count(*) FROM {relation}"), [], |row| {
                row.get(0)
            })
            .unwrap_or_else(|_| panic!("{relation} count should be readable"))
    })
    .collect()
}
