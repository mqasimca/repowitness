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

fn raw_syntax_artifact_digest(database: &Path) -> repowitness_domain::AnalysisArtifactDigest {
    let connection = Connection::open(database).expect("fixture database should open");
    let digest = connection
        .query_row(
            "SELECT artifact_digest FROM syntax_site_artifacts",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .expect("fixture should contain one raw syntax artifact");
    repowitness_domain::AnalysisArtifactDigest::try_from_slice(&digest)
        .expect("persisted raw syntax digest should be fixed width")
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
fn complete_raw_syntax_artifacts_round_trip_through_the_bounded_reuse_reader() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("seed generation should activate");
    let digest = raw_syntax_artifact_digest(&database);

    let reader =
        OwnedSqliteReader::start(&database, deadline()).expect("reuse reader should start");
    let loaded = reader
        .load_reusable_raw_syntax_artifacts(
            &[digest],
            repowitness_application::raw_syntax_artifact_identities(),
            repowitness_application::RustIndexLimits::default(),
            repowitness_analysis::RawSyntaxSiteAnalysisLimits::DEFAULT,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("complete raw syntax artifact should pass integrity validation");
    let analysis = loaded.get(&digest).expect("requested artifact should load");
    assert!(analysis.visited_nodes() > 0);
    assert_eq!(analysis.language(), repowitness_analysis::RawSyntaxLanguage::Rust);

    let duplicate = reader
        .load_reusable_raw_syntax_artifacts(
            &[digest, digest],
            repowitness_application::raw_syntax_artifact_identities(),
            repowitness_application::RustIndexLimits::default(),
            repowitness_analysis::RawSyntaxSiteAnalysisLimits::DEFAULT,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect_err("duplicate requests must fail before reaching SQLite");
    assert_eq!(duplicate, crate::SqliteStoreError::IntegrityCheckFailed);
    reader
        .shutdown(deadline())
        .expect("reuse reader should shut down");
}

#[test]
fn reusable_raw_syntax_loading_honors_cancellation_and_deadline() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    index_local_rust_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("seed generation should activate");
    let digest = raw_syntax_artifact_digest(&database);
    let reader =
        OwnedSqliteReader::start(&database, deadline()).expect("reuse reader should start");
    let load = |cancelled, load_deadline| {
        reader.load_reusable_raw_syntax_artifacts(
            &[digest],
            repowitness_application::raw_syntax_artifact_identities(),
            repowitness_application::RustIndexLimits::default(),
            repowitness_analysis::RawSyntaxSiteAnalysisLimits::DEFAULT,
            Arc::new(AtomicBool::new(cancelled)),
            load_deadline,
        )
    };
    assert_eq!(load(true, deadline()), Err(crate::SqliteStoreError::Cancelled));
    assert_eq!(
        load(false, Instant::now()),
        Err(crate::SqliteStoreError::DeadlineExceeded)
    );
    reader
        .shutdown(deadline())
        .expect("reuse reader should shut down");
}

#[test]
fn raw_syntax_reuse_validates_every_supported_language_identity() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    fs::create_dir_all(repository.join("web")).expect("web fixture directory should be created");
    fs::create_dir_all(repository.join("sdk")).expect("SDK fixture directory should be created");
    let fixtures = [
        (
            "source.go",
            "package fixture\nfunc Run() { helper() }\nfunc helper() {}\n",
        ),
        (
            "web/api.ts",
            "export function run() { helper(); }\nfunction helper() {}\n",
        ),
        (
            "web/view.tsx",
            "export function View() { helper(); return <main />; }\nfunction helper() {}\n",
        ),
        (
            "sdk/client.py",
            "def run():\n    helper()\ndef helper():\n    pass\n",
        ),
    ];
    for (path, contents) in fixtures {
        fs::write(repository.join(path), contents).expect("mixed-language fixture should be written");
    }
    let status = Command::new("git")
        .current_dir(&repository)
        .args([
            "add",
            "--",
            "source.go",
            "web/api.ts",
            "web/view.tsx",
            "sdk/client.py",
        ])
        .status()
        .expect("Git should start");
    assert!(status.success());
    index_local_rust_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("mixed-language generation should activate");

    let connection = Connection::open(&database).expect("fixture database should open");
    let mut statement = connection
        .prepare(
            "SELECT artifact_digest
             FROM syntax_site_artifacts
             ORDER BY artifact_digest",
        )
        .expect("raw artifact query should prepare");
    let digests = statement
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .expect("raw artifact query should execute")
        .map(|row| {
            repowitness_domain::AnalysisArtifactDigest::try_from_slice(
                &row.expect("raw artifact row should decode"),
            )
            .expect("raw artifact digest should be fixed width")
        })
        .collect::<Vec<_>>();
    assert_eq!(digests.len(), 5);
    drop(statement);
    drop(connection);

    let reader =
        OwnedSqliteReader::start(&database, deadline()).expect("reuse reader should start");
    let loaded = reader
        .load_reusable_raw_syntax_artifacts(
            &digests,
            repowitness_application::raw_syntax_artifact_identities(),
            repowitness_application::RustIndexLimits::default(),
            repowitness_analysis::RawSyntaxSiteAnalysisLimits::DEFAULT,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("every supported raw syntax identity should reload");
    assert_eq!(loaded.len(), 5);
    reader
        .shutdown(deadline())
        .expect("reuse reader should shut down");
}

#[test]
fn corrupt_complete_raw_syntax_artifact_fails_reuse_and_preserves_active_generation() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let raw = repository.join("src/raw.rs");
    fs::write(&raw, "fn selected() { helper(); }\nfn helper() {}\n")
        .expect("raw-site fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "src/raw.rs"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    let first = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("seed generation should activate");

    let connection = Connection::open(&database).expect("fixture database should open");
    connection
        .execute_batch(
            "DROP TRIGGER syntax_sites_no_update;
             UPDATE syntax_sites
             SET raw_target = 'Tamper'
             WHERE (artifact_digest, ordinal) = (
                 SELECT artifact_digest, ordinal
                 FROM syntax_sites
                 WHERE raw_target = 'helper'
                 LIMIT 1
             );",
        )
        .expect("fixture should inject a raw-site payload mismatch");
    drop(connection);

    let error = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect_err("corrupt complete raw syntax data must fail closed");
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

#[test]
fn raw_syntax_sites_are_exactly_contained_in_the_selected_declaration() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    fs::write(
        repository.join("src/raw.rs"),
        "fn selected() { helper(); let _ = value; }\nfn helper() {}\n",
    )
    .expect("raw-site fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "src/raw.rs"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    index_local_rust_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("raw-site generation should activate");

    let identity = RepositoryIdentityTextV1::decode(REPOSITORY_ID)
        .expect("fixture identity should decode");
    let reader = OwnedSqliteReader::start(&database, deadline()).expect("reader should start");
    let search = reader
        .search(
            identity,
            "selected",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("selected declaration should search");
    let declaration = search
        .hits()
        .iter()
        .find(|hit| hit.name() == "selected")
        .expect("selected declaration should be indexed");
    let selector = repowitness_application::SymbolGetSelector::new(
        declaration.path().clone(),
        declaration.content_digest(),
        declaration.artifact_digest(),
        declaration.fact_ordinal(),
    );
    let result = reader
        .raw_syntax_sites_for_symbol(
            identity,
            search.snapshot(),
            search.generation(),
            selector.clone(),
            RawSyntaxSiteReadLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("raw sites should load from the complete projection");
    assert_eq!(
        result.availability(),
        RawSyntaxSiteProjectionAvailability::Complete
    );
    assert_eq!(result.declaration().map(|hit| hit.name()), Some("selected"));
    assert!(result.sites().iter().any(|record| {
        record.site().kind() == repowitness_analysis::RawSyntaxSiteKind::Call
            && record.site().raw_target() == "helper"
    }));
    assert!(result.sites().iter().all(|record| {
        record.site().occurrence_span().start().get()
            >= declaration.declaration_span().start().get()
            && record.site().occurrence_span().end().get()
                <= declaration.declaration_span().end().get()
    }));
    let application_result = repowitness_application::outbound_sites(
        &reader,
        repowitness_application::OutboundSitesRequest::new(
            identity,
            search.snapshot(),
            search.generation(),
            selector,
            repowitness_application::OutboundSitesLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
    )
    .expect("shared application contract should validate the local adapter");
    assert_eq!(
        application_result.availability(),
        repowitness_application::OutboundSitesAvailability::Complete
    );
    assert_eq!(
        application_result.notice(),
        repowitness_application::OutboundSitesNotice::NoTargetResolutionOrInferredEdges
    );
    assert!(application_result.sites().iter().any(|record| {
        record.site().kind() == repowitness_analysis::RawSyntaxSiteKind::Call
            && record.site().raw_target() == "helper"
    }));
    reader.shutdown(deadline()).expect("reader should shut down");
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the three generations and post-sweep assertions form one retention regression narrative"
)]
fn retention_collects_expired_raw_artifacts_only_through_the_planned_sweep() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let raw = repository.join("src/raw.rs");
    fs::write(&raw, "fn selected() { old_helper(); }\nfn old_helper() {}\n")
        .expect("first raw fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "src/raw.rs"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    let first = index_local_rust_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("first raw generation should activate");
    let connection = Connection::open(&database).expect("fixture database should open");
    let old_artifact: Vec<u8> = connection
        .query_row(
            "SELECT syntax_site_artifact_digest
             FROM generation_syntax_site_artifacts
             WHERE generation_id = ?1 AND repository_path = ?2",
            rusqlite::params![first.generation().get(), b"src/raw.rs".as_slice()],
            |row| row.get(0),
        )
        .expect("first raw artifact should exist");
    drop(connection);

    fs::write(&raw, "fn selected() { new_helper(); }\nfn new_helper() {}\n")
        .expect("second raw fixture should be written");
    fs::write(
        repository.join("src/lib.rs"),
        "pub struct UpdatedWidget;\nimpl UpdatedWidget { pub fn run() {} }\n",
    )
    .expect("second Rust fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "src/raw.rs", "src/lib.rs"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    let second = index_local_rust_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("second raw generation should activate");
    assert_ne!(first.generation(), second.generation());
    fs::write(
        &raw,
        "fn selected() { latest_helper(); }\nfn latest_helper() {}\n",
    )
    .expect("third raw fixture should be written");
    fs::write(
        repository.join("src/lib.rs"),
        "pub struct LatestWidget;\nimpl LatestWidget { pub fn run() {} }\n",
    )
    .expect("third Rust fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "src/raw.rs", "src/lib.rs"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    let latest = index_local_rust_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("third raw generation should activate");
    assert_ne!(second.generation(), latest.generation());

    let (store, _) = OwnedSqliteIndex::start(&database, 0, deadline())
        .expect("retention store should open");
    let policy = GenerationRetentionPolicy::try_new(
        1,
        RetentionLimits::default(),
        RetentionPins::default(),
    )
    .expect("one-generation retention policy should validate");
    let plan = store
        .plan_generation_retention(RetentionPlanRequest::new(
            policy.clone(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("retention plan should include only the expired generation");
    assert_eq!(plan.candidate_generations(), &[first.generation()]);
    let outcome = store
        .apply_generation_retention(RetentionApplyRequest::new(
            policy,
            plan.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("planned retention should collect expired raw artifacts");
    assert_eq!(outcome.generation_count(), 1);
    store.shutdown(deadline()).expect("retention store should stop");

    let connection = Connection::open(&database).expect("fixture database should reopen");
    let retained_old_artifact: i64 = connection
        .query_row(
            "SELECT count(*) FROM analysis_artifacts WHERE artifact_digest = ?1",
            [old_artifact],
            |row| row.get(0),
        )
        .expect("artifact retention state should load");
    assert_eq!(retained_old_artifact, 0);
    let active_raw_sites: i64 = connection
        .query_row(
            "SELECT count(*)
             FROM generation_syntax_site_artifacts
             WHERE generation_id = ?1",
            [latest.generation().get()],
            |row| row.get(0),
        )
        .expect("active raw projection should remain readable");
    assert!(active_raw_sites > 0);
}
