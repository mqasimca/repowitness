use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use repowitness_application::{ContextItem, ContextOmission};

use crate::{LocalIndexRequest, index_local_repository};

use super::*;

const REPOSITORY_ID: &str = concat!(
    "rwi1:h:",
    "0101010101010101010101010101010101010101010101010101010101010101"
);
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repowitness-local-context-{}-{ordinal}",
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
        "pub struct Widget;\nimpl Widget { pub fn run() {} }\n",
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
fn request_bounds_and_debug_output_are_explicit_and_redacted() {
    let request = LocalContextBuildRequest::new(
        Path::new("/private/root"),
        Path::new("/private/index.sqlite3"),
        REPOSITORY_ID,
        "private intent",
    )
    .with_budget_units(4096)
    .expect("budget")
    .with_max_provider_results(7)
    .expect("provider limit")
    .with_deadline(Duration::from_secs(1));
    let debug = format!("{request:?}");
    assert!(debug.contains("max_provider_results: 7"));
    assert!(!debug.contains("/private"));
    assert!(!debug.contains(REPOSITORY_ID));
    assert!(!debug.contains("private intent"));
    assert!(
        request
            .with_max_provider_results(0)
            .expect_err("zero provider results")
            .to_string()
            .contains("source-provider")
    );
    assert!(request.with_budget_units(0).is_err());
}

#[test]
fn invalid_boundaries_and_control_fail_before_filesystem_or_database_io() {
    let missing = Path::new("/missing/private-context-input");
    let invalid_identity = build_local_context(
        LocalContextBuildRequest::new(missing, missing, "invalid", "symbol"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("identity validation");
    assert!(matches!(
        invalid_identity,
        LocalContextBuildError::RepositoryIdentity(_)
    ));

    let invalid_intent = build_local_context(
        LocalContextBuildRequest::new(missing, missing, REPOSITORY_ID, ""),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("intent validation");
    assert!(matches!(
        invalid_intent,
        LocalContextBuildError::SourceQuery(_)
    ));

    let cancelled = build_local_context(
        LocalContextBuildRequest::new(missing, missing, REPOSITORY_ID, "symbol"),
        Arc::new(AtomicBool::new(true)),
    )
    .expect_err("cancellation");
    assert!(matches!(cancelled, LocalContextBuildError::Cancelled));

    let deadline = build_local_context(
        LocalContextBuildRequest::new(missing, missing, REPOSITORY_ID, "symbol")
            .with_deadline(Duration::ZERO),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("deadline");
    assert!(matches!(deadline, LocalContextBuildError::DeadlineExceeded));
}

#[test]
fn indexed_source_builds_exact_source_only_context_and_mutation_fails_closed() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let report = index_local_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("index");

    let result = build_local_context(
        LocalContextBuildRequest::new(&repository, &database, REPOSITORY_ID, "Widget"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("source-only context");
    assert_eq!(result.generation(), &report.generation());
    assert!(result.memory().is_none());
    assert!(!result.items().is_empty());
    assert!(
        result
            .items()
            .iter()
            .all(|item| matches!(item, ContextItem::Source(_)))
    );
    assert!(
        result
            .omissions()
            .contains(&ContextOmission::MemoryProjectionUnavailable)
    );
    let ContextItem::Source(first) = &result.items()[0] else {
        panic!("source item");
    };
    assert!(
        std::str::from_utf8(first.candidate().declaration())
            .expect("UTF-8 fixture")
            .contains("Widget")
    );

    fs::write(
        repository.join("src/lib.rs"),
        "pub struct ChangedAfterIndex;\n",
    )
    .expect("mutated source");
    let error = build_local_context(
        LocalContextBuildRequest::new(&repository, &database, REPOSITORY_ID, "Widget"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("stale source must fail");
    assert!(matches!(error, LocalContextBuildError::Symbol(_)));
}
