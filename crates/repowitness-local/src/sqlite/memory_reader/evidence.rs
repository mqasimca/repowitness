use std::collections::BTreeMap;

use repowitness_application::{
    MemoryRecallCandidate, MemoryRecallEvidence, MemoryRecallEvidenceOutcome,
    MemoryRecallOccurrence,
};
use repowitness_domain::{
    AnalysisArtifactDigest, CanonicalMemoryDigest, CorrespondenceFingerprintDigest,
    DeclarationDigest, MemoryRecordId, RepositoryPath, RepositoryPathLimits, SourceContentDigest,
    SourceSnapshotDigest,
};
use rusqlite::{Transaction, params};

use super::{
    ActiveMemoryProjection, RecallFailure, SqliteStoreError,
    decode::{
        memory_record_id, parse_candidate_relation, parse_evidence_assurance,
        parse_evidence_outcome, persisted_count,
    },
};
use crate::sqlite::memory_projection::{MANUAL_REVIEW_METHOD_ID, MANUAL_REVIEW_METHOD_VERSION};

const PERSISTED_PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(32_764, 32_764);

pub(super) fn load_projected_evidence(
    transaction: &Transaction<'_>,
    state: &ActiveMemoryProjection,
    record_ordinal: i64,
    record_id: MemoryRecordId,
    revision: CanonicalMemoryDigest,
) -> Result<Vec<MemoryRecallEvidence>, RecallFailure> {
    let mut candidates =
        load_projection_candidates(transaction, state, record_ordinal, record_id, revision)?;
    let raw_evidence = load_raw_projection_evidence(transaction, state, record_ordinal)?;
    let mut results = Vec::with_capacity(raw_evidence.len());
    for (expected_ordinal, raw) in raw_evidence.into_iter().enumerate() {
        let expected_ordinal = i64::try_from(expected_ordinal)
            .map_err(|_| RecallFailure::Store(SqliteStoreError::CountNotRepresentable))?;
        let projected = decode_projected_evidence(
            raw,
            state,
            expected_ordinal,
            record_id,
            revision,
            candidates.remove(&expected_ordinal).unwrap_or_default(),
        )?;
        results.push(projected);
    }
    if !candidates.is_empty() {
        return Err(RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    Ok(results)
}

fn load_raw_projection_evidence(
    transaction: &Transaction<'_>,
    state: &ActiveMemoryProjection,
    record_ordinal: i64,
) -> Result<Vec<RawProjectionEvidence>, RecallFailure> {
    let mut statement = transaction.prepare(
        "SELECT
            evidence.evidence_ordinal,
            evidence.record_id,
            evidence.revision_digest,
            evidence.outcome,
            evidence.method_id,
            evidence.method_version,
            evidence.assurance,
            evidence.target_snapshot_digest,
            evidence.target_repository_path,
            evidence.target_artifact_digest,
            evidence.target_fact_ordinal,
            evidence.target_declaration_digest,
            evidence.target_name_elided_digest,
            evidence.candidate_coverage,
            evidence.candidate_count_before_limit,
            file.content_digest
         FROM memory_projection_evidence AS evidence
         LEFT JOIN generation_files AS file
           ON file.generation_id = ?3
          AND file.repository_path = evidence.target_repository_path
          AND file.artifact_digest = evidence.target_artifact_digest
         WHERE evidence.projection_id = ?1
           AND evidence.record_ordinal = ?2
         ORDER BY evidence.evidence_ordinal",
    )?;
    let rows = statement.query_map(
        params![state.projection, record_ordinal, state.generation.get()],
        |row| {
            Ok(RawProjectionEvidence {
                ordinal: row.get(0)?,
                record_id: row.get(1)?,
                revision: row.get(2)?,
                outcome: row.get(3)?,
                method_id: row.get(4)?,
                method_version: row.get(5)?,
                assurance: row.get(6)?,
                target_snapshot: row.get(7)?,
                target_path: row.get(8)?,
                target_artifact: row.get(9)?,
                target_fact: row.get(10)?,
                target_declaration: row.get(11)?,
                target_name_elided: row.get(12)?,
                candidate_coverage: row.get(13)?,
                candidate_count: row.get(14)?,
                content_digest: row.get(15)?,
            })
        },
    )?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn decode_projected_evidence(
    raw: RawProjectionEvidence,
    state: &ActiveMemoryProjection,
    expected_ordinal: i64,
    record_id: MemoryRecordId,
    revision: CanonicalMemoryDigest,
    candidates: Vec<MemoryRecallCandidate>,
) -> Result<MemoryRecallEvidence, RecallFailure> {
    let outcome = parse_evidence_outcome(&raw.outcome)?;
    let method_matches = if outcome == MemoryRecallEvidenceOutcome::ReviewedLink {
        raw.method_id == MANUAL_REVIEW_METHOD_ID
            && u32::try_from(raw.method_version).ok() == Some(MANUAL_REVIEW_METHOD_VERSION)
    } else {
        raw.method_id == state.producer.id()
            && u32::try_from(raw.method_version).ok() == Some(state.producer.version())
    };
    if raw.ordinal != expected_ordinal
        || memory_record_id(&raw.record_id)? != record_id
        || CanonicalMemoryDigest::try_from_slice(&raw.revision)
            .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?
            != revision
        || !method_matches
    {
        return Err(RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    let target = optional_occurrence(
        raw.target_snapshot.as_deref(),
        raw.target_path.as_deref(),
        raw.target_artifact.as_deref(),
        raw.target_fact,
        raw.target_declaration.as_deref(),
        raw.target_name_elided.as_deref(),
        raw.content_digest.as_deref(),
        state.snapshot,
    )?;
    MemoryRecallEvidence::try_new(
        outcome,
        parse_evidence_assurance(&raw.assurance)?,
        target,
        match raw.candidate_coverage.as_str() {
            "complete" => true,
            "partial" => false,
            _ => {
                return Err(RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed));
            }
        },
        persisted_count(raw.candidate_count)?,
        candidates,
    )
    .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))
}

struct RawProjectionEvidence {
    ordinal: i64,
    record_id: Vec<u8>,
    revision: Vec<u8>,
    outcome: String,
    method_id: String,
    method_version: i64,
    assurance: String,
    target_snapshot: Option<Vec<u8>>,
    target_path: Option<Vec<u8>>,
    target_artifact: Option<Vec<u8>>,
    target_fact: Option<i64>,
    target_declaration: Option<Vec<u8>>,
    target_name_elided: Option<Vec<u8>>,
    candidate_coverage: String,
    candidate_count: i64,
    content_digest: Option<Vec<u8>>,
}

fn load_projection_candidates(
    transaction: &Transaction<'_>,
    state: &ActiveMemoryProjection,
    record_ordinal: i64,
    record_id: MemoryRecordId,
    revision: CanonicalMemoryDigest,
) -> Result<BTreeMap<i64, Vec<MemoryRecallCandidate>>, RecallFailure> {
    let mut statement = transaction.prepare(
        "SELECT
            candidate.evidence_ordinal,
            candidate.ordinal,
            candidate.record_id,
            candidate.revision_digest,
            candidate.target_snapshot_digest,
            candidate.target_repository_path,
            candidate.target_artifact_digest,
            candidate.target_fact_ordinal,
            candidate.target_declaration_digest,
            candidate.target_name_elided_digest,
            candidate.proposed_relation,
            candidate.method_id,
            candidate.method_version,
            candidate.assurance,
            file.content_digest
         FROM memory_projection_candidates AS candidate
         LEFT JOIN generation_files AS file
           ON file.generation_id = ?3
          AND file.repository_path = candidate.target_repository_path
          AND file.artifact_digest = candidate.target_artifact_digest
         WHERE candidate.projection_id = ?1
           AND candidate.record_ordinal = ?2
         ORDER BY candidate.evidence_ordinal, candidate.ordinal",
    )?;
    let rows = statement.query_map(
        params![state.projection, record_ordinal, state.generation.get()],
        |row| {
            Ok(RawProjectionCandidate {
                evidence_ordinal: row.get(0)?,
                ordinal: row.get(1)?,
                record_id: row.get(2)?,
                revision: row.get(3)?,
                target_snapshot: row.get(4)?,
                target_path: row.get(5)?,
                target_artifact: row.get(6)?,
                target_fact: row.get(7)?,
                target_declaration: row.get(8)?,
                target_name_elided: row.get(9)?,
                relation: row.get(10)?,
                method_id: row.get(11)?,
                method_version: row.get(12)?,
                assurance: row.get(13)?,
                content_digest: row.get(14)?,
            })
        },
    )?;
    let mut candidates = BTreeMap::<i64, Vec<MemoryRecallCandidate>>::new();
    for raw in rows {
        let raw = raw?;
        let grouped = candidates.entry(raw.evidence_ordinal).or_default();
        let expected_ordinal = i64::try_from(grouped.len())
            .map_err(|_| RecallFailure::Store(SqliteStoreError::CountNotRepresentable))?;
        if raw.evidence_ordinal < 0
            || raw.ordinal != expected_ordinal
            || memory_record_id(&raw.record_id)? != record_id
            || CanonicalMemoryDigest::try_from_slice(&raw.revision)
                .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?
                != revision
            || raw.method_id != state.producer.id()
            || u32::try_from(raw.method_version).ok() != Some(state.producer.version())
            || raw.assurance != "review_required"
        {
            return Err(RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed));
        }
        let occurrence = required_occurrence(
            &raw.target_snapshot,
            &raw.target_path,
            &raw.target_artifact,
            raw.target_fact,
            &raw.target_declaration,
            &raw.target_name_elided,
            raw.content_digest.as_deref(),
            state.snapshot,
        )?;
        grouped.push(MemoryRecallCandidate::new(
            occurrence,
            parse_candidate_relation(&raw.relation)?,
        ));
    }
    Ok(candidates)
}

struct RawProjectionCandidate {
    evidence_ordinal: i64,
    ordinal: i64,
    record_id: Vec<u8>,
    revision: Vec<u8>,
    target_snapshot: Vec<u8>,
    target_path: Vec<u8>,
    target_artifact: Vec<u8>,
    target_fact: i64,
    target_declaration: Vec<u8>,
    target_name_elided: Vec<u8>,
    relation: String,
    method_id: String,
    method_version: i64,
    assurance: String,
    content_digest: Option<Vec<u8>>,
}

#[allow(
    clippy::too_many_arguments,
    reason = "all exact occurrence identity components must become present together"
)]
fn optional_occurrence(
    snapshot: Option<&[u8]>,
    path: Option<&[u8]>,
    artifact: Option<&[u8]>,
    fact: Option<i64>,
    declaration: Option<&[u8]>,
    name_elided: Option<&[u8]>,
    content: Option<&[u8]>,
    expected_snapshot: SourceSnapshotDigest,
) -> Result<Option<MemoryRecallOccurrence>, RecallFailure> {
    match (
        snapshot,
        path,
        artifact,
        fact,
        declaration,
        name_elided,
        content,
    ) {
        (
            Some(snapshot),
            Some(path),
            Some(artifact),
            Some(fact),
            Some(declaration),
            Some(name_elided),
            Some(content),
        ) => required_occurrence(
            snapshot,
            path,
            artifact,
            fact,
            declaration,
            name_elided,
            Some(content),
            expected_snapshot,
        )
        .map(Some),
        (None, None, None, None, None, None, None) => Ok(None),
        _ => Err(RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed)),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "all exact occurrence identity components are independently verified"
)]
fn required_occurrence(
    snapshot: &[u8],
    path: &[u8],
    artifact: &[u8],
    fact: i64,
    declaration: &[u8],
    name_elided: &[u8],
    content: Option<&[u8]>,
    expected_snapshot: SourceSnapshotDigest,
) -> Result<MemoryRecallOccurrence, RecallFailure> {
    if SourceSnapshotDigest::try_from_slice(snapshot)
        .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?
        != expected_snapshot
    {
        return Err(RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    let content = content.ok_or(RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    Ok(MemoryRecallOccurrence::new(
        RepositoryPath::try_from_bytes(path, PERSISTED_PATH_LIMITS)
            .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?,
        SourceContentDigest::try_from_slice(content)
            .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?,
        AnalysisArtifactDigest::try_from_slice(artifact)
            .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?,
        u64::try_from(fact)
            .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?,
        DeclarationDigest::try_from_slice(declaration)
            .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?,
        CorrespondenceFingerprintDigest::try_from_slice(name_elided)
            .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?,
    ))
}
