#[cfg(unix)]
#[test]
fn writer_opened_identity_rejects_valid_database_replacement() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    let initial = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("initial database should activate");
    let replacement = directory.0.join("post-open-replacement.sqlite3");
    fs::copy(&database, &replacement).expect("valid replacement should be copied");
    let displaced = directory.0.join("post-open-displaced.sqlite3");

    let error = super::index_local_rust_repository_with_control_hooks(
        request,
        Arc::new(AtomicBool::new(false)),
        |phase| {
            if phase == super::LocalIndexPhase::WriterStarted {
                fs::rename(&database, &displaced).expect("opened database should be displaced");
                fs::rename(&replacement, &database)
                    .expect("valid replacement should occupy the database path");
            }
        },
        |_, deadline| deadline,
    )
    .expect_err("writer-opened identity must reject post-open path replacement");

    assert!(matches!(
        error,
        LocalIndexError::DatabaseChangedDuringIndexing
    ));
    assert_prior_generation_readable(&database, initial.generation(), "NotPresent");
}

#[cfg(windows)]
#[test]
fn opened_database_cannot_be_replaced_while_the_writer_holds_it() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    let initial = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("initial database should activate");
    let replacement = directory.0.join("post-open-replacement.sqlite3");
    fs::copy(&database, &replacement).expect("valid replacement should be copied");
    let displaced = directory.0.join("post-open-displaced.sqlite3");
    let mut replacement_blocked = false;

    let report = super::index_local_rust_repository_with_control_hooks(
        request,
        Arc::new(AtomicBool::new(false)),
        |phase| {
            if phase == super::LocalIndexPhase::WriterStarted {
                replacement_blocked = fs::rename(&database, &displaced).is_err();
            }
        },
        |_, deadline| deadline,
    )
    .expect("the operating system must retain the authoritative opened database");

    assert!(replacement_blocked, "Windows must block replacement of the open database");
    assert_ne!(report.generation(), initial.generation());
    assert_prior_generation_readable(&database, report.generation(), "NotPresent");
}

#[test]
fn post_commit_checkpoint_and_shutdown_failures_do_not_hide_activation() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    let initial = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("initial generation should activate");

    for (fault, symbol) in [
        (
            super::post_commit::PostCommitMaintenancePhase::Checkpoint,
            "CheckpointCommitted",
        ),
        (
            super::post_commit::PostCommitMaintenancePhase::Shutdown,
            "ShutdownCommitted",
        ),
    ] {
        fs::write(
            repository.join("src/lib.rs"),
            format!("pub struct {symbol};\nimpl {symbol} {{ pub fn run() {{}} }}\n"),
        )
        .expect("changed source should be written");
        let report = super::index_local_rust_repository_with_control_hooks(
            request,
            Arc::new(AtomicBool::new(false)),
            |_| {},
            |phase, deadline| {
                if phase == fault {
                    Instant::now()
                } else {
                    deadline
                }
            },
        )
        .expect("post-commit maintenance failure must return the committed outcome");
        assert_ne!(report.generation(), initial.generation());

        let repository_identity = RepositoryIdentityTextV1::decode(REPOSITORY_ID)
            .expect("fixture identity should decode");
        let reader = OwnedSqliteReader::start(&database, deadline())
            .expect("committed generation should remain readable");
        let result = reader
            .search(
                repository_identity,
                symbol,
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("committed symbol should be searchable");
        assert_eq!(result.generation(), report.generation());
        assert!(!result.hits().is_empty());
        reader
            .shutdown(deadline())
            .expect("reader should shut down");
    }
}
