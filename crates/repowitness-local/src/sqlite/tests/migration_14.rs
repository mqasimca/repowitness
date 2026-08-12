#[test]
fn version_thirteen_database_upgrades_with_one_logical_memory_read_boundary() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let mut connection = Connection::open(&database).expect("fixture database should open");
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
        (13, MIGRATION_13_NAME, MIGRATION_13),
    ] {
        apply_migration(&mut connection, version, name, sql, 111)
            .expect("accepted migration should apply");
    }
    drop(connection);

    let connection = open_index_writer(&database, 222).expect("database should upgrade");
    for view in [
        "memory_versions_all",
        "memory_audit_all",
        "memory_current_trust",
    ] {
        let present: Option<String> = connection
            .query_row(
                "SELECT name FROM sqlite_schema WHERE type = 'view' AND name = ?1",
                [view],
                |row| row.get(0),
            )
            .optional()
            .expect("compatibility view lookup should execute");
        assert_eq!(present.as_deref(), Some(view));
    }
    let migration: (String, Vec<u8>, i64) = connection
        .query_row(
            "SELECT name, checksum, applied_at_unix_ms
             FROM schema_migrations WHERE version = 14",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("migration-fourteen ledger row should be readable");
    assert_eq!(
        migration,
        (
            MIGRATION_14_NAME.to_owned(),
            migration_checksum(MIGRATION_14).to_vec(),
            222,
        ),
    );
}
