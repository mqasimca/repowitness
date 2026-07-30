//! Bounded repository-path discovery through a sanitized Git subprocess.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use repowitness_domain::{RepositoryPath, RepositoryPathError, RepositoryPathLimits};

use crate::contained_source::{ContainedSourceError, ContainedSourceRoot, ExactReadSessionError};

const POLL_INTERVAL: Duration = Duration::from_millis(5);
const DEFAULT_DEADLINE: Duration = Duration::from_secs(30);
const DEFAULT_OUTPUT_BYTE_LIMIT: u64 = 64 * 1024 * 1024;
const DEFAULT_PATH_LIMIT: u64 = 1_000_000;
const DEFAULT_PATH_BYTE_LIMIT: u64 = 1024 * 1024;
const DEFAULT_PATH_COMPONENT_LIMIT: u64 = 65_535;

#[derive(Clone, Copy)]
enum GitPathDiscoveryScope {
    Cached,
    Untracked,
    Deleted,
}

type CapturedGitPathOutputs = (Vec<u8>, Vec<u8>, Vec<u8>);

/// Resource and time bounds for one Git repository-path discovery operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GitPathDiscoveryLimits {
    deadline: Duration,
    output_bytes: u64,
    paths: u64,
    repository_path: RepositoryPathLimits,
}

impl GitPathDiscoveryLimits {
    /// Creates explicit bounds for one discovery operation.
    #[must_use]
    pub const fn new(
        deadline: Duration,
        output_bytes: u64,
        paths: u64,
        repository_path: RepositoryPathLimits,
    ) -> Self {
        Self {
            deadline,
            output_bytes,
            paths,
            repository_path,
        }
    }

    /// Returns the inclusive wall-clock deadline.
    #[must_use]
    pub const fn deadline(self) -> Duration {
        self.deadline
    }

    /// Returns the inclusive captured-output byte bound.
    #[must_use]
    pub const fn output_bytes(self) -> u64 {
        self.output_bytes
    }

    /// Returns the inclusive repository-path count bound.
    #[must_use]
    pub const fn paths(self) -> u64 {
        self.paths
    }

    /// Returns the per-path identity bounds.
    #[must_use]
    pub const fn repository_path(self) -> RepositoryPathLimits {
        self.repository_path
    }
}

impl Default for GitPathDiscoveryLimits {
    fn default() -> Self {
        Self::new(
            DEFAULT_DEADLINE,
            DEFAULT_OUTPUT_BYTE_LIMIT,
            DEFAULT_PATH_LIMIT,
            RepositoryPathLimits::new(DEFAULT_PATH_BYTE_LIMIT, DEFAULT_PATH_COMPONENT_LIMIT),
        )
    }
}

/// Aggregate, non-sensitive facts about one repository-path discovery result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GitPathDiscoveryStats {
    output_bytes: u64,
    path_count: u64,
    total_path_bytes: u64,
    longest_path_bytes: u64,
    most_components: u64,
}

impl GitPathDiscoveryStats {
    /// Creates aggregate statistics from fixed-width counts.
    #[must_use]
    pub const fn new(
        output_bytes: u64,
        path_count: u64,
        total_path_bytes: u64,
        longest_path_bytes: u64,
        most_components: u64,
    ) -> Self {
        Self {
            output_bytes,
            path_count,
            total_path_bytes,
            longest_path_bytes,
            most_components,
        }
    }

    /// Returns the captured NUL-delimited Git output size.
    #[must_use]
    pub const fn output_bytes(self) -> u64 {
        self.output_bytes
    }

    /// Returns the number of validated repository paths.
    #[must_use]
    pub const fn path_count(self) -> u64 {
        self.path_count
    }

    /// Returns the sum of validated repository-path byte lengths.
    #[must_use]
    pub const fn total_path_bytes(self) -> u64 {
        self.total_path_bytes
    }

    /// Returns the longest validated repository-path byte length.
    #[must_use]
    pub const fn longest_path_bytes(self) -> u64 {
        self.longest_path_bytes
    }

    /// Returns the largest validated repository-path component count.
    #[must_use]
    pub const fn most_components(self) -> u64 {
        self.most_components
    }
}

/// Validated, deterministically ordered paths discovered from one Git worktree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredRepositoryPaths {
    paths: Box<[RepositoryPath]>,
    stats: GitPathDiscoveryStats,
}

impl DiscoveredRepositoryPaths {
    /// Returns the validated paths in unsigned-byte lexicographic order.
    #[must_use]
    pub fn paths(&self) -> &[RepositoryPath] {
        &self.paths
    }

    /// Returns non-sensitive aggregate discovery statistics.
    #[must_use]
    pub const fn stats(&self) -> GitPathDiscoveryStats {
        self.stats
    }

    /// Consumes the result and returns its validated paths.
    #[must_use]
    pub fn into_paths(self) -> Box<[RepositoryPath]> {
        self.paths
    }
}

/// A bounded Git repository-path discovery failure.
#[derive(Debug)]
pub enum GitPathDiscoveryError {
    /// The requested deadline cannot be represented by the monotonic clock.
    DeadlineNotRepresentable,
    /// Discovery was cancelled before it completed.
    Cancelled,
    /// The requested path could not be resolved before repository discovery.
    WorktreeRootResolve {
        /// The underlying filesystem error.
        source: io::Error,
    },
    /// A potential worktree marker could not be inspected safely.
    WorktreeMarkerInspect {
        /// The underlying filesystem error.
        source: io::Error,
    },
    /// A worktree marker was a symlink or special file.
    WorktreeMarkerUnsupported,
    /// No containing Git worktree marker was found.
    WorktreeMarkerNotFound,
    /// Git could not be started.
    GitStart {
        /// The underlying process error.
        source: io::Error,
    },
    /// The Git stdout pipe was unavailable.
    GitStdoutUnavailable,
    /// The bounded output-reader thread could not be started.
    OutputReaderStart {
        /// The underlying thread creation error.
        source: io::Error,
    },
    /// Reading Git output failed.
    GitOutputRead {
        /// The underlying I/O error.
        source: io::Error,
    },
    /// Git output exceeded its declared byte bound.
    OutputByteLimitExceeded {
        /// The inclusive configured bound.
        limit: u64,
    },
    /// Git output length cannot be represented as a fixed-width count.
    OutputByteCountNotRepresentable,
    /// The bounded output-reader thread stopped without returning a result.
    OutputReaderStopped,
    /// The bounded output-reader thread panicked.
    OutputReaderPanicked,
    /// Polling the Git subprocess failed.
    GitPoll {
        /// The underlying process error.
        source: io::Error,
    },
    /// Git did not complete before the configured deadline.
    DeadlineExceeded {
        /// The configured deadline.
        deadline: Duration,
    },
    /// Git exited unsuccessfully.
    GitUnsuccessful {
        /// The platform exit code, when one was available.
        code: Option<i32>,
    },
    /// Non-empty Git output was not terminated by a NUL byte.
    OutputNotNulTerminated,
    /// The repository-path count overflowed its fixed-width representation.
    PathCountOverflowed,
    /// Discovery exceeded its declared repository-path count bound.
    PathLimitExceeded {
        /// The inclusive configured bound.
        limit: u64,
    },
    /// One discovered path failed domain validation.
    InvalidRepositoryPath {
        /// The one-based record position without any path content.
        ordinal: u64,
        /// The redacted domain validation failure.
        source: RepositoryPathError,
    },
    /// Git returned the same repository identity more than once.
    DuplicateRepositoryPath,
    /// Exact worktree path spelling could not be inspected safely.
    RepositoryPathInspection {
        /// The redacted contained-filesystem failure.
        source: ContainedSourceError,
    },
    /// Git and exact worktree path observations could not be reconciled.
    InconsistentRepositoryPathSet,
    /// The aggregate validated path-byte count overflowed.
    TotalPathBytesOverflowed,
}

impl fmt::Display for GitPathDiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeadlineNotRepresentable => {
                formatter.write_str("Git discovery deadline cannot be represented")
            }
            Self::Cancelled => formatter.write_str("Git repository-path discovery was cancelled"),
            Self::WorktreeRootResolve { .. } => {
                formatter.write_str("could not resolve the requested repository path")
            }
            Self::WorktreeMarkerInspect { .. } => {
                formatter.write_str("could not inspect a repository worktree marker")
            }
            Self::WorktreeMarkerUnsupported => {
                formatter.write_str("repository worktree marker type is not supported")
            }
            Self::WorktreeMarkerNotFound => {
                formatter.write_str("no containing Git worktree was found")
            }
            Self::GitStart { .. } => formatter.write_str("could not start Git"),
            Self::GitStdoutUnavailable => formatter.write_str("Git stdout pipe was unavailable"),
            Self::OutputReaderStart { .. } => {
                formatter.write_str("could not start the bounded Git output reader")
            }
            Self::GitOutputRead { .. } => formatter.write_str("could not read bounded Git output"),
            Self::OutputByteLimitExceeded { limit } => {
                write!(formatter, "Git output exceeded its {limit} byte bound")
            }
            Self::OutputByteCountNotRepresentable => {
                formatter.write_str("Git output length cannot be represented")
            }
            Self::OutputReaderStopped => {
                formatter.write_str("the bounded Git output reader stopped unexpectedly")
            }
            Self::OutputReaderPanicked => {
                formatter.write_str("the bounded Git output reader panicked")
            }
            Self::GitPoll { .. } => formatter.write_str("could not poll Git"),
            Self::DeadlineExceeded { deadline } => write!(
                formatter,
                "Git exceeded its {} millisecond deadline",
                deadline.as_millis()
            ),
            Self::GitUnsuccessful { code: Some(code) } => {
                write!(formatter, "Git exited unsuccessfully with code {code}")
            }
            Self::GitUnsuccessful { code: None } => {
                formatter.write_str("Git exited unsuccessfully without an exit code")
            }
            Self::OutputNotNulTerminated => {
                formatter.write_str("Git output was not NUL terminated")
            }
            Self::PathCountOverflowed => formatter.write_str("repository path count overflowed"),
            Self::PathLimitExceeded { limit } => {
                write!(
                    formatter,
                    "repository path count exceeded its {limit} path bound"
                )
            }
            Self::InvalidRepositoryPath { ordinal, source } => {
                write!(
                    formatter,
                    "repository path {ordinal} failed validation: {source}"
                )
            }
            Self::DuplicateRepositoryPath => {
                formatter.write_str("Git returned a duplicate repository path")
            }
            Self::RepositoryPathInspection { .. } => {
                formatter.write_str("repository path spelling could not be inspected safely")
            }
            Self::InconsistentRepositoryPathSet => {
                formatter.write_str("repository path observations were inconsistent")
            }
            Self::TotalPathBytesOverflowed => {
                formatter.write_str("total repository path bytes overflowed")
            }
        }
    }
}

impl std::error::Error for GitPathDiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::WorktreeRootResolve { source }
            | Self::WorktreeMarkerInspect { source }
            | Self::GitStart { source }
            | Self::OutputReaderStart { source }
            | Self::GitOutputRead { source }
            | Self::GitPoll { source } => Some(source),
            Self::RepositoryPathInspection { source } => Some(source),
            Self::InvalidRepositoryPath { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Discovers cached and untracked, non-ignored repository paths through Git.
///
/// Git is invoked without a shell, with prompts, pagers, hooks, fsmonitor,
/// external diff, optional locks, system configuration, and global
/// configuration disabled. Repository paths are captured as NUL-delimited
/// bytes, bounded before validation, validated as domain identities, and
/// returned in deterministic byte order. This function does not ingest
/// discovered file contents for analysis or create an index.
///
/// # Errors
///
/// Returns a typed error for process, deadline, resource-bound, cancellation,
/// malformed-output, duplicate-path, or domain-validation failures.
pub fn discover_repository_paths(
    root: &Path,
    limits: GitPathDiscoveryLimits,
) -> Result<DiscoveredRepositoryPaths, GitPathDiscoveryError> {
    discover_repository_paths_with_cancel(root, limits, || false)
}

/// Discovers repository paths while polling a caller-provided cancellation signal.
///
/// The cancellation callback is checked before process creation and while
/// reading or waiting for Git. Cancellation terminates and reaps the child
/// process before returning.
///
/// # Errors
///
/// Returns the same failures as [`discover_repository_paths`], including
/// [`GitPathDiscoveryError::Cancelled`] when the callback requests
/// cancellation.
pub fn discover_repository_paths_with_cancel(
    root: &Path,
    limits: GitPathDiscoveryLimits,
    mut is_cancelled: impl FnMut() -> bool,
) -> Result<DiscoveredRepositoryPaths, GitPathDiscoveryError> {
    if is_cancelled() {
        return Err(GitPathDiscoveryError::Cancelled);
    }
    if limits.deadline().is_zero() {
        return Err(GitPathDiscoveryError::DeadlineExceeded {
            deadline: limits.deadline(),
        });
    }
    let deadline = Instant::now()
        .checked_add(limits.deadline())
        .ok_or(GitPathDiscoveryError::DeadlineNotRepresentable)?;
    let worktree_root = discovered_worktree_root(root)?;
    let (cached, untracked, deleted) =
        capture_git_path_outputs(&worktree_root, limits, deadline, &mut is_cancelled)?;
    let cached_paths = parse_git_paths_with_control(cached, limits, deadline, &mut is_cancelled)?;
    let untracked_paths =
        parse_git_paths_with_control(untracked, limits, deadline, &mut is_cancelled)?;
    let deleted_paths = parse_git_paths_with_control(deleted, limits, deadline, &mut is_cancelled)?;
    reconcile_repository_paths(
        &worktree_root,
        cached_paths,
        untracked_paths,
        deleted_paths,
        limits,
        deadline,
        &mut is_cancelled,
    )
}

fn capture_git_path_outputs(
    root: &Path,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<CapturedGitPathOutputs, GitPathDiscoveryError> {
    let mut captured_output_bytes = 0_u64;
    let cached = capture_git_scope_output(
        root,
        GitPathDiscoveryScope::Cached,
        limits,
        deadline,
        &mut captured_output_bytes,
        is_cancelled,
    )?;
    let untracked = capture_git_scope_output(
        root,
        GitPathDiscoveryScope::Untracked,
        limits,
        deadline,
        &mut captured_output_bytes,
        is_cancelled,
    )?;
    let deleted = capture_git_scope_output(
        root,
        GitPathDiscoveryScope::Deleted,
        limits,
        deadline,
        &mut captured_output_bytes,
        is_cancelled,
    )?;
    Ok((cached, untracked, deleted))
}

fn capture_git_scope_output(
    worktree_root: &Path,
    scope: GitPathDiscoveryScope,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    captured_output_bytes: &mut u64,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<u8>, GitPathDiscoveryError> {
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
    check_operation_control(deadline, limits.deadline(), is_cancelled)?;
    let command = sanitized_git_command(worktree_root, scope);
    let output = capture_git_output_from_command(command, scoped_limits, deadline, is_cancelled)?;
    let output_bytes = u64::try_from(output.len())
        .map_err(|_| GitPathDiscoveryError::OutputByteCountNotRepresentable)?;
    *captured_output_bytes = captured_output_bytes
        .checked_add(output_bytes)
        .ok_or(GitPathDiscoveryError::OutputByteCountNotRepresentable)?;
    Ok(output)
}

pub(crate) fn capture_git_output_from_command(
    command: Command,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<u8>, GitPathDiscoveryError> {
    let (status, output) =
        capture_git_output_with_status_from_command(command, limits, deadline, is_cancelled)?;
    if !status.success() {
        return Err(GitPathDiscoveryError::GitUnsuccessful {
            code: status.code(),
        });
    }

    Ok(output)
}

pub(crate) fn capture_git_output_with_status_from_command(
    mut command: Command,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(ExitStatus, Vec<u8>), GitPathDiscoveryError> {
    let mut child = command
        .spawn()
        .map_err(|source| GitPathDiscoveryError::GitStart { source })?;
    let Some(stdout) = child.stdout.take() else {
        terminate(&mut child);
        return Err(GitPathDiscoveryError::GitStdoutUnavailable);
    };

    let (sender, receiver) = mpsc::sync_channel(1);
    let output_limit = limits.output_bytes();
    let reader = thread::Builder::new()
        .name("repowitness-bounded-git-output".to_owned())
        .spawn(move || {
            let result = read_bounded(stdout, output_limit);
            let _ = sender.send(result);
        })
        .map_err(|source| {
            terminate(&mut child);
            GitPathDiscoveryError::OutputReaderStart { source }
        })?;

    let mut reader_finished = false;
    let outcome = loop {
        if is_cancelled() {
            terminate(&mut child);
            break Err(GitPathDiscoveryError::Cancelled);
        }

        match receiver.try_recv() {
            Ok(Ok(output)) => {
                reader_finished = true;
                break wait_until_deadline(&mut child, deadline, limits.deadline(), is_cancelled)
                    .map(|status| (status, output));
            }
            Ok(Err(BoundedReadError::Io(source))) => {
                reader_finished = true;
                terminate(&mut child);
                break Err(GitPathDiscoveryError::GitOutputRead { source });
            }
            Ok(Err(BoundedReadError::LimitExceeded)) => {
                reader_finished = true;
                terminate(&mut child);
                break Err(GitPathDiscoveryError::OutputByteLimitExceeded {
                    limit: output_limit,
                });
            }
            Err(TryRecvError::Disconnected) => {
                reader_finished = true;
                terminate(&mut child);
                break Err(GitPathDiscoveryError::OutputReaderStopped);
            }
            Err(TryRecvError::Empty) => {}
        }

        if Instant::now() >= deadline {
            terminate(&mut child);
            break Err(GitPathDiscoveryError::DeadlineExceeded {
                deadline: limits.deadline(),
            });
        }
        sleep_until_next_poll(deadline);
    };

    if reader_finished {
        reader
            .join()
            .map_err(|_| GitPathDiscoveryError::OutputReaderPanicked)?;
    } else {
        // A descendant can inherit the stdout writer after the direct child is
        // reaped. Joining here would let that descendant extend cancellation
        // or the declared deadline. The bounded reader owns no other resource
        // and exits when the final inherited writer closes.
        drop(reader);
    }
    outcome
}

include!("git_paths/reconciliation.rs");
include!("git_paths/process.rs");

#[cfg(test)]
mod tests;
