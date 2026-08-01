#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the migration fixture keeps its complete version-one artifact and upgraded schema assertions together"
)]
fn version_one_database_upgrades_without_losing_immutable_artifacts() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let mut connection =
        Connection::open(&database).expect("version-one fixture database should open");
    apply_migration(&mut connection, 1, MIGRATION_1_NAME, MIGRATION_1, 111)
        .expect("accepted version-one baseline should apply");
    connection
        .execute(
            "INSERT INTO analysis_artifacts(
                artifact_digest, lifecycle_state, source_content_digest,
                producer_manifest_digest, configuration_digest,
                analysis_schema_digest, canonicalization_version,
                fact_count, visited_nodes, syntax_error_nodes,
                payload_digest, language
             ) VALUES (
                X'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA',
                'complete', zeroblob(32), zeroblob(32), zeroblob(32),
                zeroblob(32), 1, 0, 5, 2, zeroblob(32), 'typescript'
             )",
            [],
        )
        .expect("version-one artifact should persist");
    drop(connection);

    let connection =
        open_index_writer(&database, 222).expect("version-one database should upgrade");
    let user_version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema version should be readable");
    let ledger = {
        let mut statement = connection
            .prepare(
                "SELECT version, name, checksum, applied_at_unix_ms
                 FROM schema_migrations ORDER BY version",
            )
            .expect("migration ledger should prepare");
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .expect("migration ledger should be readable")
            .collect::<Result<Vec<_>, _>>()
            .expect("migration ledger should decode")
    };
    let artifact: (i64, i64, String, Vec<u8>) = connection
        .query_row(
            "SELECT syntax_error_nodes, known_parser_limitation_nodes,
                    language, payload_digest
             FROM analysis_artifacts
             WHERE artifact_digest =
                X'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("upgraded artifact should remain readable");

    assert_eq!(user_version, 5);
    assert_eq!(
        ledger,
        vec![
            (
                1,
                MIGRATION_1_NAME.to_owned(),
                migration_checksum(MIGRATION_1).to_vec(),
                111,
            ),
            (
                2,
                MIGRATION_2_NAME.to_owned(),
                migration_checksum(MIGRATION_2).to_vec(),
                222,
            ),
            (
                3,
                MIGRATION_3_NAME.to_owned(),
                migration_checksum(MIGRATION_3).to_vec(),
                222,
            ),
            (
                4,
                MIGRATION_4_NAME.to_owned(),
                migration_checksum(MIGRATION_4).to_vec(),
                222,
            ),
            (
                5,
                MIGRATION_5_NAME.to_owned(),
                migration_checksum(MIGRATION_5).to_vec(),
                222,
            ),
        ]
    );
    assert_eq!(artifact, (2, 0, "typescript".to_owned(), vec![0; 32]));
    assert!(
        connection
            .execute(
                "UPDATE analysis_artifacts
                 SET known_parser_limitation_nodes = 1
                 WHERE artifact_digest =
                    X'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA'",
                [],
            )
            .is_err(),
        "migration 2 must preserve immutable artifact semantics"
    );
}

#[test]
fn cancellation_after_committed_migrations_reports_unknown_and_preserves_the_upgrade() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let mut connection =
        Connection::open(&database).expect("version-one fixture database should open");
    apply_migration(&mut connection, 1, MIGRATION_1_NAME, MIGRATION_1, 111)
        .expect("accepted version-one baseline should apply");
    drop(connection);

    let expected_identity =
        database_file_identity(&database).expect("database identity should be captured");
    let cancelled = Arc::new(AtomicBool::new(false));
    let hook_cancelled = Arc::clone(&cancelled);
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(5))
        .expect("test deadline should be representable");

    let error = open_index_writer_with_identity_and_migration_hook(
        &database,
        expected_identity,
        222,
        Some(cancelled),
        Some(deadline),
        move |migrated| {
            assert!(migrated, "the version-one fixture must be upgraded");
            hook_cancelled.store(true, Ordering::Release);
        },
    )
    .expect_err("post-commit cancellation must report an unknown mutation outcome");
    assert_eq!(error, SqliteStoreError::MutationOutcomeUnknown);

    let connection = raw_connection(&database);
    let user_version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("upgraded schema version should remain readable");
    let ledger_rows: i64 = connection
        .query_row("SELECT count(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("upgraded migration ledger should remain readable");
    assert_eq!(user_version, SCHEMA_VERSION);
    assert_eq!(ledger_rows, SCHEMA_VERSION);
}

#[test]
fn reopening_current_version_is_idempotent() {
    let directory = TempDirectory::new();
    let database = directory.database();
    drop(open_index_writer(&database, 111).expect("fresh database should migrate"));
    drop(open_index_writer(&database, 222).expect("version five should reopen"));

    let connection = raw_connection(&database);
    let ledger_rows: i64 = connection
        .query_row("SELECT count(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("migration ledger count should be readable");
    let latest_timestamp: i64 = connection
        .query_row(
            "SELECT applied_at_unix_ms FROM schema_migrations WHERE version = 5",
            [],
            |row| row.get(0),
        )
        .expect("migration-four timestamp should be readable");

    assert_eq!(ledger_rows, 5);
    assert_eq!(latest_timestamp, 111);
}
