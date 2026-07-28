mod decode;
mod evidence;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_application::{
    MemoryRecallLimits, MemoryRecallPortResult, MemoryRecallProducer,
    MemoryRecallProjectionCoverage, MemoryRecallRecord, RepositoryDiagnosticsMemoryProjection,
};
use repowitness_domain::{
    CanonicalMemoryDigest, CorrespondenceProfileDigest, MemoryDisplayRevision, MemoryRecord,
    MemoryRecordId, MemoryRevalidationTarget, RepositoryIdentityDigest, SourceSnapshotDigest,
};
use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params_from_iter,
    types::Value,
};

use self::{
    decode::{
        memory_record_id, parse_effective_state, parse_evidence_state, parse_reason, parse_target,
        parse_validity_state, persisted_count, persisted_u32,
    },
    evidence::load_projected_evidence,
};
use super::{GenerationId, SqliteStoreError};
use crate::memory_format::{MemoryFormatControl, parse_persisted_canonical_memory_record};

const PROGRESS_OPCODES: i32 = 1_000;
const MAX_QUERY_TERMS: usize = 8;
const MAX_QUERY_BYTES: usize = 256;
const MAX_TERM_BYTES: usize = 64;

pub(super) fn diagnostics_memory_projection(
    transaction: &Transaction<'_>,
    repository: RepositoryIdentityDigest,
) -> Result<Option<RepositoryDiagnosticsMemoryProjection<i64>>, SqliteStoreError> {
    match active_memory_projection(transaction, repository) {
        Ok(state) => Ok(Some(RepositoryDiagnosticsMemoryProjection::new(
            state.projection,
            state.source_epoch,
            state.snapshot,
            state.coverage,
        ))),
        Err(RecallFailure::Store(SqliteStoreError::MemoryProjectionUnavailable)) => Ok(None),
        Err(RecallFailure::Store(error)) => Err(error),
        Err(RecallFailure::Sqlite(_)) => Err(SqliteStoreError::DatabaseOperationFailed),
    }
}

pub(super) fn recall_active_memory(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    query: Option<&str>,
    limits: MemoryRecallLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<MemoryRecallPortResult<GenerationId, i64>, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let terms = canonical_query_terms(query)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = recall_transaction(connection, repository, &terms, limits, &cancelled, deadline);
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    match result {
        Ok(result) => {
            check_control(&cancelled, deadline)?;
            Ok(result)
        }
        Err(RecallFailure::Sqlite(error)) if is_interrupted(&error) => {
            check_control(&cancelled, deadline)?;
            Err(SqliteStoreError::DatabaseOperationFailed)
        }
        Err(RecallFailure::Sqlite(_)) => Err(SqliteStoreError::DatabaseOperationFailed),
        Err(RecallFailure::Store(error)) => Err(error),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one deferred transaction pins and validates the complete recall projection"
)]
fn recall_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    terms: &[String],
    limits: MemoryRecallLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<MemoryRecallPortResult<GenerationId, i64>, RecallFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let state = active_memory_projection(&transaction, repository)?;
    check_control(cancelled, deadline).map_err(RecallFailure::Store)?;
    let total_matches = count_matching_records(&transaction, state.projection, terms)?;
    let raw_records =
        select_matching_records(&transaction, state.projection, terms, limits.max_results())?;

    let mut records = Vec::with_capacity(raw_records.len());
    let mut output_bytes = 0_u64;
    let mut scan_bytes = 0_u64;
    for raw in raw_records {
        check_control(cancelled, deadline).map_err(RecallFailure::Store)?;
        let record_id = memory_record_id(&raw.record_id)?;
        let revision = raw
            .revision
            .as_deref()
            .map(CanonicalMemoryDigest::try_from_slice)
            .transpose()
            .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        let record = selected_record(
            &raw,
            record_id,
            revision,
            repository,
            &mut scan_bytes,
            limits.max_scan_bytes(),
            cancelled,
            deadline,
        )?;
        let evidence = if let Some(revision) = revision {
            load_projected_evidence(&transaction, &state, raw.ordinal, record_id, revision)?
        } else {
            Vec::new()
        };
        let projected = MemoryRecallRecord::try_new(
            record_id,
            revision,
            record,
            parse_effective_state(&raw.effective_state)?,
            parse_validity_state(&raw.validity_state)?,
            parse_evidence_state(&raw.evidence_state)?,
            parse_reason(&raw.reason)?,
            persisted_u32(raw.evidence_count)?,
            persisted_u32(raw.resolved_count)?,
            persisted_u32(raw.review_count)?,
            persisted_u32(raw.indeterminate_count)?,
            persisted_u32(raw.head_count)?,
            persisted_u32(raw.missing_parent_count)?,
            evidence,
        )
        .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        output_bytes = output_bytes
            .checked_add(
                projected
                    .encoded_output_bytes()
                    .map_err(|_| RecallFailure::Store(SqliteStoreError::CountNotRepresentable))?,
            )
            .ok_or(RecallFailure::Store(
                SqliteStoreError::CountNotRepresentable,
            ))?;
        if output_bytes > limits.max_output_bytes() {
            return Err(RecallFailure::Store(
                SqliteStoreError::MemoryRecallOutputLimitExceeded,
            ));
        }
        records.push(projected);
    }
    check_control(cancelled, deadline).map_err(RecallFailure::Store)?;
    transaction.commit()?;
    Ok(MemoryRecallPortResult::new(
        state.snapshot,
        state.generation,
        state.projection,
        state.source_epoch,
        state.target,
        state.producer,
        state.coverage,
        records,
        total_matches,
        output_bytes,
        scan_bytes,
    ))
}

struct ActiveMemoryProjection {
    projection: i64,
    generation: GenerationId,
    source_epoch: u64,
    snapshot: SourceSnapshotDigest,
    target: MemoryRevalidationTarget,
    producer: MemoryRecallProducer,
    coverage: MemoryRecallProjectionCoverage,
}

#[allow(
    clippy::too_many_lines,
    reason = "the active pointer query validates one exact immutable projection receipt"
)]
fn active_memory_projection(
    transaction: &Transaction<'_>,
    repository: RepositoryIdentityDigest,
) -> Result<ActiveMemoryProjection, RecallFailure> {
    let raw = transaction
        .query_row(
            "SELECT
                projection.projection_id,
                projection.index_generation_id,
                projection.source_epoch,
                projection.snapshot_digest,
                projection.target_kind,
                projection.target_format,
                projection.target_revision,
                projection.head_format,
                projection.head_revision,
                projection.correspondence_profile_id,
                projection.correspondence_profile_version,
                projection.correspondence_profile_digest,
                projection.searched_count,
                projection.skipped_count,
                projection.unresolved_count,
                projection.truncated_count,
                projection.total_count,
                projection.current_count,
                projection.not_applicable_count,
                projection.stale_count,
                projection.needs_review_count,
                projection.indeterminate_count,
                projection.conflicted_count,
                projection.contradicted_count,
                projection.superseded_count,
                projection.quarantined_count,
                projection.tombstoned_count
             FROM workspaces AS workspace
             JOIN active_memory_projections AS active
               ON active.workspace_id = workspace.workspace_id
             JOIN memory_projection_generations AS projection
               ON projection.projection_id = active.projection_id
              AND projection.workspace_id = workspace.workspace_id
              AND projection.lifecycle_state = 'complete'
              AND projection.source_epoch = workspace.source_epoch
              AND projection.index_generation_id = workspace.active_generation_id
             JOIN index_generations AS generation
               ON generation.generation_id = workspace.active_generation_id
              AND generation.workspace_id = workspace.workspace_id
              AND generation.source_epoch = workspace.source_epoch
              AND generation.snapshot_digest = projection.snapshot_digest
              AND generation.lifecycle_state = 'active'
             JOIN source_snapshots AS snapshot
               ON snapshot.snapshot_digest = projection.snapshot_digest
              AND snapshot.lifecycle_state = 'complete'
             WHERE workspace.repository_identity = ?1",
            [repository.as_bytes().as_slice()],
            |row| {
                Ok(RawProjection {
                    projection: row.get(0)?,
                    generation: row.get(1)?,
                    source_epoch: row.get(2)?,
                    snapshot: row.get(3)?,
                    target_kind: row.get(4)?,
                    target_format: row.get(5)?,
                    target_revision: row.get(6)?,
                    head_format: row.get(7)?,
                    head_revision: row.get(8)?,
                    producer_id: row.get(9)?,
                    producer_version: row.get(10)?,
                    producer_digest: row.get(11)?,
                    searched: row.get(12)?,
                    skipped: row.get(13)?,
                    unresolved: row.get(14)?,
                    truncated: row.get(15)?,
                    total: row.get(16)?,
                    current: row.get(17)?,
                    not_applicable: row.get(18)?,
                    stale: row.get(19)?,
                    needs_review: row.get(20)?,
                    indeterminate: row.get(21)?,
                    conflicted: row.get(22)?,
                    contradicted: row.get(23)?,
                    superseded: row.get(24)?,
                    quarantined: row.get(25)?,
                    tombstoned: row.get(26)?,
                })
            },
        )
        .optional()?
        .ok_or(RecallFailure::Store(
            SqliteStoreError::MemoryProjectionUnavailable,
        ))?;
    if raw.projection <= 0 || raw.generation <= 0 {
        return Err(RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    let source_epoch = persisted_count(raw.source_epoch)?;
    let snapshot = SourceSnapshotDigest::try_from_slice(&raw.snapshot)
        .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let target = parse_target(
        &raw.target_kind,
        &raw.target_format,
        &raw.target_revision,
        raw.head_format.as_deref(),
        raw.head_revision.as_deref(),
        snapshot,
    )?;
    let producer_version = u32::try_from(raw.producer_version)
        .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let producer_digest = CorrespondenceProfileDigest::try_from_slice(&raw.producer_digest)
        .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let producer =
        MemoryRecallProducer::try_new(raw.producer_id, producer_version, producer_digest)
            .map_err(|_| RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let coverage = MemoryRecallProjectionCoverage::new(
        persisted_count(raw.searched)?,
        persisted_count(raw.skipped)?,
        persisted_count(raw.unresolved)?,
        persisted_count(raw.truncated)?,
        persisted_count(raw.total)?,
        persisted_count(raw.current)?,
        persisted_count(raw.not_applicable)?,
        persisted_count(raw.stale)?,
        persisted_count(raw.needs_review)?,
        persisted_count(raw.indeterminate)?,
        persisted_count(raw.conflicted)?,
        persisted_count(raw.contradicted)?,
        persisted_count(raw.superseded)?,
        persisted_count(raw.quarantined)?,
        persisted_count(raw.tombstoned)?,
    );
    Ok(ActiveMemoryProjection {
        projection: raw.projection,
        generation: GenerationId::from_database(raw.generation),
        source_epoch,
        snapshot,
        target,
        producer,
        coverage,
    })
}

struct RawProjection {
    projection: i64,
    generation: i64,
    source_epoch: i64,
    snapshot: Vec<u8>,
    target_kind: String,
    target_format: String,
    target_revision: Vec<u8>,
    head_format: Option<String>,
    head_revision: Option<Vec<u8>>,
    producer_id: String,
    producer_version: i64,
    producer_digest: Vec<u8>,
    searched: i64,
    skipped: i64,
    unresolved: i64,
    truncated: i64,
    total: i64,
    current: i64,
    not_applicable: i64,
    stale: i64,
    needs_review: i64,
    indeterminate: i64,
    conflicted: i64,
    contradicted: i64,
    superseded: i64,
    quarantined: i64,
    tombstoned: i64,
}

fn count_matching_records(
    transaction: &Transaction<'_>,
    projection: i64,
    terms: &[String],
) -> Result<u64, RecallFailure> {
    let predicate = recall_predicate(terms.len(), 2);
    let sql = format!(
        "SELECT count(*)
         FROM memory_projection_records AS record
         LEFT JOIN memory_versions AS version
           ON version.workspace_id = record.workspace_id
          AND version.record_id = record.record_id
          AND version.revision_digest = record.revision_digest
         WHERE record.projection_id = ?1{predicate}"
    );
    let parameters = recall_parameters(projection, terms, None);
    let count = transaction.query_row(&sql, params_from_iter(parameters.iter()), |row| {
        row.get::<_, i64>(0)
    })?;
    persisted_count(count)
}

fn select_matching_records(
    transaction: &Transaction<'_>,
    projection: i64,
    terms: &[String],
    max_results: u16,
) -> Result<Vec<RawProjectionRecord>, RecallFailure> {
    let predicate = recall_predicate(terms.len(), 2);
    let limit_parameter = terms.len() + 2;
    let sql = format!(
        "SELECT
            record.ordinal,
            record.record_id,
            record.revision_digest,
            record.effective_state,
            record.validity_state,
            record.evidence_state,
            record.reason,
            record.evidence_count,
            record.resolved_count,
            record.review_count,
            record.indeterminate_count,
            record.head_count,
            record.missing_parent_count,
            record.has_trusted_approval,
            version.canonical_json,
            (
                SELECT audit.display_revision
                FROM memory_audit AS audit
                WHERE audit.workspace_id = record.workspace_id
                  AND audit.record_id = record.record_id
                  AND audit.revision_digest = record.revision_digest
                  AND audit.operation = 'locally_approved'
                ORDER BY audit.event_id
                LIMIT 1
            )
         FROM memory_projection_records AS record
         LEFT JOIN memory_versions AS version
           ON version.workspace_id = record.workspace_id
          AND version.record_id = record.record_id
          AND version.revision_digest = record.revision_digest
         WHERE record.projection_id = ?1{predicate}
         ORDER BY record.record_id
         LIMIT ?{limit_parameter}"
    );
    let parameters = recall_parameters(projection, terms, Some(i64::from(max_results)));
    let mut statement = transaction.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(parameters.iter()), |row| {
        Ok(RawProjectionRecord {
            ordinal: row.get(0)?,
            record_id: row.get(1)?,
            revision: row.get(2)?,
            effective_state: row.get(3)?,
            validity_state: row.get(4)?,
            evidence_state: row.get(5)?,
            reason: row.get(6)?,
            evidence_count: row.get(7)?,
            resolved_count: row.get(8)?,
            review_count: row.get(9)?,
            indeterminate_count: row.get(10)?,
            head_count: row.get(11)?,
            missing_parent_count: row.get(12)?,
            trusted_approval: row.get(13)?,
            canonical_json: row.get(14)?,
            display_revision: row.get(15)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

struct RawProjectionRecord {
    ordinal: i64,
    record_id: Vec<u8>,
    revision: Option<Vec<u8>>,
    effective_state: String,
    validity_state: String,
    evidence_state: String,
    reason: String,
    evidence_count: i64,
    resolved_count: i64,
    review_count: i64,
    indeterminate_count: i64,
    head_count: i64,
    missing_parent_count: i64,
    trusted_approval: i64,
    canonical_json: Option<Vec<u8>>,
    display_revision: Option<i64>,
}

#[allow(
    clippy::too_many_arguments,
    reason = "canonical-record integrity checks keep all selected projection identities explicit"
)]
fn selected_record(
    raw: &RawProjectionRecord,
    record_id: MemoryRecordId,
    revision: Option<CanonicalMemoryDigest>,
    repository: RepositoryIdentityDigest,
    scan_bytes: &mut u64,
    max_scan_bytes: u64,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Option<MemoryRecord>, RecallFailure> {
    if raw.trusted_approval != 1 || raw.ordinal < 0 {
        return Err(RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    let (Some(revision), Some(canonical_json), Some(display_revision)) = (
        revision,
        raw.canonical_json.as_deref(),
        raw.display_revision,
    ) else {
        if revision.is_some() || raw.canonical_json.is_some() || raw.display_revision.is_some() {
            return Err(RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed));
        }
        return Ok(None);
    };
    *scan_bytes = scan_bytes
        .checked_add(
            u64::try_from(canonical_json.len())
                .map_err(|_| RecallFailure::Store(SqliteStoreError::CountNotRepresentable))?,
        )
        .ok_or(RecallFailure::Store(
            SqliteStoreError::CountNotRepresentable,
        ))?;
    if *scan_bytes > max_scan_bytes {
        return Err(RecallFailure::Store(
            SqliteStoreError::MemoryRecallScanLimitExceeded,
        ));
    }
    let display_revision = u32::try_from(display_revision)
        .ok()
        .and_then(|value| MemoryDisplayRevision::try_new(value).ok())
        .ok_or(RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let parsed = parse_persisted_canonical_memory_record(
        canonical_json,
        display_revision,
        revision,
        MemoryFormatControl::new(cancelled, deadline),
    )
    .map_err(|error| match error {
        crate::MemoryFormatError::Cancelled => RecallFailure::Store(SqliteStoreError::Cancelled),
        crate::MemoryFormatError::DeadlineExceeded => {
            RecallFailure::Store(SqliteStoreError::DeadlineExceeded)
        }
        _ => RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed),
    })?;
    if parsed.record().header().record_id() != record_id
        || parsed.record().scope().repository() != repository
    {
        return Err(RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    Ok(Some(parsed.into_record()))
}

fn recall_predicate(term_count: usize, first_parameter: usize) -> String {
    (0..term_count)
        .map(|offset| {
            format!(
                " AND instr(lower(version.title || char(10) || version.body), ?{}) > 0",
                first_parameter + offset
            )
        })
        .collect()
}

fn recall_parameters(projection: i64, terms: &[String], limit: Option<i64>) -> Vec<Value> {
    let mut parameters = Vec::with_capacity(1 + terms.len() + usize::from(limit.is_some()));
    parameters.push(Value::Integer(projection));
    parameters.extend(terms.iter().cloned().map(Value::Text));
    if let Some(limit) = limit {
        parameters.push(Value::Integer(limit));
    }
    parameters
}

fn canonical_query_terms(query: Option<&str>) -> Result<Vec<String>, SqliteStoreError> {
    let Some(query) = query else {
        return Ok(Vec::new());
    };
    if query.is_empty() || query.len() > MAX_QUERY_BYTES {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    let terms = query.split(' ').collect::<Vec<_>>();
    if terms.is_empty()
        || terms.len() > MAX_QUERY_TERMS
        || terms.iter().any(|term| {
            term.is_empty()
                || term.len() > MAX_TERM_BYTES
                || term.chars().any(char::is_control)
                || term.bytes().any(|byte| byte.is_ascii_uppercase())
        })
    {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    Ok(terms.into_iter().map(str::to_owned).collect())
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), SqliteStoreError> {
    if cancelled.load(Ordering::Acquire) {
        Err(SqliteStoreError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(SqliteStoreError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

fn is_interrupted(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(code, _) if code.code == ErrorCode::OperationInterrupted
    )
}

enum RecallFailure {
    Sqlite(rusqlite::Error),
    Store(SqliteStoreError),
}

impl From<rusqlite::Error> for RecallFailure {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}
