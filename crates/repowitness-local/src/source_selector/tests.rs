use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::*;

const TEST_LIMITS: SourceSelectorLimits = SourceSelectorLimits::new(Duration::from_secs(5), 256);

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempRepository {
    root: PathBuf,
}

impl TempRepository {
    fn new(object_format: Option<&str>) -> Self {
        Self::new_named("selector", object_format)
    }

    fn new_named(name: &str, object_format: Option<&str>) -> Self {
        let ordinal = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "repowitness-{name}-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("temporary repository directory must be created");
        let mut command = Command::new("git");
        command
            .arg("init")
            .arg("--quiet")
            .arg("--initial-branch=main");
        if let Some(format) = object_format {
            command.arg(format!("--object-format={format}"));
        }
        command.arg(&root);
        let output = command.output().expect("Git init must start");
        assert_success(&output, "Git init");
        Self { root }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn write(&self, name: &str, contents: &str) {
        fs::write(self.root.join(name), contents).expect("fixture source must be written");
    }

    fn commit(&self, name: &str, contents: &str, message: &str) -> String {
        self.write(name, contents);
        self.git(&["add", "--", name]);
        self.git(&["commit", "--quiet", "-m", message]);
        self.git_text(&["rev-parse", "HEAD"])
    }

    fn git(&self, arguments: &[&str]) {
        let output = self.git_output(arguments);
        assert_success(&output, "fixture Git command");
    }

    fn git_text(&self, arguments: &[&str]) -> String {
        let output = self.git_output(arguments);
        assert_success(&output, "fixture Git text command");
        String::from_utf8(output.stdout)
            .expect("fixture Git text must be UTF-8")
            .trim_end()
            .to_owned()
    }

    fn git_output(&self, arguments: &[&str]) -> Output {
        Command::new("git")
            .arg("--no-pager")
            .arg("-c")
            .arg("user.name=RepoWitness Tests")
            .arg("-c")
            .arg("user.email=repowitness-tests@example.invalid")
            .arg("-C")
            .arg(&self.root)
            .args(arguments)
            .output()
            .expect("fixture Git command must start")
    }
}

impl Drop for TempRepository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_success(output: &Output, operation: &str) {
    assert!(
        output.status.success(),
        "{operation} failed with status {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn selector(text: &str) -> SourceSelectorV1 {
    SourceSelectorV1::parse(text).expect("fixture selector must pass admission")
}

fn resolve(
    repository: &TempRepository,
    text: &str,
) -> Result<ResolvedSourceSelector, SourceSelectorResolutionError> {
    resolve_source_selector(
        repository.root(),
        selector(text),
        TEST_LIMITS,
        &AtomicBool::new(false),
    )
}

#[test]
fn defaults_and_exact_allow_list_are_stable() {
    assert_eq!(
        SourceSelectorLimits::default(),
        SourceSelectorLimits::new(Duration::from_secs(30), 256)
    );
    assert_eq!(TEST_LIMITS.deadline(), Duration::from_secs(5));
    assert_eq!(TEST_LIMITS.output_bytes(), 256);

    assert_eq!(
        selector("worktree-head").category(),
        SourceSelectorCategory::WorktreeHead
    );
    assert_eq!(
        selector(&"a".repeat(40)).category(),
        SourceSelectorCategory::ExactRevision
    );
    assert_eq!(
        selector(&"B".repeat(64)).category(),
        SourceSelectorCategory::ExactRevision
    );
    for reference in [
        "refs/heads/main",
        "refs/tags/release",
        "refs/remotes/origin/main",
    ] {
        assert_eq!(
            selector(reference).category(),
            SourceSelectorCategory::FullRef
        );
    }
}

#[test]
fn malformed_control_and_option_shaped_selectors_fail_closed() {
    let cases = [
        ("", SourceSelectorAdmissionError::Empty),
        ("main", SourceSelectorAdmissionError::UnsupportedCategory),
        ("--help", SourceSelectorAdmissionError::UnsupportedCategory),
        (
            "refs/pull/1/head",
            SourceSelectorAdmissionError::UnsupportedCategory,
        ),
        (
            "refs/heads/",
            SourceSelectorAdmissionError::UnsupportedCategory,
        ),
        (
            "refs/heads/private\nvalue",
            SourceSelectorAdmissionError::ControlCharacter,
        ),
        (
            "refs/heads/private\0value",
            SourceSelectorAdmissionError::ControlCharacter,
        ),
    ];
    for (text, expected) in cases {
        assert_eq!(SourceSelectorV1::parse(text), Err(expected));
    }

    let mut malformed_exact = "a".repeat(40);
    malformed_exact.replace_range(19..20, "g");
    assert_eq!(
        SourceSelectorV1::parse(&malformed_exact),
        Err(SourceSelectorAdmissionError::InvalidExactRevision)
    );
    assert_eq!(
        SourceSelectorV1::parse(&"a".repeat(39)),
        Err(SourceSelectorAdmissionError::UnsupportedCategory)
    );
}

#[test]
fn selector_byte_bound_accepts_exact_limit_and_rejects_one_more() {
    let prefix = "refs/heads/";
    let exact = format!(
        "{prefix}{}",
        "a".repeat(MAX_SOURCE_SELECTOR_BYTES - prefix.len())
    );
    assert!(SourceSelectorV1::parse(&exact).is_ok());

    let over = format!("{exact}a");
    assert_eq!(
        SourceSelectorV1::parse(&over),
        Err(SourceSelectorAdmissionError::ByteLimitExceeded {
            limit: MAX_SOURCE_SELECTOR_BYTES
        })
    );
}

#[test]
fn sha1_symbolic_head_exact_and_all_ref_namespaces_resolve() {
    let repository = TempRepository::new(None);
    let head = repository.commit("lib.rs", "fn first() {}\n", "first");
    repository.git(&["tag", "release"]);
    repository.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);

    let expected = [
        ("worktree-head", SourceSelectorCategory::WorktreeHead),
        (head.as_str(), SourceSelectorCategory::ExactRevision),
        ("refs/heads/main", SourceSelectorCategory::FullRef),
        ("refs/tags/release", SourceSelectorCategory::FullRef),
        ("refs/remotes/origin/main", SourceSelectorCategory::FullRef),
    ];
    for (text, category) in expected {
        let resolved = resolve(&repository, text).expect("matching selector must resolve");
        assert_eq!(resolved.category(), category);
        assert_eq!(
            resolved.commit().object_format(),
            SourceSelectorObjectFormat::Sha1
        );
        assert_eq!(
            resolved.moving_ref_digest().is_some(),
            category == SourceSelectorCategory::FullRef
        );
        resolved
            .confirm(
                TEST_LIMITS,
                &AtomicBool::new(false),
                Instant::now() + TEST_LIMITS.deadline(),
            )
            .expect("unchanged selector must pass its final fence");
    }
}

#[test]
fn detached_and_unborn_head_states_are_categorical() {
    let detached = TempRepository::new(None);
    let head = detached.commit("main.rs", "fn main() {}\n", "detached");
    detached.git(&["checkout", "--quiet", "--detach", &head]);
    assert!(resolve(&detached, "worktree-head").is_ok());
    assert!(resolve(&detached, &head).is_ok());

    let unborn = TempRepository::new(None);
    let error = resolve(&unborn, "worktree-head").expect_err("unborn HEAD must fail");
    assert!(matches!(
        error,
        SourceSelectorResolutionError::HeadUnavailable
    ));
}

#[test]
fn sha256_object_format_and_width_are_enforced() {
    let repository = TempRepository::new(Some("sha256"));
    let head = repository.commit("lib.rs", "fn sha256() {}\n", "sha256");
    assert_eq!(head.len(), 64);
    let resolved = resolve(&repository, &head).expect("full SHA-256 revision must resolve");
    assert_eq!(resolved.category(), SourceSelectorCategory::ExactRevision);
    assert_eq!(
        resolved.commit().object_format(),
        SourceSelectorObjectFormat::Sha256
    );
    assert_eq!(resolved.commit().as_bytes().len(), 32);

    let error = resolve(&repository, &"a".repeat(40))
        .expect_err("SHA-1 width must not enter a SHA-256 repository");
    assert!(matches!(
        error,
        SourceSelectorResolutionError::ExactRevisionObjectFormatMismatch
    ));
}

#[test]
fn annotated_tags_are_peeled_to_the_matching_commit() {
    let repository = TempRepository::new(None);
    repository.commit("lib.rs", "fn tagged() {}\n", "tagged");
    repository.git(&["tag", "-a", "annotated", "-m", "annotated"]);

    let resolved =
        resolve(&repository, "refs/tags/annotated").expect("annotated tag must peel to commit");
    assert_eq!(resolved.category(), SourceSelectorCategory::FullRef);
}

#[test]
fn missing_malformed_and_mismatched_selectors_are_distinct() {
    let repository = TempRepository::new(None);
    let first = repository.commit("lib.rs", "fn first() {}\n", "first");

    let missing =
        resolve(&repository, "refs/heads/missing").expect_err("missing full ref must not resolve");
    assert!(matches!(
        missing,
        SourceSelectorResolutionError::SelectorUnavailable
    ));

    for malformed in ["refs/heads/bad..name", "refs/tags/.hidden"] {
        let error = resolve(&repository, malformed).expect_err("malformed full ref must fail");
        assert!(matches!(
            error,
            SourceSelectorResolutionError::InvalidFullRef
        ));
    }

    repository.commit("lib.rs", "fn second() {}\n", "second");
    let mismatch =
        resolve(&repository, &first).expect_err("selector must equal caller worktree HEAD");
    assert!(matches!(
        mismatch,
        SourceSelectorResolutionError::WorktreeHeadMismatch
    ));
}

#[test]
fn moving_ref_is_rechecked_at_the_final_fence() {
    let repository = TempRepository::new(None);
    let first = repository.commit("lib.rs", "fn first() {}\n", "first");
    repository.git(&["tag", "moving", &first]);
    let resolved =
        resolve(&repository, "refs/tags/moving").expect("initial moving ref must match HEAD");

    let second = repository.commit("lib.rs", "fn second() {}\n", "second");
    repository.git(&["checkout", "--quiet", "--detach", &first]);
    repository.git(&["tag", "--force", "moving", &second]);

    let error = resolved
        .confirm(
            TEST_LIMITS,
            &AtomicBool::new(false),
            Instant::now() + TEST_LIMITS.deadline(),
        )
        .expect_err("retargeted moving ref must fail the final fence");
    assert!(matches!(
        error,
        SourceSelectorFinalFenceError::SourceChanged
    ));
}

#[test]
fn worktree_head_is_rechecked_at_the_final_fence() {
    let repository = TempRepository::new(None);
    repository.commit("lib.rs", "fn first() {}\n", "first");
    let resolved =
        resolve(&repository, "worktree-head").expect("initial worktree HEAD must resolve");
    repository.commit("lib.rs", "fn second() {}\n", "second");

    let error = resolved
        .confirm(
            TEST_LIMITS,
            &AtomicBool::new(false),
            Instant::now() + TEST_LIMITS.deadline(),
        )
        .expect_err("changed worktree HEAD must fail the final fence");
    assert!(matches!(
        error,
        SourceSelectorFinalFenceError::SourceChanged
    ));
}

#[test]
fn cancellation_deadline_and_output_bounds_fail_closed() {
    let missing = Path::new("repowitness-selector-path-must-not-be-opened");
    let cancelled = AtomicBool::new(true);
    let error =
        resolve_source_selector(missing, selector("worktree-head"), TEST_LIMITS, &cancelled)
            .expect_err("pre-cancelled resolution must fail before path access");
    assert!(matches!(error, SourceSelectorResolutionError::Cancelled));

    let deadline = SourceSelectorLimits::new(Duration::ZERO, 256);
    let error = resolve_source_selector(
        missing,
        selector("worktree-head"),
        deadline,
        &AtomicBool::new(false),
    )
    .expect_err("zero deadline must fail before path access");
    assert!(matches!(
        error,
        SourceSelectorResolutionError::DeadlineExceeded {
            deadline: Duration::ZERO
        }
    ));

    let repository = TempRepository::new(None);
    repository.commit("lib.rs", "fn bounded() {}\n", "bounded");
    let error = resolve_source_selector(
        repository.root(),
        selector("worktree-head"),
        SourceSelectorLimits::new(Duration::from_secs(1), 0),
        &AtomicBool::new(false),
    )
    .expect_err("zero stdout bound must reject Git output");
    assert!(matches!(
        error,
        SourceSelectorResolutionError::Git {
            source: GitPathDiscoveryError::OutputByteLimitExceeded { limit: 0 }
        }
    ));
}

#[test]
fn final_fence_observes_cancellation_and_absolute_deadline() {
    let repository = TempRepository::new(None);
    repository.commit("lib.rs", "fn controls() {}\n", "controls");
    let resolved = resolve(&repository, "worktree-head").expect("HEAD must resolve");

    let error = resolved
        .confirm(
            TEST_LIMITS,
            &AtomicBool::new(true),
            Instant::now() + TEST_LIMITS.deadline(),
        )
        .expect_err("final fence must observe cancellation");
    assert!(matches!(error, SourceSelectorFinalFenceError::Cancelled));

    let error = resolved
        .confirm(TEST_LIMITS, &AtomicBool::new(false), Instant::now())
        .expect_err("expired absolute deadline must fail");
    assert!(matches!(
        error,
        SourceSelectorFinalFenceError::DeadlineExceeded { .. }
    ));
}

#[test]
fn debug_and_errors_do_not_expose_paths_refs_or_object_ids() {
    let repository = TempRepository::new_named("privacy-root-canary", None);
    let head = repository.commit("lib.rs", "fn private() {}\n", "private");
    let reference = "refs/heads/privacy-ref-canary";
    repository.git(&["update-ref", reference, "HEAD"]);
    let admitted = selector(reference);
    let resolved = resolve(&repository, reference).expect("privacy ref must resolve");

    let combined = format!("{admitted:?}\n{resolved:?}");
    let root_text = repository.root().to_string_lossy().into_owned();
    for secret in [
        "privacy-root-canary",
        "privacy-ref-canary",
        head.as_str(),
        root_text.as_str(),
    ] {
        assert!(!combined.contains(secret));
    }
    assert!(combined.contains("<redacted-path>"));
    assert!(combined.contains("<redacted-selector>"));
    assert!(combined.contains("<redacted-digest>"));

    let missing_text = "refs/heads/privacy-missing-ref-canary";
    let error = resolve(&repository, missing_text).expect_err("missing ref must fail");
    let diagnostic = format!("{error:?}\n{error}");
    assert!(!diagnostic.contains(missing_text));
    assert!(!diagnostic.contains(root_text.as_str()));

    let root_canary = Path::new("privacy-missing-root-canary");
    let error = resolve_source_selector(
        root_canary,
        selector("worktree-head"),
        TEST_LIMITS,
        &AtomicBool::new(false),
    )
    .expect_err("missing root must fail");
    let diagnostic = format!("{error:?}\n{error}");
    assert!(!diagnostic.contains("privacy-missing-root-canary"));
}
