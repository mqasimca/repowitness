#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the migration ledger golden vectors are intentionally kept in one exact fixture"
)]
fn migration_checksums_are_stable_golden_vectors() {
    assert_eq!(
        migration_checksum(MIGRATION_1),
        [
            0x47, 0xca, 0xe5, 0x1f, 0x5f, 0x5f, 0xa8, 0x39, 0xd0, 0xcd, 0xe3, 0xdc, 0xb8, 0x53,
            0x48, 0x78, 0x7e, 0x0c, 0x9d, 0xe7, 0x6a, 0xb4, 0x08, 0xd8, 0xd3, 0x06, 0x48, 0x83,
            0x1d, 0xc2, 0x76, 0xd9,
        ]
    );
    assert_eq!(
        migration_checksum(MIGRATION_2),
        [
            0x20, 0xef, 0xea, 0x28, 0xa3, 0x13, 0x9d, 0xfe, 0x67, 0xcf, 0x22, 0x64, 0x31, 0xb5,
            0x6e, 0x0d, 0xf0, 0xdb, 0xfe, 0x2d, 0xeb, 0x35, 0xbb, 0x96, 0x42, 0x51, 0xac, 0x47,
            0xd7, 0x88, 0xc3, 0x39,
        ]
    );
    assert_eq!(
        migration_checksum(MIGRATION_3),
        [
            0xb2, 0xcc, 0x73, 0x3c, 0xe8, 0xeb, 0xd2, 0xd2, 0x3e, 0x33, 0x12, 0x62, 0x57, 0xec,
            0x40, 0x92, 0xb7, 0xad, 0xf4, 0xdc, 0xdd, 0xb8, 0x64, 0xd9, 0x20, 0x12, 0x51, 0xaa,
            0x27, 0x17, 0xfc, 0xd8,
        ]
    );
    assert_eq!(
        migration_checksum(MIGRATION_4),
        [
            0x20, 0xcb, 0x92, 0x11, 0xca, 0x11, 0xc4, 0x04, 0x1b, 0x48, 0xb6, 0xc2, 0x87,
            0xb7, 0x2c, 0x71, 0x43, 0x96, 0xeb, 0x85, 0x3d, 0x11, 0xde, 0x41, 0xaf, 0xf3,
            0x6c, 0xbd, 0x52, 0xad, 0x23, 0xd8,
        ]
    );
    assert_eq!(
        migration_checksum(MIGRATION_5),
        [
            0x7f, 0x72, 0x30, 0x6e, 0xd3, 0xd2, 0x79, 0x7a, 0x52, 0xbd, 0xae, 0x17, 0x6d,
            0x7a, 0x01, 0x32, 0xa5, 0xd0, 0x54, 0x56, 0xd2, 0x5a, 0x81, 0xd3, 0x44, 0x6b,
            0xa9, 0x8a, 0x7d, 0x08, 0xab, 0x1e,
        ]
    );
    assert_eq!(
        migration_checksum(MIGRATION_6),
        [
            0x74, 0xbe, 0xa1, 0xfd, 0xe3, 0x65, 0xed, 0x16, 0x99, 0x34, 0xbd, 0xcf, 0xe3,
            0x03, 0x3c, 0x31, 0x3f, 0x83, 0x39, 0x13, 0xfc, 0x72, 0x9e, 0x64, 0x3b, 0xbe,
            0xcc, 0xe1, 0x90, 0x90, 0xc7, 0xd2,
        ]
    );
    assert_eq!(
        migration_checksum(MIGRATION_7),
        [
            0xf9, 0x9c, 0x88, 0x44, 0x76, 0xcf, 0x33, 0x42, 0x35, 0xa2, 0x57, 0x06, 0xcd,
            0x97, 0xfd, 0x77, 0xd2, 0xa8, 0x66, 0xd0, 0x07, 0xfd, 0x64, 0x97, 0x10, 0x7d,
            0xac, 0x4a, 0x7c, 0x76, 0xcf, 0x1b,
        ]
    );
    assert_eq!(
        migration_checksum(MIGRATION_9),
        [
            0x4a, 0x2d, 0xb4, 0x51, 0x7e, 0xdd, 0x4d, 0x3b, 0xeb, 0xe1, 0x10, 0xb2, 0xf0,
            0xc8, 0x46, 0x19, 0xcd, 0x4e, 0x6b, 0xba, 0x7e, 0x08, 0x76, 0xae, 0xb1, 0x20,
            0xbc, 0x80, 0x4c, 0x6b, 0xa1, 0x6b,
        ]
    );
    assert_eq!(
        migration_checksum(MIGRATION_10),
        [
            0x73, 0x5a, 0xcf, 0x10, 0x3e, 0x33, 0x19, 0x73, 0xad, 0x76, 0xba, 0xb6, 0xed,
            0x9c, 0xc3, 0x39, 0x01, 0x97, 0xbd, 0xe2, 0x33, 0x97, 0x32, 0x3d, 0xe1, 0x2b,
            0xcb, 0x28, 0xac, 0xb7, 0x8b, 0x93,
        ]
    );
    assert_eq!(
        migration_checksum(MIGRATION_12),
        [
            0xf9, 0xa2, 0x14, 0x3e, 0x31, 0x7b, 0x52, 0x08, 0x55, 0x46, 0x5c, 0xa7, 0x74,
            0xea, 0x31, 0x6b, 0xc5, 0x5b, 0x45, 0x7a, 0xf2, 0x27, 0x34, 0x65, 0x32, 0x2e,
            0x23, 0xd0, 0x9d, 0x0d, 0x00, 0xfa,
        ]
    );
    assert_eq!(
        migration_checksum(MIGRATION_13),
        [
            0xcf, 0x82, 0xa1, 0x16, 0xff, 0xcc, 0x98, 0x4d, 0x88, 0x1a, 0x7b, 0xa5, 0x8a, 0xac,
            0x04, 0xeb, 0x09, 0xe2, 0x32, 0x1f, 0x56, 0xe3, 0xaf, 0xf6, 0x0e, 0x12, 0x22, 0xda,
            0xea, 0x47, 0x6c, 0xf6,
        ]
    );
    assert_eq!(
        migration_checksum(MIGRATION_14),
        [
            0xba, 0x68, 0xd9, 0xe1, 0x8f, 0xac, 0x77, 0xe0, 0x35, 0x81, 0x08, 0x66, 0x55, 0xbb,
            0x3e, 0x39, 0x81, 0xec, 0x2a, 0xf8, 0xd7, 0x3b, 0xaf, 0xe7, 0x8a, 0x28, 0x41, 0x8c,
            0x74, 0x8c, 0x5d, 0x5d,
        ]
    );
    assert_eq!(
        migration_checksum(MIGRATION_15),
        [
            0x24, 0x7f, 0xb2, 0x7c, 0xf2, 0x46, 0x70, 0x0b, 0x2e, 0x99, 0xfa, 0x83, 0x94, 0x26,
            0xe2, 0x82, 0xc3, 0x31, 0x90, 0x82, 0xc8, 0x98, 0x9c, 0x19, 0x33, 0x9f, 0xb7, 0xba,
            0x1b, 0x39, 0xf7, 0xc7,
        ]
    );
    assert_eq!(
        migration_checksum(MIGRATION_16),
        [
            0x66, 0x7a, 0x8c, 0xe6, 0xca, 0xbd, 0x24, 0x5e, 0xa2, 0x14, 0x70, 0x4f, 0x50, 0x20,
            0x6b, 0xad, 0xc4, 0x60, 0x72, 0x5b, 0x82, 0x09, 0xc0, 0x76, 0xa1, 0x85, 0xcf, 0xae,
            0x29, 0x4d, 0x65, 0x2a,
        ]
    );
    assert_eq!(
        migrations(),
        [
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
            (15, MIGRATION_15_NAME, MIGRATION_15),
            (16, MIGRATION_16_NAME, MIGRATION_16),
        ]
    );
    for transitional_statement in ["CREATE TEMP", "ALTER TABLE", "DROP TABLE"] {
        assert!(!MIGRATION_1.contains(transitional_statement));
    }
}

#[test]
fn current_catalog_matches_the_current_schema_golden() {
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
            0x84, 0xfb, 0x2c, 0x33, 0xbe, 0x65, 0x5f, 0x85, 0xc6, 0x36, 0xee, 0x44, 0xa9, 0x7d,
            0x06, 0x1d, 0x2d, 0x61, 0x1f, 0x59, 0xcd, 0xe4, 0x1f, 0xca, 0x09, 0x49, 0x0f, 0xd6,
            0x8b, 0x0f, 0xd5, 0x0e,
        ]
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the immutable overlay receipt fixture validates every scoped persisted field in one transactionally constructed schema state"
)]
fn scip_overlay_receipt_is_exact_complete_immutable_and_view_scoped() {
    let directory = TempDirectory::new();
    let connection =
        open_index_writer(&directory.database(), 123).expect("migration should succeed");
    insert_workspace(&connection);
    insert_active_generation_fixture(&connection);
    connection
        .execute_batch(
            "BEGIN IMMEDIATE;
             INSERT INTO connected_workspaces(connected_workspace_id)
             VALUES (X'1010101010101010101010101010101010101010101010101010101010101010');
             INSERT INTO workspace_source_slots(
                connected_workspace_id, source_slot_id, repository_identity,
                generation_workspace_id, source_epoch
             ) VALUES (
                X'1010101010101010101010101010101010101010101010101010101010101010',
                X'2020202020202020202020202020202020202020202020202020202020202020',
                X'1010101010101010101010101010101010101010101010101010101010101010',
                1, 0
             );
             INSERT INTO source_slot_generation_receipts(
                connected_workspace_id, source_slot_id, source_epoch,
                generation_workspace_id, generation_id
             ) VALUES (
                X'1010101010101010101010101010101010101010101010101010101010101010',
                X'2020202020202020202020202020202020202020202020202020202020202020',
                0, 1, 1
             );
             INSERT INTO workspace_views(connected_workspace_id, lifecycle_state)
             VALUES (
                X'1010101010101010101010101010101010101010101010101010101010101010',
                'staging'
             );
             INSERT INTO workspace_view_members(
                workspace_view_id, connected_workspace_id, source_slot_id,
                source_epoch, ordinal, generation_workspace_id, generation_id
             ) VALUES (
                1,
                X'1010101010101010101010101010101010101010101010101010101010101010',
                X'2020202020202020202020202020202020202020202020202020202020202020',
                0, 0, 1, 1
             );
             UPDATE workspace_views SET lifecycle_state = 'published'
             WHERE workspace_view_id = 1;
             INSERT INTO scip_overlay_receipts(
                overlay_digest, connected_workspace_id, workspace_view_id,
                source_slot_id, source_epoch, generation_workspace_id, generation_id,
                source_snapshot_digest, source_manifest_digest, configuration_digest,
                producer_digest, schema_digest, importer_digest, input_digest,
                lifecycle_state, document_count, occurrence_count, relationship_count
             ) VALUES (
                X'9999999999999999999999999999999999999999999999999999999999999999',
                X'1010101010101010101010101010101010101010101010101010101010101010',
                1,
                X'2020202020202020202020202020202020202020202020202020202020202020',
                0, 1, 1,
                X'3333333333333333333333333333333333333333333333333333333333333333',
                zeroblob(32), zeroblob(32), zeroblob(32), zeroblob(32),
                zeroblob(32), zeroblob(32),
                'staging', 1, 1, 1
             );
             COMMIT;",
        )
        .expect("exact staging receipt should be accepted");

    assert!(
        connection
            .execute(
                "INSERT INTO scip_overlay_documents(
                    overlay_digest, document_ordinal, repository_path, content_digest,
                    occurrence_count, relationship_count
                 ) VALUES (
                    X'9999999999999999999999999999999999999999999999999999999999999999',
                    0, X'6F746865722E7273',
                    X'4444444444444444444444444444444444444444444444444444444444444444',
                    1, 1
                 )",
                [],
            )
            .is_err(),
        "a document path outside the pinned generation must be rejected"
    );
    connection
        .execute_batch(
            "INSERT INTO scip_overlay_documents(
                overlay_digest, document_ordinal, repository_path, content_digest,
                occurrence_count, relationship_count
             ) VALUES (
                X'9999999999999999999999999999999999999999999999999999999999999999',
                0, X'7372632F6C69622E7273',
                X'4444444444444444444444444444444444444444444444444444444444444444',
                1, 1
             );",
        )
        .expect("exact generation document should stage");
    assert!(
        connection
            .execute(
                "UPDATE scip_overlay_receipts SET lifecycle_state = 'complete'
                 WHERE overlay_digest =
                   X'9999999999999999999999999999999999999999999999999999999999999999'",
                [],
            )
            .is_err(),
        "completion requires all declared facts"
    );
    connection
        .execute_batch(
            "INSERT INTO scip_overlay_occurrences(
                overlay_digest, document_ordinal, occurrence_ordinal,
                symbol, roles, start_byte, end_byte
             ) VALUES (
                X'9999999999999999999999999999999999999999999999999999999999999999',
                0, 0, X'73796D626F6C', 1, 0, 7
             );
             INSERT INTO scip_overlay_relationships(
                overlay_digest, document_ordinal, relationship_ordinal,
                source_symbol, target_symbol, kinds
             ) VALUES (
                X'9999999999999999999999999999999999999999999999999999999999999999',
                0, 0, X'73796D626F6C', X'746172676574', 1
             );
             UPDATE scip_overlay_receipts SET lifecycle_state = 'complete'
             WHERE overlay_digest =
               X'9999999999999999999999999999999999999999999999999999999999999999';
             INSERT INTO active_scip_overlays(
                connected_workspace_id, source_slot_id, workspace_view_id, overlay_digest
             ) VALUES (
                X'1010101010101010101010101010101010101010101010101010101010101010',
                X'2020202020202020202020202020202020202020202020202020202020202020',
                1,
                X'9999999999999999999999999999999999999999999999999999999999999999'
             );",
        )
        .expect("complete exact overlay should activate");

    assert!(
        connection
            .execute(
                "UPDATE scip_overlay_documents SET occurrence_count = 0
                 WHERE overlay_digest =
                   X'9999999999999999999999999999999999999999999999999999999999999999'",
                [],
            )
            .is_err(),
        "completed overlay documents must be immutable"
    );
    let active: (i64, i64, i64) = connection
        .query_row(
            "SELECT receipt.document_count, receipt.occurrence_count,
                    receipt.relationship_count
             FROM active_scip_overlays AS active
             JOIN scip_overlay_receipts AS receipt
               ON receipt.overlay_digest = active.overlay_digest",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("active receipt should remain readable");
    assert_eq!(active, (1, 1, 1));
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
                    'memory_profile_v2_versions', 'memory_profile_v2_audit',
                    'memory_profile_v2_parents',
                    'memory_validity_commits', 'memory_evidence',
                    'memory_relationships', 'memory_audit',
                    'artifact_fact_correspondence',
                    'memory_correspondence_audit',
                    'memory_projection_generations',
                    'memory_projection_records',
                    'memory_projection_evidence',
                    'memory_projection_candidates',
                    'active_memory_projections',
                    'connected_workspaces', 'workspace_source_slots',
                    'source_slot_generation_receipts',
                    'workspace_views', 'workspace_view_members',
                    'active_workspace_views',
                    'scip_overlay_receipts', 'scip_overlay_documents',
                    'scip_overlay_occurrences', 'scip_overlay_relationships',
                    'scip_enclosed_reference_edges',
                    'active_scip_overlays',
                    'retention_scip_overlay_garbage',
                    'rust_graph_artifacts', 'rust_graph_sites',
                    'generation_graph_requirements',
                    'generation_graph_publications',
                    'generation_graph_sources',
                    'generation_graph_artifacts',
                    'generation_graph_definitions',
                    'generation_graph_resolutions',
                    'generation_graph_candidates',
                    'generation_graph_edges',
                    'generation_repository_topology_requirements',
                    'generation_repository_topology_publications',
                    'generation_repository_topology_entries',
                    'retention_generation_garbage',
                    'retention_snapshot_garbage',
                    'retention_artifact_garbage',
                    'retention_workspace_view_garbage',
                    'retention_source_slot_receipt_garbage',
                    'retention_collection_audit'
                    , 'personal_memory_records', 'personal_memory_audit',
                    'engineering_tasks', 'engineering_task_checkpoints',
                    'engineering_task_verifications'
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
                           'unique_memory_profile_v2_observation',
                           'unique_memory_profile_v2_local_approval',
                           'memory_evidence_occurrence_identity',
                           'unique_memory_correspondence_event'
                       )),
                    (SELECT count(*) FROM sqlite_schema
                     WHERE type = 'trigger' AND name GLOB 'memory_*')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("memory indexes and triggers should be introspectable");
    let graph_schema_objects: (i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT count(*) FROM sqlite_schema
                 WHERE type = 'index'
                   AND name IN (
                     'generation_graph_edges_by_kind',
                     'generation_graph_candidates_by_target'
                   )),
                (SELECT count(*) FROM sqlite_schema
                 WHERE type = 'trigger'
                   AND (
                     name GLOB 'rust_graph_*'
                     OR name GLOB 'generation_graph_*'
                     OR name = 'generation_activation_requires_graph_when_required'
                   ))",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("graph indexes and triggers should be introspectable");
    let retention_schema_objects: (i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT count(*) FROM sqlite_schema
                 WHERE type = 'table' AND name GLOB 'retention_*'),
                (SELECT count(*) FROM sqlite_schema
                 WHERE type = 'trigger'
                   AND (
                     name GLOB 'retention_*'
                     OR name = 'retained_generation_delete_requires_garbage'
                   ))",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("retention tables and triggers should be introspectable");

    assert_eq!(application_id, APPLICATION_ID);
    assert_eq!(user_version, SCHEMA_VERSION);
    assert_eq!(
        ledger,
        vec![
            (
                1,
                MIGRATION_1_NAME.to_owned(),
                migration_checksum(MIGRATION_1).to_vec(),
                123
            ),
            (
                2,
                MIGRATION_2_NAME.to_owned(),
                migration_checksum(MIGRATION_2).to_vec(),
                123
            ),
            (
                3,
                MIGRATION_3_NAME.to_owned(),
                migration_checksum(MIGRATION_3).to_vec(),
                123
            ),
            (
                4,
                MIGRATION_4_NAME.to_owned(),
                migration_checksum(MIGRATION_4).to_vec(),
                123
            ),
            (
                5,
                MIGRATION_5_NAME.to_owned(),
                migration_checksum(MIGRATION_5).to_vec(),
                123
            ),
            (
                6,
                MIGRATION_6_NAME.to_owned(),
                migration_checksum(MIGRATION_6).to_vec(),
                123
            ),
            (
                7,
                MIGRATION_7_NAME.to_owned(),
                migration_checksum(MIGRATION_7).to_vec(),
                123
            ),
            (
                9,
                MIGRATION_9_NAME.to_owned(),
                migration_checksum(MIGRATION_9).to_vec(),
                123
            ),
            (
                10,
                MIGRATION_10_NAME.to_owned(),
                migration_checksum(MIGRATION_10).to_vec(),
                123
            ),
            (
                11,
                MIGRATION_11_NAME.to_owned(),
                migration_checksum(MIGRATION_11).to_vec(),
                123
            ),
            (
                12,
                MIGRATION_12_NAME.to_owned(),
                migration_checksum(MIGRATION_12).to_vec(),
                123
            ),
            (
                13,
                MIGRATION_13_NAME.to_owned(),
                migration_checksum(MIGRATION_13).to_vec(),
                123
            ),
            (
                14,
                MIGRATION_14_NAME.to_owned(),
                migration_checksum(MIGRATION_14).to_vec(),
                123
            ),
            (
                15,
                MIGRATION_15_NAME.to_owned(),
                migration_checksum(MIGRATION_15).to_vec(),
                123
            ),
            (
                16,
                MIGRATION_16_NAME.to_owned(),
                migration_checksum(MIGRATION_16).to_vec(),
                123
            ),
        ]
    );
    assert_eq!(tables, 64);
    assert_eq!(memory_schema_objects, (6, 39));
    assert_eq!(graph_schema_objects, (2, 26));
    assert_eq!(retention_schema_objects, (7, 15));
    let payload_column: (String, i64) = connection
        .query_row(
            "SELECT type, [notnull] FROM pragma_table_info('analysis_artifacts')
                 WHERE name = 'payload_digest'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("artifact payload column should be present");
    assert_eq!(payload_column, ("BLOB".to_owned(), 0));
    let parser_diagnostic_column: (String, i64, Option<String>) = connection
        .query_row(
            "SELECT type, [notnull], dflt_value
             FROM pragma_table_info('analysis_artifacts')
             WHERE name = 'known_parser_limitation_nodes'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("known parser diagnostic column should be present");
    assert_eq!(
        parser_diagnostic_column,
        ("INTEGER".to_owned(), 1, Some("0".to_owned()))
    );
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
                    known_parser_limitation_nodes, payload_digest, language
                 ) VALUES (
                    zeroblob(32), 'staging', zeroblob(32), zeroblob(32),
                    zeroblob(32), zeroblob(32), 7, 1, 1, 0, 0,
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
fn graph_artifact_completion_rejects_missing_site_ordinals() {
    let directory = TempDirectory::new();
    let connection =
        open_index_writer(&directory.database(), 123).expect("migration should succeed");
    connection
        .execute_batch(
            "BEGIN IMMEDIATE;
             INSERT INTO analysis_artifacts(
                 artifact_digest, lifecycle_state, source_content_digest,
                 producer_manifest_digest, configuration_digest,
                 analysis_schema_digest, canonicalization_version,
                 fact_count, visited_nodes, syntax_error_nodes,
                 known_parser_limitation_nodes, payload_digest, language
              ) VALUES (
                 zeroblob(32), 'staging', zeroblob(32), zeroblob(32),
                 zeroblob(32), zeroblob(32), 1, 0, 3, 0, 0,
                 zeroblob(32), 'rust'
              );
             INSERT INTO rust_graph_artifacts(
                 artifact_digest, site_profile_version, site_count,
                 max_observed_depth, owned_text_bytes
              ) VALUES (zeroblob(32), 1, 2, 0, 0);
             INSERT INTO rust_graph_sites(
                 artifact_digest, ordinal, site_kind, extraction_evidence,
                 occurrence_start, occurrence_end, target_start, target_end,
                 raw_target
              ) VALUES
                 (zeroblob(32), 0, 'reference', 'direct_syntax', 0, 1, 0, 1, 'a'),
                 (zeroblob(32), 2, 'reference', 'direct_syntax', 2, 3, 2, 3, 'b');
             COMMIT;",
        )
        .expect("gapped graph artifact should stage");

    assert!(
        connection
            .execute(
                "UPDATE analysis_artifacts
                 SET lifecycle_state = 'complete'
                 WHERE artifact_digest = zeroblob(32)",
                [],
            )
            .is_err(),
        "completion must reject a graph artifact with a missing site ordinal"
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
