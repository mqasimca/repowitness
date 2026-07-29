fn graph_artifact_digest(database: &Path) -> repowitness_domain::AnalysisArtifactDigest {
    let connection = Connection::open(database).expect("fixture database should open");
    let digest = connection
        .query_row(
            "SELECT artifact_digest FROM rust_graph_artifacts",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .expect("fixture should contain one graph artifact");
    repowitness_domain::AnalysisArtifactDigest::try_from_slice(&digest)
        .expect("persisted graph digest should be fixed width")
}

#[test]
fn complete_graph_artifacts_round_trip_through_the_bounded_reuse_reader() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("seed generation should activate");
    let digest = graph_artifact_digest(&database);

    let reader =
        OwnedSqliteReader::start(&database, deadline()).expect("reuse reader should start");
    let loaded = reader
        .load_reusable_graph_artifacts(
            &[digest],
            repowitness_application::phase1_rust_graph_artifact_identity(),
            repowitness_application::RustIndexLimits::default(),
            repowitness_analysis::RustGraphAnalysisLimits::DEFAULT,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("complete graph artifact should pass integrity validation");
    let analysis = loaded.get(&digest).expect("requested artifact should load");
    assert!(!analysis.sites().is_empty());
    assert!(analysis.visited_nodes() > 0);

    let duplicate = reader
        .load_reusable_graph_artifacts(
            &[digest, digest],
            repowitness_application::phase1_rust_graph_artifact_identity(),
            repowitness_application::RustIndexLimits::default(),
            repowitness_analysis::RustGraphAnalysisLimits::DEFAULT,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect_err("duplicate requests must fail before reaching SQLite");
    assert_eq!(
        duplicate,
        crate::SqliteStoreError::IntegrityCheckFailed
    );
    reader
        .shutdown(deadline())
        .expect("reuse reader should shut down");
}

#[test]
fn reusable_graph_loading_honors_cancellation_and_deadline() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    index_local_rust_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("seed generation should activate");
    let digest = graph_artifact_digest(&database);
    let reader =
        OwnedSqliteReader::start(&database, deadline()).expect("reuse reader should start");
    let load = |cancelled, load_deadline| {
        reader.load_reusable_graph_artifacts(
            &[digest],
            repowitness_application::phase1_rust_graph_artifact_identity(),
            repowitness_application::RustIndexLimits::default(),
            repowitness_analysis::RustGraphAnalysisLimits::DEFAULT,
            Arc::new(AtomicBool::new(cancelled)),
            load_deadline,
        )
    };

    assert_eq!(
        load(true, deadline()),
        Err(crate::SqliteStoreError::Cancelled)
    );
    assert_eq!(
        load(false, Instant::now()),
        Err(crate::SqliteStoreError::DeadlineExceeded)
    );
    reader
        .shutdown(deadline())
        .expect("reuse reader should shut down");
}

#[test]
fn corrupt_complete_graph_artifact_fails_reuse_and_preserves_active_generation() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    let first = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("seed generation should activate");

    let connection = Connection::open(&database).expect("fixture database should open");
    let original_target: String = connection
        .query_row(
            "SELECT raw_target FROM rust_graph_sites ORDER BY artifact_digest, ordinal LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("fixture should contain a graph site");
    assert_eq!(original_target.len(), 6);
    connection
        .execute_batch(
            "DROP TRIGGER rust_graph_sites_no_update;
             UPDATE rust_graph_sites
             SET raw_target = 'Tamper'
             WHERE (artifact_digest, ordinal) = (
                 SELECT artifact_digest, ordinal
                 FROM rust_graph_sites
                 ORDER BY artifact_digest, ordinal
                 LIMIT 1
             );",
        )
        .expect("fixture should inject a payload mismatch");
    drop(connection);

    let error = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect_err("corrupt complete reusable graph data must fail closed");
    assert!(matches!(
        error,
        LocalIndexError::ArtifactReuse {
            source: crate::SqliteStoreError::IntegrityCheckFailed
        }
    ));
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains("Tamper"));
    assert!(!rendered.contains(repository.to_string_lossy().as_ref()));
    assert_prior_generation_readable(&database, first.generation(), "Unpublished");
}

#[test]
fn identical_rust_contents_share_one_artifact_across_distinct_occurrences() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    fs::copy(repository.join("src/lib.rs"), repository.join("src/copy.rs"))
        .expect("identical Rust fixture should be copied");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "src/copy.rs"])
        .status()
        .expect("Git should start");
    assert!(status.success());

    let report = index_local_rust_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("content-local graph artifacts must support repeated occurrences");
    assert_eq!(report.indexed_rust_files(), 2);

    let connection = Connection::open(&database).expect("fixture database should open");
    let artifact_count: i64 = connection
        .query_row("SELECT count(*) FROM rust_graph_artifacts", [], |row| {
            row.get(0)
        })
        .expect("artifact count should load");
    let occurrence_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM generation_graph_artifacts",
            [],
            |row| row.get(0),
        )
        .expect("occurrence count should load");
    assert_eq!(artifact_count, 1);
    assert_eq!(occurrence_count, 2);
}
