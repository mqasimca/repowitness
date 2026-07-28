impl CodeSearchPort for OwnedSqliteReader {
    type Generation = GenerationId;
    type Error = SqliteStoreError;

    fn search(
        &self,
        repository: RepositoryIdentityDigest,
        query: &CodeSearchQuery,
        limits: CodeSearchLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<CodeSearchPortResult<Self::Generation>, Self::Error> {
        let storage_limits =
            SearchLimits::try_new(limits.max_results(), limits.max_output_bytes())?;
        let results = OwnedSqliteReader::search(
            self,
            repository,
            query.as_str(),
            storage_limits,
            cancelled,
            deadline,
        )?;
        let SearchResults {
            snapshot,
            generation,
            producer_manifest: _,
            index_coverage,
            hits,
            total_matches,
            output_bytes,
        } = results;
        let mut candidates = Vec::with_capacity(hits.len());
        for hit in hits.into_vec() {
            let occurrence = RustSymbolOccurrence::try_new(
                hit.fact_ordinal,
                SourceArtifactEvidence::new(hit.artifact_digest, hit.producer_manifest),
                hit.kind,
                hit.name,
                hit.qualified_name,
                hit.name_span,
                hit.declaration_span,
            )
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?
            .with_language(hit.language);
            candidates.push(CodeSearchCandidate::new(
                hit.path,
                hit.content_digest,
                occurrence,
            ));
        }
        Ok(CodeSearchPortResult::new(
            snapshot,
            generation,
            index_coverage,
            candidates,
            total_matches,
            output_bytes,
        ))
    }
}

impl MemoryRecallPort for OwnedSqliteReader {
    type Generation = GenerationId;
    type Projection = i64;
    type Error = SqliteStoreError;

    fn recall(
        &self,
        repository: RepositoryIdentityDigest,
        query: &MemoryRecallQuery,
        limits: MemoryRecallLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<MemoryRecallPortResult<Self::Generation, Self::Projection>, Self::Error> {
        self.recall_memory(repository, query, limits, cancelled, deadline)
    }
}

impl RepositoryDiagnosticsPort for OwnedSqliteReader {
    type Generation = GenerationId;
    type Projection = i64;
    type Error = SqliteStoreError;

    fn diagnose(
        &self,
        repository: RepositoryIdentityDigest,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RepositoryDiagnosticsPortResult<Self::Generation, Self::Projection>, Self::Error>
    {
        self.diagnostics(repository, cancelled, deadline)
    }
}

impl Drop for OwnedSqliteReader {
    fn drop(&mut self) {
        if self.worker.is_none() {
            return;
        }
        let (reply, _receiver) = mpsc::sync_channel(1);
        if self
            .commands
            .try_send(ReaderCommand::Shutdown { reply })
            .is_ok()
        {
            let _ = self.join_worker();
        } else {
            // Dropping the sender disconnects the worker after any queued
            // command. Do not wait without having delivered a shutdown.
            let _ = self.worker.take();
        }
    }
}

fn run_reader(connection: &mut Connection, receiver: Receiver<ReaderCommand>) {
    while let Ok(command) = receiver.recv() {
        match command {
            ReaderCommand::Search(command) => {
                let SearchCommand {
                    repository,
                    query,
                    limits,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result =
                    search_active(connection, repository, &query, limits, cancelled, deadline);
                let _ = reply.try_send(result);
            }
            ReaderCommand::GetSymbol(command) => {
                let SymbolCommand {
                    repository,
                    expected_snapshot,
                    expected_generation,
                    selector,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = get_active_symbol(
                    connection,
                    repository,
                    expected_snapshot,
                    expected_generation,
                    &selector,
                    cancelled,
                    deadline,
                );
                let _ = reply.try_send(result);
            }
            ReaderCommand::LoadArtifacts(command) => {
                let ArtifactCommand {
                    requested,
                    language,
                    identity,
                    limits,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = load_reusable_artifacts(
                    connection, &requested, language, identity, limits, cancelled, deadline,
                );
                let _ = reply.try_send(result);
            }
            ReaderCommand::RecallMemory(command) => {
                let MemoryRecallCommand {
                    repository,
                    query,
                    limits,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = recall_active_memory(
                    connection,
                    repository,
                    query.as_deref(),
                    limits,
                    cancelled,
                    deadline,
                );
                let _ = reply.try_send(result);
            }
            ReaderCommand::Diagnostics(command) => {
                let DiagnosticsCommand {
                    repository,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result =
                    diagnose_active_repository(connection, repository, cancelled, deadline);
                let _ = reply.try_send(result);
            }
            ReaderCommand::Shutdown { reply } => {
                let _ = reply.try_send(Ok(()));
                break;
            }
        }
    }
}

fn search_active(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    query: &str,
    limits: SearchLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<SearchResults, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = search_transaction(connection, repository, query, limits);
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
        Err(SearchFailure::Store(error)) => Err(error),
    }
}

fn get_active_symbol(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    expected_snapshot: SourceSnapshotDigest,
    expected_generation: GenerationId,
    selector: &SymbolGetSelector,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<SymbolLookupResults, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = symbol_transaction(
        connection,
        repository,
        expected_snapshot,
        expected_generation,
        selector,
    );
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
        Err(SearchFailure::Store(error)) => Err(error),
    }
}

fn load_reusable_artifacts(
    connection: &mut Connection,
    requested: &[AnalysisArtifactDigest],
    language: SourceLanguage,
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = artifact_transaction(
        connection, requested, language, identity, limits, &cancelled, deadline,
    );
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    match result {
        Ok(artifacts) => {
            check_control(&cancelled, deadline)?;
            Ok(artifacts)
        }
        Err(SearchFailure::Sqlite(error)) if is_interrupted(&error) => {
            check_control(&cancelled, deadline)?;
            Err(SqliteStoreError::DatabaseOperationFailed)
        }
        Err(SearchFailure::Sqlite(_)) => Err(SqliteStoreError::DatabaseOperationFailed),
        Err(SearchFailure::Store(error)) => Err(error),
    }
}
