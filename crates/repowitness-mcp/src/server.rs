use std::{
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
        CallToolRequestParams, CallToolResult, ContentBlock, Implementation, JsonObject,
        ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerInfo,
        Tool, ToolAnnotations,
    },
    service::RequestContext,
};
use serde::{Serialize, de::DeserializeOwned};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
    time::Instant,
};

use crate::{
    BoundedLineReader, CODE_SEARCH_TOOL_NAME, CONTEXT_BUILD_TOOL_NAME, CodeSearchInput,
    CodeSearchOutput, CodeSearchServiceRequest, ContextBuildInput, ContextBuildOutput,
    ContextBuildServiceRequest, DIAGNOSTICS_TOOL_NAME, DiagnosticsInput, DiagnosticsOutput,
    DiagnosticsServiceRequest, MAX_MCP_INPUT_LINE_BYTES, MEMORY_MANAGE_TOOL_NAME,
    MEMORY_RECALL_TOOL_NAME, MemoryManageInput, MemoryManageOutput, MemoryManageServiceRequest,
    MemoryRecallInput, MemoryRecallOutput, MemoryRecallServiceRequest, RepositoryService,
    RepositoryServiceError, SYMBOL_GET_TOOL_NAME, SymbolGetInput, SymbolGetOutput,
    SymbolGetServiceRequest,
    wire::{
        MAX_MCP_CONTEXT_OUTPUT_BYTES, MAX_MCP_DIAGNOSTICS_OUTPUT_BYTES,
        MAX_MCP_MEMORY_MANAGE_OUTPUT_BYTES, MAX_MCP_MEMORY_RECALL_OUTPUT_BYTES,
        MAX_MCP_SEARCH_OUTPUT_BYTES, MAX_MCP_SYMBOL_OUTPUT_BYTES,
    },
};

/// Maximum synchronous repository operations admitted concurrently by default.
pub const DEFAULT_MCP_OPERATION_CONCURRENCY: usize = 4;

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
}

impl fmt::Debug for RepoWitnessMcpServer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepoWitnessMcpServer")
            .field("service", &"<injected-repository-service>")
            .field("available_permits", &self.operations.available_permits())
            .field("tool_count", &self.tools.len())
            .field("memory_writes_enabled", &self.memory_writes_enabled)
            .finish()
    }
}

impl RepoWitnessMcpServer {
    /// Constructs the default bounded Phase 0 MCP server.
    #[must_use]
    pub fn new(service: Arc<dyn RepositoryService>) -> Self {
        Self::with_operation_concurrency(service, DEFAULT_MCP_OPERATION_CONCURRENCY)
    }

    /// Constructs a bounded server with the local memory-mutation tool enabled.
    #[must_use]
    pub fn with_memory_writes(service: Arc<dyn RepositoryService>) -> Self {
        Self::configured(service, DEFAULT_MCP_OPERATION_CONCURRENCY, true)
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
        Self::configured(service, operation_concurrency, false)
    }

    fn configured(
        service: Arc<dyn RepositoryService>,
        operation_concurrency: usize,
        memory_writes_enabled: bool,
    ) -> Self {
        assert!(
            operation_concurrency > 0,
            "MCP operation concurrency must be positive"
        );
        Self {
            service,
            operations: Arc::new(Semaphore::new(operation_concurrency)),
            tools: Arc::from(tools(memory_writes_enabled)),
            memory_writes_enabled,
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
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.memory_manage(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_MEMORY_MANAGE_OUTPUT_BYTES)
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

    async fn run_blocking<T, F>(
        &self,
        timeout: Duration,
        context: RequestContext<RoleServer>,
        operation: F,
    ) -> Result<Result<T, RepositoryServiceError>, McpError>
    where
        T: Send + 'static,
        F: FnOnce(Duration, Arc<AtomicBool>) -> Result<T, RepositoryServiceError> + Send + 'static,
    {
        let deadline = Instant::now() + timeout;
        let permit = acquire_permit(
            Arc::clone(&self.operations),
            deadline,
            context.ct.cancelled(),
        )
        .await?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(deadline_error)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let task_cancelled = Arc::clone(&cancelled);
        let mut task = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            operation(remaining, task_cancelled)
        });

        tokio::select! {
            result = &mut task => join_result(result),
            () = context.ct.cancelled() => {
                cancelled.store(true, Ordering::Release);
                await_cancelled_task(task).await;
                Err(cancelled_error())
            }
            () = tokio::time::sleep_until(deadline) => {
                cancelled.store(true, Ordering::Release);
                await_cancelled_task(task).await;
                Err(deadline_error())
            }
        }
    }
}

impl ServerHandler for RepoWitnessMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_11_25)
            .with_server_info(Implementation::new(
                "repowitness",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(if self.memory_writes_enabled {
                "Use context_build for one deterministic budgeted source-and-memory pack. \
                     Use code_search first when selecting an exact occurrence for symbol_get. \
                     Use memory_recall to inspect projected records including non-current states. \
                     The operator explicitly enabled memory_manage with one fixed local actor; \
                     inspect exact evidence before mutation. Results are generation-pinned and \
                     evidence-bearing."
            } else {
                "Use context_build for one deterministic budgeted source-and-memory pack. \
                     Use code_search first when selecting an exact occurrence for symbol_get. \
                     Use memory_recall to inspect projected records including non-current states. \
                     Use diagnostics to inspect active coverage, capabilities, and limitations. \
                     Results are generation-pinned and evidence-bearing."
            })
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
            DIAGNOSTICS_TOOL_NAME => {
                let input = parse_arguments::<DiagnosticsInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_diagnostics(request, context).await
            }
            MEMORY_RECALL_TOOL_NAME => {
                let input = parse_arguments::<MemoryRecallInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_memory_recall(request, context).await
            }
            MEMORY_MANAGE_TOOL_NAME if self.memory_writes_enabled => {
                let input = parse_arguments::<MemoryManageInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_memory_manage(request, context).await
            }
            SYMBOL_GET_TOOL_NAME => {
                let input = parse_arguments::<SymbolGetInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_symbol_get(request, context).await
            }
            _ => Err(McpError::invalid_params("unknown RepoWitness tool", None)),
        }
    }
}

/// Serves the bounded local MCP protocol on process stdin/stdout until EOF.
pub async fn serve_stdio(service: Arc<dyn RepositoryService>) -> Result<(), McpServeError> {
    serve_stdio_configured(service, false).await
}

/// Serves local stdio with explicitly authorized memory mutation enabled.
pub async fn serve_stdio_with_memory_writes(
    service: Arc<dyn RepositoryService>,
) -> Result<(), McpServeError> {
    serve_stdio_configured(service, true).await
}

async fn serve_stdio_configured(
    service: Arc<dyn RepositoryService>,
    memory_writes_enabled: bool,
) -> Result<(), McpServeError> {
    let input = BoundedLineReader::try_new(tokio::io::stdin(), MAX_MCP_INPUT_LINE_BYTES)
        .expect("the fixed MCP input-line limit is positive");
    let server = if memory_writes_enabled {
        RepoWitnessMcpServer::with_memory_writes(service)
    } else {
        RepoWitnessMcpServer::new(service)
    };
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

fn tools(memory_writes_enabled: bool) -> Vec<Tool> {
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
    .annotate(annotations);
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
        diagnostics,
        memory_recall,
        symbol_get,
    ];
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
        tools.insert(3, memory_manage);
    }
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

fn join_result<T>(
    result: Result<Result<T, RepositoryServiceError>, tokio::task::JoinError>,
) -> Result<Result<T, RepositoryServiceError>, McpError> {
    result.map_err(|_| McpError::internal_error("repository operation task failed", None))
}

async fn await_cancelled_task<T>(task: JoinHandle<Result<T, RepositoryServiceError>>) {
    let _ = task.await;
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
