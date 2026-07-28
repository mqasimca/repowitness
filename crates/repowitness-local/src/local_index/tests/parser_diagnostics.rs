const PARSER_DIAGNOSTIC_SOURCE: &[u8] = b"pub fn incomplete(\n";

fn parser_diagnostic_source() -> repowitness_application::ImmutableRustSource {
    repowitness_application::ImmutableRustSource::new(
        repowitness_domain::RepositoryPath::try_from_bytes(
            b"src/lib.rs",
            repowitness_domain::RepositoryPathLimits::new(4_096, 256),
        )
        .expect("parser diagnostic path should be valid"),
        PARSER_DIAGNOSTIC_SOURCE.to_vec().into_boxed_slice(),
    )
}

fn classified_parser_diagnostic_index() -> repowitness_application::PreparedRustIndex {
    let identity = phase0_local_rust_artifact_identity();
    let clean = repowitness_application::prepare_rust_index(
        vec![parser_diagnostic_source()],
        identity,
        repowitness_application::RustIndexLimits::default(),
        &AtomicBool::new(false),
        deadline(),
    )
    .expect("raw parser diagnostics should prepare");
    let file = &clean.files()[0];
    assert!(file.analysis().syntax_error_nodes() > 0);
    let classified = repowitness_analysis::RustSourceAnalysis::try_from_parts(
        file.analysis().facts().to_vec(),
        file.analysis().visited_nodes(),
        file.analysis().syntax_error_nodes(),
        1,
        repowitness_analysis::RustAnalysisLimits::DEFAULT,
    )
    .expect("known parser diagnostic fixture should be valid");
    let reusable = std::collections::BTreeMap::from([(file.artifact_digest(), classified)]);
    repowitness_application::prepare_rust_index_with_reuse(
        vec![parser_diagnostic_source()],
        identity,
        repowitness_application::RustIndexLimits::default(),
        &reusable,
        &AtomicBool::new(false),
        deadline(),
    )
    .expect("classified parser diagnostics should be reusable")
}

fn seed_classified_parser_diagnostic_artifact(database: &Path) -> (u64, u64) {
    let prepared = classified_parser_diagnostic_index();
    let raw = prepared.total_syntax_error_nodes();
    let known = prepared.total_known_parser_limitation_nodes();
    assert_eq!(known, 1);
    assert!(known <= raw);
    let repository = RepositoryIdentityTextV1::decode(REPOSITORY_ID)
        .expect("fixture repository identity should decode");
    let artifact_identity = phase0_local_rust_artifact_identity();
    let snapshot = repowitness_application::RustSourceSnapshotIdentity::new(
        repository,
        repowitness_domain::GitStateDigest::new([2; 32]),
        repowitness_domain::WorktreeStateDigest::new([3; 32]),
        artifact_identity.configuration(),
        artifact_identity.producer_manifest(),
        artifact_identity.schema(),
        artifact_identity.canonicalization_version(),
    );
    let (writer, _) =
        crate::OwnedSqliteIndex::start(database, 123, deadline()).expect("writer should start");
    writer
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let generation = writer
        .stage(
            0,
            snapshot,
            prepared,
            crate::GenerationCoverage::new(1, 0, raw, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("classified parser artifact should stage");
    writer
        .activate(generation, 0, deadline())
        .expect("classified parser artifact should activate");
    writer.shutdown(deadline()).expect("writer should stop");
    (raw, known)
}

#[test]
fn parser_diagnostics_survive_persistence_reuse_report_and_diagnostics() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    fs::write(repository.join("src/lib.rs"), PARSER_DIAGNOSTIC_SOURCE)
        .expect("parser diagnostic source should be written");
    assert!(
        Command::new("git")
            .current_dir(&repository)
            .args(["add", "--", "src/lib.rs"])
            .status()
            .expect("Git should start")
            .success()
    );
    let database = directory.database();
    let (raw, known) = seed_classified_parser_diagnostic_artifact(&database);

    let report = index_local_rust_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("the exact parser diagnostic artifact should be reused");
    assert_eq!(report.syntax_error_nodes(), raw);
    assert_eq!(report.known_parser_limitation_nodes(), known);
    assert_eq!(report.reused_rust_files(), 1);
    assert_eq!(report.analyzed_rust_files(), 0);

    let diagnostics = crate::diagnose_local_repository(
        crate::LocalRepositoryDiagnosticsRequest::new(&database, REPOSITORY_ID),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("active parser diagnostics should be readable");
    assert_eq!(diagnostics.syntax_error_nodes(), raw);
    assert_eq!(diagnostics.known_parser_limitation_nodes(), known);
}
