use super::*;

#[cfg(unix)]
#[test]
fn revalidation_reports_a_database_rename_after_projection_publication() {
    let fixture = TempDirectory::new();
    let repository = fixture.repository();
    let database = fixture.database();
    let moved = fixture.path.join("writer-opened.sqlite3");
    initialize_repository(&repository);
    let identity = RepositoryIdentityTextV1::encode(RepositoryIdentityDigest::new([0xAA; 32]));
    index_local_repository(
        LocalIndexRequest::new(&repository, &database, identity.as_str(), 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("source index should activate");
    import_commit_memory(&database);

    let report = super::super::revalidate_local_memory_with_hook(
        LocalMemoryRevalidationRequest::new(&repository, &database, identity.as_str(), 123),
        Arc::new(AtomicBool::new(false)),
        || fs::rename(&database, &moved).expect("published database should move"),
    )
    .expect("known projection publication should retain its report");

    let maintenance = report.maintenance();
    assert!(!maintenance.complete());
    assert_eq!(maintenance.warning_count(), 1);
    assert_eq!(
        maintenance.database_identity(),
        LocalMemoryDatabaseIdentity::ChangedAfterCommit
    );
    assert_eq!(
        maintenance.checkpoint(),
        LocalMemoryMaintenanceStep::Complete
    );
    assert_eq!(maintenance.shutdown(), LocalMemoryMaintenanceStep::Complete);
    let active_projection: i64 = Connection::open(moved)
        .expect("writer-opened database should remain readable")
        .query_row(
            "SELECT projection_id FROM active_memory_projections",
            [],
            |row| row.get(0),
        )
        .expect("published projection should remain active");
    assert_eq!(active_projection, report.projection_id());
}

#[test]
fn unknown_sqlite_mutations_keep_revalidation_operation_and_guidance() {
    let startup = map_store_startup_error(SqliteStoreError::MutationOutcomeUnknown);
    assert!(matches!(
        startup,
        LocalMemoryRevalidationError::MutationOutcomeUnknown {
            operation: LocalMemoryRevalidationMutation::StoreStartup,
        }
    ));
    assert!(
        startup
            .reconciliation_guidance()
            .is_some_and(|guidance| guidance.contains("read-only database diagnostics"))
    );

    let publication = map_revalidation_mutation_error(
        LocalMemoryRevalidationMutation::ProjectionPublication,
        SqliteStoreError::MutationOutcomeUnknown,
        |source| LocalMemoryRevalidationError::Publication { source },
    );
    assert!(matches!(
        publication,
        LocalMemoryRevalidationError::MutationOutcomeUnknown {
            operation: LocalMemoryRevalidationMutation::ProjectionPublication,
        }
    ));
    assert!(
        publication
            .reconciliation_guidance()
            .is_some_and(|guidance| guidance.contains("active memory projection"))
    );
    assert_eq!(
        publication.to_string(),
        "local memory revalidation mutation outcome could not be determined"
    );
    assert!(!publication.to_string().contains("publication failed"));

    let checkpoint = map_revalidation_mutation_error(
        LocalMemoryRevalidationMutation::Checkpoint,
        SqliteStoreError::MutationOutcomeUnknown,
        |source| LocalMemoryRevalidationError::Checkpoint { source },
    );
    assert!(matches!(
        checkpoint,
        LocalMemoryRevalidationError::MutationOutcomeUnknown {
            operation: LocalMemoryRevalidationMutation::Checkpoint,
        }
    ));
    assert!(
        checkpoint
            .reconciliation_guidance()
            .is_some_and(|guidance| guidance.contains("already-published memory projection"))
    );
}
