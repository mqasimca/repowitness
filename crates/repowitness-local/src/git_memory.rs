//! Bounded, sanitized Git queries used by memory revalidation.

use std::{
    error::Error,
    fmt,
    fmt::Write as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use repowitness_domain::{
    MemoryAncestryOutcome, MemoryCommitId, MemoryObjectFormat, RepositoryPath, RepositoryPathLimits,
};

use crate::git_paths::{
    GitPathDiscoveryError, GitPathDiscoveryLimits, capture_git_output_with_status_from_command,
    sanitized_git_base_command,
};

const MAX_GIT_MEMORY_OUTPUT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_GIT_MEMORY_DIFF_RECORDS: u32 = 65_536;
const MAX_GIT_MEMORY_COMMAND_DEADLINE: Duration = Duration::from_secs(30);
const DEFAULT_GIT_MEMORY_OUTPUT_BYTES: u64 = 1024 * 1024;
const DEFAULT_GIT_MEMORY_DIFF_RECORDS: u32 = 4_096;
const DEFAULT_GIT_MEMORY_COMMAND_DEADLINE: Duration = Duration::from_secs(5);
const GIT_MEMORY_PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(1_048_576, 65_535);

/// Explicit resource limits for one bounded Git memory query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GitMemoryQueryLimits {
    command_deadline: Duration,
    output_bytes: u64,
    diff_records: u32,
}

impl GitMemoryQueryLimits {
    /// Constructs nonzero limits no larger than the local hard ceilings.
    pub fn try_new(
        command_deadline: Duration,
        output_bytes: u64,
        diff_records: u32,
    ) -> Result<Self, GitMemoryQueryError> {
        if command_deadline.is_zero()
            || command_deadline > MAX_GIT_MEMORY_COMMAND_DEADLINE
            || output_bytes == 0
            || output_bytes > MAX_GIT_MEMORY_OUTPUT_BYTES
            || diff_records == 0
            || diff_records > MAX_GIT_MEMORY_DIFF_RECORDS
        {
            return Err(GitMemoryQueryError::InvalidLimits);
        }
        Ok(Self {
            command_deadline,
            output_bytes,
            diff_records,
        })
    }

    /// Returns the per-command wall-clock limit.
    #[must_use]
    pub const fn command_deadline(self) -> Duration {
        self.command_deadline
    }

    /// Returns the captured stdout byte limit.
    #[must_use]
    pub const fn output_bytes(self) -> u64 {
        self.output_bytes
    }

    /// Returns the parsed diff-record limit.
    #[must_use]
    pub const fn diff_records(self) -> u32 {
        self.diff_records
    }

    fn discovery_limits(self) -> GitPathDiscoveryLimits {
        GitPathDiscoveryLimits::new(
            self.command_deadline,
            self.output_bytes,
            u64::from(self.diff_records),
            GIT_MEMORY_PATH_LIMITS,
        )
    }
}

impl Default for GitMemoryQueryLimits {
    fn default() -> Self {
        Self {
            command_deadline: DEFAULT_GIT_MEMORY_COMMAND_DEADLINE,
            output_bytes: DEFAULT_GIT_MEMORY_OUTPUT_BYTES,
            diff_records: DEFAULT_GIT_MEMORY_DIFF_RECORDS,
        }
    }
}

/// Categorical result for an exact Git path-continuity query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitPathContinuityOutcome {
    /// Exactly one 100%-similar rename maps the old path to the target path.
    ExactMove,
    /// Complete bounded history found no exact move.
    NoMatch,
    /// Repository history or complete bounded output was unavailable.
    Indeterminate,
}

/// Stable, content-redacted failure from Git memory inspection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitMemoryQueryError {
    /// Configured limits are zero or exceed a hard ceiling.
    InvalidLimits,
    /// The operation deadline cannot be represented.
    DeadlineNotRepresentable,
    /// Cancellation was observed.
    Cancelled,
    /// The operation deadline elapsed.
    DeadlineExceeded,
}

impl fmt::Display for GitMemoryQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLimits => "Git memory query limits are invalid",
            Self::DeadlineNotRepresentable => "Git memory query deadline is not representable",
            Self::Cancelled => "Git memory query cancelled",
            Self::DeadlineExceeded => "Git memory query deadline exceeded",
        })
    }
}

impl Error for GitMemoryQueryError {}

/// Reusable sanitized Git query adapter for one concrete worktree.
pub struct GitMemoryQueries {
    worktree_root: PathBuf,
    object_format: Option<MemoryObjectFormat>,
    limits: GitMemoryQueryLimits,
}

impl GitMemoryQueries {
    /// Opens one query adapter. Unavailable or malformed repository metadata is
    /// retained as explicit indeterminate coverage rather than guessed.
    pub fn open(
        worktree_root: &Path,
        limits: GitMemoryQueryLimits,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<Self, GitMemoryQueryError> {
        check_control(cancelled, deadline)?;
        let mut command = sanitized_git_base_command(worktree_root);
        command.arg("rev-parse").arg("--show-object-format");
        let object_format =
            capture(command, limits, cancelled, deadline)?.and_then(|(status, output)| {
                if !status.success() {
                    return None;
                }
                match output.as_slice() {
                    b"sha1\n" => Some(MemoryObjectFormat::Sha1),
                    b"sha256\n" => Some(MemoryObjectFormat::Sha256),
                    _ => None,
                }
            });
        Ok(Self {
            worktree_root: worktree_root.to_path_buf(),
            object_format,
            limits,
        })
    }

    /// Returns the exact current commit at `HEAD`, or `None` when the
    /// repository is unborn or its bounded metadata cannot be established.
    pub fn head_commit(
        &self,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<Option<MemoryCommitId>, GitMemoryQueryError> {
        check_control(cancelled, deadline)?;
        let Some(object_format) = self.object_format else {
            return Ok(None);
        };
        let mut command = sanitized_git_base_command(&self.worktree_root);
        command
            .arg("rev-parse")
            .arg("--verify")
            .arg("--quiet")
            .arg("HEAD^{commit}");
        let Some((status, output)) = capture(command, self.limits, cancelled, deadline)? else {
            return Ok(None);
        };
        if !status.success() {
            return Ok(None);
        }
        Ok(parse_commit_output(object_format, &output))
    }

    /// Checks whether one exact commit is an ancestor of another.
    pub fn is_ancestor(
        &self,
        ancestor: MemoryCommitId,
        target: MemoryCommitId,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<MemoryAncestryOutcome, GitMemoryQueryError> {
        check_control(cancelled, deadline)?;
        if self.object_format != Some(ancestor.object_format())
            || ancestor.object_format() != target.object_format()
        {
            return Ok(MemoryAncestryOutcome::Indeterminate);
        }
        let mut command = sanitized_git_base_command(&self.worktree_root);
        command
            .arg("merge-base")
            .arg("--is-ancestor")
            .arg(commit_hex(ancestor))
            .arg(commit_hex(target));
        let Some((status, output)) = capture(command, self.limits, cancelled, deadline)? else {
            return Ok(MemoryAncestryOutcome::Indeterminate);
        };
        if !output.is_empty() {
            return Ok(MemoryAncestryOutcome::Indeterminate);
        }
        Ok(match status.code() {
            Some(0) => MemoryAncestryOutcome::Ancestor,
            Some(1) => MemoryAncestryOutcome::NotAncestor,
            _ => MemoryAncestryOutcome::Indeterminate,
        })
    }

    /// Checks for one exact 100%-similar Git rename between two commits.
    #[allow(
        clippy::too_many_arguments,
        reason = "both exact commits and paths plus control are semantic query inputs"
    )]
    pub fn exact_path_continuity(
        &self,
        source_commit: MemoryCommitId,
        target_commit: MemoryCommitId,
        source_path: &RepositoryPath,
        target_path: &RepositoryPath,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<GitPathContinuityOutcome, GitMemoryQueryError> {
        check_control(cancelled, deadline)?;
        if source_path == target_path {
            return Ok(GitPathContinuityOutcome::NoMatch);
        }
        if self.object_format != Some(source_commit.object_format())
            || source_commit.object_format() != target_commit.object_format()
        {
            return Ok(GitPathContinuityOutcome::Indeterminate);
        }
        let mut command = sanitized_git_base_command(&self.worktree_root);
        command
            .arg("diff-tree")
            .arg("--no-commit-id")
            .arg("--name-status")
            .arg("-z")
            .arg("-r")
            .arg("--find-renames=100%")
            .arg("--diff-filter=R")
            .arg("--no-ext-diff")
            .arg("--no-textconv")
            .arg(commit_hex(source_commit))
            .arg(commit_hex(target_commit));
        let Some((status, output)) = capture(command, self.limits, cancelled, deadline)? else {
            return Ok(GitPathContinuityOutcome::Indeterminate);
        };
        if !status.success() {
            return Ok(GitPathContinuityOutcome::Indeterminate);
        }
        Ok(parse_exact_move(
            &output,
            source_path,
            target_path,
            self.limits.diff_records,
        ))
    }
}

impl fmt::Debug for GitMemoryQueries {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GitMemoryQueries")
            .field("worktree_root", &"<redacted-path>")
            .field("object_format", &self.object_format)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

fn capture(
    command: std::process::Command,
    limits: GitMemoryQueryLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Option<(std::process::ExitStatus, Vec<u8>)>, GitMemoryQueryError> {
    check_control(cancelled, deadline)?;
    let command_deadline = Instant::now()
        .checked_add(limits.command_deadline)
        .ok_or(GitMemoryQueryError::DeadlineNotRepresentable)?
        .min(deadline);
    let mut is_cancelled = || cancelled.load(Ordering::Acquire);
    match capture_git_output_with_status_from_command(
        command,
        limits.discovery_limits(),
        command_deadline,
        &mut is_cancelled,
    ) {
        Ok(result) => Ok(Some(result)),
        Err(GitPathDiscoveryError::Cancelled) => Err(GitMemoryQueryError::Cancelled),
        Err(GitPathDiscoveryError::DeadlineExceeded { .. }) => {
            Err(GitMemoryQueryError::DeadlineExceeded)
        }
        Err(_) => Ok(None),
    }
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), GitMemoryQueryError> {
    if cancelled.load(Ordering::Acquire) {
        Err(GitMemoryQueryError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(GitMemoryQueryError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

fn commit_hex(commit: MemoryCommitId) -> String {
    let mut encoded = String::with_capacity(commit.as_bytes().len() * 2);
    for byte in commit.as_bytes() {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

fn parse_commit_output(object_format: MemoryObjectFormat, output: &[u8]) -> Option<MemoryCommitId> {
    let expected_hex_bytes = match object_format {
        MemoryObjectFormat::Sha1 => 40,
        MemoryObjectFormat::Sha256 => 64,
    };
    if output.len() != expected_hex_bytes + 1 || output.last() != Some(&b'\n') {
        return None;
    }
    let mut decoded = [0_u8; 32];
    for (index, pair) in output[..expected_hex_bytes].chunks_exact(2).enumerate() {
        decoded[index] = decode_hex_pair(pair)?;
    }
    Some(match object_format {
        MemoryObjectFormat::Sha1 => {
            let mut sha1 = [0_u8; 20];
            sha1.copy_from_slice(&decoded[..20]);
            MemoryCommitId::Sha1(sha1)
        }
        MemoryObjectFormat::Sha256 => MemoryCommitId::Sha256(decoded),
    })
}

fn decode_hex_pair(pair: &[u8]) -> Option<u8> {
    let digit = |value| match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    };
    Some(digit(*pair.first()?)? * 16 + digit(*pair.get(1)?)?)
}

fn parse_exact_move(
    output: &[u8],
    source_path: &RepositoryPath,
    target_path: &RepositoryPath,
    max_records: u32,
) -> GitPathContinuityOutcome {
    if output.is_empty() {
        return GitPathContinuityOutcome::NoMatch;
    }
    if !output.ends_with(&[0]) {
        return GitPathContinuityOutcome::Indeterminate;
    }
    let mut fields = output[..output.len() - 1].split(|byte| *byte == 0);
    let mut records = 0_u32;
    let mut exact_matches = 0_u8;
    while let Some(status) = fields.next() {
        records = match records.checked_add(1) {
            Some(records) if records <= max_records => records,
            _ => return GitPathContinuityOutcome::Indeterminate,
        };
        let Some(old_path) = fields.next() else {
            return GitPathContinuityOutcome::Indeterminate;
        };
        let Some(new_path) = fields.next() else {
            return GitPathContinuityOutcome::Indeterminate;
        };
        if RepositoryPath::try_from_bytes(old_path, GIT_MEMORY_PATH_LIMITS).is_err()
            || RepositoryPath::try_from_bytes(new_path, GIT_MEMORY_PATH_LIMITS).is_err()
        {
            return GitPathContinuityOutcome::Indeterminate;
        }
        if status == b"R100"
            && old_path == source_path.as_bytes()
            && new_path == target_path.as_bytes()
        {
            exact_matches = match exact_matches.checked_add(1) {
                Some(matches) => matches,
                None => return GitPathContinuityOutcome::Indeterminate,
            };
        }
    }
    match exact_matches {
        0 => GitPathContinuityOutcome::NoMatch,
        1 => GitPathContinuityOutcome::ExactMove,
        _ => GitPathContinuityOutcome::Indeterminate,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::Path,
        process::Command,
        sync::atomic::{AtomicBool, AtomicU64, Ordering},
        time::{Duration, Instant},
    };

    use repowitness_domain::{MemoryCommitId, RepositoryPath, RepositoryPathLimits};

    use super::{
        GitMemoryQueries, GitMemoryQueryError, GitMemoryQueryLimits, GitPathContinuityOutcome,
        MemoryAncestryOutcome,
    };

    const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(4_096, 256);
    static NEXT_TEMP_REPOSITORY: AtomicU64 = AtomicU64::new(0);

    struct TempRepository(std::path::PathBuf);

    impl TempRepository {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "repowitness-git-memory-{}-{}",
                std::process::id(),
                NEXT_TEMP_REPOSITORY.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("fixture directory");
            git(&path, &["init", "-q", "--object-format=sha1"]);
            git(&path, &["config", "user.name", "RepoWitness"]);
            git(
                &path,
                &[
                    "config",
                    "user.email",
                    "repowitness.invalid@example.invalid",
                ],
            );
            Self(path)
        }

        fn commit(&self, message: &str) -> MemoryCommitId {
            git(&self.0, &["add", "."]);
            git(&self.0, &["commit", "-q", "-m", message]);
            let output = Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(["rev-parse", "HEAD"])
                .output()
                .expect("Git should run");
            assert!(output.status.success());
            let text = std::str::from_utf8(&output.stdout)
                .expect("fixture ID should be UTF-8")
                .trim();
            assert_eq!(text.len(), 40, "fixture should use SHA-1 object IDs");
            let mut bytes = [0_u8; 20];
            for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
                bytes[index] = decode_hex(pair);
            }
            MemoryCommitId::Sha1(bytes)
        }
    }

    impl Drop for TempRepository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git(root: &Path, arguments: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(arguments)
            .status()
            .expect("Git should run");
        assert!(status.success());
    }

    fn decode_hex(pair: &[u8]) -> u8 {
        fn digit(value: u8) -> u8 {
            match value {
                b'0'..=b'9' => value - b'0',
                b'a'..=b'f' => value - b'a' + 10,
                _ => panic!("fixture object ID must be lowercase hexadecimal"),
            }
        }
        digit(pair[0]) * 16 + digit(pair[1])
    }

    fn deadline() -> Instant {
        Instant::now()
            .checked_add(Duration::from_secs(5))
            .expect("test deadline")
    }

    fn path(value: &str) -> RepositoryPath {
        RepositoryPath::try_from_bytes(value.as_bytes(), PATH_LIMITS).expect("fixture path")
    }

    #[test]
    fn ancestry_and_exact_rename_are_categorical() {
        let repository = TempRepository::new();
        fs::write(repository.0.join("old.rs"), b"fn kept() {}\n").expect("fixture source");
        let first = repository.commit("first");
        fs::rename(repository.0.join("old.rs"), repository.0.join("new.rs"))
            .expect("fixture rename");
        let second = repository.commit("rename");
        let cancelled = AtomicBool::new(false);
        let queries = GitMemoryQueries::open(
            &repository.0,
            GitMemoryQueryLimits::default(),
            &cancelled,
            deadline(),
        )
        .expect("query adapter");

        assert_eq!(
            queries
                .head_commit(&cancelled, deadline())
                .expect("HEAD query"),
            Some(second)
        );
        assert_eq!(
            queries
                .is_ancestor(first, second, &cancelled, deadline())
                .expect("ancestry"),
            MemoryAncestryOutcome::Ancestor
        );
        assert_eq!(
            queries
                .is_ancestor(second, first, &cancelled, deadline())
                .expect("ancestry"),
            MemoryAncestryOutcome::NotAncestor
        );
        assert_eq!(
            queries
                .exact_path_continuity(
                    first,
                    second,
                    &path("old.rs"),
                    &path("new.rs"),
                    &cancelled,
                    deadline(),
                )
                .expect("continuity"),
            GitPathContinuityOutcome::ExactMove
        );
        assert_eq!(
            queries
                .exact_path_continuity(
                    first,
                    second,
                    &path("old.rs"),
                    &path("other.rs"),
                    &cancelled,
                    deadline(),
                )
                .expect("continuity"),
            GitPathContinuityOutcome::NoMatch
        );
    }

    #[test]
    fn missing_history_formats_and_control_are_never_guessed() {
        let repository = TempRepository::new();
        fs::write(repository.0.join("a.rs"), b"fn a() {}\n").expect("fixture source");
        let head = repository.commit("first");
        let cancelled = AtomicBool::new(false);
        let queries = GitMemoryQueries::open(
            &repository.0,
            GitMemoryQueryLimits::default(),
            &cancelled,
            deadline(),
        )
        .expect("query adapter");

        assert_eq!(
            queries
                .is_ancestor(
                    MemoryCommitId::Sha1([0xFF; 20]),
                    head,
                    &cancelled,
                    deadline(),
                )
                .expect("missing object"),
            MemoryAncestryOutcome::Indeterminate
        );
        assert_eq!(
            queries
                .is_ancestor(
                    MemoryCommitId::Sha256([0x11; 32]),
                    MemoryCommitId::Sha256([0x22; 32]),
                    &cancelled,
                    deadline(),
                )
                .expect("format mismatch"),
            MemoryAncestryOutcome::Indeterminate
        );
        cancelled.store(true, std::sync::atomic::Ordering::Release);
        assert_eq!(
            queries.is_ancestor(head, head, &cancelled, deadline()),
            Err(GitMemoryQueryError::Cancelled)
        );
    }

    #[test]
    fn limits_and_debug_output_are_redacted() {
        assert_eq!(
            GitMemoryQueryLimits::try_new(Duration::ZERO, 1, 1),
            Err(GitMemoryQueryError::InvalidLimits)
        );
        let repository = TempRepository::new();
        let cancelled = AtomicBool::new(false);
        let queries = GitMemoryQueries::open(
            &repository.0,
            GitMemoryQueryLimits::default(),
            &cancelled,
            deadline(),
        )
        .expect("query adapter");
        assert_eq!(
            queries
                .head_commit(&cancelled, deadline())
                .expect("unborn HEAD query"),
            None
        );
        let debug = format!("{queries:?}");
        assert!(!debug.contains(repository.0.to_string_lossy().as_ref()));
        assert!(debug.contains("<redacted-path>"));
    }
}
