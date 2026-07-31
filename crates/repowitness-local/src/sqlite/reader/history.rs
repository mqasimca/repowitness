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
