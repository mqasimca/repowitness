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
        code_search_port_result_from_search_results(results)
    }
}

pub(crate) fn code_search_port_result_from_search_results(
    results: SearchResults,
) -> Result<CodeSearchPortResult<GenerationId>, SqliteStoreError> {
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

impl SymbolSearchPort for OwnedSqliteReader {
    type Generation = GenerationId;
    type Error = SqliteStoreError;

    fn search_symbols(
        &self,
        repository: RepositoryIdentityDigest,
        query: &SymbolSearchQuery,
        limits: CodeSearchLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<CodeSearchPortResult<Self::Generation>, Self::Error> {
        let storage_limits =
            SearchLimits::try_new(limits.max_results(), limits.max_output_bytes())?;
        let results = OwnedSqliteReader::search_symbols(
            self,
            repository,
            query.clone(),
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

impl ArchitectureMapPort for OwnedSqliteReader {
    type Generation = GenerationId;
    type Error = SqliteStoreError;

    fn architecture_map(
        &self,
        repository: RepositoryIdentityDigest,
        limits: ArchitectureMapLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ArchitectureMapPortResult<Self::Generation>, Self::Error> {
        Self::architecture_map(self, repository, limits, cancelled, deadline)
    }
}

impl ArchitectureOverviewPort for OwnedSqliteReader {
    type Generation = GenerationId;
    type Error = SqliteStoreError;

    fn architecture_overview(
        &self,
        repository: RepositoryIdentityDigest,
        limits: ArchitectureOverviewLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ArchitectureOverviewPortResult<Self::Generation>, Self::Error> {
        Self::architecture_overview(self, repository, limits, cancelled, deadline)
    }
}

impl RepositoryTopologyPort for OwnedSqliteReader {
    type Generation = GenerationId;
    type Error = SqliteStoreError;

    fn repository_topology(
        &self,
        repository: RepositoryIdentityDigest,
        limits: RepositoryTopologyLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RepositoryTopologyPortResult<Self::Generation>, Self::Error> {
        Self::repository_topology(self, repository, limits, cancelled, deadline)
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

impl repowitness_application::PersonalMemoryReadPort for OwnedSqliteReader {
    type Error = SqliteStoreError;

    fn read_personal_memory(
        &self,
        profile: PersonalMemoryProfileId,
        repository: RepositoryIdentityDigest,
        limit: u16,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Vec<PersonalMemoryRecord>, Self::Error> {
        Self::read_personal_memory(self, profile, repository, limit, cancelled, deadline)
    }
}

impl repowitness_application::TaskStatusPort for OwnedSqliteReader {
    type Error = SqliteStoreError;

    fn task_status(
        &self,
        repository: RepositoryIdentityDigest,
        task_id: TaskId,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Option<TaskStatus>, Self::Error> {
        Self::task_status(self, repository, task_id, cancelled, deadline)
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
        if execute_reader_command(connection, command) {
            break;
        }
    }
}

fn execute_reader_command(connection: &mut Connection, command: ReaderCommand) -> bool {
    match command {
        ReaderCommand::Search(command) => execute_search_command(connection, *command),
        ReaderCommand::SymbolSearch(command) => execute_symbol_search_command(connection, *command),
        ReaderCommand::ArchitectureMap(command) => {
            execute_architecture_map_command(connection, *command)
        }
        ReaderCommand::WorkspaceArchitectureMap(command) => {
            execute_workspace_architecture_map_command(connection, *command)
        }
        ReaderCommand::ArchitectureOverview(command) => {
            execute_architecture_overview_command(connection, *command)
        }
        ReaderCommand::RepositoryTopology(command) => {
            execute_repository_topology_command(connection, *command)
        }
        ReaderCommand::WorkspaceSearch(command) => {
            execute_workspace_search_command(connection, *command)
        }
        ReaderCommand::WorkspaceSymbolSearch(command) => {
            execute_workspace_symbol_search_command(connection, *command)
        }
        ReaderCommand::GetSymbol(command) => execute_symbol_command(connection, *command),
        ReaderCommand::LoadArtifacts(command) => execute_artifact_command(connection, *command),
        ReaderCommand::LoadGraphArtifacts(command) => {
            execute_graph_artifact_command(connection, *command);
        }
        ReaderCommand::LoadRawSyntaxArtifacts(command) => {
            execute_raw_syntax_artifact_command(connection, *command);
        }
        ReaderCommand::RawSyntaxSites(command) => {
            execute_raw_syntax_sites_command(connection, *command);
        }
        ReaderCommand::RawSyntaxSiteSearch(command) => {
            execute_raw_syntax_site_search_command(connection, *command);
        }
        ReaderCommand::RawSyntaxTestMarkers(command) => {
            execute_raw_syntax_test_markers_command(connection, *command);
        }
        ReaderCommand::RecallMemory(command) => execute_memory_recall_command(connection, *command),
        ReaderCommand::HistoryEvidence(command) => {
            execute_history_evidence_command(connection, *command)
        }
        ReaderCommand::KnownAtHistoryEvidence(command) => {
            execute_known_at_history_evidence_command(connection, *command)
        }
        ReaderCommand::KnownAtHistoryReceipt(command) => {
            execute_known_at_history_receipt_command(connection, *command)
        }
        ReaderCommand::PersonalMemoryRead(command) => {
            execute_personal_memory_read_command(connection, *command)
        }
        ReaderCommand::TaskStatus(command) => execute_task_status_command(connection, *command),
        ReaderCommand::TaskStatuses(command) => {
            execute_task_statuses_command(connection, *command)
        }
        ReaderCommand::Diagnostics(command) => execute_diagnostics_command(connection, *command),
        ReaderCommand::WorkspaceView(command) => {
            let result = execute_workspace_view_command(connection, &command);
            let _ = command.reply.try_send(result);
        }
        ReaderCommand::ScipOverlayStatus(command) => {
            let result = execute_scip_overlay_status_command(connection, &command);
            let _ = command.reply.try_send(result);
        }
        ReaderCommand::ScipSymbolEvidence(command) => {
            let result = execute_scip_symbol_evidence_command(connection, &command);
            let _ = command.reply.try_send(result);
        }
        ReaderCommand::ScipRelationshipTrace(command) => {
            let result = execute_scip_relationship_trace_command(connection, &command);
            let _ = command.reply.try_send(result);
        }
        ReaderCommand::ScipSyntaxSymbol(command) => {
            let result = execute_scip_syntax_symbol_command(connection, &command);
            let _ = command.reply.try_send(result);
        }
        ReaderCommand::ScipImportScope(command) => {
            let result = execute_scip_import_scope_command(connection, &command);
            let _ = command.reply.try_send(result);
        }
        ReaderCommand::Graph(command) => {
            let result = execute_graph_command(connection, &command);
            let _ = command.reply.try_send(result);
        }
        ReaderCommand::Shutdown { reply } => {
            let _ = reply.try_send(Ok(()));
            return true;
        }
    }
    false
}

fn execute_search_command(connection: &mut Connection, command: SearchCommand) {
    let SearchCommand {
        repository,
        query,
        limits,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = search_active(connection, repository, &query, limits, cancelled, deadline);
    let _ = reply.try_send(result);
}

fn execute_symbol_search_command(connection: &mut Connection, command: SymbolSearchCommand) {
    let SymbolSearchCommand {
        repository,
        query,
        limits,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = search_active_symbols(connection, repository, &query, limits, cancelled, deadline);
    let _ = reply.try_send(result);
}

fn execute_workspace_search_command(connection: &mut Connection, command: WorkspaceSearchCommand) {
    let WorkspaceSearchCommand {
        view,
        source_slot,
        query,
        limits,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = search_workspace_member(
        connection,
        &view,
        source_slot,
        &query,
        limits,
        cancelled,
        deadline,
    );
    let _ = reply.try_send(result);
}

fn execute_workspace_symbol_search_command(
    connection: &mut Connection,
    command: WorkspaceSymbolSearchCommand,
) {
    let WorkspaceSymbolSearchCommand {
        view,
        source_slot,
        query,
        limits,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = search_workspace_member_symbols(
        connection,
        &view,
        source_slot,
        &query,
        limits,
        cancelled,
        deadline,
    );
    let _ = reply.try_send(result);
}

fn execute_symbol_command(connection: &mut Connection, command: SymbolCommand) {
    let SymbolCommand {
        repository,
        expected_snapshot,
        expected_generation,
        selector,
        cancelled,
        deadline,
        reply,
    } = command;
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

fn execute_artifact_command(connection: &mut Connection, command: ArtifactCommand) {
    let ArtifactCommand {
        requested,
        language,
        identity,
        limits,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = load_reusable_artifacts(
        connection,
        &requested,
        language,
        identity,
        limits,
        cancelled,
        deadline,
    );
    let _ = reply.try_send(result);
}

fn execute_graph_artifact_command(connection: &mut Connection, command: GraphArtifactCommand) {
    let GraphArtifactCommand {
        requested,
        identity,
        limits,
        graph_limits,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = load_reusable_graph_artifacts(
        connection,
        &requested,
        identity,
        limits,
        graph_limits,
        cancelled,
        deadline,
    );
    let _ = reply.try_send(result);
}

fn execute_raw_syntax_artifact_command(
    connection: &mut Connection,
    command: RawSyntaxArtifactCommand,
) {
    let RawSyntaxArtifactCommand {
        requested,
        identities,
        limits,
        raw_syntax_limits,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = load_reusable_raw_syntax_artifacts(
        connection,
        &requested,
        identities,
        limits,
        raw_syntax_limits,
        cancelled,
        deadline,
    );
    let _ = reply.try_send(result);
}

fn execute_memory_recall_command(connection: &mut Connection, command: MemoryRecallCommand) {
    let MemoryRecallCommand {
        repository,
        query,
        limits,
        cancelled,
        deadline,
        reply,
    } = command;
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

fn execute_history_evidence_command(
    connection: &mut Connection,
    command: HistoryEvidenceCommand,
) {
    let HistoryEvidenceCommand {
        repository,
        expected_snapshot,
        expected_generation,
        expected_source_epoch,
        max_results,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = read_trusted_git_history_evidence(
        connection,
        repository,
        expected_snapshot,
        expected_generation,
        expected_source_epoch,
        max_results,
        cancelled,
        deadline,
    );
    let _ = reply.try_send(result);
}

fn execute_diagnostics_command(connection: &mut Connection, command: DiagnosticsCommand) {
    let DiagnosticsCommand {
        repository,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = diagnose_active_repository(connection, repository, cancelled, deadline);
    let _ = reply.try_send(result);
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

fn search_active_symbols(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    query: &SymbolSearchQuery,
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
    let result = symbol_search_transaction(connection, repository, query, limits);
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

fn search_workspace_member(
    connection: &mut Connection,
    view: &PinnedWorkspaceView,
    source_slot: repowitness_domain::SourceSlotId,
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
    let result = workspace_search_transaction(connection, view, source_slot, query, limits);
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

fn search_workspace_member_symbols(
    connection: &mut Connection,
    view: &PinnedWorkspaceView,
    source_slot: repowitness_domain::SourceSlotId,
    query: &SymbolSearchQuery,
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
    let result = workspace_symbol_search_transaction(connection, view, source_slot, query, limits);
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
