use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use repowitness_application::{
    RepositoryIdentityTextV1, SourceLanguage, phase0_rust_artifact_identity,
    phase0_source_artifact_identities, phase0_source_snapshot_profile,
};
use rusqlite::Connection;

use crate::{
    GenerationRetentionPolicy, OwnedSqliteIndex, OwnedSqliteReader,
    RawSyntaxSiteProjectionAvailability, RawSyntaxSiteReadLimits, RetentionApplyRequest,
    RetentionLimits, RetentionPins, RetentionPlanRequest, SearchLimits,
};

use super::polling_runner::reconcile_local_repository;
use super::{
    LocalIndexError, LocalIndexMutation, LocalIndexRequest, LocalReconciliationOutcome,
    index_local_rust_repository, index_local_rust_repository_with_hook,
    index_local_rust_repository_with_hooks, local_snapshot_implementation_fingerprint_inputs,
    map_index_mutation_error, phase0_local_rust_artifact_identity,
    phase0_local_source_artifact_identities, phase0_local_source_snapshot_profile,
    reconcile_local_repository_with_control_hooks,
};

const REPOSITORY_ID: &str = concat!(
    "rwi1:h:",
    "A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1"
);
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repowitness-local-index-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture directory should be created");
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

fn deadline() -> Instant {
    Instant::now()
        .checked_add(Duration::from_secs(10))
        .expect("test deadline should be representable")
}

#[test]
fn local_snapshot_identity_is_stable_while_source_artifacts_use_the_analysis_profile() {
    let base = phase0_rust_artifact_identity();
    let first = phase0_local_rust_artifact_identity();
    let second = phase0_local_rust_artifact_identity();
    let artifacts = phase0_local_source_artifact_identities();
    let snapshot = phase0_local_source_snapshot_profile(artifacts, base.configuration());
    let base_snapshot = phase0_source_snapshot_profile();

    assert_eq!(first, second);
    assert_eq!(first.producer_manifest(), base.producer_manifest());
    assert_eq!(first.configuration(), base.configuration());
    assert_eq!(first.schema(), base.schema());
    assert_eq!(
        first.canonicalization_version(),
        base.canonicalization_version()
    );
    assert_eq!(artifacts, phase0_source_artifact_identities());
    assert_ne!(
        snapshot.producer_manifest,
        base_snapshot.producer_manifest()
    );
}

#[test]
fn local_snapshot_fingerprint_covers_exact_read_session_implementation() {
    let exact_session = include_bytes!("../contained_source/exact_session.rs").as_slice();
    let inputs = local_snapshot_implementation_fingerprint_inputs();

    assert!(inputs.iter().all(|input| !input.is_empty()));
    assert!(inputs.contains(&exact_session));
}

fn fixture_repository(directory: &TempDirectory) -> PathBuf {
    let repository = directory.repository();
    fs::create_dir_all(repository.join("src")).expect("fixture source directory should be created");
    let status = Command::new("git")
        .args(["-c", "core.autocrlf=false", "-c", "core.eol=lf"])
        .args(["init", "--quiet"])
        .arg(&repository)
        .status()
        .expect("Git should start");
    assert!(status.success());
    fs::write(
        repository.join("src/lib.rs"),
        "pub struct Widget;\nimpl Widget { pub fn run() {} }\n",
    )
    .expect("Rust fixture should be written");
    fs::write(repository.join("README.md"), "fixture\n")
        .expect("non-Rust fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["-c", "core.autocrlf=false", "-c", "core.eol=lf"])
        .args(["add", "--", "src/lib.rs", "README.md"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    repository
}

#[test]
fn facade_activates_searchable_production_generations_and_reindexes() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);

    let first = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("first generation should activate");
    assert_eq!(first.generation().get(), 1);
    assert_eq!(first.source_epoch(), 1);
    assert_eq!(first.recovered_generations(), 0);
    assert_eq!(first.discovered_paths(), 2);
    assert_eq!(first.indexed_rust_files(), 1);
    assert_eq!(first.skipped_non_rust_paths(), 1);
    assert_eq!(first.total_facts(), 2);
    assert_eq!(first.syntax_error_nodes(), 0);
    assert_eq!(first.reused_rust_files(), 0);
    assert_eq!(first.analyzed_rust_files(), 1);

    let reader = OwnedSqliteReader::start(&database, deadline())
        .expect("reader should open the active generation");
    let results = reader
        .search(
            RepositoryIdentityTextV1::decode(REPOSITORY_ID)
                .expect("fixture identity should decode"),
            "Widget",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("active facts should be searchable");
    assert_eq!(results.generation(), first.generation());
    assert_eq!(results.hits().len(), 2);
    reader
        .shutdown(deadline())
        .expect("reader should shut down");

    let second = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("equivalent second reconciliation should publish a fresh generation");
    assert_eq!(second.generation().get(), 2);
    assert_eq!(second.source_epoch(), 2);
    assert_eq!(second.total_facts(), first.total_facts());
    assert_eq!(second.total_source_bytes(), first.total_source_bytes());
    assert_eq!(second.reused_rust_files(), 1);
    assert_eq!(second.analyzed_rust_files(), 0);

    fs::write(
        repository.join("src/lib.rs"),
        "pub struct Changed;\nimpl Changed { pub fn run() {} }\n",
    )
    .expect("changed Rust fixture should be written");
    let third = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("changed third generation should activate");
    assert_eq!(third.generation().get(), 3);
    assert_eq!(third.source_epoch(), 3);
    assert_eq!(third.reused_rust_files(), 0);
    assert_eq!(third.analyzed_rust_files(), 1);
}

#[test]
fn mixed_facade_persists_searches_and_reuses_go_and_rust_artifacts() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    fs::create_dir_all(repository.join("cmd")).expect("Go fixture directory should be created");
    fs::write(
        repository.join("cmd/main.go"),
        "package main\nfunc Execute() {}\ntype Options interface { Apply() }\n",
    )
    .expect("Go fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "cmd/main.go"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);

    let first = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("mixed generation should activate");
    assert_eq!(first.discovered_paths(), 3);
    assert_eq!(first.indexed_rust_files(), 1);
    assert_eq!(first.indexed_go_files(), 1);
    assert_eq!(first.skipped_unsupported_paths(), 1);
    assert_eq!(first.reused_rust_files(), 0);
    assert_eq!(first.reused_go_files(), 0);
    assert_eq!(first.analyzed_rust_files(), 1);
    assert_eq!(first.analyzed_go_files(), 1);

    let reader = OwnedSqliteReader::start(&database, deadline())
        .expect("reader should open the mixed generation");
    let repository_id =
        RepositoryIdentityTextV1::decode(REPOSITORY_ID).expect("fixture identity should decode");
    let go = reader
        .search(
            repository_id,
            "Execute",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("Go fact should be searchable");
    assert_eq!(go.hits().len(), 1);
    assert_eq!(go.hits()[0].language(), SourceLanguage::Go);
    assert_eq!(go.hits()[0].path().as_bytes(), b"cmd/main.go");
    let rust = reader
        .search(
            repository_id,
            "Widget",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("Rust facts should remain searchable");
    assert_eq!(rust.hits().len(), 2);
    assert!(
        rust.hits()
            .iter()
            .all(|hit| hit.language() == SourceLanguage::Rust)
    );
    reader
        .shutdown(deadline())
        .expect("reader should shut down");

    let second = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("equivalent mixed generation should reuse both languages");
    assert_eq!(second.reused_rust_files(), 1);
    assert_eq!(second.reused_go_files(), 1);
    assert_eq!(second.analyzed_rust_files(), 0);
    assert_eq!(second.analyzed_go_files(), 0);
}

#[test]
fn typescript_facade_persists_searches_and_reuses_both_dialects() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    fs::create_dir_all(repository.join("web"))
        .expect("TypeScript fixture directory should be created");
    fs::write(
        repository.join("web/api.ts"),
        "export function loadFrontend() {}\n",
    )
    .expect("TypeScript fixture should be written");
    fs::write(
        repository.join("web/view.tsx"),
        "export function FrontendView() { return <main />; }\n",
    )
    .expect("TSX fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "web/api.ts", "web/view.tsx"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);

    let first = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("TypeScript generation should activate");
    assert_eq!(first.discovered_paths(), 4);
    assert_eq!(first.indexed_rust_files(), 1);
    assert_eq!(first.indexed_typescript_files(), 1);
    assert_eq!(first.indexed_tsx_files(), 1);
    assert_eq!(first.skipped_unsupported_paths(), 1);
    assert_eq!(first.analyzed_typescript_files(), 1);
    assert_eq!(first.analyzed_tsx_files(), 1);
    assert_eq!(first.reused_typescript_files(), 0);
    assert_eq!(first.reused_tsx_files(), 0);

    let reader = OwnedSqliteReader::start(&database, deadline())
        .expect("reader should open the TypeScript generation");
    let repository_id =
        RepositoryIdentityTextV1::decode(REPOSITORY_ID).expect("fixture identity should decode");
    for (query, language, path) in [
        (
            "loadFrontend",
            SourceLanguage::TypeScript,
            b"web/api.ts".as_slice(),
        ),
        (
            "FrontendView",
            SourceLanguage::Tsx,
            b"web/view.tsx".as_slice(),
        ),
    ] {
        let results = reader
            .search(
                repository_id,
                query,
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("TypeScript fact should be searchable");
        assert_eq!(results.hits().len(), 1);
        assert_eq!(results.hits()[0].language(), language);
        assert_eq!(results.hits()[0].path().as_bytes(), path);
        assert_eq!(
            results.hits()[0].producer_manifest(),
            phase0_local_source_artifact_identities()
                .for_language(language)
                .producer_manifest()
        );
    }
    reader
        .shutdown(deadline())
        .expect("reader should shut down");

    let second = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("equivalent TypeScript generation should reuse both dialects");
    assert_eq!(second.reused_typescript_files(), 1);
    assert_eq!(second.reused_tsx_files(), 1);
    assert_eq!(second.analyzed_typescript_files(), 0);
    assert_eq!(second.analyzed_tsx_files(), 0);
}

#[test]
fn missing_payload_is_reanalyzed_repaired_and_then_reused() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);

    let first = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("first generation should activate");
    assert_eq!(first.analyzed_rust_files(), 1);

    let connection = Connection::open(&database).expect("fixture database should open");
    connection
        .execute_batch(
            "DROP TRIGGER analysis_artifact_payload_digest_set_once;
                 UPDATE analysis_artifacts SET payload_digest = NULL;
                 CREATE TRIGGER analysis_artifact_payload_digest_set_once
                 BEFORE UPDATE OF payload_digest ON analysis_artifacts
                 WHEN NOT (
                     OLD.payload_digest IS NULL
                     AND NEW.payload_digest IS NOT NULL
                     AND length(NEW.payload_digest) = 32
                 )
                 BEGIN
                     SELECT RAISE(ABORT, 'immutable analysis artifact payload identity');
                 END;",
        )
        .expect("fixture should remove the persisted payload identity");
    drop(connection);

    let backfilled = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("incomplete artifact identity should be analyzed and repaired");
    assert_eq!(backfilled.reused_rust_files(), 0);
    assert_eq!(backfilled.analyzed_rust_files(), 1);
    let connection = Connection::open(&database).expect("fixture database should reopen");
    let incomplete_artifacts: i64 = connection
        .query_row(
            "SELECT count(*) FROM analysis_artifacts WHERE payload_digest IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("every source and graph artifact identity should be repaired");
    assert_eq!(incomplete_artifacts, 0);
    drop(connection);

    let reused = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("backfilled artifact should become reusable");
    assert_eq!(reused.reused_rust_files(), 1);
    assert_eq!(reused.analyzed_rust_files(), 0);
}

#[test]
fn mutation_lease_is_acquired_before_source_capture() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);

    let report =
        index_local_rust_repository_with_hook(request, Arc::new(AtomicBool::new(false)), || {
            fs::write(repository.join("src/lib.rs"), "pub struct AfterLease;\n")
                .expect("source mutation after lease acquisition should succeed");
        })
        .expect("capture after lease acquisition should activate");
    let reader = OwnedSqliteReader::start(&database, deadline())
        .expect("reader should open the active generation");
    let identity =
        RepositoryIdentityTextV1::decode(REPOSITORY_ID).expect("fixture identity should decode");
    let current = reader
        .search(
            identity,
            "AfterLease",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("post-lease source should be searchable");
    let stale = reader
        .search(
            identity,
            "Widget",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("pre-lease source query should complete");

    assert_eq!(current.generation(), report.generation());
    assert!(!current.hits().is_empty());
    assert!(stale.hits().is_empty());
    reader
        .shutdown(deadline())
        .expect("reader should shut down");
}

#[test]
fn validation_and_preparation_failures_do_not_create_a_database() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let invalid_identity = "rwi1:h:PRIVATE";
    let request = LocalIndexRequest::new(
        Path::new("private-repository-that-does-not-exist"),
        &database,
        invalid_identity,
        0,
    );
    let debug = format!("{request:?}");
    assert!(!debug.contains("private-repository"));
    assert!(!debug.contains(invalid_identity));
    assert!(!debug.contains(database.to_string_lossy().as_ref()));
    let error = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect_err("invalid identity should fail first");
    assert!(matches!(error, LocalIndexError::RepositoryIdentity { .. }));
    assert_eq!(error.to_string(), "repository identity is invalid");
    assert!(!format!("{error:?}").contains(invalid_identity));
    assert!(!database.exists());

    let error = index_local_rust_repository(
        LocalIndexRequest::new(
            Path::new("private-repository-that-does-not-exist"),
            &database,
            REPOSITORY_ID,
            0,
        ),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("missing repository should fail preparation");
    assert!(matches!(error, LocalIndexError::Preparation { .. }));
    assert!(!error.to_string().contains("private-repository"));
    assert!(!format!("{error:?}").contains("private-repository"));
    assert!(!database.exists());
}

#[test]
fn directory_database_target_is_rejected_before_preparation() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);

    let error = index_local_rust_repository(
        LocalIndexRequest::new(&repository, &directory.0, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("a directory cannot serve as the database file");
    assert!(matches!(error, LocalIndexError::DatabasePathUnavailable));
}

#[test]
fn cancellation_is_shared_across_the_complete_facade() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let cancelled = Arc::new(AtomicBool::new(true));

    let error = index_local_rust_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        cancelled,
    )
    .expect_err("pre-cancelled indexing should fail closed");

    assert!(matches!(error, LocalIndexError::Preparation { .. }));
    assert!(!database.exists());
}

#[test]
fn worktree_local_database_is_rejected_before_it_can_change_source_state() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = repository.join("private-index.sqlite3");

    let error = index_local_rust_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("worktree-local database should fail closed");

    assert!(matches!(error, LocalIndexError::DatabaseInsideWorktree));
    assert_eq!(
        error.to_string(),
        "local index database must be outside the repository worktree"
    );
    assert!(!database.exists());
}

#[cfg(any(unix, windows))]
#[test]
fn hard_linked_database_cannot_alias_a_repository_source() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    let first = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("initial database should activate");
    fs::hard_link(&database, repository.join("index.rs"))
        .expect("fixture database hard link should be created");

    let error = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect_err("a database alias inside the worktree must be rejected");
    assert!(matches!(
        error,
        LocalIndexError::DatabaseHasMultipleLinks | LocalIndexError::DatabaseChangedDuringIndexing
    ));

    let reader = OwnedSqliteReader::start(&database, deadline())
        .expect("the previous active generation should remain readable");
    let results = reader
        .search(
            RepositoryIdentityTextV1::decode(REPOSITORY_ID)
                .expect("fixture identity should decode"),
            "Widget",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("the previous generation should remain searchable");
    assert_eq!(results.generation(), first.generation());
    reader
        .shutdown(deadline())
        .expect("reader should shut down");
}

#[cfg(any(unix, windows))]
#[test]
fn database_alias_created_after_lease_cannot_modify_a_repository_source() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let seed_database = directory.database();
    index_local_rust_repository(
        LocalIndexRequest::new(&repository, &seed_database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("seed database should activate");
    let source = repository.join("database-image.rs");
    fs::copy(&seed_database, &source)
        .expect("valid SQLite image should be copied into a repository source");
    let original_source = fs::read(&source).expect("source image should be readable");
    let database = directory.0.join("late-database.sqlite3");

    let error = index_local_rust_repository_with_hook(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
        || {
            fs::hard_link(&source, &database)
                .expect("database alias should be created after the initial identity check");
        },
    )
    .expect_err("a late database alias must fail before SQLite can modify the source");

    assert!(matches!(
        error,
        LocalIndexError::DatabaseHasMultipleLinks | LocalIndexError::DatabaseChangedDuringIndexing
    ));
    assert_eq!(
        fs::read(&source).expect("source should remain readable"),
        original_source,
        "failed indexing must leave the aliased source unchanged"
    );
}

#[cfg(any(unix, windows))]
#[test]
fn database_replacement_after_lease_is_rejected_before_writes() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("initial database should activate");
    let replacement = directory.0.join("replacement.sqlite3");
    fs::copy(&database, &replacement).expect("replacement database should be copied");
    let replacement_bytes =
        fs::read(&replacement).expect("replacement database should be readable");
    let displaced = directory.0.join("displaced.sqlite3");

    let error =
        index_local_rust_repository_with_hook(request, Arc::new(AtomicBool::new(false)), || {
            fs::rename(&database, &displaced)
                .expect("original database should be displaced after identity capture");
            fs::rename(&replacement, &database)
                .expect("replacement should occupy the database path");
        })
        .expect_err("a replaced database must fail before the writer starts");

    assert!(matches!(
        error,
        LocalIndexError::DatabaseChangedDuringIndexing
    ));
    assert_eq!(
        fs::read(&database).expect("replacement database should remain readable"),
        replacement_bytes,
        "failed indexing must not write through the replacement path"
    );
}

#[cfg(unix)]
#[test]
fn dangling_database_symlink_cannot_bypass_worktree_isolation() {
    use std::os::unix::fs::symlink;

    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let target = repository.join("private-index.sqlite3");
    let database = directory.0.join("database-link");
    symlink(&target, &database).expect("fixture symlink should be created");

    let error = index_local_rust_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("dangling database symlink should fail closed");

    assert!(matches!(error, LocalIndexError::DatabasePathUnavailable));
    assert!(!target.exists());
}

#[test]
fn unknown_index_mutation_is_explicit_and_not_rendered_as_failed() {
    let error = map_index_mutation_error(
        LocalIndexMutation::GenerationPublication,
        crate::SqliteStoreError::MutationOutcomeUnknown,
        |source| LocalIndexError::PublicationActivation { source },
    );

    assert!(matches!(
        error,
        LocalIndexError::MutationOutcomeUnknown {
            operation: LocalIndexMutation::GenerationPublication
        }
    ));
    assert_eq!(
        error.reconciliation_guidance(),
        Some(
            "reopen the store and read the active generation and source-slot completion before retrying"
        )
    );
    assert_eq!(
        error.to_string(),
        "local index mutation outcome could not be determined"
    );
}

include!("tests/python.rs");
include!("tests/parser_diagnostics.rs");
include!("tests/configuration.rs");
include!("tests/final_fence.rs");
include!("tests/graph_artifact_reuse.rs");
include!("tests/post_commit_semantics.rs");
include!("tests/watched_reconciliation.rs");
