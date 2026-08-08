use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use rmcp::{
    ServiceExt,
    model::{
        CallToolRequest, CallToolRequestParams, ClientRequest, GetTaskParams, GetTaskPayloadParams,
        GetTaskPayloadRequest, GetTaskRequest, ServerResult, TaskMetadata,
    },
};

use super::*;
use crate::{
    MAX_MCP_INTEROPERABLE_INTEGER, MEMORY_MANAGE_SCHEMA_VERSION, McpContextCoverage, McpCoverage,
    McpMemoryCoverage, McpMemoryProducer, McpMemoryTarget, McpSearchMatch, McpSpan, McpSymbol,
    MemoryManageDatabaseIdentityStatus, MemoryManageMaintenanceStatus,
    MemoryManageMaintenanceStepStatus, MemoryManageOperation, MemoryRecallServiceSelection,
    PersonalMemoryOperation, RepositoryTopologyOutput, SymbolSelectorOutput,
};

mod fixtures;
use fixtures::*;

const fn confirmed_memory_maintenance() -> MemoryManageMaintenanceStatus {
    MemoryManageMaintenanceStatus::from_evidence(
        MemoryManageMaintenanceStepStatus::Complete,
        MemoryManageMaintenanceStepStatus::Complete,
        MemoryManageDatabaseIdentityStatus::ConfirmedAtFinalFence,
    )
}

const fn checkpoint_deferred_memory_maintenance() -> MemoryManageMaintenanceStatus {
    MemoryManageMaintenanceStatus::from_evidence(
        MemoryManageMaintenanceStepStatus::Deferred,
        MemoryManageMaintenanceStepStatus::Complete,
        MemoryManageDatabaseIdentityStatus::ConfirmedAtFinalFence,
    )
}

struct FakeService {
    architecture_map_calls: AtomicUsize,
    architecture_overview_calls: AtomicUsize,
    repository_topology_calls: AtomicUsize,
    code_graph_query_calls: AtomicUsize,
    search_calls: AtomicUsize,
    context_calls: AtomicUsize,
    phase2_context_calls: AtomicUsize,
    diagnostics_calls: AtomicUsize,
    graph_calls: AtomicUsize,
    scip_calls: AtomicUsize,
    scip_relationship_trace_calls: AtomicUsize,
    invalid_diagnostics: AtomicBool,
    manage_calls: AtomicUsize,
    memory_calls: AtomicUsize,
    symbol_calls: AtomicUsize,
    outbound_sites_calls: AtomicUsize,
    syntax_site_search_calls: AtomicUsize,
    search_request: Mutex<Option<(String, u16)>>,
    context_request: Mutex<Option<(String, u64, u16)>>,
    memory_request: Mutex<Option<(bool, u16)>>,
    manage_request: Mutex<Option<MemoryManageOperation>>,
    native_tasks: Mutex<BTreeMap<String, NativeTaskStatus>>,
    next_native_task: AtomicUsize,
}

struct ConcurrencyService {
    active: AtomicUsize,
    maximum: AtomicUsize,
}

impl RepositoryService for ConcurrencyService {
    fn code_search(
        &self,
        _request: CodeSearchServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<CodeSearchOutput, RepositoryServiceError> {
        let active = self.active.fetch_add(1, Ordering::AcqRel) + 1;
        self.maximum.fetch_max(active, Ordering::AcqRel);
        std::thread::sleep(Duration::from_millis(30));
        self.active.fetch_sub(1, Ordering::AcqRel);
        Ok(search_output())
    }

    fn symbol_get(
        &self,
        _request: SymbolGetServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<SymbolGetOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::SymbolGet)
    }

    fn context_build(
        &self,
        _request: ContextBuildServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<ContextBuildOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::ContextBuild)
    }

    fn diagnostics(
        &self,
        _request: DiagnosticsServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<DiagnosticsOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::Diagnostics)
    }

    fn memory_recall(
        &self,
        _request: MemoryRecallServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<MemoryRecallOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::MemoryRecall)
    }
}

impl FakeService {
    fn new() -> Self {
        Self {
            architecture_map_calls: AtomicUsize::new(0),
            architecture_overview_calls: AtomicUsize::new(0),
            repository_topology_calls: AtomicUsize::new(0),
            code_graph_query_calls: AtomicUsize::new(0),
            search_calls: AtomicUsize::new(0),
            context_calls: AtomicUsize::new(0),
            phase2_context_calls: AtomicUsize::new(0),
            diagnostics_calls: AtomicUsize::new(0),
            graph_calls: AtomicUsize::new(0),
            scip_calls: AtomicUsize::new(0),
            scip_relationship_trace_calls: AtomicUsize::new(0),
            invalid_diagnostics: AtomicBool::new(false),
            manage_calls: AtomicUsize::new(0),
            memory_calls: AtomicUsize::new(0),
            symbol_calls: AtomicUsize::new(0),
            outbound_sites_calls: AtomicUsize::new(0),
            syntax_site_search_calls: AtomicUsize::new(0),
            search_request: Mutex::new(None),
            context_request: Mutex::new(None),
            memory_request: Mutex::new(None),
            manage_request: Mutex::new(None),
            native_tasks: Mutex::new(BTreeMap::new()),
            next_native_task: AtomicUsize::new(1),
        }
    }
}

impl RepositoryService for FakeService {
    fn native_task_start(
        &self,
        _objective: &str,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<NativeTaskStatus, RepositoryServiceError> {
        let task_id = format!(
            "{:032x}",
            self.next_native_task.fetch_add(1, Ordering::Relaxed)
        );
        let status = NativeTaskStatus::new(task_id.clone(), NativeTaskState::Working, 1, 0);
        self.native_tasks
            .lock()
            .expect("lock")
            .insert(task_id, status.clone());
        Ok(status)
    }

    fn native_task_transition(
        &self,
        task_id: &str,
        state: NativeTaskState,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<NativeTaskStatus, RepositoryServiceError> {
        let mut tasks = self.native_tasks.lock().expect("lock");
        let previous = tasks
            .get(task_id)
            .ok_or(RepositoryServiceError::NativeTask)?;
        let status = NativeTaskStatus::new(
            task_id.to_owned(),
            state,
            previous.checkpoint_sequence() + 1,
            previous.verification_count(),
        );
        tasks.insert(task_id.to_owned(), status.clone());
        Ok(status)
    }

    fn native_task_status(
        &self,
        task_id: &str,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<Option<NativeTaskStatus>, RepositoryServiceError> {
        Ok(self
            .native_tasks
            .lock()
            .expect("lock")
            .get(task_id)
            .cloned())
    }

    fn native_task_list(
        &self,
        limit: u16,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<Box<[NativeTaskStatus]>, RepositoryServiceError> {
        Ok(self
            .native_tasks
            .lock()
            .expect("lock")
            .values()
            .take(usize::from(limit))
            .cloned()
            .collect::<Vec<_>>()
            .into_boxed_slice())
    }

    fn code_search(
        &self,
        request: CodeSearchServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<CodeSearchOutput, RepositoryServiceError> {
        self.search_calls.fetch_add(1, Ordering::Relaxed);
        self.search_request
            .lock()
            .expect("lock")
            .replace((request.query().to_owned(), request.max_results()));
        Ok(search_output())
    }

    fn relevant_paths(
        &self,
        request: RelevantPathsServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<RelevantPathsOutput, RepositoryServiceError> {
        assert_eq!(request.query(), "run");
        assert_eq!(request.max_paths(), 7);
        Ok(relevant_paths_output())
    }

    fn code_graph_query(
        &self,
        request: CodeGraphQueryServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<CodeGraphQueryOutput, RepositoryServiceError> {
        self.code_graph_query_calls.fetch_add(1, Ordering::Relaxed);
        match request {
            CodeGraphQueryServiceRequest::Files(request) => {
                assert_eq!(request.max_files(), 1);
                Ok(CodeGraphQueryOutput::new(
                    crate::CodeGraphQueryResultOutput::Files(architecture_map_output()),
                ))
            }
            _ => Err(RepositoryServiceError::CodeGraphQuery),
        }
    }

    fn symbol_search(
        &self,
        request: SymbolSearchServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<SymbolSearchOutput, RepositoryServiceError> {
        assert_eq!(request.name(), "run");
        assert_eq!(request.match_mode().as_str(), "prefix");
        assert_eq!(
            request.language().map(|language| language.as_str()),
            Some("rust")
        );
        assert_eq!(request.kind().map(|kind| kind.as_str()), Some("struct"));
        assert_eq!(request.path_prefix(), Some("src"));
        assert_eq!(request.max_results(), 7);
        Ok(symbol_search_output())
    }

    fn outbound_sites(
        &self,
        request: OutboundSitesServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<OutboundSitesOutput, RepositoryServiceError> {
        self.outbound_sites_calls.fetch_add(1, Ordering::Relaxed);
        assert_eq!(request.generation(), 9);
        assert_eq!(request.fact_ordinal(), 7);
        assert_eq!(request.max_sites(), 7);
        Ok(OutboundSitesOutput {
            schema_version: 1,
            outbound_sites_profile: 1,
            snapshot_sha256: "11".repeat(32),
            generation: 9,
            selector: crate::OutboundSitesSelectorOutput {
                path: "rwp1:h:7372632F6C69622E7273".to_owned(),
                content_sha256: "22".repeat(32),
                artifact_sha256: "33".repeat(32),
                fact_ordinal: 7,
            },
            availability: "complete".to_owned(),
            declaration: None,
            coverage: McpCoverage {
                searched: 1,
                skipped: 0,
                unresolved: 0,
                truncated: 0,
            },
            sites_returned: 0,
            sites_total: 0,
            truncated: false,
            output_bytes: 0,
            limitation: "raw_syntax_observations_only_no_target_resolution_or_inferred_edges"
                .to_owned(),
            sites: Vec::new(),
        })
    }

    fn syntax_site_search(
        &self,
        request: SyntaxSiteSearchServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<SyntaxSiteSearchOutput, RepositoryServiceError> {
        self.syntax_site_search_calls
            .fetch_add(1, Ordering::Relaxed);
        assert_eq!(request.target(), "run");
        assert_eq!(request.max_sites(), 7);
        Ok(syntax_site_search_output())
    }

    fn architecture_map(
        &self,
        request: ArchitectureMapServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<ArchitectureMapOutput, RepositoryServiceError> {
        self.architecture_map_calls.fetch_add(1, Ordering::Relaxed);
        assert_eq!(request.max_files(), 7);
        Ok(architecture_map_output())
    }

    fn architecture_overview(
        &self,
        request: ArchitectureOverviewServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<ArchitectureOverviewOutput, RepositoryServiceError> {
        self.architecture_overview_calls
            .fetch_add(1, Ordering::Relaxed);
        assert_eq!(request.max_roots(), 3);
        assert_eq!(request.max_entry_point_candidates(), 5);
        assert_eq!(request.max_files(), 7);
        Ok(architecture_overview_output())
    }

    fn repository_topology(
        &self,
        request: RepositoryTopologyServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<RepositoryTopologyOutput, RepositoryServiceError> {
        self.repository_topology_calls
            .fetch_add(1, Ordering::Relaxed);
        assert_eq!(request.max_paths(), 7);
        Ok(repository_topology_output())
    }

    fn symbol_get(
        &self,
        request: SymbolGetServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<SymbolGetOutput, RepositoryServiceError> {
        self.symbol_calls.fetch_add(1, Ordering::Relaxed);
        assert_eq!(request.generation(), 9);
        Ok(symbol_output())
    }

    fn context_build(
        &self,
        request: ContextBuildServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<ContextBuildOutput, RepositoryServiceError> {
        self.context_calls.fetch_add(1, Ordering::Relaxed);
        self.context_request.lock().expect("lock").replace((
            request.intent().to_owned(),
            request.budget_units(),
            request.max_provider_results(),
        ));
        Ok(context_output())
    }

    fn phase2_context_build(
        &self,
        request: Phase2ContextBuildServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<Phase2ContextBuildOutput, RepositoryServiceError> {
        self.phase2_context_calls.fetch_add(1, Ordering::Relaxed);
        assert_eq!(request.intent(), "run");
        assert_eq!(request.budget_units(), 4096);
        assert_eq!(request.max_provider_results(), 7);
        Ok(phase2_context_output())
    }

    fn diagnostics(
        &self,
        _request: DiagnosticsServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<DiagnosticsOutput, RepositoryServiceError> {
        self.diagnostics_calls.fetch_add(1, Ordering::Relaxed);
        let mut output = diagnostics_output();
        if self.invalid_diagnostics.load(Ordering::Relaxed) {
            output.known_parser_limitation_nodes = output.syntax_error_nodes + 1;
        }
        Ok(output)
    }

    fn graph_read(
        &self,
        request: GraphReadServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<GraphReadServiceOutput, RepositoryServiceError> {
        self.graph_calls.fetch_add(1, Ordering::Relaxed);
        Ok(graph_output(request))
    }

    fn scip_evidence(
        &self,
        request: ScipEvidenceServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<ScipEvidenceOutput, RepositoryServiceError> {
        self.scip_calls.fetch_add(1, Ordering::Relaxed);
        assert_eq!(request.symbol().as_str(), "scip-rust pkg 1 Symbol.");
        Ok(scip_evidence_output())
    }

    fn scip_relationship_trace(
        &self,
        request: ScipRelationshipTraceServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<ScipRelationshipTraceOutput, RepositoryServiceError> {
        self.scip_relationship_trace_calls
            .fetch_add(1, Ordering::Relaxed);
        assert_eq!(request.symbol().as_str(), "scip-rust pkg 1 Symbol.");
        assert!(matches!(
            request.direction(),
            repowitness_application::ScipRelationshipTraceDirection::Outgoing
        ));
        assert_eq!(request.max_depth().get(), 2);
        assert_eq!(request.max_edges().get(), 8);
        Ok(scip_relationship_trace_output())
    }

    fn memory_recall(
        &self,
        request: MemoryRecallServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<MemoryRecallOutput, RepositoryServiceError> {
        self.memory_calls.fetch_add(1, Ordering::Relaxed);
        self.memory_request.lock().expect("lock").replace((
            request.selection() == &MemoryRecallServiceSelection::All,
            request.max_results(),
        ));
        Ok(memory_output())
    }

    fn memory_manage(
        &self,
        request: MemoryManageServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<MemoryManageOutput, RepositoryServiceError> {
        self.manage_calls.fetch_add(1, Ordering::Relaxed);
        let operation = match request {
            MemoryManageServiceRequest::Write { .. } => MemoryManageOperation::Write,
            MemoryManageServiceRequest::Approve { .. } => MemoryManageOperation::Approve,
            MemoryManageServiceRequest::Review { .. } => MemoryManageOperation::Review,
            MemoryManageServiceRequest::ImportHistory { .. } => {
                MemoryManageOperation::ImportHistory
            }
        };
        self.manage_request.lock().expect("lock").replace(operation);
        Ok(MemoryManageOutput::review_with_maintenance(
            true,
            confirmed_memory_maintenance(),
        ))
    }

    fn personal_memory(
        &self,
        request: PersonalMemoryServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<PersonalMemoryOutput, RepositoryServiceError> {
        assert!(matches!(
            request,
            PersonalMemoryServiceRequest::Read { max_results: 1, .. }
        ));
        Ok(PersonalMemoryOutput {
            schema_version: 1,
            scope: "personal".to_owned(),
            operation: PersonalMemoryOperation::Read,
            records: Vec::new(),
        })
    }
}

mod adversarial;
mod cancellation;
mod compatibility;
mod graph;
mod memory_manage;
mod mutation_timeout;

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one exact advertised-tool contract keeps the inventory, strict schemas, annotations, and protocol version assertions together"
)]
fn tool_contract_is_exact_sorted_versioned_and_read_only() {
    let server = RepoWitnessMcpServer::new(Arc::new(FakeService::new()));
    assert_eq!(
        server
            .tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>(),
        graph::native_tool_names()
    );
    for tool in server.tools.iter() {
        assert!(tool.input_schema.contains_key("properties"));
        assert_eq!(
            tool.input_schema.get("additionalProperties"),
            Some(&serde_json::Value::Bool(false))
        );
        assert!(tool.output_schema.is_some());
        let annotations = tool.annotations.as_ref().expect("annotations");
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(false));
    }
    let code_search = server
        .tools
        .iter()
        .find(|tool| tool.name.as_ref() == CODE_SEARCH_TOOL_NAME)
        .expect("code-search tool");
    assert!(
        code_search
            .description
            .as_deref()
            .is_some_and(|description| description.contains("Python"))
    );
    let code_graph_query = server
        .tools
        .iter()
        .find(|tool| tool.name.as_ref() == CODE_GRAPH_QUERY_TOOL_NAME)
        .expect("code graph query tool");
    let properties = code_graph_query
        .input_schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .expect("code graph query properties");
    for field in [
        "operation",
        "name",
        "match_mode",
        "language",
        "kind",
        "path_prefix",
        "query",
        "max_results",
        "timeout_ms",
        "snapshot_sha256",
        "generation",
        "path",
        "content_sha256",
        "artifact_sha256",
        "fact_ordinal",
        "max_sites",
        "target",
        "max_roots",
        "max_entry_point_candidates",
        "max_files",
        "max_paths",
    ] {
        assert!(
            properties.contains_key(field),
            "the advertised closed-union schema must permit {field:?} for its matching variant"
        );
    }
    for tool_name in [
        ARCHITECTURE_MAP_TOOL_NAME,
        CODE_SEARCH_TOOL_NAME,
        CONTEXT_BUILD_TOOL_NAME,
        SYMBOL_GET_TOOL_NAME,
        SYMBOL_SEARCH_TOOL_NAME,
    ] {
        let tool = server
            .tools
            .iter()
            .find(|tool| tool.name.as_ref() == tool_name)
            .expect("language-bearing tool");
        let schema = serde_json::to_string(
            tool.output_schema
                .as_ref()
                .expect("language-bearing tool has an output schema"),
        )
        .expect("output schema serializes");
        assert!(
            schema.contains("`python`"),
            "{tool_name} output schema must describe Python"
        );
    }
    let diagnostics = server
        .tools
        .iter()
        .find(|tool| tool.name.as_ref() == DIAGNOSTICS_TOOL_NAME)
        .expect("diagnostics tool");
    let diagnostics_properties = diagnostics
        .output_schema
        .as_ref()
        .and_then(|schema| schema.get("properties"))
        .and_then(serde_json::Value::as_object)
        .expect("diagnostics output properties");
    assert!(diagnostics_properties.contains_key("syntax_error_nodes"));
    assert!(diagnostics_properties.contains_key("known_parser_limitation_nodes"));
    assert_eq!(
        server.get_info().protocol_version,
        ProtocolVersion::V_2025_11_25
    );
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one transport-level isolation fixture keeps selector-schema, non-invocation, single-surface, and exact-routing assertions auditable together"
)]
async fn registry_mode_requires_an_explicit_selector_and_routes_only_to_its_service() {
    let first_id = "rwi1:h:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let second_id = "rwi1:h:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
    let first = Arc::new(FakeService::new());
    let second = Arc::new(FakeService::new());
    let server = RepoWitnessMcpServer::with_repository_registry(BTreeMap::from([
        (
            first_id.to_owned(),
            first.clone() as Arc<dyn RepositoryService>,
        ),
        (
            second_id.to_owned(),
            second.clone() as Arc<dyn RepositoryService>,
        ),
    ]))
    .expect("non-empty bounded registry");
    assert_eq!(server.tools.len(), 24);
    assert!(
        server
            .tools
            .iter()
            .all(|tool| tool.name.as_ref() != MEMORY_MANAGE_TOOL_NAME)
    );
    let code_search = server
        .tools
        .iter()
        .find(|tool| tool.name.as_ref() == CODE_SEARCH_TOOL_NAME)
        .expect("code-search tool");
    let properties = code_search
        .input_schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .expect("object properties");
    assert_eq!(
        properties
            .get("repository_id")
            .and_then(|value| value.get("enum")),
        Some(&serde_json::json!([first_id, second_id]))
    );
    assert!(
        code_search
            .input_schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|required| required.contains(&serde_json::json!("repository_id")))
    );
    let single_repository_server = RepoWitnessMcpServer::new(first.clone());
    let single_code_search = single_repository_server
        .tools
        .iter()
        .find(|tool| tool.name.as_ref() == CODE_SEARCH_TOOL_NAME)
        .expect("single-repository code-search tool");
    assert!(
        single_code_search
            .input_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|properties| !properties.contains_key("repository_id"))
    );

    let (server_transport, client_transport) = tokio::io::duplex(32 * 1024);
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("server starts")
            .waiting()
            .await
            .expect("server stops")
    });
    let client = ().serve(client_transport).await.expect("client starts");
    for arguments in [
        serde_json::json!({"query": "run"}),
        serde_json::json!({"query": "run", "repository_id": "unknown"}),
        serde_json::json!({"query": "run", "repository_id": 7}),
    ] {
        client
            .call_tool(
                CallToolRequestParams::new(CODE_SEARCH_TOOL_NAME)
                    .with_arguments(json_object(arguments)),
            )
            .await
            .expect_err("invalid registry selection must be a protocol error");
    }
    assert_eq!(first.search_calls.load(Ordering::Relaxed), 0);
    assert_eq!(second.search_calls.load(Ordering::Relaxed), 0);

    for (repository_id, expected_first_calls, expected_second_calls) in
        [(first_id, 1, 0), (second_id, 1, 1)]
    {
        let response = client
            .call_tool(
                CallToolRequestParams::new(CODE_SEARCH_TOOL_NAME).with_arguments(json_object(
                    serde_json::json!({"query": "run", "repository_id": repository_id}),
                )),
            )
            .await
            .expect("registered service call succeeds");
        assert_eq!(response.is_error, Some(false));
        assert_eq!(
            first.search_calls.load(Ordering::Relaxed),
            expected_first_calls
        );
        assert_eq!(
            second.search_calls.load(Ordering::Relaxed),
            expected_second_calls
        );
    }

    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}

#[tokio::test]
async fn catalog_mode_defaults_only_to_its_process_fixed_repository() {
    let first_id = "rwi1:h:CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC";
    let second_id = "rwi1:h:DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD";
    let first = Arc::new(FakeService::new());
    let second = Arc::new(FakeService::new());
    let server = RepoWitnessMcpServer::with_repository_catalog(
        BTreeMap::from([
            (
                first_id.to_owned(),
                first.clone() as Arc<dyn RepositoryService>,
            ),
            (
                second_id.to_owned(),
                second.clone() as Arc<dyn RepositoryService>,
            ),
        ]),
        first_id.to_owned(),
    )
    .expect("catalog default names an admitted service");
    let code_search = server
        .tools
        .iter()
        .find(|tool| tool.name.as_ref() == CODE_SEARCH_TOOL_NAME)
        .expect("code-search tool");
    assert!(
        code_search
            .input_schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|required| !required.contains(&serde_json::json!("repository_id")))
    );

    let (server_transport, client_transport) = tokio::io::duplex(32 * 1024);
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("server starts")
            .waiting()
            .await
            .expect("server stops")
    });
    let client = ().serve(client_transport).await.expect("client starts");
    client
        .call_tool(
            CallToolRequestParams::new(CODE_SEARCH_TOOL_NAME)
                .with_arguments(json_object(serde_json::json!({"query": "run"}))),
        )
        .await
        .expect("default catalog call succeeds");
    assert_eq!(first.search_calls.load(Ordering::Relaxed), 1);
    assert_eq!(second.search_calls.load(Ordering::Relaxed), 0);
    client
        .call_tool(
            CallToolRequestParams::new(CODE_SEARCH_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({"query": "run", "repository_id": second_id}),
            )),
        )
        .await
        .expect("explicit catalog call succeeds");
    assert_eq!(first.search_calls.load(Ordering::Relaxed), 1);
    assert_eq!(second.search_calls.load(Ordering::Relaxed), 1);
    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}

#[test]
fn encoded_call_tool_result_is_checked_against_the_output_budget() {
    let result = operation_result(Ok(search_output()), 32).expect("serialization succeeds");
    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        result.content[0].as_text().expect("text error").text,
        "tool output exceeded its byte limit"
    );
}

#[tokio::test]
async fn personal_memory_is_absent_by_default_and_requires_explicit_server_capability() {
    let default = RepoWitnessMcpServer::new(Arc::new(FakeService::new()));
    assert!(
        default
            .tools
            .iter()
            .all(|tool| tool.name.as_ref() != PERSONAL_MEMORY_TOOL_NAME)
    );
    let (server_transport, client_transport) = tokio::io::duplex(8 * 1024);
    let server = RepoWitnessMcpServer::with_surface_and_personal_memory(
        Arc::new(FakeService::new()),
        McpToolSurface::NativeV1,
    );
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("server starts")
            .waiting()
            .await
            .expect("server stops")
    });
    let client = ().serve(client_transport).await.expect("client starts");
    let tools = client.list_all_tools().await.expect("tools list");
    assert!(
        tools
            .iter()
            .any(|tool| tool.name.as_ref() == PERSONAL_MEMORY_TOOL_NAME)
    );
    let response = client
        .call_tool(
            CallToolRequestParams::new(PERSONAL_MEMORY_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({"operation": "read", "max_results": 1}),
            )),
        )
        .await
        .expect("personal-memory response");
    assert_eq!(response.is_error, Some(false));
    assert_eq!(
        response
            .structured_content
            .as_ref()
            .and_then(|value| value.get("scope"))
            .and_then(serde_json::Value::as_str),
        Some("personal")
    );
    drop(client);
    server_task.await.expect("server task joins");
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one linear protocol test verifies listing, invocation, and forwarding for every tool"
)]
async fn initialized_client_lists_and_calls_all_tools() {
    let service = Arc::new(FakeService::new());
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server = RepoWitnessMcpServer::new(service.clone());
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("server starts")
            .waiting()
            .await
            .expect("server stops")
    });
    let client = ().serve(client_transport).await.expect("client starts");

    let listed = client.list_all_tools().await.expect("tools list");
    assert_eq!(
        listed
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>(),
        graph::native_tool_names()
    );

    let architecture_map = client
        .call_tool(
            CallToolRequestParams::new(ARCHITECTURE_MAP_TOOL_NAME)
                .with_arguments(json_object(serde_json::json!({"max_files": 7}))),
        )
        .await
        .expect("architecture-map response");
    assert_eq!(architecture_map.is_error, Some(false));
    let architecture_content = architecture_map
        .structured_content
        .as_ref()
        .expect("architecture map structured content");
    assert_eq!(
        architecture_content
            .get("schema_version")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        architecture_content
            .get("limitation")
            .and_then(serde_json::Value::as_str),
        Some("file_inventory_only_no_relationship_inference")
    );

    let architecture_overview = client
        .call_tool(
            CallToolRequestParams::new(ARCHITECTURE_OVERVIEW_TOOL_NAME).with_arguments(
                json_object(serde_json::json!({
                    "max_roots": 3,
                    "max_entry_point_candidates": 5,
                    "max_files": 7,
                })),
            ),
        )
        .await
        .expect("architecture-overview response");
    assert_eq!(architecture_overview.is_error, Some(false));
    let architecture_overview_content = architecture_overview
        .structured_content
        .as_ref()
        .expect("architecture overview structured content");
    assert_eq!(
        architecture_overview_content
            .get("overview_profile")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        architecture_overview_content
            .get("limitations")
            .and_then(serde_json::Value::as_array)
            .map(|limitations| limitations.len()),
        Some(3)
    );

    let repository_topology = client
        .call_tool(
            CallToolRequestParams::new(REPOSITORY_TOPOLOGY_TOOL_NAME)
                .with_arguments(json_object(serde_json::json!({"max_paths": 7}))),
        )
        .await
        .expect("repository-topology response");
    assert_eq!(repository_topology.is_error, Some(false));
    let repository_topology_content = repository_topology
        .structured_content
        .as_ref()
        .expect("repository topology structured content");
    assert_eq!(
        repository_topology_content
            .get("topology_profile")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );

    let code_graph_query = client
        .call_tool(
            CallToolRequestParams::new(CODE_GRAPH_QUERY_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({
                    "operation": "files",
                    "max_files": 1,
                }),
            )),
        )
        .await
        .expect("code graph query response");
    assert_eq!(code_graph_query.is_error, Some(false));
    let code_graph_content = code_graph_query
        .structured_content
        .as_ref()
        .expect("code graph query structured content");
    assert_eq!(
        code_graph_content.get("code_graph_query_profile"),
        Some(&serde_json::json!(1))
    );
    assert_eq!(
        code_graph_content.get("operation"),
        Some(&serde_json::json!("files"))
    );

    let search = client
        .call_tool(
            CallToolRequestParams::new(CODE_SEARCH_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({"query": "  run  ", "max_results": 7}),
            )),
        )
        .await
        .expect("search response");
    assert_eq!(search.is_error, Some(false));
    assert_eq!(
        search
            .structured_content
            .as_ref()
            .and_then(|value| value.get("schema_version"))
            .and_then(serde_json::Value::as_u64),
        Some(3)
    );

    let relevant_paths = client
        .call_tool(
            CallToolRequestParams::new(RELEVANT_PATHS_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({"query": "run", "max_paths": 7}),
            )),
        )
        .await
        .expect("relevant-path response");
    assert_eq!(relevant_paths.is_error, Some(false));
    assert_eq!(
        relevant_paths
            .structured_content
            .as_ref()
            .and_then(|value| value.get("path_ranking_profile"))
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        relevant_paths
            .structured_content
            .as_ref()
            .and_then(|value| value.get("limitations"))
            .and_then(serde_json::Value::as_array)
            .map(|limitations| limitations.len()),
        Some(4)
    );
    let relevant_paths = relevant_paths
        .structured_content
        .as_ref()
        .expect("relevant-path content");
    assert_eq!(relevant_paths["matches_returned"], 1);
    assert_eq!(relevant_paths["matches_total"], 2);
    assert_eq!(relevant_paths["coverage"]["truncated"], 1);
    assert_eq!(relevant_paths["paths_returned"], 1);
    assert_eq!(relevant_paths["returned_match_paths_total"], 1);
    assert_eq!(relevant_paths["returned_match_paths_truncated"], false);

    let symbol_search = client
        .call_tool(
            CallToolRequestParams::new(SYMBOL_SEARCH_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({
                    "name": "run",
                    "match_mode": "prefix",
                    "language": "rust",
                    "kind": "struct",
                    "path_prefix": "src",
                    "max_results": 7,
                }),
            )),
        )
        .await
        .expect("symbol-search response");
    assert_eq!(symbol_search.is_error, Some(false));
    assert_eq!(
        symbol_search
            .structured_content
            .as_ref()
            .and_then(|value| value.get("schema_version"))
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );

    let outbound_sites = client
        .call_tool(
            CallToolRequestParams::new(OUTBOUND_SITES_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({
                    "snapshot_sha256": "11".repeat(32),
                    "generation": 9,
                    "path": "rwp1:h:7372632F6C69622E7273",
                    "content_sha256": "22".repeat(32),
                    "artifact_sha256": "33".repeat(32),
                    "fact_ordinal": 7,
                    "max_sites": 7,
                }),
            )),
        )
        .await
        .expect("outbound-sites response");
    assert_eq!(outbound_sites.is_error, Some(false));
    assert_eq!(
        outbound_sites
            .structured_content
            .as_ref()
            .and_then(|value| value.get("limitation"))
            .and_then(serde_json::Value::as_str),
        Some("raw_syntax_observations_only_no_target_resolution_or_inferred_edges")
    );

    let syntax_site_search = client
        .call_tool(
            CallToolRequestParams::new(SYNTAX_SITE_SEARCH_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({"target": "run", "max_sites": 7}),
            )),
        )
        .await
        .expect("syntax-site-search response");
    assert_eq!(syntax_site_search.is_error, Some(false));
    assert_eq!(
        syntax_site_search
            .structured_content
            .as_ref()
            .and_then(|value| value.get("syntax_site_search_profile"))
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        syntax_site_search
            .structured_content
            .as_ref()
            .and_then(|value| value.get("limitation"))
            .and_then(serde_json::Value::as_str),
        Some("exact_raw_target_syntax_observations_only_no_target_resolution_or_inferred_edges")
    );

    let context = client
        .call_tool(
            CallToolRequestParams::new(CONTEXT_BUILD_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({
                    "query": "  run  ",
                    "max_chars": 4096,
                    "max_provider_results": 7
                }),
            )),
        )
        .await
        .expect("context response");
    assert_eq!(context.is_error, Some(false));
    assert_eq!(
        context
            .structured_content
            .as_ref()
            .and_then(|value| value.get("schema_version"))
            .and_then(serde_json::Value::as_u64),
        Some(2)
    );

    let phase2_context = client
        .call_tool(
            CallToolRequestParams::new(PHASE2_CONTEXT_BUILD_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({
                    "intent": "  run  ",
                    "budget_units": 4096,
                    "max_provider_results": 7
                }),
            )),
        )
        .await
        .expect("Phase 2 context response");
    assert_eq!(phase2_context.is_error, Some(false));
    assert_eq!(
        phase2_context
            .structured_content
            .as_ref()
            .and_then(|value| value.get("schema_version"))
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );

    let diagnostics = client
        .call_tool(CallToolRequestParams::new(DIAGNOSTICS_TOOL_NAME))
        .await
        .expect("diagnostics response");
    assert_eq!(diagnostics.is_error, Some(false));
    assert_eq!(
        diagnostics
            .structured_content
            .as_ref()
            .and_then(|value| value.get("schema_version"))
            .and_then(serde_json::Value::as_u64),
        Some(3)
    );
    let diagnostics_content = diagnostics
        .structured_content
        .as_ref()
        .expect("diagnostics structured content");
    assert_eq!(
        diagnostics_content
            .get("configuration")
            .and_then(|value| value.get("profile"))
            .and_then(serde_json::Value::as_str),
        Some("local")
    );
    assert_eq!(
        diagnostics_content
            .get("syntax_error_nodes")
            .and_then(serde_json::Value::as_u64),
        Some(4)
    );
    assert_eq!(
        diagnostics_content
            .get("known_parser_limitation_nodes")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );

    let memory = client
        .call_tool(
            CallToolRequestParams::new(MEMORY_RECALL_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({"all_records": true, "max_results": 7}),
            )),
        )
        .await
        .expect("memory response");
    assert_eq!(memory.is_error, Some(false));
    assert_eq!(
        memory
            .structured_content
            .as_ref()
            .and_then(|value| value.get("schema_version"))
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );

    let symbol = client
        .call_tool(
            CallToolRequestParams::new(SYMBOL_GET_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({
                    "snapshot_sha256": "11".repeat(32),
                    "generation": 9,
                    "path": "rwp1:h:7372632F6C69622E7273",
                    "content_sha256": "22".repeat(32),
                    "artifact_sha256": "33".repeat(32),
                    "fact_ordinal": 7,
                }),
            )),
        )
        .await
        .expect("symbol response");
    assert_eq!(symbol.is_error, Some(false));
    let scip = client
        .call_tool(
            CallToolRequestParams::new(SCIP_EVIDENCE_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({
                    "symbol": "scip-rust pkg 1 Symbol.",
                }),
            )),
        )
        .await
        .expect("SCIP evidence response");
    assert_eq!(scip.is_error, Some(false));
    assert_eq!(
        scip.structured_content
            .as_ref()
            .and_then(|value| value.get("resolution"))
            .and_then(serde_json::Value::as_str),
        Some("not_produced")
    );
    let scip_trace = client
        .call_tool(
            CallToolRequestParams::new(SCIP_RELATIONSHIP_TRACE_TOOL_NAME).with_arguments(
                json_object(serde_json::json!({
                    "symbol": "scip-rust pkg 1 Symbol.",
                    "direction": "outgoing",
                    "max_depth": 2,
                    "max_edges": 8,
                })),
            ),
        )
        .await
        .expect("SCIP relationship trace response");
    assert_eq!(scip_trace.is_error, Some(false));
    assert_eq!(
        scip_trace
            .structured_content
            .as_ref()
            .and_then(|value| value.get("resolution"))
            .and_then(serde_json::Value::as_str),
        Some("not_produced")
    );
    for (tool, arguments) in graph::tool_requests() {
        let response = client
            .call_tool(CallToolRequestParams::new(tool).with_arguments(json_object(arguments)))
            .await
            .expect("graph response");
        assert_eq!(response.is_error, Some(false), "{tool}");
        assert_eq!(
            response
                .structured_content
                .as_ref()
                .and_then(|value| value.get("schema_version"))
                .and_then(serde_json::Value::as_u64),
            Some(1),
            "{tool}"
        );
    }
    assert_eq!(service.architecture_map_calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        service.architecture_overview_calls.load(Ordering::Relaxed),
        1
    );
    assert_eq!(service.repository_topology_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.code_graph_query_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.search_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.context_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.phase2_context_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.diagnostics_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.memory_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.symbol_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.outbound_sites_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.syntax_site_search_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.scip_calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        service
            .scip_relationship_trace_calls
            .load(Ordering::Relaxed),
        1
    );
    assert_eq!(service.graph_calls.load(Ordering::Relaxed), 6);
    assert_eq!(
        service.search_request.lock().expect("lock").as_ref(),
        Some(&("run".to_owned(), 7))
    );
    assert_eq!(
        service.memory_request.lock().expect("lock").as_ref(),
        Some(&(true, 7))
    );
    assert_eq!(
        service.context_request.lock().expect("lock").as_ref(),
        Some(&("run".to_owned(), 4096, 7))
    );

    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}

#[test]
fn native_tasks_are_opt_in_and_only_the_phase2_context_tool_is_task_capable() {
    let default = RepoWitnessMcpServer::new(Arc::new(FakeService::new()));
    assert!(default.get_info().capabilities.tasks.is_none());
    let enabled = RepoWitnessMcpServer::with_native_tasks(Arc::new(FakeService::new()));
    assert!(enabled.get_info().capabilities.tasks.is_some());
    let task_tools = enabled
        .tools
        .iter()
        .filter(|tool| tool.task_support() != rmcp::model::TaskSupport::Forbidden)
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(task_tools, vec![PHASE2_CONTEXT_BUILD_TOOL_NAME]);
}

#[tokio::test]
async fn native_task_submission_returns_an_opaque_id_and_retains_a_bounded_result() {
    let (server_transport, client_transport) = tokio::io::duplex(32 * 1024);
    let server = RepoWitnessMcpServer::with_native_tasks(Arc::new(FakeService::new()));
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("server starts")
            .waiting()
            .await
            .expect("server stops")
    });
    let client = ().serve(client_transport).await.expect("client starts");
    let response = client
        .send_request(ClientRequest::CallToolRequest(CallToolRequest::new(
            CallToolRequestParams::new(PHASE2_CONTEXT_BUILD_TOOL_NAME)
                .with_arguments(json_object(serde_json::json!({
                    "intent": "run",
                    "budget_units": 4096,
                    "max_provider_results": 7
                })))
                .with_task(TaskMetadata::new()),
        )))
        .await
        .expect("task is accepted");
    let ServerResult::CreateTaskResult(created) = response else {
        panic!("task invocation must create a native task");
    };
    let task_id = created.task.task_id;
    let suffix = task_id.as_str();
    assert_eq!(suffix.len(), 32);
    assert!(
        suffix
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    );

    let mut completed = false;
    let mut last_status = None;
    for _ in 0..100 {
        let response = client
            .send_request(ClientRequest::GetTaskRequest(GetTaskRequest::new(
                GetTaskParams::new(task_id.clone()),
            )))
            .await
            .expect("task remains queryable");
        let ServerResult::GetTaskResult(status) = response else {
            panic!("task query must return its status");
        };
        last_status = Some((
            status.task.status.clone(),
            status.task.status_message.clone(),
        ));
        if status.task.status == TaskStatus::Completed {
            completed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        completed,
        "native task must reach its terminal state: {last_status:?}"
    );
    let response = client
        .send_request(ClientRequest::GetTaskPayloadRequest(
            GetTaskPayloadRequest::new(GetTaskPayloadParams::new(task_id)),
        ))
        .await
        .expect("completed task result is available");
    // Task payloads retain the original `CallToolResult` wire shape, which
    // the SDK decodes as that concrete result before its custom fallback.
    let ServerResult::CallToolResult(result) = response else {
        panic!("task result must use the negotiated task payload response");
    };
    assert!(result.structured_content.is_some());

    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}

#[tokio::test]
async fn invalid_arguments_do_not_invoke_the_service() {
    let service = Arc::new(FakeService::new());
    let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
    let server = RepoWitnessMcpServer::new(service.clone());
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("server starts")
            .waiting()
            .await
            .expect("server stops")
    });
    let client = ().serve(client_transport).await.expect("client starts");
    let error = client
        .call_tool(
            CallToolRequestParams::new(CODE_SEARCH_TOOL_NAME)
                .with_arguments(json_object(serde_json::json!({"query": ""}))),
        )
        .await
        .expect_err("invalid params must be a protocol error");
    assert!(error.to_string().contains("bounded literal"));
    assert_eq!(service.search_calls.load(Ordering::Relaxed), 0);
    let error = client
        .call_tool(
            CallToolRequestParams::new(CODE_GRAPH_QUERY_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({
                    "operation": "cypher",
                    "query": "MATCH (n)",
                }),
            )),
        )
        .await
        .expect_err("unknown finite operation must be a protocol error");
    assert!(!error.to_string().is_empty());
    assert_eq!(service.code_graph_query_calls.load(Ordering::Relaxed), 0);
    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn synchronous_repository_work_never_exceeds_the_semaphore_bound() {
    let service = Arc::new(ConcurrencyService {
        active: AtomicUsize::new(0),
        maximum: AtomicUsize::new(0),
    });
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server = RepoWitnessMcpServer::with_operation_concurrency(service.clone(), 2);
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("server starts")
            .waiting()
            .await
            .expect("server stops")
    });
    let client = ().serve(client_transport).await.expect("client starts");
    let request = || {
        CallToolRequestParams::new(CODE_SEARCH_TOOL_NAME).with_arguments(json_object(
            serde_json::json!({"query": "run", "timeout_ms": 1000}),
        ))
    };
    let (one, two, three, four) = tokio::join!(
        client.call_tool(request()),
        client.call_tool(request()),
        client.call_tool(request()),
        client.call_tool(request()),
    );
    for result in [one, two, three, four] {
        assert_eq!(result.expect("tool result").is_error, Some(false));
    }
    assert_eq!(service.maximum.load(Ordering::Acquire), 2);
    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}
