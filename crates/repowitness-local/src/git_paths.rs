//! Bounded repository-path discovery through a sanitized Git subprocess.

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

const POLL_INTERVAL: Duration = Duration::from_millis(5);
const DEFAULT_DEADLINE: Duration = Duration::from_secs(30);
const DEFAULT_OUTPUT_BYTE_LIMIT: u64 = 64 * 1024 * 1024;
const DEFAULT_PATH_LIMIT: u64 = 1_000_000;
const DEFAULT_PATH_BYTE_LIMIT: u64 = 1024 * 1024;
const DEFAULT_PATH_COMPONENT_LIMIT: u64 = 65_535;

#[derive(Clone, Copy)]
enum GitPathDiscoveryScope {
    #[cfg(test)]
    Cached,
    CachedAndUntracked,
}

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
    let output = capture_git_output(root, limits, deadline, &mut is_cancelled)?;
    parse_git_paths_with_control(output, limits, deadline, &mut is_cancelled)
}

fn capture_git_output(
    root: &Path,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<u8>, GitPathDiscoveryError> {
    let worktree_root = discovered_worktree_root(root)?;
    check_operation_control(deadline, limits.deadline(), is_cancelled)?;
    let command = sanitized_git_command(&worktree_root, GitPathDiscoveryScope::CachedAndUntracked);
    capture_git_output_from_command(command, limits, deadline, is_cancelled)
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

fn sanitized_git_command(worktree_root: &Path, scope: GitPathDiscoveryScope) -> Command {
    let mut command = sanitized_git_base_command(worktree_root);
    command
        .arg("ls-files")
        .arg("-z")
        .arg("--full-name")
        .arg("--cached")
        .arg("--deduplicate");
    if matches!(scope, GitPathDiscoveryScope::CachedAndUntracked) {
        command.arg("--others").arg("--exclude-standard");
    }
    command
}

pub(crate) fn sanitized_git_base_command(worktree_root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .arg("--literal-pathspecs")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg(format!("core.hooksPath={}", null_device()))
        .arg("-c")
        .arg(format!("core.excludesFile={}", null_device()))
        .arg("-c")
        .arg("core.untrackedCache=false")
        .arg("-c")
        .arg("diff.external=")
        .arg("-c")
        .arg("pager.ls-files=false")
        .arg("-c")
        .arg("pager.rev-parse=false")
        .arg("-c")
        .arg("pager.status=false")
        .arg(worktree_argument(worktree_root))
        .arg("-c")
        .arg("core.bare=false")
        .arg("-C")
        .arg(worktree_root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_SHALLOW_FILE")
        .env_remove("GIT_REPLACE_REF_BASE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_NAMESPACE")
        .env_remove("GIT_IMPLICIT_WORK_TREE")
        .env_remove("GIT_CEILING_DIRECTORIES")
        .env_remove("GIT_DISCOVERY_ACROSS_FILESYSTEM")
        .env_remove("GIT_REFERENCE_BACKEND")
        .env_remove("GIT_LITERAL_PATHSPECS")
        .env_remove("GIT_GLOB_PATHSPECS")
        .env_remove("GIT_NOGLOB_PATHSPECS")
        .env_remove("GIT_ICASE_PATHSPECS")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_EXTERNAL_DIFF")
        .env_remove("GIT_FLUSH")
        .env_remove("GIT_TRACE")
        .env_remove("GIT_TRACE_CURL")
        .env_remove("GIT_TRACE_CURL_NO_DATA")
        .env_remove("GIT_TRACE_FSMONITOR")
        .env_remove("GIT_TRACE_PACKET")
        .env_remove("GIT_TRACE_PACK_ACCESS")
        .env_remove("GIT_TRACE_PACKFILE")
        .env_remove("GIT_TRACE_PERFORMANCE")
        .env_remove("GIT_TRACE_REDACT")
        .env_remove("GIT_TRACE_REFS")
        .env_remove("GIT_TRACE_SETUP")
        .env_remove("GIT_TRACE_SHALLOW")
        .env_remove("GIT_TRACE2")
        .env_remove("GIT_TRACE2_BRIEF")
        .env_remove("GIT_TRACE2_CONFIG_PARAMS")
        .env_remove("GIT_TRACE2_DST_DEBUG")
        .env_remove("GIT_TRACE2_ENV_VARS")
        .env_remove("GIT_TRACE2_EVENT")
        .env_remove("GIT_TRACE2_EVENT_BRIEF")
        .env_remove("GIT_TRACE2_EVENT_NESTING")
        .env_remove("GIT_TRACE2_MAX_FILES")
        .env_remove("GIT_TRACE2_PARENT_SID")
        .env_remove("GIT_TRACE2_PERF")
        .env_remove("GIT_TRACE2_PERF_BRIEF")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}

pub(crate) fn discovered_worktree_root(root: &Path) -> Result<PathBuf, GitPathDiscoveryError> {
    let mut current = fs::canonicalize(root)
        .map_err(|source| GitPathDiscoveryError::WorktreeRootResolve { source })?;
    loop {
        match fs::symlink_metadata(current.join(".git")) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(GitPathDiscoveryError::WorktreeMarkerUnsupported);
            }
            Ok(metadata) if metadata.is_dir() || metadata.is_file() => return Ok(current),
            Ok(_) => return Err(GitPathDiscoveryError::WorktreeMarkerUnsupported),
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(GitPathDiscoveryError::WorktreeMarkerInspect { source });
            }
        }
        if !current.pop() {
            return Err(GitPathDiscoveryError::WorktreeMarkerNotFound);
        }
    }
}

fn worktree_argument(worktree: &Path) -> OsString {
    let mut argument = OsString::from("--work-tree=");
    argument.push(worktree.as_os_str());
    argument
}

#[cfg(unix)]
const fn null_device() -> &'static str {
    "/dev/null"
}

#[cfg(windows)]
const fn null_device() -> &'static str {
    "NUL"
}

fn wait_until_deadline(
    child: &mut Child,
    deadline: Instant,
    configured_deadline: Duration,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<ExitStatus, GitPathDiscoveryError> {
    loop {
        if is_cancelled() {
            terminate(child);
            return Err(GitPathDiscoveryError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(source) => {
                terminate(child);
                return Err(GitPathDiscoveryError::GitPoll { source });
            }
        }
        if Instant::now() >= deadline {
            terminate(child);
            return Err(GitPathDiscoveryError::DeadlineExceeded {
                deadline: configured_deadline,
            });
        }
        sleep_until_next_poll(deadline);
    }
}

fn sleep_until_next_poll(deadline: Instant) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    thread::sleep(POLL_INTERVAL.min(remaining));
}

fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[derive(Debug)]
enum BoundedReadError {
    Io(io::Error),
    LimitExceeded,
}

fn read_bounded(mut reader: impl Read, limit: u64) -> Result<Vec<u8>, BoundedReadError> {
    let capacity = usize::try_from(limit.min(64 * 1024)).unwrap_or(64 * 1024);
    let mut output = Vec::with_capacity(capacity);
    let mut buffer = [0_u8; 8 * 1024];
    let mut total = 0_u64;

    loop {
        let read_count = reader.read(&mut buffer).map_err(BoundedReadError::Io)?;
        if read_count == 0 {
            return Ok(output);
        }
        let read_count = u64::try_from(read_count)
            .map_err(|_| BoundedReadError::Io(io::Error::other("read length overflowed u64")))?;
        total = total
            .checked_add(read_count)
            .ok_or_else(|| BoundedReadError::Io(io::Error::other("read length overflowed u64")))?;
        if total > limit {
            return Err(BoundedReadError::LimitExceeded);
        }
        let read_count = usize::try_from(read_count)
            .map_err(|_| BoundedReadError::Io(io::Error::other("read length overflowed usize")))?;
        output.extend_from_slice(&buffer[..read_count]);
    }
}

#[cfg(test)]
fn parse_git_paths(
    output: Vec<u8>,
    limits: GitPathDiscoveryLimits,
) -> Result<DiscoveredRepositoryPaths, GitPathDiscoveryError> {
    let deadline = Instant::now()
        .checked_add(limits.deadline())
        .ok_or(GitPathDiscoveryError::DeadlineNotRepresentable)?;
    parse_git_paths_with_control(output, limits, deadline, &mut || false)
}

fn parse_git_paths_with_control(
    output: Vec<u8>,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<DiscoveredRepositoryPaths, GitPathDiscoveryError> {
    check_operation_control(deadline, limits.deadline(), is_cancelled)?;
    let output_bytes = u64::try_from(output.len())
        .map_err(|_| GitPathDiscoveryError::OutputByteCountNotRepresentable)?;
    if output_bytes > limits.output_bytes() {
        return Err(GitPathDiscoveryError::OutputByteLimitExceeded {
            limit: limits.output_bytes(),
        });
    }
    if output.is_empty() {
        return Ok(DiscoveredRepositoryPaths {
            paths: Box::new([]),
            stats: GitPathDiscoveryStats::new(0, 0, 0, 0, 0),
        });
    }

    let path_bytes = output
        .strip_suffix(&[0])
        .ok_or(GitPathDiscoveryError::OutputNotNulTerminated)?;
    let initial_capacity = usize::try_from(limits.paths().min(4096)).unwrap_or(4096);
    let mut paths = Vec::with_capacity(initial_capacity);
    let mut stats = GitPathDiscoveryStats::new(output_bytes, 0, 0, 0, 0);

    for raw_path in path_bytes.split(|byte| *byte == 0) {
        check_operation_control(deadline, limits.deadline(), is_cancelled)?;
        stats.path_count = stats
            .path_count
            .checked_add(1)
            .ok_or(GitPathDiscoveryError::PathCountOverflowed)?;
        if stats.path_count > limits.paths() {
            return Err(GitPathDiscoveryError::PathLimitExceeded {
                limit: limits.paths(),
            });
        }

        let path = RepositoryPath::try_from_bytes(raw_path, limits.repository_path()).map_err(
            |source| GitPathDiscoveryError::InvalidRepositoryPath {
                ordinal: stats.path_count,
                source,
            },
        )?;
        stats.total_path_bytes = stats
            .total_path_bytes
            .checked_add(path.byte_count().get())
            .ok_or(GitPathDiscoveryError::TotalPathBytesOverflowed)?;
        stats.longest_path_bytes = stats.longest_path_bytes.max(path.byte_count().get());
        stats.most_components = stats.most_components.max(path.component_count().get());
        paths.push(path);
    }

    check_operation_control(deadline, limits.deadline(), is_cancelled)?;
    paths.sort_unstable();
    check_operation_control(deadline, limits.deadline(), is_cancelled)?;
    if paths.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(GitPathDiscoveryError::DuplicateRepositoryPath);
    }

    Ok(DiscoveredRepositoryPaths {
        paths: paths.into_boxed_slice(),
        stats,
    })
}

fn check_operation_control(
    deadline: Instant,
    configured_deadline: Duration,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(), GitPathDiscoveryError> {
    if is_cancelled() {
        return Err(GitPathDiscoveryError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(GitPathDiscoveryError::DeadlineExceeded {
            deadline: configured_deadline,
        });
    }
    Ok(())
}

#[cfg(test)]
mod gix_spike_tests;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::{OsStr, OsString};
    use std::io::Cursor;

    use super::*;

    const TEST_LIMITS: GitPathDiscoveryLimits = GitPathDiscoveryLimits::new(
        Duration::from_secs(1),
        128,
        4,
        RepositoryPathLimits::new(32, 4),
    );

    #[test]
    fn default_limits_are_explicit_and_stable() {
        let limits = GitPathDiscoveryLimits::default();
        assert_eq!(limits.deadline(), Duration::from_secs(30));
        assert_eq!(limits.output_bytes(), 64 * 1024 * 1024);
        assert_eq!(limits.paths(), 1_000_000);
        assert_eq!(
            limits.repository_path(),
            RepositoryPathLimits::new(1024 * 1024, 65_535)
        );
    }

    #[test]
    fn parses_valid_paths_into_deterministic_order_and_stats() {
        let discovered = parse_git_paths(b"src/lib.rs\0Cargo.toml\0".to_vec(), TEST_LIMITS)
            .expect("valid Git output must pass");
        assert_eq!(
            discovered
                .paths()
                .iter()
                .map(RepositoryPath::as_bytes)
                .collect::<Vec<_>>(),
            [b"Cargo.toml".as_slice(), b"src/lib.rs".as_slice()]
        );
        assert_eq!(
            discovered.stats(),
            GitPathDiscoveryStats {
                output_bytes: 22,
                path_count: 2,
                total_path_bytes: 20,
                longest_path_bytes: 10,
                most_components: 2,
            }
        );
        let owned_paths = discovered.clone().into_paths();
        assert_eq!(owned_paths.as_ref(), discovered.paths());
    }

    #[test]
    fn accepts_an_empty_repository() {
        let discovered =
            parse_git_paths(Vec::new(), TEST_LIMITS).expect("empty output is canonical");
        assert!(discovered.paths().is_empty());
        assert_eq!(
            discovered.stats(),
            GitPathDiscoveryStats {
                output_bytes: 0,
                path_count: 0,
                total_path_bytes: 0,
                longest_path_bytes: 0,
                most_components: 0,
            }
        );
    }

    #[test]
    fn parsing_has_cooperative_cancellation_and_deadline_diagnostics() {
        let mut checks = 0_u8;
        let error = parse_git_paths_with_control(
            b"a\0b\0".to_vec(),
            TEST_LIMITS,
            Instant::now() + TEST_LIMITS.deadline(),
            &mut || {
                checks += 1;
                checks >= 2
            },
        )
        .expect_err("parsing cancellation must be observed between records");
        assert!(matches!(error, GitPathDiscoveryError::Cancelled));

        let mut cancelled = || false;
        let error = parse_git_paths_with_control(
            b"a\0".to_vec(),
            TEST_LIMITS,
            Instant::now(),
            &mut cancelled,
        )
        .expect_err("an expired parse deadline must fail");
        assert!(matches!(
            error,
            GitPathDiscoveryError::DeadlineExceeded {
                deadline
            } if deadline == TEST_LIMITS.deadline()
        ));
    }

    #[test]
    fn enforces_output_and_path_count_bounds() {
        let output_limited = GitPathDiscoveryLimits::new(
            Duration::from_secs(1),
            1,
            4,
            RepositoryPathLimits::new(32, 4),
        );
        assert!(matches!(
            parse_git_paths(b"a\0".to_vec(), output_limited),
            Err(GitPathDiscoveryError::OutputByteLimitExceeded { limit: 1 })
        ));

        let path_limited = GitPathDiscoveryLimits::new(
            Duration::from_secs(1),
            128,
            1,
            RepositoryPathLimits::new(32, 4),
        );
        assert!(matches!(
            parse_git_paths(b"a\0b\0".to_vec(), path_limited),
            Err(GitPathDiscoveryError::PathLimitExceeded { limit: 1 })
        ));
    }

    #[test]
    fn rejects_unterminated_invalid_and_duplicate_paths_without_exposing_bytes() {
        assert!(matches!(
            parse_git_paths(b"src/lib.rs".to_vec(), TEST_LIMITS),
            Err(GitPathDiscoveryError::OutputNotNulTerminated)
        ));

        let error = parse_git_paths(b"secret/../value\0".to_vec(), TEST_LIMITS)
            .expect_err("invalid path must fail");
        assert!(matches!(
            error,
            GitPathDiscoveryError::InvalidRepositoryPath { ordinal: 1, .. }
        ));
        let diagnostic = error.to_string();
        assert!(!diagnostic.contains("secret"));
        assert!(!diagnostic.contains("value"));

        assert!(matches!(
            parse_git_paths(b"same\0same\0".to_vec(), TEST_LIMITS),
            Err(GitPathDiscoveryError::DuplicateRepositoryPath)
        ));
    }

    #[test]
    fn bounded_reader_accepts_exact_limit_and_rejects_one_more_byte() {
        assert_eq!(
            read_bounded(Cursor::new(b"abcd"), 4).expect("exact limit must pass"),
            b"abcd"
        );
        assert!(matches!(
            read_bounded(Cursor::new(b"abcde"), 4),
            Err(BoundedReadError::LimitExceeded)
        ));
    }

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("intentional test failure"))
        }
    }

    #[test]
    fn bounded_reader_preserves_io_failures() {
        let error = read_bounded(FailingReader, 4).expect_err("I/O failure must propagate");
        assert!(matches!(error, BoundedReadError::Io(_)));
    }

    #[test]
    fn cancellation_before_spawn_is_deterministic() {
        let error = discover_repository_paths_with_cancel(
            Path::new("does-not-need-to-exist"),
            TEST_LIMITS,
            || true,
        )
        .expect_err("pre-cancelled discovery must fail");
        assert!(matches!(error, GitPathDiscoveryError::Cancelled));
    }

    #[test]
    fn zero_deadline_fails_before_spawn() {
        let limits = GitPathDiscoveryLimits::new(
            Duration::ZERO,
            TEST_LIMITS.output_bytes(),
            TEST_LIMITS.paths(),
            TEST_LIMITS.repository_path(),
        );
        let error = discover_repository_paths(Path::new("does-not-need-to-exist"), limits)
            .expect_err("a zero deadline must fail before Git starts");
        assert!(matches!(
            error,
            GitPathDiscoveryError::DeadlineExceeded {
                deadline: Duration::ZERO
            }
        ));
    }

    #[test]
    fn command_start_stdout_deadline_output_limit_and_cancellation_are_bounded() {
        let mut cancelled = || false;
        let error = capture_git_output_from_command(
            Command::new("repowitness-command-that-does-not-exist"),
            TEST_LIMITS,
            Instant::now() + TEST_LIMITS.deadline(),
            &mut cancelled,
        )
        .expect_err("a missing executable must fail to start");
        assert!(matches!(error, GitPathDiscoveryError::GitStart { .. }));

        let mut no_stdout = Command::new("git");
        no_stdout
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let error = capture_git_output_from_command(
            no_stdout,
            TEST_LIMITS,
            Instant::now() + TEST_LIMITS.deadline(),
            &mut cancelled,
        )
        .expect_err("a command without piped stdout must fail");
        assert!(matches!(error, GitPathDiscoveryError::GitStdoutUnavailable));

        let output_limited = GitPathDiscoveryLimits::new(
            Duration::from_secs(1),
            0,
            TEST_LIMITS.paths(),
            TEST_LIMITS.repository_path(),
        );
        let mut output_command = Command::new("git");
        output_command
            .arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let error = capture_git_output_from_command(
            output_command,
            output_limited,
            Instant::now() + output_limited.deadline(),
            &mut cancelled,
        )
        .expect_err("output over a zero-byte limit must fail");
        assert!(matches!(
            error,
            GitPathDiscoveryError::OutputByteLimitExceeded { limit: 0 }
        ));

        let mut waiting_command = Command::new("git");
        waiting_command
            .args(["hash-object", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let deadline = Duration::from_millis(10);
        let error = capture_git_output_from_command(
            waiting_command,
            GitPathDiscoveryLimits::new(
                deadline,
                TEST_LIMITS.output_bytes(),
                TEST_LIMITS.paths(),
                TEST_LIMITS.repository_path(),
            ),
            Instant::now() + deadline,
            &mut cancelled,
        )
        .expect_err("a waiting command must hit its deadline");
        assert!(matches!(
            error,
            GitPathDiscoveryError::DeadlineExceeded {
                deadline: observed
            } if observed == deadline
        ));

        let mut waiting_command = Command::new("git");
        waiting_command
            .args(["hash-object", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut cancel_now = || true;
        let error = capture_git_output_from_command(
            waiting_command,
            TEST_LIMITS,
            Instant::now() + TEST_LIMITS.deadline(),
            &mut cancel_now,
        )
        .expect_err("cancellation must terminate a waiting command");
        assert!(matches!(error, GitPathDiscoveryError::Cancelled));
    }

    #[cfg(unix)]
    #[test]
    fn inherited_stdout_writer_cannot_extend_the_declared_deadline() {
        let mut command = Command::new("sh");
        command
            .args(["-c", "sleep 1 &"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let deadline = Duration::from_millis(20);
        let started = Instant::now();
        let mut cancelled = || false;
        let error = capture_git_output_from_command(
            command,
            GitPathDiscoveryLimits::new(
                deadline,
                TEST_LIMITS.output_bytes(),
                TEST_LIMITS.paths(),
                TEST_LIMITS.repository_path(),
            ),
            started + deadline,
            &mut cancelled,
        )
        .expect_err("an inherited writer must not extend the direct child deadline");
        assert!(matches!(
            error,
            GitPathDiscoveryError::DeadlineExceeded {
                deadline: observed
            } if observed == deadline
        ));
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "the reader join extended the declared deadline"
        );
    }

    #[test]
    fn an_invalid_root_returns_a_redacted_resolution_failure() {
        let error = discover_repository_paths(
            Path::new("repowitness-path-that-does-not-exist"),
            TEST_LIMITS,
        )
        .expect_err("worktree resolution must reject an invalid root");
        assert!(matches!(
            error,
            GitPathDiscoveryError::WorktreeRootResolve { .. }
        ));
        assert!(
            !error
                .to_string()
                .contains("repowitness-path-that-does-not-exist")
        );
    }

    #[test]
    fn git_command_disables_ambient_and_interactive_behavior() {
        let command = sanitized_git_command(
            Path::new("repository"),
            GitPathDiscoveryScope::CachedAndUntracked,
        );
        assert_eq!(command.get_program(), OsStr::new("git"));

        let args = command.get_args().map(OsStr::to_owned).collect::<Vec<_>>();
        for expected in [
            "--no-pager",
            "--literal-pathspecs",
            "core.fsmonitor=false",
            "core.untrackedCache=false",
            "diff.external=",
            "pager.ls-files=false",
            "-C",
            "repository",
            "ls-files",
            "-z",
            "--full-name",
            "--cached",
            "--deduplicate",
            "--others",
            "--exclude-standard",
        ] {
            assert!(
                args.contains(&OsString::from(expected)),
                "missing {expected}"
            );
        }

        let environment = command
            .get_envs()
            .map(|(key, value)| (key.to_owned(), value.map(OsStr::to_owned)))
            .collect::<BTreeMap<_, _>>();
        for (key, expected) in [
            ("GIT_CONFIG_NOSYSTEM", Some("1")),
            ("GIT_CONFIG_GLOBAL", Some(null_device())),
            ("GIT_CONFIG_SYSTEM", Some(null_device())),
            ("GIT_TERMINAL_PROMPT", Some("0")),
            ("GCM_INTERACTIVE", Some("never")),
            ("GIT_OPTIONAL_LOCKS", Some("0")),
            ("GIT_NO_REPLACE_OBJECTS", Some("1")),
            ("GIT_PAGER", Some("cat")),
            ("PAGER", Some("cat")),
            ("GIT_DIR", None),
            ("GIT_WORK_TREE", None),
            ("GIT_INDEX_FILE", None),
            ("GIT_OBJECT_DIRECTORY", None),
            ("GIT_ALTERNATE_OBJECT_DIRECTORIES", None),
            ("GIT_SHALLOW_FILE", None),
            ("GIT_REPLACE_REF_BASE", None),
            ("GIT_COMMON_DIR", None),
            ("GIT_CONFIG", None),
            ("GIT_NAMESPACE", None),
            ("GIT_IMPLICIT_WORK_TREE", None),
            ("GIT_CEILING_DIRECTORIES", None),
            ("GIT_DISCOVERY_ACROSS_FILESYSTEM", None),
            ("GIT_REFERENCE_BACKEND", None),
            ("GIT_LITERAL_PATHSPECS", None),
            ("GIT_GLOB_PATHSPECS", None),
            ("GIT_NOGLOB_PATHSPECS", None),
            ("GIT_ICASE_PATHSPECS", None),
            ("GIT_CONFIG_COUNT", None),
            ("GIT_CONFIG_PARAMETERS", None),
            ("GIT_EXTERNAL_DIFF", None),
            ("GIT_FLUSH", None),
            ("GIT_TRACE", None),
            ("GIT_TRACE_CURL", None),
            ("GIT_TRACE_CURL_NO_DATA", None),
            ("GIT_TRACE_FSMONITOR", None),
            ("GIT_TRACE_PACKET", None),
            ("GIT_TRACE_PACK_ACCESS", None),
            ("GIT_TRACE_PACKFILE", None),
            ("GIT_TRACE_PERFORMANCE", None),
            ("GIT_TRACE_REDACT", None),
            ("GIT_TRACE_REFS", None),
            ("GIT_TRACE_SETUP", None),
            ("GIT_TRACE_SHALLOW", None),
            ("GIT_TRACE2", None),
            ("GIT_TRACE2_BRIEF", None),
            ("GIT_TRACE2_CONFIG_PARAMS", None),
            ("GIT_TRACE2_DST_DEBUG", None),
            ("GIT_TRACE2_ENV_VARS", None),
            ("GIT_TRACE2_EVENT", None),
            ("GIT_TRACE2_EVENT_BRIEF", None),
            ("GIT_TRACE2_EVENT_NESTING", None),
            ("GIT_TRACE2_MAX_FILES", None),
            ("GIT_TRACE2_PARENT_SID", None),
            ("GIT_TRACE2_PERF", None),
            ("GIT_TRACE2_PERF_BRIEF", None),
        ] {
            assert_eq!(
                environment.get(OsStr::new(key)),
                Some(&expected.map(OsString::from)),
                "unexpected environment setting for {key}"
            );
        }

        assert_cached_command_scope();
    }

    fn assert_cached_command_scope() {
        let cached = sanitized_git_command(Path::new("repository"), GitPathDiscoveryScope::Cached);
        let cached_args = cached.get_args().map(OsStr::to_owned).collect::<Vec<_>>();
        assert!(cached_args.contains(&OsString::from("--cached")));
        assert!(cached_args.contains(&OsString::from("--deduplicate")));
        assert!(!cached_args.contains(&OsString::from("--others")));
        assert!(!cached_args.contains(&OsString::from("--exclude-standard")));
    }

    #[test]
    fn errors_expose_only_safe_sources() {
        let io_error = GitPathDiscoveryError::GitStart {
            source: io::Error::other("safe test"),
        };
        assert!(std::error::Error::source(&io_error).is_some());

        let limit_error = GitPathDiscoveryError::PathLimitExceeded { limit: 1 };
        assert!(std::error::Error::source(&limit_error).is_none());
        assert_eq!(
            limit_error.to_string(),
            "repository path count exceeded its 1 path bound"
        );
    }

    #[test]
    fn every_error_variant_has_a_stable_redacted_diagnostic() {
        let io_error = || io::Error::other("private-source-detail");
        let errors = [
            GitPathDiscoveryError::DeadlineNotRepresentable,
            GitPathDiscoveryError::Cancelled,
            GitPathDiscoveryError::WorktreeRootResolve { source: io_error() },
            GitPathDiscoveryError::WorktreeMarkerInspect { source: io_error() },
            GitPathDiscoveryError::WorktreeMarkerUnsupported,
            GitPathDiscoveryError::WorktreeMarkerNotFound,
            GitPathDiscoveryError::GitStart { source: io_error() },
            GitPathDiscoveryError::GitStdoutUnavailable,
            GitPathDiscoveryError::OutputReaderStart { source: io_error() },
            GitPathDiscoveryError::GitOutputRead { source: io_error() },
            GitPathDiscoveryError::OutputByteLimitExceeded { limit: 7 },
            GitPathDiscoveryError::OutputByteCountNotRepresentable,
            GitPathDiscoveryError::OutputReaderStopped,
            GitPathDiscoveryError::OutputReaderPanicked,
            GitPathDiscoveryError::GitPoll { source: io_error() },
            GitPathDiscoveryError::DeadlineExceeded {
                deadline: Duration::from_millis(9),
            },
            GitPathDiscoveryError::GitUnsuccessful { code: Some(128) },
            GitPathDiscoveryError::GitUnsuccessful { code: None },
            GitPathDiscoveryError::OutputNotNulTerminated,
            GitPathDiscoveryError::PathCountOverflowed,
            GitPathDiscoveryError::PathLimitExceeded { limit: 3 },
            GitPathDiscoveryError::InvalidRepositoryPath {
                ordinal: 2,
                source: RepositoryPathError::Empty,
            },
            GitPathDiscoveryError::DuplicateRepositoryPath,
            GitPathDiscoveryError::TotalPathBytesOverflowed,
        ];
        for error in errors {
            let diagnostic = error.to_string();
            assert!(!diagnostic.is_empty());
            assert!(!diagnostic.contains("private-source-detail"));
        }
    }
}
