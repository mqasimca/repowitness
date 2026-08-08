//! Bounded, revision-pinned Git change manifests for read-only receipts.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::time::{Duration, Instant};

use repowitness_application::{
    ChangeKind, ChangeManifest, ChangeManifestEntry, ChangeManifestError,
};
use repowitness_domain::{GitObjectId, RepositoryPath, RepositoryPathError};
use sha2::{Digest, Sha256};

use crate::git_paths::{
    GitPathDiscoveryError, GitPathDiscoveryLimits, capture_git_output_from_command,
    discovered_worktree_root, sanitized_git_base_command,
};

/// Default wall-clock deadline for a local revision-pinned change comparison.
pub const DEFAULT_LOCAL_CHANGE_MANIFEST_DEADLINE: Duration = Duration::from_secs(30);

const TRACKED_DIFF_FINGERPRINT_DOMAIN: &[u8] = b"RepoWitness\0change-manifest-tracked-diff\0";

/// A bounded, deterministic comparison of an exact base commit and worktree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalChangeManifest {
    manifest: ChangeManifest,
    tracked_diff_fingerprint: [u8; 32],
    captured_output_bytes: u64,
}

impl LocalChangeManifest {
    /// Returns the immutable base commit resolved by the local Git repository.
    #[must_use]
    pub const fn base(&self) -> &GitObjectId {
        self.manifest.base()
    }

    /// Returns all changed paths in deterministic unsigned-byte path order.
    #[must_use]
    pub fn entries(&self) -> &[ChangeManifestEntry] {
        self.manifest.entries()
    }

    /// Returns the number of paths in the comparison manifest.
    #[must_use]
    pub fn path_count(&self) -> u64 {
        self.manifest.path_count()
    }

    /// Returns total Git output captured while deriving this manifest.
    #[must_use]
    pub const fn captured_output_bytes(&self) -> u64 {
        self.captured_output_bytes
    }

    /// Returns whether two captures observed the same exact tracked base-to-worktree diff.
    ///
    /// The fingerprint stays local and opaque: it prevents a status-preserving
    /// content change from passing a source fence without exposing patch bytes.
    #[must_use]
    pub(crate) fn same_tracked_diff(&self, other: &Self) -> bool {
        self.tracked_diff_fingerprint == other.tracked_diff_fingerprint
    }

    /// Consumes the local capture and returns its application-level manifest.
    #[must_use]
    pub fn into_manifest(self) -> ChangeManifest {
        self.manifest
    }
}

/// Bounded execution limits for a local change manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalChangeManifestLimits {
    git: GitPathDiscoveryLimits,
}

impl LocalChangeManifestLimits {
    /// Creates limits using the same bounded Git/path contract as local indexing.
    #[must_use]
    pub const fn new(git: GitPathDiscoveryLimits) -> Self {
        Self { git }
    }

    /// Returns the underlying bounded Git and repository-path limits.
    #[must_use]
    pub const fn git(self) -> GitPathDiscoveryLimits {
        self.git
    }
}

impl Default for LocalChangeManifestLimits {
    fn default() -> Self {
        Self::new(GitPathDiscoveryLimits::default())
    }
}

/// A bounded local change-manifest failure.
#[derive(Debug)]
pub enum LocalChangeManifestError {
    /// The configured deadline could not be represented by the monotonic clock.
    DeadlineNotRepresentable,
    /// A sanitized Git operation failed.
    Git {
        /// The redacted bounded Git failure.
        source: GitPathDiscoveryError,
    },
    /// Git resolved the supplied full object ID to something else.
    ResolvedBaseMismatch,
    /// Git returned a malformed resolved base object identifier.
    InvalidResolvedBase,
    /// Git returned a malformed raw-diff record.
    InvalidDiffRecord,
    /// Git returned a raw-diff status outside this version's supported scope.
    UnsupportedDiffStatus,
    /// One changed path failed the repository-path identity contract.
    InvalidRepositoryPath {
        /// The one-based record position without path content.
        ordinal: u64,
        /// The redacted domain validation failure.
        source: RepositoryPathError,
    },
    /// More paths than the declared inclusive bound were returned.
    PathLimitExceeded {
        /// The inclusive configured bound.
        limit: u64,
    },
    /// Git returned the same path from incompatible comparison scopes.
    DuplicateChangePath,
    /// A fixed-width path count could not represent the result.
    PathCountNotRepresentable,
    /// Aggregate captured Git output could not be represented.
    CapturedOutputBytesNotRepresentable,
    /// Locally derived entries violated the application receipt contract.
    InvalidManifest {
        /// The application-level validation failure.
        source: ChangeManifestError,
    },
}

impl fmt::Display for LocalChangeManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeadlineNotRepresentable => {
                formatter.write_str("change-manifest deadline cannot be represented")
            }
            Self::Git { .. } => formatter.write_str("sanitized Git change comparison failed"),
            Self::ResolvedBaseMismatch => {
                formatter.write_str("Git resolved the supplied base to a different object")
            }
            Self::InvalidResolvedBase => {
                formatter.write_str("Git returned an invalid resolved base object identifier")
            }
            Self::InvalidDiffRecord => {
                formatter.write_str("Git returned an invalid raw diff record")
            }
            Self::UnsupportedDiffStatus => {
                formatter.write_str("Git returned an unsupported raw diff status")
            }
            Self::InvalidRepositoryPath { ordinal, source } => {
                write!(
                    formatter,
                    "change path {ordinal} failed validation: {source}"
                )
            }
            Self::PathLimitExceeded { limit } => {
                write!(formatter, "change manifest exceeded its {limit} path bound")
            }
            Self::DuplicateChangePath => {
                formatter.write_str("Git returned duplicate paths in the change manifest")
            }
            Self::PathCountNotRepresentable => {
                formatter.write_str("change-manifest path count cannot be represented")
            }
            Self::CapturedOutputBytesNotRepresentable => {
                formatter.write_str("captured Git output bytes cannot be represented")
            }
            Self::InvalidManifest { .. } => {
                formatter.write_str("locally derived change manifest was inconsistent")
            }
        }
    }
}

impl Error for LocalChangeManifestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Git { source } => Some(source),
            Self::InvalidRepositoryPath { source, .. } => Some(source),
            Self::InvalidManifest { source } => Some(source),
            _ => None,
        }
    }
}

impl From<GitPathDiscoveryError> for LocalChangeManifestError {
    fn from(source: GitPathDiscoveryError) -> Self {
        Self::Git { source }
    }
}

/// Captures a deterministic local change manifest from `base` to the worktree.
///
/// The caller supplies a validated, complete object identifier rather than a
/// ref or revision expression. Git independently verifies it resolves to a
/// commit before the adapter derives raw tracked changes and non-ignored
/// untracked paths. Raw patch bytes are never accepted from the caller.
///
/// # Errors
///
/// Returns a typed error for bounded Git execution, cancellation, malformed
/// Git output, unsupported change kinds, or invalid repository paths.
pub fn capture_local_change_manifest(
    root: &Path,
    base: GitObjectId,
    limits: LocalChangeManifestLimits,
) -> Result<LocalChangeManifest, LocalChangeManifestError> {
    capture_local_change_manifest_with_cancel(root, base, limits, || false)
}

/// Captures a local change manifest while polling a caller-provided cancellation signal.
///
/// # Errors
///
/// Returns the same failures as [`capture_local_change_manifest`], including a
/// wrapped cancellation failure when the callback requests cancellation.
pub fn capture_local_change_manifest_with_cancel(
    root: &Path,
    base: GitObjectId,
    limits: LocalChangeManifestLimits,
    mut is_cancelled: impl FnMut() -> bool,
) -> Result<LocalChangeManifest, LocalChangeManifestError> {
    let git_limits = limits.git();
    if git_limits.deadline().is_zero() {
        return Err(GitPathDiscoveryError::DeadlineExceeded {
            deadline: git_limits.deadline(),
        }
        .into());
    }
    let deadline = Instant::now()
        .checked_add(git_limits.deadline())
        .ok_or(LocalChangeManifestError::DeadlineNotRepresentable)?;
    let worktree_root = discovered_worktree_root(root)?;
    let base_text = base.to_hex();
    let mut captured_output_bytes = 0_u64;

    let resolved_base = capture_output(
        &worktree_root,
        git_limits,
        deadline,
        &mut captured_output_bytes,
        &mut is_cancelled,
        |command| {
            command
                .arg("rev-parse")
                .arg("--verify")
                .arg("--quiet")
                .arg("--end-of-options")
                .arg(format!("{base_text}^{{commit}}"));
        },
    )?;
    let resolved_base = parse_resolved_base(&resolved_base)?;
    if resolved_base != base {
        return Err(LocalChangeManifestError::ResolvedBaseMismatch);
    }

    let raw_diff = capture_output(
        &worktree_root,
        git_limits,
        deadline,
        &mut captured_output_bytes,
        &mut is_cancelled,
        |command| {
            command
                .arg("diff")
                .arg("--raw")
                .arg("-z")
                .arg("--no-abbrev")
                .arg("--no-ext-diff")
                .arg("--no-renames")
                .arg(&base_text)
                .arg("--");
        },
    )?;
    let mut changes = parse_raw_diff(&raw_diff, git_limits, deadline, &mut is_cancelled)?;

    let tracked_diff_fingerprint = capture_tracked_diff_fingerprint(
        &worktree_root,
        git_limits,
        deadline,
        &mut captured_output_bytes,
        &mut is_cancelled,
        &base_text,
    )?;

    let untracked = capture_output(
        &worktree_root,
        git_limits,
        deadline,
        &mut captured_output_bytes,
        &mut is_cancelled,
        |command| {
            command
                .arg("ls-files")
                .arg("-z")
                .arg("--full-name")
                .arg("--others")
                .arg("--exclude-standard");
        },
    )?;
    add_untracked_paths(
        &mut changes,
        &untracked,
        git_limits,
        deadline,
        &mut is_cancelled,
    )?;

    let entries = changes
        .into_iter()
        .map(|(path, kind)| ChangeManifestEntry::new(path, kind))
        .collect::<Vec<_>>();
    let manifest = ChangeManifest::try_new(base, entries)
        .map_err(|source| LocalChangeManifestError::InvalidManifest { source })?;
    Ok(LocalChangeManifest {
        manifest,
        tracked_diff_fingerprint,
        captured_output_bytes,
    })
}

fn hash_tracked_diff(diff: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(TRACKED_DIFF_FINGERPRINT_DOMAIN);
    hasher.update(diff);
    hasher.finalize().into()
}

fn capture_tracked_diff_fingerprint(
    worktree_root: &Path,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    captured_output_bytes: &mut u64,
    is_cancelled: &mut impl FnMut() -> bool,
    base: &str,
) -> Result<[u8; 32], LocalChangeManifestError> {
    let diff = capture_output(
        worktree_root,
        limits,
        deadline,
        captured_output_bytes,
        is_cancelled,
        |command| {
            command
                .arg("diff")
                .arg("--binary")
                .arg("--full-index")
                .arg("--no-ext-diff")
                .arg("--no-textconv")
                .arg("--no-renames")
                .arg(base)
                .arg("--");
        },
    )?;
    Ok(hash_tracked_diff(&diff))
}

fn capture_output(
    worktree_root: &Path,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    captured_output_bytes: &mut u64,
    is_cancelled: &mut impl FnMut() -> bool,
    configure: impl FnOnce(&mut std::process::Command),
) -> Result<Vec<u8>, LocalChangeManifestError> {
    let remaining_output_bytes = limits
        .output_bytes()
        .checked_sub(*captured_output_bytes)
        .ok_or(GitPathDiscoveryError::OutputByteLimitExceeded {
            limit: limits.output_bytes(),
        })?;
    let scoped_limits = GitPathDiscoveryLimits::new(
        limits.deadline(),
        remaining_output_bytes,
        limits.paths(),
        limits.repository_path(),
    );
    let mut command = sanitized_git_base_command(worktree_root);
    configure(&mut command);
    let output = capture_git_output_from_command(command, scoped_limits, deadline, is_cancelled)?;
    let output_bytes = u64::try_from(output.len())
        .map_err(|_| LocalChangeManifestError::CapturedOutputBytesNotRepresentable)?;
    *captured_output_bytes = captured_output_bytes
        .checked_add(output_bytes)
        .ok_or(LocalChangeManifestError::CapturedOutputBytesNotRepresentable)?;
    Ok(output)
}

fn parse_resolved_base(output: &[u8]) -> Result<GitObjectId, LocalChangeManifestError> {
    let value = output
        .strip_suffix(b"\n")
        .filter(|value| !value.contains(&b'\n'))
        .ok_or(LocalChangeManifestError::InvalidResolvedBase)?;
    let value =
        std::str::from_utf8(value).map_err(|_| LocalChangeManifestError::InvalidResolvedBase)?;
    GitObjectId::try_from_hex(value).map_err(|_| LocalChangeManifestError::InvalidResolvedBase)
}

fn parse_raw_diff(
    output: &[u8],
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<BTreeMap<RepositoryPath, ChangeKind>, LocalChangeManifestError> {
    let mut changes = BTreeMap::new();
    let mut remaining = output;
    while !remaining.is_empty() {
        check_control(deadline, limits, is_cancelled)?;
        let (header, after_header) = take_nul_record(remaining)?;
        let (path_bytes, after_path) = take_nul_record(after_header)?;
        remaining = after_path;
        let kind = parse_raw_diff_header(header)?;
        let ordinal = next_ordinal(&changes)?;
        let path = RepositoryPath::try_from_bytes(path_bytes, limits.repository_path()).map_err(
            |source| LocalChangeManifestError::InvalidRepositoryPath { ordinal, source },
        )?;
        insert_change(&mut changes, path, kind, limits)?;
    }
    Ok(changes)
}

fn add_untracked_paths(
    changes: &mut BTreeMap<RepositoryPath, ChangeKind>,
    output: &[u8],
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(), LocalChangeManifestError> {
    if output.is_empty() {
        return Ok(());
    }
    let output = output
        .strip_suffix(&[0])
        .ok_or(LocalChangeManifestError::InvalidDiffRecord)?;
    for path_bytes in output.split(|byte| *byte == 0) {
        check_control(deadline, limits, is_cancelled)?;
        let ordinal = next_ordinal(changes)?;
        let path = RepositoryPath::try_from_bytes(path_bytes, limits.repository_path()).map_err(
            |source| LocalChangeManifestError::InvalidRepositoryPath { ordinal, source },
        )?;
        insert_change(changes, path, ChangeKind::Untracked, limits)?;
    }
    Ok(())
}

fn take_nul_record(input: &[u8]) -> Result<(&[u8], &[u8]), LocalChangeManifestError> {
    let Some(index) = input.iter().position(|byte| *byte == 0) else {
        return Err(LocalChangeManifestError::InvalidDiffRecord);
    };
    Ok((&input[..index], &input[index + 1..]))
}

fn parse_raw_diff_header(header: &[u8]) -> Result<ChangeKind, LocalChangeManifestError> {
    let Some(header) = header.strip_prefix(b":") else {
        return Err(LocalChangeManifestError::InvalidDiffRecord);
    };
    let mut fields = header.split(|byte| *byte == b' ');
    let (Some(old_mode), Some(new_mode), Some(old_id), Some(new_id), Some(status), None) = (
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
    ) else {
        return Err(LocalChangeManifestError::InvalidDiffRecord);
    };
    if !is_git_mode(old_mode)
        || !is_git_mode(new_mode)
        || !is_full_lower_hex_object_id(old_id)
        || !is_full_lower_hex_object_id(new_id)
    {
        return Err(LocalChangeManifestError::InvalidDiffRecord);
    }
    match status {
        b"A" => Ok(ChangeKind::Added),
        b"M" => Ok(ChangeKind::Modified),
        b"D" => Ok(ChangeKind::Deleted),
        b"T" => Ok(ChangeKind::TypeChanged),
        _ => Err(LocalChangeManifestError::UnsupportedDiffStatus),
    }
}

fn is_git_mode(value: &[u8]) -> bool {
    value.len() == 6 && value.iter().all(|byte| matches!(byte, b'0'..=b'7'))
}

fn is_full_lower_hex_object_id(value: &[u8]) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn insert_change(
    changes: &mut BTreeMap<RepositoryPath, ChangeKind>,
    path: RepositoryPath,
    kind: ChangeKind,
    limits: GitPathDiscoveryLimits,
) -> Result<(), LocalChangeManifestError> {
    if changes.contains_key(&path) {
        return Err(LocalChangeManifestError::DuplicateChangePath);
    }
    let path_count = u64::try_from(changes.len())
        .map_err(|_| LocalChangeManifestError::PathCountNotRepresentable)?;
    let next_count = path_count
        .checked_add(1)
        .ok_or(LocalChangeManifestError::PathCountNotRepresentable)?;
    if next_count > limits.paths() {
        return Err(LocalChangeManifestError::PathLimitExceeded {
            limit: limits.paths(),
        });
    }
    changes.insert(path, kind);
    Ok(())
}

fn next_ordinal(
    changes: &BTreeMap<RepositoryPath, ChangeKind>,
) -> Result<u64, LocalChangeManifestError> {
    u64::try_from(changes.len())
        .map_err(|_| LocalChangeManifestError::PathCountNotRepresentable)?
        .checked_add(1)
        .ok_or(LocalChangeManifestError::PathCountNotRepresentable)
}

fn check_control(
    deadline: Instant,
    limits: GitPathDiscoveryLimits,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(), LocalChangeManifestError> {
    if is_cancelled() {
        return Err(GitPathDiscoveryError::Cancelled.into());
    }
    if Instant::now() >= deadline {
        return Err(GitPathDiscoveryError::DeadlineExceeded {
            deadline: limits.deadline(),
        }
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use repowitness_domain::{GitObjectId, RepositoryPathLimits};

    use super::{
        ChangeKind, LocalChangeManifestError, LocalChangeManifestLimits,
        capture_local_change_manifest, parse_raw_diff,
    };
    use crate::GitPathDiscoveryLimits;

    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    struct TempRepository {
        root: PathBuf,
    }

    impl TempRepository {
        fn new() -> Self {
            let ordinal = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "repowitness-change-manifest-{}-{ordinal}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("fixture directory should be created");
            let repository = Self { root };
            repository.git(&["init", "--quiet", "--initial-branch=main"]);
            repository
        }

        fn path(&self) -> &Path {
            &self.root
        }

        fn write(&self, relative: &str, content: &[u8]) {
            fs::write(self.path().join(relative), content).expect("fixture file should be written");
        }

        fn git(&self, arguments: &[&str]) {
            let status = self
                .git_command(arguments)
                .status()
                .expect("fixture Git should start");
            assert!(status.success(), "fixture Git failed: {status}");
        }

        fn git_text(&self, arguments: &[&str]) -> String {
            let output = self
                .git_command(arguments)
                .output()
                .expect("fixture Git should start");
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
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            command
        }

        fn commit_all(&self) {
            self.git(&["add", "--all"]);
            self.git(&[
                "-c",
                "user.name=RepoWitness Test",
                "-c",
                "user.email=repowitness@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "base",
            ]);
        }
    }

    impl Drop for TempRepository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn null_device() -> &'static str {
        if cfg!(windows) { "NUL" } else { "/dev/null" }
    }

    fn limits(paths: u64) -> LocalChangeManifestLimits {
        LocalChangeManifestLimits::new(GitPathDiscoveryLimits::new(
            Duration::from_secs(5),
            4096,
            paths,
            RepositoryPathLimits::new(1024, 32),
        ))
    }

    #[test]
    fn raw_diff_parser_is_strict_and_orders_paths() {
        let zero = "0".repeat(40);
        let one = "1".repeat(40);
        let output =
            format!(":100644 100644 {one} {one} M\0z.rs\0:000000 100644 {zero} {one} A\0a.rs\0");
        let parsed = parse_raw_diff(
            output.as_bytes(),
            limits(2).git(),
            std::time::Instant::now() + Duration::from_secs(1),
            &mut || false,
        )
        .expect("valid raw diff should parse");
        let entries = parsed.into_iter().collect::<Vec<_>>();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0.as_bytes(), b"a.rs");
        assert_eq!(entries[0].1, ChangeKind::Added);
        assert_eq!(entries[1].0.as_bytes(), b"z.rs");
        assert_eq!(entries[1].1, ChangeKind::Modified);

        assert!(matches!(
            parse_raw_diff(
                b":100644 100644 111 111 M\0bad.rs\0",
                limits(1).git(),
                std::time::Instant::now() + Duration::from_secs(1),
                &mut || false,
            ),
            Err(LocalChangeManifestError::InvalidDiffRecord)
        ));
    }

    #[test]
    fn rejects_unresolved_base_before_deriving_a_manifest() {
        let base = GitObjectId::try_from_hex(&"1".repeat(40)).expect("base is syntactically valid");
        let directory = std::env::temp_dir().join(format!(
            "repowitness-change-manifest-not-a-repo-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("fixture directory should be created");
        let result = capture_local_change_manifest(&directory, base, limits(1));
        let _ = std::fs::remove_dir_all(&directory);
        assert!(result.is_err());
    }

    #[test]
    fn derives_tracked_and_untracked_changes_from_a_real_worktree() {
        let repository = TempRepository::new();
        repository.write("tracked.rs", b"before\n");
        repository.commit_all();
        let base = GitObjectId::try_from_hex(&repository.git_text(&["rev-parse", "HEAD"]))
            .expect("Git should return a full canonical id");
        repository.write("tracked.rs", b"after\n");
        repository.write("new.py", b"print('new')\n");

        let manifest = capture_local_change_manifest(repository.path(), base.clone(), limits(2))
            .expect("real change manifest should succeed");
        assert_eq!(manifest.base(), &base);
        assert_eq!(manifest.path_count(), 2);
        let entries = manifest.entries();
        assert_eq!(entries[0].path().as_bytes(), b"new.py");
        assert_eq!(entries[0].kind(), ChangeKind::Untracked);
        assert_eq!(entries[1].path().as_bytes(), b"tracked.rs");
        assert_eq!(entries[1].kind(), ChangeKind::Modified);
    }

    #[test]
    fn fingerprint_detects_a_status_preserving_tracked_content_change() {
        let repository = TempRepository::new();
        repository.write("tracked.rs", b"before\n");
        repository.commit_all();
        let base = GitObjectId::try_from_hex(&repository.git_text(&["rev-parse", "HEAD"]))
            .expect("Git should return a full canonical id");
        repository.write("tracked.rs", b"first change\n");
        let first = capture_local_change_manifest(repository.path(), base.clone(), limits(1))
            .expect("first manifest should succeed");

        repository.write("tracked.rs", b"second change\n");
        let second = capture_local_change_manifest(repository.path(), base, limits(1))
            .expect("second manifest should succeed");

        assert_eq!(first.entries(), second.entries());
        assert!(!first.same_tracked_diff(&second));
    }
}
