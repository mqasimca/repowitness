use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use repowitness_application::{
    ImmutableRustSource, ImportMemoryRecordRequest, MemoryEvidenceOutcome, MemoryImportApproval,
    PackageScope, PublishSourceSlotIndexError, PublishSourceSlotIndexRequest,
    RepositoryIdentityTextV1, RustArtifactIdentity, RustIndexLimits, RustSourceSnapshotIdentity,
    ScipOverlayIdentityInput, ScipOverlayScopeIdentity, SourceSlotFinalFence,
    bounded_scip_importer_digest, evaluate_memory_projection, hash_scip_input,
    hash_source_snapshot, import_memory_record, prepare_rust_index, publish_source_slot_index,
    reviewed_scip_schema_digest,
};
use repowitness_domain::{
    AnalysisSchemaDigest, CanonicalMemoryDigest, ConfigurationDigest, ConnectedWorkspaceId,
    GitStateDigest, MemoryAuditActorId, MemoryCommitId, MemoryCorrespondenceReviewOperation,
    MemoryEvidence, MemoryLifecycle, MemoryObservationSource, MemoryPresentationDigest,
    MemoryProjectValidity, MemoryRecord, MemoryRecordedAtUnixMillis, MemoryRevalidationTarget,
    PersonalMemoryId, PersonalMemoryKind, PersonalMemoryProfileId, PersonalMemoryRecord,
    PersonalMemoryRevision, ProducerManifestDigest, RepositoryIdentityDigest, RepositoryPath,
    RepositoryPathLimits, ScipSymbol, SourceSlotId, SourceSnapshotDigest, TaskCheckpoint, TaskId,
    TaskState, TaskText, TaskVerification, TaskVerificationOutcome, WorktreeStateDigest,
};
use rusqlite::{Connection, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use crate::{
    BackupLimits, GenerationRetentionPolicy, MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS,
    MemoryFormatControl, OwnedSqliteReader, PreparedScipOverlay, ProjectionRebuildLimits,
    RetentionApplyRequest, RetentionLimits, RetentionPins, RetentionPlanRequest, SearchLimits,
    create_online_backup, parse_memory_record,
};

use super::{
    CompletedWorkspaceSource, GenerationCoverage, GenerationId, ObservedMemoryHistoryItem,
    OwnedSqliteIndex, SourceSlotEpoch, SqliteStoreError, WorkspaceSourceSlot, WorkspaceViewId,
    WorkspaceViewMember, WriterCommand, mutation_lease_path, receive_mutation_reply,
};
use crate::sqlite::memory_projection::{
    MemoryProjectionLoadLimits, MemoryProjectionResultLimits, PreparedMemoryProjection,
    PreparedProjectionCandidate, PreparedProjectionEvidence, PreparedProjectionRecord,
    PreparedProjectionRecordKind, ProjectionCandidateRelation, ProjectionEvidenceAssurance,
    ProjectionEvidenceOutcome, ProjectionOccurrence,
};
use crate::sqlite::memory_review::PreparedMemoryCorrespondenceReview;
use crate::sqlite::writer::MAX_STARTUP_RECOVERY_GENERATIONS;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(4096, 256);
const COMMIT_MEMORY_YAML: &[u8] = include_bytes!("../../../tests/fixtures/memory-v1/commit.yaml");
const WORKTREE_RELATIONSHIP_MEMORY_YAML: &[u8] =
    include_bytes!("../../../tests/fixtures/memory-v1/worktree-relationship.yaml");

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repowitness-owned-store-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture directory should be created");
        Self(path)
    }

    fn database(&self) -> PathBuf {
        self.0.join("index.sqlite3")
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn deadline() -> Instant {
    Instant::now()
        .checked_add(Duration::from_secs(5))
        .expect("test deadline should be representable")
}

fn memory_input(
    yaml: &[u8],
) -> (
    MemoryRecord,
    CanonicalMemoryDigest,
    MemoryPresentationDigest,
) {
    let cancelled = AtomicBool::new(false);
    let parsed = parse_memory_record(yaml, MemoryFormatControl::new(&cancelled, deadline()))
        .expect("memory fixture should parse");
    let revision = parsed.digest();
    (
        parsed.into_record(),
        revision,
        MemoryPresentationDigest::new(Sha256::digest(yaml).into()),
    )
}

fn memory_actor() -> MemoryAuditActorId {
    MemoryAuditActorId::try_new("trusted-test-actor".to_owned())
        .expect("fixture audit actor should be valid")
}

fn memory_recorded_at() -> MemoryRecordedAtUnixMillis {
    MemoryRecordedAtUnixMillis::try_new(1_722_000_000_000)
        .expect("fixture timestamp should be valid")
}

fn memory_source() -> MemoryObservationSource {
    MemoryObservationSource::Git(MemoryCommitId::Sha1([0x11; 20]))
}

include!("tests/memory_import.rs");
include!("tests/commit_outcomes.rs");
include!("tests/mutation_outcome.rs");
include!("tests/generation.rs");
include!("tests/projection_publication.rs");
include!("tests/progress_handler_cleanup.rs");
include!("tests/projection.rs");
include!("tests/recovery.rs");
include!("tests/source_slot_epochs.rs");
include!("tests/workspace.rs");
include!("tests/workspace_adversarial.rs");
include!("tests/workspace_publication.rs");
include!("tests/retention.rs");
include!("tests/retention_budget.rs");
include!("tests/retention_recovery.rs");
include!("tests/retention_roots.rs");
include!("tests/retention_views.rs");
include!("tests/graph.rs");
include!("tests/graph_adversarial.rs");
include!("tests/scip_overlay.rs");
include!("tests/tasks.rs");
