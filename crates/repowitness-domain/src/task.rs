//! Pure, bounded durable engineering-work identities and receipts.

use std::{error::Error, fmt};

use crate::{ConfigurationDigest, RepositoryIdentityDigest, SourceSnapshotDigest};

/// Maximum UTF-8 bytes in a persisted task summary field.
pub const MAX_TASK_TEXT_BYTES: usize = 4 * 1024;
/// Maximum number of immutable checkpoints retained for one task before policy cleanup.
pub const MAX_TASK_CHECKPOINTS: u32 = 4_096;
/// Maximum recorded output byte count admitted in a verification receipt.
pub const MAX_TASK_VERIFICATION_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;

/// Opaque 128-bit identifier for one application-owned engineering task.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TaskId([u8; 16]);

impl TaskId {
    /// Constructs an identifier from locally generated opaque bytes.
    #[must_use]
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Returns the exact opaque bytes for local storage.
    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }
}

impl fmt::Debug for TaskId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TaskId")
            .field("bits", &128_u16)
            .finish_non_exhaustive()
    }
}

/// Lifecycle of one durable engineering task.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TaskState {
    /// Work may proceed.
    Open,
    /// Work requires an external answer or decision.
    Blocked,
    /// Acceptance criteria have been satisfied.
    Completed,
    /// Work was intentionally stopped before completion.
    Cancelled,
}

/// Categorical outcome of one independently recorded verification.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TaskVerificationOutcome {
    /// The bounded check completed successfully.
    Passed,
    /// The bounded check completed and failed.
    Failed,
    /// The check was cancelled before a conclusive result.
    Cancelled,
    /// Required evidence or output coverage was unavailable.
    Incomplete,
}

/// Bounded polling-safe summary of one durable task. It intentionally excludes
/// task text and captured output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskStatus {
    task_id: TaskId,
    repository: RepositoryIdentityDigest,
    state: TaskState,
    checkpoint_sequence: u32,
    verification_count: u32,
}

impl TaskStatus {
    /// Creates a status after the persistence boundary has verified its counts.
    #[must_use]
    pub const fn new(
        task_id: TaskId,
        repository: RepositoryIdentityDigest,
        state: TaskState,
        checkpoint_sequence: u32,
        verification_count: u32,
    ) -> Self {
        Self {
            task_id,
            repository,
            state,
            checkpoint_sequence,
            verification_count,
        }
    }

    /// Returns the durable task identity.
    #[must_use]
    pub const fn task_id(self) -> TaskId {
        self.task_id
    }
    /// Returns the repository scope required by polling callers.
    #[must_use]
    pub const fn repository(self) -> RepositoryIdentityDigest {
        self.repository
    }
    /// Returns the last immutable checkpoint state.
    #[must_use]
    pub const fn state(self) -> TaskState {
        self.state
    }
    /// Returns the last checkpoint sequence.
    #[must_use]
    pub const fn checkpoint_sequence(self) -> u32 {
        self.checkpoint_sequence
    }
    /// Returns the bounded number of verification receipts.
    #[must_use]
    pub const fn verification_count(self) -> u32 {
        self.verification_count
    }
}

/// Content-redacted task validation error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskError {
    /// A persisted text value is empty, over budget, or contains a disallowed control character.
    InvalidText,
    /// A checkpoint sequence is zero or exceeds the bounded retention range.
    InvalidCheckpoint,
    /// A verification reports an output size above the durable bound.
    InvalidVerificationOutputBytes,
}

impl fmt::Display for TaskError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidText => "task text is invalid",
            Self::InvalidCheckpoint => "task checkpoint is invalid",
            Self::InvalidVerificationOutputBytes => "task verification output size is invalid",
        })
    }
}

impl Error for TaskError {}

/// Validated structured task text. Secret scanning is enforced at the local promotion boundary.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TaskText(Box<str>);

impl TaskText {
    /// Validates and owns one nonempty single-line-or-LF task summary.
    pub fn try_new(value: impl Into<Box<str>>) -> Result<Self, TaskError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_TASK_TEXT_BYTES
            || value.chars().any(|character| {
                matches!(character, '\0' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
            })
        {
            return Err(TaskError::InvalidText);
        }
        Ok(Self(value))
    }

    /// Returns the validated text without changing its scope or trust level.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for TaskText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TaskText(<redacted>)")
    }
}

/// Immutable structured checkpoint for one task.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskCheckpoint {
    task_id: TaskId,
    repository: RepositoryIdentityDigest,
    sequence: u32,
    state: TaskState,
    objective: TaskText,
    hypothesis: Option<TaskText>,
    next_safe_action: Option<TaskText>,
    recorded_at_unix_ms: u64,
}

impl TaskCheckpoint {
    /// Creates a bounded checkpoint. The local adapter separately scans text for secrets.
    #[allow(
        clippy::too_many_arguments,
        reason = "the durable task checkpoint has fixed evidence fields"
    )]
    pub fn try_new(
        task_id: TaskId,
        repository: RepositoryIdentityDigest,
        sequence: u32,
        state: TaskState,
        objective: TaskText,
        hypothesis: Option<TaskText>,
        next_safe_action: Option<TaskText>,
        recorded_at_unix_ms: u64,
    ) -> Result<Self, TaskError> {
        if sequence == 0 || sequence > MAX_TASK_CHECKPOINTS {
            return Err(TaskError::InvalidCheckpoint);
        }
        Ok(Self {
            task_id,
            repository,
            sequence,
            state,
            objective,
            hypothesis,
            next_safe_action,
            recorded_at_unix_ms,
        })
    }

    /// Returns the task identity.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }
    /// Returns the immutable repository scope.
    #[must_use]
    pub const fn repository(&self) -> RepositoryIdentityDigest {
        self.repository
    }
    /// Returns the monotonic checkpoint sequence.
    #[must_use]
    pub const fn sequence(&self) -> u32 {
        self.sequence
    }
    /// Returns the checkpoint state.
    #[must_use]
    pub const fn state(&self) -> TaskState {
        self.state
    }
    /// Returns the objective.
    #[must_use]
    pub fn objective(&self) -> &TaskText {
        &self.objective
    }
    /// Returns the current hypothesis, when present.
    #[must_use]
    pub fn hypothesis(&self) -> Option<&TaskText> {
        self.hypothesis.as_ref()
    }
    /// Returns the next explicitly safe action, when present.
    #[must_use]
    pub fn next_safe_action(&self) -> Option<&TaskText> {
        self.next_safe_action.as_ref()
    }
    /// Returns the system-recorded timestamp supplied by the trusted local boundary.
    #[must_use]
    pub const fn recorded_at_unix_ms(&self) -> u64 {
        self.recorded_at_unix_ms
    }
}

/// Immutable receipt proving the result category of one bounded verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskVerification {
    task_id: TaskId,
    checkpoint_sequence: u32,
    source_snapshot: SourceSnapshotDigest,
    check: TaskText,
    producer: TaskText,
    configuration: ConfigurationDigest,
    outcome: TaskVerificationOutcome,
    captured_output_digest: [u8; 32],
    captured_output_bytes: u64,
    recorded_at_unix_ms: u64,
}

impl TaskVerification {
    /// Validates one redacted verification receipt.
    #[allow(
        clippy::too_many_arguments,
        reason = "verification evidence must keep independent identities explicit"
    )]
    pub fn try_new(
        task_id: TaskId,
        checkpoint_sequence: u32,
        source_snapshot: SourceSnapshotDigest,
        check: TaskText,
        producer: TaskText,
        configuration: ConfigurationDigest,
        outcome: TaskVerificationOutcome,
        captured_output_digest: [u8; 32],
        captured_output_bytes: u64,
        recorded_at_unix_ms: u64,
    ) -> Result<Self, TaskError> {
        if checkpoint_sequence == 0 || checkpoint_sequence > MAX_TASK_CHECKPOINTS {
            return Err(TaskError::InvalidCheckpoint);
        }
        if captured_output_bytes > MAX_TASK_VERIFICATION_OUTPUT_BYTES {
            return Err(TaskError::InvalidVerificationOutputBytes);
        }
        Ok(Self {
            task_id,
            checkpoint_sequence,
            source_snapshot,
            check,
            producer,
            configuration,
            outcome,
            captured_output_digest,
            captured_output_bytes,
            recorded_at_unix_ms,
        })
    }

    /// Returns the owning task identity.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }
    /// Returns the exact checkpoint this evidence verifies.
    #[must_use]
    pub const fn checkpoint_sequence(&self) -> u32 {
        self.checkpoint_sequence
    }
    /// Returns the source snapshot at which the check ran.
    #[must_use]
    pub const fn source_snapshot(&self) -> SourceSnapshotDigest {
        self.source_snapshot
    }
    /// Returns the check identity.
    #[must_use]
    pub fn check(&self) -> &TaskText {
        &self.check
    }
    /// Returns the check producer identity.
    #[must_use]
    pub fn producer(&self) -> &TaskText {
        &self.producer
    }
    /// Returns the resolved configuration digest.
    #[must_use]
    pub const fn configuration(&self) -> ConfigurationDigest {
        self.configuration
    }
    /// Returns the categorical outcome.
    #[must_use]
    pub const fn outcome(&self) -> TaskVerificationOutcome {
        self.outcome
    }
    /// Returns the digest of captured output, never the output itself.
    #[must_use]
    pub const fn captured_output_digest(&self) -> [u8; 32] {
        self.captured_output_digest
    }
    /// Returns the captured output byte count.
    #[must_use]
    pub const fn captured_output_bytes(&self) -> u64 {
        self.captured_output_bytes
    }
    /// Returns the trusted recorded timestamp.
    #[must_use]
    pub const fn recorded_at_unix_ms(&self) -> u64 {
        self.recorded_at_unix_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_text_rejects_control_and_over_budget_values() {
        assert_eq!(
            TaskText::try_new("line\rbreak").unwrap_err(),
            TaskError::InvalidText
        );
        assert_eq!(
            TaskText::try_new("x".repeat(MAX_TASK_TEXT_BYTES + 1)).unwrap_err(),
            TaskError::InvalidText
        );
    }
}
