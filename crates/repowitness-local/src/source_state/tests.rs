use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use repowitness_domain::{RepositoryPathLimits, SourceManifestDigest};

use super::{
    GIT_STATE_VERSION, GIT_STATUS_PROFILE_VERSION, GitObjectFormat, RUST_WORKTREE_STATE_VERSION,
    SUPPORTED_LANGUAGES_WORKTREE_STATE_DOMAIN, SUPPORTED_LANGUAGES_WORKTREE_STATE_VERSION,
    SourceStateError, capture_source_state, capture_source_state_with_cancel, hash_git_state,
    parse_index_scope, parse_object_format, parse_shallow_state, parse_status_records,
};
use crate::GitPathDiscoveryLimits;

static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

const SHA1_ZERO: &str = "0000000000000000000000000000000000000000";
const SHA1_ONE: &str = "1111111111111111111111111111111111111111";
const SHA1_TWO: &str = "2222222222222222222222222222222222222222";
const SHA1_THREE: &str = "3333333333333333333333333333333333333333";

struct TempDirectory {
    root: PathBuf,
}

impl TempDirectory {
    fn new(label: &str) -> Self {
        let ordinal = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "repowitness-source-state-{label}-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("fixture directory should be created");
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct TempRepository {
    directory: TempDirectory,
}

impl TempRepository {
    fn new(object_format: &str) -> Self {
        let directory = TempDirectory::new("repository");
        let repository = Self { directory };
        let format_argument = format!("--object-format={object_format}");
        repository.git(&["init", "--quiet", "--initial-branch=main", &format_argument]);
        repository
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }

    fn write(&self, relative: &str, content: &[u8]) {
        let path = self.path().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent should be created");
        }
        fs::write(path, content).expect("fixture file should be written");
    }

    fn git(&self, arguments: &[&str]) {
        let status = self
            .git_command(arguments)
            .status()
            .expect("Git should start");
        assert!(status.success(), "fixture Git failed: {status}");
    }

    fn git_text(&self, arguments: &[&str]) -> String {
        let output = self
            .git_command(arguments)
            .output()
            .expect("Git should start");
        assert!(output.status.success(), "fixture Git failed");
        String::from_utf8(output.stdout)
            .expect("fixture Git output should be UTF-8")
            .trim()
            .to_owned()
    }

    fn git_command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new("git");
        command
            .arg("--no-pager")
            .arg("-C")
            .arg(self.path())
            .args(arguments)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", null_device())
            .env("GIT_CONFIG_SYSTEM", null_device())
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        command
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
}

fn null_device() -> OsString {
    if cfg!(windows) {
        OsString::from("NUL")
    } else {
        OsString::from("/dev/null")
    }
}

fn limits(paths: u64) -> GitPathDiscoveryLimits {
    GitPathDiscoveryLimits::new(
        Duration::from_secs(1),
        4096,
        paths,
        RepositoryPathLimits::new(1024, 32),
    )
}

fn ordinary(path: &[u8]) -> Vec<u8> {
    let mut record = format!("1 M. N... 100644 100644 100644 {SHA1_ONE} {SHA1_TWO} ").into_bytes();
    record.extend_from_slice(path);
    record
}

#[test]
fn metadata_parsers_are_strict_and_support_both_object_formats() {
    assert_eq!(
        parse_object_format(b"sha1\n").unwrap(),
        GitObjectFormat::Sha1
    );
    assert_eq!(
        parse_object_format(b"sha256\n").unwrap(),
        GitObjectFormat::Sha256
    );
    assert!(matches!(
        parse_object_format(b"blake3\n"),
        Err(SourceStateError::UnsupportedObjectFormat)
    ));
    for malformed in [b"sha1".as_slice(), b"SHA1\n", b"sha1\r\n", b"sha1\nextra\n"] {
        assert!(matches!(
            parse_object_format(malformed),
            Err(SourceStateError::InvalidObjectFormat)
        ));
    }
    assert!(parse_shallow_state(b"true\n").unwrap());
    assert!(!parse_shallow_state(b"false\n").unwrap());
    assert!(matches!(
        parse_shallow_state(b"TRUE\n"),
        Err(SourceStateError::InvalidShallowState)
    ));
}

#[test]
fn git_state_hash_has_stable_vectors_and_every_field_participates() {
    let oid = [0x11; 20];
    let baseline = hash_git_state(GitObjectFormat::Sha1, Some(&oid), false);
    assert_eq!(
        baseline.into_bytes(),
        [
            0x88, 0xE7, 0xF0, 0x98, 0xAA, 0x81, 0xB7, 0xAE, 0xE4, 0xB4, 0xFB, 0x91, 0xE6, 0x97,
            0x86, 0xEA, 0xB9, 0x66, 0x86, 0x60, 0xD3, 0xEB, 0xBD, 0xC3, 0x0A, 0x21, 0xDD, 0x32,
            0x04, 0xEC, 0x2E, 0x0E,
        ]
    );
    assert_ne!(
        baseline,
        hash_git_state(GitObjectFormat::Sha256, Some(&[0x11; 32]), false)
    );
    assert_ne!(baseline, hash_git_state(GitObjectFormat::Sha1, None, false));
    assert_ne!(
        baseline,
        hash_git_state(GitObjectFormat::Sha1, Some(&[0x22; 20]), false)
    );
    assert_ne!(
        baseline,
        hash_git_state(GitObjectFormat::Sha1, Some(&oid), true)
    );
    assert_eq!(GIT_STATE_VERSION, 1);
}

#[test]
fn index_scope_rejects_sparse_gitlinks_and_malformed_records() {
    let ordinary = format!("H 100644 {SHA1_ONE} 0\tlib.rs\0");
    parse_index_scope(ordinary.as_bytes(), GitObjectFormat::Sha1, limits(1)).unwrap();

    let conflict = format!(
        "M 100644 {SHA1_ONE} 1\tlib.rs\0M 100644 {SHA1_TWO} 2\tlib.rs\0M 100644 {SHA1_THREE} 3\tlib.rs\0"
    );
    parse_index_scope(conflict.as_bytes(), GitObjectFormat::Sha1, limits(1))
        .expect("three conflict stages are one bounded repository path");
    let duplicate_stage = format!("M 100644 {SHA1_ONE} 1\tlib.rs\0M 100644 {SHA1_TWO} 1\tlib.rs\0");
    assert!(matches!(
        parse_index_scope(duplicate_stage.as_bytes(), GitObjectFormat::Sha1, limits(1)),
        Err(SourceStateError::InvalidIndexRecord)
    ));

    let sparse = format!("S 040000 {SHA1_ZERO} 0\tsrc\0");
    assert!(matches!(
        parse_index_scope(sparse.as_bytes(), GitObjectFormat::Sha1, limits(1)),
        Err(SourceStateError::SparseWorktreeUnsupported)
    ));
    let gitlink = format!("H 160000 {SHA1_ONE} 0\tdependency\0");
    assert!(matches!(
        parse_index_scope(gitlink.as_bytes(), GitObjectFormat::Sha1, limits(1)),
        Err(SourceStateError::SubmoduleUnsupported)
    ));
    for malformed in [
        format!("H 100644 {SHA1_ONE} 0\tlib.rs"),
        format!("H 100644 {SHA1_ONE}\tlib.rs\0"),
        format!("H 100644 {SHA1_ONE} 4\tlib.rs\0"),
        format!("X 100644 {SHA1_ONE} 0\tlib.rs\0"),
        format!("H 777777 {SHA1_ONE} 0\tlib.rs\0"),
        format!("H 100644 {SHA1_ONE} 0\t/lib.rs\0"),
    ] {
        assert!(parse_index_scope(malformed.as_bytes(), GitObjectFormat::Sha1, limits(1)).is_err());
    }
}

#[test]
fn status_parser_canonicalizes_order_and_preserves_categories() {
    let mut output = Vec::new();
    output.extend_from_slice(b"? z.rs\0");
    output.extend_from_slice(
        format!(
            "u UU N... 100644 100644 100644 100644 {SHA1_ONE} {SHA1_TWO} {SHA1_THREE} conflict.rs\0"
        )
        .as_bytes(),
    );
    output.extend_from_slice(&ordinary(b"a path.rs"));
    output.push(0);

    let parsed = parse_status_records(&output, GitObjectFormat::Sha1, limits(3)).unwrap();
    assert_eq!(parsed.len(), 3);
    assert_eq!(parsed[0].path.as_bytes(), b"a path.rs");
    assert_eq!(parsed[0].tag, 1);
    assert_eq!(parsed[1].path.as_bytes(), b"conflict.rs");
    assert_eq!(parsed[1].tag, 2);
    assert_eq!(parsed[2].path.as_bytes(), b"z.rs");
    assert_eq!(parsed[2].tag, 3);
}

#[test]
fn worktree_hash_is_stable_and_every_component_participates() {
    let mut record = ordinary(b"src/lib.rs");
    record.push(0);
    let parsed = parse_status_records(&record, GitObjectFormat::Sha1, limits(1)).unwrap();
    let baseline = super::hash_worktree_state(&parsed, SourceManifestDigest::new([0x44; 32]));
    assert_eq!(
        baseline.into_bytes(),
        [
            0x8B, 0x82, 0x11, 0x5D, 0x8C, 0xA1, 0x32, 0xA8, 0xBD, 0x7E, 0x72, 0x60, 0x09, 0xC4,
            0x55, 0xFF, 0x76, 0x37, 0x94, 0x91, 0x58, 0x7F, 0xE5, 0xB7, 0xB4, 0xAE, 0xE0, 0xA7,
            0xC7, 0x89, 0x0E, 0xED,
        ]
    );
    assert_ne!(
        baseline,
        super::hash_worktree_state(&parsed, SourceManifestDigest::new([0x55; 32]))
    );

    let mut changed_status = ordinary(b"src/lib.rs");
    changed_status[2] = b'A';
    changed_status.push(0);
    let changed = parse_status_records(&changed_status, GitObjectFormat::Sha1, limits(1)).unwrap();
    assert_ne!(
        baseline,
        super::hash_worktree_state(&changed, SourceManifestDigest::new([0x44; 32]))
    );
    let supported_languages = super::hash_worktree_state_with_profile(
        &parsed,
        SourceManifestDigest::new([0x44; 32]),
        SUPPORTED_LANGUAGES_WORKTREE_STATE_DOMAIN,
        SUPPORTED_LANGUAGES_WORKTREE_STATE_VERSION,
    );
    assert_ne!(baseline, supported_languages);
    assert_eq!(RUST_WORKTREE_STATE_VERSION, 1);
    assert_eq!(SUPPORTED_LANGUAGES_WORKTREE_STATE_VERSION, 3);
    assert_eq!(GIT_STATUS_PROFILE_VERSION, 1);
}

#[test]
fn status_parser_rejects_unknown_malformed_duplicate_and_over_limit_records() {
    let mut duplicate = ordinary(b"same.rs");
    duplicate.push(0);
    duplicate.extend_from_slice(b"? same.rs\0");
    assert!(matches!(
        parse_status_records(&duplicate, GitObjectFormat::Sha1, limits(2)),
        Err(SourceStateError::DuplicateStatusPath)
    ));

    for malformed in [
        b"2 R. N... malformed\0".as_slice(),
        b"# branch.head main\0",
        b"! ignored.rs\0",
        b"? /absolute.rs\0",
        b"? unterminated",
        b"? one.rs\0\0",
    ] {
        assert!(parse_status_records(malformed, GitObjectFormat::Sha1, limits(2)).is_err());
    }
    assert!(matches!(
        parse_status_records(b"? one.rs\0? two.rs\0", GitObjectFormat::Sha1, limits(1)),
        Err(SourceStateError::StatusPathLimitExceeded { limit: 1 })
    ));
}

#[test]
fn status_parser_preserves_non_utf8_and_rejects_submodule_status() {
    let output = [b'?', b' ', b'n', b'o', b'n', b'-', 0xFF, 0];
    let parsed = parse_status_records(&output, GitObjectFormat::Sha1, limits(1)).unwrap();
    assert_eq!(parsed[0].path.as_bytes(), &output[2..7]);

    let submodule = format!("1 M. S.M. 160000 160000 160000 {SHA1_ONE} {SHA1_TWO} dependency\0");
    assert!(matches!(
        parse_status_records(submodule.as_bytes(), GitObjectFormat::Sha1, limits(1)),
        Err(SourceStateError::SubmoduleUnsupported)
    ));
    let hidden_gitlink =
        format!("1 M. N... 160000 160000 160000 {SHA1_ONE} {SHA1_TWO} dependency\0");
    assert!(matches!(
        parse_status_records(hidden_gitlink.as_bytes(), GitObjectFormat::Sha1, limits(1)),
        Err(SourceStateError::SubmoduleUnsupported)
    ));
    let invalid_mode = format!("1 M. N... 777777 100644 100644 {SHA1_ONE} {SHA1_TWO} source.rs\0");
    assert!(matches!(
        parse_status_records(invalid_mode.as_bytes(), GitObjectFormat::Sha1, limits(1)),
        Err(SourceStateError::InvalidStatusRecord)
    ));
}

#[test]
fn diagnostics_and_debug_are_redacted() {
    let error = parse_status_records(
        b"? /secret/repository/path.rs\0",
        GitObjectFormat::Sha1,
        limits(1),
    )
    .unwrap_err();
    let display = error.to_string();
    let debug = format!("{error:?}");
    assert!(!display.contains("secret"));
    assert!(!debug.contains("secret"));

    let parsed = parse_status_records(b"? private.rs\0", GitObjectFormat::Sha1, limits(1))
        .expect("valid status");
    let capture = super::CapturedSourceState {
        git_state: hash_git_state(GitObjectFormat::Sha1, None, false),
        status_records: parsed,
    };
    let debug = format!("{capture:?}");
    assert!(!debug.contains("private.rs"));
    assert_eq!(capture.status_record_count(), 1);
}

#[test]
fn real_capture_supports_unborn_symbolic_detached_and_linked_states() {
    let repository = TempRepository::new("sha1");
    repository.write("src/lib.rs", b"pub struct Visible;\n");

    let unborn =
        capture_source_state(repository.path(), limits(10)).expect("unborn state should work");
    assert_eq!(unborn.status_record_count(), 1);

    repository.commit_all("initial");
    let symbolic =
        capture_source_state(repository.path(), limits(10)).expect("symbolic HEAD should work");
    assert_ne!(symbolic.git_state(), unborn.git_state());
    assert_eq!(symbolic.status_record_count(), 0);

    repository.git(&["checkout", "--quiet", "--detach", "HEAD"]);
    let detached =
        capture_source_state(repository.path(), limits(10)).expect("detached HEAD should work");
    assert_eq!(detached, symbolic);

    let linked = TempDirectory::new("linked-worktree");
    repository.git(&[
        "worktree",
        "add",
        "--quiet",
        "--detach",
        linked
            .path()
            .to_str()
            .expect("fixture path should be UTF-8"),
        "HEAD",
    ]);
    let linked_state =
        capture_source_state(linked.path(), limits(10)).expect("linked worktree should work");
    assert_eq!(linked_state, detached);
}

#[test]
fn real_capture_supports_sha256_and_fails_closed_on_sparse_and_gitlinks() {
    let sha256 = TempRepository::new("sha256");
    sha256.write("lib.rs", b"pub fn sha256() {}\n");
    sha256.commit_all("sha256");
    capture_source_state(sha256.path(), limits(10)).expect("SHA-256 repository should work");

    let sparse = TempRepository::new("sha1");
    sparse.write("lib.rs", b"pub fn sparse() {}\n");
    sparse.commit_all("sparse");
    sparse.git(&["update-index", "--skip-worktree", "lib.rs"]);
    assert!(matches!(
        capture_source_state(sparse.path(), limits(10)),
        Err(SourceStateError::SparseWorktreeUnsupported)
    ));

    let gitlink = TempRepository::new("sha1");
    gitlink.write("lib.rs", b"pub fn gitlink() {}\n");
    gitlink.commit_all("gitlink");
    let head = gitlink.git_text(&["rev-parse", "HEAD"]);
    let cache_info = format!("160000,{head},dependency");
    gitlink.git(&["update-index", "--add", "--cacheinfo", &cache_info]);
    assert!(matches!(
        capture_source_state(gitlink.path(), limits(10)),
        Err(SourceStateError::SubmoduleUnsupported)
    ));
}

#[test]
fn real_capture_distinguishes_shallow_history_at_the_same_head() {
    let origin = TempRepository::new("sha1");
    origin.write("lib.rs", b"pub fn first() {}\n");
    origin.commit_all("first");
    origin.write("lib.rs", b"pub fn second() {}\n");
    origin.commit_all("second");

    let shallow = TempDirectory::new("shallow-clone");
    let origin_url = format!(
        "file://{}",
        origin
            .path()
            .to_str()
            .expect("fixture path should be UTF-8")
    );
    let status = Command::new("git")
        .arg("--no-pager")
        .arg("clone")
        .arg("--quiet")
        .arg("--depth=1")
        .arg(origin_url)
        .arg(shallow.path())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("fixture clone should start");
    assert!(status.success(), "fixture clone failed: {status}");

    let origin_state =
        capture_source_state(origin.path(), limits(10)).expect("origin should capture");
    let shallow_state =
        capture_source_state(shallow.path(), limits(10)).expect("shallow clone should capture");
    assert_ne!(origin_state.git_state(), shallow_state.git_state());
    assert_eq!(origin_state.status_record_count(), 0);
    assert_eq!(shallow_state.status_record_count(), 0);
}

#[cfg(unix)]
#[cfg_attr(
    target_vendor = "apple",
    ignore = "Apple filesystems reject the byte-invalid fixture name"
)]
#[test]
fn real_capture_preserves_non_utf8_untracked_path_identity() {
    use std::os::unix::ffi::OsStringExt;

    let repository = TempRepository::new("sha1");
    let path = repository
        .path()
        .join(OsString::from_vec(b"non-utf8-\xFF.rs".to_vec()));
    fs::write(path, b"pub fn non_utf8() {}\n").expect("fixture file should be written");

    let capture = capture_source_state(repository.path(), limits(10))
        .expect("non-UTF-8 status path should capture");
    assert_eq!(capture.status_record_count(), 1);
}

#[test]
fn real_capture_enforces_cancellation_output_and_path_bounds() {
    let repository = TempRepository::new("sha1");
    repository.write("one.rs", b"fn one() {}\n");
    repository.write("two.rs", b"fn two() {}\n");

    assert!(matches!(
        capture_source_state_with_cancel(repository.path(), limits(10), || true),
        Err(SourceStateError::Git {
            source: crate::GitPathDiscoveryError::Cancelled
        })
    ));

    let output_limited = GitPathDiscoveryLimits::new(
        Duration::from_secs(1),
        1,
        10,
        RepositoryPathLimits::new(1024, 32),
    );
    assert!(matches!(
        capture_source_state(repository.path(), output_limited),
        Err(SourceStateError::Git {
            source: crate::GitPathDiscoveryError::OutputByteLimitExceeded { limit: 1 }
        })
    ));
    assert!(matches!(
        capture_source_state(repository.path(), limits(1)),
        Err(SourceStateError::StatusPathLimitExceeded { limit: 1 })
    ));
}
