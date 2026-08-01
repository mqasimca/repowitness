#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one adversarial schema test keeps audit uniqueness and append-only mutations on the same seeded version"
)]
fn baseline_memory_checks_idempotency_keys_and_immutability_fail_closed() {
    let directory = TempDirectory::new();
    let connection =
        open_index_writer(&directory.database(), 123).expect("migration should succeed");
    insert_workspace(&connection);
    insert_minimal_worktree_memory(&connection);

    let invalid_source = connection.execute(
        "INSERT INTO memory_audit(
                workspace_id, record_id, revision_digest, operation,
                trusted_actor_kind, trusted_actor_id, recorded_at_unix_ms,
                source_kind, source_format, source_revision,
                display_revision, presentation_digest
             ) VALUES (
                1, X'11111111111111111111111111111111',
                X'2222222222222222222222222222222222222222222222222222222222222222',
                'observed', 'local_asserted', 'trusted', 1,
                'git', 'source_snapshot', zeroblob(32), 1, zeroblob(32)
             )",
        [],
    );
    assert!(invalid_source.is_err());
    let invalid_actor = connection.execute(
        "INSERT INTO memory_audit(
                workspace_id, record_id, revision_digest, operation,
                trusted_actor_kind, trusted_actor_id, recorded_at_unix_ms,
                source_kind, source_format, source_revision,
                display_revision, presentation_digest
             ) VALUES (
                1, X'11111111111111111111111111111111',
                X'2222222222222222222222222222222222222222222222222222222222222222',
                'observed', 'local_asserted', ?1, 1,
                'worktree', 'source_snapshot', zeroblob(32), 1, zeroblob(32)
             )",
        params!["private\nactor"],
    );
    assert!(invalid_actor.is_err());

    let insert_audit = |operation: &str| {
        connection.execute(
            "INSERT INTO memory_audit(
                    workspace_id, record_id, revision_digest, operation,
                    trusted_actor_kind, trusted_actor_id, recorded_at_unix_ms,
                    source_kind, source_format, source_revision,
                    display_revision, presentation_digest
                 ) VALUES (
                    1, X'11111111111111111111111111111111',
                    X'2222222222222222222222222222222222222222222222222222222222222222',
                    ?1, 'local_asserted', 'trusted', 1,
                    'worktree', 'source_snapshot',
                    X'3333333333333333333333333333333333333333333333333333333333333333',
                    1,
                    X'7777777777777777777777777777777777777777777777777777777777777777'
                 )",
            params![operation],
        )
    };
    assert_eq!(
        insert_audit("observed").expect("observation should insert"),
        1
    );
    assert!(insert_audit("observed").is_err());
    assert_eq!(
        insert_audit("locally_approved").expect("approval should insert"),
        1
    );
    assert!(insert_audit("locally_approved").is_err());

    assert!(
        connection
            .execute("UPDATE memory_versions SET title = 'changed'", [])
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM memory_versions", [])
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO memory_relationships(
                        workspace_id, record_id, revision_digest, ordinal,
                        relationship_kind, target_record_id, target_revision_digest
                     ) VALUES (
                        1, X'11111111111111111111111111111111',
                        X'2222222222222222222222222222222222222222222222222222222222222222',
                        0, 'contradicts', zeroblob(16), zeroblob(32)
                     )",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute("UPDATE memory_audit SET recorded_at_unix_ms = 2", [])
            .is_err()
    );
    assert!(connection.execute("DELETE FROM memory_audit", []).is_err());

    let counts: (i64, i64) = connection
        .query_row(
            "SELECT
                    (SELECT count(*) FROM memory_versions),
                    (SELECT count(*) FROM memory_audit)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("memory counts should remain readable");
    assert_eq!(counts, (1, 2));
}

#[test]
fn baseline_memory_rejects_missing_or_sparse_children_before_publication() {
    let directory = TempDirectory::new();
    let connection =
        open_index_writer(&directory.database(), 123).expect("migration should succeed");
    insert_workspace(&connection);

    let missing_evidence = connection.execute(
        "INSERT INTO memory_versions(
                workspace_id, record_id, revision_digest, schema_version,
                canonical_json, kind, title, body, subject_evidence,
                provenance_origin, authored_actor_kind, authored_actor_id,
                authored_assurance, authored_lifecycle, validity_kind,
                validity_source_snapshot, tombstone
             ) VALUES (
                1, zeroblob(16), zeroblob(32), 1, X'7B7D', 'decision',
                'title', 'body', 0, 'human', 'local_asserted', 'actor',
                'locally_approved', 'active', 'worktree', zeroblob(32), 0
             )",
        [],
    );
    assert!(missing_evidence.is_err());

    connection
        .execute_batch(
            "BEGIN IMMEDIATE;
                 INSERT INTO memory_evidence(
                    workspace_id, record_id, revision_digest, ordinal,
                    evidence_kind, source_snapshot_digest, repository_path,
                    content_digest, artifact_digest, fact_ordinal, symbol_kind,
                    name, qualified_name, name_start, name_length,
                    declaration_start, declaration_length, declaration_digest,
                    producer_id, producer_version
                 ) VALUES (
                    1, X'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA',
                    X'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB',
                    1, 'rust_symbol', zeroblob(32), X'61', zeroblob(32),
                    zeroblob(32), 0, 'function', 'n', 'n', 0, 1, 0, 1,
                    zeroblob(32), 'producer', 'version'
                 );",
        )
        .expect("sparse child should remain deferred until publication");
    let sparse = connection.execute(
        "INSERT INTO memory_versions(
                workspace_id, record_id, revision_digest, schema_version,
                canonical_json, kind, title, body, subject_evidence,
                provenance_origin, authored_actor_kind, authored_actor_id,
                authored_assurance, authored_lifecycle, validity_kind,
                validity_source_snapshot, tombstone
             ) VALUES (
                1, X'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA',
                X'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB',
                1, X'7B7D', 'decision', 'title', 'body', 1, 'human',
                'local_asserted', 'actor', 'locally_approved', 'active',
                'worktree', zeroblob(32), 0
             )",
        [],
    );
    assert!(sparse.is_err());
    connection
        .execute_batch("ROLLBACK;")
        .expect("sparse fixture transaction should roll back");
}

#[cfg(unix)]
#[test]
fn writer_revalidates_the_guarded_file_after_sqlite_opens() {
    let directory = TempDirectory::new();
    let database = directory.database();
    drop(open_index_writer(&database, 123).expect("seed migration should succeed"));
    let replacement = directory.0.join("replacement.sqlite3");
    fs::copy(&database, &replacement).expect("replacement should be copied");
    let expected_identity =
        database_file_identity(&database).expect("database identity should be captured");
    let displaced = directory.0.join("displaced.sqlite3");
    let original_bytes = fs::read(&database).expect("seed database should be readable");
    let replacement_bytes = fs::read(&replacement).expect("replacement should be readable");

    let error = open_index_writer_with_identity_and_hook(
        &database,
        expected_identity,
        456,
        None,
        None,
        || {
            fs::rename(&database, &displaced).expect("opened database should be displaced");
            fs::rename(&replacement, &database)
                .expect("replacement should occupy the database path");
        },
    )
    .expect_err("a post-open path replacement must fail before configuration");

    assert_eq!(error, SqliteStoreError::DatabaseIdentityChanged);
    assert_eq!(
        fs::read(&displaced).expect("displaced database should be readable"),
        original_bytes
    );
    assert_eq!(
        fs::read(&database).expect("replacement database should be readable"),
        replacement_bytes
    );
}

#[cfg(any(unix, windows))]
#[test]
fn writer_guard_rejects_a_hard_link_before_sqlite_can_write() {
    let directory = TempDirectory::new();
    let database = directory.database();
    drop(open_index_writer(&database, 123).expect("seed migration should succeed"));
    let original_bytes = fs::read(&database).expect("seed database should be readable");
    fs::hard_link(&database, directory.0.join("database-alias"))
        .expect("database hard link should be created");
    let expected_identity =
        database_file_identity(&database).expect("database identity should be captured");

    let error = open_index_writer_with_identity_and_hook(
        &database,
        expected_identity,
        456,
        None,
        None,
        || {},
    )
    .expect_err("a multiply linked database must fail before SQLite opens");

    assert_eq!(error, SqliteStoreError::DatabaseIdentityChanged);
    assert_eq!(
        fs::read(&database).expect("database should remain readable"),
        original_bytes
    );
}

#[test]
fn writer_startup_cancellation_after_open_prevents_configuration_writes() {
    let directory = TempDirectory::new();
    let database = directory.database();
    drop(open_index_writer(&database, 123).expect("seed migration should succeed"));
    let expected_identity =
        database_file_identity(&database).expect("database identity should be captured");
    let original_bytes = fs::read(&database).expect("seed database should be readable");
    let cancelled = Arc::new(AtomicBool::new(false));
    let hook_cancelled = Arc::clone(&cancelled);
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(5))
        .expect("test deadline should be representable");

    let error = open_index_writer_with_identity_and_hook(
        &database,
        expected_identity,
        456,
        Some(cancelled),
        Some(deadline),
        move || hook_cancelled.store(true, Ordering::Release),
    )
    .expect_err("post-open cancellation must fail before connection configuration");

    assert_eq!(error, SqliteStoreError::Cancelled);
    assert_eq!(
        fs::read(&database).expect("database should remain readable"),
        original_bytes
    );
}

#[test]
fn cancelled_new_database_startup_removes_only_its_reserved_file() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let expected_identity =
        database_file_identity(&database).expect("missing identity should be captured");
    assert!(expected_identity.is_none());
    let cancelled = Arc::new(AtomicBool::new(false));
    let hook_cancelled = Arc::clone(&cancelled);
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(5))
        .expect("test deadline should be representable");

    let error = open_index_writer_with_identity_and_hook(
        &database,
        expected_identity,
        123,
        Some(cancelled),
        Some(deadline),
        move || hook_cancelled.store(true, Ordering::Release),
    )
    .expect_err("cancelled new startup should fail");

    assert_eq!(error, SqliteStoreError::Cancelled);
    assert!(!database.exists());
    assert!(!directory.0.join("index.sqlite3-wal").exists());
    assert!(!directory.0.join("index.sqlite3-shm").exists());
}

#[test]
fn unrelated_sqlite_files_are_rejected_without_mutation() {
    let directory = TempDirectory::new();

    for (name, application_id) in [("unmarked", 0_i64), ("foreign", 0x1234_i64)] {
        let database = directory.0.join(format!("{name}.sqlite3"));
        {
            let connection =
                Connection::open(&database).expect("unrelated fixture should be created");
            connection
                .pragma_update(None, "journal_mode", "DELETE")
                .expect("fixture should use rollback journaling");
            connection
                .execute(
                    "CREATE TABLE unrelated_data(value TEXT NOT NULL)",
                    [],
                )
                .expect("unrelated table should be created");
            connection
                .execute(
                    "INSERT INTO unrelated_data(value) VALUES ('sentinel')",
                    [],
                )
                .expect("unrelated row should be inserted");
            connection
                .pragma_update(None, "application_id", application_id)
                .expect("fixture identity should be set");
        }

        let original_bytes =
            fs::read(&database).expect("unrelated database should be readable");
        let wal_path = directory.0.join(format!("{name}.sqlite3-wal"));
        let shm_path = directory.0.join(format!("{name}.sqlite3-shm"));
        assert!(!wal_path.exists());
        assert!(!shm_path.exists());

        let error = open_index_writer(&database, 456)
            .expect_err("an existing unrelated database must not be adopted");

        assert_eq!(error, SqliteStoreError::ApplicationIdMismatch);
        assert_eq!(
            fs::read(&database).expect("rejected database should remain readable"),
            original_bytes
        );
        assert!(!wal_path.exists());
        assert!(!shm_path.exists());

        let connection = Connection::open_with_flags(
            &database,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .expect("rejected database should remain a valid SQLite file");
        let sentinel: String = connection
            .query_row("SELECT value FROM unrelated_data", [], |row| row.get(0))
            .expect("unrelated data should remain intact");
        assert_eq!(sentinel, "sentinel");
    }
}

#[test]
fn reopening_is_idempotent_and_preserves_the_original_ledger() {
    let directory = TempDirectory::new();
    drop(open_index_writer(&directory.database(), 123).expect("migration should succeed"));
    let connection = open_index_writer(&directory.database(), 456).expect("reopen should validate");
    let applied_at: (i64, i64, i64) = connection
        .query_row(
            "SELECT count(*), min(applied_at_unix_ms), max(applied_at_unix_ms)
                 FROM schema_migrations",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("ledger should remain readable");

    assert_eq!(applied_at, (6, 123, 123));
}
