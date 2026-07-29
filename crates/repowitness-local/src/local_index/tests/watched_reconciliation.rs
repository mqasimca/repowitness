#[test]
fn watched_reconciliation_skips_unchanged_source_and_publishes_changes() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);

    let first = reconcile_local_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("startup reconciliation should publish");
    let LocalReconciliationOutcome::Published(first) = first else {
        panic!("startup reconciliation must publish");
    };
    assert_eq!(first.generation().get(), 1);
    assert_eq!(first.source_epoch(), 1);

    let unchanged = reconcile_local_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("quiet reconciliation should succeed");
    let LocalReconciliationOutcome::Unchanged(unchanged) = unchanged else {
        panic!("quiet reconciliation must not publish");
    };
    assert_eq!(unchanged.generation(), first.generation());
    assert_eq!(unchanged.source_epoch(), first.source_epoch());

    fs::write(
        repository.join("src/lib.rs"),
        "pub struct Widget;\nimpl Widget { pub fn changed() {} }\n",
    )
    .expect("fixture source should change");
    let changed = reconcile_local_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("changed reconciliation should publish");
    let LocalReconciliationOutcome::Published(changed) = changed else {
        panic!("changed reconciliation must publish");
    };
    assert_eq!(changed.generation().get(), 2);
    assert_eq!(changed.source_epoch(), 2);
}

#[test]
fn cancelled_watched_reconciliation_preserves_the_active_generation() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    let first = reconcile_local_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("startup reconciliation should publish");
    let LocalReconciliationOutcome::Published(first) = first else {
        panic!("startup reconciliation must publish");
    };
    let cancelled = Arc::new(AtomicBool::new(true));

    let error = reconcile_local_repository(request, cancelled)
        .expect_err("cancelled reconciliation should fail before publication");
    assert!(matches!(
        error,
        LocalIndexError::Preparation {
            source: crate::LocalRustIndexError::Cancelled
        }
    ));

    let reader = OwnedSqliteReader::start(&database, deadline())
        .expect("previous active generation should remain readable");
    let identity =
        RepositoryIdentityTextV1::decode(REPOSITORY_ID).expect("fixture identity should decode");
    let active = reader
        .search(
            identity,
            "Widget",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("previous active generation should remain searchable");
    assert_eq!(active.generation(), first.generation());
    assert!(!active.hits().is_empty());
    reader
        .shutdown(deadline())
        .expect("reader should shut down");
}
