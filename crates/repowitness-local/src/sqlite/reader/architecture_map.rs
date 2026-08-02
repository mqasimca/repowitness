const FIXED_ARCHITECTURE_MAP_FILE_OUTPUT_BYTES: u64 = 136;

fn execute_architecture_map_command(connection: &mut Connection, command: ArchitectureMapCommand) {
    let ArchitectureMapCommand {
        repository,
        limits,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = read_active_architecture_map(connection, repository, limits, cancelled, deadline);
    let _ = reply.try_send(result);
}

fn read_active_architecture_map(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    limits: ArchitectureMapLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<ArchitectureMapPortResult<GenerationId>, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = architecture_map_transaction(connection, repository, limits);
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    match result {
        Ok(result) => {
            check_control(&cancelled, deadline)?;
            Ok(result)
        }
        Err(SearchFailure::Sqlite(error)) if is_interrupted(&error) => {
            check_control(&cancelled, deadline)?;
            Err(SqliteStoreError::DatabaseOperationFailed)
        }
        Err(SearchFailure::Sqlite(_)) => Err(SqliteStoreError::DatabaseOperationFailed),
        Err(SearchFailure::Store(error)) => Err(error),
    }
}

fn architecture_map_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    limits: ArchitectureMapLimits,
) -> Result<ArchitectureMapPortResult<GenerationId>, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let state = active_generation_state(&transaction, repository)?;
    let (total_files, total_declarations) = architecture_map_totals(&transaction, state.generation)?;
    let language_summaries = architecture_map_language_summaries(&transaction, state.generation)?;
    let (files, output_bytes) = architecture_map_files(&transaction, state.generation, limits)?;
    transaction.commit()?;
    Ok(ArchitectureMapPortResult::new(
        state.snapshot,
        state.generation,
        state.index_coverage,
        files,
        language_summaries,
        total_files,
        total_declarations,
        output_bytes,
    ))
}

fn architecture_map_totals(
    transaction: &Transaction<'_>,
    generation: GenerationId,
) -> Result<(u64, u64), SearchFailure> {
    let (files, declarations) = transaction.query_row(
        "SELECT COUNT(*), COALESCE(SUM(artifact.fact_count), 0)
         FROM generation_files AS file
         JOIN analysis_artifacts AS artifact
           ON artifact.artifact_digest = file.artifact_digest
          AND artifact.source_content_digest = file.content_digest
          AND artifact.lifecycle_state = 'complete'
         WHERE file.generation_id = ?1",
        [generation.get()],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )?;
    Ok((persisted_count(files)?, persisted_count(declarations)?))
}

fn architecture_map_language_summaries(
    transaction: &Transaction<'_>,
    generation: GenerationId,
) -> Result<Vec<ArchitectureMapLanguageSummary>, SearchFailure> {
    let mut statement = transaction.prepare(
        "SELECT artifact.language, COUNT(*), COALESCE(SUM(artifact.fact_count), 0)
         FROM generation_files AS file
         JOIN analysis_artifacts AS artifact
           ON artifact.artifact_digest = file.artifact_digest
          AND artifact.source_content_digest = file.content_digest
          AND artifact.lifecycle_state = 'complete'
         WHERE file.generation_id = ?1
         GROUP BY artifact.language
         ORDER BY artifact.language ASC",
    )?;
    let mut rows = statement.query([generation.get()])?;
    let mut summaries = Vec::new();
    while let Some(row) = rows.next()? {
        let language: String = row.get(0)?;
        let files: i64 = row.get(1)?;
        let declarations: i64 = row.get(2)?;
        let language = SourceLanguage::from_stable_str(&language)
            .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        summaries.push(ArchitectureMapLanguageSummary::new(
            language,
            persisted_count(files)?,
            persisted_count(declarations)?,
        ));
    }
    Ok(summaries)
}

fn architecture_map_files(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    limits: ArchitectureMapLimits,
) -> Result<(Vec<ArchitectureMapFile>, u64), SearchFailure> {
    let mut statement = transaction.prepare(
        "SELECT file.repository_path, file.content_digest, file.artifact_digest,
                artifact.producer_manifest_digest, artifact.language, artifact.fact_count
         FROM generation_files AS file
         JOIN analysis_artifacts AS artifact
           ON artifact.artifact_digest = file.artifact_digest
          AND artifact.source_content_digest = file.content_digest
          AND artifact.lifecycle_state = 'complete'
         WHERE file.generation_id = ?1
         ORDER BY file.repository_path ASC
         LIMIT ?2",
    )?;
    let mut rows = statement.query(params![generation.get(), i64::from(limits.max_files())])?;
    let mut files = Vec::with_capacity(usize::from(limits.max_files()));
    let mut output_bytes = 0_u64;
    while let Some(row) = rows.next()? {
        let file = read_architecture_map_file(row)?;
        let next_output = architecture_map_file_output_bytes(output_bytes, &file)?;
        if next_output > limits.max_output_bytes() {
            break;
        }
        output_bytes = next_output;
        files.push(file);
    }
    Ok((files, output_bytes))
}

fn read_architecture_map_file(
    row: &rusqlite::Row<'_>,
) -> Result<ArchitectureMapFile, SearchFailure> {
    let path_bytes: Vec<u8> = row.get(0)?;
    let content_digest: Vec<u8> = row.get(1)?;
    let artifact_digest: Vec<u8> = row.get(2)?;
    let producer_manifest: Vec<u8> = row.get(3)?;
    let language: String = row.get(4)?;
    let declaration_count: i64 = row.get(5)?;
    let path = RepositoryPath::try_from_bytes(&path_bytes, PERSISTED_PATH_LIMITS)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let content_digest = SourceContentDigest::try_from_slice(&content_digest)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let artifact_digest = AnalysisArtifactDigest::try_from_slice(&artifact_digest)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let producer_manifest = ProducerManifestDigest::try_from_slice(&producer_manifest)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let language = SourceLanguage::from_stable_str(&language)
        .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    Ok(ArchitectureMapFile::new(
        path,
        language,
        content_digest,
        artifact_digest,
        producer_manifest,
        persisted_count(declaration_count)?,
    ))
}

fn architecture_map_file_output_bytes(
    current: u64,
    file: &ArchitectureMapFile,
) -> Result<u64, SearchFailure> {
    let row_bytes = file
        .path()
        .byte_count()
        .get()
        .checked_add(FIXED_ARCHITECTURE_MAP_FILE_OUTPUT_BYTES)
        .and_then(|value| value.checked_add(u64::try_from(file.language().as_str().len()).ok()?))
        .ok_or(SearchFailure::Store(SqliteStoreError::CountNotRepresentable))?;
    current
        .checked_add(row_bytes)
        .ok_or(SearchFailure::Store(SqliteStoreError::CountNotRepresentable))
}
