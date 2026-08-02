#[test]
fn version_ten_database_upgrades_with_directional_scip_trace_indexes() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let mut connection = Connection::open(&database).expect("version-ten fixture database should open");
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
    ] {
        apply_migration(&mut connection, version, name, sql, 111)
            .expect("accepted version-ten migration should apply");
    }
    drop(connection);

    let connection = open_index_writer(&database, 222)
        .expect("version-ten database should upgrade through SCIP trace indexes");
    let migration: (String, Vec<u8>, i64) = connection
        .query_row(
            "SELECT name, checksum, applied_at_unix_ms FROM schema_migrations WHERE version = 11",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("migration-eleven ledger row should be readable");
    assert_eq!(
        migration,
        (
            MIGRATION_11_NAME.to_owned(),
            migration_checksum(MIGRATION_11).to_vec(),
            222,
        ),
    );
    for index_name in [
        "scip_overlay_relationships_trace_outbound",
        "scip_overlay_relationships_trace_inbound",
    ] {
        let index: Option<String> = connection
            .query_row(
                "SELECT name FROM sqlite_schema WHERE type = 'index' AND name = ?1",
                [index_name],
                |row| row.get(0),
            )
            .optional()
            .expect("index lookup should execute");
        assert_eq!(index.as_deref(), Some(index_name));
    }
    let redundant_target_index: Option<String> = connection
        .query_row(
            "SELECT name FROM sqlite_schema
             WHERE type = 'index' AND name = 'scip_overlay_relationships_by_target'",
            [],
            |row| row.get(0),
        )
        .optional()
        .expect("superseded target index lookup should execute");
    assert!(redundant_target_index.is_none());
    for (predicate, index_name) in [
        ("source_symbol = X'01'", "scip_overlay_relationships_trace_outbound"),
        ("target_symbol = X'01'", "scip_overlay_relationships_trace_inbound"),
    ] {
        let mut statement = connection
            .prepare(&format!(
                "EXPLAIN QUERY PLAN
                 SELECT document_ordinal, relationship_ordinal
                 FROM scip_overlay_relationships
                 WHERE overlay_digest = X'00' AND {predicate}
                 ORDER BY document_ordinal, relationship_ordinal
                 LIMIT 1"
            ))
            .expect("directional trace query plan should prepare");
        let details = statement
            .query_map([], |row| row.get::<_, String>(3))
            .expect("directional trace query plan should execute")
            .collect::<Result<Vec<_>, _>>()
            .expect("directional trace query plan should decode");
        assert!(
            details.iter().any(|detail| detail.contains(index_name)),
            "query plan must select {index_name}: {details:?}"
        );
    }
}
