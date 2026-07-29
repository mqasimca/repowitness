fn gc_plan(database: &Path) -> Output {
    repowitness_os([
        OsStr::new("gc"),
        OsStr::new("plan"),
        OsStr::new("--database"),
        database.as_os_str(),
    ])
}

fn gc_apply(database: &Path, plan_digest: &str) -> Output {
    repowitness_os([
        OsStr::new("gc"),
        OsStr::new("apply"),
        OsStr::new("--database"),
        database.as_os_str(),
        OsStr::new("--plan-digest"),
        OsStr::new(plan_digest),
    ])
}

fn assert_index_succeeds(repository: &Path, database: &Path) {
    let output = index(repository, database, REPOSITORY_ID);
    assert!(
        output.status.success(),
        "index failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
}

fn parse_report_count(report: &str, key: &str) -> u64 {
    report_value(report, key)
        .parse::<u64>()
        .expect("aggregate report count should be an unsigned integer")
}

fn gc_plan_report(database: &Path) -> String {
    let output = gc_plan(database);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout).expect("GC plan must be UTF-8")
}

fn gc_apply_report(database: &Path, digest: &str) -> String {
    let output = gc_apply(database, digest);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout).expect("GC apply must be UTF-8")
}

fn assert_initial_plan(report: &str, database: &Path) {
    assert!(report.starts_with("status=ok\noperation=gc_plan\nschema_version=1\n"));
    assert_eq!(report_value(report, "retention_profile"), "1");
    assert_eq!(report_value(report, "candidate_count"), "1");
    assert_eq!(report_value(report, "unresolved_candidate_count"), "0");
    assert_eq!(report_value(report, "unresolved_candidates_truncated"), "false");
    assert_eq!(report_value(report, "more_work"), "false");
    assert!(parse_report_count(report, "estimated_rows") > 0);
    assert!(parse_report_count(report, "estimated_bytes") > 0);
    assert!(parse_report_count(report, "root_count") > 0);
    assert!(parse_report_count(report, "logical_work_rows") <= parse_report_count(report, "max_rows"));
    assert!(!report.contains(REPOSITORY_ID));
    assert!(!report.contains(database.to_string_lossy().as_ref()));
}

fn assert_apply_report(report: &str, digest: &str, database: &Path) {
    assert!(report.starts_with("status=ok\noperation=gc_apply\nschema_version=1\n"));
    assert_eq!(report_value(report, "retention_plan_sha256"), digest);
    assert_eq!(report_value(report, "deleted_generations"), "1");
    assert!(parse_report_count(report, "deleted_rows") > 0);
    assert_eq!(report_value(report, "maintenance_shutdown"), "complete");
    assert_eq!(report_value(report, "database_identity_fence"), "confirmed");
    assert_eq!(report_value(report, "warning_count"), "0");
    assert!(!report.contains(REPOSITORY_ID));
    assert!(!report.contains(database.to_string_lossy().as_ref()));
}

fn assert_stale_apply_is_rejected(database: &Path, digest: &str) {
    let rejected = gc_apply(database, digest);
    assert_eq!(rejected.status.code(), Some(70));
    assert!(rejected.stdout.is_empty());
    assert_eq!(rejected.stderr, b"error: gc operation failed\n");
}

fn assert_active_generation_remains_searchable(database: &Path) {
    let active = search(database, REPOSITORY_ID, "Widget", "1");
    assert!(active.status.success());
    assert!(active.stderr.is_empty());
    let active = String::from_utf8(active.stdout).expect("search report must be UTF-8");
    assert_eq!(report_value(&active, "generation"), "6");
    assert_eq!(report_value(&active, "match_0_name"), "Widget");
}

#[test]
fn gc_plan_apply_replay_and_stale_rejection_preserve_the_active_generation() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    for _ in 0..4 {
        assert_index_succeeds(&repository, &database);
    }

    let planned = gc_plan_report(&database);
    assert_initial_plan(&planned, &database);
    assert_eq!(gc_plan_report(&database), planned);

    let digest = report_value(&planned, "retention_plan_sha256").to_owned();
    let applied = gc_apply_report(&database, &digest);
    assert_apply_report(&applied, &digest, &database);
    let replayed = gc_apply_report(&database, &digest);
    assert_eq!(
        report_value(&replayed, "collection_id"),
        report_value(&applied, "collection_id")
    );
    assert_eq!(
        report_value(&replayed, "deleted_rows"),
        report_value(&applied, "deleted_rows")
    );

    let empty = gc_plan_report(&database);
    assert_eq!(report_value(&empty, "candidate_count"), "0");

    assert_index_succeeds(&repository, &database);
    let stale_plan = gc_plan_report(&database);
    assert_eq!(report_value(&stale_plan, "candidate_count"), "1");
    let stale_digest = report_value(&stale_plan, "retention_plan_sha256").to_owned();

    assert_index_succeeds(&repository, &database);
    assert_stale_apply_is_rejected(&database, &stale_digest);

    let after_rejection = gc_plan_report(&database);
    assert_eq!(report_value(&after_rejection, "candidate_count"), "2");
    assert_active_generation_remains_searchable(&database);
}

#[test]
fn gc_help_documents_explicit_digest_bound_apply_without_creating_state() {
    let output = repowitness(&["gc", "--help"]);

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).expect("GC help must be UTF-8");
    assert!(help.contains("gc plan --database <path>"));
    assert!(help.contains("gc apply --database <path> --plan-digest"));
    assert!(help.contains("rejects stale"));
}
