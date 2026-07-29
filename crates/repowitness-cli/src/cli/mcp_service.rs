struct LocalMcpRepositoryService {
    root: PathBuf,
    database: PathBuf,
    repository_identity: String,
    graph_workspace: GraphWorkspaceContext,
    memory_actor: Option<String>,
    configuration: ResolvedConfiguration,
}

impl RepositoryService for LocalMcpRepositoryService {
    fn code_search(
        &self,
        request: CodeSearchServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<CodeSearchOutput, RepositoryServiceError> {
        let local_request =
            LocalCodeSearchRequest::new(&self.database, &self.repository_identity, request.query())
                .with_max_results(request.max_results())
                .map_err(|_| RepositoryServiceError::CodeSearch)?
                .with_configuration(&self.configuration)
                .with_deadline(request.timeout());
        search_local_index(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::CodeSearch)
            .and_then(|result| {
                mcp_search_output(result).map_err(|_| RepositoryServiceError::CodeSearch)
            })
    }

    fn context_build(
        &self,
        request: ContextBuildServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<ContextBuildOutput, RepositoryServiceError> {
        let local_request = LocalContextBuildRequest::new(
            &self.root,
            &self.database,
            &self.repository_identity,
            request.intent(),
        )
        .with_budget_units(request.budget_units())
        .map_err(|_| RepositoryServiceError::ContextBuild)?
        .with_max_provider_results(request.max_provider_results())
        .map_err(|_| RepositoryServiceError::ContextBuild)?
        .with_configuration(&self.configuration)
        .with_deadline(request.timeout());
        build_local_context(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::ContextBuild)
            .and_then(|result| {
                mcp_context_output(result).map_err(|_| RepositoryServiceError::ContextBuild)
            })
    }

    fn diagnostics(
        &self,
        request: DiagnosticsServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<DiagnosticsOutput, RepositoryServiceError> {
        let local_request =
            LocalRepositoryDiagnosticsRequest::new(&self.database, &self.repository_identity)
                .with_deadline(request.timeout());
        diagnose_local_repository(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::Diagnostics)
            .map(|result| mcp_diagnostics_output(result, &self.configuration))
    }

    fn graph_read(
        &self,
        request: GraphReadServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<GraphReadServiceOutput, RepositoryServiceError> {
        read_local_graph_service(
            &self.database,
            &self.graph_workspace,
            request,
            &self.configuration,
            cancelled,
        )
        .map_err(|_| RepositoryServiceError::GraphRead)
    }

    fn memory_recall(
        &self,
        request: MemoryRecallServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<MemoryRecallOutput, RepositoryServiceError> {
        let selection = match request.selection() {
            MemoryRecallServiceSelection::All => LocalMemoryRecallSelection::All,
            MemoryRecallServiceSelection::Query(query) => {
                LocalMemoryRecallSelection::Query(query.as_str())
            }
        };
        let local_request =
            LocalMemoryRecallRequest::new(&self.database, &self.repository_identity, selection)
                .with_max_results(request.max_results())
                .map_err(|_| RepositoryServiceError::MemoryRecall)?
                .with_configuration(&self.configuration)
                .with_deadline(request.timeout());
        recall_local_memory(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::MemoryRecall)
            .and_then(|result| {
                mcp_memory_output(result).map_err(|_| RepositoryServiceError::MemoryRecall)
            })
    }

    fn memory_manage(
        &self,
        request: MemoryManageServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<MemoryManageOutput, RepositoryServiceError> {
        manage_mcp_memory(self, request, cancelled)
    }

    fn symbol_get(
        &self,
        request: SymbolGetServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<SymbolGetOutput, RepositoryServiceError> {
        let selector = LocalSymbolSelectorText::new(
            request.snapshot_sha256(),
            request.generation(),
            request.path(),
            request.content_sha256(),
            request.artifact_sha256(),
            request.fact_ordinal(),
        );
        let local_request = LocalSymbolGetRequest::new(
            &self.root,
            &self.database,
            &self.repository_identity,
            selector,
        )
        .with_deadline(request.timeout());
        get_local_symbol(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::SymbolGet)
            .and_then(|result| {
                mcp_symbol_output(result).map_err(|_| RepositoryServiceError::SymbolGet)
            })
    }
}
