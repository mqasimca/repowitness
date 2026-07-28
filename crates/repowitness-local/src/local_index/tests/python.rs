#[test]
fn python_facade_persists_searches_and_reuses_py_and_pyi_artifacts() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    fs::create_dir_all(repository.join("sdk"))
        .expect("Python fixture directory should be created");
    fs::write(
        repository.join("sdk/client.py"),
        "class Client:\n    def send(self):\n        return None\n",
    )
    .expect("Python fixture should be written");
    fs::write(
        repository.join("sdk/types.pyi"),
        "class Response:\n    status: int\n",
    )
    .expect("Python stub fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "sdk/client.py", "sdk/types.pyi"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);

    let first = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("Python generation should activate");
    assert_eq!(first.discovered_paths(), 4);
    assert_eq!(first.indexed_rust_files(), 1);
    assert_eq!(first.indexed_python_files(), 2);
    assert_eq!(first.skipped_unsupported_paths(), 1);
    assert_eq!(first.reused_python_files(), 0);
    assert_eq!(first.analyzed_python_files(), 2);

    let reader = OwnedSqliteReader::start(&database, deadline())
        .expect("reader should open the Python generation");
    let repository_id =
        RepositoryIdentityTextV1::decode(REPOSITORY_ID).expect("fixture identity should decode");
    for (query, path) in [
        ("send", b"sdk/client.py".as_slice()),
        ("Response", b"sdk/types.pyi".as_slice()),
    ] {
        let results = reader
            .search(
                repository_id,
                query,
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("Python fact should be searchable");
        assert_eq!(results.hits().len(), 1);
        assert_eq!(results.hits()[0].language(), SourceLanguage::Python);
        assert_eq!(results.hits()[0].path().as_bytes(), path);
        assert_eq!(
            results.hits()[0].producer_manifest(),
            phase0_local_source_artifact_identities()
                .for_language(SourceLanguage::Python)
                .producer_manifest()
        );
    }
    reader
        .shutdown(deadline())
        .expect("reader should shut down");

    let second = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("equivalent Python generation should reuse both file kinds");
    assert_eq!(second.reused_python_files(), 2);
    assert_eq!(second.analyzed_python_files(), 0);
}
