#[test]
fn version_fourteen_profile_v2_rows_are_backfilled_into_the_unified_memory_journal() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let mut connection = Connection::open(&database).expect("fixture database should open");
    apply_migrations_through_fourteen(&mut connection);
    insert_workspace(&connection);

    let parsed = parse_v2_fixture();
    let record = parsed.record();
    let record_id = record.header().record_id();
    let revision = parsed.digest();
    insert_legacy_v2_rows(&connection, &parsed);
    drop(connection);

    let connection = open_index_writer(&database, 222).expect("migration fifteen should succeed");
    assert_backfilled_v2_rows(&connection, record_id.as_bytes(), revision.as_bytes());
}

#[test]
fn version_fourteen_rejects_a_profile_v2_key_that_disagrees_with_canonical_identity() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let mut connection = Connection::open(&database).expect("fixture database should open");
    apply_migrations_through_fourteen(&mut connection);
    insert_workspace(&connection);
    insert_legacy_v2_rows_with_record_id(&connection, &parse_v2_fixture(), &[0x77; 16]);
    drop(connection);

    let error = open_index_writer(&database, 222).expect_err("corrupt key should reject migration");
    assert_eq!(error, SqliteStoreError::MigrationFailed);
    let connection = raw_connection(&database);
    let user_version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("rolled-back migration should preserve the prior schema version");
    assert_eq!(user_version, 14);
    let unified: i64 = connection
        .query_row("SELECT count(*) FROM memory_versions", [], |row| row.get(0))
        .expect("baseline memory table should remain readable");
    assert_eq!(unified, 0);
}

fn apply_migrations_through_fourteen(connection: &mut Connection) {
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
        (14, MIGRATION_14_NAME, MIGRATION_14),
    ] {
        apply_migration(connection, version, name, sql, 111)
            .expect("accepted migration should apply");
    }
}

fn parse_v2_fixture() -> crate::memory_format::ParsedMemoryRecord {
    let cancelled = AtomicBool::new(false);
    let yaml = String::from_utf8(
        include_bytes!("../../../tests/fixtures/memory-v1/commit.yaml").to_vec(),
    )
    .expect("fixture should be UTF-8")
    .replacen("schema_version: 1", "schema_version: 2", 1)
    .replacen("kind: decision", "kind: fact", 1);
    crate::memory_format::parse_memory_record(
        yaml.as_bytes(),
        crate::memory_format::MemoryFormatControl::new(
            &cancelled,
            Instant::now() + Duration::from_secs(5),
        ),
    )
    .expect("v2 fixture should parse")
}

fn insert_legacy_v2_rows(
    connection: &Connection,
    parsed: &crate::memory_format::ParsedMemoryRecord,
) {
    let record = parsed.record();
    let record_id = record.header().record_id();
    insert_legacy_v2_rows_with_record_id(connection, parsed, record_id.as_bytes());
}

fn insert_legacy_v2_rows_with_record_id(
    connection: &Connection,
    parsed: &crate::memory_format::ParsedMemoryRecord,
    persisted_record_id: &[u8],
) {
    let record = parsed.record();
    let revision = parsed.digest();
    connection
        .execute(
            "INSERT INTO memory_profile_v2_versions(
                workspace_id, record_id, revision_digest, schema_version,
                canonical_json, kind, title, body, subject_evidence,
                provenance_origin, authored_actor_kind, authored_actor_id,
                authored_assurance, authored_lifecycle, validity_kind,
                validity_source_snapshot, tombstone
             ) VALUES (
                1, ?1, ?2, 2, ?3, 'fact', ?4, ?5, 0, 'human',
                'local_asserted', 'maintainer', 'locally_approved', 'active',
                'commits', NULL, 0
             )",
            params![
                persisted_record_id,
                revision.as_bytes().as_slice(),
                parsed.canonical_json(),
                record.claim().title().as_str(),
                record.claim().body().as_str(),
            ],
        )
        .expect("legacy v2 version should be inserted");
    connection
        .execute(
            "INSERT INTO memory_profile_v2_audit(
                event_id, workspace_id, record_id, revision_digest, operation,
                trusted_actor_kind, trusted_actor_id, recorded_at_unix_ms,
                source_kind, source_format, source_revision,
                display_revision, presentation_digest
             ) VALUES
                (1, 1, ?1, ?2, 'observed', 'local_asserted', 'importer', 1,
                 'git', 'sha1', X'1111111111111111111111111111111111111111', 1, zeroblob(32)),
                (2, 1, ?1, ?2, 'locally_approved', 'local_asserted', 'trusted', 2,
                 'git', 'sha1', X'1111111111111111111111111111111111111111', 1, zeroblob(32))",
            params![persisted_record_id, revision.as_bytes().as_slice()],
        )
        .expect("legacy v2 audit should be inserted");
}

fn assert_backfilled_v2_rows(connection: &Connection, record_id: &[u8], revision: &[u8]) {
    let unified: (i64, i64, i64) = connection
        .query_row(
            "SELECT
                 (SELECT count(*) FROM memory_versions WHERE schema_version = 2),
                 (SELECT count(*) FROM memory_evidence),
                 (SELECT count(*) FROM memory_audit)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("unified v2 rows should be readable");
    assert_eq!(unified, (1, 1, 2));
    let trusted: i64 = connection
        .query_row(
            "SELECT count(*) FROM memory_current_trust
             WHERE record_id = ?1 AND revision_digest = ?2",
            params![record_id, revision],
            |row| row.get(0),
        )
        .expect("migrated v2 approval should be trusted");
    assert_eq!(trusted, 1);
    let foreign_keys: i64 = connection
        .query_row("PRAGMA foreign_key_check", [], |_row| Ok(1_i64))
        .optional()
        .expect("foreign-key check should execute")
        .unwrap_or(0);
    assert_eq!(foreign_keys, 0);
}
