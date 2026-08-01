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
    PersonalMemoryOperation, SymbolSelectorOutput,
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
    search_calls: AtomicUsize,
    context_calls: AtomicUsize,
    phase2_context_calls: AtomicUsize,
    diagnostics_calls: AtomicUsize,
    graph_calls: AtomicUsize,
    scip_calls: AtomicUsize,
    invalid_diagnostics: AtomicBool,
    manage_calls: AtomicUsize,
    memory_calls: AtomicUsize,
    symbol_calls: AtomicUsize,
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
            search_calls: AtomicUsize::new(0),
            context_calls: AtomicUsize::new(0),
            phase2_context_calls: AtomicUsize::new(0),
            diagnostics_calls: AtomicUsize::new(0),
            graph_calls: AtomicUsize::new(0),
            scip_calls: AtomicUsize::new(0),
            invalid_diagnostics: AtomicBool::new(false),
            manage_calls: AtomicUsize::new(0),
            memory_calls: AtomicUsize::new(0),
            symbol_calls: AtomicUsize::new(0),
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
    for tool_name in [
        CODE_SEARCH_TOOL_NAME,
        CONTEXT_BUILD_TOOL_NAME,
        SYMBOL_GET_TOOL_NAME,
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
    assert_eq!(service.search_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.context_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.phase2_context_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.diagnostics_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.memory_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.symbol_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.scip_calls.load(Ordering::Relaxed), 1);
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
