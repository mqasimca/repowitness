fn exact_projection_fixture() -> (
    TempDirectory,
    PathBuf,
    PathBuf,
    RepositoryIdentityDigest,
    String,
) {
    let fixture = TempDirectory::new();
    let repository = fixture.repository();
    let database = fixture.database();
    initialize_repository(&repository);
    let repository_identity = RepositoryIdentityDigest::new([0xA7; 32]);
    let identity = RepositoryIdentityTextV1::encode(repository_identity);
    index_local_repository(
        LocalIndexRequest::new(&repository, &database, identity.as_str(), 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("source index should activate");
    import_exact_memory(&database, repository_identity);

    let report = revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(&repository, &database, identity.as_str(), 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("exact memory projection should activate");

    assert_eq!(report.projected_records(), 1);
    assert_eq!(report.skipped_records(), 0);
    assert_eq!(report.unresolved_records(), 0);
    assert_eq!(report.git_queries(), 2);
    assert!(report.head_available());
    (
        fixture,
        repository,
        database,
        repository_identity,
        identity.as_str().to_owned(),
    )
}

fn exact_commit_projection_fixture() -> (
    TempDirectory,
    PathBuf,
    PathBuf,
    RepositoryIdentityDigest,
    String,
) {
    let fixture = TempDirectory::new();
    let repository = fixture.repository();
    let database = fixture.database();
    initialize_repository(&repository);
    let introduction = current_git_commit(&repository);
    let repository_identity = RepositoryIdentityDigest::new([0xA4; 32]);
    let identity = RepositoryIdentityTextV1::encode(repository_identity);
    index_local_repository(
        LocalIndexRequest::new(&repository, &database, identity.as_str(), 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("source index should activate");
    import_exact_commit_memory(&database, repository_identity, introduction);
    let report = revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(&repository, &database, identity.as_str(), 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("initial exact commit projection should activate");
    assert_eq!(report.unresolved_records(), 0);
    (
        fixture,
        repository,
        database,
        repository_identity,
        identity.into_string(),
    )
}
