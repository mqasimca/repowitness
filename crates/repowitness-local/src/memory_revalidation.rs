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

use repowitness_analysis::{
    RustAutomaticCorrespondence, RustCorrespondenceCandidate, RustCorrespondenceError,
    RustCorrespondenceResolution, RustCorrespondenceSubject, RustPathContinuity, RustSymbolKind,
    resolve_rust_correspondence,
};
use repowitness_application::{
    MAX_MEMORY_PROJECTION_VERSIONS, MemoryEvidenceOutcome, MemoryHeadState, MemoryProjectionError,
    MemoryVersionHeadInput, RepositoryIdentityTextError, RepositoryIdentityTextV1,
    evaluate_memory_projection, select_memory_head,
};
use repowitness_domain::{
    CanonicalMemoryDigest, MemoryAncestryCheck, MemoryCommitId, MemoryEvidence, MemoryLifecycle,
    MemoryProjectValidity, MemoryRecord, MemoryRecordId, MemoryRevalidationTarget, MemoryValidity,
    MemoryValidityEvaluationError, RustMemorySymbolKind, RustSymbolMemoryEvidence,
    evaluate_memory_project_validity,
};

use crate::{
    GenerationId, GitMemoryQueries, GitMemoryQueryError, GitMemoryQueryLimits,
    GitPathContinuityOutcome, GitPathDiscoveryError, GitPathDiscoveryLimits, LocalIndexError,
    OwnedSqliteIndex, SourceStateError, SqliteStoreError,
    git_paths::discovered_worktree_root,
    local_index::{database_alias_identity, validated_database_outside_worktree},
    source_state::{CapturedSourceState, capture_source_state_with_cancel},
    sqlite::{
        SqliteMutationLease,
        memory_projection::{
            LoadedMemoryJournal, LoadedMemoryVersion, MemoryProjectionLoadLimits,
            MemoryProjectionPublication, MemoryProjectionResultLimits, MemoryProjectionSource,
            PreparedMemoryProjection, PreparedProjectionCandidate, PreparedProjectionEvidence,
            PreparedProjectionRecord, PreparedProjectionRecordKind, ProjectionCandidateRelation,
            ProjectionEvidenceAssurance, ProjectionEvidenceOutcome, ProjectionHeadReason,
            ProjectionOccurrence,
        },
        memory_review::{CorrespondenceReviewDecision, LoadedCorrespondenceReviews},
    },
};

/// Default wall-clock deadline for a complete local memory revalidation.
pub const DEFAULT_LOCAL_MEMORY_REVALIDATION_DEADLINE: Duration = Duration::from_secs(60);
/// Default complete immutable-journal byte budget.
pub const DEFAULT_LOCAL_MEMORY_CANONICAL_BYTES: u64 = 64 * 1024 * 1024;
/// Default aggregate review-candidate output budget.
pub const DEFAULT_LOCAL_MEMORY_RESULT_CANDIDATES: u64 = 16_384;
/// Default number of sanitized Git history queries per rebuild.
pub const DEFAULT_LOCAL_MEMORY_GIT_QUERIES: u32 = 4_096;
/// Hard ceiling for sanitized Git history queries per rebuild.
pub const MAX_LOCAL_MEMORY_GIT_QUERIES: u32 = 65_536;

/// Complete resource policy for one local memory-projection rebuild.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalMemoryRevalidationLimits {
    deadline: Duration,
    source_state: GitPathDiscoveryLimits,
    git: GitMemoryQueryLimits,
    max_versions: u32,
    max_canonical_bytes: u64,
    max_result_candidates: u64,
    max_git_queries: u32,
}

impl LocalMemoryRevalidationLimits {
    /// Constructs one explicit end-to-end revalidation policy.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "each independent resource boundary remains explicit"
    )]
    pub const fn new(
        deadline: Duration,
        source_state: GitPathDiscoveryLimits,
        git: GitMemoryQueryLimits,
        max_versions: u32,
        max_canonical_bytes: u64,
        max_result_candidates: u64,
        max_git_queries: u32,
    ) -> Self {
        Self {
            deadline,
            source_state,
            git,
            max_versions,
            max_canonical_bytes,
            max_result_candidates,
            max_git_queries,
        }
    }

    /// Returns the end-to-end monotonic deadline.
    #[must_use]
    pub const fn deadline(self) -> Duration {
        self.deadline
    }

    /// Returns bounded canonical Git/source-state capture limits.
    #[must_use]
    pub const fn source_state(self) -> GitPathDiscoveryLimits {
        self.source_state
    }

    /// Returns per-command sanitized Git query limits.
    #[must_use]
    pub const fn git(self) -> GitMemoryQueryLimits {
        self.git
    }

    /// Returns the complete immutable-version load bound.
    #[must_use]
    pub const fn max_versions(self) -> u32 {
        self.max_versions
    }

    /// Returns the complete canonical-record byte bound.
    #[must_use]
    pub const fn max_canonical_bytes(self) -> u64 {
        self.max_canonical_bytes
    }

    /// Returns the aggregate persisted review-candidate bound.
    #[must_use]
    pub const fn max_result_candidates(self) -> u64 {
        self.max_result_candidates
    }

    /// Returns the maximum sanitized Git history command count.
    #[must_use]
    pub const fn max_git_queries(self) -> u32 {
        self.max_git_queries
    }
}

impl Default for LocalMemoryRevalidationLimits {
    fn default() -> Self {
        Self {
            deadline: DEFAULT_LOCAL_MEMORY_REVALIDATION_DEADLINE,
            source_state: GitPathDiscoveryLimits::default(),
            git: GitMemoryQueryLimits::default(),
            max_versions: MAX_MEMORY_PROJECTION_VERSIONS as u32,
            max_canonical_bytes: DEFAULT_LOCAL_MEMORY_CANONICAL_BYTES,
            max_result_candidates: DEFAULT_LOCAL_MEMORY_RESULT_CANDIDATES,
            max_git_queries: DEFAULT_LOCAL_MEMORY_GIT_QUERIES,
        }
    }
}

/// Complete explicit input for one bounded local memory revalidation.
#[derive(Clone, Copy)]
pub struct LocalMemoryRevalidationRequest<'a> {
    repository_root: &'a Path,
    database: &'a Path,
    repository_identity: &'a str,
    migration_applied_at_unix_ms: u64,
    limits: LocalMemoryRevalidationLimits,
}

impl<'a> LocalMemoryRevalidationRequest<'a> {
    /// Constructs a request using conservative default limits.
    #[must_use]
    pub fn new(
        repository_root: &'a Path,
        database: &'a Path,
        repository_identity: &'a str,
        migration_applied_at_unix_ms: u64,
    ) -> Self {
        Self {
            repository_root,
            database,
            repository_identity,
            migration_applied_at_unix_ms,
            limits: LocalMemoryRevalidationLimits::default(),
        }
    }

    /// Replaces the complete end-to-end resource policy.
    #[must_use]
    pub const fn with_limits(mut self, limits: LocalMemoryRevalidationLimits) -> Self {
        self.limits = limits;
        self
    }
}

impl fmt::Debug for LocalMemoryRevalidationRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalMemoryRevalidationRequest")
            .field("repository_root", &"<redacted-path>")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field(
                "migration_applied_at_unix_ms",
                &self.migration_applied_at_unix_ms,
            )
            .field("limits", &self.limits)
            .finish()
    }
}

/// Non-sensitive receipt for one atomically activated projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalMemoryRevalidationReport {
    projection_id: i64,
    generation: GenerationId,
    source_epoch: u64,
    recovered_generations: u64,
    projected_records: u32,
    skipped_records: u32,
    unresolved_records: u32,
    git_queries: u32,
    head_available: bool,
}

impl LocalMemoryRevalidationReport {
    /// Returns the database-local immutable projection identity.
    #[must_use]
    pub const fn projection_id(self) -> i64 {
        self.projection_id
    }

    /// Returns the exact active index generation used by the projection.
    #[must_use]
    pub const fn generation(self) -> GenerationId {
        self.generation
    }

    /// Returns the source epoch fenced during publication.
    #[must_use]
    pub const fn source_epoch(self) -> u64 {
        self.source_epoch
    }

    /// Returns incomplete index generations recovered at writer startup.
    #[must_use]
    pub const fn recovered_generations(self) -> u64 {
        self.recovered_generations
    }

    /// Returns records with a persisted projection result.
    #[must_use]
    pub const fn projected_records(self) -> u32 {
        self.projected_records
    }

    /// Returns records skipped because no trusted approved version exists.
    #[must_use]
    pub const fn skipped_records(self) -> u32 {
        self.skipped_records
    }

    /// Returns projected records requiring review or lacking complete evidence.
    #[must_use]
    pub const fn unresolved_records(self) -> u32 {
        self.unresolved_records
    }

    /// Returns sanitized Git history commands executed by the profile.
    #[must_use]
    pub const fn git_queries(self) -> u32 {
        self.git_queries
    }

    /// Reports whether an exact HEAD was safely bound to the indexed Git state.
    #[must_use]
    pub const fn head_available(self) -> bool {
        self.head_available
    }
}

/// Stable, content-redacted failure phase for local memory revalidation.
#[derive(Debug)]
pub enum LocalMemoryRevalidationError {
    /// The configured repository identity text was invalid.
    RepositoryIdentity {
        /// Stable scalar-validation failure.
        source: RepositoryIdentityTextError,
    },
    /// One configured resource bound was zero or exceeded a hard ceiling.
    InvalidLimits,
    /// The end-to-end monotonic deadline could not be represented.
    DeadlineNotRepresentable,
    /// Cancellation was visible before an adapter operation.
    Cancelled,
    /// The end-to-end monotonic deadline elapsed.
    DeadlineExceeded,
    /// Repository discovery failed.
    Discovery {
        /// Redacted bounded Git discovery failure.
        source: GitPathDiscoveryError,
    },
    /// Canonical source-state capture failed.
    SourceState {
        /// Redacted source-state failure.
        source: SourceStateError,
    },
    /// Source state changed while correspondence was being evaluated.
    ConcurrentSourceChange,
    /// The explicit database path could not be resolved safely.
    DatabasePathUnavailable,
    /// The database path would modify the indexed worktree.
    DatabaseInsideWorktree,
    /// The database has hard-link aliases that bypass path isolation.
    DatabaseHasMultipleLinks,
    /// The authorized database file changed before the writer opened.
    DatabaseChangedDuringRevalidation,
    /// SQLite startup, migration, or bounded recovery failed.
    StoreStartup {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// The complete immutable journal could not be loaded or verified.
    JournalLoad {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// The trusted approved-head graph was invalid.
    HeadSelection {
        /// Stable pure-policy failure.
        source: MemoryProjectionError,
    },
    /// Git-DAG project validity inputs were inconsistent.
    ValidityEvaluation {
        /// Stable pure-domain failure.
        source: MemoryValidityEvaluationError,
    },
    /// A sanitized Git history query was cancelled or exceeded its deadline.
    GitQuery {
        /// Stable subprocess-control failure.
        source: GitMemoryQueryError,
    },
    /// The complete Git history query budget was exhausted.
    GitQueryLimitExceeded,
    /// Complete current Rust candidates could not be loaded or verified.
    CandidateLoad {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// Exact Rust correspondence inputs violated the profile contract.
    Correspondence {
        /// Stable pure-analysis failure.
        source: RustCorrespondenceError,
    },
    /// Effective-state policy rejected inconsistent inputs.
    ProjectionPolicy {
        /// Stable pure-policy failure.
        source: MemoryProjectionError,
    },
    /// Prepared projection rows violated bounds or adapter invariants.
    ProjectionPreparation {
        /// Stable SQLite-boundary validation failure.
        source: SqliteStoreError,
    },
    /// Atomic immutable projection publication failed.
    Publication {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// The post-publication WAL checkpoint failed.
    Checkpoint {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// The owned writer did not shut down cleanly.
    Shutdown {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// A bounded count could not be represented by the projection contract.
    CountNotRepresentable,
    /// Integrity-checked journal data contradicted its selected head.
    JournalIntegrity,
}

impl fmt::Display for LocalMemoryRevalidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity { .. } => "repository identity is invalid",
            Self::InvalidLimits => "local memory revalidation limits are invalid",
            Self::DeadlineNotRepresentable => {
                "local memory revalidation deadline is not representable"
            }
            Self::Cancelled => "local memory revalidation was cancelled",
            Self::DeadlineExceeded => "local memory revalidation deadline elapsed",
            Self::Discovery { .. } => "local memory repository discovery failed",
            Self::SourceState { .. } => "local memory source-state capture failed",
            Self::ConcurrentSourceChange => {
                "repository source state changed during memory revalidation"
            }
            Self::DatabasePathUnavailable => {
                "local memory revalidation database path is unavailable"
            }
            Self::DatabaseInsideWorktree => {
                "local memory database must be outside the repository worktree"
            }
            Self::DatabaseHasMultipleLinks => {
                "local memory database must not have hard-link aliases"
            }
            Self::DatabaseChangedDuringRevalidation => {
                "local memory database changed during revalidation"
            }
            Self::StoreStartup { .. } => "local memory store startup failed",
            Self::JournalLoad { .. } => "local memory journal loading failed",
            Self::HeadSelection { .. } => "local memory approved-head selection failed",
            Self::ValidityEvaluation { .. } => "local memory project validity evaluation failed",
            Self::GitQuery { .. } => "local memory Git history query failed",
            Self::GitQueryLimitExceeded => "local memory Git history query limit exceeded",
            Self::CandidateLoad { .. } => "local memory correspondence candidate loading failed",
            Self::Correspondence { .. } => "local Rust memory correspondence failed",
            Self::ProjectionPolicy { .. } => "local memory effective-state evaluation failed",
            Self::ProjectionPreparation { .. } => "local memory projection preparation failed",
            Self::Publication { .. } => "local memory projection publication failed",
            Self::Checkpoint { .. } => "local memory checkpoint failed after publication",
            Self::Shutdown { .. } => "local memory writer shutdown failed after publication",
            Self::CountNotRepresentable => "local memory count is not representable",
            Self::JournalIntegrity => "local memory journal integrity validation failed",
        })
    }
}

impl Error for LocalMemoryRevalidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity { source } => Some(source),
            Self::Discovery { source } => Some(source),
            Self::SourceState { source } => Some(source),
            Self::StoreStartup { source }
            | Self::JournalLoad { source }
            | Self::CandidateLoad { source }
            | Self::ProjectionPreparation { source }
            | Self::Publication { source }
            | Self::Checkpoint { source }
            | Self::Shutdown { source } => Some(source),
            Self::HeadSelection { source } | Self::ProjectionPolicy { source } => Some(source),
            Self::ValidityEvaluation { source } => Some(source),
            Self::GitQuery { source } => Some(source),
            Self::Correspondence { source } => Some(source),
            Self::InvalidLimits
            | Self::DeadlineNotRepresentable
            | Self::Cancelled
            | Self::DeadlineExceeded
            | Self::ConcurrentSourceChange
            | Self::DatabasePathUnavailable
            | Self::DatabaseInsideWorktree
            | Self::DatabaseHasMultipleLinks
            | Self::DatabaseChangedDuringRevalidation
            | Self::GitQueryLimitExceeded
            | Self::CountNotRepresentable
            | Self::JournalIntegrity => None,
        }
    }
}

include!("memory_revalidation/use_case.rs");
include!("memory_revalidation/evidence_resolution.rs");

#[cfg(test)]
mod tests;
