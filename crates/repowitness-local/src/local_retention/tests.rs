use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use repowitness_application::resolve_configuration;

use super::*;
use crate::{
    DEFAULT_RETAINED_GENERATIONS_PER_SOURCE_SLOT, MAX_RETENTION_GENERATION_PINS,
    MAX_RETENTION_VIEW_PINS, OwnedSqliteIndex, SqliteStoreError, sqlite::SqliteMutationLease,
    sqlite::open_index_writer,
};

mod process_recovery;

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let physical_temporary_directory = std::fs::canonicalize(std::env::temp_dir())
            .expect("canonicalize temporary directory for no-follow fixture");
        let path = physical_temporary_directory.join(format!(
            "repowitness-local-retention-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("create temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn pins_and_timeouts_are_bounded_and_debug_is_redacted() {
    assert_eq!(
        LocalRetentionPins::try_new(vec![0], Vec::new(), Vec::new()),
        Err(LocalRetentionRequestError::InvalidGenerationPin)
    );
    assert_eq!(
        LocalRetentionPins::try_new(Vec::new(), Vec::new(), vec![-1]),
        Err(LocalRetentionRequestError::InvalidWorkspaceViewPin)
    );
    assert_eq!(
        LocalRetentionPins::try_new(vec![1, 1], Vec::new(), Vec::new()),
        Err(LocalRetentionRequestError::DuplicatePin)
    );
    assert_eq!(
        LocalRetentionPins::try_new(
            vec![1; MAX_RETENTION_GENERATION_PINS + 1],
            Vec::new(),
            Vec::new()
        ),
        Err(LocalRetentionRequestError::TooManyPins)
    );
    assert_eq!(
        LocalRetentionPins::try_new(Vec::new(), Vec::new(), vec![1; MAX_RETENTION_VIEW_PINS + 1]),
        Err(LocalRetentionRequestError::TooManyPins)
    );

    let configuration = resolve_configuration(&[]).expect("configuration");
    for timeout in [
        Duration::ZERO,
        MAX_LOCAL_RETENTION_TIMEOUT + Duration::from_nanos(1),
    ] {
        assert_eq!(
            LocalRetentionPlanRequest::try_new(
                Path::new("../private/index.db"),
                1,
                &configuration,
                LocalRetentionPins::default(),
                Arc::new(AtomicBool::new(false)),
                timeout,
            )
            .expect_err("invalid timeout"),
            LocalRetentionRequestError::InvalidTimeout
        );
    }
    let request = LocalRetentionPlanRequest::try_new(
        Path::new("../private/index.db"),
        1,
        &configuration,
        LocalRetentionPins::try_new(vec![3], vec![4], vec![5]).expect("pins"),
        Arc::new(AtomicBool::new(false)),
        DEFAULT_LOCAL_RETENTION_TIMEOUT,
    )
    .expect("request");
    let debug = format!("{request:?}");
    assert!(debug.contains("generation_pin_count: 2"));
    assert!(debug.contains("workspace_view_pin_count: 1"));
    assert!(!debug.contains("private"));
    assert!(!debug.contains("index.db"));
}

#[test]
fn preexisting_cancellation_wins_before_filesystem_access() {
    let directory = TempDirectory::new();
    let database = directory.path().join("missing/index.db");
    let configuration = resolve_configuration(&[]).expect("configuration");
    let request = LocalRetentionPlanRequest::try_new(
        &database,
        1,
        &configuration,
        LocalRetentionPins::default(),
        Arc::new(AtomicBool::new(true)),
        DEFAULT_LOCAL_RETENTION_TIMEOUT,
    )
    .expect("request");

    let error = plan_local_retention(request).expect_err("cancelled");
    assert_eq!(error.kind(), LocalRetentionErrorKind::Cancelled);
    assert!(!directory.path().join("missing").exists());
    assert_eq!(error.to_string(), "local retention maintenance failed");
    assert!(!format!("{error:?}").contains("missing"));
}

#[test]
fn storage_budget_and_unknown_apply_errors_remain_categorical() {
    let blocked = super::execution::map_store_error(SqliteStoreError::RetentionLimitExceeded);
    assert_eq!(blocked.kind(), LocalRetentionErrorKind::BlockedByLimit);
    for source in [
        SqliteStoreError::MutationOutcomeUnknown,
        SqliteStoreError::WorkerUnavailable,
    ] {
        let unknown = super::execution::map_apply_store_error(source);
        assert_eq!(unknown.kind(), LocalRetentionErrorKind::OutcomeUnknown);
        assert_eq!(
            unknown.reconciliation_guidance(),
            Some(
                "look up retention_collection_audit by the exact policy and plan digests; only when no committed receipt exists, run a fresh read-only retention plan and compare current roots with the expected plan before retrying apply"
            )
        );
        assert_eq!(
            unknown.to_string(),
            "local retention apply outcome could not be determined"
        );
    }
}

#[test]
fn empty_database_plan_is_deterministic_across_owner_restarts() {
    let directory = TempDirectory::new();
    let database = initialize_database(directory.path());
    let configuration = resolve_configuration(&[]).expect("configuration");

    let first = plan(&database, &configuration, Duration::from_secs(2)).expect("first plan");
    let second = plan(&database, &configuration, Duration::from_secs(2)).expect("second plan");
    assert_eq!(first, second);
    assert_eq!(first.candidate_count(), 0);
    assert_eq!(first.estimated_rows(), 0);
    assert_eq!(first.estimated_bytes(), 0);
    assert_eq!(first.root_count(), 0);
    assert_eq!(first.unresolved_count(), 0);
    assert!(!first.unresolved_truncated());
    assert_eq!(first.logical_work_rows(), 1);
    assert!(!first.more_work());
    assert_eq!(
        first.policy().retained_generations_per_source_slot(),
        DEFAULT_RETAINED_GENERATIONS_PER_SOURCE_SLOT
    );
    assert_eq!(first.policy().max_generation_candidates(), 64);
    assert_eq!(first.policy().max_rows(), 1_000_000);
    assert_eq!(first.policy().max_bytes(), 512 * 1024 * 1024);
    assert_eq!(first.policy().generation_pin_count(), 0);
    assert_eq!(first.policy().workspace_view_pin_count(), 0);
    let debug = format!("{first:?}");
    assert!(!debug.contains(database.to_string_lossy().as_ref()));
}

#[test]
fn plan_is_byte_ledger_freelist_and_lease_invariant() {
    let directory = TempDirectory::new();
    let database = initialize_database(directory.path());
    let lease = mutation_lease_path(&database);
    std::fs::remove_file(&lease).expect("remove initialization lease");
    let configuration = resolve_configuration(&[]).expect("configuration");
    let before = database_fingerprint(&database);

    let report = plan(&database, &configuration, Duration::from_secs(2)).expect("read-only plan");

    assert_eq!(report.candidate_count(), 0);
    assert_eq!(database_fingerprint(&database), before);
    assert!(
        !lease.exists(),
        "planning must not create the mutation lease"
    );
}

#[test]
fn apply_revalidates_and_rejects_a_stale_digest() {
    let directory = TempDirectory::new();
    let database = initialize_database(directory.path());
    let configuration = resolve_configuration(&[]).expect("configuration");
    let request = LocalRetentionApplyRequest::try_new(
        &database,
        2,
        &configuration,
        LocalRetentionPins::default(),
        [0x5a; 32],
        Arc::new(AtomicBool::new(false)),
        Duration::from_secs(2),
    )
    .expect("request");

    let error = apply_local_retention(request).expect_err("stale plan");
    assert_eq!(error.kind(), LocalRetentionErrorKind::PlanStale);
    let after = plan(&database, &configuration, Duration::from_secs(2)).expect("plan after");
    assert_eq!(after.candidate_count(), 0);
}

#[test]
fn exact_plan_apply_returns_only_aggregate_counts() {
    let directory = TempDirectory::new();
    let database = initialize_database(directory.path());
    let configuration = resolve_configuration(&[]).expect("configuration");
    let plan = plan(&database, &configuration, Duration::from_secs(2)).expect("plan");
    let request = LocalRetentionApplyRequest::try_new(
        &database,
        2,
        &configuration,
        LocalRetentionPins::default(),
        plan.plan_digest(),
        Arc::new(AtomicBool::new(false)),
        Duration::from_secs(2),
    )
    .expect("request");

    let report = apply_local_retention(request).expect("apply");
    assert_eq!(report.configuration_digest(), plan.configuration_digest());
    assert_eq!(report.policy_digest(), plan.policy_digest());
    assert_eq!(report.plan_digest(), plan.plan_digest());
    assert_eq!(report.generation_count(), 0);
    assert_eq!(report.workspace_view_count(), 0);
    assert_eq!(report.source_slot_receipt_count(), 0);
    assert_eq!(report.snapshot_count(), 0);
    assert_eq!(report.artifact_count(), 0);
    assert_eq!(report.deleted_rows(), 0);
    assert_eq!(report.estimated_deleted_bytes(), 0);
    assert!(!report.more_work());
    assert!(report.collection_id() > 0);
    assert!(report.shutdown_complete());
    assert!(report.database_identity_confirmed());
    assert_eq!(report.warning_count(), 0);
    let debug = format!("{report:?}");
    assert!(!debug.contains(database.to_string_lossy().as_ref()));

    let replay = LocalRetentionApplyRequest::try_new(
        &database,
        3,
        &configuration,
        LocalRetentionPins::default(),
        plan.plan_digest(),
        Arc::new(AtomicBool::new(false)),
        Duration::from_secs(2),
    )
    .expect("replay request");
    assert_eq!(
        apply_local_retention(replay).expect("idempotent no-op replay"),
        report
    );
}

#[test]
fn committed_apply_reports_shutdown_deadline_as_a_warning_not_an_error() {
    let directory = TempDirectory::new();
    let database = initialize_database(directory.path());
    let configuration = resolve_configuration(&[]).expect("configuration");
    let plan = plan(&database, &configuration, Duration::from_secs(2)).expect("plan");
    let request = LocalRetentionApplyRequest::try_new(
        &database,
        2,
        &configuration,
        LocalRetentionPins::default(),
        plan.plan_digest(),
        Arc::new(AtomicBool::new(false)),
        Duration::from_secs(2),
    )
    .expect("request");

    let report =
        super::execution::apply_local_retention_with_hooks(request, || {}, |_| Instant::now())
            .expect("a committed apply must remain an outcome");
    assert!(!report.shutdown_complete());
    assert!(report.database_identity_confirmed());
    assert_eq!(report.warning_count(), 1);

    let replay = LocalRetentionApplyRequest::try_new(
        &database,
        3,
        &configuration,
        LocalRetentionPins::default(),
        plan.plan_digest(),
        Arc::new(AtomicBool::new(false)),
        Duration::from_secs(2),
    )
    .expect("replay request");
    let replay = apply_local_retention(replay).expect("committed operation can be replayed");
    assert_eq!(replay.collection_id(), report.collection_id());
}

#[cfg(unix)]
#[test]
fn committed_apply_reports_database_replacement_at_the_final_fence() {
    let directory = TempDirectory::new();
    let database = initialize_database(directory.path());
    let moved = directory.path().join("opened.db");
    let configuration = resolve_configuration(&[]).expect("configuration");
    let plan = plan(&database, &configuration, Duration::from_secs(2)).expect("plan");
    let request = LocalRetentionApplyRequest::try_new(
        &database,
        2,
        &configuration,
        LocalRetentionPins::default(),
        plan.plan_digest(),
        Arc::new(AtomicBool::new(false)),
        Duration::from_secs(2),
    )
    .expect("request");

    let report = super::execution::apply_local_retention_with_hooks(
        request,
        || {
            std::fs::rename(&database, &moved).expect("move writer-opened database");
            std::fs::copy(&moved, &database).expect("replace database pathname");
        },
        |deadline| deadline,
    )
    .expect("committed apply must return a warning outcome");
    assert!(!report.database_identity_confirmed());
    assert_eq!(report.warning_count(), 1);
}

#[test]
fn lock_wait_observes_deadline_and_releases_cleanly_for_restart() {
    let directory = TempDirectory::new();
    let database = initialize_database(directory.path());
    let configuration = resolve_configuration(&[]).expect("configuration");
    let initial_plan = plan(&database, &configuration, Duration::from_secs(2)).expect("plan");
    let deadline = Instant::now() + Duration::from_secs(2);
    let (holder, _) = OwnedSqliteIndex::start(&database, 2, deadline).expect("lease holder");

    plan(&database, &configuration, Duration::from_secs(1))
        .expect("read-only plan can coexist with writer lease");
    let request = LocalRetentionApplyRequest::try_new(
        &database,
        3,
        &configuration,
        LocalRetentionPins::default(),
        initial_plan.plan_digest(),
        Arc::new(AtomicBool::new(false)),
        Duration::from_millis(25),
    )
    .expect("apply request");
    let error = apply_local_retention(request).expect_err("deadline while lease held");
    assert_eq!(error.kind(), LocalRetentionErrorKind::DeadlineExceeded);
    holder
        .shutdown(Instant::now() + Duration::from_secs(2))
        .expect("release holder");
    plan(&database, &configuration, Duration::from_secs(2)).expect("restart after release");
}

#[test]
fn lock_wait_observes_external_cancellation() {
    let directory = TempDirectory::new();
    let database = initialize_database(directory.path());
    let configuration = resolve_configuration(&[]).expect("configuration");
    let plan = plan(&database, &configuration, Duration::from_secs(2)).expect("plan");
    let deadline = Instant::now() + Duration::from_secs(2);
    let (holder, _) = OwnedSqliteIndex::start(&database, 2, deadline).expect("lease holder");
    let cancellation = Arc::new(AtomicBool::new(false));
    let task_cancellation = Arc::clone(&cancellation);
    let task_database = database.clone();
    let plan_digest = plan.plan_digest();
    let handle = std::thread::spawn(move || {
        let configuration = resolve_configuration(&[]).expect("configuration");
        let request = LocalRetentionApplyRequest::try_new(
            &task_database,
            3,
            &configuration,
            LocalRetentionPins::default(),
            plan_digest,
            task_cancellation,
            Duration::from_secs(1),
        )
        .expect("request");
        apply_local_retention(request)
    });
    std::thread::sleep(Duration::from_millis(30));
    cancellation.store(true, Ordering::Release);

    let error = handle
        .join()
        .expect("retention thread")
        .expect_err("cancelled lock wait");
    assert_eq!(error.kind(), LocalRetentionErrorKind::Cancelled);
    holder
        .shutdown(Instant::now() + Duration::from_secs(2))
        .expect("release holder");
}

#[cfg(unix)]
#[test]
fn symlink_aliases_fail_before_creating_alias_leases() {
    use std::os::unix::fs::symlink;

    let directory = TempDirectory::new();
    let database = initialize_database(directory.path());
    let symlink_path = directory.path().join("alias.db");
    symlink(&database, &symlink_path).expect("symlink");
    let configuration = resolve_configuration(&[]).expect("configuration");
    let error =
        plan(&symlink_path, &configuration, Duration::from_secs(2)).expect_err("symlink rejected");
    assert_eq!(error.kind(), LocalRetentionErrorKind::DatabaseUnavailable);
    assert!(!alias_lease_path(&symlink_path).exists());
}

#[cfg(any(unix, windows))]
#[test]
fn hardlink_aliases_fail_before_creating_alias_leases() {
    let directory = TempDirectory::new();
    let database = initialize_database(directory.path());
    let hardlink_path = directory.path().join("alias.db");
    std::fs::hard_link(&database, &hardlink_path).expect("hard link");
    let configuration = resolve_configuration(&[]).expect("configuration");
    let error = plan(&hardlink_path, &configuration, Duration::from_secs(2))
        .expect_err("hard link rejected");
    assert_eq!(error.kind(), LocalRetentionErrorKind::DatabaseUnavailable);
    assert!(!alias_lease_path(&hardlink_path).exists());
}

fn initialize_database(directory: &Path) -> PathBuf {
    let database = directory.join("index.db");
    let deadline = Instant::now() + Duration::from_secs(2);
    let lease = SqliteMutationLease::acquire(&database, deadline).expect("initialize lease");
    let connection = open_index_writer(&database, 1).expect("initialize database");
    drop(connection);
    drop(lease);
    database
}

#[derive(Debug, Eq, PartialEq)]
struct DatabaseFingerprint {
    bytes: Vec<u8>,
    migration_ledger: Vec<(i64, String, Vec<u8>, i64)>,
    user_version: i64,
    freelist_count: i64,
    retention_audit_count: i64,
}

fn database_fingerprint(database: &Path) -> DatabaseFingerprint {
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
        | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
        | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let connection =
        rusqlite::Connection::open_with_flags(database, flags).expect("open read-only database");
    let mut statement = connection
        .prepare(
            "SELECT version, name, checksum, applied_at_unix_ms
             FROM schema_migrations ORDER BY version",
        )
        .expect("prepare migration ledger");
    let migration_ledger = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .expect("query migration ledger")
        .collect::<Result<Vec<_>, _>>()
        .expect("decode migration ledger");
    drop(statement);
    let user_version = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read user version");
    let freelist_count = connection
        .pragma_query_value(None, "freelist_count", |row| row.get(0))
        .expect("read freelist");
    let retention_audit_count = connection
        .query_row(
            "SELECT count(*) FROM retention_collection_audit",
            [],
            |row| row.get(0),
        )
        .expect("read retention audit count");
    drop(connection);
    DatabaseFingerprint {
        bytes: std::fs::read(database).expect("read database bytes"),
        migration_ledger,
        user_version,
        freelist_count,
        retention_audit_count,
    }
}

fn plan(
    database: &Path,
    configuration: &repowitness_application::ResolvedConfiguration,
    timeout: Duration,
) -> Result<LocalRetentionPlanReport, LocalRetentionError> {
    let request = LocalRetentionPlanRequest::try_new(
        database,
        2,
        configuration,
        LocalRetentionPins::default(),
        Arc::new(AtomicBool::new(false)),
        timeout,
    )
    .expect("request");
    plan_local_retention(request)
}

fn mutation_lease_path(database: &Path) -> PathBuf {
    let mut value = database.as_os_str().to_os_string();
    value.push(".repowitness-mutation.lock");
    PathBuf::from(value)
}

fn alias_lease_path(database: &Path) -> PathBuf {
    mutation_lease_path(database)
}
