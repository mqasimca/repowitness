use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    future::Future,
    sync::{
        Arc, RwLock,
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
    sync::{Mutex, OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
    task::JoinSet,
    time::Instant,
};

use crate::{
    ARCHITECTURE_MAP_TOOL_NAME, ARCHITECTURE_OVERVIEW_TOOL_NAME, ArchitectureMapInput,
    ArchitectureMapOutput, ArchitectureMapServiceRequest, ArchitectureOverviewInput,
    ArchitectureOverviewOutput, ArchitectureOverviewServiceRequest, BoundedLineReader,
    CHANGE_REVIEW_TOOL_NAME, CODE_GRAPH_QUERY_TOOL_NAME, CODE_SEARCH_TOOL_NAME,
    CONTEXT_BUILD_TOOL_NAME, CROSS_REPOSITORY_SEARCH_TOOL_NAME, ChangeReviewInput,
    ChangeReviewOutput, ChangeReviewServiceRequest, CodeGraphQueryInput, CodeGraphQueryOutput,
    CodeGraphQueryServiceRequest, CodeSearchInput, CodeSearchOutput, CodeSearchServiceRequest,
    CrossRepositorySearchInput, CrossRepositorySearchOutput, CrossRepositorySearchRepository,
    CrossRepositorySearchServiceRequest, DIAGNOSTICS_TOOL_NAME, DiagnosticsInput,
    DiagnosticsOutput, DiagnosticsServiceRequest, EvidenceContextBuildInput,
    EvidenceContextBuildOutput, EvidenceContextBuildServiceRequest, GRAPH_ARCHITECTURE_TOOL_NAME,
    GRAPH_EVIDENCE_TOOL_NAME, GRAPH_SEARCH_TOOL_NAME, GRAPH_STATUS_TOOL_NAME,
    GRAPH_TRACE_TOOL_NAME, GraphArchitectureInput, GraphArchitectureOutput, GraphEvidenceInput,
    GraphEvidenceOutput, GraphImpactInput, GraphImpactOutput, GraphReadServiceOutput,
    GraphReadServiceRequest, GraphSearchInput, GraphSearchOutput, GraphStatusInput,
    GraphStatusOutput, GraphTraceInput, GraphTraceOutput, IMPACT_ANALYZE_TOOL_NAME,
    MAX_CROSS_REPOSITORY_RESULTS, MAX_MCP_INPUT_LINE_BYTES, MEMORY_MANAGE_TOOL_NAME,
    MEMORY_RECALL_TOOL_NAME, McpCoverage, MemoryManageInput, MemoryManageOutput,
    MemoryManageServiceRequest, MemoryMutationRequestScope, MemoryRecallInput, MemoryRecallOutput,
    MemoryRecallServiceRequest, OUTBOUND_SITES_TOOL_NAME, OutboundSitesInput, OutboundSitesOutput,
    OutboundSitesServiceRequest, RELEVANT_PATHS_TOOL_NAME, REPOSITORY_TOPOLOGY_TOOL_NAME,
    RelevantPathsInput, RelevantPathsOutput, RelevantPathsServiceRequest, RepositoryService,
    RepositoryServiceError, RepositoryTopologyInput, RepositoryTopologyOutput,
    RepositoryTopologyServiceRequest, SYMBOL_GET_TOOL_NAME, SYMBOL_SEARCH_TOOL_NAME,
    SYNTAX_SITE_SEARCH_TOOL_NAME, SymbolGetInput, SymbolGetOutput, SymbolGetServiceRequest,
    SymbolSearchInput, SymbolSearchOutput, SymbolSearchServiceRequest, SyntaxSiteSearchInput,
    SyntaxSiteSearchOutput, SyntaxSiteSearchServiceRequest,
    wire::{
        MAX_MCP_ARCHITECTURE_MAP_OUTPUT_BYTES, MAX_MCP_ARCHITECTURE_OVERVIEW_OUTPUT_BYTES,
        MAX_MCP_CODE_GRAPH_QUERY_OUTPUT_BYTES, MAX_MCP_CONTEXT_OUTPUT_BYTES,
        MAX_MCP_DIAGNOSTICS_OUTPUT_BYTES, MAX_MCP_EVIDENCE_CONTEXT_OUTPUT_BYTES,
        MAX_MCP_GRAPH_OUTPUT_BYTES, MAX_MCP_MEMORY_MANAGE_OUTPUT_BYTES,
        MAX_MCP_MEMORY_RECALL_OUTPUT_BYTES, MAX_MCP_OUTBOUND_SITES_OUTPUT_BYTES,
        MAX_MCP_RELEVANT_PATHS_OUTPUT_BYTES, MAX_MCP_REPOSITORY_TOPOLOGY_OUTPUT_BYTES,
        MAX_MCP_SEARCH_OUTPUT_BYTES, MAX_MCP_SYMBOL_OUTPUT_BYTES,
        MAX_MCP_SYNTAX_SITE_SEARCH_OUTPUT_BYTES,
    },
};

/// Maximum synchronous repository operations admitted concurrently by default.
pub const DEFAULT_MCP_OPERATION_CONCURRENCY: usize = 4;
/// Maximum repositories admitted by one local MCP process.
pub const MAX_MCP_REGISTERED_REPOSITORIES: usize = 32;

fn code_graph_query_input_schema() -> JsonObject {
    let mut union = serde_json::to_value(schemars::schema_for!(CodeGraphQueryInput))
        .expect("the closed code graph query schema must serialize");
    let union = union
        .as_object_mut()
        .expect("schemars must produce an object-root schema");
    let schema_draft = union.remove("$schema");
    let definitions = union.remove("$defs");

    let mut properties = JsonObject::new();
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
        properties.insert(field.to_owned(), serde_json::Value::Bool(true));
    }

    let mut schema = JsonObject::new();
    if let Some(schema_draft) = schema_draft {
        schema.insert("$schema".to_owned(), schema_draft);
    }
    if let Some(definitions) = definitions {
        schema.insert("$defs".to_owned(), definitions);
    }
    schema.insert(
        "type".to_owned(),
        serde_json::Value::String("object".to_owned()),
    );
    schema.insert(
        "additionalProperties".to_owned(),
        serde_json::Value::Bool(false),
    );
    schema.insert(
        "properties".to_owned(),
        serde_json::Value::Object(properties),
    );
    schema.insert(
        "required".to_owned(),
        serde_json::Value::Array(vec![serde_json::Value::String("operation".to_owned())]),
    );
    schema.insert(
        "allOf".to_owned(),
        serde_json::Value::Array(vec![serde_json::Value::Object(union.clone())]),
    );
    schema
}

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

/// Bounded repository services keyed by opaque catalog identity.
pub type McpRepositoryCatalog = BTreeMap<String, Arc<dyn RepositoryService>>;

/// Bounded loader used by a reloadable local MCP catalog.
pub type McpRepositoryCatalogLoader =
    Arc<dyn Fn() -> Result<(McpRepositoryCatalog, Option<String>), ()> + Send + Sync>;

#[derive(Clone)]
struct CatalogState {
    registry: Arc<McpRepositoryCatalog>,
    default_repository_id: Option<Arc<str>>,
}

impl CatalogState {
    fn new(
        registry: McpRepositoryCatalog,
        default_repository_id: Option<String>,
    ) -> Result<Self, McpRepositoryRegistryError> {
        if registry.is_empty() {
            return Err(McpRepositoryRegistryError::Empty);
        }
        if registry.len() > MAX_MCP_REGISTERED_REPOSITORIES {
            return Err(McpRepositoryRegistryError::TooMany);
        }
        if let Some(default) = default_repository_id.as_deref()
            && !registry.contains_key(default)
        {
            return Err(McpRepositoryRegistryError::DefaultMissing);
        }
        Ok(Self {
            registry: Arc::new(registry),
            default_repository_id: default_repository_id.as_deref().map(Arc::from),
        })
    }
}

struct CatalogRuntime {
    state: RwLock<CatalogState>,
    loader: Option<McpRepositoryCatalogLoader>,
    refresh: Mutex<()>,
}

impl CatalogRuntime {
    fn new(
        registry: McpRepositoryCatalog,
        default_repository_id: Option<String>,
        loader: Option<McpRepositoryCatalogLoader>,
    ) -> Result<Self, McpRepositoryRegistryError> {
        Ok(Self {
            state: RwLock::new(CatalogState::new(registry, default_repository_id)?),
            loader,
            refresh: Mutex::new(()),
        })
    }
}

#[derive(Clone)]
/// Bounded MCP server over an injected path-confined repository service.
pub struct RepoWitnessMcpServer {
    service: Arc<dyn RepositoryService>,
    catalog: Option<Arc<CatalogRuntime>>,
    operations: Arc<Semaphore>,
    tools: Arc<[Tool]>,
    memory_writes_enabled: bool,
}

impl fmt::Debug for RepoWitnessMcpServer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepoWitnessMcpServer")
            .field("service", &"<injected-repository-service>")
            .field(
                "registered_repository_count",
                &self
                    .catalog
                    .as_ref()
                    .and_then(|catalog| catalog.state.read().ok().map(|state| state.registry.len()))
                    .unwrap_or(0),
            )
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
        Self::configured(service, DEFAULT_MCP_OPERATION_CONCURRENCY, false)
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
            catalog: None,
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
        let output = self.run_code_search(request, context).await?;
        operation_result(output, MAX_MCP_SEARCH_OUTPUT_BYTES)
    }

    async fn run_code_search(
        &self,
        request: CodeSearchServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<Result<CodeSearchOutput, RepositoryServiceError>, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        self.run_blocking(timeout, context, move |remaining, cancelled| {
            service.code_search(request.with_timeout(remaining), cancelled)
        })
        .await
    }

    async fn call_cross_repository_search(
        &self,
        request: CrossRepositorySearchServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let registry = self.catalog_snapshot()?.ok_or_else(|| {
            McpError::invalid_params("cross_repository_search requires catalog mode", None)
        })?;
        let mut repository_ids = request.repository_ids().map_or_else(
            || registry.registry.keys().cloned().collect(),
            ToOwned::to_owned,
        );
        repository_ids.sort();
        let mut jobs = JoinSet::new();
        for repository_id in repository_ids {
            let service = registry
                .registry
                .get(&repository_id)
                .cloned()
                .ok_or_else(|| {
                    McpError::invalid_params(
                        "repository_ids contains an unregistered repository",
                        None,
                    )
                })?;
            let selected = self.with_selected_service(service);
            let search = request.clone().code_search_request();
            let job_context = context.clone();
            jobs.spawn(async move {
                (
                    repository_id,
                    selected.run_code_search(search, job_context).await,
                )
            });
        }

        let mut results = Vec::new();
        while let Some(job) = jobs.join_next().await {
            let (repository_id, result) = job.map_err(|_| {
                McpError::internal_error("cross-repository search task failed", None)
            })?;
            results.push(CrossRepositorySearchRepository {
                repository_id,
                status: if matches!(result, Ok(Ok(_))) {
                    "complete".to_owned()
                } else {
                    "unavailable".to_owned()
                },
                result: result.ok().and_then(Result::ok),
            });
        }
        results.sort_by(|left, right| left.repository_id.cmp(&right.repository_id));
        let mut matches_returned = 0_u64;
        let mut matches_total = 0_u64;
        let mut completed = 0_u64;
        let mut failed = 0_u64;
        let mut truncated = 0_u64;
        for repository in &mut results {
            let Some(result) = repository.result.as_mut() else {
                failed += 1;
                continue;
            };
            completed += 1;
            truncated = truncated.saturating_add(result.coverage.truncated);
            matches_total = matches_total.saturating_add(result.matches_total);
            let remaining = u64::from(request.max_results())
                .saturating_sub(matches_returned)
                .min(u64::from(MAX_CROSS_REPOSITORY_RESULTS));
            if result.matches.len() as u64 > remaining {
                truncated = truncated
                    .saturating_add((result.matches.len() as u64).saturating_sub(remaining));
                result.matches.truncate(remaining as usize);
                result.matches_returned = result.matches.len() as u64;
                result.coverage.truncated = result.coverage.truncated.saturating_add(1);
                result.limitation.push_str(";cross_repository_result_bound");
                truncated += 1;
            }
            matches_returned = matches_returned.saturating_add(result.matches_returned);
        }
        let resolution = if failed > 0 || truncated > 0 {
            "partial"
        } else {
            "complete"
        };
        let output = CrossRepositorySearchOutput {
            schema_version: 1,
            query_profile: 3,
            resolution: resolution.to_owned(),
            repositories_requested: results.len() as u64,
            repositories_completed: completed,
            repositories_failed: failed,
            matches_returned,
            matches_total,
            coverage: McpCoverage {
                searched: completed,
                skipped: failed,
                unresolved: failed,
                truncated,
            },
            limitation: "fts5_literal_search_only_no_cross_repository_relationship_claim"
                .to_owned(),
            repositories: results,
        };
        operation_result(Ok(output), MAX_MCP_SEARCH_OUTPUT_BYTES)
    }

    async fn call_relevant_paths(
        &self,
        request: RelevantPathsServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.relevant_paths(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_RELEVANT_PATHS_OUTPUT_BYTES)
    }

    async fn call_symbol_search(
        &self,
        request: SymbolSearchServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.symbol_search(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_SEARCH_OUTPUT_BYTES)
    }

    async fn call_outbound_sites(
        &self,
        request: OutboundSitesServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.outbound_sites(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_OUTBOUND_SITES_OUTPUT_BYTES)
    }

    async fn call_syntax_site_search(
        &self,
        request: SyntaxSiteSearchServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.syntax_site_search(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_SYNTAX_SITE_SEARCH_OUTPUT_BYTES)
    }

    async fn call_code_graph_query(
        &self,
        request: CodeGraphQueryServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.code_graph_query(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_CODE_GRAPH_QUERY_OUTPUT_BYTES)
    }

    async fn call_architecture_map(
        &self,
        request: ArchitectureMapServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.architecture_map(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_ARCHITECTURE_MAP_OUTPUT_BYTES)
    }

    async fn call_architecture_overview(
        &self,
        request: ArchitectureOverviewServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.architecture_overview(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_ARCHITECTURE_OVERVIEW_OUTPUT_BYTES)
    }

    async fn call_repository_topology(
        &self,
        request: RepositoryTopologyServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.repository_topology(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_REPOSITORY_TOPOLOGY_OUTPUT_BYTES)
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
        request: EvidenceContextBuildServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.context_build(request.with_timeout(remaining), cancelled)
            })
            .await?;
        operation_result(output, MAX_MCP_EVIDENCE_CONTEXT_OUTPUT_BYTES)
    }

    async fn call_change_review(
        &self,
        request: ChangeReviewServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.change_review(request.with_timeout(remaining), cancelled)
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

    async fn call_navigation_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        match request.name.as_ref() {
            ARCHITECTURE_MAP_TOOL_NAME => {
                let input = parse_arguments::<ArchitectureMapInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_architecture_map(request, context).await
            }
            ARCHITECTURE_OVERVIEW_TOOL_NAME => {
                let input = parse_arguments::<ArchitectureOverviewInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_architecture_overview(request, context).await
            }
            REPOSITORY_TOPOLOGY_TOOL_NAME => {
                let input = parse_arguments::<RepositoryTopologyInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_repository_topology(request, context).await
            }
            CODE_SEARCH_TOOL_NAME => {
                let input = parse_arguments::<CodeSearchInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_code_search(request, context).await
            }
            RELEVANT_PATHS_TOOL_NAME => {
                let input = parse_arguments::<RelevantPathsInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_relevant_paths(request, context).await
            }
            SYMBOL_SEARCH_TOOL_NAME => {
                let input = parse_arguments::<SymbolSearchInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_symbol_search(request, context).await
            }
            OUTBOUND_SITES_TOOL_NAME => {
                let input = parse_arguments::<OutboundSitesInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_outbound_sites(request, context).await
            }
            SYNTAX_SITE_SEARCH_TOOL_NAME => {
                let input = parse_arguments::<SyntaxSiteSearchInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_syntax_site_search(request, context).await
            }
            CODE_GRAPH_QUERY_TOOL_NAME => {
                let input = parse_arguments::<CodeGraphQueryInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_code_graph_query(request, context).await
            }
            _ => Err(McpError::invalid_params("unknown RepoWitness tool", None)),
        }
    }

    /// Constructs one read-only MCP surface over a bounded repository catalog.
    /// Tool calls select an opaque repository ID; the default may be omitted.
    pub fn with_repository_catalog(
        registry: McpRepositoryCatalog,
        default_repository_id: Option<String>,
    ) -> Result<Self, McpRepositoryRegistryError> {
        Self::from_catalog(
            CatalogRuntime::new(registry, default_repository_id, None)?,
            false,
        )
    }

    /// Constructs a read-only MCP surface that reloads its catalog at each
    /// request boundary and keeps the last valid snapshot on reload failure.
    pub fn with_reloadable_repository_catalog(
        registry: McpRepositoryCatalog,
        default_repository_id: Option<String>,
        loader: McpRepositoryCatalogLoader,
    ) -> Result<Self, McpRepositoryRegistryError> {
        Self::with_reloadable_repository_catalog_capability(
            registry,
            default_repository_id,
            loader,
            false,
        )
    }

    /// Constructs a reloadable catalog with explicitly authorized memory writes.
    pub fn with_reloadable_repository_catalog_with_memory_writes(
        registry: McpRepositoryCatalog,
        default_repository_id: Option<String>,
        loader: McpRepositoryCatalogLoader,
    ) -> Result<Self, McpRepositoryRegistryError> {
        Self::with_reloadable_repository_catalog_capability(
            registry,
            default_repository_id,
            loader,
            true,
        )
    }

    fn with_reloadable_repository_catalog_capability(
        registry: McpRepositoryCatalog,
        default_repository_id: Option<String>,
        loader: McpRepositoryCatalogLoader,
        memory_writes_enabled: bool,
    ) -> Result<Self, McpRepositoryRegistryError> {
        Self::from_catalog(
            CatalogRuntime::new(registry, default_repository_id, Some(loader))?,
            memory_writes_enabled,
        )
    }

    fn from_catalog(
        runtime: CatalogRuntime,
        memory_writes_enabled: bool,
    ) -> Result<Self, McpRepositoryRegistryError> {
        let catalog = Arc::new(runtime);
        let state = catalog
            .state
            .read()
            .map_err(|_| McpRepositoryRegistryError::Empty)?;
        let service = state
            .registry
            .values()
            .next()
            .expect("catalog is non-empty")
            .clone();
        let repository_ids = state.registry.keys().cloned().collect::<Vec<_>>();
        let selector_optional = state.default_repository_id.is_some();
        drop(state);
        Ok(Self {
            service,
            catalog: Some(catalog),
            operations: Arc::new(Semaphore::new(DEFAULT_MCP_OPERATION_CONCURRENCY)),
            tools: Arc::from(tools_with_repository_selector(
                &repository_ids,
                selector_optional,
                memory_writes_enabled,
            )),
            memory_writes_enabled,
        })
    }

    fn selected_service(
        &self,
        arguments: Option<JsonObject>,
    ) -> Result<(Arc<dyn RepositoryService>, Option<JsonObject>), McpError> {
        let Some(state) = self.catalog_snapshot()? else {
            return Ok((Arc::clone(&self.service), arguments));
        };
        let mut arguments = arguments.unwrap_or_default();
        let repository_id = match arguments.remove("repository_id") {
            Some(value) => value.as_str().map(ToOwned::to_owned),
            None => state
                .default_repository_id
                .as_deref()
                .map(ToOwned::to_owned),
        }
        .ok_or_else(|| {
            McpError::invalid_params(
                "repository_id is required when no default repository is configured",
                None,
            )
        })?;
        let service = state.registry.get(&repository_id).cloned().ok_or_else(|| {
            McpError::invalid_params("repository_id does not name a registered repository", None)
        })?;
        Ok((service, Some(arguments)))
    }

    fn with_selected_service(&self, service: Arc<dyn RepositoryService>) -> Self {
        Self {
            service,
            catalog: None,
            operations: Arc::clone(&self.operations),
            tools: Arc::clone(&self.tools),
            memory_writes_enabled: self.memory_writes_enabled,
        }
    }

    fn catalog_snapshot(&self) -> Result<Option<CatalogState>, McpError> {
        let Some(catalog) = &self.catalog else {
            return Ok(None);
        };
        catalog
            .state
            .read()
            .map(|state| Some(state.clone()))
            .map_err(|_| McpError::internal_error("catalog state is unavailable", None))
    }

    async fn refresh_catalog(&self) -> Result<(), McpError> {
        let Some(catalog) = &self.catalog else {
            return Ok(());
        };
        let Some(loader) = &catalog.loader else {
            return Ok(());
        };
        let _refresh = catalog.refresh.lock().await;
        let loader = Arc::clone(loader);
        let loaded = tokio::task::spawn_blocking(move || loader())
            .await
            .map_err(|_| McpError::internal_error("catalog refresh failed", None))?
            .map_err(|_| McpError::internal_error("catalog refresh failed", None))?;
        let state = CatalogState::new(loaded.0, loaded.1)
            .map_err(|_| McpError::internal_error("catalog refresh failed", None))?;
        *catalog
            .state
            .write()
            .map_err(|_| McpError::internal_error("catalog state is unavailable", None))? = state;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Failure while constructing a bounded local repository catalog.
pub enum McpRepositoryRegistryError {
    /// No repositories were supplied.
    Empty,
    /// The repository count exceeds the local bound.
    TooMany,
    /// The configured default is absent from the catalog.
    DefaultMissing,
}

include!("server/operation_supervisor.rs");
include!("server/graph.rs");
fn server_instructions(
    memory_writes_enabled: bool,
    multi_repository: bool,
    has_default_repository: bool,
) -> String {
    if multi_repository {
        return if has_default_repository {
            if memory_writes_enabled {
                "RepoWitness exposes bounded source evidence, context, diagnostics, memory recall, explicitly authorized memory management, and catalog-wide FTI search. The current repository is the default; pass repository_id to select another registered repository."
                    .to_owned()
            } else {
                "RepoWitness exposes bounded read-only source evidence, context, diagnostics, memory recall, and catalog-wide FTI search. The current repository is the default; pass repository_id to select another registered repository."
                    .to_owned()
            }
        } else {
            if memory_writes_enabled {
                "RepoWitness exposes bounded source evidence, context, diagnostics, memory recall, explicitly authorized memory management, and catalog-wide FTI search for registered repositories. Repository-scoped calls require repository_id."
                    .to_owned()
            } else {
                "RepoWitness exposes bounded read-only source evidence, context, diagnostics, memory recall, and catalog-wide FTI search for registered repositories. Repository-scoped calls require repository_id."
                    .to_owned()
            }
        };
    }
    if memory_writes_enabled {
        "RepoWitness exposes bounded source evidence, context, diagnostics, recall, and explicitly authorized memory management for one repository.".to_owned()
    } else {
        "RepoWitness exposes bounded read-only source evidence, context, diagnostics, and memory recall for one repository.".to_owned()
    }
}

impl RepoWitnessMcpServer {
    #[allow(
        clippy::too_many_lines,
        reason = "the fixed native read-only tool dispatch keeps request validation auditable"
    )]
    async fn call_selected_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if matches!(
            request.name.as_ref(),
            ARCHITECTURE_MAP_TOOL_NAME
                | ARCHITECTURE_OVERVIEW_TOOL_NAME
                | REPOSITORY_TOPOLOGY_TOOL_NAME
                | CODE_SEARCH_TOOL_NAME
                | RELEVANT_PATHS_TOOL_NAME
                | SYMBOL_SEARCH_TOOL_NAME
                | OUTBOUND_SITES_TOOL_NAME
                | SYNTAX_SITE_SEARCH_TOOL_NAME
                | CODE_GRAPH_QUERY_TOOL_NAME
        ) {
            return self.call_navigation_tool(request, context).await;
        }
        match request.name.as_ref() {
            CHANGE_REVIEW_TOOL_NAME => {
                let input = parse_arguments::<ChangeReviewInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_change_review(request, context).await
            }
            CONTEXT_BUILD_TOOL_NAME => {
                let input = parse_arguments::<EvidenceContextBuildInput>(request.arguments)?;
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

impl ServerHandler for RepoWitnessMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_11_25)
            .with_server_info(Implementation::new(
                "repowitness",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(server_instructions(
                self.memory_writes_enabled,
                self.catalog.is_some(),
                self.catalog
                    .as_ref()
                    .and_then(|catalog| catalog.state.read().ok())
                    .is_some_and(|state| state.default_repository_id.is_some()),
            ))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        self.refresh_catalog().await?;
        let tools = if let Some(state) = self.catalog_snapshot()? {
            tools_with_repository_selector(
                &state.registry.keys().cloned().collect::<Vec<_>>(),
                state.default_repository_id.is_some(),
                self.memory_writes_enabled,
            )
        } else {
            self.tools.iter().cloned().collect()
        };
        Ok(ListToolsResult {
            tools,
            ..ListToolsResult::default()
        })
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the complete versioned tool dispatch is intentionally one auditable closed match over fixed capabilities"
    )]
    async fn call_tool(
        &self,
        mut request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.refresh_catalog().await?;
        if self.catalog.is_some() {
            if request.name.as_ref() == CROSS_REPOSITORY_SEARCH_TOOL_NAME {
                let input = parse_arguments::<CrossRepositorySearchInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                return self.call_cross_repository_search(request, context).await;
            }
            let (service, arguments) = self.selected_service(request.arguments)?;
            request.arguments = arguments;
            return self
                .with_selected_service(service)
                .call_selected_tool(request, context)
                .await;
        }
        if matches!(
            request.name.as_ref(),
            ARCHITECTURE_MAP_TOOL_NAME
                | ARCHITECTURE_OVERVIEW_TOOL_NAME
                | REPOSITORY_TOPOLOGY_TOOL_NAME
                | CODE_SEARCH_TOOL_NAME
                | RELEVANT_PATHS_TOOL_NAME
                | SYMBOL_SEARCH_TOOL_NAME
                | OUTBOUND_SITES_TOOL_NAME
                | SYNTAX_SITE_SEARCH_TOOL_NAME
                | CODE_GRAPH_QUERY_TOOL_NAME
                | CHANGE_REVIEW_TOOL_NAME
                | CONTEXT_BUILD_TOOL_NAME
                | DIAGNOSTICS_TOOL_NAME
                | GRAPH_STATUS_TOOL_NAME
                | GRAPH_SEARCH_TOOL_NAME
                | GRAPH_EVIDENCE_TOOL_NAME
                | GRAPH_ARCHITECTURE_TOOL_NAME
                | GRAPH_TRACE_TOOL_NAME
                | IMPACT_ANALYZE_TOOL_NAME
                | MEMORY_RECALL_TOOL_NAME
                | SYMBOL_GET_TOOL_NAME
        ) {
            return self.call_selected_tool(request, context).await;
        }
        match request.name.as_ref() {
            MEMORY_MANAGE_TOOL_NAME if self.memory_writes_enabled => {
                let input = parse_arguments::<MemoryManageInput>(request.arguments)?;
                let request = input
                    .validate()
                    .map_err(|message| McpError::invalid_params(message, None))?;
                self.call_memory_manage(request, context).await
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

/// Serves one read-only MCP process over a bounded repository catalog.
pub async fn serve_stdio_with_repository_catalog(
    registry: McpRepositoryCatalog,
    default_repository_id: Option<String>,
) -> Result<(), McpServeError> {
    let input = BoundedLineReader::try_new(tokio::io::stdin(), MAX_MCP_INPUT_LINE_BYTES)
        .expect("the fixed MCP input-line limit is positive");
    let server = RepoWitnessMcpServer::with_repository_catalog(registry, default_repository_id)
        .map_err(|_| McpServeError::Initialize)?;
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

/// Serves a read-only MCP process over a catalog reloaded at request boundaries.
pub async fn serve_stdio_with_reloadable_repository_catalog(
    registry: McpRepositoryCatalog,
    default_repository_id: Option<String>,
    loader: McpRepositoryCatalogLoader,
) -> Result<(), McpServeError> {
    serve_stdio_with_reloadable_repository_catalog_configured(
        registry,
        default_repository_id,
        loader,
        false,
    )
    .await
}

/// Serves a reloadable catalog with explicitly authorized memory mutation enabled.
pub async fn serve_stdio_with_reloadable_repository_catalog_with_memory_writes(
    registry: McpRepositoryCatalog,
    default_repository_id: Option<String>,
    loader: McpRepositoryCatalogLoader,
) -> Result<(), McpServeError> {
    serve_stdio_with_reloadable_repository_catalog_configured(
        registry,
        default_repository_id,
        loader,
        true,
    )
    .await
}

async fn serve_stdio_with_reloadable_repository_catalog_configured(
    registry: McpRepositoryCatalog,
    default_repository_id: Option<String>,
    loader: McpRepositoryCatalogLoader,
    memory_writes_enabled: bool,
) -> Result<(), McpServeError> {
    let input = BoundedLineReader::try_new(tokio::io::stdin(), MAX_MCP_INPUT_LINE_BYTES)
        .expect("the fixed MCP input-line limit is positive");
    let server = RepoWitnessMcpServer::with_reloadable_repository_catalog_capability(
        registry,
        default_repository_id,
        loader,
        memory_writes_enabled,
    )
    .map_err(|_| McpServeError::Initialize)?;
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

async fn serve_stdio_configured(
    service: Arc<dyn RepositoryService>,
    memory_writes_enabled: bool,
) -> Result<(), McpServeError> {
    let input = BoundedLineReader::try_new(tokio::io::stdin(), MAX_MCP_INPUT_LINE_BYTES)
        .expect("the fixed MCP input-line limit is positive");
    let server = RepoWitnessMcpServer::configured(
        service,
        DEFAULT_MCP_OPERATION_CONCURRENCY,
        memory_writes_enabled,
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
    let relevant_paths = Tool::new(
        RELEVANT_PATHS_TOOL_NAME,
        "Group bounded literal declaration matches into generation-pinned source paths. Paths, counts, and path truncation cover returned candidates only; use match coverage before treating them as exhaustive. Ordering is returned lexical-match count then canonical path, with no semantic or relationship claim.",
        JsonObject::new(),
    )
    .with_input_schema::<RelevantPathsInput>()
    .with_output_schema::<RelevantPathsOutput>()
    .annotate(annotations.clone());
    let symbol_search = Tool::new(
        SYMBOL_SEARCH_TOOL_NAME,
        "Find exact or prefix direct declaration facts across Rust/Go/TypeScript/TSX/Python with optional persisted language, kind, and repository-relative path filters. Same-name results do not imply relationships.",
        JsonObject::new(),
    )
    .with_input_schema::<SymbolSearchInput>()
    .with_output_schema::<SymbolSearchOutput>()
    .annotate(annotations.clone());
    let outbound_sites = Tool::new(
        OUTBOUND_SITES_TOOL_NAME,
        "Read bounded exact parser-attributed import, reference, call, and test-marker observations physically contained in one selected declaration. Raw target spellings remain unresolved; this tool creates no edges or relationships.",
        JsonObject::new(),
    )
    .with_input_schema::<OutboundSitesInput>()
    .with_output_schema::<OutboundSitesOutput>()
    .annotate(annotations.clone());
    let syntax_site_search = Tool::new(
        SYNTAX_SITE_SEARCH_TOOL_NAME,
        "Find bounded parser-attributed import, reference, call, and test-marker observations with one exact raw target spelling across the active supported-language generation. Equal spelling is not target resolution, a caller relationship, or an inferred edge.",
        JsonObject::new(),
    )
    .with_input_schema::<SyntaxSiteSearchInput>()
    .with_output_schema::<SyntaxSiteSearchOutput>()
    .annotate(annotations.clone());
    let code_graph_query = Tool::new(
        CODE_GRAPH_QUERY_TOOL_NAME,
        "Run exactly one bounded evidence-backed code discovery operation: symbols, outbound_sites, syntax_site_search, architecture, files, test_markers, or relevant_paths. This is a closed union, not Cypher, SQL, or a general graph query surface.",
        code_graph_query_input_schema(),
    )
    .with_output_schema::<CodeGraphQueryOutput>()
    .annotate(annotations.clone());
    let architecture_map = Tool::new(
        ARCHITECTURE_MAP_TOOL_NAME,
        "Map exact indexed Rust/Go/TypeScript/TSX/Python source files by canonical path with \
         generation-pinned source and parser-artifact receipts. This is a file inventory, not a \
         relationship or call graph.",
        JsonObject::new(),
    )
    .with_input_schema::<ArchitectureMapInput>()
    .with_output_schema::<ArchitectureMapOutput>()
    .annotate(annotations.clone());
    let architecture_overview = Tool::new(
        ARCHITECTURE_OVERVIEW_TOOL_NAME,
        "Summarize exact active Rust/Go/TypeScript/TSX/Python source facts with independent bounded \
         source-root, direct-syntax `function main` candidate, and file receipts. Structural roots \
         are not package or ownership boundaries; this tool does not infer relationships.",
        JsonObject::new(),
    )
    .with_input_schema::<ArchitectureOverviewInput>()
    .with_output_schema::<ArchitectureOverviewOutput>()
    .annotate(annotations.clone());
    let repository_topology = Tool::new(
        REPOSITORY_TOPOLOGY_TOOL_NAME,
        "Inventory exact Git-discovered repository paths by a fixed path-only category. It returns no file contents, relationships, ownership, build, or runtime claims.",
        JsonObject::new(),
    )
    .with_input_schema::<RepositoryTopologyInput>()
    .with_output_schema::<RepositoryTopologyOutput>()
    .annotate(annotations.clone());
    let context_build = Tool::new(
        CONTEXT_BUILD_TOOL_NAME,
        "Compile a deterministic evidence-balanced, generation-pinned context pack from exact \
         source, native graph, eligible SCIP, current memory, and qualified history evidence \
         under a labeled conservative content budget.",
        JsonObject::new(),
    )
    .with_input_schema::<EvidenceContextBuildInput>()
    .with_output_schema::<EvidenceContextBuildOutput>()
    .annotate(annotations.clone());
    let change_review = Tool::new(
        CHANGE_REVIEW_TOOL_NAME,
        "Build a bounded, read-only revision-pinned change-review receipt. It fences the current worktree and includes separately pinned indexed context only when its exact source remains current; otherwise it reports categorical absence without stale source. It never returns an approval, test-execution claim, or inferred index/worktree equivalence.",
        JsonObject::new(),
    )
    .with_input_schema::<ChangeReviewInput>()
    .with_output_schema::<ChangeReviewOutput>()
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
        architecture_map,
        architecture_overview,
        repository_topology,
        code_search,
        relevant_paths,
        symbol_search,
        outbound_sites,
        syntax_site_search,
        code_graph_query,
        change_review,
        context_build,
        diagnostics,
        memory_recall,
        symbol_get,
    ];
    tools.extend(graph_tools(&annotations));
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
    tools.sort_by(|left, right| left.name.as_ref().cmp(right.name.as_ref()));
    tools
}

fn tools_with_repository_selector(
    repository_ids: &[String],
    selector_optional: bool,
    memory_writes_enabled: bool,
) -> Vec<Tool> {
    let mut tools = tools(memory_writes_enabled);
    tools.push(cross_repository_search_tool());
    for tool in &mut tools {
        if tool.name.as_ref() == CROSS_REPOSITORY_SEARCH_TOOL_NAME {
            let schema = Arc::make_mut(&mut tool.input_schema);
            if let Some(items) = schema
                .get_mut("properties")
                .and_then(serde_json::Value::as_object_mut)
                .and_then(|properties| properties.get_mut("repository_ids"))
                .and_then(serde_json::Value::as_object_mut)
                .and_then(|ids| ids.get_mut("items"))
                .and_then(serde_json::Value::as_object_mut)
            {
                items.insert(
                    "enum".to_owned(),
                    serde_json::Value::Array(
                        repository_ids
                            .iter()
                            .cloned()
                            .map(serde_json::Value::String)
                            .collect(),
                    ),
                );
            }
            continue;
        }
        let schema = Arc::make_mut(&mut tool.input_schema);
        let properties = schema
            .get_mut("properties")
            .and_then(serde_json::Value::as_object_mut)
            .expect("RepoWitness tool schemas have object properties");
        properties.insert(
            "repository_id".to_owned(),
            serde_json::json!({
                "type": "string",
                "enum": repository_ids,
                "description": "Opaque repository identity from the local RepoWitness catalog."
            }),
        );
        if !selector_optional {
            schema
                .entry("required")
                .or_insert_with(|| serde_json::Value::Array(Vec::new()))
                .as_array_mut()
                .expect("RepoWitness tool schemas have array required fields")
                .push(serde_json::Value::String("repository_id".to_owned()));
        }
    }
    tools
}

fn cross_repository_search_tool() -> Tool {
    Tool::new(
        CROSS_REPOSITORY_SEARCH_TOOL_NAME,
        "Search the registered repositories with bounded SQLite FTS5 literal terms. Results are grouped by opaque repository identity and generation; matching text across repositories is candidate evidence only and creates no relationship claim.",
        JsonObject::new(),
    )
    .with_input_schema::<CrossRepositorySearchInput>()
    .with_output_schema::<CrossRepositorySearchOutput>()
    .annotate(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    )
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
