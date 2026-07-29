//! Bounded local write-side engineering-memory orchestration.

use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use repowitness_application::{
    MemoryImportReceipt, MemoryRecordIdTextError, RepositoryIdentityTextError,
    ResolvedConfiguration,
};
use repowitness_domain::{CanonicalMemoryDigest, MemoryAuditActorId, MemoryRecordId};

use crate::{
    GitPathDiscoveryError, LocalIndexError, MemoryFileImportError, SqliteStoreError,
    git_paths::discovered_worktree_root,
    local_index::{database_alias_identity, validated_database_outside_worktree},
    sqlite::{OwnedSqliteIndex, SqliteMutationLease},
};

mod approval;
mod history;
mod review;
mod secret;
mod write;

pub use history::{
    LocalMemoryHistoryImportLimits, LocalMemoryHistoryImportReport,
    LocalMemoryHistoryImportRequest, import_local_memory_history,
};
pub use review::LocalMemoryCorrespondenceReviewReceipt;
pub use write::{
    LocalMemoryFilePublicationStatus, LocalMemoryWriteReceipt, MemoryFileIdentityStatus,
    MemoryFilePublicationStepStatus,
};

/// Default deadline for one local memory-management operation.
pub const DEFAULT_LOCAL_MEMORY_MANAGE_DEADLINE: Duration = Duration::from_secs(60);

/// Complete input for approving one exact current worktree memory record.
#[derive(Clone, Copy)]
pub struct LocalMemoryApprovalRequest<'a> {
    repository_root: &'a Path,
    database: &'a Path,
    repository_identity: &'a str,
    record_id: &'a str,
    actor: &'a str,
    migration_applied_at_unix_ms: u64,
    recorded_at_unix_ms: u64,
    configuration: Option<&'a ResolvedConfiguration>,
    deadline: Duration,
}

impl<'a> LocalMemoryApprovalRequest<'a> {
    /// Constructs an approval request with the default bounded deadline.
    #[must_use]
    pub const fn new(
        repository_root: &'a Path,
        database: &'a Path,
        repository_identity: &'a str,
        record_id: &'a str,
        actor: &'a str,
        migration_applied_at_unix_ms: u64,
        recorded_at_unix_ms: u64,
    ) -> Self {
        Self {
            repository_root,
            database,
            repository_identity,
            record_id,
            actor,
            migration_applied_at_unix_ms,
            recorded_at_unix_ms,
            configuration: None,
            deadline: DEFAULT_LOCAL_MEMORY_MANAGE_DEADLINE,
        }
    }

    /// Applies resolved memory-mutation policy to this request.
    #[must_use]
    pub const fn with_configuration(mut self, configuration: &'a ResolvedConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }

    /// Replaces the end-to-end operation deadline.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalMemoryApprovalRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalMemoryApprovalRequest")
            .field(
                "configuration_digest",
                &self.configuration.map(ResolvedConfiguration::digest),
            )
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

/// Complete input for one canonical conflict-preserving Git-memory file write.
#[derive(Clone, Copy)]
pub struct LocalMemoryWriteRequest<'a> {
    repository_root: &'a Path,
    input: LocalMemoryWriteInput<'a>,
    repository_identity: &'a str,
    configuration: Option<&'a ResolvedConfiguration>,
    deadline: Duration,
}

#[derive(Clone, Copy)]
pub(super) enum LocalMemoryWriteInput<'a> {
    File(&'a Path),
    Bytes(&'a [u8]),
}

/// Complete input for one exact trusted correspondence-review event.
#[derive(Clone, Copy)]
pub struct LocalMemoryCorrespondenceReviewRequest<'a> {
    repository_root: &'a Path,
    database: &'a Path,
    repository_identity: &'a str,
    record_id: &'a str,
    revision_sha256: &'a str,
    evidence_ordinal: u8,
    operation: repowitness_domain::MemoryCorrespondenceReviewOperation,
    target_path: &'a str,
    target_artifact_sha256: &'a str,
    target_fact_ordinal: u64,
    actor: &'a str,
    migration_applied_at_unix_ms: u64,
    recorded_at_unix_ms: u64,
    configuration: Option<&'a ResolvedConfiguration>,
    deadline: Duration,
}

impl<'a> LocalMemoryCorrespondenceReviewRequest<'a> {
    /// Constructs a review request with the default bounded deadline.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "the exact source selector, target occurrence, and trusted actor remain explicit"
    )]
    pub const fn new(
        repository_root: &'a Path,
        database: &'a Path,
        repository_identity: &'a str,
        record_id: &'a str,
        revision_sha256: &'a str,
        evidence_ordinal: u8,
        operation: repowitness_domain::MemoryCorrespondenceReviewOperation,
        target_path: &'a str,
        target_artifact_sha256: &'a str,
        target_fact_ordinal: u64,
        actor: &'a str,
        migration_applied_at_unix_ms: u64,
        recorded_at_unix_ms: u64,
    ) -> Self {
        Self {
            repository_root,
            database,
            repository_identity,
            record_id,
            revision_sha256,
            evidence_ordinal,
            operation,
            target_path,
            target_artifact_sha256,
            target_fact_ordinal,
            actor,
            migration_applied_at_unix_ms,
            recorded_at_unix_ms,
            configuration: None,
            deadline: DEFAULT_LOCAL_MEMORY_MANAGE_DEADLINE,
        }
    }

    /// Applies resolved memory-mutation policy to this request.
    #[must_use]
    pub const fn with_configuration(mut self, configuration: &'a ResolvedConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }

    /// Replaces the end-to-end operation deadline.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalMemoryCorrespondenceReviewRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalMemoryCorrespondenceReviewRequest")
            .field("evidence_ordinal", &self.evidence_ordinal)
            .field("operation", &self.operation)
            .field("target_fact_ordinal", &self.target_fact_ordinal)
            .field(
                "configuration_digest",
                &self.configuration.map(ResolvedConfiguration::digest),
            )
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

impl<'a> LocalMemoryWriteRequest<'a> {
    /// Constructs a write request with the default bounded deadline.
    #[must_use]
    pub const fn new(
        repository_root: &'a Path,
        input: &'a Path,
        repository_identity: &'a str,
    ) -> Self {
        Self {
            repository_root,
            input: LocalMemoryWriteInput::File(input),
            repository_identity,
            configuration: None,
            deadline: DEFAULT_LOCAL_MEMORY_MANAGE_DEADLINE,
        }
    }

    /// Constructs an inline bounded record write without accepting a host input path.
    #[must_use]
    pub const fn from_bytes(
        repository_root: &'a Path,
        input: &'a [u8],
        repository_identity: &'a str,
    ) -> Self {
        Self {
            repository_root,
            input: LocalMemoryWriteInput::Bytes(input),
            repository_identity,
            configuration: None,
            deadline: DEFAULT_LOCAL_MEMORY_MANAGE_DEADLINE,
        }
    }

    /// Applies resolved memory-mutation policy to this request.
    #[must_use]
    pub const fn with_configuration(mut self, configuration: &'a ResolvedConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }

    /// Replaces the end-to-end operation deadline.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalMemoryWriteRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalMemoryWriteRequest")
            .field("input", &"<redacted-input>")
            .field(
                "configuration_digest",
                &self.configuration.map(ResolvedConfiguration::digest),
            )
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

/// Durable outcomes from approving one exact memory version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalMemoryApprovalReceipt {
    revision: CanonicalMemoryDigest,
    version_inserted: bool,
    observation_inserted: bool,
    approval_inserted: bool,
}

impl LocalMemoryApprovalReceipt {
    /// Returns the exact canonical semantic revision that was approved.
    #[must_use]
    pub const fn revision(self) -> CanonicalMemoryDigest {
        self.revision
    }

    /// Reports whether the immutable semantic version was newly inserted.
    #[must_use]
    pub const fn version_inserted(self) -> bool {
        self.version_inserted
    }

    /// Reports whether the exact worktree observation was newly appended.
    #[must_use]
    pub const fn observation_inserted(self) -> bool {
        self.observation_inserted
    }

    /// Reports whether the exact local approval was newly appended.
    #[must_use]
    pub const fn approval_inserted(self) -> bool {
        self.approval_inserted
    }
}

impl From<MemoryImportReceipt> for LocalMemoryApprovalReceipt {
    fn from(receipt: MemoryImportReceipt) -> Self {
        Self {
            revision: receipt.revision(),
            version_inserted: receipt.version_inserted(),
            observation_inserted: receipt.observation_inserted(),
            approval_inserted: receipt.approval_inserted(),
        }
    }
}

/// Stable content-, path-, actor-, object-, and digest-redacted management failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalMemoryManageError {
    /// One configured resource limit was invalid.
    InvalidLimits,
    /// Resolved policy denies all memory mutation capabilities.
    PolicyDenied,
    /// The absolute operation deadline could not be represented.
    DeadlineNotRepresentable,
    /// Cancellation was observed.
    Cancelled,
    /// The absolute operation deadline elapsed.
    DeadlineExceeded,
    /// Repository identity text was not canonical.
    RepositoryIdentityInvalid,
    /// Memory record identity text was not canonical.
    RecordIdentityInvalid,
    /// A canonical semantic revision digest was malformed.
    RevisionInvalid,
    /// The configured local actor was invalid.
    ActorInvalid,
    /// The requested repository worktree could not be safely resolved.
    RepositoryUnavailable,
    /// The SQLite path was unavailable or not authorized for this worktree.
    DatabaseUnavailable,
    /// The current worktree memory file failed contained admission.
    RecordUnavailable,
    /// The exact review target selector was malformed or unavailable.
    ReviewTargetUnavailable,
    /// The external record input could not be safely read or validated.
    InputUnavailable,
    /// Record scope did not match the configured repository.
    ScopeMismatch,
    /// The active source generation required for approval was unavailable.
    ActiveSourceUnavailable,
    /// High-confidence secret material was detected before promotion.
    SensitiveContent,
    /// The proposed version did not match the current optimistic parent state.
    WriteConflict,
    /// Phase 0 does not support writing a multi-parent merge.
    MergeUnsupported,
    /// The canonical file could not be durably published.
    FilePublicationFailed,
    /// Sanitized Git history was unavailable or malformed.
    HistoryUnavailable,
    /// A history count or byte bound was exceeded.
    HistoryLimitExceeded,
    /// Correspondence audit exceeded its per-evidence bound.
    ReviewLimitExceeded,
    /// The immutable journal or audit writer failed.
    PersistenceFailed,
    /// A fixed-width count could not represent the result.
    CountNotRepresentable,
}

impl fmt::Display for LocalMemoryManageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLimits => "memory management limits are invalid",
            Self::PolicyDenied => "memory mutation is denied by policy",
            Self::DeadlineNotRepresentable => "memory management deadline cannot be represented",
            Self::Cancelled => "memory management was cancelled",
            Self::DeadlineExceeded => "memory management deadline elapsed",
            Self::RepositoryIdentityInvalid => "repository identity is invalid",
            Self::RecordIdentityInvalid => "memory record identity is invalid",
            Self::RevisionInvalid => "memory revision identity is invalid",
            Self::ActorInvalid => "local memory actor is invalid",
            Self::RepositoryUnavailable => "repository worktree is unavailable",
            Self::DatabaseUnavailable => "memory database is unavailable",
            Self::RecordUnavailable => "memory record is unavailable",
            Self::ReviewTargetUnavailable => "memory correspondence review target is unavailable",
            Self::InputUnavailable => "memory record input is unavailable",
            Self::ScopeMismatch => "memory record repository scope does not match",
            Self::ActiveSourceUnavailable => "active source generation is unavailable",
            Self::SensitiveContent => "memory record contains disallowed sensitive material",
            Self::WriteConflict => "memory record changed or has an invalid parent",
            Self::MergeUnsupported => "multi-parent memory merge is not supported",
            Self::FilePublicationFailed => "canonical memory file publication failed",
            Self::HistoryUnavailable => "memory Git history is unavailable",
            Self::HistoryLimitExceeded => "memory Git history exceeded a resource limit",
            Self::ReviewLimitExceeded => "memory correspondence review exceeded a resource limit",
            Self::PersistenceFailed => "memory persistence failed",
            Self::CountNotRepresentable => "memory management count cannot be represented",
        })
    }
}

impl Error for LocalMemoryManageError {}

/// Approves one capability-contained current memory record.
pub fn approve_local_memory(
    request: LocalMemoryApprovalRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalMemoryApprovalReceipt, LocalMemoryManageError> {
    check_memory_write_policy(request.configuration)?;
    approval::approve(request, cancelled)
}

/// Validates and atomically publishes one canonical worktree memory version.
pub fn write_local_memory(
    request: LocalMemoryWriteRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalMemoryWriteReceipt, LocalMemoryManageError> {
    check_memory_write_policy(request.configuration)?;
    write::write(request, cancelled)
}

/// Appends or verifies one exact trusted correspondence-review event.
pub fn review_local_memory_correspondence(
    request: LocalMemoryCorrespondenceReviewRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalMemoryCorrespondenceReviewReceipt, LocalMemoryManageError> {
    check_memory_write_policy(request.configuration)?;
    review::review(request, cancelled)
}

/// Validates a fixed local trust actor before enabling a mutation capability.
pub fn validate_local_memory_actor(actor: &str) -> Result<(), LocalMemoryManageError> {
    MemoryAuditActorId::try_new(actor.to_owned())
        .map(|_| ())
        .map_err(|_| LocalMemoryManageError::ActorInvalid)
}

fn checked_deadline(duration: Duration) -> Result<Instant, LocalMemoryManageError> {
    if duration.is_zero() {
        return Err(LocalMemoryManageError::InvalidLimits);
    }
    Instant::now()
        .checked_add(duration)
        .ok_or(LocalMemoryManageError::DeadlineNotRepresentable)
}

fn check_memory_write_policy(
    configuration: Option<&ResolvedConfiguration>,
) -> Result<(), LocalMemoryManageError> {
    if configuration
        .is_some_and(|configuration| *configuration.policy().deny_memory_writes().effective())
    {
        Err(LocalMemoryManageError::PolicyDenied)
    } else {
        Ok(())
    }
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), LocalMemoryManageError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(LocalMemoryManageError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(LocalMemoryManageError::DeadlineExceeded);
    }
    Ok(())
}

fn open_worktree(root: &Path) -> Result<PathBuf, LocalMemoryManageError> {
    discovered_worktree_root(root).map_err(map_discovery_error)
}

fn open_store(
    worktree: &Path,
    database: &Path,
    migration_applied_at_unix_ms: u64,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<OwnedSqliteIndex, LocalMemoryManageError> {
    let database =
        validated_database_outside_worktree(worktree, database).map_err(map_database_error)?;
    let lease = SqliteMutationLease::acquire(&database, deadline).map_err(map_store_error)?;
    let identity = database_alias_identity(&database).map_err(map_database_error)?;
    OwnedSqliteIndex::start_with_lease(
        lease,
        identity,
        migration_applied_at_unix_ms,
        cancelled,
        deadline,
    )
    .map(|(store, _)| store)
    .map_err(map_store_error)
}

fn map_repository_identity_error(_: RepositoryIdentityTextError) -> LocalMemoryManageError {
    LocalMemoryManageError::RepositoryIdentityInvalid
}

fn map_record_identity_error(_: MemoryRecordIdTextError) -> LocalMemoryManageError {
    LocalMemoryManageError::RecordIdentityInvalid
}

fn map_discovery_error(error: GitPathDiscoveryError) -> LocalMemoryManageError {
    match error {
        GitPathDiscoveryError::Cancelled => LocalMemoryManageError::Cancelled,
        GitPathDiscoveryError::DeadlineExceeded { .. }
        | GitPathDiscoveryError::DeadlineNotRepresentable => {
            LocalMemoryManageError::DeadlineExceeded
        }
        _ => LocalMemoryManageError::RepositoryUnavailable,
    }
}

fn map_database_error(_: LocalIndexError) -> LocalMemoryManageError {
    LocalMemoryManageError::DatabaseUnavailable
}

fn map_file_error(error: MemoryFileImportError) -> LocalMemoryManageError {
    match error {
        MemoryFileImportError::Cancelled => LocalMemoryManageError::Cancelled,
        MemoryFileImportError::DeadlineExceeded => LocalMemoryManageError::DeadlineExceeded,
        _ => LocalMemoryManageError::RecordUnavailable,
    }
}

fn map_store_error(error: SqliteStoreError) -> LocalMemoryManageError {
    match error {
        SqliteStoreError::Cancelled => LocalMemoryManageError::Cancelled,
        SqliteStoreError::DeadlineExceeded | SqliteStoreError::ReplyTimeout => {
            LocalMemoryManageError::DeadlineExceeded
        }
        SqliteStoreError::GenerationUnavailable | SqliteStoreError::WorkspaceUnavailable => {
            LocalMemoryManageError::ActiveSourceUnavailable
        }
        SqliteStoreError::CountNotRepresentable => LocalMemoryManageError::CountNotRepresentable,
        SqliteStoreError::InvalidMemoryCorrespondenceReview => {
            LocalMemoryManageError::ReviewTargetUnavailable
        }
        SqliteStoreError::MemoryCorrespondenceReviewLimitExceeded => {
            LocalMemoryManageError::ReviewLimitExceeded
        }
        _ => LocalMemoryManageError::PersistenceFailed,
    }
}

fn record_id(request: &str) -> Result<MemoryRecordId, LocalMemoryManageError> {
    repowitness_application::MemoryRecordIdTextV1::decode(request)
        .map_err(map_record_identity_error)
}

#[cfg(test)]
mod tests;
