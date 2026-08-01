//! Bounded historical applicability reads over retained snapshots and Git objects.

use std::{
    error::Error,
    fmt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use repowitness_application::RepositoryIdentityTextV1;
use repowitness_domain::{MemoryCommitId, MemoryObservationSource, MemoryRecordedAtUnixMillis};

use crate::{
    GitPathDiscoveryError, GitPathDiscoveryLimits, KnownAtHistoryReceipt, OwnedSqliteReader,
    SqliteStoreError,
    git_paths::{
        capture_git_output_with_status_from_command, discovered_worktree_root,
        sanitized_git_base_command,
    },
};

/// Default bound for one historical applicability receipt.
pub const DEFAULT_LOCAL_KNOWN_AT_HISTORY_DEADLINE: Duration = Duration::from_secs(5);
const MAX_LOCAL_KNOWN_AT_HISTORY_DEADLINE: Duration = Duration::from_secs(30);
const MAX_HISTORY_RESULTS: u16 = 100;
const GIT_OUTPUT_BYTES: u64 = 1;

/// Complete, read-only request for a historical applicability receipt.
#[derive(Clone, Copy)]
pub struct LocalKnownAtHistoryRequest<'a> {
    repository_root: &'a Path,
    database: &'a Path,
    repository_identity: &'a str,
    known_at_unix_ms: u64,
    target: MemoryObservationSource,
    max_results: u16,
    deadline: Duration,
}

impl<'a> LocalKnownAtHistoryRequest<'a> {
    /// Creates a bounded read for one exact target; branch names are
    /// intentionally not accepted because they are not stable selectors.
    #[must_use]
    pub const fn new(
        repository_root: &'a Path,
        database: &'a Path,
        repository_identity: &'a str,
        known_at_unix_ms: u64,
        target: MemoryObservationSource,
    ) -> Self {
        Self {
            repository_root,
            database,
            repository_identity,
            known_at_unix_ms,
            target,
            max_results: 32,
            deadline: DEFAULT_LOCAL_KNOWN_AT_HISTORY_DEADLINE,
        }
    }

    /// Replaces the independently bounded result count.
    #[must_use]
    pub const fn with_max_results(mut self, max_results: u16) -> Self {
        self.max_results = max_results;
        self
    }

    /// Replaces the complete read deadline.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalKnownAtHistoryRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalKnownAtHistoryRequest")
            .field("known_at_unix_ms", &self.known_at_unix_ms)
            .field("target", &self.target)
            .field("max_results", &self.max_results)
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

/// Read failure without leaking a repository path, source, or Git output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalKnownAtHistoryError {
    /// A supplied identifier, bound, or timestamp was invalid.
    InvalidRequest,
    /// Cancellation was observed before a definitive receipt.
    Cancelled,
    /// The bounded operation expired.
    DeadlineExceeded,
    /// The local Git worktree could not be safely opened.
    RepositoryUnavailable,
    /// The immutable local database could not be read.
    DatabaseUnavailable,
}

impl fmt::Display for LocalKnownAtHistoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidRequest => "historical applicability request is invalid",
            Self::Cancelled => "historical applicability read was cancelled",
            Self::DeadlineExceeded => "historical applicability deadline elapsed",
            Self::RepositoryUnavailable => "historical Git target could not be evaluated",
            Self::DatabaseUnavailable => "historical memory evidence could not be read",
        })
    }
}

impl Error for LocalKnownAtHistoryError {}

/// Returns a retained-coverage receipt for the exact target at the supplied
/// recorded-time cutoff. Worktree targets are evaluated inside the immutable
/// reader. Git targets additionally require a sanitized `cat-file` object
/// fence; a missing or pruned object remains `Unavailable`, never applicable.
pub fn read_local_known_at_history(
    request: LocalKnownAtHistoryRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<KnownAtHistoryReceipt, LocalKnownAtHistoryError> {
    if request.max_results == 0
        || request.max_results > MAX_HISTORY_RESULTS
        || request.deadline.is_zero()
        || request.deadline > MAX_LOCAL_KNOWN_AT_HISTORY_DEADLINE
    {
        return Err(LocalKnownAtHistoryError::InvalidRequest);
    }
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalKnownAtHistoryError::InvalidRequest)?;
    check_control(&cancelled, deadline)?;
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(|_| LocalKnownAtHistoryError::InvalidRequest)?;
    let known_at = MemoryRecordedAtUnixMillis::try_new(request.known_at_unix_ms)
        .map_err(|_| LocalKnownAtHistoryError::InvalidRequest)?;
    let git_available = match request.target {
        MemoryObservationSource::Git(commit) => Some(git_commit_available(
            request.repository_root,
            commit,
            &cancelled,
            deadline,
        )?),
        MemoryObservationSource::Worktree(_) => None,
    };
    let reader = OwnedSqliteReader::start(request.database, deadline).map_err(map_store_error)?;
    let result = reader.known_at_history_receipt(
        repository,
        known_at,
        request.target,
        request.max_results,
        Arc::clone(&cancelled),
        deadline,
    );
    let shutdown = reader.shutdown(deadline);
    let receipt = result.map_err(map_store_error)?;
    shutdown.map_err(map_store_error)?;
    Ok(match git_available {
        Some(available) => receipt.with_git_object_availability(available),
        None => receipt,
    })
}

fn git_commit_available(
    root: &Path,
    commit: MemoryCommitId,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<bool, LocalKnownAtHistoryError> {
    check_control(cancelled, deadline)?;
    let worktree = discovered_worktree_root(root).map_err(map_git_error)?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(LocalKnownAtHistoryError::DeadlineExceeded);
    }
    let limits = GitPathDiscoveryLimits::new(
        remaining.min(DEFAULT_LOCAL_KNOWN_AT_HISTORY_DEADLINE),
        GIT_OUTPUT_BYTES,
        1,
        repowitness_domain::RepositoryPathLimits::new(1_048_576, 65_535),
    );
    let mut command = sanitized_git_base_command(&worktree);
    command
        .arg("cat-file")
        .arg("-e")
        .arg(format!("{}^{{commit}}", commit_hex(commit)));
    match capture_git_output_with_status_from_command(command, limits, deadline, &mut || {
        cancelled.load(Ordering::Acquire)
    }) {
        Ok((status, _)) => Ok(status.success()),
        Err(error) => Err(map_git_error(error)),
    }
}

fn check_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalKnownAtHistoryError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(LocalKnownAtHistoryError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(LocalKnownAtHistoryError::DeadlineExceeded);
    }
    Ok(())
}

fn commit_hex(commit: MemoryCommitId) -> String {
    let mut output = String::with_capacity(commit.as_bytes().len() * 2);
    for byte in commit.as_bytes() {
        use fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn map_git_error(error: GitPathDiscoveryError) -> LocalKnownAtHistoryError {
    match error {
        GitPathDiscoveryError::Cancelled => LocalKnownAtHistoryError::Cancelled,
        GitPathDiscoveryError::DeadlineExceeded { .. }
        | GitPathDiscoveryError::DeadlineNotRepresentable => {
            LocalKnownAtHistoryError::DeadlineExceeded
        }
        _ => LocalKnownAtHistoryError::RepositoryUnavailable,
    }
}

fn map_store_error(error: SqliteStoreError) -> LocalKnownAtHistoryError {
    match error {
        SqliteStoreError::Cancelled => LocalKnownAtHistoryError::Cancelled,
        SqliteStoreError::DeadlineExceeded | SqliteStoreError::ReplyTimeout => {
            LocalKnownAtHistoryError::DeadlineExceeded
        }
        _ => LocalKnownAtHistoryError::DatabaseUnavailable,
    }
}
