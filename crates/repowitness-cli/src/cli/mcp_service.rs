struct LocalMcpRepositoryService {
    root: PathBuf,
    database: PathBuf,
    repository_identity: String,
    graph_workspace: GraphWorkspaceContext,
    memory_actor: Option<String>,
    personal_memory_profile: Option<PersonalMemoryProfileId>,
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
}

impl RepositoryService for LocalMcpRepositoryService {
    fn native_task_start(
        &self,
        objective: &str,
        cancelled: Arc<AtomicBool>,
    ) -> Result<NativeTaskStatus, RepositoryServiceError> {
        let recorded_at = native_task_recorded_at().ok_or(RepositoryServiceError::NativeTask)?;
        let receipt = append_local_task_checkpoint(
            LocalTaskCheckpointRequest::create(
                &self.database,
                &self.repository_identity,
                TaskState::Open,
                objective,
                Some("native MCP task admitted"),
                Some("await bounded context result"),
                recorded_at,
            ),
            cancelled,
        )
        .map_err(|_| RepositoryServiceError::NativeTask)?;
        Ok(NativeTaskStatus::new(
            native_task_id_text(receipt.task_id()),
            NativeTaskState::Working,
            receipt.sequence(),
            0,
        ))
    }

    fn native_task_transition(
        &self,
        task_id: &str,
        state: NativeTaskState,
        cancelled: Arc<AtomicBool>,
    ) -> Result<NativeTaskStatus, RepositoryServiceError> {
        let task_id = parse_native_task_id(task_id).ok_or(RepositoryServiceError::NativeTask)?;
        let previous = poll_local_task(
            LocalTaskPollRequest::new(&self.database, &self.repository_identity, task_id),
            Arc::clone(&cancelled),
        )
        .map_err(|_| RepositoryServiceError::NativeTask)?
        .ok_or(RepositoryServiceError::NativeTask)?;
        let (task_state, hypothesis, action) = match state {
            NativeTaskState::Working => (
                TaskState::Open,
                Some("native MCP task continues"),
                Some("await bounded context result"),
            ),
            NativeTaskState::Completed => (
                TaskState::Completed,
                Some("bounded context result completed"),
                Some("review the retained MCP result"),
            ),
            NativeTaskState::Failed => (
                TaskState::Blocked,
                Some("native MCP context operation did not complete"),
                Some("inspect bounded diagnostics and retry only after reconciliation"),
            ),
            NativeTaskState::Cancelled => (
                TaskState::Cancelled,
                Some("native MCP task cancellation was requested"),
                Some("resume through a new explicit task if still needed"),
            ),
        };
        let recorded_at = native_task_recorded_at().ok_or(RepositoryServiceError::NativeTask)?;
        let receipt = append_local_task_checkpoint(
            LocalTaskCheckpointRequest::update(
                &self.database,
                &self.repository_identity,
                task_id,
                task_state,
                "MCP Phase 2 context build",
                hypothesis,
                action,
                recorded_at,
            ),
            cancelled,
        )
        .map_err(|_| RepositoryServiceError::NativeTask)?;
        Ok(NativeTaskStatus::new(
            native_task_id_text(receipt.task_id()),
            state,
            receipt.sequence(),
            previous.verification_count(),
        ))
    }

    fn native_task_status(
        &self,
        task_id: &str,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Option<NativeTaskStatus>, RepositoryServiceError> {
        let task_id = parse_native_task_id(task_id).ok_or(RepositoryServiceError::NativeTask)?;
        poll_local_task(
            LocalTaskPollRequest::new(&self.database, &self.repository_identity, task_id),
            cancelled,
        )
        .map_err(|_| RepositoryServiceError::NativeTask)
        .map(|status| status.map(native_task_status))
    }

    fn native_task_list(
        &self,
        limit: u16,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Box<[NativeTaskStatus]>, RepositoryServiceError> {
        list_local_tasks(
            LocalTaskListRequest::new(&self.database, &self.repository_identity, limit),
            cancelled,
        )
        .map_err(|_| RepositoryServiceError::NativeTask)
        .map(|statuses| {
            statuses
                .into_iter()
                .map(native_task_status)
                .collect::<Vec<_>>()
                .into_boxed_slice()
        })
    }

    fn code_search(
        &self,
        request: CodeSearchServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<CodeSearchOutput, RepositoryServiceError> {
        let local_request = match &self.graph_workspace {
            GraphWorkspaceContext::SingleRepository(repository_identity) => {
                LocalCodeSearchRequest::new(&self.database, repository_identity, request.query())
            }
            GraphWorkspaceContext::ConnectedWorkspace {
                connected_workspace,
                source_slot,
            } => LocalCodeSearchRequest::for_connected_workspace(
                &self.database,
                connected_workspace,
                source_slot,
                request.query(),
            ),
        }
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
        let local_request = match &self.graph_workspace {
            GraphWorkspaceContext::SingleRepository(repository_identity) => {
                LocalRelevantPathsRequest::new(&self.database, repository_identity, request.query())
            }
            GraphWorkspaceContext::ConnectedWorkspace {
                connected_workspace,
                source_slot,
            } => LocalRelevantPathsRequest::for_connected_workspace(
                &self.database,
                connected_workspace,
                source_slot,
                request.query(),
            ),
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
        let local_request = match &self.graph_workspace {
            GraphWorkspaceContext::SingleRepository(repository_identity) => {
                LocalSymbolSearchRequest::new(
                    &self.database,
                    repository_identity,
                    request.name(),
                    request.match_mode(),
                )
            }
            GraphWorkspaceContext::ConnectedWorkspace {
                connected_workspace,
                source_slot,
            } => LocalSymbolSearchRequest::for_connected_workspace(
                &self.database,
                connected_workspace,
                source_slot,
                request.name(),
                request.match_mode(),
            ),
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
        let local_request = match &self.graph_workspace {
            GraphWorkspaceContext::SingleRepository(repository_identity) => {
                LocalArchitectureMapRequest::new(&self.database, repository_identity)
            }
            GraphWorkspaceContext::ConnectedWorkspace {
                connected_workspace,
                source_slot,
            } => LocalArchitectureMapRequest::for_connected_workspace(
                &self.database,
                connected_workspace,
                source_slot,
            ),
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

    fn phase2_context_build(
        &self,
        request: Phase2ContextBuildServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Phase2ContextBuildOutput, RepositoryServiceError> {
        let local_request = match &self.graph_workspace {
            GraphWorkspaceContext::SingleRepository(_) => LocalPhase2ContextBuildRequest::new(
                &self.root,
                &self.database,
                &self.repository_identity,
                request.intent(),
            ),
            GraphWorkspaceContext::ConnectedWorkspace {
                connected_workspace,
                source_slot,
            } => LocalPhase2ContextBuildRequest::for_connected_workspace(
                &self.root,
                &self.database,
                &self.repository_identity,
                connected_workspace,
                source_slot,
                request.intent(),
            ),
        };
        let local_request = match request.scip_symbol() {
            Some(scip_symbol) => local_request.with_scip_symbol(scip_symbol),
            None => local_request,
        }
        .with_budget_units(request.budget_units())
        .map_err(|_| RepositoryServiceError::Phase2ContextBuild)?
        .with_max_provider_results(request.max_provider_results())
        .map_err(|_| RepositoryServiceError::Phase2ContextBuild)?
        .with_deadline(request.timeout());
        build_local_phase2_context(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::Phase2ContextBuild)
            .and_then(|result| {
                mcp_phase2_context_output(result)
                    .map_err(|_| RepositoryServiceError::Phase2ContextBuild)
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

    fn scip_evidence(
        &self,
        request: ScipEvidenceServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<ScipEvidenceOutput, RepositoryServiceError> {
        read_local_scip_evidence_service(&self.database, &self.graph_workspace, request, cancelled)
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

    fn historical_memory(
        &self,
        request: HistoricalMemoryServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<HistoricalMemoryOutput, RepositoryServiceError> {
        let target = match request.target() {
            HistoricalMemoryTarget::GitCommit(commit) => parse_history_commit(commit)
                .map(MemoryObservationSource::Git)
                .map_err(|_| RepositoryServiceError::HistoricalMemory)?,
            HistoricalMemoryTarget::WorktreeSnapshot(snapshot) => {
                let bytes = decode_history_hex::<32>(snapshot)
                    .map_err(|_| RepositoryServiceError::HistoricalMemory)?;
                let snapshot = SourceSnapshotDigest::try_from_slice(&bytes)
                    .map_err(|_| RepositoryServiceError::HistoricalMemory)?;
                MemoryObservationSource::Worktree(snapshot)
            }
        };
        read_local_known_at_history(
            LocalKnownAtHistoryRequest::new(
                &self.root,
                &self.database,
                &self.repository_identity,
                request.known_at_unix_ms(),
                target,
            )
            .with_max_results(request.max_results())
            .with_deadline(request.timeout()),
            cancelled,
        )
        .map_err(|_| RepositoryServiceError::HistoricalMemory)
        .map(historical_memory_output)
    }

    fn memory_manage(
        &self,
        request: MemoryManageServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<MemoryManageOutput, RepositoryServiceError> {
        manage_mcp_memory(self, request, cancelled)
    }

    fn personal_memory(
        &self,
        request: PersonalMemoryServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<PersonalMemoryOutput, RepositoryServiceError> {
        let profile = self
            .personal_memory_profile
            .ok_or(RepositoryServiceError::PersonalMemory)?;
        match request {
            PersonalMemoryServiceRequest::Read {
                max_results,
                timeout,
            } => read_local_personal_memory(
                LocalPersonalMemoryReadRequest::new(
                    &self.database,
                    &self.repository_identity,
                    profile,
                    max_results,
                )
                .with_deadline(timeout),
                cancelled,
            )
            .map_err(|_| RepositoryServiceError::PersonalMemory)
            .map(|records| personal_memory_mcp_output(PersonalMemoryOperation::Read, records)),
            PersonalMemoryServiceRequest::Append {
                kind,
                title,
                body,
                lifecycle,
                timeout,
            } => {
                let recorded_at_unix_ms = personal_memory_current_unix_ms()
                    .ok_or(RepositoryServiceError::PersonalMemory)?;
                append_local_personal_memory(
                    LocalPersonalMemoryAppendRequest::new(
                        &self.database,
                        &self.repository_identity,
                        profile,
                        local_personal_memory_kind(kind),
                        &title,
                        &body,
                        local_personal_memory_lifecycle(lifecycle),
                        recorded_at_unix_ms,
                    )
                    .with_deadline(timeout),
                    cancelled,
                )
                .map_err(|_| RepositoryServiceError::PersonalMemory)
                .map(|record| {
                    personal_memory_mcp_output(PersonalMemoryOperation::Append, vec![record])
                })
            }
        }
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

fn historical_memory_output(
    receipt: repowitness_local::KnownAtHistoryReceipt,
) -> HistoricalMemoryOutput {
    let coverage = match receipt.coverage() {
        KnownAtHistoryCoverage::Complete => HistoricalMemoryCoverage::Complete,
        KnownAtHistoryCoverage::Truncated => HistoricalMemoryCoverage::Truncated,
    };
    let applicability = match receipt.applicability() {
        KnownAtApplicability::Unavailable => HistoricalMemoryApplicability::Unavailable,
        KnownAtApplicability::NotApplicable => HistoricalMemoryApplicability::NotApplicable,
        KnownAtApplicability::Applicable => HistoricalMemoryApplicability::Applicable,
    };
    let evidence = receipt
        .evidence()
        .iter()
        .map(|evidence| HistoricalMemoryEvidence {
            record_id: MemoryRecordIdTextV1::encode(evidence.record_id()).into_string(),
            revision_sha256: hex(evidence.revision().as_bytes()),
            basis: match evidence.basis() {
                KnownAtEvidenceBasis::Observation => HistoricalMemoryEvidenceBasis::Observation,
                KnownAtEvidenceBasis::ReviewedCorrespondence => {
                    HistoricalMemoryEvidenceBasis::ReviewedCorrespondence
                }
            },
        })
        .collect();
    HistoricalMemoryOutput::new(coverage, applicability, evidence)
}

fn native_task_status(status: TaskStatus) -> NativeTaskStatus {
    let state = match status.state() {
        TaskState::Open => NativeTaskState::Working,
        TaskState::Blocked => NativeTaskState::Failed,
        TaskState::Completed => NativeTaskState::Completed,
        TaskState::Cancelled => NativeTaskState::Cancelled,
    };
    NativeTaskStatus::new(
        native_task_id_text(status.task_id()),
        state,
        status.checkpoint_sequence(),
        status.verification_count(),
    )
}

fn native_task_id_text(task_id: TaskId) -> String {
    let mut output = String::with_capacity(32);
    for byte in task_id.as_bytes() {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn parse_native_task_id(text: &str) -> Option<TaskId> {
    if text.len() != 32
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let mut bytes = [0_u8; 16];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        bytes[index] = (native_task_hex_nibble(pair[0])? << 4) | native_task_hex_nibble(pair[1])?;
    }
    Some(TaskId::new(bytes))
}

const fn native_task_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn native_task_recorded_at() -> Option<u64> {
    SystemTime::now().duration_since(UNIX_EPOCH).ok().and_then(|duration| {
        u64::try_from(duration.as_millis()).ok()
    })
}
