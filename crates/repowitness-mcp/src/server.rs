use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, CancelTaskParams, CancelTaskResult, ContentBlock,
        CreateTaskResult, GetTaskParams, GetTaskPayloadParams, GetTaskPayloadResult, GetTaskResult,
        Implementation, JsonObject, ListTasksResult, ListToolsResult, PaginatedRequestParams,
        ProtocolVersion, ServerCapabilities, ServerInfo, Task, TaskStatus, TaskSupport, Tool,
        ToolAnnotations, ToolExecution,
    },
    service::RequestContext,
};
use serde::{Serialize, de::DeserializeOwned};
use tokio::{
    sync::{Mutex, OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
    time::Instant,
};

use crate::{
    BoundedLineReader, CODE_SEARCH_TOOL_NAME, CONTEXT_BUILD_TOOL_NAME, CodeSearchInput,
    CodeSearchOutput, CodeSearchServiceRequest, ContextBuildInput, ContextBuildOutput,
    ContextBuildServiceRequest, DIAGNOSTICS_TOOL_NAME, DiagnosticsInput, DiagnosticsOutput,
    DiagnosticsServiceRequest, GRAPH_ARCHITECTURE_TOOL_NAME, GRAPH_EVIDENCE_TOOL_NAME,
    GRAPH_SEARCH_TOOL_NAME, GRAPH_STATUS_TOOL_NAME, GRAPH_TRACE_TOOL_NAME, GraphArchitectureInput,
    GraphArchitectureOutput, GraphEvidenceInput, GraphEvidenceOutput, GraphImpactInput,
    GraphImpactOutput, GraphReadServiceOutput, GraphReadServiceRequest, GraphSearchInput,
    GraphSearchOutput, GraphStatusInput, GraphStatusOutput, GraphTraceInput, GraphTraceOutput,
    HISTORICAL_MEMORY_TOOL_NAME, HistoricalMemoryInput, HistoricalMemoryOutput,
    HistoricalMemoryServiceRequest, IMPACT_ANALYZE_TOOL_NAME, MAX_MCP_INPUT_LINE_BYTES,
    MEMORY_MANAGE_TOOL_NAME, MEMORY_RECALL_TOOL_NAME, MemoryManageInput, MemoryManageOutput,
    MemoryManageServiceRequest, MemoryMutationRequestScope, MemoryRecallInput, MemoryRecallOutput,
    MemoryRecallServiceRequest, NativeTaskState, NativeTaskStatus, PERSONAL_MEMORY_TOOL_NAME,
    PHASE2_CONTEXT_BUILD_TOOL_NAME, PersonalMemoryInput, PersonalMemoryOutput,
    PersonalMemoryServiceRequest, Phase2ContextBuildInput, Phase2ContextBuildOutput,
    Phase2ContextBuildServiceRequest, RepositoryService, RepositoryServiceError,
    SCIP_EVIDENCE_TOOL_NAME, SYMBOL_GET_TOOL_NAME, ScipEvidenceInput, ScipEvidenceOutput,
    ScipEvidenceServiceRequest, SymbolGetInput, SymbolGetOutput, SymbolGetServiceRequest,
    wire::{
        MAX_MCP_CONTEXT_OUTPUT_BYTES, MAX_MCP_DIAGNOSTICS_OUTPUT_BYTES, MAX_MCP_GRAPH_OUTPUT_BYTES,
        MAX_MCP_HISTORICAL_MEMORY_OUTPUT_BYTES, MAX_MCP_MEMORY_MANAGE_OUTPUT_BYTES,
        MAX_MCP_MEMORY_RECALL_OUTPUT_BYTES, MAX_MCP_PERSONAL_MEMORY_OUTPUT_BYTES,
        MAX_MCP_PHASE2_CONTEXT_OUTPUT_BYTES, MAX_MCP_SCIP_EVIDENCE_OUTPUT_BYTES,
        MAX_MCP_SEARCH_OUTPUT_BYTES, MAX_MCP_SYMBOL_OUTPUT_BYTES,
    },
};

/// Maximum synchronous repository operations admitted concurrently by default.
pub const DEFAULT_MCP_OPERATION_CONCURRENCY: usize = 4;

/// Maximum number of native MCP task payload records retained at once.
///
/// Durable engineering-task state is authoritative and survives this registry;
/// only bounded transport payloads and running handles live here.
const MAX_NATIVE_TASKS: usize = 16;
const NATIVE_TASK_PAGE_SIZE: usize = 8;
const DEFAULT_NATIVE_TASK_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_NATIVE_TASK_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_NATIVE_TASK_RESULT_BYTES: usize = MAX_MCP_PHASE2_CONTEXT_OUTPUT_BYTES;

/// Stable local MCP server lifecycle failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpServeError {
    /// The SDK could not initialize the stdio service.
    Initialize,
    /// The initialized service stopped with a transport or protocol failure.
    Runtime,
}

impl fmt::Display for McpServeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Initialize => "MCP service initialization failed",
            Self::Runtime => "MCP service runtime failed",
        })
    }
}

impl Error for McpServeError {}

/// Bounded MCP server over an injected path-confined repository service.
#[derive(Clone)]
pub struct RepoWitnessMcpServer {
    service: Arc<dyn RepositoryService>,
    operations: Arc<Semaphore>,
    tools: Arc<[Tool]>,
    memory_writes_enabled: bool,
    personal_memory_enabled: bool,
    tasks_enabled: bool,
    tasks: Arc<Mutex<NativeTasks>>,
    surface: McpToolSurface,
}

struct NativeTasks {
    entries: BTreeMap<String, NativeTask>,
}

struct NativeTask {
    task: Task,
    result: Option<serde_json::Value>,
    handle: Option<tokio::task::JoinHandle<()>>,
    expires_at: Instant,
}

impl NativeTasks {
    fn prune_expired(&mut self, now: Instant) {
        self.entries
            .retain(|_, entry| entry.task.status == TaskStatus::Working || entry.expires_at > now);
    }
}

fn native_task_retention(requested_ttl: Option<u64>) -> Duration {
    requested_ttl
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_NATIVE_TASK_TTL)
        .max(Duration::from_secs(1))
        .min(MAX_NATIVE_TASK_TTL)
}

fn bounded_native_task_result(result: CallToolResult) -> Option<serde_json::Value> {
    let result = serde_json::to_value(result).ok()?;
    (serde_json::to_vec(&result).ok()?.len() <= MAX_NATIVE_TASK_RESULT_BYTES).then_some(result)
}

impl fmt::Debug for RepoWitnessMcpServer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepoWitnessMcpServer")
            .field("service", &"<injected-repository-service>")
            .field("available_permits", &self.operations.available_permits())
            .field("tool_count", &self.tools.len())
            .field("memory_writes_enabled", &self.memory_writes_enabled)
            .field("personal_memory_enabled", &self.personal_memory_enabled)
            .field("tasks_enabled", &self.tasks_enabled)
            .field("surface", &self.surface)
            .finish()
    }
}

impl RepoWitnessMcpServer {
    /// Constructs the default bounded Phase 0 MCP server.
    #[must_use]
    pub fn new(service: Arc<dyn RepositoryService>) -> Self {
        Self::configured(
            service,
            DEFAULT_MCP_OPERATION_CONCURRENCY,
            false,
            false,
            McpToolSurface::NativeV1,
        )
    }

    /// Constructs a bounded server with the local memory-mutation tool enabled.
    #[must_use]
    pub fn with_memory_writes(service: Arc<dyn RepositoryService>) -> Self {
        Self::configured(
            service,
            DEFAULT_MCP_OPERATION_CONCURRENCY,
            true,
            false,
            McpToolSurface::NativeV1,
        )
    }

    /// Constructs a bounded server with native negotiated MCP Tasks enabled.
    #[must_use]
    pub fn with_native_tasks(service: Arc<dyn RepositoryService>) -> Self {
        Self::configured_with_tasks(
            service,
            DEFAULT_MCP_OPERATION_CONCURRENCY,
            false,
            false,
            McpToolSurface::NativeV1,
            true,
        )
    }

    /// Constructs a bounded server with one explicit fixed tool surface.
    #[must_use]
    pub fn with_surface(service: Arc<dyn RepositoryService>, surface: McpToolSurface) -> Self {
        Self::configured(
            service,
            DEFAULT_MCP_OPERATION_CONCURRENCY,
            false,
            false,
            surface,
        )
    }

    /// Constructs a bounded server with an explicit surface and authorized memory writes.
    #[must_use]
    pub fn with_surface_and_memory_writes(
        service: Arc<dyn RepositoryService>,
        surface: McpToolSurface,
    ) -> Self {
        Self::configured(
            service,
            DEFAULT_MCP_OPERATION_CONCURRENCY,
            true,
            false,
            surface,
        )
    }

    /// Constructs a server whose local composition has fixed exactly one
    /// personal-memory profile before transport startup.
    #[must_use]
    pub fn with_surface_and_personal_memory(
        service: Arc<dyn RepositoryService>,
        surface: McpToolSurface,
    ) -> Self {
        Self::configured(
            service,
            DEFAULT_MCP_OPERATION_CONCURRENCY,
            false,
            true,
            surface,
        )
    }

    /// Constructs a server with an explicit positive operation-concurrency bound.
    ///
    /// # Panics
    ///
    /// Panics if `operation_concurrency` is zero. This value is an internal
    /// composition constant rather than untrusted protocol input.
    #[must_use]
    pub fn with_operation_concurrency(
        service: Arc<dyn RepositoryService>,
        operation_concurrency: usize,
    ) -> Self {
        Self::configured(
            service,
            operation_concurrency,
            false,
            false,
            McpToolSurface::NativeV1,
        )
    }

    fn configured(
        service: Arc<dyn RepositoryService>,
        operation_concurrency: usize,
        memory_writes_enabled: bool,
        personal_memory_enabled: bool,
        surface: McpToolSurface,
    ) -> Self {
        Self::configured_with_tasks(
            service,
            operation_concurrency,
            memory_writes_enabled,
            personal_memory_enabled,
            surface,
            false,
        )
    }

    fn configured_with_tasks(
        service: Arc<dyn RepositoryService>,
        operation_concurrency: usize,
        memory_writes_enabled: bool,
        personal_memory_enabled: bool,
        surface: McpToolSurface,
        tasks_enabled: bool,
    ) -> Self {
        assert!(
            operation_concurrency > 0,
            "MCP operation concurrency must be positive"
        );
        Self {
            service,
            operations: Arc::new(Semaphore::new(operation_concurrency)),
            tools: Arc::from(tools(
                memory_writes_enabled,
                personal_memory_enabled,
                surface,
                tasks_enabled,
            )),
            memory_writes_enabled,
            personal_memory_enabled,
            tasks_enabled,
            tasks: Arc::new(Mutex::new(NativeTasks {
                entries: BTreeMap::new(),
            })),
            surface,
        }
    }

    async fn call_code_search(
        &self,
        request: CodeSearchServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.code_search(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_SEARCH_OUTPUT_BYTES)
    }

    async fn call_symbol_get(
        &self,
        request: SymbolGetServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.symbol_get(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_SYMBOL_OUTPUT_BYTES)
    }

    async fn call_context_build(
        &self,
        request: ContextBuildServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.context_build(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_CONTEXT_OUTPUT_BYTES)
    }

    async fn call_phase2_context_build(
        &self,
        request: Phase2ContextBuildServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.phase2_context_build(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_PHASE2_CONTEXT_OUTPUT_BYTES)
    }

    async fn call_memory_recall(
        &self,
        request: MemoryRecallServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.memory_recall(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_MEMORY_RECALL_OUTPUT_BYTES)
    }

    async fn call_memory_manage(
        &self,
        request: MemoryManageServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let request_scope = request.mutation_request_scope();
        let output = self
            .run_memory_mutation_blocking(
                timeout,
                context,
                request_scope,
                move |remaining, cancelled| {
                    service.memory_manage(request.with_timeout(remaining), cancelled)
                },
            )
            .await?;
        operation_result(output, MAX_MCP_MEMORY_MANAGE_OUTPUT_BYTES)
    }

    async fn call_historical_memory(
        &self,
        request: HistoricalMemoryServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.historical_memory(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_HISTORICAL_MEMORY_OUTPUT_BYTES)
    }

    async fn call_personal_memory(
        &self,
        request: PersonalMemoryServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.personal_memory(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_PERSONAL_MEMORY_OUTPUT_BYTES)
    }

    async fn call_diagnostics(
        &self,
        request: DiagnosticsServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.diagnostics(request.with_timeout(remaining), cancelled)
            })
            .await?;
        let output = output.and_then(|output| {
            if output.parser_diagnostics_are_valid() {
                Ok(output)
            } else {
                Err(RepositoryServiceError::Diagnostics)
            }
        });
        operation_result(output, MAX_MCP_DIAGNOSTICS_OUTPUT_BYTES)
    }

    async fn call_scip_evidence(
        &self,
        request: ScipEvidenceServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.scip_evidence(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_SCIP_EVIDENCE_OUTPUT_BYTES)
    }
}

include!("server/operation_supervisor.rs");
include!("server/graph.rs");
include!("server/compatibility.rs");

impl ServerHandler for RepoWitnessMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(if self.tasks_enabled {
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tasks()
                .build()
        } else {
            ServerCapabilities::builder().enable_tools().build()
        })
        .with_protocol_version(ProtocolVersion::V_2025_11_25)
        .with_server_info(Implementation::new(
            "repowitness",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(server_instructions(
            self.surface,
            self.memory_writes_enabled,
        ))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: self.tools.iter().cloned().collect(),
            ..ListToolsResult::default()
        })
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the complete versioned tool dispatch is intentionally one auditable closed match over fixed capabilities"
    )]
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        match request.name.as_ref() {
            CODE_SEARCH_TOOL_NAME => {
                let input = parse_arguments::<CodeSearchInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_code_search(request, context).await
            }
            CONTEXT_BUILD_TOOL_NAME => {
                let input = parse_arguments::<ContextBuildInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_context_build(request, context).await
            }
            PHASE2_CONTEXT_BUILD_TOOL_NAME => {
                let input = parse_arguments::<Phase2ContextBuildInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_phase2_context_build(request, context).await
            }
            DIAGNOSTICS_TOOL_NAME => {
                let input = parse_arguments::<DiagnosticsInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_diagnostics(request, context).await
            }
            GRAPH_STATUS_TOOL_NAME => {
                let input = parse_arguments::<GraphStatusInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_graph_status(request, context).await
            }
            GRAPH_SEARCH_TOOL_NAME => {
                let input = parse_arguments::<GraphSearchInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_graph_search(request, context).await
            }
            GRAPH_EVIDENCE_TOOL_NAME => {
                let input = parse_arguments::<GraphEvidenceInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_graph_evidence(request, context).await
            }
            GRAPH_ARCHITECTURE_TOOL_NAME => {
                let input = parse_arguments::<GraphArchitectureInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_graph_architecture(request, context).await
            }
            GRAPH_TRACE_TOOL_NAME => {
                let input = parse_arguments::<GraphTraceInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_graph_trace(request, context).await
            }
            IMPACT_ANALYZE_TOOL_NAME => {
                let input = parse_arguments::<GraphImpactInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_graph_impact(request, context).await
            }
            MEMORY_RECALL_TOOL_NAME => {
                let input = parse_arguments::<MemoryRecallInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_memory_recall(request, context).await
            }
            HISTORICAL_MEMORY_TOOL_NAME => {
                let input = parse_arguments::<HistoricalMemoryInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_historical_memory(request, context).await
            }
            MEMORY_MANAGE_TOOL_NAME if self.memory_writes_enabled => {
                let input = parse_arguments::<MemoryManageInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_memory_manage(request, context).await
            }
            PERSONAL_MEMORY_TOOL_NAME if self.personal_memory_enabled => {
                let input = parse_arguments::<PersonalMemoryInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_personal_memory(request, context).await
            }
            SCIP_EVIDENCE_TOOL_NAME => {
                let input = parse_arguments::<ScipEvidenceInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_scip_evidence(request, context).await
            }
            SYMBOL_GET_TOOL_NAME => {
                let input = parse_arguments::<SymbolGetInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_symbol_get(request, context).await
            }
            _ if self.surface.includes_compatibility_aliases() => {
                self.call_compatibility_tool(request, context).await
            }
            _ => Err(McpError::invalid_params("unknown RepoWitness tool", None)),
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "atomic native-task admission keeps durable creation, registry publication, and cancellation fencing in one reviewable operation"
    )]
    async fn enqueue_task(
        &self,
        mut request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CreateTaskResult, McpError> {
        if !self.tasks_enabled || request.name.as_ref() != PHASE2_CONTEXT_BUILD_TOOL_NAME {
            return Err(McpError::invalid_params(
                "native task invocation is not enabled for this tool",
                None,
            ));
        }
        let objective = parse_arguments::<Phase2ContextBuildInput>(request.arguments.clone())?
            .validate()
            .map_err(|message| McpError::invalid_params(message, None))?
            .intent()
            .to_owned();
        let durable = self.native_task_start(objective, context.clone()).await?;
        let retention = native_task_retention(request.task.as_ref().and_then(|task| task.ttl));
        request.task = None;
        let now = rmcp::task_manager::current_timestamp();
        let mut tasks = self.tasks.lock().await;
        tasks.prune_expired(Instant::now());
        if tasks.entries.len() >= MAX_NATIVE_TASKS {
            return Err(McpError::invalid_params(
                "native task capacity is temporarily exhausted",
                None,
            ));
        }
        let id = durable.task_id().to_owned();
        let task = Task::new(
            id.clone(),
            mcp_task_state(durable.state()),
            now.clone(),
            now,
        )
        .with_ttl(retention.as_millis().try_into().unwrap_or(u64::MAX))
        .with_poll_interval(250);
        tasks.entries.insert(
            id.clone(),
            NativeTask {
                task: task.clone(),
                result: None,
                handle: None,
                expires_at: Instant::now() + retention,
            },
        );
        // Retaining the lock until the handle is recorded makes cancellation atomic with admission.
        let server = self.clone();
        let task_id = id.clone();
        // The response cancels its context, so background work owns another; its join handle handles cancellation.
        let task_context = RequestContext::new(context.id.clone(), context.peer.clone());
        let transition_context = RequestContext::new(context.id.clone(), context.peer.clone());
        let handle = tokio::spawn(async move {
            let result = server.call_tool(request, task_context).await;
            let transition = match &result {
                Ok(result) if bounded_native_task_result(result.clone()).is_some() => {
                    server
                        .native_task_transition(
                            &task_id,
                            NativeTaskState::Completed,
                            transition_context,
                        )
                        .await
                }
                _ => {
                    server
                        .native_task_transition(
                            &task_id,
                            NativeTaskState::Failed,
                            transition_context,
                        )
                        .await
                }
            };
            let mut tasks = server.tasks.lock().await;
            if let Some(entry) = tasks.entries.get_mut(&task_id) {
                // A cancelled task is terminal even if its blocking operation
                // races with the abort request and later returns a result.
                if entry.task.status != TaskStatus::Working {
                    return;
                }
                entry.task.last_updated_at = rmcp::task_manager::current_timestamp();
                match (result, transition) {
                    (Ok(result), Ok(durable)) => match bounded_native_task_result(result) {
                        Some(result) => {
                            entry.task.status = mcp_task_state(durable.state());
                            entry.result = Some(result);
                        }
                        None => {
                            entry.task.status = TaskStatus::Failed;
                            entry.task.status_message =
                                Some("task result exceeded the retained payload limit".to_owned());
                        }
                    },
                    (_, Ok(durable)) => {
                        entry.task.status = mcp_task_state(durable.state());
                        entry.task.status_message = Some("native task execution failed".to_owned());
                    }
                    (_, Err(_)) => {
                        entry.task.status = TaskStatus::Failed;
                        entry.task.status_message =
                            Some("durable task checkpoint failed".to_owned());
                    }
                }
            }
        });
        if let Some(entry) = tasks.entries.get_mut(&id) {
            entry.handle = Some(handle);
        }
        Ok(CreateTaskResult::new(task))
    }

    async fn list_tasks(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListTasksResult, McpError> {
        if !self.tasks_enabled {
            return Err(McpError::method_not_found::<rmcp::model::ListTasksMethod>());
        }
        let cursor = request.and_then(|request| request.cursor);
        let durable = self
            .native_task_list(NATIVE_TASK_PAGE_SIZE as u16 + 1, context)
            .await?;
        let mut page = durable
            .into_iter()
            .filter(|status| {
                cursor
                    .as_deref()
                    .is_none_or(|cursor| status.task_id() > cursor)
            })
            .take(NATIVE_TASK_PAGE_SIZE + 1)
            .map(native_task_as_mcp_task)
            .collect::<Vec<_>>();
        let has_more = page.len() > NATIVE_TASK_PAGE_SIZE;
        if has_more {
            let _ = page.pop();
        }
        let mut result = ListTasksResult::new(page);
        if has_more {
            result.next_cursor = result.tasks.last().map(|task| task.task_id.clone());
        }
        Ok(result)
    }

    async fn get_task_info(
        &self,
        request: GetTaskParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetTaskResult, McpError> {
        if !self.tasks_enabled {
            return Err(McpError::method_not_found::<rmcp::model::GetTaskMethod>());
        }
        self.native_task_status(request.task_id, context)
            .await?
            .map(native_task_as_mcp_task)
            .map(GetTaskResult::new)
            .ok_or_else(|| McpError::invalid_params("unknown task", None))
    }

    async fn get_task_result(
        &self,
        request: GetTaskPayloadParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetTaskPayloadResult, McpError> {
        if !self.tasks_enabled {
            return Err(McpError::method_not_found::<
                rmcp::model::GetTaskPayloadMethod,
            >());
        }
        let mut tasks = self.tasks.lock().await;
        tasks.prune_expired(Instant::now());
        let entry = tasks
            .entries
            .get(&request.task_id)
            .ok_or_else(|| McpError::invalid_params("unknown task", None))?;
        entry
            .result
            .clone()
            .map(GetTaskPayloadResult::new)
            .ok_or_else(|| McpError::invalid_params("task result is not available", None))
    }

    async fn cancel_task(
        &self,
        request: CancelTaskParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CancelTaskResult, McpError> {
        if !self.tasks_enabled {
            return Err(McpError::method_not_found::<rmcp::model::CancelTaskMethod>());
        }
        {
            let mut tasks = self.tasks.lock().await;
            tasks.prune_expired(Instant::now());
            if let Some(entry) = tasks.entries.get_mut(&request.task_id)
                && entry.task.status == TaskStatus::Working
                && let Some(handle) = entry.handle.as_ref()
            {
                handle.abort();
            }
        }
        let durable = self
            .native_task_transition(&request.task_id, NativeTaskState::Cancelled, context)
            .await?;
        let task = native_task_as_mcp_task(durable);
        let mut tasks = self.tasks.lock().await;
        if let Some(entry) = tasks.entries.get_mut(&request.task_id) {
            entry.task = task.clone();
        }
        Ok(CancelTaskResult::new(task))
    }
}

impl RepoWitnessMcpServer {
    async fn native_task_start(
        &self,
        objective: String,
        context: RequestContext<RoleServer>,
    ) -> Result<NativeTaskStatus, McpError> {
        let service = Arc::clone(&self.service);
        self.run_blocking(Duration::from_secs(5), context, move |_, cancelled| {
            service.native_task_start(&objective, cancelled)
        })
        .await?
        .map_err(|_| McpError::internal_error("durable native task admission failed", None))
    }

    async fn native_task_transition(
        &self,
        task_id: &str,
        state: NativeTaskState,
        context: RequestContext<RoleServer>,
    ) -> Result<NativeTaskStatus, McpError> {
        let service = Arc::clone(&self.service);
        let task_id = task_id.to_owned();
        self.run_blocking(Duration::from_secs(5), context, move |_, cancelled| {
            service.native_task_transition(&task_id, state, cancelled)
        })
        .await?
        .map_err(|_| McpError::internal_error("durable native task transition failed", None))
    }

    async fn native_task_status(
        &self,
        task_id: String,
        context: RequestContext<RoleServer>,
    ) -> Result<Option<NativeTaskStatus>, McpError> {
        let service = Arc::clone(&self.service);
        self.run_blocking(Duration::from_secs(5), context, move |_, cancelled| {
            service.native_task_status(&task_id, cancelled)
        })
        .await?
        .map_err(|_| McpError::internal_error("durable native task poll failed", None))
    }

    async fn native_task_list(
        &self,
        limit: u16,
        context: RequestContext<RoleServer>,
    ) -> Result<Box<[NativeTaskStatus]>, McpError> {
        let service = Arc::clone(&self.service);
        self.run_blocking(Duration::from_secs(5), context, move |_, cancelled| {
            service.native_task_list(limit, cancelled)
        })
        .await?
        .map_err(|_| McpError::internal_error("durable native task list failed", None))
    }
}

fn mcp_task_state(state: NativeTaskState) -> TaskStatus {
    match state {
        NativeTaskState::Working => TaskStatus::Working,
        NativeTaskState::Completed => TaskStatus::Completed,
        NativeTaskState::Failed => TaskStatus::Failed,
        NativeTaskState::Cancelled => TaskStatus::Cancelled,
    }
}

fn native_task_as_mcp_task(status: NativeTaskStatus) -> Task {
    let now = rmcp::task_manager::current_timestamp();
    Task::new(
        status.task_id().to_owned(),
        mcp_task_state(status.state()),
        now.clone(),
        now,
    )
    .with_ttl(
        DEFAULT_NATIVE_TASK_TTL
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
    )
    .with_poll_interval(250)
}

/// Serves the bounded local MCP protocol on process stdin/stdout until EOF.
pub async fn serve_stdio(service: Arc<dyn RepositoryService>) -> Result<(), McpServeError> {
    serve_stdio_configured(service, McpToolSurface::NativeV1, false, false, false).await
}

/// Serves local stdio with explicitly authorized memory mutation enabled.
pub async fn serve_stdio_with_memory_writes(
    service: Arc<dyn RepositoryService>,
) -> Result<(), McpServeError> {
    serve_stdio_configured(service, McpToolSurface::NativeV1, true, false, false).await
}

/// Serves local stdio with an explicitly selected fixed tool surface.
pub async fn serve_stdio_with_surface(
    service: Arc<dyn RepositoryService>,
    surface: McpToolSurface,
    memory_writes_enabled: bool,
) -> Result<(), McpServeError> {
    serve_stdio_configured(service, surface, memory_writes_enabled, false, false).await
}

/// Serves local stdio with an explicitly selected surface and native MCP Task
/// support. Callers must opt in; the default transport remains task-free.
pub async fn serve_stdio_with_surface_and_native_tasks(
    service: Arc<dyn RepositoryService>,
    surface: McpToolSurface,
    memory_writes_enabled: bool,
    native_tasks_enabled: bool,
) -> Result<(), McpServeError> {
    serve_stdio_configured(
        service,
        surface,
        memory_writes_enabled,
        native_tasks_enabled,
        false,
    )
    .await
}

/// Serves local stdio with an explicit fixed local personal profile enabled.
///
/// The composition root owns the profile identity and must not accept it from
/// MCP callers. Existing context and recall tools remain team-only.
pub async fn serve_stdio_with_surface_tasks_and_personal_memory(
    service: Arc<dyn RepositoryService>,
    surface: McpToolSurface,
    memory_writes_enabled: bool,
    native_tasks_enabled: bool,
) -> Result<(), McpServeError> {
    serve_stdio_configured(
        service,
        surface,
        memory_writes_enabled,
        native_tasks_enabled,
        true,
    )
    .await
}

async fn serve_stdio_configured(
    service: Arc<dyn RepositoryService>,
    surface: McpToolSurface,
    memory_writes_enabled: bool,
    native_tasks_enabled: bool,
    personal_memory_enabled: bool,
) -> Result<(), McpServeError> {
    let input = BoundedLineReader::try_new(tokio::io::stdin(), MAX_MCP_INPUT_LINE_BYTES)
        .expect("the fixed MCP input-line limit is positive");
    let server = RepoWitnessMcpServer::configured_with_tasks(
        service,
        DEFAULT_MCP_OPERATION_CONCURRENCY,
        memory_writes_enabled,
        personal_memory_enabled,
        surface,
        native_tasks_enabled,
    );
    let running = server
        .serve((input, tokio::io::stdout()))
        .await
        .map_err(|_| McpServeError::Initialize)?;
    running
        .waiting()
        .await
        .map_err(|_| McpServeError::Runtime)?;
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "one fixed capability constructor makes the complete advertised MCP surface directly auditable"
)]
fn tools(
    memory_writes_enabled: bool,
    personal_memory_enabled: bool,
    surface: McpToolSurface,
    tasks_enabled: bool,
) -> Vec<Tool> {
    let annotations = ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(false);
    let code_search = Tool::new(
        CODE_SEARCH_TOOL_NAME,
        "Search the active Rust/Go/TypeScript/TSX/Python index with bounded literal terms and return attributed, \
         generation-pinned symbol evidence.",
        JsonObject::new(),
    )
    .with_input_schema::<CodeSearchInput>()
    .with_output_schema::<CodeSearchOutput>()
    .annotate(annotations.clone());
    let context_build = Tool::new(
        CONTEXT_BUILD_TOOL_NAME,
        "Compile a deterministic generation-pinned context pack from exact source declarations \
         and current engineering memory under a labeled conservative content budget.",
        JsonObject::new(),
    )
    .with_input_schema::<ContextBuildInput>()
    .with_output_schema::<ContextBuildOutput>()
    .annotate(annotations.clone());
    let phase2_context_build = Tool::new(
        PHASE2_CONTEXT_BUILD_TOOL_NAME,
        "Compile the separately versioned evidence-balanced Phase 2 context pack from pinned \
         exact source and current-memory evidence under a labeled conservative content budget.",
        JsonObject::new(),
    )
    .with_input_schema::<Phase2ContextBuildInput>()
    .with_output_schema::<Phase2ContextBuildOutput>()
    .annotate(annotations.clone());
    let phase2_context_build = if tasks_enabled {
        phase2_context_build
            .with_execution(ToolExecution::new().with_task_support(TaskSupport::Optional))
    } else {
        phase2_context_build
    };
    let diagnostics = Tool::new(
        DIAGNOSTICS_TOOL_NAME,
        "Inspect the exact active source generation, optional matching memory projection, \
         coverage, implemented capabilities, and explicit Phase 0 limitations.",
        JsonObject::new(),
    )
    .with_input_schema::<DiagnosticsInput>()
    .with_output_schema::<DiagnosticsOutput>()
    .annotate(annotations.clone());
    let symbol_get = Tool::new(
        SYMBOL_GET_TOOL_NAME,
        "Retrieve one exact verified supported-language declaration selected from code_search output; stale \
         generations or changed source fail instead of retargeting.",
        JsonObject::new(),
    )
    .with_input_schema::<SymbolGetInput>()
    .with_output_schema::<SymbolGetOutput>()
    .annotate(annotations.clone());
    let memory_recall = Tool::new(
        MEMORY_RECALL_TOOL_NAME,
        "Recall bounded engineering memories from the complete active source projection with \
         conflicts, freshness, correspondence evidence, and projection coverage.",
        JsonObject::new(),
    )
    .with_input_schema::<MemoryRecallInput>()
    .with_output_schema::<MemoryRecallOutput>()
    .annotate(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    );
    let mut tools = vec![
        code_search,
        context_build,
        phase2_context_build,
        diagnostics,
        memory_recall,
        Tool::new(
            HISTORICAL_MEMORY_TOOL_NAME,
            "Read bounded path-free historical memory applicability for one exact Git object or retained source snapshot.",
            JsonObject::new(),
        )
        .with_input_schema::<HistoricalMemoryInput>()
        .with_output_schema::<HistoricalMemoryOutput>()
        .annotate(annotations.clone()),
        symbol_get,
        Tool::new(
            SCIP_EVIDENCE_TOOL_NAME,
            "Read bounded exact package-scoped evidence for one imported SCIP symbol from the active or selected immutable overlay.",
            JsonObject::new(),
        )
        .with_input_schema::<ScipEvidenceInput>()
        .with_output_schema::<ScipEvidenceOutput>()
        .annotate(annotations.clone()),
    ];
    tools.extend(graph_tools(&annotations));
    if surface.includes_compatibility_aliases() {
        tools.extend(compatibility_tools(&annotations));
    }
    if memory_writes_enabled {
        let memory_manage = Tool::new(
            MEMORY_MANAGE_TOOL_NAME,
            "Perform one explicitly authorized, path-confined memory write, approval, \
             correspondence review, or observation-only reachable-history import. The \
             local actor and repository capability are fixed at server startup.",
            JsonObject::new(),
        )
        .with_input_schema::<MemoryManageInput>()
        .with_output_schema::<MemoryManageOutput>()
        .annotate(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(true)
                .idempotent(false)
                .open_world(false),
        );
        tools.push(memory_manage);
    }
    if personal_memory_enabled {
        let personal_memory = Tool::new(
            PERSONAL_MEMORY_TOOL_NAME,
            "Read or append bounded local-only personal memory for the one opaque profile fixed at server startup. This tool is absent unless that profile is explicitly enabled; ordinary context and memory tools remain team-only.",
            JsonObject::new(),
        )
        .with_input_schema::<PersonalMemoryInput>()
        .with_output_schema::<PersonalMemoryOutput>()
        .annotate(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(false)
                .idempotent(false)
                .open_world(false),
        );
        tools.push(personal_memory);
    }
    tools.sort_by(|left, right| left.name.as_ref().cmp(right.name.as_ref()));
    tools
}

fn parse_arguments<T: DeserializeOwned>(arguments: Option<JsonObject>) -> Result<T, McpError> {
    serde_json::from_value(serde_json::Value::Object(arguments.unwrap_or_default()))
        .map_err(|_| McpError::invalid_params("tool arguments do not match the schema", None))
}

fn operation_result<T: Serialize>(
    result: Result<T, RepositoryServiceError>,
    output_limit: usize,
) -> Result<CallToolResult, McpError> {
    match result {
        Ok(output) => {
            let value = serde_json::to_value(output)
                .map_err(|_| McpError::internal_error("tool output serialization failed", None))?;
            let result = CallToolResult::structured(value);
            let encoded = serde_json::to_vec(&result)
                .map_err(|_| McpError::internal_error("tool output serialization failed", None))?;
            if encoded.len() > output_limit {
                return Ok(tool_error("tool output exceeded its byte limit"));
            }
            Ok(result)
        }
        Err(error) => Ok(tool_error(error.to_string())),
    }
}

async fn acquire_permit<F>(
    semaphore: Arc<Semaphore>,
    deadline: Instant,
    cancelled: F,
) -> Result<OwnedSemaphorePermit, McpError>
where
    F: Future<Output = ()>,
{
    tokio::pin!(cancelled);
    tokio::select! {
        permit = semaphore.acquire_owned() => {
            permit.map_err(|_| McpError::internal_error("operation supervisor is unavailable", None))
        }
        () = &mut cancelled => Err(cancelled_error()),
        () = tokio::time::sleep_until(deadline) => Err(deadline_error()),
    }
}

fn tool_error(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message.into())])
}

fn cancelled_error() -> McpError {
    McpError::internal_error("request cancelled", None)
}

fn deadline_error() -> McpError {
    McpError::internal_error("request deadline exceeded", None)
}

#[cfg(test)]
mod tests;
