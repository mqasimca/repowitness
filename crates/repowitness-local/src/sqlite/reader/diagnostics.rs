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
    let (syntax_error_nodes, known_parser_limitation_nodes) =
        active_parser_diagnostics(&transaction, &source)?;
    let memory = super::memory_reader::diagnostics_memory_projection(&transaction, repository)
        .map_err(SearchFailure::Store)?;
    transaction.commit()?;
    Ok(RepositoryDiagnosticsPortResult::new(
        source.snapshot,
        source.generation,
        source.source_epoch,
        source.producer_manifest,
        source.index_coverage,
        repowitness_application::RepositoryParserDiagnostics::new(
            syntax_error_nodes,
            known_parser_limitation_nodes,
        ),
        memory,
    ))
}

fn active_parser_diagnostics(
    transaction: &Transaction<'_>,
    source: &ActiveGenerationState,
) -> Result<(u64, u64), SearchFailure> {
    let persisted = transaction
        .query_row(
            "SELECT snapshot.total_syntax_error_nodes,
                    snapshot.file_count,
                    count(files.ordinal),
                    count(artifact.artifact_digest),
                    coalesce(sum(artifact.syntax_error_nodes), 0),
                    coalesce(sum(artifact.known_parser_limitation_nodes), 0),
                    coalesce(sum(
                        CASE
                            WHEN artifact.artifact_digest IS NOT NULL
                             AND (
                                artifact.visited_nodes < 0
                                OR artifact.syntax_error_nodes < 0
                                OR artifact.known_parser_limitation_nodes < 0
                                OR artifact.syntax_error_nodes > artifact.visited_nodes
                                OR artifact.known_parser_limitation_nodes
                                   > artifact.syntax_error_nodes
                             )
                            THEN 1
                            ELSE 0
                        END
                    ), 0)
             FROM index_generations AS generation
             JOIN source_snapshots AS snapshot
               ON snapshot.snapshot_digest = generation.snapshot_digest
              AND snapshot.lifecycle_state = 'complete'
             LEFT JOIN generation_files AS files
               ON files.generation_id = generation.generation_id
             LEFT JOIN analysis_artifacts AS artifact
               ON artifact.artifact_digest = files.artifact_digest
              AND artifact.lifecycle_state = 'complete'
             WHERE generation.generation_id = ?1
               AND generation.snapshot_digest = ?2
               AND generation.lifecycle_state = 'active'
             GROUP BY snapshot.total_syntax_error_nodes, snapshot.file_count",
            params![
                source.generation.get(),
                source.snapshot.as_bytes().as_slice()
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )
        .optional()?
        .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let syntax_error_nodes = persisted_count(persisted.0)?;
    let expected_files = persisted_count(persisted.1)?;
    let generation_files = persisted_count(persisted.2)?;
    let complete_artifacts = persisted_count(persisted.3)?;
    let artifact_syntax_error_nodes = persisted_count(persisted.4)?;
    let known_parser_limitation_nodes = persisted_count(persisted.5)?;
    let invalid_artifacts = persisted_count(persisted.6)?;
    if generation_files != expected_files
        || complete_artifacts != expected_files
        || artifact_syntax_error_nodes != syntax_error_nodes
        || known_parser_limitation_nodes > syntax_error_nodes
        || invalid_artifacts != 0
    {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    Ok((syntax_error_nodes, known_parser_limitation_nodes))
}
