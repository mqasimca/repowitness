#[cfg(test)]
use std::cell::Cell;
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_analysis::{RUST_CORRESPONDENCE_PROFILE_ID, RUST_CORRESPONDENCE_PROFILE_VERSION};
use repowitness_application::{
    MemoryImportApproval, MemoryImportReceipt, PreparedRustFile, PreparedRustIndex,
    RustIndexCoverage, RustSourceSnapshotIdentity, SourceSlotEpoch, hash_analysis_artifact_key,
    hash_analysis_artifact_payload, hash_source_snapshot,
};
use repowitness_domain::{
    AnalysisArtifactKey, CanonicalMemoryDigest, ConnectedWorkspaceId, MemoryActorKind,
    MemoryAssurance, MemoryAuditActorId, MemoryEvidence, MemoryKind, MemoryLifecycle,
    MemoryObjectFormat, MemoryObservationSource, MemoryPresentationDigest, MemoryProvenanceOrigin,
    MemoryRecord, MemoryRecordedAtUnixMillis, MemoryRelationshipKind, MemoryValidity,
    PersonalMemoryKind, PersonalMemoryRecord, RepositoryIdentityDigest, RustMemorySymbolKind,
    SourceFileKind, SourceSlotId, SourceSnapshotDigest, TaskCheckpoint, TaskId, TaskState,
    TaskStatus, TaskText, TaskVerification, TaskVerificationOutcome,
};
use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params,
};
use sha2::{Digest, Sha256};

use super::{
    GenerationRetentionPolicy, MAX_RETENTION_GENERATION_CANDIDATES, MAX_RETENTION_GENERATION_PINS,
    RetentionApplyOutcome, RetentionPlan, RetentionPlanDigest, RetentionPolicyDigest,
    SqliteStoreError,
    memory_projection::{
        LoadedMemoryJournal, LoadedRustCandidateSet, MemoryProjectionLoadLimits,
        MemoryProjectionPublication, MemoryProjectionSource, PreparedMemoryProjection,
        load_memory_journal, load_memory_source, load_rust_candidates, publish_memory_projection,
    },
    memory_review::{
        LoadedCorrespondenceReviews, MemoryCorrespondenceReviewReceipt,
        PreparedMemoryCorrespondenceReview, append_memory_correspondence_review,
        load_memory_correspondence_reviews,
    },
    schema::{RECREATE_GENERATION_SEARCH, RECREATE_GENERATION_SEARCH_REBUILD},
    workspace::{
        MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS, PinnedWorkspaceView, PinnedWorkspaceViewMember,
        SourceSlotGeneration, SourceSlotState, WorkspaceSourceSlot, WorkspaceViewId,
        WorkspaceViewMember, canonical_source_slots, canonical_view_members,
    },
};

const WRITE_BATCH_ROWS: usize = 256;
const SCIP_OVERLAY_PROGRESS_INSTRUCTIONS: i32 = 1_000;
pub(super) const MAX_STARTUP_RECOVERY_GENERATIONS: usize = 4_096;
const RECOVERY_PROGRESS_INSTRUCTIONS: i32 = 1_000;
const MAX_PROJECTION_REBUILD_ROWS: u64 = 100_000_000;
const DEFAULT_PROJECTION_REBUILD_ROWS: u64 = 5_000_000;

type PersistedArtifactMetadata = (
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    i64,
    i64,
    i64,
    i64,
    i64,
    String,
    Option<Vec<u8>>,
);

/// Inclusive authoritative-row limit for one search-projection rebuild.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionRebuildLimits {
    max_rows: u64,
}

impl ProjectionRebuildLimits {
    /// Constructs a nonzero Phase 0 projection-rebuild limit.
    pub const fn try_new(max_rows: u64) -> Result<Self, SqliteStoreError> {
        if max_rows == 0 || max_rows > MAX_PROJECTION_REBUILD_ROWS {
            return Err(SqliteStoreError::InvalidProjectionRebuildLimits);
        }
        Ok(Self { max_rows })
    }

    /// Returns the inclusive authoritative-row limit.
    #[must_use]
    pub const fn max_rows(self) -> u64 {
        self.max_rows
    }
}

impl Default for ProjectionRebuildLimits {
    fn default() -> Self {
        Self {
            max_rows: DEFAULT_PROJECTION_REBUILD_ROWS,
        }
    }
}

/// Bounded facts about one atomically published search-projection rebuild.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionRebuildOutcome {
    previous_slot: u8,
    active_slot: u8,
    rebuilt_rows: u64,
    write_batches: u64,
}

impl ProjectionRebuildOutcome {
    /// Returns the projection slot that remained readable during the rebuild.
    #[must_use]
    pub const fn previous_slot(self) -> u8 {
        self.previous_slot
    }

    /// Returns the newly published projection slot.
    #[must_use]
    pub const fn active_slot(self) -> u8 {
        self.active_slot
    }

    /// Returns the exact number of rebuilt authoritative rows.
    #[must_use]
    pub const fn rebuilt_rows(self) -> u64 {
        self.rebuilt_rows
    }

    /// Returns the number of transactions that inserted at most 256 rows.
    #[must_use]
    pub const fn write_batches(self) -> u64 {
        self.write_batches
    }
}

/// Fixed-width database identity for one immutable index generation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GenerationId(i64);

impl GenerationId {
    pub(crate) const fn from_database(value: i64) -> Self {
        Self(value)
    }

    /// Returns the positive database-local identity.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// One explicit WAL checkpoint observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckpointOutcome {
    busy: u64,
    log_frames: u64,
    checkpointed_frames: u64,
}

impl CheckpointOutcome {
    /// Returns SQLite's busy result count.
    #[must_use]
    pub const fn busy(self) -> u64 {
        self.busy
    }

    /// Returns the number of frames observed in the WAL.
    #[must_use]
    pub const fn log_frames(self) -> u64 {
        self.log_frames
    }

    /// Returns the number of frames checkpointed.
    #[must_use]
    pub const fn checkpointed_frames(self) -> u64 {
        self.checkpointed_frames
    }
}

#[derive(Clone, Copy)]
pub(super) struct WriteControl<'a> {
    pub(super) cancelled: &'a Arc<AtomicBool>,
    pub(super) deadline: Instant,
}

#[cfg(test)]
thread_local! {
    static MUTATION_COMMIT_IN_PROGRESS: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
struct MutationCommitScope;

#[cfg(test)]
impl MutationCommitScope {
    fn enter() -> Self {
        MUTATION_COMMIT_IN_PROGRESS.with(|in_progress| {
            assert!(
                !in_progress.replace(true),
                "mutation commit scopes must not nest"
            );
        });
        Self
    }
}

#[cfg(test)]
impl Drop for MutationCommitScope {
    fn drop(&mut self) {
        MUTATION_COMMIT_IN_PROGRESS.with(|in_progress| {
            assert!(
                in_progress.replace(false),
                "mutation commit scope must remain active until drop"
            );
        });
    }
}

#[cfg(test)]
pub(super) fn mutation_commit_in_progress() -> bool {
    MUTATION_COMMIT_IN_PROGRESS.with(Cell::get)
}

pub(super) fn commit_mutation(transaction: Transaction<'_>) -> Result<(), SqliteStoreError> {
    #[cfg(test)]
    let _scope = MutationCommitScope::enter();
    transaction
        .commit()
        .map_err(|_| SqliteStoreError::MutationOutcomeUnknown)
}

pub(super) struct WriterMutationResult<T> {
    result: Result<T, SqliteStoreError>,
    connection_usable: bool,
}

impl<T> WriterMutationResult<T> {
    pub(super) const fn new(result: Result<T, SqliteStoreError>, connection_usable: bool) -> Self {
        Self {
            result,
            connection_usable,
        }
    }

    pub(super) fn into_parts(self) -> (Result<T, SqliteStoreError>, bool) {
        (self.result, self.connection_usable)
    }
}

#[derive(Clone, Copy)]
pub(super) struct SourceSlotReservation {
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    source_epoch: SourceSlotEpoch,
}

impl SourceSlotReservation {
    pub(super) const fn new(
        connected_workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        source_epoch: SourceSlotEpoch,
    ) -> Self {
        Self {
            connected_workspace,
            source_slot,
            source_epoch,
        }
    }
}

pub(super) struct PreparedMemoryImport {
    repository: RepositoryIdentityDigest,
    record: MemoryRecord,
    canonical_json: Vec<u8>,
    revision: CanonicalMemoryDigest,
    presentation: MemoryPresentationDigest,
    source: MemoryObservationSource,
    audit_actor: MemoryAuditActorId,
    recorded_at: MemoryRecordedAtUnixMillis,
    approval: MemoryImportApproval,
}

impl PreparedMemoryImport {
    #[allow(
        clippy::too_many_arguments,
        reason = "each semantic and audit identity remains explicit"
    )]
    pub(super) const fn new(
        repository: RepositoryIdentityDigest,
        record: MemoryRecord,
        canonical_json: Vec<u8>,
        revision: CanonicalMemoryDigest,
        presentation: MemoryPresentationDigest,
        source: MemoryObservationSource,
        audit_actor: MemoryAuditActorId,
        recorded_at: MemoryRecordedAtUnixMillis,
        approval: MemoryImportApproval,
    ) -> Self {
        Self {
            repository,
            record,
            canonical_json,
            revision,
            presentation,
            source,
            audit_actor,
            recorded_at,
            approval,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SearchProjection {
    Primary,
    Rebuild,
}

impl SearchProjection {
    const fn from_slot(slot: i64) -> Result<Self, SqliteStoreError> {
        match slot {
            0 => Ok(Self::Primary),
            1 => Ok(Self::Rebuild),
            _ => Err(SqliteStoreError::IntegrityCheckFailed),
        }
    }

    const fn slot(self) -> i64 {
        match self {
            Self::Primary => 0,
            Self::Rebuild => 1,
        }
    }

    const fn inactive(self) -> Self {
        match self {
            Self::Primary => Self::Rebuild,
            Self::Rebuild => Self::Primary,
        }
    }

    const fn stage_insert_sql(self) -> &'static str {
        match self {
            Self::Primary => INSERT_PRIMARY_SEARCH_ROW,
            Self::Rebuild => INSERT_REBUILD_SEARCH_ROW,
        }
    }

    const fn rebuild_insert_sql(self) -> &'static str {
        match self {
            Self::Primary => REBUILD_PRIMARY_BATCH,
            Self::Rebuild => REBUILD_SHADOW_BATCH,
        }
    }

    const fn count_sql(self) -> &'static str {
        match self {
            Self::Primary => "SELECT count(*) FROM generation_search",
            Self::Rebuild => "SELECT count(*) FROM generation_search_rebuild",
        }
    }

    const fn integrity_sql(self) -> &'static str {
        match self {
            Self::Primary => {
                "INSERT INTO generation_search(generation_search) VALUES('integrity-check')"
            }
            Self::Rebuild => {
                "INSERT INTO generation_search_rebuild(generation_search_rebuild)
                 VALUES('integrity-check')"
            }
        }
    }

    const fn recreate_sql(self) -> &'static str {
        match self {
            Self::Primary => RECREATE_GENERATION_SEARCH,
            Self::Rebuild => RECREATE_GENERATION_SEARCH_REBUILD,
        }
    }
}

#[derive(Clone, Copy)]
struct ProjectionCursor {
    generation: i64,
    file_ordinal: i64,
    fact_ordinal: i64,
}

const INITIAL_PROJECTION_CURSOR: ProjectionCursor = ProjectionCursor {
    generation: 0,
    file_ordinal: -1,
    fact_ordinal: -1,
};

const INSERT_PRIMARY_SEARCH_ROW: &str = "INSERT INTO generation_search(
    generation_id, repository_path, fact_ordinal,
    content_digest, artifact_digest, name_start, name_end,
    declaration_start, declaration_end, kind, name, qualified_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)";

const INSERT_REBUILD_SEARCH_ROW: &str = "INSERT INTO generation_search_rebuild(
    generation_id, repository_path, fact_ordinal,
    content_digest, artifact_digest, name_start, name_end,
    declaration_start, declaration_end, kind, name, qualified_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)";

const REBUILD_PRIMARY_BATCH: &str = "INSERT INTO generation_search(
    generation_id, repository_path, fact_ordinal,
    content_digest, artifact_digest, name_start, name_end,
    declaration_start, declaration_end, kind, name, qualified_name
)
SELECT generation.generation_id, file.repository_path, fact.ordinal,
       file.content_digest, file.artifact_digest, fact.name_start, fact.name_end,
       fact.declaration_start, fact.declaration_end, fact.kind, fact.name,
       fact.qualified_name
FROM index_generations AS generation
JOIN generation_files AS file USING (generation_id)
JOIN artifact_facts AS fact USING (artifact_digest)
WHERE generation.lifecycle_state IN ('ready', 'active', 'retained')
  AND (
    generation.generation_id > ?1 OR
    (generation.generation_id = ?1 AND file.ordinal > ?2) OR
    (generation.generation_id = ?1 AND file.ordinal = ?2 AND fact.ordinal > ?3)
  )
ORDER BY generation.generation_id, file.ordinal, fact.ordinal
LIMIT ?4";

const REBUILD_SHADOW_BATCH: &str = "INSERT INTO generation_search_rebuild(
    generation_id, repository_path, fact_ordinal,
    content_digest, artifact_digest, name_start, name_end,
    declaration_start, declaration_end, kind, name, qualified_name
)
SELECT generation.generation_id, file.repository_path, fact.ordinal,
       file.content_digest, file.artifact_digest, fact.name_start, fact.name_end,
       fact.declaration_start, fact.declaration_end, fact.kind, fact.name,
       fact.qualified_name
FROM index_generations AS generation
JOIN generation_files AS file USING (generation_id)
JOIN artifact_facts AS fact USING (artifact_digest)
WHERE generation.lifecycle_state IN ('ready', 'active', 'retained')
  AND (
    generation.generation_id > ?1 OR
    (generation.generation_id = ?1 AND file.ordinal > ?2) OR
    (generation.generation_id = ?1 AND file.ordinal = ?2 AND fact.ordinal > ?3)
  )
ORDER BY generation.generation_id, file.ordinal, fact.ordinal
LIMIT ?4";

const NEXT_PROJECTION_CURSOR: &str = "SELECT generation.generation_id, file.ordinal, fact.ordinal
FROM index_generations AS generation
JOIN generation_files AS file USING (generation_id)
JOIN artifact_facts AS fact USING (artifact_digest)
WHERE generation.lifecycle_state IN ('ready', 'active', 'retained')
  AND (
    generation.generation_id > ?1 OR
    (generation.generation_id = ?1 AND file.ordinal > ?2) OR
    (generation.generation_id = ?1 AND file.ordinal = ?2 AND fact.ordinal > ?3)
  )
ORDER BY generation.generation_id, file.ordinal, fact.ordinal
LIMIT 1 OFFSET ?4";

pub(super) struct WriterState {
    connection: Connection,
}

include!("writer/lifecycle.rs");
include!("writer/workspace.rs");
include!("writer/search_projection.rs");
include!("writer/staging.rs");
include!("writer/verification.rs");
include!("writer/graph.rs");
include!("writer/graph_artifact_verification.rs");
include!("writer/syntax_sites.rs");
include!("writer/repository_topology.rs");
include!("writer/scip_overlay.rs");
include!("writer/retention.rs");

include!("writer/memory.rs");
include!("writer/task.rs");
include!("writer/personal_memory.rs");
include!("writer/helpers.rs");

#[cfg(test)]
mod tests;
