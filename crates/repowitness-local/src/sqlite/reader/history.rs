/// Bounded history evidence read from the immutable memory journal.
///
/// This deliberately does not invoke Git or attempt to resolve an observed
/// object. An append-only observation says that the approved version was seen
/// at the exact commit; rewritten or pruned Git history remains historical
/// provenance rather than becoming a current-source claim.
#[allow(
    clippy::too_many_arguments,
    reason = "the source fence, provider bound, cancellation, and deadline are independent reader controls"
)]
fn read_trusted_git_history_evidence(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    expected_snapshot: SourceSnapshotDigest,
    expected_generation: GenerationId,
    expected_source_epoch: u64,
    max_results: u16,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<Vec<GitHistoryEvidence>, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = read_trusted_git_history_evidence_transaction(
        connection,
        repository,
        expected_snapshot,
        expected_generation,
        expected_source_epoch,
        max_results,
    );
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    match result {
        Ok(evidence) => {
            check_control(&cancelled, deadline)?;
            Ok(evidence)
        }
        Err(error) if is_interrupted(&error) => {
            check_control(&cancelled, deadline)?;
            Err(SqliteStoreError::DatabaseOperationFailed)
        }
        Err(_) => Err(SqliteStoreError::DatabaseOperationFailed),
    }
}

fn read_trusted_git_history_evidence_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    expected_snapshot: SourceSnapshotDigest,
    expected_generation: GenerationId,
    expected_source_epoch: u64,
    max_results: u16,
) -> Result<Vec<GitHistoryEvidence>, rusqlite::Error> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let rows = {
        let mut statement = transaction.prepare(
            "SELECT record.record_id, record.revision_digest,
                    audit.source_format, audit.source_revision
             FROM workspaces AS workspace
             JOIN active_memory_projections AS active
               ON active.workspace_id = workspace.workspace_id
             JOIN memory_projection_generations AS projection
               ON projection.projection_id = active.projection_id
              AND projection.workspace_id = workspace.workspace_id
              AND projection.lifecycle_state = 'complete'
             JOIN memory_projection_records AS record
               ON record.projection_id = projection.projection_id
              AND record.workspace_id = workspace.workspace_id
             JOIN memory_audit AS audit
               ON audit.workspace_id = record.workspace_id
              AND audit.record_id = record.record_id
              AND audit.revision_digest = record.revision_digest
              AND audit.operation = 'observed'
              AND audit.source_kind = 'git'
             WHERE workspace.repository_identity = ?1
               AND projection.snapshot_digest = ?2
               AND projection.index_generation_id = ?3
               AND projection.source_epoch = ?4
               AND record.effective_state = 'current'
               AND record.has_trusted_approval = 1
               AND record.revision_digest IS NOT NULL
             ORDER BY record.record_id, audit.event_id
             LIMIT ?5",
        )?;
        let mut rows = statement.query(params![
            repository.as_bytes().as_slice(),
            expected_snapshot.as_bytes().as_slice(),
            expected_generation.get(),
            i64::try_from(expected_source_epoch).map_err(|_| rusqlite::Error::InvalidQuery)?,
            i64::from(max_results),
        ])?;
        let mut evidence = Vec::new();
        while let Some(row) = rows.next()? {
            let record_id = row.get::<_, Vec<u8>>(0)?;
            let revision = row.get::<_, Vec<u8>>(1)?;
            let format = row.get::<_, String>(2)?;
            let source_revision = row.get::<_, Vec<u8>>(3)?;
            let record_id = <[u8; 16]>::try_from(record_id.as_slice())
                .map(MemoryRecordId::new)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            let revision = CanonicalMemoryDigest::try_from_slice(&revision)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            let commit = match format.as_str() {
                "sha1" => <[u8; 20]>::try_from(source_revision.as_slice())
                    .map(MemoryCommitId::Sha1)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                "sha256" => <[u8; 32]>::try_from(source_revision.as_slice())
                    .map(MemoryCommitId::Sha256)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            evidence.push(GitHistoryEvidence::new(record_id, revision, commit));
        }
        evidence
    };
    transaction.commit()?;
    Ok(rows)
}

fn execute_known_at_history_evidence_command(
    connection: &mut Connection,
    command: KnownAtHistoryEvidenceCommand,
) {
    let KnownAtHistoryEvidenceCommand {
        repository,
        known_at_unix_ms,
        max_results,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = read_known_at_trusted_git_history_evidence(
        connection,
        repository,
        known_at_unix_ms,
        max_results,
        cancelled,
        deadline,
    );
    let _ = reply.try_send(result);
}

fn read_known_at_trusted_git_history_evidence(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    known_at_unix_ms: repowitness_domain::MemoryRecordedAtUnixMillis,
    max_results: u16,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<Vec<GitHistoryEvidence>, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = read_known_at_trusted_git_history_evidence_transaction(
        connection,
        repository,
        known_at_unix_ms,
        max_results,
    );
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    match result {
        Ok(evidence) => {
            check_control(&cancelled, deadline)?;
            Ok(evidence)
        }
        Err(error) if is_interrupted(&error) => {
            check_control(&cancelled, deadline)?;
            Err(SqliteStoreError::DatabaseOperationFailed)
        }
        Err(_) => Err(SqliteStoreError::DatabaseOperationFailed),
    }
}

fn read_known_at_trusted_git_history_evidence_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    known_at_unix_ms: repowitness_domain::MemoryRecordedAtUnixMillis,
    max_results: u16,
) -> Result<Vec<GitHistoryEvidence>, rusqlite::Error> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let rows = {
        let mut statement = transaction.prepare(
            "SELECT observed.record_id, observed.revision_digest,
                    observed.source_format, observed.source_revision
               FROM workspaces AS workspace
               JOIN memory_audit AS observed
                 ON observed.workspace_id = workspace.workspace_id
                AND observed.operation = 'observed'
                AND observed.source_kind = 'git'
                AND observed.recorded_at_unix_ms <= ?2
              WHERE workspace.repository_identity = ?1
                AND EXISTS (
                    SELECT 1 FROM memory_audit AS approved
                     WHERE approved.workspace_id = observed.workspace_id
                       AND approved.record_id = observed.record_id
                       AND approved.revision_digest = observed.revision_digest
                       AND approved.operation = 'locally_approved'
                       AND approved.recorded_at_unix_ms <= ?2
                )
              ORDER BY observed.record_id, observed.revision_digest, observed.event_id
              LIMIT ?3",
        )?;
        let mut rows = statement.query(params![
            repository.as_bytes().as_slice(),
            i64::try_from(known_at_unix_ms.get()).map_err(|_| rusqlite::Error::InvalidQuery)?,
            i64::from(max_results),
        ])?;
        let mut evidence = Vec::new();
        while let Some(row) = rows.next()? {
            let record_id = row.get::<_, Vec<u8>>(0)?;
            let revision = row.get::<_, Vec<u8>>(1)?;
            let format = row.get::<_, String>(2)?;
            let source_revision = row.get::<_, Vec<u8>>(3)?;
            let record_id = <[u8; 16]>::try_from(record_id.as_slice())
                .map(MemoryRecordId::new)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            let revision = CanonicalMemoryDigest::try_from_slice(&revision)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            let commit = match format.as_str() {
                "sha1" => <[u8; 20]>::try_from(source_revision.as_slice())
                    .map(MemoryCommitId::Sha1)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                "sha256" => <[u8; 32]>::try_from(source_revision.as_slice())
                    .map(MemoryCommitId::Sha256)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            evidence.push(GitHistoryEvidence::new(record_id, revision, commit));
        }
        evidence
    };
    transaction.commit()?;
    Ok(rows)
}

fn execute_known_at_history_receipt_command(
    connection: &mut Connection,
    command: KnownAtHistoryReceiptCommand,
) {
    let KnownAtHistoryReceiptCommand {
        repository,
        known_at_unix_ms,
        target,
        max_results,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = read_known_at_history_receipt(
        connection,
        repository,
        known_at_unix_ms,
        target,
        max_results,
        cancelled,
        deadline,
    );
    let _ = reply.try_send(result);
}

#[allow(
    clippy::too_many_arguments,
    reason = "the cutoff, concrete target, bound, cancellation, and deadline are independent trust controls"
)]
fn read_known_at_history_receipt(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    known_at_unix_ms: repowitness_domain::MemoryRecordedAtUnixMillis,
    target: MemoryObservationSource,
    max_results: u16,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<KnownAtHistoryReceipt, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = read_known_at_history_receipt_transaction(
        connection,
        repository,
        known_at_unix_ms,
        target,
        max_results,
    );
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    match result {
        Ok((evidence, coverage, applicability)) => {
            check_control(&cancelled, deadline)?;
            Ok(KnownAtHistoryReceipt::new(
                known_at_unix_ms,
                target,
                evidence,
                coverage,
                applicability,
            ))
        }
        Err(error) if is_interrupted(&error) => {
            check_control(&cancelled, deadline)?;
            Err(SqliteStoreError::DatabaseOperationFailed)
        }
        Err(_) => Err(SqliteStoreError::DatabaseOperationFailed),
    }
}

const KNOWN_AT_HISTORY_EVIDENCE_QUERY: &str = "SELECT record_id, revision_digest, basis, event_id
   FROM (
        SELECT observed.record_id AS record_id,
               observed.revision_digest AS revision_digest,
               'observation' AS basis,
               observed.event_id AS event_id
          FROM workspaces AS workspace
          JOIN memory_audit AS observed
            ON observed.workspace_id = workspace.workspace_id
           AND observed.operation = 'observed'
           AND observed.recorded_at_unix_ms <= ?2
           AND observed.source_kind = ?3
           AND observed.source_format = ?4
           AND observed.source_revision = ?5
         WHERE workspace.repository_identity = ?1
           AND EXISTS (
               SELECT 1 FROM memory_audit AS approved
                WHERE approved.workspace_id = observed.workspace_id
                  AND approved.record_id = observed.record_id
                  AND approved.revision_digest = observed.revision_digest
                  AND approved.operation = 'locally_approved'
                  AND approved.recorded_at_unix_ms <= ?2
           )
        UNION ALL
        SELECT review.record_id AS record_id,
               review.revision_digest AS revision_digest,
               'reviewed_correspondence' AS basis,
               review.event_id AS event_id
          FROM workspaces AS workspace
          JOIN memory_correspondence_audit AS review
            ON review.workspace_id = workspace.workspace_id
           AND review.target_snapshot_digest = ?7
           AND review.operation IN ('approved', 'manual_link')
           AND review.recorded_at_unix_ms <= ?2
         WHERE workspace.repository_identity = ?1
           AND EXISTS (
               SELECT 1 FROM memory_audit AS approved
                WHERE approved.workspace_id = review.workspace_id
                  AND approved.record_id = review.record_id
                  AND approved.revision_digest = review.revision_digest
                  AND approved.operation = 'locally_approved'
                  AND approved.recorded_at_unix_ms <= ?2
           )
           AND NOT EXISTS (
               SELECT 1 FROM memory_correspondence_audit AS rejected
                WHERE rejected.workspace_id = review.workspace_id
                  AND rejected.record_id = review.record_id
                  AND rejected.revision_digest = review.revision_digest
                  AND rejected.evidence_ordinal = review.evidence_ordinal
                  AND rejected.target_snapshot_digest = review.target_snapshot_digest
                  AND rejected.target_repository_path = review.target_repository_path
                  AND rejected.target_artifact_digest = review.target_artifact_digest
                  AND rejected.target_fact_ordinal = review.target_fact_ordinal
                  AND rejected.operation = 'rejected'
                  AND rejected.recorded_at_unix_ms <= ?2
           )
   )
  ORDER BY record_id, revision_digest, basis, event_id
  LIMIT ?6";

fn read_known_at_history_receipt_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    known_at_unix_ms: repowitness_domain::MemoryRecordedAtUnixMillis,
    target: MemoryObservationSource,
    max_results: u16,
) -> Result<
    (
        Vec<KnownAtObservationEvidence>,
        KnownAtHistoryCoverage,
        KnownAtApplicability,
    ),
    rusqlite::Error,
> {
    let (source_kind, source_format, source_revision) = match target {
        MemoryObservationSource::Git(MemoryCommitId::Sha1(bytes)) => {
            ("git", "sha1", bytes.to_vec())
        }
        MemoryObservationSource::Git(MemoryCommitId::Sha256(bytes)) => {
            ("git", "sha256", bytes.to_vec())
        }
        MemoryObservationSource::Worktree(snapshot) => {
            ("worktree", "source_snapshot", snapshot.as_bytes().to_vec())
        }
    };
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let target_snapshot_bytes = match target {
        MemoryObservationSource::Worktree(snapshot) => snapshot.as_bytes().to_vec(),
        MemoryObservationSource::Git(_) => Vec::new(),
    };
    let target_retained = match target {
        MemoryObservationSource::Git(_) => None,
        MemoryObservationSource::Worktree(snapshot) => Some(
            transaction
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1
                          FROM source_snapshots
                         WHERE snapshot_digest = ?1
                           AND repository_identity = ?2
                           AND lifecycle_state = 'complete'
                    )",
                    params![
                        snapshot.as_bytes().as_slice(),
                        repository.as_bytes().as_slice(),
                    ],
                    |row| row.get::<_, i64>(0),
                )?
                != 0,
        ),
    };
    let mut rows = {
        let mut statement = transaction.prepare(KNOWN_AT_HISTORY_EVIDENCE_QUERY)?;
        let mut rows = statement.query(params![
            repository.as_bytes().as_slice(),
            i64::try_from(known_at_unix_ms.get()).map_err(|_| rusqlite::Error::InvalidQuery)?,
            source_kind,
            source_format,
            source_revision,
            i64::from(max_results) + 1,
            target_snapshot_bytes,
        ])?;
        let mut evidence = Vec::new();
        while let Some(row) = rows.next()? {
            let record_id = row.get::<_, Vec<u8>>(0)?;
            let revision = row.get::<_, Vec<u8>>(1)?;
            let basis = row.get::<_, String>(2)?;
            let record_id = <[u8; 16]>::try_from(record_id.as_slice())
                .map(MemoryRecordId::new)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            let revision = CanonicalMemoryDigest::try_from_slice(&revision)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            let basis = match basis.as_str() {
                "observation" => KnownAtEvidenceBasis::Observation,
                "reviewed_correspondence" => KnownAtEvidenceBasis::ReviewedCorrespondence,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            evidence.push(KnownAtObservationEvidence::new(
                record_id, revision, target, basis,
            ));
        }
        evidence
    };
    transaction.commit()?;
    let coverage = if rows.len() > usize::from(max_results) {
        rows.pop();
        KnownAtHistoryCoverage::Truncated
    } else {
        KnownAtHistoryCoverage::Complete
    };
    let applicability = match target_retained {
        Some(false) | None => KnownAtApplicability::Unavailable,
        Some(true) if rows.is_empty() => KnownAtApplicability::NotApplicable,
        Some(true) => KnownAtApplicability::Applicable,
    };
    Ok((rows, coverage, applicability))
}
