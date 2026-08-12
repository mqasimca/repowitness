use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use rmcp::{ServiceExt, model::CallToolRequestParams};

use super::*;
use crate::{
    MAX_MCP_INTEROPERABLE_INTEGER, MEMORY_MANAGE_SCHEMA_VERSION, McpCoverage, McpMemoryCoverage,
    McpMemoryProducer, McpMemoryTarget, McpSearchMatch, McpSpan, McpSymbol,
    MemoryManageDatabaseIdentityStatus, MemoryManageMaintenanceStatus,
    MemoryManageMaintenanceStepStatus, MemoryManageOperation, MemoryRecallServiceSelection,
    RepositoryTopologyOutput, SymbolSelectorOutput,
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
    diagnostics_calls: AtomicUsize,
    graph_calls: AtomicUsize,
    invalid_diagnostics: AtomicBool,
    search_truncated: AtomicBool,
    manage_calls: AtomicUsize,
    memory_calls: AtomicUsize,
    symbol_calls: AtomicUsize,
    outbound_sites_calls: AtomicUsize,
    syntax_site_search_calls: AtomicUsize,
    search_request: Mutex<Option<(String, u16)>>,
    context_request: Mutex<Option<(String, u64, u16)>>,
    memory_request: Mutex<Option<(bool, u16)>>,
    manage_request: Mutex<Option<MemoryManageOperation>>,
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
        _request: EvidenceContextBuildServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<EvidenceContextBuildOutput, RepositoryServiceError> {
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
            diagnostics_calls: AtomicUsize::new(0),
            graph_calls: AtomicUsize::new(0),
            invalid_diagnostics: AtomicBool::new(false),
            search_truncated: AtomicBool::new(false),
            manage_calls: AtomicUsize::new(0),
            memory_calls: AtomicUsize::new(0),
            symbol_calls: AtomicUsize::new(0),
            outbound_sites_calls: AtomicUsize::new(0),
            syntax_site_search_calls: AtomicUsize::new(0),
            search_request: Mutex::new(None),
            context_request: Mutex::new(None),
            memory_request: Mutex::new(None),
            manage_request: Mutex::new(None),
        }
    }
}

impl RepositoryService for FakeService {
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
        let mut output = search_output();
        if self.search_truncated.load(Ordering::Relaxed) {
            output.coverage.truncated = 1;
        }
        Ok(output)
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
        request: EvidenceContextBuildServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<EvidenceContextBuildOutput, RepositoryServiceError> {
        self.context_calls.fetch_add(1, Ordering::Relaxed);
        self.context_request.lock().expect("lock").replace((
            request.intent().to_owned(),
            request.budget_units(),
            request.max_provider_results(),
        ));
        Ok(evidence_context_output())
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
}

mod adversarial;
mod cancellation;
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
                    "intent": "  run  ",
                    "budget_units": 4096,
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
    assert_eq!(service.diagnostics_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.memory_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.symbol_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.outbound_sites_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.syntax_site_search_calls.load(Ordering::Relaxed), 1);
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

#[tokio::test]
async fn catalog_routes_one_mcp_connection_to_the_selected_repository() {
    let first = Arc::new(FakeService::new());
    let second = Arc::new(FakeService::new());
    let first_id = "rwi1:h:11".to_owned();
    let second_id = "rwi1:h:22".to_owned();
    let mut registry: BTreeMap<String, Arc<dyn RepositoryService>> = BTreeMap::new();
    registry.insert(first_id.clone(), first.clone());
    registry.insert(second_id.clone(), second.clone());

    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server =
        RepoWitnessMcpServer::with_repository_catalog(registry, None).expect("catalog is valid");
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
    let code_search = client
        .list_all_tools()
        .await
        .expect("tools list")
        .into_iter()
        .find(|tool| tool.name.as_ref() == CODE_SEARCH_TOOL_NAME)
        .expect("code search tool");
    assert!(
        code_search
            .input_schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|required| required.iter().any(|value| value == "repository_id"))
    );

    let response = client
        .call_tool(
            CallToolRequestParams::new(CODE_SEARCH_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({"repository_id": second_id, "query": "run"}),
            )),
        )
        .await
        .expect("catalog response");
    assert_eq!(response.is_error, Some(false));
    assert_eq!(first.search_calls.load(Ordering::Relaxed), 0);
    assert_eq!(second.search_calls.load(Ordering::Relaxed), 1);

    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}

#[tokio::test]
async fn catalog_cross_repository_search_fans_out_and_keeps_repository_receipts() {
    let first = Arc::new(FakeService::new());
    let second = Arc::new(FakeService::new());
    let first_id = "rwi1:h:11".to_owned();
    let second_id = "rwi1:h:22".to_owned();
    let mut registry: BTreeMap<String, Arc<dyn RepositoryService>> = BTreeMap::new();
    registry.insert(first_id.clone(), first.clone());
    registry.insert(second_id.clone(), second.clone());

    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server = RepoWitnessMcpServer::with_repository_catalog(registry, Some(first_id.clone()))
        .expect("catalog is valid");
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
            .any(|tool| tool.name.as_ref() == CROSS_REPOSITORY_SEARCH_TOOL_NAME)
    );

    let response = client
        .call_tool(
            CallToolRequestParams::new(CROSS_REPOSITORY_SEARCH_TOOL_NAME).with_arguments(
                json_object(serde_json::json!({"query": "run", "max_results": 1})),
            ),
        )
        .await
        .expect("cross-repository response");
    assert_eq!(response.is_error, Some(false));
    let output = response.structured_content.expect("structured output");
    assert_eq!(output["repositories_requested"], 2);
    assert_eq!(output["repositories_completed"], 2);
    assert_eq!(output["matches_returned"], 1);
    let repositories = output["repositories"].as_array().expect("repositories");
    assert_eq!(repositories[0]["repository_id"], first_id);
    assert_eq!(repositories[1]["repository_id"], second_id);
    assert_eq!(first.search_calls.load(Ordering::Relaxed), 1);
    assert_eq!(second.search_calls.load(Ordering::Relaxed), 1);

    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}

#[tokio::test]
async fn catalog_cross_repository_search_preserves_partial_search_coverage() {
    let first = Arc::new(FakeService::new());
    first.search_truncated.store(true, Ordering::Relaxed);
    let second = Arc::new(FakeService::new());
    let mut registry: BTreeMap<String, Arc<dyn RepositoryService>> = BTreeMap::new();
    registry.insert("rwi1:h:11".to_owned(), first);
    registry.insert("rwi1:h:22".to_owned(), second);

    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server =
        RepoWitnessMcpServer::with_repository_catalog(registry, None).expect("catalog is valid");
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
        .call_tool(
            CallToolRequestParams::new(CROSS_REPOSITORY_SEARCH_TOOL_NAME)
                .with_arguments(json_object(serde_json::json!({"query": "run"}))),
        )
        .await
        .expect("cross-repository response");
    let output = response.structured_content.expect("structured output");
    assert_eq!(output["resolution"], "partial");
    assert_eq!(output["coverage"]["truncated"], 1);

    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}

#[tokio::test]
async fn single_repository_mcp_does_not_advertise_cross_repository_search() {
    let service = Arc::new(FakeService::new());
    let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
    let server = RepoWitnessMcpServer::new(service);
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
    assert!(
        !client
            .list_all_tools()
            .await
            .expect("tools list")
            .iter()
            .any(|tool| tool.name.as_ref() == CROSS_REPOSITORY_SEARCH_TOOL_NAME)
    );
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
