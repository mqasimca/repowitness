//! Local composition for bounded, repository-scoped durable-task operations.

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

use repowitness_application::{
    EngineeringTaskError, RepositoryIdentityTextError, RepositoryIdentityTextV1,
    TaskCheckpointReceipt, append_task_checkpoint, poll_task,
};
use repowitness_domain::{TaskCheckpoint, TaskError, TaskId, TaskState, TaskStatus, TaskText};

use crate::{OwnedSqliteIndex, OwnedSqliteReader, SqliteStoreError};

/// Default end-to-end deadline for one local task polling request.
pub const DEFAULT_LOCAL_TASK_POLL_DEADLINE: Duration = Duration::from_secs(5);

/// Maximum durable task summaries one MCP polling page may request.
pub const MAX_LOCAL_TASK_LIST_RESULTS: u16 = 16;

/// Default end-to-end deadline for one local task checkpoint append.
pub const DEFAULT_LOCAL_TASK_WRITE_DEADLINE: Duration = Duration::from_secs(60);

/// Complete local input for one append-only task checkpoint.
///
/// The absence of a task ID creates a fresh opaque task identity at this local
/// boundary. A supplied identity appends the next checkpoint only; concurrent
/// writers are rejected by the SQLite sequence fence rather than overwriting
/// work state.
#[derive(Clone, Copy)]
pub struct LocalTaskCheckpointRequest<'a> {
    database: &'a Path,
    repository_identity: &'a str,
    task_id: Option<TaskId>,
    state: TaskState,
    objective: &'a str,
    hypothesis: Option<&'a str>,
    next_safe_action: Option<&'a str>,
    recorded_at_unix_ms: u64,
    deadline: Duration,
}

impl<'a> LocalTaskCheckpointRequest<'a> {
    /// Creates a first checkpoint with a locally generated opaque task ID.
    #[must_use]
    pub const fn create(
        database: &'a Path,
        repository_identity: &'a str,
        state: TaskState,
        objective: &'a str,
        hypothesis: Option<&'a str>,
        next_safe_action: Option<&'a str>,
        recorded_at_unix_ms: u64,
    ) -> Self {
        Self {
            database,
            repository_identity,
            task_id: None,
            state,
            objective,
            hypothesis,
            next_safe_action,
            recorded_at_unix_ms,
            deadline: DEFAULT_LOCAL_TASK_WRITE_DEADLINE,
        }
    }

    /// Creates the next checkpoint request for one existing task identity.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "the exact repository, task identity, structured checkpoint, and timestamp are independent trust inputs"
    )]
    pub const fn update(
        database: &'a Path,
        repository_identity: &'a str,
        task_id: TaskId,
        state: TaskState,
        objective: &'a str,
        hypothesis: Option<&'a str>,
        next_safe_action: Option<&'a str>,
        recorded_at_unix_ms: u64,
    ) -> Self {
        Self {
            database,
            repository_identity,
            task_id: Some(task_id),
            state,
            objective,
            hypothesis,
            next_safe_action,
            recorded_at_unix_ms,
            deadline: DEFAULT_LOCAL_TASK_WRITE_DEADLINE,
        }
    }

    /// Replaces the end-to-end operation deadline.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalTaskCheckpointRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalTaskCheckpointRequest")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("task_id", &self.task_id)
            .field("state", &self.state)
            .field("objective", &"<redacted>")
            .field("hypothesis_present", &self.hypothesis.is_some())
            .field("next_safe_action_present", &self.next_safe_action.is_some())
            .field("recorded_at_unix_ms", &self.recorded_at_unix_ms)
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

/// Explicit local input for one redacted task-status read.
#[derive(Clone, Copy)]
pub struct LocalTaskPollRequest<'a> {
    database: &'a Path,
    repository_identity: &'a str,
    task_id: TaskId,
    deadline: Duration,
}

/// Explicit local input for one bounded, repository-scoped task-status page.
#[derive(Clone, Copy)]
pub struct LocalTaskListRequest<'a> {
    database: &'a Path,
    repository_identity: &'a str,
    limit: u16,
    deadline: Duration,
}

impl<'a> LocalTaskListRequest<'a> {
    /// Constructs a polling page within the fixed native-MCP retention bound.
    #[must_use]
    pub const fn new(database: &'a Path, repository_identity: &'a str, limit: u16) -> Self {
        Self {
            database,
            repository_identity,
            limit,
            deadline: DEFAULT_LOCAL_TASK_POLL_DEADLINE,
        }
    }

    /// Replaces the monotonic end-to-end deadline.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalTaskListRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalTaskListRequest")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("limit", &self.limit)
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

impl<'a> LocalTaskPollRequest<'a> {
    /// Constructs a request with a bounded default deadline.
    #[must_use]
    pub const fn new(database: &'a Path, repository_identity: &'a str, task_id: TaskId) -> Self {
        Self {
            database,
            repository_identity,
            task_id,
            deadline: DEFAULT_LOCAL_TASK_POLL_DEADLINE,
        }
    }

    /// Replaces the monotonic end-to-end deadline.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalTaskPollRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalTaskPollRequest")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("task_id", &self.task_id)
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

/// Stable content-redacted local polling failure.
#[derive(Debug)]
pub enum LocalTaskPollError {
    /// The repository identity was malformed or non-canonical.
    RepositoryIdentity {
        /// Stable identity-validation failure.
        source: RepositoryIdentityTextError,
    },
    /// The absolute deadline was not representable.
    DeadlineNotRepresentable,
    /// The caller requested an unsupported bounded task-status page size.
    InvalidLimit,
    /// Cancellation was visible before database I/O.
    Cancelled,
    /// The deadline elapsed before database I/O.
    DeadlineExceeded,
    /// The owned local store could not start.
    StoreStart {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// The shared application poll use case failed.
    Poll {
        /// Stable application or SQLite boundary failure.
        source: EngineeringTaskError<SqliteStoreError>,
    },
    /// The owned local store could not shut down cleanly.
    Shutdown {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
}

/// Stable content- and path-redacted task-checkpoint failure.
#[derive(Debug)]
pub enum LocalTaskCheckpointError {
    /// The repository identity was malformed or non-canonical.
    RepositoryIdentity {
        /// Stable identity-validation failure.
        source: RepositoryIdentityTextError,
    },
    /// One checkpoint field violated the bounded domain contract.
    Checkpoint {
        /// Stable structured task validation failure.
        source: TaskError,
    },
    /// The operating system could not provide opaque task-ID entropy.
    EntropyUnavailable,
    /// The task update did not name an existing exact repository-scoped task.
    TaskNotFound,
    /// A concurrent or malformed append violated the immutable sequence fence.
    CheckpointConflict,
    /// The absolute deadline was not representable.
    DeadlineNotRepresentable,
    /// Cancellation was visible before database I/O.
    Cancelled,
    /// The deadline elapsed before database I/O.
    DeadlineExceeded,
    /// The owned local store could not start.
    StoreStart {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// The application checkpoint use case failed.
    Append {
        /// Stable application or SQLite boundary failure.
        source: EngineeringTaskError<SqliteStoreError>,
    },
    /// The owned local store could not shut down cleanly.
    Shutdown {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
}

impl fmt::Display for LocalTaskCheckpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity { .. } => "task checkpoint repository identity is invalid",
            Self::Checkpoint { .. } => "task checkpoint is invalid",
            Self::EntropyUnavailable => "task checkpoint identity entropy is unavailable",
            Self::TaskNotFound => "task checkpoint task is not found",
            Self::CheckpointConflict => {
                "task checkpoint sequence conflicts with current task state"
            }
            Self::DeadlineNotRepresentable => "task checkpoint deadline is not representable",
            Self::Cancelled => "task checkpoint was cancelled",
            Self::DeadlineExceeded => "task checkpoint deadline elapsed",
            Self::StoreStart { .. } => "task checkpoint store startup failed",
            Self::Append { .. } => "task checkpoint append failed",
            Self::Shutdown { .. } => "task checkpoint store shutdown failed",
        })
    }
}

impl Error for LocalTaskCheckpointError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity { source } => Some(source),
            Self::Checkpoint { source } => Some(source),
            Self::StoreStart { source } | Self::Shutdown { source } => Some(source),
            Self::Append { source } => Some(source),
            Self::EntropyUnavailable
            | Self::TaskNotFound
            | Self::CheckpointConflict
            | Self::DeadlineNotRepresentable
            | Self::Cancelled
            | Self::DeadlineExceeded => None,
        }
    }
}

impl fmt::Display for LocalTaskPollError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity { .. } => "task polling repository identity is invalid",
            Self::DeadlineNotRepresentable => "task polling deadline is not representable",
            Self::InvalidLimit => "task polling limit is invalid",
            Self::Cancelled => "task polling was cancelled",
            Self::DeadlineExceeded => "task polling deadline elapsed",
            Self::StoreStart { .. } => "task polling store startup failed",
            Self::Poll { .. } => "task polling failed",
            Self::Shutdown { .. } => "task polling store shutdown failed",
        })
    }
}

impl Error for LocalTaskPollError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity { source } => Some(source),
            Self::StoreStart { source } | Self::Shutdown { source } => Some(source),
            Self::Poll { source } => Some(source),
            Self::DeadlineNotRepresentable
            | Self::InvalidLimit
            | Self::Cancelled
            | Self::DeadlineExceeded => None,
        }
    }
}

/// Polls one task with exact repository scope, then cleanly releases the owned store.
pub fn poll_local_task(
    request: LocalTaskPollRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<Option<TaskStatus>, LocalTaskPollError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(|source| LocalTaskPollError::RepositoryIdentity { source })?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalTaskPollError::DeadlineNotRepresentable)?;
    check_control(cancelled.as_ref(), deadline)?;
    if !request.database.is_file() {
        return Ok(None);
    }
    let store = OwnedSqliteReader::start(request.database, deadline)
        .map_err(|source| LocalTaskPollError::StoreStart { source })?;
    let result = poll_task(&store, repository, request.task_id, cancelled, deadline);
    let shutdown = store.shutdown(deadline);
    match (result, shutdown) {
        (Ok(status), Ok(())) => Ok(status),
        (Err(source), _) => Err(LocalTaskPollError::Poll { source }),
        (Ok(_), Err(source)) => Err(LocalTaskPollError::Shutdown { source }),
    }
}

/// Lists the bounded most-recent task summaries in one exact repository scope.
///
/// This read path never creates or migrates a missing database and intentionally
/// excludes checkpoint text and captured verification output.
pub fn list_local_tasks(
    request: LocalTaskListRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<Vec<TaskStatus>, LocalTaskPollError> {
    if request.limit == 0 || request.limit > MAX_LOCAL_TASK_LIST_RESULTS {
        return Err(LocalTaskPollError::InvalidLimit);
    }
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(|source| LocalTaskPollError::RepositoryIdentity { source })?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalTaskPollError::DeadlineNotRepresentable)?;
    check_control(cancelled.as_ref(), deadline)?;
    if !request.database.is_file() {
        return Ok(Vec::new());
    }
    let store = OwnedSqliteReader::start(request.database, deadline)
        .map_err(|source| LocalTaskPollError::StoreStart { source })?;
    let result = store.task_statuses(repository, request.limit, Arc::clone(&cancelled), deadline);
    let shutdown = store.shutdown(deadline);
    match (result, shutdown) {
        (Ok(statuses), Ok(())) => Ok(statuses),
        (Err(source), _) => Err(LocalTaskPollError::Poll {
            source: EngineeringTaskError::Port(source),
        }),
        (Ok(_), Err(source)) => Err(LocalTaskPollError::Shutdown { source }),
    }
}

/// Appends one first or next durable checkpoint through the exclusive SQLite writer.
///
/// A supplied task ID is resolved in the exact repository scope before the
/// append. The writer repeats sequence validation transactionally, so a racing
/// checkpoint reports [`LocalTaskCheckpointError::CheckpointConflict`] and
/// never overwrites history.
pub fn append_local_task_checkpoint(
    request: LocalTaskCheckpointRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<TaskCheckpointReceipt, LocalTaskCheckpointError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(|source| LocalTaskCheckpointError::RepositoryIdentity { source })?;
    let objective = TaskText::try_new(request.objective.to_owned())
        .map_err(|source| LocalTaskCheckpointError::Checkpoint { source })?;
    let hypothesis = request
        .hypothesis
        .map(|text| TaskText::try_new(text.to_owned()))
        .transpose()
        .map_err(|source| LocalTaskCheckpointError::Checkpoint { source })?;
    let next_safe_action = request
        .next_safe_action
        .map(|text| TaskText::try_new(text.to_owned()))
        .transpose()
        .map_err(|source| LocalTaskCheckpointError::Checkpoint { source })?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalTaskCheckpointError::DeadlineNotRepresentable)?;
    check_checkpoint_control(cancelled.as_ref(), deadline)?;
    let task_id = request.task_id.map_or_else(generate_task_id, Ok)?;

    let (store, _) =
        OwnedSqliteIndex::start(request.database, request.recorded_at_unix_ms, deadline)
            .map_err(|source| LocalTaskCheckpointError::StoreStart { source })?;
    let identity = match request.task_id {
        None => Ok((task_id, 1)),
        Some(_) => poll_task(
            &store,
            repository,
            task_id,
            Arc::clone(&cancelled),
            deadline,
        )
        .map_err(|source| LocalTaskCheckpointError::Append { source })?
        .ok_or(LocalTaskCheckpointError::TaskNotFound)
        .and_then(|status| {
            status
                .checkpoint_sequence()
                .checked_add(1)
                .map(|sequence| (task_id, sequence))
                .ok_or(LocalTaskCheckpointError::CheckpointConflict)
        }),
    };
    let (task_id, sequence) = match identity {
        Ok(identity) => identity,
        Err(error) => {
            let _ = store.shutdown(deadline);
            return Err(error);
        }
    };
    let checkpoint = match TaskCheckpoint::try_new(
        task_id,
        repository,
        sequence,
        request.state,
        objective,
        hypothesis,
        next_safe_action,
        request.recorded_at_unix_ms,
    ) {
        Ok(checkpoint) => checkpoint,
        Err(source) => {
            let _ = store.shutdown(deadline);
            return Err(LocalTaskCheckpointError::Checkpoint { source });
        }
    };
    let result = append_task_checkpoint(&store, checkpoint, Arc::clone(&cancelled), deadline)
        .map_err(map_append_error);
    let shutdown = store.shutdown(deadline);
    match (result, shutdown) {
        (Ok(receipt), Ok(())) => Ok(receipt),
        (Err(error), _) => Err(error),
        (Ok(_), Err(source)) => Err(LocalTaskCheckpointError::Shutdown { source }),
    }
}

fn generate_task_id() -> Result<TaskId, LocalTaskCheckpointError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| LocalTaskCheckpointError::EntropyUnavailable)?;
    Ok(TaskId::new(bytes))
}

fn map_append_error(source: EngineeringTaskError<SqliteStoreError>) -> LocalTaskCheckpointError {
    match source {
        EngineeringTaskError::Cancelled => LocalTaskCheckpointError::Cancelled,
        EngineeringTaskError::DeadlineExceeded => LocalTaskCheckpointError::DeadlineExceeded,
        EngineeringTaskError::Port(SqliteStoreError::InvalidTask) => {
            LocalTaskCheckpointError::CheckpointConflict
        }
        source => LocalTaskCheckpointError::Append { source },
    }
}

fn check_checkpoint_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalTaskCheckpointError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalTaskCheckpointError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(LocalTaskCheckpointError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), LocalTaskPollError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalTaskPollError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(LocalTaskPollError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::atomic::AtomicBool};

    use super::*;

    #[test]
    fn invalid_identity_and_cancellation_fail_before_store_start() {
        let task_id = TaskId::new([0x11; 16]);
        let missing = Path::new("must-not-be-opened-task.db");
        assert!(!missing.exists(), "fixture path must be absent");
        let invalid = poll_local_task(
            LocalTaskPollRequest::new(missing, "invalid", task_id),
            Arc::new(AtomicBool::new(false)),
        )
        .expect_err("identity must fail before opening a store");
        assert!(matches!(
            invalid,
            LocalTaskPollError::RepositoryIdentity { .. }
        ));

        let identity = format!("rwi1:h:{}", "00".repeat(32));
        let cancelled = Arc::new(AtomicBool::new(true));
        let cancelled_error = poll_local_task(
            LocalTaskPollRequest::new(missing, &identity, task_id),
            cancelled,
        )
        .expect_err("cancellation must fail before opening a store");
        assert!(matches!(cancelled_error, LocalTaskPollError::Cancelled));

        let absent = poll_local_task(
            LocalTaskPollRequest::new(missing, &identity, task_id),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("an absent task database is not found, not initialized");
        assert!(absent.is_none());
        assert!(!missing.exists(), "polling must not create a database");
    }
}
