#[test]
fn shared_logical_row_budget_accepts_exact_boundary_and_blocks_one_over() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0xDA; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("retention store should start");
    let generations = retention_history(&store, repository, 3, 0xA0);
    let baseline_policy = retention_policy(1, RetentionLimits::default(), RetentionPins::default());
    let baseline = plan_retention(&store, baseline_policy);
    assert_eq!(baseline.candidate_generations(), &generations[..1]);
    let exact_rows = baseline.logical_work_rows();
    assert!(exact_rows > 1);

    let exact_limits =
        RetentionLimits::try_new(64, exact_rows, 512 * 1024 * 1024).expect("exact limits");
    let exact_policy = retention_policy(1, exact_limits, RetentionPins::default());
    let exact = plan_retention(&store, exact_policy.clone());
    assert_eq!(exact.logical_work_rows(), exact_rows);
    let outcome = store
        .apply_generation_retention(RetentionApplyRequest::new(
            exact_policy,
            exact.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("exact-boundary apply should commit");
    assert_eq!(outcome.generation_count(), 1);

    let second_repository = RepositoryIdentityDigest::new([0xDB; 32]);
    let second_generations = retention_history(&store, second_repository, 3, 0xB0);
    let second_baseline = plan_retention(
        &store,
        retention_policy(1, RetentionLimits::default(), RetentionPins::default()),
    );
    assert_eq!(
        second_baseline.candidate_generations(),
        &second_generations[..1]
    );
    let one_over_work = second_baseline.logical_work_rows();
    let one_under_limits = RetentionLimits::try_new(64, one_over_work - 1, 512 * 1024 * 1024)
        .expect("one-under limits");
    let one_under_policy = retention_policy(1, one_under_limits, RetentionPins::default());
    assert_eq!(
        store.plan_generation_retention(RetentionPlanRequest::new(
            one_under_policy,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )),
        Err(SqliteStoreError::RetentionLimitExceeded)
    );
    store.shutdown(deadline()).expect("store should stop");
}
