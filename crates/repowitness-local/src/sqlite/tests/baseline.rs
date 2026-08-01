#[test]
fn pre_baseline_schema_versions_are_rejected_without_mutation() {
    for legacy_version in 1_i64..=8 {
        let directory = TempDirectory::new();
        let database = directory.database();
        let connection =
            Connection::open(&database).expect("legacy fixture database should be created");
        connection
            .execute_batch(
                "CREATE TABLE sentinel(value TEXT NOT NULL);
                 INSERT INTO sentinel(value) VALUES ('preserved');",
            )
            .expect("legacy sentinel should be created");
        connection
            .pragma_update(None, "application_id", APPLICATION_ID)
            .expect("legacy application ID should be set");
        connection
            .pragma_update(None, "user_version", legacy_version)
            .expect("legacy schema version should be set");
        if legacy_version == 1 {
            connection
                .execute_batch(
                    "CREATE TABLE schema_migrations (
                        version INTEGER PRIMARY KEY,
                        name TEXT NOT NULL,
                        checksum BLOB NOT NULL,
                        applied_at_unix_ms INTEGER NOT NULL
                     );
                     INSERT INTO schema_migrations(
                        version, name, checksum, applied_at_unix_ms
                     ) VALUES (
                        1,
                        'phase0_immutable_generations',
                        X'479ADD59E4AA5F9D2CBFC7E08E2608112DDC96E73FA9232F6ED0DD13361C9ECA',
                        123
                     );",
                )
                .expect("legacy version-one ledger should be created");
        }
        drop(connection);

        let original_bytes = fs::read(&database).expect("legacy database should be readable");
        let error = open_index_writer(&database, 456)
            .expect_err("pre-baseline databases must require an explicit rebuild");
        let expected = if legacy_version <= SCHEMA_VERSION {
            SqliteStoreError::MigrationLedgerMismatch
        } else {
            SqliteStoreError::SchemaVersionMismatch
        };
        assert_eq!(error, expected);
        assert_eq!(
            fs::read(&database).expect("rejected legacy database should remain readable"),
            original_bytes
        );
        assert!(!PathBuf::from(format!("{}-wal", database.display())).exists());
        assert!(!PathBuf::from(format!("{}-shm", database.display())).exists());
    }
}

#[test]
fn current_baseline_identity_version_and_ledger_fail_closed() {
    for (pragma, value, expected) in [
        ("application_id", 7, SqliteStoreError::ApplicationIdMismatch),
        ("user_version", 99, SqliteStoreError::SchemaVersionMismatch),
    ] {
        let directory = TempDirectory::new();
        drop(open_index_writer(&directory.database(), 123).expect("baseline should succeed"));
        let connection = raw_connection(&directory.database());
        connection
            .pragma_update(None, pragma, value)
            .expect("fixture pragma should change");
        drop(connection);
        let error = open_index_writer(&directory.database(), 456)
            .expect_err("mismatched identity or version should fail");
        assert_eq!(error, expected);
    }

    let directory = TempDirectory::new();
    drop(open_index_writer(&directory.database(), 123).expect("baseline should succeed"));
    let connection = raw_connection(&directory.database());
    connection
        .execute("UPDATE schema_migrations SET checksum = zeroblob(32)", [])
        .expect("fixture ledger should change");
    drop(connection);
    let error =
        open_index_writer(&directory.database(), 456).expect_err("a changed ledger should fail");
    assert_eq!(error, SqliteStoreError::MigrationLedgerMismatch);
}

#[test]
fn typescript_and_tsx_artifacts_accept_their_persisted_fact_kinds() {
    let directory = TempDirectory::new();
    let connection =
        open_index_writer(&directory.database(), 123).expect("baseline should succeed");

    for (byte, language, kinds) in [
        (
            2_u8,
            "typescript",
            ["class", "interface", "type_alias", "variable"],
        ),
        (3_u8, "tsx", ["function", "method", "enum", "module"]),
    ] {
        let digest = vec![byte; 32];
        connection
            .execute(
                "INSERT INTO analysis_artifacts(
                        artifact_digest, lifecycle_state, source_content_digest,
                        producer_manifest_digest, configuration_digest,
                        analysis_schema_digest, canonicalization_version,
                        fact_count, visited_nodes, syntax_error_nodes,
                        known_parser_limitation_nodes, payload_digest, language
                     ) VALUES (
                        ?1, 'staging', zeroblob(32), zeroblob(32), zeroblob(32),
                        zeroblob(32), 1, 4, 4, 0, 0, zeroblob(32), ?2
                     )",
                params![digest, language],
            )
            .expect("supported language should be accepted");
        for (ordinal, kind) in kinds.into_iter().enumerate() {
            connection
                .execute(
                    "INSERT INTO artifact_facts(
                            artifact_digest, ordinal, kind, name, qualified_name,
                            name_start, name_end, declaration_start, declaration_end
                         ) VALUES (?1, ?2, ?3, 'Name', 'fixture::Name', 0, 4, 0, 4)",
                    params![
                        vec![byte; 32],
                        i64::try_from(ordinal).expect("fixture ordinal fits"),
                        kind
                    ],
                )
                .expect("supported TypeScript fact kind should be accepted");
        }
    }
}

#[test]
fn parser_diagnostic_counts_are_nonnegative_and_known_is_a_raw_subset() {
    let directory = TempDirectory::new();
    let connection =
        open_index_writer(&directory.database(), 123).expect("baseline should succeed");
    let insert = "INSERT INTO analysis_artifacts(
            artifact_digest, lifecycle_state, source_content_digest,
            producer_manifest_digest, configuration_digest, analysis_schema_digest,
            canonicalization_version, fact_count, visited_nodes, syntax_error_nodes,
            known_parser_limitation_nodes, payload_digest, language
         ) VALUES (
            ?1, 'staging', zeroblob(32), zeroblob(32), zeroblob(32), zeroblob(32),
            1, 0, 1, ?2, ?3, zeroblob(32), 'typescript'
         )";

    for (byte, raw, known) in [(10_u8, -1_i64, 0_i64), (11, 0, 1)] {
        assert!(
            connection
                .execute(insert, params![vec![byte; 32], raw, known])
                .is_err(),
            "invalid parser diagnostics must fail at the schema boundary"
        );
    }
    connection
        .execute(insert, params![vec![12_u8; 32], 2_i64, 1_i64])
        .expect("a nonnegative known subset should persist");
}

#[test]
fn errors_are_stable_and_redacted() {
    let diagnostics = [
        SqliteStoreError::UnsupportedSqliteVersion,
        SqliteStoreError::OpenFailed,
        SqliteStoreError::ConfigurationFailed,
        SqliteStoreError::ApplicationIdMismatch,
        SqliteStoreError::SchemaVersionMismatch,
        SqliteStoreError::MigrationLedgerMismatch,
        SqliteStoreError::MigrationFailed,
        SqliteStoreError::Fts5Unavailable,
        SqliteStoreError::MutationLeaseUnavailable,
        SqliteStoreError::DatabaseIdentityChanged,
        SqliteStoreError::RecoveryGenerationLimitExceeded,
        SqliteStoreError::DatabaseStartupCleanupFailed,
        SqliteStoreError::DatabaseOperationFailed,
        SqliteStoreError::CountNotRepresentable,
        SqliteStoreError::WorkspaceUnavailable,
        SqliteStoreError::InvalidWorkspaceMembership,
        SqliteStoreError::WorkspaceSourceSlotLimitExceeded,
        SqliteStoreError::ConnectedWorkspaceUnavailable,
        SqliteStoreError::InvalidWorkspaceView,
        SqliteStoreError::InvalidMemoryImport,
        SqliteStoreError::StaleSourceEpoch,
        SqliteStoreError::InvalidSourceEpoch,
        SqliteStoreError::PreparedIdentityMismatch,
        SqliteStoreError::IntegrityCheckFailed,
        SqliteStoreError::GenerationUnavailable,
        SqliteStoreError::Cancelled,
        SqliteStoreError::DeadlineExceeded,
        SqliteStoreError::QueueFull,
        SqliteStoreError::WorkerUnavailable,
        SqliteStoreError::WorkerPanicked,
        SqliteStoreError::ReplyTimeout,
        SqliteStoreError::MutationOutcomeUnknown,
        SqliteStoreError::InvalidSearchLimits,
        SqliteStoreError::InvalidProjectionRebuildLimits,
        SqliteStoreError::ProjectionRebuildRowLimitExceeded,
        SqliteStoreError::InvalidSearchQuery,
        SqliteStoreError::SearchOutputLimitExceeded,
        SqliteStoreError::ArtifactReuseLimitExceeded,
        SqliteStoreError::InvalidBackupLimits,
        SqliteStoreError::BackupDestinationUnavailable,
        SqliteStoreError::BackupFailed,
        SqliteStoreError::BackupStepLimitExceeded,
        SqliteStoreError::BackupCleanupFailed,
    ];
    for error in diagnostics {
        let display = error.to_string();
        assert!(!display.contains('/'));
        assert!(!display.contains("sqlite_schema"));
    }
}
