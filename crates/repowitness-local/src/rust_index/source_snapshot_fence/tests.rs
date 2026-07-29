use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

use repowitness_application::{
    RustSourceSnapshotIdentity, SourceLanguage, hash_source_snapshot,
    phase0_source_artifact_identities, phase0_source_snapshot_profile,
};
use repowitness_domain::RepositoryIdentityDigest;

use crate::contained_source::FileIdentity;

use super::super::{
    LocalRustIndexLimits, SourceLanguageSelection,
    prepare_local_source_index_excluding_identity_with_reuse,
};
use super::{
    LocalSourceSnapshotFenceError, LocalSourceSnapshotFenceRequest, confirm_local_source_snapshot,
};

static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
struct Baseline {
    identity: RustSourceSnapshotIdentity,
    expected: repowitness_domain::SourceSnapshotDigest,
    languages: SourceLanguageSelection,
    limits: LocalRustIndexLimits,
}

struct TempRepository {
    base: PathBuf,
    root: PathBuf,
}

impl TempRepository {
    fn new() -> Self {
        let fixture_id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!(
            "repowitness-source-snapshot-fence-{}-{fixture_id}",
            std::process::id()
        ));
        let root = base.join("repository");
        fs::create_dir_all(root.join("src")).expect("fixture directories should be created");
        run_git(&root, &["init", "--quiet"]);
        fs::write(root.join("src/lib.rs"), b"pub fn alpha() {}\n")
            .expect("Rust fixture should be written");
        fs::write(root.join("app.py"), b"def alpha():\n    return 1\n")
            .expect("Python fixture should be written");
        fs::write(root.join("README.md"), b"fixture\n")
            .expect("non-source fixture should be written");
        run_git(&root, &["add", "--all"]);
        run_git(&root, &["commit", "--quiet", "-m", "initial"]);
        Self { base, root }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn database(&self) -> PathBuf {
        self.base.join("index.sqlite3")
    }
}

impl Drop for TempRepository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn run_git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-c")
        .arg("user.name=RepoWitness Tests")
        .arg("-c")
        .arg("user.email=repowitness@example.invalid")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .expect("Git fixture command should start");
    assert!(
        output.status.success(),
        "Git fixture command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("Git fixture output should be UTF-8")
        .trim()
        .to_owned()
}

fn baseline(repository: &TempRepository, languages: SourceLanguageSelection) -> Baseline {
    let limits = LocalRustIndexLimits::default();
    let cancelled = AtomicBool::new(false);
    let preparation = prepare_local_source_index_excluding_identity_with_reuse(
        repository.root(),
        phase0_source_artifact_identities(),
        languages,
        limits,
        &cancelled,
        None,
        |_, _, _| -> Result<_, crate::sqlite::SqliteStoreError> { Ok(BTreeMap::new()) },
    )
    .expect("baseline source preparation should succeed");
    let profile = phase0_source_snapshot_profile();
    let identity = RustSourceSnapshotIdentity::new_supported_languages(
        RepositoryIdentityDigest::new([42; 32]),
        preparation.git_state(),
        preparation.worktree_state(),
        profile.configuration(),
        profile.producer_manifest(),
        profile.analysis_schema(),
        profile.canonicalization_version(),
    );
    let expected = hash_source_snapshot(identity, preparation.prepared().manifest_digest());
    Baseline {
        identity,
        expected,
        languages,
        limits,
    }
}

fn fence_request<'a>(
    repository: &'a TempRepository,
    baseline: Baseline,
    cancelled: &'a AtomicBool,
    deadline: Instant,
    excluded_identity: Option<&'a FileIdentity>,
) -> LocalSourceSnapshotFenceRequest<'a> {
    LocalSourceSnapshotFenceRequest::new(
        repository.root(),
        baseline.identity,
        baseline.expected,
        baseline.languages,
        baseline.limits,
        cancelled,
        deadline,
        excluded_identity,
    )
}

fn future_deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

fn all_languages() -> SourceLanguageSelection {
    SourceLanguageSelection::all()
}

fn rust_only() -> SourceLanguageSelection {
    SourceLanguageSelection::from_allowed(&BTreeSet::from([SourceLanguage::Rust]))
}

fn assert_changed(repository: &TempRepository, baseline: Baseline) {
    let cancelled = AtomicBool::new(false);
    assert_eq!(
        confirm_local_source_snapshot(fence_request(
            repository,
            baseline,
            &cancelled,
            future_deadline(),
            None,
        )),
        Err(LocalSourceSnapshotFenceError::SourceChanged)
    );
}

#[test]
fn unchanged_snapshot_is_confirmed_and_request_debug_is_redacted() {
    let repository = TempRepository::new();
    let baseline = baseline(&repository, all_languages());
    let cancelled = AtomicBool::new(false);
    let request = fence_request(&repository, baseline, &cancelled, future_deadline(), None);
    let debug = format!("{request:?}");

    assert!(debug.contains("<redacted-path>"));
    assert!(debug.contains("<redacted-identity>"));
    assert!(debug.contains("<redacted-digest>"));
    assert!(!debug.contains(repository.root().to_string_lossy().as_ref()));
    confirm_local_source_snapshot(request).expect("unchanged snapshot should pass its final fence");
}

#[test]
fn selected_content_mutation_is_rejected() {
    let repository = TempRepository::new();
    let baseline = baseline(&repository, all_languages());
    fs::write(
        repository.root().join("src/lib.rs"),
        b"pub fn changed() -> usize { 1 }\n",
    )
    .expect("source mutation should be written");

    assert_changed(&repository, baseline);
}

#[test]
fn same_size_selected_content_mutation_is_rejected() {
    let repository = TempRepository::new();
    let baseline = baseline(&repository, all_languages());
    let source = repository.root().join("src/lib.rs");
    let original = fs::read(&source).expect("original source should be read");
    let replacement = b"pub fn omega() {}\n";
    assert_eq!(original.len(), replacement.len());
    fs::write(source, replacement).expect("same-size source mutation should be written");

    assert_changed(&repository, baseline);
}

#[test]
fn selected_path_addition_is_rejected() {
    let repository = TempRepository::new();
    let baseline = baseline(&repository, all_languages());
    fs::write(
        repository.root().join("src/added.rs"),
        b"pub fn added() {}\n",
    )
    .expect("added source should be written");

    assert_changed(&repository, baseline);
}

#[test]
fn selected_path_deletion_is_rejected() {
    let repository = TempRepository::new();
    let baseline = baseline(&repository, all_languages());
    fs::remove_file(repository.root().join("src/lib.rs"))
        .expect("selected source should be deleted");

    assert_changed(&repository, baseline);
}

#[test]
fn selected_path_rename_is_rejected() {
    let repository = TempRepository::new();
    let baseline = baseline(&repository, all_languages());
    fs::rename(
        repository.root().join("src/lib.rs"),
        repository.root().join("src/renamed.rs"),
    )
    .expect("selected source should be renamed");

    assert_changed(&repository, baseline);
}

#[test]
fn dirty_git_state_is_rejected_even_when_selected_manifest_is_unchanged() {
    let repository = TempRepository::new();
    let baseline = baseline(&repository, all_languages());
    fs::write(repository.root().join("README.md"), b"dirty fixture\n")
        .expect("non-source mutation should be written");

    assert_changed(&repository, baseline);
}

#[test]
fn language_selection_is_reapplied_during_the_fence() {
    let repository = TempRepository::new();
    let baseline = baseline(&repository, rust_only());
    let cancelled = AtomicBool::new(false);
    let wrong_selection = Baseline {
        languages: all_languages(),
        ..baseline
    };

    assert_eq!(
        confirm_local_source_snapshot(fence_request(
            &repository,
            wrong_selection,
            &cancelled,
            future_deadline(),
            None,
        )),
        Err(LocalSourceSnapshotFenceError::SourceChanged)
    );
}

#[test]
fn cancellation_and_elapsed_deadline_fail_before_confirmation() {
    let repository = TempRepository::new();
    let baseline = baseline(&repository, all_languages());
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        confirm_local_source_snapshot(fence_request(
            &repository,
            baseline,
            &cancelled,
            future_deadline(),
            None,
        )),
        Err(LocalSourceSnapshotFenceError::Cancelled)
    );

    cancelled.store(false, Ordering::Release);
    assert_eq!(
        confirm_local_source_snapshot(fence_request(
            &repository,
            baseline,
            &cancelled,
            Instant::now(),
            None,
        )),
        Err(LocalSourceSnapshotFenceError::DeadlineExceeded)
    );
}

#[cfg(any(unix, windows))]
#[test]
fn excluded_database_hard_link_alias_is_rejected() {
    let repository = TempRepository::new();
    let baseline = baseline(&repository, all_languages());
    let database = repository.database();
    fs::write(&database, b"database fixture").expect("database fixture should be written");
    let identity =
        FileIdentity::from_path(&database).expect("database identity should be captured");
    fs::hard_link(&database, repository.root().join("src/database.rs"))
        .expect("database hard-link alias should be created");
    let cancelled = AtomicBool::new(false);

    assert_eq!(
        confirm_local_source_snapshot(fence_request(
            &repository,
            baseline,
            &cancelled,
            future_deadline(),
            Some(&identity),
        )),
        Err(LocalSourceSnapshotFenceError::ExcludedFileAlias)
    );
}

#[cfg(unix)]
#[test]
fn excluded_database_symlink_alias_fails_closed() {
    use std::os::unix::fs::symlink;

    let repository = TempRepository::new();
    let baseline = baseline(&repository, all_languages());
    let database = repository.database();
    fs::write(&database, b"database fixture").expect("database fixture should be written");
    let identity =
        FileIdentity::from_path(&database).expect("database identity should be captured");
    symlink(&database, repository.root().join("src/database.rs"))
        .expect("database symlink alias should be created");
    let cancelled = AtomicBool::new(false);

    assert!(matches!(
        confirm_local_source_snapshot(fence_request(
            &repository,
            baseline,
            &cancelled,
            future_deadline(),
            Some(&identity),
        )),
        Err(LocalSourceSnapshotFenceError::ExcludedFileAlias
            | LocalSourceSnapshotFenceError::CaptureFailed
            | LocalSourceSnapshotFenceError::SourceChanged)
    ));
}

#[test]
fn newly_unsupported_submodule_state_is_rejected() {
    let repository = TempRepository::new();
    let baseline = baseline(&repository, all_languages());
    let head = run_git(repository.root(), &["rev-parse", "HEAD"]);
    let cache_entry = format!("160000,{head},vendor");
    run_git(
        repository.root(),
        &["update-index", "--add", "--cacheinfo", &cache_entry],
    );
    let cancelled = AtomicBool::new(false);

    assert_eq!(
        confirm_local_source_snapshot(fence_request(
            &repository,
            baseline,
            &cancelled,
            future_deadline(),
            None,
        )),
        Err(LocalSourceSnapshotFenceError::UnsupportedSourceState)
    );
}
