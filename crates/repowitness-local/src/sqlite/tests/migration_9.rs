#[test]
fn version_seven_database_upgrades_with_the_exact_repository_topology_migration() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let mut connection = Connection::open(&database).expect("version-seven fixture database should open");
    for (version, name, sql) in [
        (1, MIGRATION_1_NAME, MIGRATION_1),
        (2, MIGRATION_2_NAME, MIGRATION_2),
        (3, MIGRATION_3_NAME, MIGRATION_3),
        (4, MIGRATION_4_NAME, MIGRATION_4),
        (5, MIGRATION_5_NAME, MIGRATION_5),
        (6, MIGRATION_6_NAME, MIGRATION_6),
        (7, MIGRATION_7_NAME, MIGRATION_7),
    ] {
        apply_migration(&mut connection, version, name, sql, 111)
            .expect("accepted version-seven migration should apply");
    }
    drop(connection);

    let connection = open_index_writer(&database, 222)
        .expect("version-seven database should upgrade through topology migration");
    let migration: (String, Vec<u8>, i64) = connection
        .query_row(
            "SELECT name, checksum, applied_at_unix_ms FROM schema_migrations WHERE version = 9",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("migration-nine ledger row should be readable");
    assert_eq!(
        migration,
        (MIGRATION_9_NAME.to_owned(), migration_checksum(MIGRATION_9).to_vec(), 222),
    );
}
