#[test]
fn version_six_database_upgrades_with_the_exact_raw_syntax_site_migration() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let mut connection =
        Connection::open(&database).expect("version-six fixture database should open");
    for (version, name, sql) in [
        (1, MIGRATION_1_NAME, MIGRATION_1),
        (2, MIGRATION_2_NAME, MIGRATION_2),
        (3, MIGRATION_3_NAME, MIGRATION_3),
        (4, MIGRATION_4_NAME, MIGRATION_4),
        (5, MIGRATION_5_NAME, MIGRATION_5),
        (6, MIGRATION_6_NAME, MIGRATION_6),
    ] {
        apply_migration(&mut connection, version, name, sql, 111)
            .expect("accepted version-six migration should apply");
    }
    drop(connection);

    let connection =
        open_index_writer(&database, 222).expect("version-six database should upgrade");
    let user_version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema version should be readable");
    let migration: (String, Vec<u8>, i64) = connection
        .query_row(
            "SELECT name, checksum, applied_at_unix_ms
             FROM schema_migrations WHERE version = 7",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("migration-seven ledger row should be readable");

    assert_eq!(user_version, SCHEMA_VERSION);
    assert_eq!(
        migration,
        (
            MIGRATION_7_NAME.to_owned(),
            migration_checksum(MIGRATION_7).to_vec(),
            222,
        )
    );
}
