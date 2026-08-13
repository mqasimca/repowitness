struct LocalMcpRepositoryService {
    root: PathBuf,
    database: PathBuf,
    repository_identity: String,
    graph_workspace: GraphWorkspaceContext,
    workspace: Option<WorkspaceServiceSelection>,
    memory_actor: Option<String>,
    configuration: ResolvedConfiguration,
}

impl LocalMcpRepositoryService {
    fn code_graph_query(
        &self,
        operation: CodeGraphQueryOperation<repowitness_local::GenerationId>,
        timeout: std::time::Duration,
        cancelled: Arc<AtomicBool>,
    ) -> Result<CodeGraphQueryOutput, RepositoryServiceError> {
        read_local_code_graph_query(
            LocalCodeGraphQueryRequest::new(&self.database, &self.repository_identity, operation)
                .with_configuration(&self.configuration)
                .with_deadline(timeout),
            cancelled,
        )
        .map_err(|_| RepositoryServiceError::CodeGraphQuery)
        .and_then(|result| {
            mcp_code_graph_query_output(result).map_err(|_| RepositoryServiceError::CodeGraphQuery)
        })
    }

    fn code_search_request<'a>(&'a self, query: &'a str) -> LocalCodeSearchRequest<'a> {
        match &self.workspace {
            Some(workspace) => LocalCodeSearchRequest::for_connected_workspace(
                &self.database,
                &workspace.connected_workspace_id,
                &workspace.source_slot_id,
                query,
            ),
            None => LocalCodeSearchRequest::new(&self.database, &self.repository_identity, query),
        }
    }
}

impl RepositoryService for LocalMcpRepositoryService {
    fn change_review(
        &self,
        request: ChangeReviewServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<ChangeReviewOutput, RepositoryServiceError> {
        if self.workspace.is_some() {
            return Err(RepositoryServiceError::ChangeReview);
        }
        let base = GitObjectId::try_from_hex(request.base())
            .map_err(|_| RepositoryServiceError::ChangeReview)?;
        build_local_change_review(
            LocalChangeReviewRequest::new(
                &self.root,
                &self.database,
                &self.repository_identity,
                request.intent(),
                base,
            )
            .with_configuration(&self.configuration)
            .with_deadline(request.timeout()),
            cancelled,
        )
        .map_err(|_| RepositoryServiceError::ChangeReview)
        .and_then(|receipt| {
            mcp_change_review_output(receipt).map_err(|_| RepositoryServiceError::ChangeReview)
        })
    }

    fn code_search(
        &self,
        request: CodeSearchServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<CodeSearchOutput, RepositoryServiceError> {
        let local_request = self.code_search_request(request.query())
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

    fn relevant_paths(
        &self,
        request: RelevantPathsServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<RelevantPathsOutput, RepositoryServiceError> {
        let local_request = match &self.workspace {
            Some(workspace) => LocalRelevantPathsRequest::for_connected_workspace(
                &self.database,
                &workspace.connected_workspace_id,
                &workspace.source_slot_id,
                request.query(),
            ),
            None => LocalRelevantPathsRequest::new(&self.database, &self.repository_identity, request.query()),
        }
        .with_max_paths(request.max_paths())
        .map_err(|_| RepositoryServiceError::RelevantPaths)?
        .with_configuration(&self.configuration)
        .with_deadline(request.timeout());
        locate_local_relevant_paths(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::RelevantPaths)
            .and_then(|result| {
                mcp_relevant_paths_output(result).map_err(|_| RepositoryServiceError::RelevantPaths)
            })
    }

    fn symbol_search(
        &self,
        request: SymbolSearchServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<SymbolSearchOutput, RepositoryServiceError> {
        let local_request = match &self.workspace {
            Some(workspace) => LocalSymbolSearchRequest::for_connected_workspace(
                &self.database,
                &workspace.connected_workspace_id,
                &workspace.source_slot_id,
                request.name(),
                request.match_mode(),
            ),
            None => LocalSymbolSearchRequest::new(&self.database, &self.repository_identity, request.name(), request.match_mode()),
        }
        .with_filters(request.language(), request.kind(), request.path_prefix())
        .with_max_results(request.max_results())
        .map_err(|_| RepositoryServiceError::SymbolSearch)?
        .with_configuration(&self.configuration)
        .with_deadline(request.timeout());
        search_local_symbols(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::SymbolSearch)
            .and_then(|result| {
                mcp_symbol_search_output(result).map_err(|_| RepositoryServiceError::SymbolSearch)
            })
    }

    fn outbound_sites(
        &self,
        request: OutboundSitesServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<OutboundSitesOutput, RepositoryServiceError> {
        if self.workspace.is_some() {
            return Err(RepositoryServiceError::OutboundSites);
        }
        let selector = LocalSymbolSelectorText::new(
            request.snapshot_sha256(),
            request.generation(),
            request.path(),
            request.content_sha256(),
            request.artifact_sha256(),
            request.fact_ordinal(),
        );
        let local_request = LocalOutboundSitesRequest::new(
            &self.database,
            &self.repository_identity,
            selector,
        )
        .with_max_results(request.max_sites())
        .map_err(|_| RepositoryServiceError::OutboundSites)?
        .with_deadline(request.timeout());
        get_local_outbound_sites(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::OutboundSites)
            .and_then(|result| {
                mcp_outbound_sites_output(result).map_err(|_| RepositoryServiceError::OutboundSites)
            })
    }

    fn syntax_site_search(
        &self,
        request: SyntaxSiteSearchServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<SyntaxSiteSearchOutput, RepositoryServiceError> {
        if self.workspace.is_some() {
            return Err(RepositoryServiceError::SyntaxSiteSearch);
        }
        let local_request = LocalSyntaxSiteSearchRequest::new(
            &self.database,
            &self.repository_identity,
            request.target(),
        )
        .with_max_results(request.max_sites())
        .map_err(|_| RepositoryServiceError::SyntaxSiteSearch)?
        .with_configuration(&self.configuration)
        .with_deadline(request.timeout());
        search_local_syntax_sites(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::SyntaxSiteSearch)
            .and_then(|result| {
                mcp_syntax_site_search_output(result)
                    .map_err(|_| RepositoryServiceError::SyntaxSiteSearch)
            })
    }

    fn code_graph_query(
        &self,
        request: CodeGraphQueryServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<CodeGraphQueryOutput, RepositoryServiceError> {
        if self.workspace.is_some()
            && !matches!(
                &request,
                CodeGraphQueryServiceRequest::Symbols(_) | CodeGraphQueryServiceRequest::RelevantPaths(_)
            )
        {
            return Err(RepositoryServiceError::CodeGraphQuery);
        }
        match request {
            CodeGraphQueryServiceRequest::Symbols(request) => self
                .symbol_search(request, cancelled)
                .map(CodeGraphQueryResultOutput::Symbols)
                .map(CodeGraphQueryOutput::new)
                .map_err(|_| RepositoryServiceError::CodeGraphQuery),
            CodeGraphQueryServiceRequest::OutboundSites(request) => self
                .outbound_sites(request, cancelled)
                .map(CodeGraphQueryResultOutput::OutboundSites)
                .map(CodeGraphQueryOutput::new)
                .map_err(|_| RepositoryServiceError::CodeGraphQuery),
            CodeGraphQueryServiceRequest::SyntaxSiteSearch(request) => {
                let query = SyntaxSiteSearchQuery::try_new(request.target())
                    .map_err(|_| RepositoryServiceError::CodeGraphQuery)?;
                let defaults = SyntaxSiteSearchLimits::default();
                let limits = SyntaxSiteSearchLimits::try_new(
                    request.max_sites(),
                    defaults.max_output_bytes(),
                )
                .map_err(|_| RepositoryServiceError::CodeGraphQuery)?;
                LocalMcpRepositoryService::code_graph_query(
                    self,
                    CodeGraphQueryOperation::SyntaxSiteSearch { query, limits },
                    request.timeout(),
                    cancelled,
                )
            }
            CodeGraphQueryServiceRequest::Architecture(request) => {
                let defaults = ArchitectureOverviewLimits::default();
                let limits = ArchitectureOverviewLimits::try_new(
                    request.max_roots(),
                    request.max_entry_point_candidates(),
                    request.max_files(),
                    defaults.max_output_bytes(),
                )
                .map_err(|_| RepositoryServiceError::CodeGraphQuery)?;
                LocalMcpRepositoryService::code_graph_query(
                    self,
                    CodeGraphQueryOperation::Architecture { limits },
                    request.timeout(),
                    cancelled,
                )
            }
            CodeGraphQueryServiceRequest::Files(request) => {
                let defaults = ArchitectureMapLimits::default();
                let limits = ArchitectureMapLimits::try_new(
                    request.max_files(),
                    defaults.max_output_bytes(),
                )
                .map_err(|_| RepositoryServiceError::CodeGraphQuery)?;
                LocalMcpRepositoryService::code_graph_query(
                    self,
                    CodeGraphQueryOperation::Files { limits },
                    request.timeout(),
                    cancelled,
                )
            }
            CodeGraphQueryServiceRequest::TestMarkers(request) => {
                let query = TestMarkersQuery::try_new(request.language(), request.path_prefix())
                .map_err(|_| RepositoryServiceError::CodeGraphQuery)?
                ;
                let defaults = TestMarkersLimits::default();
                let limits = TestMarkersLimits::try_new(
                    request.max_results(),
                    defaults.max_output_bytes(),
                )
                .map_err(|_| RepositoryServiceError::CodeGraphQuery)?;
                LocalMcpRepositoryService::code_graph_query(
                    self,
                    CodeGraphQueryOperation::TestMarkers { query, limits },
                    request.timeout(),
                    cancelled,
                )
            }
            CodeGraphQueryServiceRequest::RelevantPaths(request) => self
                .relevant_paths(request, cancelled)
                .map(CodeGraphQueryResultOutput::RelevantPaths)
                .map(CodeGraphQueryOutput::new)
                .map_err(|_| RepositoryServiceError::CodeGraphQuery),
        }
    }

    fn architecture_map(
        &self,
        request: ArchitectureMapServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<ArchitectureMapOutput, RepositoryServiceError> {
        let local_request = match &self.workspace {
            Some(workspace) => LocalArchitectureMapRequest::for_connected_workspace(
                &self.database,
                &workspace.connected_workspace_id,
                &workspace.source_slot_id,
            ),
            None => LocalArchitectureMapRequest::new(&self.database, &self.repository_identity),
        }
            .with_max_files(request.max_files())
            .map_err(|_| RepositoryServiceError::ArchitectureMap)?
            .with_deadline(request.timeout());
        map_local_architecture(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::ArchitectureMap)
            .and_then(|result| {
                mcp_architecture_map_output(result)
                    .map_err(|_| RepositoryServiceError::ArchitectureMap)
            })
    }

    fn architecture_overview(
        &self,
        request: ArchitectureOverviewServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<ArchitectureOverviewOutput, RepositoryServiceError> {
        if self.workspace.is_some() {
            return Err(RepositoryServiceError::ArchitectureOverview);
        }
        let local_request = LocalArchitectureOverviewRequest::new(
            &self.database,
            &self.repository_identity,
        )
        .with_limits(
            request.max_roots(),
            request.max_entry_point_candidates(),
            request.max_files(),
        )
        .map_err(|_| RepositoryServiceError::ArchitectureOverview)?
        .with_deadline(request.timeout());
        overview_local_architecture(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::ArchitectureOverview)
            .and_then(|result| {
                mcp_architecture_overview_output(result)
                    .map_err(|_| RepositoryServiceError::ArchitectureOverview)
            })
    }

    fn repository_topology(
        &self,
        request: RepositoryTopologyServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<RepositoryTopologyOutput, RepositoryServiceError> {
        if self.workspace.is_some() {
            return Err(RepositoryServiceError::RepositoryTopology);
        }
        let local_request = LocalRepositoryTopologyRequest::new(&self.database, &self.repository_identity)
            .with_max_paths(request.max_paths())
            .map_err(|_| RepositoryServiceError::RepositoryTopology)?
            .with_deadline(request.timeout());
        read_local_repository_topology(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::RepositoryTopology)
            .and_then(|result| {
                mcp_repository_topology_output(result)
                    .map_err(|_| RepositoryServiceError::RepositoryTopology)
            })
    }

    fn context_build(
        &self,
        request: EvidenceContextBuildServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<EvidenceContextBuildOutput, RepositoryServiceError> {
        let local_request = match &self.workspace {
            Some(workspace) => LocalEvidenceContextBuildRequest::for_connected_workspace(
                &self.root,
                &self.database,
                &self.repository_identity,
                &workspace.connected_workspace_id,
                &workspace.source_slot_id,
                request.intent(),
            ),
            None => LocalEvidenceContextBuildRequest::new(
                &self.root,
                &self.database,
                &self.repository_identity,
                request.intent(),
            ),
        };
        let local_request = local_request
        .with_budget_units(request.budget_units())
        .map_err(|_| RepositoryServiceError::ContextBuild)?
        .with_max_provider_results(request.max_provider_results())
        .map_err(|_| RepositoryServiceError::ContextBuild)?
        .with_configuration(&self.configuration)
        .with_deadline(request.timeout());
        build_local_evidence_context(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::ContextBuild)
            .and_then(|result| {
                mcp_evidence_context_output(result)
                    .map_err(|_| RepositoryServiceError::ContextBuild)
            })
    }

    fn diagnostics(
        &self,
        request: DiagnosticsServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<DiagnosticsOutput, RepositoryServiceError> {
        if self.workspace.is_some() {
            return Err(RepositoryServiceError::Diagnostics);
        }
        let local_request =
            LocalRepositoryDiagnosticsRequest::new(&self.database, &self.repository_identity)
                .with_deadline(request.timeout());
        diagnose_local_repository(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::Diagnostics)
            .map(|result| mcp_diagnostics_output(result, &self.configuration))
    }

    fn scip_evidence(
        &self,
        request: ScipEvidenceServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<ScipEvidenceOutput, RepositoryServiceError> {
        read_local_scip_evidence_service(
            &self.database,
            &self.graph_workspace,
            request,
            cancelled,
        )
        .map_err(|_| RepositoryServiceError::ScipEvidence)
    }

    fn scip_relationship_trace(
        &self,
        request: ScipRelationshipTraceServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<ScipRelationshipTraceOutput, RepositoryServiceError> {
        read_local_scip_relationship_trace_service(
            &self.database,
            &self.graph_workspace,
            request,
            cancelled,
        )
        .map_err(|_| RepositoryServiceError::ScipRelationshipTrace)
    }

    fn scip_symbol_resolve(
        &self,
        request: ScipSymbolResolveServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<ScipSymbolResolveOutput, RepositoryServiceError> {
        read_local_scip_symbol_resolve_service(
            &self.database,
            &self.graph_workspace,
            request,
            cancelled,
        )
        .map_err(|_| RepositoryServiceError::ScipSymbolResolve)
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
        if self.workspace.is_some() {
            return Err(RepositoryServiceError::MemoryRecall);
        }
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
        if self.workspace.is_some() {
            return Err(RepositoryServiceError::MemoryManage);
        }
        manage_mcp_memory(self, request, cancelled)
    }

    fn symbol_get(
        &self,
        request: SymbolGetServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<SymbolGetOutput, RepositoryServiceError> {
        if self.workspace.is_some() {
            return Err(RepositoryServiceError::SymbolGet);
        }
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

fn mcp_change_review_output(
    receipt: LocalChangeReviewReceipt,
) -> Result<ChangeReviewOutput, String> {
    let mut changes = Vec::with_capacity(receipt.manifest().entries().len());
    for entry in receipt.manifest().entries() {
        changes.push(McpChangeReviewPath {
            kind: entry.kind().as_str().to_owned(),
            path: RepositoryPathTextV1::encode(entry.path(), PATH_TEXT_LIMIT)
                .map_err(|error| error.to_string())?
                .into_string(),
        });
    }
    let (
        indexed_context_availability,
        indexed_context_reason,
        indexed_snapshot_sha256,
        indexed_generation,
        indexed_context_items,
        indexed_context_omissions,
    ) = match receipt.indexed_context() {
        IndexedContext::Available(context) => (
            "available".to_owned(),
            None,
            Some(hex(context.snapshot().as_bytes())),
            Some(
                u64::try_from(context.generation().get())
                    .map_err(|_| "indexed generation was negative".to_owned())?,
            ),
            Some(
                u64::try_from(context.items().len())
                    .map_err(|_| "context item count overflowed".to_owned())?,
            ),
            Some(
                u64::try_from(context.omissions().len())
                    .map_err(|_| "context omission count overflowed".to_owned())?,
            ),
        ),
        IndexedContext::Unavailable { reason } => (
            "unavailable".to_owned(),
            Some(reason.as_str().to_owned()),
            None,
            None,
            None,
            None,
        ),
    };
    Ok(ChangeReviewOutput {
        schema_version: CHANGE_REVIEW_SCHEMA_VERSION,
        change_manifest_profile: CHANGE_MANIFEST_PROFILE_VERSION,
        base: receipt.manifest().base().to_hex(),
        worktree_git_state_sha256: hex(receipt.worktree_git_state().as_bytes()),
        changes,
        indexed_context_availability,
        indexed_context_reason,
        indexed_snapshot_sha256,
        indexed_generation,
        indexed_context_items,
        indexed_context_omissions,
        index_worktree_alignment: "unverified".to_owned(),
        verdict: "not_provided".to_owned(),
    })
}
