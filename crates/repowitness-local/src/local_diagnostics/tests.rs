use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use crate::{LocalIndexRequest, index_local_repository};

use super::*;

const REPOSITORY_ID: &str = concat!(
    "rwi1:h:",
    "0202020202020202020202020202020202020202020202020202020202020202"
);
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repowitness-local-diagnostics-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture directory");
        Self(path)
    }

    fn repository(&self) -> PathBuf {
        self.0.join("repository")
    }

    fn database(&self) -> PathBuf {
        self.0.join("index.sqlite3")
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture_repository(directory: &TempDirectory) -> PathBuf {
    let repository = directory.repository();
    fs::create_dir_all(repository.join("src")).expect("source directory");
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(&repository)
            .status()
            .expect("git init")
            .success()
    );
    fs::write(
        repository.join("src/lib.rs"),
        "pub fn diagnostic_fixture() {}\n",
    )
    .expect("source fixture");
    assert!(
        Command::new("git")
            .current_dir(&repository)
            .args(["add", "--", "src/lib.rs"])
            .status()
            .expect("git add")
            .success()
    );
    repository
}

#[test]
fn request_debug_is_redacted_and_invalid_control_fails_before_io() {
    let request =
        LocalRepositoryDiagnosticsRequest::new(Path::new("/private/index.sqlite3"), REPOSITORY_ID)
            .with_deadline(Duration::from_secs(1));
    let debug = format!("{request:?}");
    assert!(!debug.contains("/private"));
    assert!(!debug.contains(REPOSITORY_ID));

    let missing = Path::new("/missing/private-diagnostics.sqlite3");
    let invalid = diagnose_local_repository(
        LocalRepositoryDiagnosticsRequest::new(missing, "invalid"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("identity validation");
    assert!(matches!(
        invalid,
        LocalRepositoryDiagnosticsError::RepositoryIdentity(_)
    ));

    let cancelled = diagnose_local_repository(
        LocalRepositoryDiagnosticsRequest::new(missing, REPOSITORY_ID),
        Arc::new(AtomicBool::new(true)),
    )
    .expect_err("cancellation");
    assert!(matches!(
        cancelled,
        LocalRepositoryDiagnosticsError::Cancelled
    ));

    let deadline = diagnose_local_repository(
        LocalRepositoryDiagnosticsRequest::new(missing, REPOSITORY_ID)
            .with_deadline(Duration::ZERO),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("deadline");
    assert!(matches!(
        deadline,
        LocalRepositoryDiagnosticsError::DeadlineExceeded
    ));
}

#[test]
fn indexed_repository_reports_exact_source_and_absent_memory_projection() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let indexed = index_local_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("index");

    let diagnostics = diagnose_local_repository(
        LocalRepositoryDiagnosticsRequest::new(&database, REPOSITORY_ID),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("diagnostics");
    assert_eq!(diagnostics.generation(), &indexed.generation());
    assert_eq!(diagnostics.source_epoch(), indexed.source_epoch());
    assert_eq!(diagnostics.index_coverage().searched(), 1);
    assert_eq!(diagnostics.memory_projection(), None);
    assert_eq!(diagnostics.supported_languages().len(), 5);
    assert_eq!(diagnostics.capabilities().len(), 4);
    assert_eq!(diagnostics.limitations().len(), 6);
}

#[test]
fn diagnostics_rejects_a_persisted_known_count_outside_the_raw_total() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    index_local_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("index");

    let connection = rusqlite::Connection::open(&database).expect("fixture database should reopen");
    connection
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             DROP TRIGGER analysis_artifacts_no_semantic_update;
             UPDATE analysis_artifacts
             SET known_parser_limitation_nodes = 1;",
        )
        .expect("fixture parser diagnostics should be corrupted");
    drop(connection);

    let error = diagnose_local_repository(
        LocalRepositoryDiagnosticsRequest::new(&database, REPOSITORY_ID),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("known diagnostics outside the raw total must fail closed");
    assert!(matches!(
        error,
        LocalRepositoryDiagnosticsError::Diagnostics(RepositoryDiagnosticsError::Port(
            SqliteStoreError::IntegrityCheckFailed
        ))
    ));
}
