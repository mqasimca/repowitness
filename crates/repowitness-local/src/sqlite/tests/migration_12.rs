#[test]
fn version_eleven_database_upgrades_with_linear_scip_completion_validation() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let mut connection = Connection::open(&database).expect("version-eleven fixture database should open");
    for (version, name, sql) in [
        (1, MIGRATION_1_NAME, MIGRATION_1),
        (2, MIGRATION_2_NAME, MIGRATION_2),
        (3, MIGRATION_3_NAME, MIGRATION_3),
        (4, MIGRATION_4_NAME, MIGRATION_4),
        (5, MIGRATION_5_NAME, MIGRATION_5),
        (6, MIGRATION_6_NAME, MIGRATION_6),
        (7, MIGRATION_7_NAME, MIGRATION_7),
        (9, MIGRATION_9_NAME, MIGRATION_9),
        (10, MIGRATION_10_NAME, MIGRATION_10),
        (11, MIGRATION_11_NAME, MIGRATION_11),
    ] {
        apply_migration(&mut connection, version, name, sql, 111)
            .expect("accepted version-eleven migration should apply");
    }
    drop(connection);

    let connection = open_index_writer(&database, 222)
        .expect("version-eleven database should upgrade through linear SCIP validation");
    let migration: (String, Vec<u8>, i64) = connection
        .query_row(
            "SELECT name, checksum, applied_at_unix_ms FROM schema_migrations WHERE version = 12",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("migration-twelve ledger row should be readable");
    assert_eq!(
        migration,
        (
            MIGRATION_12_NAME.to_owned(),
            migration_checksum(MIGRATION_12).to_vec(),
            222,
        ),
    );
    let trigger: Option<String> = connection
        .query_row(
            "SELECT sql FROM sqlite_schema
             WHERE type = 'trigger' AND name = 'scip_overlay_receipt_completion_requires_exact_rows'",
            [],
            |row| row.get(0),
        )
        .optional()
        .expect("trigger lookup should execute");
    let trigger = trigger.expect("completion trigger should exist");
    assert!(trigger.contains("GROUP BY document_ordinal"));
    assert!(!trigger.contains("FROM scip_overlay_occurrences AS prior"));
}
