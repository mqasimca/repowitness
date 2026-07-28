use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use repowitness_analysis::RustAnalysisLimits;
use repowitness_domain::{AnalysisSchemaDigest, ConfigurationDigest, ProducerManifestDigest};

use super::*;

static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

struct TempRepository {
    root: PathBuf,
}

impl TempRepository {
    fn new() -> Self {
        let fixture_id = NEXT_FIXTURE_ID.fetch_add(1, AtomicOrdering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "repowitness-local-rust-index-{}-{fixture_id}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("fixture directory must be created");
        let repository = Self { root };
        repository.git(&["init", "--quiet", "--initial-branch=main"]);
        repository
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn write(&self, relative: &str, content: &[u8]) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent must be created");
        }
        fs::write(path, content).expect("fixture source must be written");
    }

    fn git(&self, arguments: &[&str]) {
        let status = Command::new("git")
            .arg("--no-pager")
            .arg("-C")
            .arg(&self.root)
            .args(arguments)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", null_device())
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("fixture Git command must start");
        assert!(status.success(), "fixture Git command failed: {status}");
    }

    fn commit_all(&self, message: &str) {
        self.git(&["add", "--all"]);
        self.git(&[
            "-c",
            "user.name=RepoWitness Test",
            "-c",
            "user.email=repowitness@example.invalid",
            "commit",
            "--quiet",
            "-m",
            message,
        ]);
    }

    fn commit_empty(&self, message: &str) {
        self.git(&[
            "-c",
            "user.name=RepoWitness Test",
            "-c",
            "user.email=repowitness@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            message,
        ]);
    }
}

impl Drop for TempRepository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn null_device() -> OsString {
    if cfg!(windows) {
        OsString::from("NUL")
    } else {
        OsString::from("/dev/null")
    }
}

fn identity() -> RustArtifactIdentity {
    RustArtifactIdentity::new(
        ProducerManifestDigest::new([1; 32]),
        ConfigurationDigest::new([2; 32]),
        AnalysisSchemaDigest::new([3; 32]),
        1,
    )
}

fn source_identities() -> SourceArtifactIdentities {
    SourceArtifactIdentities::new(
        identity(),
        RustArtifactIdentity::new(
            ProducerManifestDigest::new([4; 32]),
            ConfigurationDigest::new([5; 32]),
            AnalysisSchemaDigest::new([6; 32]),
            1,
        ),
        RustArtifactIdentity::new(
            ProducerManifestDigest::new([7; 32]),
            ConfigurationDigest::new([8; 32]),
            AnalysisSchemaDigest::new([9; 32]),
            1,
        ),
        RustArtifactIdentity::new(
            ProducerManifestDigest::new([10; 32]),
            ConfigurationDigest::new([11; 32]),
            AnalysisSchemaDigest::new([12; 32]),
            1,
        ),
        RustArtifactIdentity::new(
            ProducerManifestDigest::new([13; 32]),
            ConfigurationDigest::new([14; 32]),
            AnalysisSchemaDigest::new([15; 32]),
            1,
        ),
    )
}

#[test]
fn mixed_local_slice_selects_every_supported_language_case_sensitively() {
    let repository = TempRepository::new();
    repository.write("cmd/main.go", b"package main\nfunc Execute() {}\n");
    repository.write("src/lib.rs", b"pub struct Visible;\n");
    repository.write("web/api.ts", b"export function load() {}\n");
    repository.write(
        "web/view.tsx",
        b"export function View() { return <main />; }\n",
    );
    repository.write("sdk/client.py", b"class Client:\n    pass\n");
    repository.write("sdk/types.pyi", b"class Response: ...\n");
    repository.write("upper.GO", b"package ignored\n");
    repository.write("upper.TS", b"export const ignored = 1;\n");
    repository.write("upper.PY", b"class Ignored:\n    pass\n");
    repository.write("README.md", b"unsupported\n");
    repository.commit_all("mixed fixture");
    let cancelled = AtomicBool::new(false);

    let prepared = prepare_local_source_index(
        repository.root(),
        source_identities(),
        LocalRustIndexLimits::default(),
        &cancelled,
    )
    .expect("stable mixed fixture repository should prepare");

    assert_eq!(prepared.discovered_paths(), 10);
    assert_eq!(prepared.selected_rust_files(), 1);
    assert_eq!(prepared.selected_go_files(), 1);
    assert_eq!(prepared.selected_typescript_files(), 1);
    assert_eq!(prepared.selected_tsx_files(), 1);
    assert_eq!(prepared.selected_python_files(), 2);
    assert_eq!(prepared.skipped_unsupported_paths(), 4);
    assert_eq!(prepared.prepared().indexed_rust_files(), 1);
    assert_eq!(prepared.prepared().indexed_go_files(), 1);
    assert_eq!(prepared.prepared().indexed_typescript_files(), 1);
    assert_eq!(prepared.prepared().indexed_tsx_files(), 1);
    assert_eq!(prepared.prepared().indexed_python_files(), 2);
    assert_eq!(prepared.prepared().total_facts(), 6);
    assert_eq!(
        prepared
            .prepared()
            .files()
            .iter()
            .map(|file| (file.path().as_bytes(), file.language()))
            .collect::<Vec<_>>(),
        [
            (b"cmd/main.go".as_slice(), SourceLanguage::Go),
            (b"sdk/client.py".as_slice(), SourceLanguage::Python),
            (b"sdk/types.pyi".as_slice(), SourceLanguage::Python),
            (b"src/lib.rs".as_slice(), SourceLanguage::Rust),
            (b"web/api.ts".as_slice(), SourceLanguage::TypeScript),
            (b"web/view.tsx".as_slice(), SourceLanguage::Tsx),
        ]
    );
}

#[test]
fn local_vertical_slice_discovers_reads_analyzes_and_revalidates() {
    let repository = TempRepository::new();
    repository.write("src/lib.rs", b"pub struct Visible;\n");
    repository.write("README.txt", b"not Rust\n");
    repository.write("upper.RS", b"fn upper() {}\n");
    let cancelled = AtomicBool::new(false);

    let prepared = prepare_local_rust_index(
        repository.root(),
        identity(),
        LocalRustIndexLimits::default(),
        &cancelled,
    )
    .expect("stable fixture repository must prepare");

    assert_eq!(prepared.discovered_paths(), 3);
    assert_eq!(prepared.selected_rust_files(), 1);
    assert_eq!(prepared.skipped_non_rust_paths(), 2);
    assert_eq!(prepared.prepared().files().len(), 1);
    assert_eq!(prepared.prepared().total_facts(), 1);
    assert_eq!(
        prepared.prepared().files()[0].path().as_bytes(),
        b"src/lib.rs"
    );

    let repeated = prepare_local_rust_index(
        repository.root(),
        identity(),
        LocalRustIndexLimits::default(),
        &cancelled,
    )
    .expect("unchanged fixture repository must prepare identically");
    assert_eq!(repeated.git_state(), prepared.git_state());
    assert_eq!(repeated.worktree_state(), prepared.worktree_state());

    repository.write("src/lib.rs", b"pub struct Changed;\n");
    let changed = prepare_local_rust_index(
        repository.root(),
        identity(),
        LocalRustIndexLimits::default(),
        &cancelled,
    )
    .expect("new stable source state must prepare");
    assert_eq!(changed.git_state(), prepared.git_state());
    assert_ne!(changed.worktree_state(), prepared.worktree_state());
}

#[test]
fn aggregate_limits_cancellation_and_deadline_fail_closed() {
    let repository = TempRepository::new();
    repository.write("a.rs", b"fn a() {}\n");
    let cancelled = AtomicBool::new(true);
    assert!(matches!(
        prepare_local_rust_index(
            repository.root(),
            identity(),
            LocalRustIndexLimits::default(),
            &cancelled,
        ),
        Err(LocalRustIndexError::Cancelled)
    ));

    let not_cancelled = AtomicBool::new(false);
    let zero_deadline = LocalRustIndexLimits::new(
        Duration::ZERO,
        GitPathDiscoveryLimits::default(),
        SourceReadLimits::default(),
        RustIndexLimits::default(),
    );
    assert!(matches!(
        prepare_local_rust_index(repository.root(), identity(), zero_deadline, &not_cancelled,),
        Err(LocalRustIndexError::DeadlineExceeded)
    ));

    let byte_limited = RustIndexLimits::try_new(10, 1, 100, RustAnalysisLimits::default())
        .expect("fixture aggregate limits must be valid");
    let limits = LocalRustIndexLimits::new(
        Duration::from_secs(5),
        GitPathDiscoveryLimits::default(),
        SourceReadLimits::default(),
        byte_limited,
    );
    assert!(matches!(
        prepare_local_rust_index(repository.root(), identity(), limits, &not_cancelled,),
        Err(LocalRustIndexError::SourceByteLimitExceeded { limit: 1 })
    ));
}

#[test]
fn path_and_content_mutation_are_rejected_by_final_revalidation() {
    let path_repository = TempRepository::new();
    path_repository.write("stable.rs", b"fn stable() {}\n");
    let cancelled = AtomicBool::new(false);
    let path_error = prepare_local_rust_index_with_hook(
        path_repository.root(),
        identity(),
        LocalRustIndexLimits::default(),
        &cancelled,
        || path_repository.write("added.rs", b"fn added() {}\n"),
    )
    .expect_err("a changed path set must fail revalidation");
    assert!(matches!(path_error, LocalRustIndexError::StalePathSet));

    let content_repository = TempRepository::new();
    content_repository.write("stable.rs", b"fn before() {}\n");
    let content_error = prepare_local_rust_index_with_hook(
        content_repository.root(),
        identity(),
        LocalRustIndexLimits::default(),
        &cancelled,
        || content_repository.write("stable.rs", b"fn after() {}\n"),
    )
    .expect_err("changed source bytes must fail revalidation");
    assert!(matches!(
        content_error,
        LocalRustIndexError::StaleSourceContent { ordinal: 1 }
    ));
}

#[test]
fn index_status_and_head_mutations_are_rejected_by_the_source_state_fence() {
    let cancelled = AtomicBool::new(false);

    let index_repository = TempRepository::new();
    index_repository.write("stable.rs", b"fn stable() {}\n");
    let index_error = prepare_local_rust_index_with_hook(
        index_repository.root(),
        identity(),
        LocalRustIndexLimits::default(),
        &cancelled,
        || index_repository.git(&["add", "stable.rs"]),
    )
    .expect_err("an index mutation must fail the source-state fence");
    assert!(matches!(
        index_error,
        LocalRustIndexError::SourceState {
            source: SourceStateError::ConcurrentSourceChange
        }
    ));

    let status_repository = TempRepository::new();
    status_repository.write("stable.rs", b"fn stable() {}\n");
    status_repository.write("README.md", b"before\n");
    status_repository.commit_all("initial");
    let status_error = prepare_local_rust_index_with_hook(
        status_repository.root(),
        identity(),
        LocalRustIndexLimits::default(),
        &cancelled,
        || status_repository.write("README.md", b"after\n"),
    )
    .expect_err("a tracked non-Rust status mutation must fail the source-state fence");
    assert!(matches!(
        status_error,
        LocalRustIndexError::SourceState {
            source: SourceStateError::ConcurrentSourceChange
        }
    ));

    let head_repository = TempRepository::new();
    head_repository.write("stable.rs", b"fn stable() {}\n");
    head_repository.commit_all("initial");
    let head_error = prepare_local_rust_index_with_hook(
        head_repository.root(),
        identity(),
        LocalRustIndexLimits::default(),
        &cancelled,
        || head_repository.commit_empty("move head"),
    )
    .expect_err("a HEAD mutation must fail the source-state fence");
    assert!(matches!(
        head_error,
        LocalRustIndexError::SourceState {
            source: SourceStateError::ConcurrentSourceChange
        }
    ));
}

#[cfg(unix)]
#[test]
fn selected_symlink_sources_are_rejected_without_leaking_targets() {
    use std::os::unix::fs::symlink;

    let repository = TempRepository::new();
    let outside = repository
        .root()
        .parent()
        .expect("fixture has a parent")
        .join(format!(
            "repowitness-private-target-{}",
            NEXT_FIXTURE_ID.fetch_add(1, AtomicOrdering::Relaxed)
        ));
    fs::write(&outside, b"fn private_target() {}\n").expect("outside fixture must be written");
    symlink(&outside, repository.root().join("linked.rs")).expect("source symlink must be created");
    let cancelled = AtomicBool::new(false);

    let error = prepare_local_rust_index(
        repository.root(),
        identity(),
        LocalRustIndexLimits::default(),
        &cancelled,
    )
    .expect_err("source symlink must fail closed");
    let _ = fs::remove_file(&outside);

    assert!(matches!(
        error,
        LocalRustIndexError::SourceRead { ordinal: 1, .. }
    ));
    assert!(!error.to_string().contains("private-target"));
    assert!(!format!("{error:?}").contains("private-target"));
}

#[test]
fn rust_path_filter_is_case_sensitive_and_byte_based() {
    assert!(is_rust_source_path(b"src/lib.rs"));
    assert!(!is_rust_source_path(b"src/lib.RS"));
    assert!(!is_rust_source_path(b"src/rs"));
    assert!(is_rust_source_path(b"non-utf8-\xFF.rs"));
}
