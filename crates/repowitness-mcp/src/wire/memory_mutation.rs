use schemars::JsonSchema;
use serde::Serialize;

/// Stable public request scope for one authorized memory mutation.
#[derive(Clone, Copy, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMutationRequestScope {
    /// Canonical memory-file publication.
    Write,
    /// Local approval and observation append.
    Approve,
    /// Trusted correspondence-review append.
    Review,
    /// Observation-only bounded Git-history import.
    ImportHistory,
    /// Immutable memory-projection rebuild and publication.
    Revalidation,
}

impl MemoryMutationRequestScope {
    /// Returns the stable wire spelling used in redacted diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Approve => "approve",
            Self::Review => "review",
            Self::ImportHistory => "import_history",
            Self::Revalidation => "revalidation",
        }
    }

    /// Returns conservative request-level guidance when the exact phase is unknown.
    #[must_use]
    pub const fn reconciliation_guidance(self) -> &'static str {
        match self {
            Self::Write => {
                "reload the canonical record and compare its exact revision and parent state"
            }
            Self::Approve => {
                "run read-only database diagnostics, then reload the exact revision, observation, and approval receipt"
            }
            Self::Review => {
                "run read-only database diagnostics, then reload review history for the exact revision and evidence ordinal"
            }
            Self::ImportHistory => {
                "run read-only database diagnostics, then compare every intended journal revision and Git observation"
            }
            Self::Revalidation => {
                "run read-only database diagnostics, then read the active projection for the exact source generation"
            }
        }
    }
}

/// Stable path-, content-, and actor-free identity of one durable memory mutation.
#[derive(Clone, Copy, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMutationOperation {
    /// The supervisor lost the task outcome before it could identify a phase.
    UnknownPhase,
    /// Atomic publication of one canonical memory file.
    CanonicalWrite,
    /// SQLite writer startup and bounded recovery.
    StoreStartup,
    /// Immutable version, worktree observation, and local approval append.
    Approval,
    /// Exact trusted correspondence-review append.
    CorrespondenceReview,
    /// Observation-only bounded Git-history import.
    HistoryImport,
    /// Immutable memory-projection publication.
    ProjectionPublication,
    /// Post-commit WAL checkpoint maintenance.
    Checkpoint,
}

impl MemoryMutationOperation {
    /// Returns the stable wire spelling used in redacted diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownPhase => "unknown_phase",
            Self::CanonicalWrite => "canonical_write",
            Self::StoreStartup => "store_startup",
            Self::Approval => "approval",
            Self::CorrespondenceReview => "correspondence_review",
            Self::HistoryImport => "history_import",
            Self::ProjectionPublication => "projection_publication",
            Self::Checkpoint => "checkpoint",
        }
    }

    /// Returns operation-specific authoritative reconciliation guidance.
    #[must_use]
    pub const fn reconciliation_guidance(self) -> &'static str {
        match self {
            Self::UnknownPhase => {
                "inspect authoritative state for the attributed request before retrying"
            }
            Self::CanonicalWrite => {
                "reload the canonical record and compare its exact revision and parent state"
            }
            Self::StoreStartup => {
                "reopen the store and run read-only database diagnostics before retrying startup"
            }
            Self::Approval => {
                "reload the exact memory revision, worktree observation, and local approval receipt"
            }
            Self::CorrespondenceReview => {
                "reload correspondence-review history for the exact revision and evidence ordinal"
            }
            Self::HistoryImport => {
                "reload the immutable memory journal and compare every intended revision and Git observation"
            }
            Self::ProjectionPublication => {
                "reopen the store and read the active memory projection for the exact source generation"
            }
            Self::Checkpoint => {
                "retain the known memory receipt and retry only the idempotent checkpoint maintenance step"
            }
        }
    }
}
