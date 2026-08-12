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
fn source_only_reconciliation_publishes_without_eager_graph_storage() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0).without_graph();

    let outcome = reconcile_local_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("source-only reconciliation should publish");
    let LocalReconciliationOutcome::Published(report) = outcome else {
        panic!("source-only reconciliation must publish");
    };

    let connection = rusqlite::Connection::open(&database).expect("database should open");
    let graph_rows: i64 = connection
        .query_row(
            "SELECT (SELECT count(*) FROM generation_graph_requirements)
                  + (SELECT count(*) FROM generation_graph_publications)",
            [],
            |row| row.get(0),
        )
        .expect("graph receipt count should be readable");
    assert_eq!(graph_rows, 0);

    assert!(report.total_facts() > 0);

    let full = index_local_rust_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("explicit full indexing should build the graph later");
    assert!(full.generation() > report.generation());
    let connection = rusqlite::Connection::open(&database).expect("database should reopen");
    let graph_publications: i64 = connection
        .query_row(
            "SELECT count(*) FROM generation_graph_publications",
            [],
            |row| row.get(0),
        )
        .expect("graph publication count should be readable");
    assert_eq!(graph_publications, 1);
}

#[test]
fn unchanged_reconciliation_skips_generation_graph_preparation() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);

    let first = reconcile_local_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("startup reconciliation should publish");
    assert!(matches!(first, LocalReconciliationOutcome::Published(_)));

    let mut phases = Vec::new();
    let unchanged = reconcile_local_repository_with_control_hooks(
        request,
        Arc::new(AtomicBool::new(false)),
        |phase| phases.push(phase),
    )
    .expect("quiet reconciliation should succeed");
    assert!(matches!(unchanged, LocalReconciliationOutcome::Unchanged(_)));
    assert!(!phases.contains(&super::LocalIndexPhase::GraphProjectionPreparing));
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

fn watch_published_report(outcome: LocalReconciliationOutcome) -> super::LocalIndexReport {
    match outcome {
        LocalReconciliationOutcome::Published(report) => report,
        LocalReconciliationOutcome::Resumed(_) => {
            panic!("fixture reconciliation unexpectedly resumed staged work")
        }
        LocalReconciliationOutcome::Unchanged(_) => {
            panic!("fixture source change unexpectedly produced no generation")
        }
    }
}

fn watch_search_paths(database: &Path, query: &str) -> (i64, Vec<Vec<u8>>) {
    let reader = OwnedSqliteReader::start(database, deadline())
        .expect("watch fixture database should be readable");
    let results = reader
        .search(
            RepositoryIdentityTextV1::decode(REPOSITORY_ID)
                .expect("fixture identity should decode"),
            query,
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("watch fixture search should succeed");
    let generation = results.generation().get();
    let paths = results
        .hits()
        .iter()
        .map(|hit| hit.path().as_bytes().to_vec())
        .collect();
    reader
        .shutdown(deadline())
        .expect("watch fixture reader should shut down");
    (generation, paths)
}

fn watch_active_snapshot(database: &Path) -> Vec<u8> {
    Connection::open(database)
        .expect("watch fixture database should open")
        .query_row(
            "SELECT generation.snapshot_digest
             FROM workspaces AS workspace
             JOIN index_generations AS generation
               ON generation.generation_id = workspace.active_generation_id",
            [],
            |row| row.get(0),
        )
        .expect("watch fixture should have one active snapshot")
}

#[cfg(unix)]
fn add_case_colliding_index_entry(repository: &Path) {
    use std::os::unix::fs::symlink;

    let hash = Command::new("git")
        .current_dir(repository)
        .args(["hash-object", "-w", "--", "src/lib.rs"])
        .output()
        .expect("Git should hash the collision fixture");
    assert!(hash.status.success());
    match symlink("lib.rs", repository.join("src/Lib.rs")) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => panic!("case-collision alias should be created: {error}"),
    }
    let object = String::from_utf8(hash.stdout).expect("Git object id should be UTF-8");
    let status = Command::new("git")
        .current_dir(repository)
        .args([
            "-c",
            "core.ignorecase=false",
            "update-index",
            "--add",
            "--cacheinfo",
            "100644",
            object.trim(),
            "src/Lib.rs",
        ])
        .status()
        .expect("Git should add the collision fixture");
    assert!(status.success());
}

#[test]
fn tracked_delete_and_recreate_converge_to_the_same_snapshot_as_clean_indexing() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let watched_database = directory.0.join("watched.sqlite3");
    let watched_request = LocalIndexRequest::new(&repository, &watched_database, REPOSITORY_ID, 0);

    let initial = watch_published_report(
        reconcile_local_repository(watched_request, Arc::new(AtomicBool::new(false)))
            .expect("startup reconciliation should publish"),
    );
    fs::remove_file(repository.join("src/lib.rs"))
        .expect("tracked source should be deleted from the worktree");
    let deleted = watch_published_report(
        reconcile_local_repository(watched_request, Arc::new(AtomicBool::new(false)))
            .expect("stable tracked deletion should publish"),
    );
    assert_eq!(deleted.generation().get(), initial.generation().get() + 1);
    assert_eq!(deleted.source_epoch(), initial.source_epoch() + 1);
    assert_eq!(deleted.discovered_paths(), 1);
    assert_eq!(deleted.indexed_rust_files(), 0);
    assert_eq!(deleted.total_facts(), 0);
    assert_eq!(watch_search_paths(&watched_database, "Widget"), (2, vec![]));

    fs::write(
        repository.join("src/lib.rs"),
        "pub struct Recreated;\nimpl Recreated { pub fn run() {} }\n",
    )
    .expect("tracked source should be recreated");
    let recreated = watch_published_report(
        reconcile_local_repository(watched_request, Arc::new(AtomicBool::new(false)))
            .expect("recreated tracked source should publish"),
    );
    assert_eq!(recreated.generation().get(), 3);
    assert_eq!(recreated.source_epoch(), 3);
    assert_eq!(recreated.discovered_paths(), 2);
    assert_eq!(recreated.indexed_rust_files(), 1);

    let clean_database = directory.0.join("clean.sqlite3");
    let clean = index_local_rust_repository(
        LocalIndexRequest::new(&repository, &clean_database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("clean indexing of the final source should publish");
    assert_eq!(
        watch_active_snapshot(&watched_database),
        watch_active_snapshot(&clean_database)
    );
    assert_eq!(recreated.discovered_paths(), clean.discovered_paths());
    assert_eq!(recreated.indexed_rust_files(), clean.indexed_rust_files());
    assert_eq!(recreated.total_source_bytes(), clean.total_source_bytes());
    assert_eq!(recreated.total_facts(), clean.total_facts());

    let (watched_generation, watched_paths) = watch_search_paths(&watched_database, "Recreated");
    let (clean_generation, clean_paths) = watch_search_paths(&clean_database, "Recreated");
    assert_eq!(watched_generation, 3);
    assert_eq!(clean_generation, 1);
    assert_eq!(watched_paths, clean_paths);
    assert!(
        watched_paths
            .iter()
            .all(|path| path.as_slice() == b"src/lib.rs")
    );
}

#[test]
fn unstaged_case_only_rename_preserves_exact_path_identity() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    let initial = watch_published_report(
        reconcile_local_repository(request, Arc::new(AtomicBool::new(false)))
            .expect("startup reconciliation should publish"),
    );

    let intermediate = repository.join("src/rename-in-progress");
    fs::rename(repository.join("src/lib.rs"), &intermediate)
        .expect("source should move through a distinct intermediate path");
    fs::rename(&intermediate, repository.join("src/Lib.rs"))
        .expect("source should complete a case-only rename");
    let renamed = watch_published_report(
        reconcile_local_repository(request, Arc::new(AtomicBool::new(false)))
            .expect("case-only rename should publish"),
    );

    assert_eq!(renamed.generation().get(), initial.generation().get() + 1);
    assert_eq!(renamed.source_epoch(), initial.source_epoch() + 1);
    assert_eq!(renamed.discovered_paths(), 2);
    assert_eq!(renamed.indexed_rust_files(), 1);
    let (generation, paths) = watch_search_paths(&database, "Widget");
    assert_eq!(generation, renamed.generation().get());
    assert!(!paths.is_empty());
    assert!(paths.iter().all(|path| path.as_slice() == b"src/Lib.rs"));
}

#[test]
#[cfg(unix)]
fn unsupported_case_colliding_index_paths_fail_closed_and_preserve_active_state() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    let initial = watch_published_report(
        reconcile_local_repository(request, Arc::new(AtomicBool::new(false)))
            .expect("startup reconciliation should publish"),
    );
    add_case_colliding_index_entry(&repository);

    let error = reconcile_local_repository(request, Arc::new(AtomicBool::new(false)))
        .expect_err("unrepresentable case-colliding paths must fail closed");
    assert!(matches!(
        error,
        LocalIndexError::Preparation {
            source:
                crate::LocalRustIndexError::Discovery {
                    source: crate::GitPathDiscoveryError::InconsistentRepositoryPathSet
                } | crate::LocalRustIndexError::SourceRead { .. }
        }
    ));
    let diagnostic = format!("{error} {error:?}");
    assert!(!diagnostic.contains("src/lib.rs"));
    assert!(!diagnostic.contains("src/Lib.rs"));

    let (generation, paths) = watch_search_paths(&database, "Widget");
    assert_eq!(generation, initial.generation().get());
    assert!(!paths.is_empty());
    assert!(paths.iter().all(|path| path.as_slice() == b"src/lib.rs"));
}

fn wait_for_watch_activation(database: &Path, cancelled: &AtomicBool) {
    let timeout = Instant::now()
        .checked_add(Duration::from_secs(10))
        .expect("watch activation deadline should be representable");
    loop {
        if database.is_file()
            && Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .and_then(|connection| {
                    connection.query_row(
                        "SELECT count(*) FROM workspaces
                     WHERE active_generation_id IS NOT NULL",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                })
                .is_ok_and(|active| active == 1)
        {
            return;
        }
        if Instant::now() >= timeout {
            cancelled.store(true, Ordering::Release);
            panic!("watch did not publish its startup generation in time");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn cooperative_live_watch_cancellation_is_bounded_on_every_platform() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    let cancelled = Arc::new(AtomicBool::new(false));

    let report = std::thread::scope(|scope| {
        let worker_cancelled = Arc::clone(&cancelled);
        let worker = scope.spawn(move || {
            crate::watch_local_repository(crate::LocalWatchRequest::new(request), worker_cancelled)
                .expect("cooperatively cancelled watch should return a report")
        });
        wait_for_watch_activation(&database, &cancelled);
        let cancellation_started = Instant::now();
        cancelled.store(true, Ordering::Release);
        let report = worker.join().expect("watch worker should not panic");
        assert!(
            cancellation_started.elapsed() < Duration::from_secs(5),
            "cooperative watch shutdown exceeded its test bound"
        );
        report
    });

    assert_eq!(report.exit(), crate::LocalWatchExit::Cancelled);
    assert!(report.state_counters().reconciliations_started() >= 1);
    let (generation, paths) = watch_search_paths(&database, "Widget");
    assert_eq!(generation, 1);
    assert!(!paths.is_empty());
    if let Some(active) = report.last_index() {
        assert_eq!(active.generation().get(), generation);
        assert_eq!(
            report.last_reconciliation(),
            Some(crate::LocalWatchReconciliation::Published)
        );
    }
}
