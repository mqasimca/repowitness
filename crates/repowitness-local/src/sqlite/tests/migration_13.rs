#[test]
fn version_twelve_database_upgrades_with_isolated_memory_profile_v2_tables() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let mut connection =
        Connection::open(&database).expect("version-twelve fixture database should open");
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
        (12, MIGRATION_12_NAME, MIGRATION_12),
    ] {
        apply_migration(&mut connection, version, name, sql, 111)
            .expect("accepted pre-v2 migration should apply");
    }
    drop(connection);

    let connection = open_index_writer(&database, 222)
        .expect("version-twelve database should upgrade to memory profile v2");
    let migration: (String, Vec<u8>, i64) = connection
        .query_row(
            "SELECT name, checksum, applied_at_unix_ms
             FROM schema_migrations WHERE version = 13",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("migration-thirteen ledger row should be readable");
    assert_eq!(
        migration,
        (
            MIGRATION_13_NAME.to_owned(),
            migration_checksum(MIGRATION_13).to_vec(),
            222,
        ),
    );
    for table in [
        "memory_profile_v2_versions",
        "memory_profile_v2_audit",
        "memory_profile_v2_parents",
    ] {
        let present: Option<String> = connection
            .query_row(
                "SELECT name FROM sqlite_schema WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .optional()
            .expect("profile v2 table lookup should execute");
        assert_eq!(present.as_deref(), Some(table));
    }
    let old_tables: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema
             WHERE type = 'table' AND name IN (
                 'memory_versions', 'memory_version_parents',
                 'memory_evidence', 'memory_relationships', 'memory_audit'
             )",
            [],
            |row| row.get(0),
        )
        .expect("v1 memory tables should remain readable");
    assert_eq!(old_tables, 5);
}
