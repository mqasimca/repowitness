#[test]
fn migration_checksums_are_stable_golden_vectors() {
    assert_eq!(
        migration_checksum(MIGRATION_1),
        [
            0x47, 0xca, 0xe5, 0x1f, 0x5f, 0x5f, 0xa8, 0x39, 0xd0, 0xcd, 0xe3, 0xdc, 0xb8, 0x53,
            0x48, 0x78, 0x7e, 0x0c, 0x9d, 0xe7, 0x6a, 0xb4, 0x08, 0xd8, 0xd3, 0x06, 0x48, 0x83,
            0x1d, 0xc2, 0x76, 0xd9,
        ]
    );
    assert_eq!(migrations(), [(1, MIGRATION_1_NAME, MIGRATION_1)]);
    for transitional_statement in ["CREATE TEMP", "ALTER TABLE", "DROP TABLE"] {
        assert!(!MIGRATION_1.contains(transitional_statement));
    }
}

#[test]
fn baseline_catalog_matches_the_retired_final_schema_golden() {
    let directory = TempDirectory::new();
    let connection =
        open_index_writer(&directory.database(), 123).expect("baseline should succeed");
    let mut statement = connection
        .prepare(
            "SELECT type, name, tbl_name, sql
                 FROM sqlite_schema
                 WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%'
                 ORDER BY type, name",
        )
        .expect("schema catalog should be readable");
    let rows = statement
        .query_map([], |row| {
            Ok([
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ])
        })
        .expect("schema catalog should be queryable");
    let mut canonical_catalog = String::new();
    for fields in rows {
        for field in fields.expect("schema catalog row should decode") {
            canonical_catalog.push_str(&field);
            canonical_catalog.push('\0');
        }
    }

    assert_eq!(
        migration_checksum(&canonical_catalog),
        [
            0xca, 0xa4, 0x39, 0x82, 0xc8, 0x86, 0x82, 0xf2, 0xf7, 0x8d, 0xb1, 0x4e, 0xdf, 0x3c,
            0x13, 0x15, 0x44, 0xd5, 0xa2, 0xff, 0xb8, 0x34, 0x1c, 0xd9, 0x89, 0xbe, 0x2d, 0x66,
            0x07, 0x96, 0xe4, 0x27,
        ]
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one schema-introspection test compares the complete migration ledger and required memory objects"
)]
fn fresh_database_has_exact_identity_ledger_and_required_schema() {
    let directory = TempDirectory::new();
    let connection =
        open_index_writer(&directory.database(), 123).expect("migration should succeed");

    let application_id: i64 = connection
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .expect("application ID should be readable");
    let user_version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema version should be readable");
    let ledger = {
        let mut statement = connection
            .prepare(
                "SELECT version, name, checksum, applied_at_unix_ms
                     FROM schema_migrations ORDER BY version",
            )
            .expect("migration ledger should be readable");
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .expect("migration ledger should be queryable")
            .collect::<Result<Vec<_>, _>>()
            .expect("migration ledger rows should decode")
    };
    let tables: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema
                 WHERE name IN (
                    'workspaces', 'source_snapshots', 'source_manifest_entries',
                    'analysis_artifacts', 'artifact_facts', 'index_generations',
                    'generation_files', 'generation_facts', 'generation_search',
                    'generation_search_rebuild', 'search_projection_state',
                    'memory_versions', 'memory_version_parents',
                    'memory_validity_commits', 'memory_evidence',
                    'memory_relationships', 'memory_audit',
                    'artifact_fact_correspondence',
                    'memory_correspondence_audit',
                    'memory_projection_generations',
                    'memory_projection_records',
                    'memory_projection_evidence',
                    'memory_projection_candidates',
                    'active_memory_projections'
                 )",
            [],
            |row| row.get(0),
        )
        .expect("schema should be introspectable");
    let memory_schema_objects: (i64, i64) = connection
        .query_row(
            "SELECT
                    (SELECT count(*) FROM sqlite_schema
                     WHERE type = 'index'
                       AND name IN (
                           'unique_memory_observation',
                           'unique_memory_local_approval',
                           'memory_evidence_occurrence_identity',
                           'unique_memory_correspondence_event'
                       )),
                    (SELECT count(*) FROM sqlite_schema
                     WHERE type = 'trigger' AND name GLOB 'memory_*')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("memory indexes and triggers should be introspectable");

    assert_eq!(application_id, APPLICATION_ID);
    assert_eq!(user_version, SCHEMA_VERSION);
    assert_eq!(
        ledger,
        vec![(
            1,
            MIGRATION_1_NAME.to_owned(),
            migration_checksum(MIGRATION_1).to_vec(),
            123
        )]
    );
    assert_eq!(tables, 24);
    assert_eq!(memory_schema_objects, (4, 30));
    let payload_column: (String, i64) = connection
        .query_row(
            "SELECT type, [notnull] FROM pragma_table_info('analysis_artifacts')
                 WHERE name = 'payload_digest'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("artifact payload column should be present");
    assert_eq!(payload_column, ("BLOB".to_owned(), 0));
    let language_column: (String, i64, String) = connection
        .query_row(
            "SELECT type, [notnull], dflt_value
                 FROM pragma_table_info('analysis_artifacts')
                 WHERE name = 'language'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("artifact language column should be present");
    assert_eq!(language_column, ("TEXT".to_owned(), 1, "'rust'".to_owned()));
}

#[test]
fn baseline_rust_correspondence_requires_complete_immutable_companions() {
    let directory = TempDirectory::new();
    let connection =
        open_index_writer(&directory.database(), 123).expect("migration should succeed");
    connection
        .execute(
            "INSERT INTO analysis_artifacts(
                    artifact_digest, lifecycle_state, source_content_digest,
                    producer_manifest_digest, configuration_digest,
                    analysis_schema_digest, canonicalization_version,
                    fact_count, visited_nodes, syntax_error_nodes,
                    payload_digest, language
                 ) VALUES (
                    zeroblob(32), 'staging', zeroblob(32), zeroblob(32),
                    zeroblob(32), zeroblob(32), 7, 1, 1, 0,
                    zeroblob(32), 'rust'
                 )",
            [],
        )
        .expect("staging Rust artifact should insert");
    connection
        .execute(
            "INSERT INTO artifact_facts(
                    artifact_digest, ordinal, kind, name, qualified_name,
                    name_start, name_end, declaration_start, declaration_end
                 ) VALUES (
                    zeroblob(32), 0, 'function', 'f', 'f', 3, 4, 0, 7
                 )",
            [],
        )
        .expect("staging Rust fact should insert");

    assert!(
        connection
            .execute(
                "UPDATE analysis_artifacts SET lifecycle_state = 'complete'
                     WHERE artifact_digest = zeroblob(32)",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO artifact_fact_correspondence(
                        artifact_digest, fact_ordinal, profile_id, profile_version,
                        declaration_digest, name_elided_digest
                     ) VALUES (
                        zeroblob(32), 0, 'unknown-profile', 1,
                        zeroblob(32), zeroblob(32)
                     )",
                [],
            )
            .is_err()
    );
    connection
        .execute(
            "INSERT INTO artifact_fact_correspondence(
                    artifact_digest, fact_ordinal, profile_id, profile_version,
                    declaration_digest, name_elided_digest
                 ) VALUES (
                    zeroblob(32), 0, 'rust-name-elided', 1,
                    zeroblob(32), zeroblob(32)
                 )",
            [],
        )
        .expect("exact correspondence profile should insert");
    assert_eq!(
        connection
            .execute(
                "UPDATE analysis_artifacts SET lifecycle_state = 'complete'
                     WHERE artifact_digest = zeroblob(32)",
                [],
            )
            .expect("complete Rust artifact should publish"),
        1
    );
    assert!(
        connection
            .execute(
                "UPDATE artifact_fact_correspondence
                     SET name_elided_digest = randomblob(32)",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM artifact_fact_correspondence", [])
            .is_err()
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one schema fixture verifies complete projection immutability and activation constraints together"
)]
fn baseline_memory_projection_is_complete_atomic_and_immutable() {
    let directory = TempDirectory::new();
    let connection =
        open_index_writer(&directory.database(), 123).expect("migration should succeed");
    insert_workspace(&connection);
    insert_active_generation_fixture(&connection);
    insert_minimal_worktree_memory(&connection);
    insert_minimal_local_approval(&connection);

    connection
        .execute(
            "INSERT INTO memory_projection_generations(
                    projection_id, workspace_id, index_generation_id, source_epoch,
                    snapshot_digest, target_kind, target_format, target_revision,
                    head_format, head_revision, correspondence_profile_id,
                    correspondence_profile_version, correspondence_profile_digest,
                    lifecycle_state, searched_count, skipped_count,
                    unresolved_count, truncated_count, total_count, current_count,
                    not_applicable_count, stale_count, needs_review_count,
                    indeterminate_count, conflicted_count, contradicted_count,
                    superseded_count, quarantined_count, tombstoned_count
                 ) VALUES (
                    1, 1, 1, 0,
                    X'3333333333333333333333333333333333333333333333333333333333333333',
                    'worktree', 'source_snapshot',
                    X'3333333333333333333333333333333333333333333333333333333333333333',
                    'sha1', zeroblob(20), 'rust-name-elided', 1, zeroblob(32),
                    'staging', 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0
                 )",
            [],
        )
        .expect("staging projection should insert");
    assert!(
        connection
            .execute(
                "INSERT INTO active_memory_projections(workspace_id, projection_id)
                     VALUES (1, 1)",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE memory_projection_generations
                     SET lifecycle_state = 'complete' WHERE projection_id = 1",
                [],
            )
            .is_err()
    );

    connection
        .execute(
            "INSERT INTO memory_projection_records(
                    projection_id, workspace_id, ordinal, record_id,
                    revision_digest, effective_state, validity_state,
                    evidence_state, reason, evidence_count, resolved_count,
                    review_count, indeterminate_count, head_count,
                    missing_parent_count, has_trusted_approval
                 ) VALUES (
                    1, 1, 0, X'11111111111111111111111111111111',
                    X'2222222222222222222222222222222222222222222222222222222222222222',
                    'not_applicable', 'invalid', 'not_evaluated',
                    'project_not_applicable', 1, 0, 0, 0, 1, 0, 1
                 )",
            [],
        )
        .expect("complete projected record should insert");
    assert_eq!(
        connection
            .execute(
                "UPDATE memory_projection_generations
                     SET lifecycle_state = 'complete' WHERE projection_id = 1",
                [],
            )
            .expect("complete projection should publish"),
        1
    );
    assert_eq!(
        connection
            .execute(
                "INSERT INTO active_memory_projections(workspace_id, projection_id)
                     VALUES (1, 1)",
                [],
            )
            .expect("complete projection should activate"),
        1
    );

    assert!(
        connection
            .execute(
                "UPDATE memory_projection_records SET reason = 'project_indeterminate'",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM memory_projection_records", [])
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM memory_projection_generations", [])
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM active_memory_projections", [])
            .is_err()
    );
    let active: (i64, i64, String) = connection
        .query_row(
            "SELECT active.workspace_id, active.projection_id,
                        projection.lifecycle_state
                 FROM active_memory_projections AS active
                 JOIN memory_projection_generations AS projection
                   USING (projection_id)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("active projection should remain readable");
    assert_eq!(active, (1, 1, "complete".to_owned()));
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one v6 adversarial fixture verifies correspondence audit and nested projection completeness together"
)]
fn baseline_memory_projection_validates_evidence_candidates_and_review_audit() {
    let directory = TempDirectory::new();
    let connection =
        open_index_writer(&directory.database(), 123).expect("migration should succeed");
    insert_workspace(&connection);
    insert_active_generation_fixture(&connection);
    insert_minimal_worktree_memory(&connection);
    insert_minimal_local_approval(&connection);

    assert!(
        connection
            .execute(
                "INSERT INTO memory_correspondence_audit(
                        workspace_id, record_id, revision_digest, evidence_ordinal,
                        operation, source_snapshot_digest, source_repository_path,
                        source_artifact_digest, source_fact_ordinal,
                        target_snapshot_digest, target_repository_path,
                        target_artifact_digest, target_fact_ordinal,
                        method_id, method_version, trusted_actor_kind,
                        trusted_actor_id, recorded_at_unix_ms
                     ) VALUES (
                        1, X'11111111111111111111111111111111',
                        X'2222222222222222222222222222222222222222222222222222222222222222',
                        0, 'approved',
                        X'3333333333333333333333333333333333333333333333333333333333333333',
                        X'7372632F6C69622E7273',
                        X'5555555555555555555555555555555555555555555555555555555555555555',
                        0,
                        X'3333333333333333333333333333333333333333333333333333333333333333',
                        X'6D697373696E672E7273',
                        X'5555555555555555555555555555555555555555555555555555555555555555',
                        0, 'manual-review', 1, 'local_asserted', 'trusted', 2
                     )",
                [],
            )
            .is_err()
    );
    assert_eq!(
        connection
            .execute(
                "INSERT INTO memory_correspondence_audit(
                        workspace_id, record_id, revision_digest, evidence_ordinal,
                        operation, source_snapshot_digest, source_repository_path,
                        source_artifact_digest, source_fact_ordinal,
                        target_snapshot_digest, target_repository_path,
                        target_artifact_digest, target_fact_ordinal,
                        method_id, method_version, trusted_actor_kind,
                        trusted_actor_id, recorded_at_unix_ms
                     ) VALUES (
                        1, X'11111111111111111111111111111111',
                        X'2222222222222222222222222222222222222222222222222222222222222222',
                        0, 'approved',
                        X'3333333333333333333333333333333333333333333333333333333333333333',
                        X'7372632F6C69622E7273',
                        X'5555555555555555555555555555555555555555555555555555555555555555',
                        0,
                        X'3333333333333333333333333333333333333333333333333333333333333333',
                        X'7372632F6C69622E7273',
                        X'5555555555555555555555555555555555555555555555555555555555555555',
                        0, 'manual-review', 1, 'local_asserted', 'trusted', 2
                     )",
                [],
            )
            .expect("exact review audit should insert"),
        1
    );

    connection
        .execute(
            "INSERT INTO memory_projection_generations(
                    projection_id, workspace_id, index_generation_id, source_epoch,
                    snapshot_digest, target_kind, target_format, target_revision,
                    head_format, head_revision, correspondence_profile_id,
                    correspondence_profile_version, correspondence_profile_digest,
                    lifecycle_state, searched_count, skipped_count,
                    unresolved_count, truncated_count, total_count, current_count,
                    not_applicable_count, stale_count, needs_review_count,
                    indeterminate_count, conflicted_count, contradicted_count,
                    superseded_count, quarantined_count, tombstoned_count
                 ) VALUES (
                    2, 1, 1, 0,
                    X'3333333333333333333333333333333333333333333333333333333333333333',
                    'worktree', 'source_snapshot',
                    X'3333333333333333333333333333333333333333333333333333333333333333',
                    NULL, NULL, 'rust-name-elided', 1, zeroblob(32),
                    'staging', 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0
                 )",
            [],
        )
        .expect("review projection should stage");
    connection
        .execute(
            "INSERT INTO memory_projection_records(
                    projection_id, workspace_id, ordinal, record_id,
                    revision_digest, effective_state, validity_state,
                    evidence_state, reason, evidence_count, resolved_count,
                    review_count, indeterminate_count, head_count,
                    missing_parent_count, has_trusted_approval
                 ) VALUES (
                    2, 1, 0, X'11111111111111111111111111111111',
                    X'2222222222222222222222222222222222222222222222222222222222222222',
                    'needs_review', 'valid', 'ambiguous', 'evidence_ambiguous',
                    1, 0, 1, 0, 1, 0, 1
                 )",
            [],
        )
        .expect("review record should stage");
    connection
        .execute(
            "INSERT INTO memory_projection_evidence(
                    projection_id, workspace_id, record_ordinal, record_id,
                    revision_digest, evidence_ordinal, outcome, method_id,
                    method_version, assurance, target_snapshot_digest,
                    target_repository_path, target_artifact_digest,
                    target_fact_ordinal, target_declaration_digest,
                    target_name_elided_digest, candidate_coverage,
                    candidate_count_before_limit
                 ) VALUES (
                    2, 1, 0, X'11111111111111111111111111111111',
                    X'2222222222222222222222222222222222222222222222222222222222222222',
                    0, 'ambiguous', 'rust-name-elided', 1, 'none',
                    NULL, NULL, NULL, NULL, NULL, NULL, 'complete', 1
                 )",
            [],
        )
        .expect("ambiguous evidence should stage");
    assert!(
        connection
            .execute(
                "UPDATE memory_projection_generations
                     SET lifecycle_state = 'complete' WHERE projection_id = 2",
                [],
            )
            .is_err()
    );
    connection
        .execute(
            "INSERT INTO memory_projection_candidates(
                    projection_id, workspace_id, record_ordinal,
                    evidence_ordinal, ordinal, record_id, revision_digest,
                    target_snapshot_digest, target_repository_path,
                    target_artifact_digest, target_fact_ordinal,
                    target_declaration_digest, target_name_elided_digest,
                    proposed_relation, method_id, method_version, assurance
                 ) VALUES (
                    2, 1, 0, 0, 0, X'11111111111111111111111111111111',
                    X'2222222222222222222222222222222222222222222222222222222222222222',
                    X'3333333333333333333333333333333333333333333333333333333333333333',
                    X'7372632F6C69622E7273',
                    X'5555555555555555555555555555555555555555555555555555555555555555',
                    0,
                    X'6666666666666666666666666666666666666666666666666666666666666666',
                    X'7777777777777777777777777777777777777777777777777777777777777777',
                    'same', 'rust-name-elided', 1, 'review_required'
                 )",
            [],
        )
        .expect("review candidate should stage");
    assert_eq!(
        connection
            .execute(
                "UPDATE memory_projection_generations
                     SET lifecycle_state = 'complete' WHERE projection_id = 2",
                [],
            )
            .expect("complete review projection should publish"),
        1
    );
    assert!(
        connection
            .execute(
                "UPDATE memory_correspondence_audit SET operation = 'rejected'",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM memory_correspondence_audit", [])
            .is_err()
    );
}
