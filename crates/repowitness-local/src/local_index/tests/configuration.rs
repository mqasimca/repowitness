fn resolved_test_configuration(
    text: &str,
    layer: crate::ConfigurationFileLayer,
) -> repowitness_application::ResolvedConfiguration {
    let layer = crate::parse_configuration_file(text.as_bytes(), layer)
        .expect("test configuration should parse");
    repowitness_application::resolve_configuration(&[layer])
        .expect("test configuration should resolve")
}

fn add_fixture_source(repository: &Path, path: &str, content: &[u8]) {
    let destination = repository.join(path);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).expect("fixture source parent should be created");
    }
    fs::write(&destination, content).expect("fixture source should be written");
    let status = Command::new("git")
        .current_dir(repository)
        .args(["add", "--", path])
        .status()
        .expect("Git should stage the fixture source");
    assert!(status.success());
}

#[test]
fn resolved_language_policy_controls_index_selection() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    add_fixture_source(
        &repository,
        "source.go",
        b"package fixture\nfunc Run() {}\n",
    );
    let configuration = resolved_test_configuration(
        "schema_version = 1\n[policy]\nallowed_languages = [\"go\"]\n",
        crate::ConfigurationFileLayer::Repository,
    );

    let report = index_local_rust_repository(
        LocalIndexRequest::new(&repository, &directory.database(), REPOSITORY_ID, 0)
            .with_configuration(&configuration),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("the configured language subset should index");

    assert_eq!(report.indexed_rust_files(), 0);
    assert_eq!(report.indexed_go_files(), 1);
    assert_eq!(report.indexed_typescript_files(), 0);
    assert_eq!(report.indexed_tsx_files(), 0);
    assert_eq!(report.indexed_python_files(), 0);
    assert_eq!(report.skipped_policy_paths(), 1);
    assert_eq!(
        report.indexed_go_files()
            + report.skipped_policy_paths()
            + report.skipped_unsupported_paths(),
        report.discovered_paths()
    );
}

#[test]
fn empty_language_policy_publishes_an_explicit_empty_generation() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let configuration = resolved_test_configuration(
        "schema_version = 1\n[policy]\nallowed_languages = []\n",
        crate::ConfigurationFileLayer::Repository,
    );

    let report = index_local_rust_repository(
        LocalIndexRequest::new(&repository, &directory.database(), REPOSITORY_ID, 0)
            .with_configuration(&configuration),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("an empty language allow-list should safely publish empty coverage");

    assert_eq!(report.indexed_rust_files(), 0);
    assert_eq!(report.indexed_go_files(), 0);
    assert_eq!(report.total_source_bytes(), 0);
    assert_eq!(report.total_facts(), 0);
    assert_eq!(report.skipped_policy_paths(), 1);
    assert_eq!(
        report.skipped_policy_paths() + report.skipped_unsupported_paths(),
        report.discovered_paths()
    );
}

#[test]
fn configured_source_file_count_is_inclusive_and_fails_before_database_creation() {
    let exact_directory = TempDirectory::new();
    let exact_repository = fixture_repository(&exact_directory);
    let configuration = resolved_test_configuration(
        "schema_version = 1\n[policy]\nmax_source_files = 1\n",
        crate::ConfigurationFileLayer::Repository,
    );
    let exact = index_local_rust_repository(
        LocalIndexRequest::new(
            &exact_repository,
            &exact_directory.database(),
            REPOSITORY_ID,
            0,
        )
        .with_configuration(&configuration),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("the exact configured file count should be admitted");
    assert_eq!(exact.indexed_rust_files(), 1);

    let overflow_directory = TempDirectory::new();
    let overflow_repository = fixture_repository(&overflow_directory);
    add_fixture_source(
        &overflow_repository,
        "source.go",
        b"package fixture\nfunc Run() {}\n",
    );
    let database = overflow_directory.database();
    let error = index_local_rust_repository(
        LocalIndexRequest::new(&overflow_repository, &database, REPOSITORY_ID, 0)
            .with_configuration(&configuration),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("one source beyond the configured count must fail");

    assert!(matches!(
        error,
        LocalIndexError::Preparation {
            source: crate::LocalRustIndexError::Preparation {
                source: repowitness_application::RustIndexPreparationError::FileLimitExceeded {
                    limit: 1
                }
            }
        }
    ));
    assert!(
        !database.exists(),
        "failed preparation must not create a database"
    );
}

#[test]
fn configured_source_file_bytes_are_inclusive_and_one_over_fails_closed() {
    let exact_directory = TempDirectory::new();
    let exact_repository = fixture_repository(&exact_directory);
    let source_bytes =
        u64::try_from(fs::read(exact_repository.join("src/lib.rs")).unwrap().len()).unwrap();
    let exact_configuration = resolved_test_configuration(
        &format!("schema_version = 1\n[policy]\nmax_source_file_bytes = {source_bytes}\n"),
        crate::ConfigurationFileLayer::Repository,
    );
    index_local_rust_repository(
        LocalIndexRequest::new(
            &exact_repository,
            &exact_directory.database(),
            REPOSITORY_ID,
            0,
        )
        .with_configuration(&exact_configuration),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("the exact configured byte limit should be admitted");

    let overflow_directory = TempDirectory::new();
    let overflow_repository = fixture_repository(&overflow_directory);
    let database = overflow_directory.database();
    let overflow_configuration = resolved_test_configuration(
        &format!(
            "schema_version = 1\n[policy]\nmax_source_file_bytes = {}\n",
            source_bytes - 1
        ),
        crate::ConfigurationFileLayer::Repository,
    );
    let error = index_local_rust_repository(
        LocalIndexRequest::new(&overflow_repository, &database, REPOSITORY_ID, 0)
            .with_configuration(&overflow_configuration),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("one source byte beyond policy must fail");

    assert!(matches!(
        error,
        LocalIndexError::Preparation {
            source: crate::LocalRustIndexError::SourceRead { .. }
        }
    ));
    assert!(
        !database.exists(),
        "failed source reads must not create a database"
    );
}

#[test]
fn semantic_configuration_changes_snapshot_identity_without_discarding_exact_artifacts() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let defaults =
        repowitness_application::resolve_configuration(&[]).expect("defaults should resolve");
    let tuned = resolved_test_configuration(
        "schema_version = 1\n[preferences]\nquery_results = 1\n",
        crate::ConfigurationFileLayer::User,
    );
    assert_ne!(defaults.digest(), tuned.digest());

    let artifacts = phase0_local_source_artifact_identities();
    assert_ne!(
        super::phase0_local_source_snapshot_profile(artifacts, defaults.digest()).configuration,
        super::phase0_local_source_snapshot_profile(artifacts, tuned.digest()).configuration
    );

    let first = index_local_rust_repository(
        LocalIndexRequest::new(&repository, &directory.database(), REPOSITORY_ID, 0)
            .with_configuration(&defaults),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("default configuration should index");
    let second = index_local_rust_repository(
        LocalIndexRequest::new(&repository, &directory.database(), REPOSITORY_ID, 0)
            .with_configuration(&tuned),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("changed retrieval semantics should produce a new source snapshot");

    assert_ne!(first.generation(), second.generation());
    assert_eq!(second.reused_rust_files(), 1);
    assert_eq!(second.analyzed_rust_files(), 0);
}

#[test]
fn equal_semantics_from_different_layers_share_one_snapshot_configuration() {
    let user = resolved_test_configuration(
        "schema_version = 1\n[preferences]\nquery_results = 7\n",
        crate::ConfigurationFileLayer::User,
    );
    let repository = resolved_test_configuration(
        "schema_version = 1\n[preferences]\nquery_results = 7\n",
        crate::ConfigurationFileLayer::Repository,
    );
    assert_eq!(user.digest(), repository.digest());

    let artifacts = phase0_local_source_artifact_identities();
    assert_eq!(
        super::phase0_local_source_snapshot_profile(artifacts, user.digest()).configuration,
        super::phase0_local_source_snapshot_profile(artifacts, repository.digest()).configuration
    );
}
