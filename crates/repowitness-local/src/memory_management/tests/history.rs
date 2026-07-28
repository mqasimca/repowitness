use super::*;

#[test]
fn reachable_history_imports_observations_only_and_retries_idempotently() {
    let repository = GitFixture::new();
    repository.commit_memory(MEMORY_YAML, "memory one");
    let second = String::from_utf8(MEMORY_YAML.to_vec()).expect("fixture YAML should be UTF-8");
    let second = second.replacen("display_revision: 1", "display_revision: 2", 1);
    repository.commit_memory(second.as_bytes(), "memory presentation two");
    let outside = TempDirectory::new("history-database");
    let database = outside.path().join("index.sqlite3");
    let request = LocalMemoryHistoryImportRequest::new(
        repository.path(),
        &database,
        REPOSITORY_ID,
        "trusted-history-observer",
        123,
        1_722_000_000_001,
    );

    let first = import_local_memory_history(request, Arc::new(AtomicBool::new(false)))
        .expect("reachable history should import");
    assert_eq!(first.commits_inspected(), 2);
    assert_eq!(first.records_inspected(), 2);
    assert_eq!(first.imported_versions(), 1);
    assert_eq!(first.appended_observations(), 2);
    assert!(first.total_record_bytes() > 0);
    assert_eq!(first.git_processes(), 8);
    assert!(first.history_complete());

    let repeated = import_local_memory_history(request, Arc::new(AtomicBool::new(false)))
        .expect("history retry should be idempotent");
    assert_eq!(repeated.commits_inspected(), 2);
    assert_eq!(repeated.records_inspected(), 2);
    assert_eq!(repeated.imported_versions(), 0);
    assert_eq!(repeated.appended_observations(), 0);
    assert!(repeated.history_complete());

    let connection = Connection::open(database).expect("database should open");
    let counts: (i64, i64, i64) = connection
        .query_row(
            "SELECT
                 (SELECT count(*) FROM memory_versions),
                 count(*) FILTER (WHERE operation = 'observed'),
                 count(*) FILTER (WHERE operation = 'locally_approved')
             FROM memory_audit",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("history counts should be readable");
    assert_eq!(counts, (1, 2, 0));
}

#[test]
fn rewritten_and_pruned_history_retains_prior_observations_without_approval() {
    let repository = GitFixture::new();
    repository.commit_memory(MEMORY_YAML, "observed memory");
    let old_head = git_output(repository.path(), &["rev-parse", "HEAD"]);
    let old_branch = git_output(repository.path(), &["branch", "--show-current"]);
    let outside = TempDirectory::new("rewritten-history-database");
    let database = outside.path().join("index.sqlite3");
    let request = LocalMemoryHistoryImportRequest::new(
        repository.path(),
        &database,
        REPOSITORY_ID,
        "trusted-history-observer",
        123,
        1_722_000_000_010,
    );
    let first = import_local_memory_history(request, Arc::new(AtomicBool::new(false)))
        .expect("initial reachable history should import");
    assert_eq!(first.imported_versions(), 1);
    assert_eq!(first.appended_observations(), 1);

    rewrite_history(&repository, &old_branch);
    git(
        repository.path(),
        &["reflog", "expire", "--expire=now", "--all"],
    );
    git(repository.path(), &["gc", "--prune=now"]);
    assert!(
        !git_succeeds(
            repository.path(),
            &["cat-file", "-e", &format!("{old_head}^{{commit}}")]
        ),
        "the original observed commit should no longer be available"
    );

    let second = import_local_memory_history(request, Arc::new(AtomicBool::new(false)))
        .expect("rewritten reachable history should import");
    assert_eq!(second.imported_versions(), 1);
    assert_eq!(second.appended_observations(), 1);
    assert!(second.history_complete());

    let connection = Connection::open(database).expect("database should open");
    let counts: (i64, i64, i64) = connection
        .query_row(
            "SELECT
                 (SELECT count(*) FROM memory_versions),
                 count(*) FILTER (WHERE operation = 'observed'),
                 count(*) FILTER (WHERE operation = 'locally_approved')
             FROM memory_audit",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("retained history counts should be readable");
    assert_eq!(counts, (2, 2, 0));
}

fn rewrite_history(repository: &GitFixture, old_branch: &str) {
    git(
        repository.path(),
        &["checkout", "--quiet", "--orphan", "rewritten"],
    );
    git(repository.path(), &["rm", "--quiet", "-r", "-f", "."]);
    fs::write(repository.path().join("lib.rs"), b"pub fn publish() {}\n")
        .expect("rewritten source should be written");
    let rewritten = String::from_utf8(MEMORY_YAML.to_vec())
        .expect("fixture YAML should be UTF-8")
        .replacen(
            "Readers must never observe a partially staged generation.",
            "Rewritten history retains separately observed memory.",
            1,
        );
    repository.write_memory(rewritten.as_bytes());
    git(repository.path(), &["add", "."]);
    git(
        repository.path(),
        &["commit", "--quiet", "-m", "rewritten root"],
    );
    git(repository.path(), &["branch", "--quiet", "-D", old_branch]);
    git(repository.path(), &["branch", "-m", old_branch]);
}

fn git_output(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .expect("Git should start");
    assert!(
        output.status.success(),
        "Git fixture command should succeed"
    );
    String::from_utf8(output.stdout)
        .expect("Git fixture output should be UTF-8")
        .trim()
        .to_owned()
}

fn git_succeeds(root: &Path, arguments: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .expect("Git should start")
        .status
        .success()
}

#[test]
fn history_commit_bounds_and_shallow_clones_report_partial_coverage() {
    let repository = GitFixture::new();
    repository.commit_memory(MEMORY_YAML, "memory one");
    let second = String::from_utf8(MEMORY_YAML.to_vec())
        .expect("fixture YAML should be UTF-8")
        .replacen("display_revision: 1", "display_revision: 2", 1);
    repository.commit_memory(second.as_bytes(), "memory presentation two");

    let bounded_outside = TempDirectory::new("bounded-history-database");
    let bounded_database = bounded_outside.path().join("index.sqlite3");
    let bounded_limits = LocalMemoryHistoryImportLimits::try_new(
        Duration::from_secs(30),
        Duration::from_secs(5),
        1,
        4_096,
        64 * 1024 * 1024,
        16 * 1024 * 1024,
    )
    .expect("history limits should be valid");
    let bounded = import_local_memory_history(
        LocalMemoryHistoryImportRequest::new(
            repository.path(),
            &bounded_database,
            REPOSITORY_ID,
            "trusted-history-observer",
            123,
            1_722_000_000_001,
        )
        .with_limits(bounded_limits),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("the admitted newest history should import");
    assert_eq!(bounded.commits_inspected(), 1);
    assert_eq!(bounded.records_inspected(), 1);
    assert!(!bounded.history_complete());

    let shallow_parent = TempDirectory::new("shallow-history");
    let shallow = shallow_parent.path().join("checkout");
    let source = format!("file://{}", repository.path().display());
    assert!(
        Command::new("git")
            .args(["clone", "--quiet", "--depth", "1"])
            .arg(source)
            .arg(&shallow)
            .status()
            .expect("shallow clone should start")
            .success(),
        "shallow clone should succeed"
    );
    let shallow_outside = TempDirectory::new("shallow-history-database");
    let shallow_database = shallow_outside.path().join("index.sqlite3");
    let shallow_report = import_local_memory_history(
        LocalMemoryHistoryImportRequest::new(
            &shallow,
            &shallow_database,
            REPOSITORY_ID,
            "trusted-history-observer",
            123,
            1_722_000_000_002,
        ),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("reachable shallow history should import conservatively");
    assert_eq!(shallow_report.commits_inspected(), 1);
    assert_eq!(shallow_report.records_inspected(), 1);
    assert!(!shallow_report.history_complete());
}

#[test]
fn malformed_and_cancelled_history_fail_before_persisting_observations() {
    let malformed_repository = GitFixture::new();
    malformed_repository.commit_memory(b"not: [valid\n", "malformed memory");
    let malformed_outside = TempDirectory::new("malformed-history-database");
    let malformed_database = malformed_outside.path().join("index.sqlite3");
    assert_eq!(
        import_local_memory_history(
            LocalMemoryHistoryImportRequest::new(
                malformed_repository.path(),
                &malformed_database,
                REPOSITORY_ID,
                "trusted-history-observer",
                123,
                1_722_000_000_003,
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .expect_err("malformed history should fail closed"),
        LocalMemoryManageError::HistoryUnavailable
    );
    assert!(!malformed_database.exists());

    let cancelled_repository = GitFixture::new();
    cancelled_repository.commit_memory(MEMORY_YAML, "memory");
    let cancelled_outside = TempDirectory::new("cancelled-history-database");
    let cancelled_database = cancelled_outside.path().join("index.sqlite3");
    assert_eq!(
        import_local_memory_history(
            LocalMemoryHistoryImportRequest::new(
                cancelled_repository.path(),
                &cancelled_database,
                REPOSITORY_ID,
                "trusted-history-observer",
                123,
                1_722_000_000_004,
            ),
            Arc::new(AtomicBool::new(true)),
        )
        .expect_err("cancelled history should stop before Git or SQLite"),
        LocalMemoryManageError::Cancelled
    );
    assert!(!cancelled_database.exists());
}
