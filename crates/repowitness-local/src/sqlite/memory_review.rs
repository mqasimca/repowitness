use repowitness_analysis::RustCorrespondenceCandidate;
use repowitness_domain::{
    AnalysisArtifactDigest, CanonicalMemoryDigest, CorrespondenceFingerprintDigest,
    DeclarationDigest, MemoryAuditActorId, MemoryCorrespondenceReviewOperation, MemoryRecordId,
    MemoryRecordedAtUnixMillis, RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{
    SqliteStoreError,
    memory_projection::{
        MANUAL_REVIEW_METHOD_ID, MANUAL_REVIEW_METHOD_VERSION, MemoryProjectionSource,
        ProjectionOccurrence, check_control, control_database_error, load_active_source,
        require_current_write_source, with_mutation_progress_handler, with_progress_handler,
    },
    writer::{WriteControl, WriterMutationResult, commit_mutation},
};

const MAX_REVIEW_EVENTS_PER_EVIDENCE: i64 = 4_096;
const MAX_CURRENT_REVIEW_EVENTS: i64 = 256;
const REVIEW_PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(32_764, 16_382);

pub(crate) struct PreparedMemoryCorrespondenceReview {
    repository: RepositoryIdentityDigest,
    record_id: MemoryRecordId,
    revision: CanonicalMemoryDigest,
    evidence_ordinal: u8,
    operation: MemoryCorrespondenceReviewOperation,
    target_path: RepositoryPath,
    target_artifact: AnalysisArtifactDigest,
    target_fact_ordinal: u64,
    actor: MemoryAuditActorId,
    recorded_at: MemoryRecordedAtUnixMillis,
}

impl PreparedMemoryCorrespondenceReview {
    #[allow(
        clippy::too_many_arguments,
        reason = "the complete review identity is intentionally explicit"
    )]
    pub(crate) const fn new(
        repository: RepositoryIdentityDigest,
        record_id: MemoryRecordId,
        revision: CanonicalMemoryDigest,
        evidence_ordinal: u8,
        operation: MemoryCorrespondenceReviewOperation,
        target_path: RepositoryPath,
        target_artifact: AnalysisArtifactDigest,
        target_fact_ordinal: u64,
        actor: MemoryAuditActorId,
        recorded_at: MemoryRecordedAtUnixMillis,
    ) -> Self {
        Self {
            repository,
            record_id,
            revision,
            evidence_ordinal,
            operation,
            target_path,
            target_artifact,
            target_fact_ordinal,
            actor,
            recorded_at,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MemoryCorrespondenceReviewReceipt {
    inserted: bool,
}

impl MemoryCorrespondenceReviewReceipt {
    pub(crate) const fn inserted(self) -> bool {
        self.inserted
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CorrespondenceReviewDecision {
    None,
    Reviewed(ProjectionOccurrence),
    Indeterminate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LoadedCorrespondenceReviews {
    decision: CorrespondenceReviewDecision,
    rejected: Vec<ReviewTargetIdentity>,
}

impl LoadedCorrespondenceReviews {
    pub(crate) const fn decision(&self) -> &CorrespondenceReviewDecision {
        &self.decision
    }

    pub(crate) fn rejects_candidate(&self, candidate: &RustCorrespondenceCandidate) -> bool {
        self.rejected.iter().any(|target| {
            target.path == *candidate.path()
                && target.artifact == candidate.artifact()
                && target.fact_ordinal == candidate.fact_ordinal()
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReviewTargetIdentity {
    path: RepositoryPath,
    artifact: AnalysisArtifactDigest,
    fact_ordinal: u64,
}

struct ReviewTargetAggregate {
    occurrence: ProjectionOccurrence,
    approved: bool,
    rejected: bool,
}

struct SourceOccurrence {
    snapshot: Vec<u8>,
    path: Vec<u8>,
    artifact: Vec<u8>,
    fact_ordinal: i64,
}

struct RawReview {
    operation: String,
    path: Vec<u8>,
    artifact: Vec<u8>,
    fact_ordinal: i64,
    declaration: Vec<u8>,
    name_elided: Vec<u8>,
}

pub(super) fn append_memory_correspondence_review(
    connection: &mut Connection,
    prepared: &PreparedMemoryCorrespondenceReview,
    control: WriteControl<'_>,
    force_progress_handler_clear_failure: bool,
) -> WriterMutationResult<MemoryCorrespondenceReviewReceipt> {
    with_mutation_progress_handler(
        connection,
        control,
        force_progress_handler_clear_failure,
        |connection| append_review_inner(connection, prepared, control),
    )
}

fn append_review_inner(
    connection: &mut Connection,
    prepared: &PreparedMemoryCorrespondenceReview,
    control: WriteControl<'_>,
) -> Result<MemoryCorrespondenceReviewReceipt, SqliteStoreError> {
    check_control(control)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| control_database_error(control))?;
    let source = load_active_source(&transaction, prepared.repository)?;
    require_current_write_source(&transaction, source, control)?;
    let source_occurrence = load_source_occurrence(&transaction, source, prepared, control)?;
    validate_target_occurrence(&transaction, source, prepared, control)?;
    if review_exists(&transaction, source, prepared, &source_occurrence, control)? {
        transaction
            .commit()
            .map_err(|_| control_database_error(control))?;
        return Ok(MemoryCorrespondenceReviewReceipt { inserted: false });
    }
    enforce_review_bounds(&transaction, source, prepared, control)?;
    check_control(control)?;
    let inserted = transaction
        .execute(
            "INSERT INTO memory_correspondence_audit(
                workspace_id, record_id, revision_digest, evidence_ordinal,
                operation, source_snapshot_digest, source_repository_path,
                source_artifact_digest, source_fact_ordinal,
                target_snapshot_digest, target_repository_path,
                target_artifact_digest, target_fact_ordinal,
                method_id, method_version, trusted_actor_kind,
                trusted_actor_id, recorded_at_unix_ms
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, ?15, 'local_asserted', ?16, ?17
             )
             ON CONFLICT(
                workspace_id, record_id, revision_digest, evidence_ordinal, operation,
                source_snapshot_digest, source_repository_path,
                source_artifact_digest, source_fact_ordinal,
                target_snapshot_digest, target_repository_path,
                target_artifact_digest, target_fact_ordinal,
                method_id, method_version, trusted_actor_kind, trusted_actor_id
             ) DO NOTHING",
            params![
                source.workspace_id(),
                prepared.record_id.as_bytes().as_slice(),
                prepared.revision.as_bytes().as_slice(),
                i64::from(prepared.evidence_ordinal),
                review_operation(prepared.operation),
                source_occurrence.snapshot,
                source_occurrence.path,
                source_occurrence.artifact,
                source_occurrence.fact_ordinal,
                source.snapshot().as_bytes().as_slice(),
                prepared.target_path.as_bytes(),
                prepared.target_artifact.as_bytes().as_slice(),
                fixed_integer(prepared.target_fact_ordinal)?,
                MANUAL_REVIEW_METHOD_ID,
                i64::from(MANUAL_REVIEW_METHOD_VERSION),
                prepared.actor.as_str(),
                fixed_integer(prepared.recorded_at.get())?,
            ],
        )
        .map_err(|_| control_database_error(control))?;
    if inserted > 1 {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    check_control(control)?;
    commit_mutation(transaction)?;
    Ok(MemoryCorrespondenceReviewReceipt {
        inserted: inserted == 1,
    })
}

pub(super) fn load_memory_correspondence_reviews(
    connection: &mut Connection,
    source: MemoryProjectionSource,
    record_id: MemoryRecordId,
    revision: CanonicalMemoryDigest,
    evidence_ordinal: u8,
    control: WriteControl<'_>,
) -> Result<LoadedCorrespondenceReviews, SqliteStoreError> {
    with_progress_handler(connection, control, |connection| {
        check_control(control)?;
        let current = load_active_source(connection, source.repository()).map_err(|error| {
            if error == SqliteStoreError::GenerationUnavailable {
                SqliteStoreError::StaleSourceEpoch
            } else {
                error
            }
        })?;
        if current != source {
            return Err(SqliteStoreError::StaleSourceEpoch);
        }
        load_reviews_inner(
            connection,
            source,
            record_id,
            revision,
            evidence_ordinal,
            control,
        )
    })
}

fn load_reviews_inner(
    connection: &Connection,
    source: MemoryProjectionSource,
    record_id: MemoryRecordId,
    revision: CanonicalMemoryDigest,
    evidence_ordinal: u8,
    control: WriteControl<'_>,
) -> Result<LoadedCorrespondenceReviews, SqliteStoreError> {
    let query_limit = MAX_CURRENT_REVIEW_EVENTS + 1;
    let rows = {
        let mut statement = connection
            .prepare(
                "SELECT audit.operation, audit.target_repository_path,
                        audit.target_artifact_digest, audit.target_fact_ordinal,
                        correspondence.declaration_digest,
                        correspondence.name_elided_digest
                 FROM memory_correspondence_audit AS audit
                 JOIN generation_files AS file
                   ON file.generation_id = ?1
                  AND file.repository_path = audit.target_repository_path
                  AND file.artifact_digest = audit.target_artifact_digest
                 JOIN artifact_fact_correspondence AS correspondence
                   ON correspondence.artifact_digest = audit.target_artifact_digest
                  AND correspondence.fact_ordinal = audit.target_fact_ordinal
                  AND correspondence.profile_id = 'rust-name-elided'
                  AND correspondence.profile_version = 1
                 WHERE audit.workspace_id = ?2
                   AND audit.record_id = ?3
                   AND audit.revision_digest = ?4
                   AND audit.evidence_ordinal = ?5
                   AND audit.target_snapshot_digest = ?6
                 ORDER BY audit.target_repository_path,
                          audit.target_artifact_digest,
                          audit.target_fact_ordinal,
                          audit.operation,
                          audit.trusted_actor_id
                 LIMIT ?7",
            )
            .map_err(|_| control_database_error(control))?;
        statement
            .query_map(
                params![
                    source.generation().get(),
                    source.workspace_id(),
                    record_id.as_bytes().as_slice(),
                    revision.as_bytes().as_slice(),
                    i64::from(evidence_ordinal),
                    source.snapshot().as_bytes().as_slice(),
                    query_limit,
                ],
                |row| {
                    Ok(RawReview {
                        operation: row.get(0)?,
                        path: row.get(1)?,
                        artifact: row.get(2)?,
                        fact_ordinal: row.get(3)?,
                        declaration: row.get(4)?,
                        name_elided: row.get(5)?,
                    })
                },
            )
            .map_err(|_| control_database_error(control))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| control_database_error(control))?
    };
    if i64::try_from(rows.len()).unwrap_or(i64::MAX) > MAX_CURRENT_REVIEW_EVENTS {
        return Err(SqliteStoreError::MemoryCorrespondenceReviewLimitExceeded);
    }
    aggregate_reviews(rows, control)
}

fn aggregate_reviews(
    rows: Vec<RawReview>,
    control: WriteControl<'_>,
) -> Result<LoadedCorrespondenceReviews, SqliteStoreError> {
    let mut targets: Vec<ReviewTargetAggregate> = Vec::new();
    for row in rows {
        check_control(control)?;
        let occurrence = decode_occurrence(&row)?;
        let operation = parse_review_operation(&row.operation)?;
        let same_as_last = targets.last().is_some_and(|target| {
            target.occurrence.path() == occurrence.path()
                && target.occurrence.artifact() == occurrence.artifact()
                && target.occurrence.fact_ordinal() == occurrence.fact_ordinal()
        });
        if !same_as_last {
            targets.push(ReviewTargetAggregate {
                occurrence,
                approved: false,
                rejected: false,
            });
        }
        let target = targets
            .last_mut()
            .ok_or(SqliteStoreError::IntegrityCheckFailed)?;
        match operation {
            MemoryCorrespondenceReviewOperation::Approved
            | MemoryCorrespondenceReviewOperation::ManualLink => target.approved = true,
            MemoryCorrespondenceReviewOperation::Rejected => target.rejected = true,
        }
    }
    let contradictory = targets
        .iter()
        .any(|target| target.approved && target.rejected);
    let approved_count = targets
        .iter()
        .filter(|target| target.approved && !target.rejected)
        .count();
    let decision = if contradictory || approved_count > 1 {
        CorrespondenceReviewDecision::Indeterminate
    } else if approved_count == 1 {
        let target = targets
            .iter()
            .find(|target| target.approved && !target.rejected)
            .ok_or(SqliteStoreError::IntegrityCheckFailed)?;
        CorrespondenceReviewDecision::Reviewed(target.occurrence.clone())
    } else {
        CorrespondenceReviewDecision::None
    };
    let rejected = targets
        .into_iter()
        .filter(|target| target.rejected && !target.approved)
        .map(|target| ReviewTargetIdentity {
            path: target.occurrence.path().clone(),
            artifact: target.occurrence.artifact(),
            fact_ordinal: target.occurrence.fact_ordinal(),
        })
        .collect();
    Ok(LoadedCorrespondenceReviews { decision, rejected })
}

fn load_source_occurrence(
    transaction: &rusqlite::Transaction<'_>,
    source: MemoryProjectionSource,
    prepared: &PreparedMemoryCorrespondenceReview,
    control: WriteControl<'_>,
) -> Result<SourceOccurrence, SqliteStoreError> {
    transaction
        .query_row(
            "SELECT evidence.source_snapshot_digest, evidence.repository_path,
                    evidence.artifact_digest, evidence.fact_ordinal
             FROM memory_evidence AS evidence
             WHERE evidence.workspace_id = ?1
               AND evidence.record_id = ?2
               AND evidence.revision_digest = ?3
               AND evidence.ordinal = ?4
               AND EXISTS (
                    SELECT 1 FROM memory_audit AS audit
                    WHERE audit.workspace_id = evidence.workspace_id
                      AND audit.record_id = evidence.record_id
                      AND audit.revision_digest = evidence.revision_digest
                      AND audit.operation = 'locally_approved'
               )",
            params![
                source.workspace_id(),
                prepared.record_id.as_bytes().as_slice(),
                prepared.revision.as_bytes().as_slice(),
                i64::from(prepared.evidence_ordinal),
            ],
            |row| {
                Ok(SourceOccurrence {
                    snapshot: row.get(0)?,
                    path: row.get(1)?,
                    artifact: row.get(2)?,
                    fact_ordinal: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(|_| control_database_error(control))?
        .ok_or(SqliteStoreError::InvalidMemoryCorrespondenceReview)
}

fn validate_target_occurrence(
    transaction: &rusqlite::Transaction<'_>,
    source: MemoryProjectionSource,
    prepared: &PreparedMemoryCorrespondenceReview,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    let exists = transaction
        .query_row(
            "SELECT 1
             FROM generation_files AS file
             JOIN analysis_artifacts AS artifact
               ON artifact.artifact_digest = file.artifact_digest
              AND artifact.lifecycle_state = 'complete'
              AND artifact.language = 'rust'
             JOIN artifact_fact_correspondence AS correspondence
               ON correspondence.artifact_digest = file.artifact_digest
              AND correspondence.fact_ordinal = ?4
              AND correspondence.profile_id = 'rust-name-elided'
              AND correspondence.profile_version = 1
             WHERE file.generation_id = ?1
               AND file.repository_path = ?2
               AND file.artifact_digest = ?3",
            params![
                source.generation().get(),
                prepared.target_path.as_bytes(),
                prepared.target_artifact.as_bytes().as_slice(),
                fixed_integer(prepared.target_fact_ordinal)?,
            ],
            |_| Ok(()),
        )
        .optional()
        .map_err(|_| control_database_error(control))?;
    exists.ok_or(SqliteStoreError::InvalidMemoryCorrespondenceReview)
}

fn review_exists(
    transaction: &rusqlite::Transaction<'_>,
    source: MemoryProjectionSource,
    prepared: &PreparedMemoryCorrespondenceReview,
    original: &SourceOccurrence,
    control: WriteControl<'_>,
) -> Result<bool, SqliteStoreError> {
    transaction
        .query_row(
            "SELECT 1 FROM memory_correspondence_audit
             WHERE workspace_id = ?1
               AND record_id = ?2
               AND revision_digest = ?3
               AND evidence_ordinal = ?4
               AND operation = ?5
               AND source_snapshot_digest = ?6
               AND source_repository_path = ?7
               AND source_artifact_digest = ?8
               AND source_fact_ordinal = ?9
               AND target_snapshot_digest = ?10
               AND target_repository_path = ?11
               AND target_artifact_digest = ?12
               AND target_fact_ordinal = ?13
               AND method_id = ?14
               AND method_version = ?15
               AND trusted_actor_kind = 'local_asserted'
               AND trusted_actor_id = ?16",
            params![
                source.workspace_id(),
                prepared.record_id.as_bytes().as_slice(),
                prepared.revision.as_bytes().as_slice(),
                i64::from(prepared.evidence_ordinal),
                review_operation(prepared.operation),
                original.snapshot,
                original.path,
                original.artifact,
                original.fact_ordinal,
                source.snapshot().as_bytes().as_slice(),
                prepared.target_path.as_bytes(),
                prepared.target_artifact.as_bytes().as_slice(),
                fixed_integer(prepared.target_fact_ordinal)?,
                MANUAL_REVIEW_METHOD_ID,
                i64::from(MANUAL_REVIEW_METHOD_VERSION),
                prepared.actor.as_str(),
            ],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
        .map_err(|_| control_database_error(control))
}

fn enforce_review_bounds(
    transaction: &rusqlite::Transaction<'_>,
    source: MemoryProjectionSource,
    prepared: &PreparedMemoryCorrespondenceReview,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    let counts = transaction
        .query_row(
            "SELECT count(*),
                    sum(CASE WHEN target_snapshot_digest = ?5 THEN 1 ELSE 0 END)
             FROM memory_correspondence_audit
             WHERE workspace_id = ?1
               AND record_id = ?2
               AND revision_digest = ?3
               AND evidence_ordinal = ?4",
            params![
                source.workspace_id(),
                prepared.record_id.as_bytes().as_slice(),
                prepared.revision.as_bytes().as_slice(),
                i64::from(prepared.evidence_ordinal),
                source.snapshot().as_bytes().as_slice(),
            ],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
        )
        .map_err(|_| control_database_error(control))?;
    if counts.0 >= MAX_REVIEW_EVENTS_PER_EVIDENCE
        || counts.1.unwrap_or(0) >= MAX_CURRENT_REVIEW_EVENTS
    {
        Err(SqliteStoreError::MemoryCorrespondenceReviewLimitExceeded)
    } else {
        Ok(())
    }
}

fn decode_occurrence(row: &RawReview) -> Result<ProjectionOccurrence, SqliteStoreError> {
    let path = RepositoryPath::try_from_vec(row.path.clone(), REVIEW_PATH_LIMITS)
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let artifact = AnalysisArtifactDigest::try_from_slice(&row.artifact)
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let fact_ordinal =
        u64::try_from(row.fact_ordinal).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let declaration = DeclarationDigest::try_from_slice(&row.declaration)
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let name_elided = CorrespondenceFingerprintDigest::try_from_slice(&row.name_elided)
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    Ok(ProjectionOccurrence::new(
        path,
        artifact,
        fact_ordinal,
        declaration,
        name_elided,
    ))
}

const fn review_operation(operation: MemoryCorrespondenceReviewOperation) -> &'static str {
    match operation {
        MemoryCorrespondenceReviewOperation::Approved => "approved",
        MemoryCorrespondenceReviewOperation::Rejected => "rejected",
        MemoryCorrespondenceReviewOperation::ManualLink => "manual_link",
    }
}

fn parse_review_operation(
    operation: &str,
) -> Result<MemoryCorrespondenceReviewOperation, SqliteStoreError> {
    match operation {
        "approved" => Ok(MemoryCorrespondenceReviewOperation::Approved),
        "rejected" => Ok(MemoryCorrespondenceReviewOperation::Rejected),
        "manual_link" => Ok(MemoryCorrespondenceReviewOperation::ManualLink),
        _ => Err(SqliteStoreError::IntegrityCheckFailed),
    }
}

fn fixed_integer(value: u64) -> Result<i64, SqliteStoreError> {
    i64::try_from(value).map_err(|_| SqliteStoreError::CountNotRepresentable)
}
