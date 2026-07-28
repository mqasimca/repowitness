const INCOMPLETE_SOURCE: &[u8] = b"pub fn incomplete(\n";

fn parser_diagnostic_sources() -> Vec<ImmutableRustSource> {
    vec![
        ImmutableRustSource::new(
            RepositoryPath::try_from_bytes(b"src/incomplete.rs", PATH_LIMITS)
                .expect("incomplete source path should be valid"),
            INCOMPLETE_SOURCE.to_vec().into_boxed_slice(),
        ),
        ImmutableRustSource::new(
            RepositoryPath::try_from_bytes(b"src/stable.rs", PATH_LIMITS)
                .expect("stable source path should be valid"),
            b"pub fn stable() {}\n".to_vec().into_boxed_slice(),
        ),
    ]
}

fn prepared_parser_diagnostics() -> PreparedRustIndex {
    let clean = prepare_rust_index(
        parser_diagnostic_sources(),
        artifact_identity(),
        RustIndexLimits::default(),
        &AtomicBool::new(false),
        deadline(),
    )
    .expect("raw parser diagnostics should prepare");
    let incomplete = clean
        .files()
        .iter()
        .find(|file| file.path().as_bytes() == b"src/incomplete.rs")
        .expect("incomplete source should be present");
    assert!(incomplete.analysis().syntax_error_nodes() > 0);
    let classified = repowitness_analysis::RustSourceAnalysis::try_from_parts(
        incomplete.analysis().facts().to_vec(),
        incomplete.analysis().visited_nodes(),
        incomplete.analysis().syntax_error_nodes(),
        1,
        repowitness_analysis::RustAnalysisLimits::DEFAULT,
    )
    .expect("classified parser diagnostics should be valid");
    let reusable = BTreeMap::from([(incomplete.artifact_digest(), classified)]);
    repowitness_application::prepare_rust_index_with_reuse(
        parser_diagnostic_sources(),
        artifact_identity(),
        RustIndexLimits::default(),
        &reusable,
        &AtomicBool::new(false),
        deadline(),
    )
    .expect("classified parser diagnostics should prepare")
}

fn classified_artifact(
    prepared: &PreparedRustIndex,
) -> (repowitness_domain::AnalysisArtifactDigest, u64) {
    let file = prepared
        .files()
        .iter()
        .find(|file| file.analysis().known_parser_limitation_nodes() == 1)
        .expect("classified artifact should be present");
    (
        file.artifact_digest(),
        u64::from(file.analysis().syntax_error_nodes()),
    )
}

#[test]
fn active_diagnostics_aggregate_parser_counts_from_each_artifact_once() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let prepared = prepared_parser_diagnostics();
    let expected_raw = prepared.total_syntax_error_nodes();
    let expected_known = prepared.total_known_parser_limitation_nodes();
    let per_file_known = prepared
        .files()
        .iter()
        .map(|file| file.analysis().known_parser_limitation_nodes())
        .collect::<Vec<_>>();
    assert_eq!(per_file_known, vec![1, 0]);
    assert!(expected_raw >= expected_known);
    assert_eq!(expected_known, 1);

    let (writer, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("writer should start");
    writer
        .register_workspace(identity().repository(), 0, deadline())
        .expect("workspace should register");
    let generation = publish(&writer, 0, prepared);
    writer.shutdown(deadline()).expect("writer should stop");

    let reader = OwnedSqliteReader::start(&database, deadline()).expect("reader should start");
    let diagnostics = repository_diagnostics(
        &reader,
        RepositoryDiagnosticsRequest::new(
            identity().repository(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
    )
    .expect("active parser diagnostics should be valid");
    assert_eq!(*diagnostics.generation(), generation);
    assert_eq!(diagnostics.syntax_error_nodes(), expected_raw);
    assert_eq!(
        diagnostics.known_parser_limitation_nodes(),
        expected_known
    );
    reader.shutdown(deadline()).expect("reader should stop");
}

#[test]
fn active_diagnostics_reject_per_artifact_corruption_hidden_by_valid_totals() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let prepared = prepared_parser_diagnostics();
    let (classified, raw) = classified_artifact(&prepared);
    let unclassified = prepared
        .files()
        .iter()
        .find(|file| file.artifact_digest() != classified)
        .expect("unclassified artifact should be present")
        .artifact_digest();
    assert!(raw > 0);

    let (writer, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("writer should start");
    writer
        .register_workspace(identity().repository(), 0, deadline())
        .expect("workspace should register");
    publish(&writer, 0, prepared);
    writer.shutdown(deadline()).expect("writer should stop");

    let mut connection = Connection::open(&database).expect("fixture database should open");
    connection
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             DROP TRIGGER analysis_artifacts_no_semantic_update;",
        )
        .expect("fixture constraints should be bypassed");
    let transaction = connection
        .transaction()
        .expect("fixture corruption transaction should start");
    transaction
        .execute(
            "UPDATE analysis_artifacts
             SET syntax_error_nodes = 0,
                 known_parser_limitation_nodes = 1
             WHERE artifact_digest = ?1",
            params![classified.as_bytes().as_slice()],
        )
        .expect("classified artifact should be corrupted");
    transaction
        .execute(
            "UPDATE analysis_artifacts
             SET syntax_error_nodes = ?2
             WHERE artifact_digest = ?1",
            params![
                unclassified.as_bytes().as_slice(),
                i64::try_from(raw).expect("bounded parser count should fit in SQLite")
            ],
        )
        .expect("unclassified artifact should preserve the aggregate total");
    transaction
        .commit()
        .expect("fixture corruption transaction should commit");
    drop(connection);

    let reader = OwnedSqliteReader::start(&database, deadline()).expect("reader should start");
    assert!(matches!(
        reader.diagnostics(
            identity().repository(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
        Err(SqliteStoreError::IntegrityCheckFailed)
    ));
    reader.shutdown(deadline()).expect("reader should stop");
}

#[test]
fn reusable_parser_diagnostics_reject_negative_and_non_subset_counts() {
    for negative in [true, false] {
        let directory = TempDirectory::new();
        let database = directory.database();
        let prepared = prepared_parser_diagnostics();
        let (artifact, raw) = classified_artifact(&prepared);
        let invalid_known = if negative {
            -1
        } else {
            i64::try_from(raw + 1).expect("bounded parser count should fit in SQLite")
        };
        let (writer, _) =
            OwnedSqliteIndex::start(&database, 123, deadline()).expect("writer should start");
        writer
            .register_workspace(identity().repository(), 0, deadline())
            .expect("workspace should register");
        publish(&writer, 0, prepared);
        writer.shutdown(deadline()).expect("writer should stop");

        let connection = Connection::open(&database).expect("fixture database should open");
        connection
            .execute_batch(
                "PRAGMA ignore_check_constraints = ON;
                 DROP TRIGGER analysis_artifacts_no_semantic_update;",
            )
            .expect("fixture constraints should be bypassed");
        connection
            .execute(
                "UPDATE analysis_artifacts
                 SET known_parser_limitation_nodes = ?2
                 WHERE artifact_digest = ?1",
                params![artifact.as_bytes().as_slice(), invalid_known],
            )
            .expect("fixture parser diagnostics should be corrupted");
        drop(connection);

        let reader = OwnedSqliteReader::start(&database, deadline()).expect("reader should start");
        assert_eq!(
            reader.load_reusable_artifacts(
                &[artifact],
                artifact_identity(),
                RustIndexLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            ),
            Err(SqliteStoreError::IntegrityCheckFailed)
        );
        reader.shutdown(deadline()).expect("reader should stop");
    }
}

#[test]
fn immutable_artifact_verification_compares_known_parser_diagnostics() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let prepared = prepared_parser_diagnostics();
    let (artifact, raw) = classified_artifact(&prepared);
    let (writer, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("writer should start");
    writer
        .register_workspace(identity().repository(), 0, deadline())
        .expect("workspace should register");
    publish(&writer, 0, prepared);
    writer.shutdown(deadline()).expect("writer should stop");

    let connection = Connection::open(&database).expect("fixture database should open");
    connection
        .execute_batch("DROP TRIGGER analysis_artifacts_no_semantic_update")
        .expect("fixture immutability trigger should be removed");
    connection
        .execute(
            "UPDATE analysis_artifacts
             SET known_parser_limitation_nodes = 0
             WHERE artifact_digest = ?1",
            params![artifact.as_bytes().as_slice()],
        )
        .expect("fixture parser diagnostics should change");
    drop(connection);

    let (writer, _) =
        OwnedSqliteIndex::start(&database, 456, deadline()).expect("writer should restart");
    writer
        .register_workspace(identity().repository(), 0, deadline())
        .expect("workspace should remain registered");
    assert_eq!(
        writer
            .stage(
                0,
                identity(),
                prepared_parser_diagnostics(),
                GenerationCoverage::new(2, 0, raw, 0),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect_err("changed parser diagnostics must fail exact verification"),
        SqliteStoreError::IntegrityCheckFailed
    );
    writer.shutdown(deadline()).expect("writer should stop");
}
