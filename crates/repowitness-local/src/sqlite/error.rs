use std::fmt;

/// Stable failure at the local SQLite trust boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqliteStoreError {
    /// The bundled SQLite does not contain the required WAL-reset fix.
    UnsupportedSqliteVersion,
    /// The database could not be opened.
    OpenFailed,
    /// Required connection policy could not be applied.
    ConfigurationFailed,
    /// An existing file has a different application identity.
    ApplicationIdMismatch,
    /// The file uses an unknown or unsupported schema version.
    SchemaVersionMismatch,
    /// The exact compiled migration ledger does not match the database.
    MigrationLedgerMismatch,
    /// Schema creation or migration failed.
    MigrationFailed,
    /// Required FTS5 support is unavailable.
    Fts5Unavailable,
    /// The process-level database mutation lease could not be opened or locked.
    MutationLeaseUnavailable,
    /// The database path stopped naming the file authorized before startup.
    DatabaseIdentityChanged,
    /// Incomplete generations exceeded the bounded startup recovery budget.
    RecoveryGenerationLimitExceeded,
    /// A newly reserved database could not be removed after startup failed.
    DatabaseStartupCleanupFailed,
    /// A database operation failed without exposing raw SQLite text.
    DatabaseOperationFailed,
    /// A fixed-width count could not be represented by SQLite.
    CountNotRepresentable,
    /// The requested repository workspace is not registered.
    WorkspaceUnavailable,
    /// Connected-workspace membership is empty, duplicate, or inconsistent.
    InvalidWorkspaceMembership,
    /// Connected-workspace membership exceeds the fixed source-slot bound.
    WorkspaceSourceSlotLimitExceeded,
    /// The requested connected workspace is not registered.
    ConnectedWorkspaceUnavailable,
    /// Workspace-view membership is incomplete, duplicate, or ineligible.
    InvalidWorkspaceView,
    /// A prepared Rust graph projection violates the persistence contract.
    InvalidGraphPublication,
    /// Supplied memory import values failed adapter-boundary validation.
    InvalidMemoryImport,
    /// A correspondence review selector or target was not exact and current.
    InvalidMemoryCorrespondenceReview,
    /// Correspondence review history exceeded its fixed per-evidence bound.
    MemoryCorrespondenceReviewLimitExceeded,
    /// The supplied source epoch was not the current workspace epoch.
    StaleSourceEpoch,
    /// A requested source epoch transition was not monotonic.
    InvalidSourceEpoch,
    /// A durable source-slot epoch has no representable successor.
    SourceEpochExhausted,
    /// No complete memory projection matches the current active source generation.
    MemoryProjectionUnavailable,
    /// The prepared index did not match the declared snapshot semantics.
    PreparedIdentityMismatch,
    /// Persisted rows failed an exact count or identity check.
    IntegrityCheckFailed,
    /// The requested generation does not exist in the required state.
    GenerationUnavailable,
    /// Retention policy values or pins are outside the supported profile.
    InvalidRetentionPolicy,
    /// A pinned retention root is unavailable or no longer publishable.
    RetentionPinUnavailable,
    /// One eligible generation exceeds the caller's bounded collection budget.
    RetentionLimitExceeded,
    /// Roots or candidates changed after the retention plan was produced.
    RetentionPlanStale,
    /// The write was cancelled before a complete result.
    Cancelled,
    /// The absolute write deadline elapsed.
    DeadlineExceeded,
    /// The capacity-one owner queue is occupied.
    QueueFull,
    /// The owner thread is unavailable.
    WorkerUnavailable,
    /// The owner thread panicked.
    WorkerPanicked,
    /// A bounded reply did not arrive before its deadline.
    ReplyTimeout,
    /// A mutating worker did not return a definitive receipt within bounded resolution grace.
    MutationOutcomeUnknown,
    /// Lexical search limits are zero or exceed Phase 0 ceilings.
    InvalidSearchLimits,
    /// Search-projection rebuild limits are zero or exceed Phase 0 ceilings.
    InvalidProjectionRebuildLimits,
    /// Memory-projection load or result limits are invalid.
    InvalidMemoryProjectionLimits,
    /// Memory projection input or output exceeded a declared complete bound.
    MemoryProjectionLimitExceeded,
    /// A prepared memory projection violates the adapter contract.
    InvalidMemoryProjection,
    /// Authoritative searchable facts exceed the requested rebuild row limit.
    ProjectionRebuildRowLimitExceeded,
    /// The untrusted lexical query violates the literal query profile.
    InvalidSearchQuery,
    /// Search results exceeded the encoded-output byte limit.
    SearchOutputLimitExceeded,
    /// Memory recall exceeded its conservative encoded-output byte limit.
    MemoryRecallOutputLimitExceeded,
    /// Memory recall exceeded its canonical-record read byte limit.
    MemoryRecallScanLimitExceeded,
    /// Persisted reusable artifacts exceeded the bounded load budget.
    ArtifactReuseLimitExceeded,
    /// Online-backup limits are zero or exceed Phase 0 ceilings.
    InvalidBackupLimits,
    /// The backup destination or private temporary path is unavailable.
    BackupDestinationUnavailable,
    /// SQLite online backup or validation failed.
    BackupFailed,
    /// The backup exceeded its maximum number of page steps.
    BackupStepLimitExceeded,
    /// A completed backup was published but its private temporary link remained.
    BackupCleanupFailed,
}

impl fmt::Display for SqliteStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedSqliteVersion => "SQLite version does not meet the Phase 0 minimum",
            Self::OpenFailed => "SQLite database could not be opened",
            Self::ConfigurationFailed => "SQLite connection policy could not be applied",
            Self::ApplicationIdMismatch => "SQLite application identity does not match RepoWitness",
            Self::SchemaVersionMismatch => "SQLite schema version is unsupported",
            Self::MigrationLedgerMismatch => {
                "SQLite migration ledger does not match compiled schema"
            }
            Self::MigrationFailed => "SQLite schema migration failed",
            Self::Fts5Unavailable => "SQLite FTS5 support is unavailable",
            Self::MutationLeaseUnavailable => "SQLite mutation lease is unavailable",
            Self::DatabaseIdentityChanged => "SQLite database identity changed during startup",
            Self::RecoveryGenerationLimitExceeded => "SQLite recovery generation limit exceeded",
            Self::DatabaseStartupCleanupFailed => "SQLite failed-startup database cleanup failed",
            Self::DatabaseOperationFailed => "SQLite index operation failed",
            Self::CountNotRepresentable => "SQLite index count is not representable",
            Self::WorkspaceUnavailable => "SQLite workspace is unavailable",
            Self::InvalidWorkspaceMembership => "SQLite workspace membership is invalid",
            Self::WorkspaceSourceSlotLimitExceeded => "SQLite workspace source-slot limit exceeded",
            Self::ConnectedWorkspaceUnavailable => "SQLite connected workspace is unavailable",
            Self::InvalidWorkspaceView => "SQLite workspace view is invalid",
            Self::InvalidGraphPublication => "SQLite Rust graph publication is invalid",
            Self::InvalidMemoryImport => "memory import input is invalid",
            Self::InvalidMemoryCorrespondenceReview => {
                "memory correspondence review input is invalid"
            }
            Self::MemoryCorrespondenceReviewLimitExceeded => {
                "memory correspondence review limit exceeded"
            }
            Self::StaleSourceEpoch => "SQLite source epoch is stale",
            Self::InvalidSourceEpoch => "SQLite source epoch transition is invalid",
            Self::SourceEpochExhausted => "SQLite source epoch is exhausted",
            Self::MemoryProjectionUnavailable => "SQLite current-memory projection is unavailable",
            Self::PreparedIdentityMismatch => "prepared index identity is inconsistent",
            Self::IntegrityCheckFailed => "SQLite index integrity validation failed",
            Self::GenerationUnavailable => "SQLite generation is unavailable",
            Self::InvalidRetentionPolicy => "SQLite retention policy is invalid",
            Self::RetentionPinUnavailable => "SQLite retention pin is unavailable",
            Self::RetentionLimitExceeded => "SQLite retention budget is too small",
            Self::RetentionPlanStale => "SQLite retention plan is stale",
            Self::Cancelled => "SQLite index operation cancelled",
            Self::DeadlineExceeded => "SQLite index operation deadline exceeded",
            Self::QueueFull => "SQLite writer queue is full",
            Self::WorkerUnavailable => "SQLite writer is unavailable",
            Self::WorkerPanicked => "SQLite writer terminated unexpectedly",
            Self::ReplyTimeout => "SQLite writer reply deadline exceeded",
            Self::MutationOutcomeUnknown => "SQLite mutation outcome could not be determined",
            Self::InvalidSearchLimits => "SQLite search limits are invalid",
            Self::InvalidProjectionRebuildLimits => {
                "SQLite search projection rebuild limits are invalid"
            }
            Self::InvalidMemoryProjectionLimits => "SQLite memory projection limits are invalid",
            Self::MemoryProjectionLimitExceeded => "SQLite memory projection limit exceeded",
            Self::InvalidMemoryProjection => "SQLite memory projection input is invalid",
            Self::ProjectionRebuildRowLimitExceeded => {
                "SQLite search projection rebuild row limit exceeded"
            }
            Self::InvalidSearchQuery => "SQLite search query is invalid",
            Self::SearchOutputLimitExceeded => "SQLite search output byte limit exceeded",
            Self::MemoryRecallOutputLimitExceeded => {
                "SQLite memory-recall output byte limit exceeded"
            }
            Self::MemoryRecallScanLimitExceeded => {
                "SQLite memory-recall canonical scan byte limit exceeded"
            }
            Self::ArtifactReuseLimitExceeded => "SQLite reusable artifact load limit exceeded",
            Self::InvalidBackupLimits => "SQLite backup limits are invalid",
            Self::BackupDestinationUnavailable => "SQLite backup destination is unavailable",
            Self::BackupFailed => "SQLite online backup failed",
            Self::BackupStepLimitExceeded => "SQLite online backup step limit exceeded",
            Self::BackupCleanupFailed => "SQLite backup temporary file cleanup failed",
        })
    }
}

impl std::error::Error for SqliteStoreError {}
