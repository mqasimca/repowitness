use rusqlite::OptionalExtension;

#[test]
fn version_nine_database_upgrades_with_the_exact_raw_target_index() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let mut connection = Connection::open(&database).expect("version-nine fixture database should open");
    for (version, name, sql) in [
        (1, MIGRATION_1_NAME, MIGRATION_1),
        (2, MIGRATION_2_NAME, MIGRATION_2),
        (3, MIGRATION_3_NAME, MIGRATION_3),
        (4, MIGRATION_4_NAME, MIGRATION_4),
        (5, MIGRATION_5_NAME, MIGRATION_5),
        (6, MIGRATION_6_NAME, MIGRATION_6),
        (7, MIGRATION_7_NAME, MIGRATION_7),
        (9, MIGRATION_9_NAME, MIGRATION_9),
    ] {
        apply_migration(&mut connection, version, name, sql, 111)
            .expect("accepted version-nine migration should apply");
    }
    drop(connection);

    let connection = open_index_writer(&database, 222)
        .expect("version-nine database should upgrade through the raw-target index migration");
    let migration: (String, Vec<u8>, i64) = connection
        .query_row(
            "SELECT name, checksum, applied_at_unix_ms FROM schema_migrations WHERE version = 10",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("migration-ten ledger row should be readable");
    assert_eq!(
        migration,
        (
            MIGRATION_10_NAME.to_owned(),
            migration_checksum(MIGRATION_10).to_vec(),
            222,
        ),
    );
    let index: Option<String> = connection
        .query_row(
            "SELECT name FROM sqlite_schema WHERE type = 'index' AND name = 'syntax_sites_by_raw_target'",
            [],
            |row| row.get(0),
        )
        .optional()
        .expect("index lookup should execute");
    assert_eq!(index.as_deref(), Some("syntax_sites_by_raw_target"));
}
