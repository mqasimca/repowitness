fn diagnose_active_repository(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<RepositoryDiagnosticsPortResult<GenerationId, i64>, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = diagnostics_transaction(connection, repository);
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    match result {
        Ok(results) => {
            check_control(&cancelled, deadline)?;
            Ok(results)
        }
        Err(SearchFailure::Sqlite(error)) if is_interrupted(&error) => {
            check_control(&cancelled, deadline)?;
            Err(SqliteStoreError::DatabaseOperationFailed)
        }
        Err(SearchFailure::Sqlite(_)) => Err(SqliteStoreError::DatabaseOperationFailed),
        Err(SearchFailure::Store(error)) => {
            check_control(&cancelled, deadline)?;
            Err(error)
        }
    }
}

fn diagnostics_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
) -> Result<RepositoryDiagnosticsPortResult<GenerationId, i64>, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let source = active_generation_state(&transaction, repository)?;
    let memory = super::memory_reader::diagnostics_memory_projection(&transaction, repository)
        .map_err(SearchFailure::Store)?;
    transaction.commit()?;
    Ok(RepositoryDiagnosticsPortResult::new(
        source.snapshot,
        source.generation,
        source.source_epoch,
        source.producer_manifest,
        source.index_coverage,
        memory,
    ))
}
