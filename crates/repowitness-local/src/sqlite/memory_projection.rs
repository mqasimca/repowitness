use std::{
    fmt,
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

use repowitness_analysis::{
    MAX_RUST_CORRESPONDENCE_CANDIDATES, RUST_CORRESPONDENCE_PROFILE_ID,
    RUST_CORRESPONDENCE_PROFILE_VERSION, RustAnalysisLimits, RustCorrespondenceCandidate,
    RustOccurrenceFingerprint, RustPathContinuity, RustSymbolFact, RustSymbolKind,
};
use repowitness_application::{
    MAX_MEMORY_PROJECTION_VERSIONS, MemoryEffectiveState, MemoryProjectionDecision,
    MemoryProjectionEvidenceState, MemoryProjectionReason, MemoryProjectionValidityState,
    phase0_rust_correspondence_profile_digest,
};
use repowitness_domain::{
    AnalysisArtifactDigest, ByteOffset, ByteSpan, CanonicalMemoryDigest,
    CorrespondenceFingerprintDigest, DeclarationDigest, GitStateDigest, MAX_MEMORY_EVIDENCE,
    MAX_MEMORY_INTEROPERABLE_INTEGER, MemoryCommitId, MemoryDisplayRevision, MemoryRecord,
    MemoryRecordId, MemoryRevalidationTarget, RepositoryIdentityDigest, RepositoryPath,
    RepositoryPathLimits, RustMemorySymbolKind, RustSymbolMemoryEvidence, SourceSnapshotDigest,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{
    GenerationId, SqliteStoreError,
    writer::{WriteControl, WriterMutationResult, commit_mutation},
};
use crate::memory_format::{MemoryFormatControl, parse_persisted_canonical_memory_record};

const MAX_PROJECTION_CANONICAL_BYTES: u64 = 256 * 1024 * 1024;
const DEFAULT_PROJECTION_CANONICAL_BYTES: u64 = 64 * 1024 * 1024;
const PERSISTED_PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(1_048_576, 65_535);
const PROGRESS_INSTRUCTIONS: i32 = 1_000;
pub(crate) const MANUAL_REVIEW_METHOD_ID: &str = "manual-review";
pub(crate) const MANUAL_REVIEW_METHOD_VERSION: u32 = 1;

/// Explicit complete-journal load limits for one projection rebuild.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MemoryProjectionLoadLimits {
    max_versions: u32,
    max_canonical_bytes: u64,
}

impl MemoryProjectionLoadLimits {
    pub(crate) fn try_new(
        max_versions: u32,
        max_canonical_bytes: u64,
    ) -> Result<Self, SqliteStoreError> {
        if max_versions == 0
            || usize::try_from(max_versions).unwrap_or(usize::MAX)
                > repowitness_application::MAX_MEMORY_PROJECTION_VERSIONS
            || max_canonical_bytes == 0
            || max_canonical_bytes > MAX_PROJECTION_CANONICAL_BYTES
        {
            return Err(SqliteStoreError::InvalidMemoryProjectionLimits);
        }
        Ok(Self {
            max_versions,
            max_canonical_bytes,
        })
    }

    pub(crate) const fn max_versions(self) -> u32 {
        self.max_versions
    }

    pub(crate) const fn max_canonical_bytes(self) -> u64 {
        self.max_canonical_bytes
    }
}

impl Default for MemoryProjectionLoadLimits {
    fn default() -> Self {
        Self {
            max_versions: repowitness_application::MAX_MEMORY_PROJECTION_VERSIONS as u32,
            max_canonical_bytes: DEFAULT_PROJECTION_CANONICAL_BYTES,
        }
    }
}

/// Exact immutable active-generation source pinned by a revalidation read.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct MemoryProjectionSource {
    repository: RepositoryIdentityDigest,
    workspace_id: i64,
    generation: GenerationId,
    source_epoch: u64,
    snapshot: SourceSnapshotDigest,
    git_state: GitStateDigest,
    searched_count: u64,
    skipped_count: u64,
    unresolved_count: u64,
    truncated_count: u64,
}

impl MemoryProjectionSource {
    pub(crate) const fn repository(self) -> RepositoryIdentityDigest {
        self.repository
    }

    pub(crate) const fn workspace_id(self) -> i64 {
        self.workspace_id
    }

    pub(crate) const fn generation(self) -> GenerationId {
        self.generation
    }

    pub(crate) const fn source_epoch(self) -> u64 {
        self.source_epoch
    }

    pub(crate) const fn snapshot(self) -> SourceSnapshotDigest {
        self.snapshot
    }

    pub(crate) const fn git_state(self) -> GitStateDigest {
        self.git_state
    }

    pub(crate) const fn has_complete_index_coverage(self) -> bool {
        self.unresolved_count == 0 && self.truncated_count == 0
    }
}

impl fmt::Debug for MemoryProjectionSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryProjectionSource")
            .field("workspace_id", &self.workspace_id)
            .field("generation", &self.generation)
            .field("source_epoch", &self.source_epoch)
            .field("snapshot", &self.snapshot)
            .field("searched_count", &self.searched_count)
            .field("skipped_count", &self.skipped_count)
            .field("unresolved_count", &self.unresolved_count)
            .field("truncated_count", &self.truncated_count)
            .finish_non_exhaustive()
    }
}

/// One integrity-checked immutable memory version loaded from the journal.
pub(crate) struct LoadedMemoryVersion {
    revision: CanonicalMemoryDigest,
    record: MemoryRecord,
    locally_approved: bool,
    approval_git_source: Option<MemoryCommitId>,
}

impl LoadedMemoryVersion {
    pub(crate) const fn revision(&self) -> CanonicalMemoryDigest {
        self.revision
    }

    pub(crate) const fn record(&self) -> &MemoryRecord {
        &self.record
    }

    pub(crate) const fn locally_approved(&self) -> bool {
        self.locally_approved
    }

    pub(crate) const fn approval_git_source(&self) -> Option<MemoryCommitId> {
        self.approval_git_source
    }
}

impl fmt::Debug for LoadedMemoryVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LoadedMemoryVersion")
            .field("revision", &self.revision)
            .field("record_id", &self.record.header().record_id())
            .field("locally_approved", &self.locally_approved)
            .field(
                "has_approval_git_source",
                &self.approval_git_source.is_some(),
            )
            .finish_non_exhaustive()
    }
}

/// Complete bounded immutable journal plus its exact active source.
pub(crate) struct LoadedMemoryJournal {
    source: MemoryProjectionSource,
    versions: Vec<LoadedMemoryVersion>,
}

impl LoadedMemoryJournal {
    pub(crate) const fn source(&self) -> MemoryProjectionSource {
        self.source
    }

    pub(crate) fn versions(&self) -> &[LoadedMemoryVersion] {
        &self.versions
    }
}

impl fmt::Debug for LoadedMemoryJournal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LoadedMemoryJournal")
            .field("source", &self.source)
            .field("version_count", &self.versions.len())
            .finish()
    }
}

/// Complete bounded candidate enumeration for one selected Rust citation.
pub(crate) struct LoadedRustCandidateSet {
    subject_name_elided: Option<CorrespondenceFingerprintDigest>,
    candidates: Vec<RustCorrespondenceCandidate>,
    candidate_count_before_limit: u64,
}

impl LoadedRustCandidateSet {
    pub(crate) const fn subject_name_elided(&self) -> Option<CorrespondenceFingerprintDigest> {
        self.subject_name_elided
    }

    pub(crate) fn into_candidates(self) -> Vec<RustCorrespondenceCandidate> {
        self.candidates
    }

    pub(crate) const fn candidate_count_before_limit(&self) -> u64 {
        self.candidate_count_before_limit
    }
}

impl fmt::Debug for LoadedRustCandidateSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LoadedRustCandidateSet")
            .field(
                "has_subject_name_elided",
                &self.subject_name_elided.is_some(),
            )
            .field("loaded_candidates", &self.candidates.len())
            .field(
                "candidate_count_before_limit",
                &self.candidate_count_before_limit,
            )
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProjectionEvidenceOutcome {
    Exact,
    SamePathRename,
    GitExactMove,
    ReviewedLink,
    Changed,
    Ambiguous,
    Missing,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProjectionEvidenceAssurance {
    Automatic,
    Reviewed,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProjectionCandidateRelation {
    Same,
    Moved,
    Renamed,
    MovedRenamed,
    Split,
    Merged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProjectionHeadReason {
    MissingParent,
    InvalidHeadGraph,
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct ProjectionOccurrence {
    path: RepositoryPath,
    artifact: AnalysisArtifactDigest,
    fact_ordinal: u64,
    declaration: DeclarationDigest,
    name_elided: CorrespondenceFingerprintDigest,
}

impl ProjectionOccurrence {
    pub(crate) const fn new(
        path: RepositoryPath,
        artifact: AnalysisArtifactDigest,
        fact_ordinal: u64,
        declaration: DeclarationDigest,
        name_elided: CorrespondenceFingerprintDigest,
    ) -> Self {
        Self {
            path,
            artifact,
            fact_ordinal,
            declaration,
            name_elided,
        }
    }

    pub(crate) fn from_candidate(candidate: &RustCorrespondenceCandidate) -> Self {
        Self {
            path: candidate.path().clone(),
            artifact: candidate.artifact(),
            fact_ordinal: candidate.fact_ordinal(),
            declaration: candidate.fingerprint().declaration(),
            name_elided: candidate.fingerprint().name_elided(),
        }
    }

    pub(crate) const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    pub(crate) const fn artifact(&self) -> AnalysisArtifactDigest {
        self.artifact
    }

    pub(crate) const fn fact_ordinal(&self) -> u64 {
        self.fact_ordinal
    }
}

impl fmt::Debug for ProjectionOccurrence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProjectionOccurrence")
            .field("path", &self.path)
            .field("artifact", &self.artifact)
            .field("fact_ordinal", &self.fact_ordinal)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedProjectionCandidate {
    pub(crate) occurrence: ProjectionOccurrence,
    pub(crate) relation: ProjectionCandidateRelation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedProjectionEvidence {
    pub(crate) outcome: ProjectionEvidenceOutcome,
    pub(crate) assurance: ProjectionEvidenceAssurance,
    pub(crate) target: Option<ProjectionOccurrence>,
    pub(crate) candidate_coverage_complete: bool,
    pub(crate) candidate_count_before_limit: u64,
    pub(crate) candidates: Vec<PreparedProjectionCandidate>,
}

impl PreparedProjectionEvidence {
    pub(crate) fn resolved(
        outcome: ProjectionEvidenceOutcome,
        assurance: ProjectionEvidenceAssurance,
        target: ProjectionOccurrence,
        candidate_count_before_limit: u64,
    ) -> Result<Self, SqliteStoreError> {
        if !matches!(
            outcome,
            ProjectionEvidenceOutcome::Exact
                | ProjectionEvidenceOutcome::SamePathRename
                | ProjectionEvidenceOutcome::GitExactMove
                | ProjectionEvidenceOutcome::Changed
        ) || (outcome == ProjectionEvidenceOutcome::Changed
            && assurance != ProjectionEvidenceAssurance::None)
            || (outcome != ProjectionEvidenceOutcome::Changed
                && assurance != ProjectionEvidenceAssurance::Automatic)
        {
            return Err(SqliteStoreError::InvalidMemoryProjection);
        }
        Ok(Self {
            outcome,
            assurance,
            target: Some(target),
            candidate_coverage_complete: true,
            candidate_count_before_limit,
            candidates: Vec::new(),
        })
    }

    pub(crate) const fn reviewed_link(
        target: ProjectionOccurrence,
        candidate_count_before_limit: u64,
        candidate_coverage_complete: bool,
    ) -> Self {
        Self {
            outcome: ProjectionEvidenceOutcome::ReviewedLink,
            assurance: ProjectionEvidenceAssurance::Reviewed,
            target: Some(target),
            candidate_coverage_complete,
            candidate_count_before_limit,
            candidates: Vec::new(),
        }
    }

    pub(crate) fn ambiguous(
        mut candidates: Vec<PreparedProjectionCandidate>,
    ) -> Result<Self, SqliteStoreError> {
        if candidates.is_empty() || candidates.len() > MAX_RUST_CORRESPONDENCE_CANDIDATES {
            return Err(SqliteStoreError::InvalidMemoryProjection);
        }
        candidates.sort_by(|left, right| occurrence_order(&left.occurrence, &right.occurrence));
        if candidates
            .windows(2)
            .any(|pair| occurrence_order(&pair[0].occurrence, &pair[1].occurrence).is_eq())
        {
            return Err(SqliteStoreError::InvalidMemoryProjection);
        }
        let candidate_count_before_limit =
            u64::try_from(candidates.len()).map_err(|_| SqliteStoreError::CountNotRepresentable)?;
        Ok(Self {
            outcome: ProjectionEvidenceOutcome::Ambiguous,
            assurance: ProjectionEvidenceAssurance::None,
            target: None,
            candidate_coverage_complete: true,
            candidate_count_before_limit,
            candidates,
        })
    }

    pub(crate) const fn missing(candidate_count_before_limit: u64) -> Self {
        Self {
            outcome: ProjectionEvidenceOutcome::Missing,
            assurance: ProjectionEvidenceAssurance::None,
            target: None,
            candidate_coverage_complete: true,
            candidate_count_before_limit,
            candidates: Vec::new(),
        }
    }

    pub(crate) const fn indeterminate(candidate_count_before_limit: u64) -> Self {
        Self {
            outcome: ProjectionEvidenceOutcome::Indeterminate,
            assurance: ProjectionEvidenceAssurance::None,
            target: None,
            candidate_coverage_complete: false,
            candidate_count_before_limit,
            candidates: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PreparedProjectionRecordKind {
    Evaluated {
        revision: CanonicalMemoryDigest,
        decision: MemoryProjectionDecision,
        evidence: Vec<PreparedProjectionEvidence>,
    },
    Conflicted {
        head_count: u32,
    },
    IndeterminateHead {
        revision: Option<CanonicalMemoryDigest>,
        evidence_count: u32,
        head_count: u32,
        missing_parent_count: u32,
        reason: ProjectionHeadReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedProjectionRecord {
    pub(crate) record_id: MemoryRecordId,
    pub(crate) kind: PreparedProjectionRecordKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedMemoryProjection {
    source: MemoryProjectionSource,
    target: MemoryRevalidationTarget,
    records: Vec<PreparedProjectionRecord>,
    skipped_count: u32,
    unresolved_count: u32,
    truncated_count: u32,
    state_counts: ProjectionStateCounts,
}

impl PreparedMemoryProjection {
    pub(crate) fn try_new(
        source: MemoryProjectionSource,
        target: MemoryRevalidationTarget,
        mut records: Vec<PreparedProjectionRecord>,
        skipped_count: u32,
        truncated_count: u32,
        limits: MemoryProjectionResultLimits,
    ) -> Result<Self, SqliteStoreError> {
        match target {
            MemoryRevalidationTarget::Worktree {
                source_snapshot, ..
            } if source_snapshot == source.snapshot => {}
            MemoryRevalidationTarget::Git { .. } => {}
            MemoryRevalidationTarget::Worktree { .. } => {
                return Err(SqliteStoreError::InvalidMemoryProjection);
            }
        }
        if records.len() > MAX_MEMORY_PROJECTION_VERSIONS {
            return Err(SqliteStoreError::MemoryProjectionLimitExceeded);
        }
        let covered_records = u64::try_from(records.len())
            .map_err(|_| SqliteStoreError::CountNotRepresentable)?
            .checked_add(u64::from(skipped_count))
            .and_then(|count| count.checked_add(u64::from(truncated_count)))
            .ok_or(SqliteStoreError::CountNotRepresentable)?;
        if covered_records > MAX_MEMORY_PROJECTION_VERSIONS as u64 {
            return Err(SqliteStoreError::MemoryProjectionLimitExceeded);
        }
        records.sort_by_key(|record| record.record_id);
        if records
            .windows(2)
            .any(|pair| pair[0].record_id == pair[1].record_id)
        {
            return Err(SqliteStoreError::InvalidMemoryProjection);
        }

        let mut candidates = 0_u64;
        let mut unresolved_count = 0_u32;
        let mut state_counts = ProjectionStateCounts::default();
        for record in &records {
            validate_projection_record(record)?;
            let effective = record_effective_state(record);
            state_counts.increment(effective)?;
            if matches!(
                effective,
                MemoryEffectiveState::NeedsReview
                    | MemoryEffectiveState::Indeterminate
                    | MemoryEffectiveState::Conflicted
            ) {
                unresolved_count = unresolved_count
                    .checked_add(1)
                    .ok_or(SqliteStoreError::CountNotRepresentable)?;
            }
            if let PreparedProjectionRecordKind::Evaluated { evidence, .. } = &record.kind {
                for result in evidence {
                    candidates = candidates
                        .checked_add(
                            u64::try_from(result.candidates.len())
                                .map_err(|_| SqliteStoreError::CountNotRepresentable)?,
                        )
                        .ok_or(SqliteStoreError::CountNotRepresentable)?;
                    if candidates > limits.max_candidates {
                        return Err(SqliteStoreError::MemoryProjectionLimitExceeded);
                    }
                }
            }
        }
        Ok(Self {
            source,
            target,
            records,
            skipped_count,
            unresolved_count,
            truncated_count,
            state_counts,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ProjectionStateCounts {
    current: u32,
    not_applicable: u32,
    stale: u32,
    needs_review: u32,
    indeterminate: u32,
    conflicted: u32,
    contradicted: u32,
    superseded: u32,
    quarantined: u32,
    tombstoned: u32,
}

impl ProjectionStateCounts {
    fn increment(&mut self, state: MemoryEffectiveState) -> Result<(), SqliteStoreError> {
        let value = match state {
            MemoryEffectiveState::Current => &mut self.current,
            MemoryEffectiveState::NotApplicable => &mut self.not_applicable,
            MemoryEffectiveState::Stale => &mut self.stale,
            MemoryEffectiveState::NeedsReview => &mut self.needs_review,
            MemoryEffectiveState::Indeterminate => &mut self.indeterminate,
            MemoryEffectiveState::Conflicted => &mut self.conflicted,
            MemoryEffectiveState::Contradicted => &mut self.contradicted,
            MemoryEffectiveState::Superseded => &mut self.superseded,
            MemoryEffectiveState::Quarantined => &mut self.quarantined,
            MemoryEffectiveState::Tombstoned => &mut self.tombstoned,
        };
        *value = value
            .checked_add(1)
            .ok_or(SqliteStoreError::CountNotRepresentable)?;
        Ok(())
    }
}

/// Receipt for one complete atomically activated memory projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MemoryProjectionPublication {
    projection_id: i64,
    projected_records: u32,
    skipped_records: u32,
    unresolved_records: u32,
}

impl MemoryProjectionPublication {
    pub(crate) const fn projection_id(self) -> i64 {
        self.projection_id
    }

    pub(crate) const fn projected_records(self) -> u32 {
        self.projected_records
    }

    pub(crate) const fn skipped_records(self) -> u32 {
        self.skipped_records
    }

    pub(crate) const fn unresolved_records(self) -> u32 {
        self.unresolved_records
    }
}

pub(super) fn load_memory_journal(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    limits: MemoryProjectionLoadLimits,
    control: WriteControl<'_>,
) -> Result<LoadedMemoryJournal, SqliteStoreError> {
    with_progress_handler(connection, control, |connection| {
        load_memory_journal_inner(connection, repository, limits, control)
    })
}

pub(super) fn load_memory_source(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    control: WriteControl<'_>,
) -> Result<MemoryProjectionSource, SqliteStoreError> {
    with_progress_handler(connection, control, |connection| {
        check_control(control)?;
        load_active_source(connection, repository)
    })
}

pub(super) fn load_rust_candidates(
    connection: &mut Connection,
    source: MemoryProjectionSource,
    evidence: &RustSymbolMemoryEvidence,
    control: WriteControl<'_>,
) -> Result<LoadedRustCandidateSet, SqliteStoreError> {
    with_progress_handler(connection, control, |connection| {
        check_control(control)?;
        let current = load_active_source(connection, source.repository).map_err(|error| {
            if error == SqliteStoreError::GenerationUnavailable {
                SqliteStoreError::StaleSourceEpoch
            } else {
                error
            }
        })?;
        if current != source {
            return Err(SqliteStoreError::StaleSourceEpoch);
        }
        load_rust_candidates_inner(connection, source, evidence, control)
    })
}

pub(super) fn publish_memory_projection(
    connection: &mut Connection,
    prepared: &PreparedMemoryProjection,
    control: WriteControl<'_>,
    force_progress_handler_clear_failure: bool,
) -> WriterMutationResult<MemoryProjectionPublication> {
    with_mutation_progress_handler(
        connection,
        control,
        force_progress_handler_clear_failure,
        |connection| publish_memory_projection_inner(connection, prepared, control),
    )
}

include!("memory_projection/limits.rs");
include!("memory_projection/publication.rs");
include!("memory_projection/encoding.rs");
include!("memory_projection/loading.rs");
include!("memory_projection/decoding.rs");
