//! Durable, application-owned engineering-task checkpoints and verification receipts.

use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_domain::{
    RepositoryIdentityDigest, TaskCheckpoint, TaskId, TaskStatus, TaskVerification,
};

/// Adapter-confirmed receipt for one immutable checkpoint append.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskCheckpointReceipt {
    task_id: TaskId,
    sequence: u32,
}

impl TaskCheckpointReceipt {
    /// Creates a receipt after the adapter has committed the exact checkpoint.
    #[must_use]
    pub const fn new(task_id: TaskId, sequence: u32) -> Self {
        Self { task_id, sequence }
    }

    /// Returns the durable task identity.
    #[must_use]
    pub const fn task_id(self) -> TaskId {
        self.task_id
    }

    /// Returns the committed monotonic checkpoint sequence.
    #[must_use]
    pub const fn sequence(self) -> u32 {
        self.sequence
    }
}

/// Adapter-confirmed opaque identity for one immutable verification event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskVerificationReceipt {
    verification_id: i64,
}

impl TaskVerificationReceipt {
    /// Creates a receipt after the adapter has committed the verification event.
    #[must_use]
    pub fn new(verification_id: i64) -> Option<Self> {
        if verification_id > 0 {
            Some(Self { verification_id })
        } else {
            None
        }
    }

    /// Returns the positive adapter-local verification identity.
    #[must_use]
    pub const fn verification_id(self) -> i64 {
        self.verification_id
    }
}

/// Narrow polling port for durable task state.
///
/// This port is deliberately separate from appends so read-only adapters never
/// need permission to create, migrate, or otherwise mutate task storage.
pub trait TaskStatusPort {
    /// Stable adapter failure mapped at the application boundary.
    type Error;

    /// Returns a polling-safe task summary only in the exact repository scope.
    fn task_status(
        &self,
        repository: RepositoryIdentityDigest,
        task_id: TaskId,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Option<TaskStatus>, Self::Error>;
}

/// Narrow append-only port for durable task state.
pub trait EngineeringTaskPort: TaskStatusPort {
    /// Appends one checkpoint, rejecting a scope or sequence conflict atomically.
    fn append_checkpoint(
        &self,
        checkpoint: TaskCheckpoint,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<TaskCheckpointReceipt, Self::Error>;

    /// Appends verification evidence for an existing task checkpoint.
    fn append_verification(
        &self,
        verification: TaskVerification,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<TaskVerificationReceipt, Self::Error>;
}

/// Stable application failure for durable engineering-task operations.
#[derive(Clone, Eq, PartialEq)]
pub enum EngineeringTaskError<PortError> {
    /// Cancellation was visible before or after the adapter call.
    Cancelled,
    /// The absolute request deadline elapsed before or after the adapter call.
    DeadlineExceeded,
    /// A verification belongs to a different task than its checkpoint request.
    TaskMismatch,
    /// An adapter returned a receipt inconsistent with the submitted checkpoint.
    InvalidPortReceipt,
    /// The persistence adapter failed.
    Port(PortError),
}

impl<PortError> fmt::Debug for EngineeringTaskError<PortError> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Cancelled => "EngineeringTaskError::Cancelled",
            Self::DeadlineExceeded => "EngineeringTaskError::DeadlineExceeded",
            Self::TaskMismatch => "EngineeringTaskError::TaskMismatch",
            Self::InvalidPortReceipt => "EngineeringTaskError::InvalidPortReceipt",
            Self::Port(_) => "EngineeringTaskError::Port",
        })
    }
}

impl<PortError> fmt::Display for EngineeringTaskError<PortError> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Cancelled => "engineering task operation was cancelled",
            Self::DeadlineExceeded => "engineering task deadline elapsed",
            Self::TaskMismatch => "engineering task verification does not match its checkpoint",
            Self::InvalidPortReceipt => "engineering task persistence returned an invalid receipt",
            Self::Port(_) => "engineering task persistence failed",
        })
    }
}

impl<PortError> Error for EngineeringTaskError<PortError> {}

/// Scope-checks and appends one immutable checkpoint through a bounded port.
pub fn append_task_checkpoint<Port>(
    port: &Port,
    checkpoint: TaskCheckpoint,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<TaskCheckpointReceipt, EngineeringTaskError<Port::Error>>
where
    Port: EngineeringTaskPort,
{
    check_control(&cancelled, deadline)?;
    let task_id = checkpoint.task_id();
    let sequence = checkpoint.sequence();
    let receipt = port
        .append_checkpoint(checkpoint, Arc::clone(&cancelled), deadline)
        .map_err(EngineeringTaskError::Port)?;
    check_control(&cancelled, deadline)?;
    if receipt.task_id != task_id || receipt.sequence != sequence {
        return Err(EngineeringTaskError::InvalidPortReceipt);
    }
    Ok(receipt)
}

/// Scope-checks and appends immutable verification evidence through a bounded port.
pub fn append_task_verification<Port>(
    port: &Port,
    checkpoint: &TaskCheckpoint,
    verification: TaskVerification,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<TaskVerificationReceipt, EngineeringTaskError<Port::Error>>
where
    Port: EngineeringTaskPort,
{
    check_control(&cancelled, deadline)?;
    if checkpoint.task_id() != verification.task_id()
        || checkpoint.sequence() != verification.checkpoint_sequence()
    {
        return Err(EngineeringTaskError::TaskMismatch);
    }
    let receipt = port
        .append_verification(verification, Arc::clone(&cancelled), deadline)
        .map_err(EngineeringTaskError::Port)?;
    check_control(&cancelled, deadline)?;
    if receipt.verification_id <= 0 {
        return Err(EngineeringTaskError::InvalidPortReceipt);
    }
    Ok(receipt)
}

/// Polls one durable task without exposing task text or captured verification output.
pub fn poll_task<Port>(
    port: &Port,
    repository: RepositoryIdentityDigest,
    task_id: TaskId,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<Option<TaskStatus>, EngineeringTaskError<Port::Error>>
where
    Port: TaskStatusPort,
{
    check_control(&cancelled, deadline)?;
    let status = port
        .task_status(repository, task_id, Arc::clone(&cancelled), deadline)
        .map_err(EngineeringTaskError::Port)?;
    check_control(&cancelled, deadline)?;
    if status.is_some_and(|value| value.task_id() != task_id || value.repository() != repository) {
        return Err(EngineeringTaskError::InvalidPortReceipt);
    }
    Ok(status)
}

fn check_control<PortError>(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), EngineeringTaskError<PortError>> {
    if cancelled.load(Ordering::Acquire) {
        return Err(EngineeringTaskError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(EngineeringTaskError::DeadlineExceeded);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        sync::{Arc, atomic::AtomicBool},
        time::{Duration, Instant},
    };

    use repowitness_domain::{
        ConfigurationDigest, RepositoryIdentityDigest, SourceSnapshotDigest, TaskCheckpoint,
        TaskId, TaskState, TaskText, TaskVerification, TaskVerificationOutcome,
    };

    use super::{
        EngineeringTaskError, EngineeringTaskPort, TaskCheckpointReceipt, TaskStatusPort,
        TaskVerificationReceipt, append_task_checkpoint, append_task_verification,
    };

    struct FakePort {
        checkpoints: Cell<u32>,
        verifications: Cell<u32>,
    }

    impl TaskStatusPort for FakePort {
        type Error = ();

        fn task_status(
            &self,
            _repository: RepositoryIdentityDigest,
            _task_id: TaskId,
            _cancelled: Arc<AtomicBool>,
            _deadline: Instant,
        ) -> Result<Option<repowitness_domain::TaskStatus>, Self::Error> {
            Ok(None)
        }
    }

    impl EngineeringTaskPort for FakePort {
        fn append_checkpoint(
            &self,
            checkpoint: TaskCheckpoint,
            _cancelled: Arc<AtomicBool>,
            _deadline: Instant,
        ) -> Result<TaskCheckpointReceipt, Self::Error> {
            self.checkpoints.set(self.checkpoints.get() + 1);
            Ok(TaskCheckpointReceipt::new(
                checkpoint.task_id(),
                checkpoint.sequence(),
            ))
        }

        fn append_verification(
            &self,
            _verification: TaskVerification,
            _cancelled: Arc<AtomicBool>,
            _deadline: Instant,
        ) -> Result<TaskVerificationReceipt, Self::Error> {
            self.verifications.set(self.verifications.get() + 1);
            Ok(TaskVerificationReceipt::new(1).expect("positive ID"))
        }
    }

    fn checkpoint(sequence: u32) -> TaskCheckpoint {
        TaskCheckpoint::try_new(
            TaskId::new([1; 16]),
            RepositoryIdentityDigest::new([2; 32]),
            sequence,
            TaskState::Open,
            TaskText::try_new("complete task".to_owned()).expect("valid text"),
            None,
            None,
            1,
        )
        .expect("valid checkpoint")
    }

    fn verification(sequence: u32) -> TaskVerification {
        TaskVerification::try_new(
            TaskId::new([1; 16]),
            sequence,
            SourceSnapshotDigest::new([3; 32]),
            TaskText::try_new("cargo test".to_owned()).expect("valid text"),
            TaskText::try_new("local runner".to_owned()).expect("valid text"),
            ConfigurationDigest::new([4; 32]),
            TaskVerificationOutcome::Passed,
            [5; 32],
            0,
            2,
        )
        .expect("valid verification")
    }

    #[test]
    fn matching_checkpoint_and_verification_reach_the_port() {
        let port = FakePort {
            checkpoints: Cell::new(0),
            verifications: Cell::new(0),
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let deadline = Instant::now() + Duration::from_secs(1);
        let checkpoint = checkpoint(1);
        append_task_checkpoint(&port, checkpoint.clone(), Arc::clone(&cancelled), deadline)
            .expect("checkpoint succeeds");
        append_task_verification(&port, &checkpoint, verification(1), cancelled, deadline)
            .expect("verification succeeds");
        assert_eq!((port.checkpoints.get(), port.verifications.get()), (1, 1));
    }

    #[test]
    fn mismatched_verification_fails_before_the_port() {
        let port = FakePort {
            checkpoints: Cell::new(0),
            verifications: Cell::new(0),
        };
        let error = append_task_verification(
            &port,
            &checkpoint(1),
            verification(2),
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        )
        .expect_err("mismatch is rejected");
        assert_eq!(error, EngineeringTaskError::TaskMismatch);
        assert_eq!(port.verifications.get(), 0);
    }
}
