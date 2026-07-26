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
    BoundedLineReader, CODE_SEARCH_TOOL_NAME, CodeSearchInput, CodeSearchOutput,
    CodeSearchServiceRequest, MAX_MCP_INPUT_LINE_BYTES, RepositoryService, RepositoryServiceError,
    SYMBOL_GET_TOOL_NAME, SymbolGetInput, SymbolGetOutput, SymbolGetServiceRequest,
    wire::{MAX_MCP_SEARCH_OUTPUT_BYTES, MAX_MCP_SYMBOL_OUTPUT_BYTES},
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

/// Bounded read-only MCP server over an injected repository service.
#[derive(Clone)]
pub struct RepoWitnessMcpServer {
    service: Arc<dyn RepositoryService>,
    operations: Arc<Semaphore>,
    tools: Arc<[Tool]>,
}

impl fmt::Debug for RepoWitnessMcpServer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepoWitnessMcpServer")
            .field("service", &"<injected-repository-service>")
            .field("available_permits", &self.operations.available_permits())
            .field("tool_count", &self.tools.len())
            .finish()
    }
}

impl RepoWitnessMcpServer {
    /// Constructs the default bounded Phase 0 MCP server.
    #[must_use]
    pub fn new(service: Arc<dyn RepositoryService>) -> Self {
        Self::with_operation_concurrency(service, DEFAULT_MCP_OPERATION_CONCURRENCY)
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
        assert!(
            operation_concurrency > 0,
            "MCP operation concurrency must be positive"
        );
        Self {
            service,
            operations: Arc::new(Semaphore::new(operation_concurrency)),
            tools: Arc::from(tools()),
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
            .with_instructions(
                "Use code_search first, then pass its complete exact selector to symbol_get. \
                 Results are generation-pinned and evidence-bearing.",
            )
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
    let input = BoundedLineReader::try_new(tokio::io::stdin(), MAX_MCP_INPUT_LINE_BYTES)
        .expect("the fixed MCP input-line limit is positive");
    let running = RepoWitnessMcpServer::new(service)
        .serve((input, tokio::io::stdout()))
        .await
        .map_err(|_| McpServeError::Initialize)?;
    running
        .waiting()
        .await
        .map_err(|_| McpServeError::Runtime)?;
    Ok(())
}

fn tools() -> Vec<Tool> {
    let annotations = ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(false);
    let code_search = Tool::new(
        CODE_SEARCH_TOOL_NAME,
        "Search the active Rust index with bounded literal terms and return attributed, \
         generation-pinned symbol evidence.",
        JsonObject::new(),
    )
    .with_input_schema::<CodeSearchInput>()
    .with_output_schema::<CodeSearchOutput>()
    .annotate(annotations.clone());
    let symbol_get = Tool::new(
        SYMBOL_GET_TOOL_NAME,
        "Retrieve one exact verified Rust declaration selected from code_search output; stale \
         generations or changed source fail instead of retargeting.",
        JsonObject::new(),
    )
    .with_input_schema::<SymbolGetInput>()
    .with_output_schema::<SymbolGetOutput>()
    .annotate(annotations);
    vec![code_search, symbol_get]
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
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use rmcp::{
        ServiceExt,
        model::{CallToolRequestParams, JsonObject},
    };
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    use super::*;
    use crate::{McpCoverage, McpSearchMatch, McpSpan, McpSymbol, SymbolSelectorOutput};

    struct FakeService {
        search_calls: AtomicUsize,
        symbol_calls: AtomicUsize,
        search_request: Mutex<Option<(String, u16)>>,
    }

    struct ConcurrencyService {
        active: AtomicUsize,
        maximum: AtomicUsize,
    }

    struct CancellationService {
        started: AtomicBool,
        observed: AtomicBool,
    }

    impl RepositoryService for CancellationService {
        fn code_search(
            &self,
            _request: CodeSearchServiceRequest,
            cancelled: Arc<AtomicBool>,
        ) -> Result<CodeSearchOutput, RepositoryServiceError> {
            self.started.store(true, Ordering::Release);
            while !cancelled.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            self.observed.store(true, Ordering::Release);
            Err(RepositoryServiceError::CodeSearch)
        }

        fn symbol_get(
            &self,
            _request: SymbolGetServiceRequest,
            _cancelled: Arc<AtomicBool>,
        ) -> Result<SymbolGetOutput, RepositoryServiceError> {
            Err(RepositoryServiceError::SymbolGet)
        }
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
    }

    impl FakeService {
        fn new() -> Self {
            Self {
                search_calls: AtomicUsize::new(0),
                symbol_calls: AtomicUsize::new(0),
                search_request: Mutex::new(None),
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
    }

    #[test]
    fn tool_contract_is_exact_sorted_versioned_and_read_only() {
        let server = RepoWitnessMcpServer::new(Arc::new(FakeService::new()));
        assert_eq!(
            server
                .tools
                .iter()
                .map(|tool| tool.name.as_ref())
                .collect::<Vec<_>>(),
            [CODE_SEARCH_TOOL_NAME, SYMBOL_GET_TOOL_NAME]
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
    async fn initialized_client_lists_and_calls_both_tools() {
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
            [CODE_SEARCH_TOOL_NAME, SYMBOL_GET_TOOL_NAME]
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
        assert_eq!(service.search_calls.load(Ordering::Relaxed), 1);
        assert_eq!(service.symbol_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            service.search_request.lock().expect("lock").as_ref(),
            Some(&("run".to_owned(), 7))
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn protocol_cancellation_reaches_blocking_work_and_suppresses_its_response() {
        let service = Arc::new(CancellationService {
            started: AtomicBool::new(false),
            observed: AtomicBool::new(false),
        });
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
        let (client_read, mut client_write) = tokio::io::split(client_transport);
        let mut client_read = BufReader::new(client_read);

        send_json(
            &mut client_write,
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": {"name": "cancellation-test", "version": "1"}
                }
            }),
        )
        .await;
        assert_eq!(
            read_json(&mut client_read).await["id"],
            serde_json::json!(1)
        );
        send_json(
            &mut client_write,
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            }),
        )
        .await;
        send_json(
            &mut client_write,
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "code_search",
                    "arguments": {"query": "run", "timeout_ms": 10000}
                }
            }),
        )
        .await;
        tokio::time::timeout(Duration::from_secs(1), async {
            while !service.started.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("service starts");
        send_json(
            &mut client_write,
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/cancelled",
                "params": {"requestId": 2, "reason": "test cancellation"}
            }),
        )
        .await;
        send_json(
            &mut client_write,
            serde_json::json!({"jsonrpc": "2.0", "id": 3, "method": "ping"}),
        )
        .await;

        let response = read_json(&mut client_read).await;
        assert_eq!(response["id"], serde_json::json!(3));
        tokio::time::timeout(Duration::from_secs(1), async {
            while !service.observed.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("blocking service observes cancellation");
        assert!(
            tokio::time::timeout(Duration::from_millis(50), read_json(&mut client_read))
                .await
                .is_err(),
            "cancelled request must not produce a response"
        );
        drop(client_write);
        drop(client_read);
        server_task.await.expect("server task");
    }

    async fn send_json<W: tokio::io::AsyncWrite + Unpin>(writer: &mut W, value: serde_json::Value) {
        let encoded = serde_json::to_vec(&value).expect("JSON encodes");
        writer.write_all(&encoded).await.expect("message writes");
        writer.write_all(b"\n").await.expect("delimiter writes");
        writer.flush().await.expect("message flushes");
    }

    async fn read_json<R: tokio::io::AsyncRead + Unpin>(
        reader: &mut BufReader<R>,
    ) -> serde_json::Value {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).await.expect("response reads");
        assert!(bytes > 0, "server closed before responding");
        serde_json::from_str(&line).expect("response is JSON")
    }

    fn json_object(value: serde_json::Value) -> JsonObject {
        value.as_object().expect("fixture is an object").clone()
    }

    fn coverage() -> McpCoverage {
        McpCoverage {
            searched: 1,
            skipped: 0,
            unresolved: 0,
            truncated: 0,
        }
    }

    fn search_output() -> CodeSearchOutput {
        CodeSearchOutput {
            schema_version: 1,
            query_profile: 1,
            snapshot_sha256: "11".repeat(32),
            generation: 9,
            resolution: "confirmed".to_owned(),
            query_sha256: "44".repeat(32),
            matches_returned: 1,
            matches_total: 1,
            coverage: coverage(),
            limitation: "rust_symbol_lexical_only".to_owned(),
            matches: vec![McpSearchMatch {
                path: "rwp1:h:7372632F6C69622E7273".to_owned(),
                fact_ordinal: 7,
                content_sha256: "22".repeat(32),
                artifact_sha256: "33".repeat(32),
                producer_manifest_sha256: "55".repeat(32),
                evidence_tier: "syntax".to_owned(),
                kind: "function".to_owned(),
                name: "run".to_owned(),
                qualified_name: "fixture::run".to_owned(),
                name_span: McpSpan { start: 7, end: 10 },
                declaration_span: McpSpan { start: 0, end: 13 },
            }],
        }
    }

    fn symbol_output() -> SymbolGetOutput {
        SymbolGetOutput {
            schema_version: 1,
            symbol_profile: 1,
            snapshot_sha256: "11".repeat(32),
            generation: 9,
            resolution: "confirmed".to_owned(),
            selector: SymbolSelectorOutput {
                path: "rwp1:h:7372632F6C69622E7273".to_owned(),
                content_sha256: "22".repeat(32),
                artifact_sha256: "33".repeat(32),
                fact_ordinal: 7,
            },
            coverage: coverage(),
            limitation: "references_not_implemented".to_owned(),
            symbol: Some(McpSymbol {
                producer_manifest_sha256: "55".repeat(32),
                evidence_tier: "syntax".to_owned(),
                kind: "function".to_owned(),
                name: "run".to_owned(),
                qualified_name: "fixture::run".to_owned(),
                name_span: McpSpan { start: 7, end: 10 },
                declaration_span: McpSpan { start: 0, end: 13 },
                declaration_encoding: "lowercase_hex".to_owned(),
                declaration_hex: "70756220666e2072756e2829207b7d".to_owned(),
            }),
        }
    }
}
