use std::{
    fmt,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use repowitness_application::{
    CodeSearchQuery, DEFAULT_CODE_SEARCH_RESULTS, MAX_CODE_SEARCH_RESULTS, RepositoryPathLimits,
    RepositoryPathTextByteLimit, RepositoryPathTextV1,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

mod architecture_map;
mod architecture_overview;
mod change_review;
mod code_graph_query;
mod cross_repository_search;
mod diagnostics;
mod evidence_context;
mod graph;
mod memory_manage;
mod memory_mutation;
mod memory_recall;
mod outbound_sites;
mod relevant_paths;
mod repository_service_error;
mod repository_topology;
mod scip_evidence;
mod scip_relationship_trace;
mod scip_symbol_resolve;
mod symbol_search;
mod syntax_site_search;
mod test_markers;

pub use architecture_map::{
    ARCHITECTURE_MAP_TOOL_NAME, ArchitectureMapInput, ArchitectureMapOutput,
    ArchitectureMapServiceRequest, McpArchitectureMapFile, McpArchitectureMapLanguage,
};
pub use architecture_overview::{
    ARCHITECTURE_OVERVIEW_LIMITATIONS, ARCHITECTURE_OVERVIEW_TOOL_NAME, ArchitectureOverviewInput,
    ArchitectureOverviewOutput, ArchitectureOverviewServiceRequest, McpArchitectureOverviewKind,
    McpArchitectureOverviewRoot,
};
pub use change_review::{
    CHANGE_REVIEW_SCHEMA_VERSION, CHANGE_REVIEW_TOOL_NAME, ChangeReviewInput, ChangeReviewOutput,
    ChangeReviewServiceRequest, McpChangeReviewPath,
};
pub use code_graph_query::{
    CODE_GRAPH_QUERY_PROFILE_VERSION, CODE_GRAPH_QUERY_SCHEMA_VERSION, CODE_GRAPH_QUERY_TOOL_NAME,
    CodeGraphQueryInput, CodeGraphQueryOutput, CodeGraphQueryResultOutput,
    CodeGraphQueryServiceRequest,
};
pub use cross_repository_search::{
    CROSS_REPOSITORY_SEARCH_TOOL_NAME, CrossRepositorySearchInput, CrossRepositorySearchOutput,
    CrossRepositorySearchRepository, CrossRepositorySearchServiceRequest,
    MAX_CROSS_REPOSITORY_RESULTS, MAX_CROSS_REPOSITORY_SELECTIONS,
};
pub use diagnostics::{
    DiagnosticsInput, DiagnosticsOutput, DiagnosticsServiceRequest, McpConfigurationIdentity,
    McpDiagnosticsMemoryProjection,
};
pub use evidence_context::{
    EvidenceContextBuildInput, EvidenceContextBuildOutput, EvidenceContextBuildServiceRequest,
    McpEvidenceContextAttribution, McpEvidenceContextItem, McpEvidenceContextOmission,
    McpEvidenceContextPayload, McpEvidenceContextProviderCoverage, McpEvidenceContextScope,
};
pub use graph::{
    GRAPH_ARCHITECTURE_TOOL_NAME, GRAPH_EVIDENCE_TOOL_NAME, GRAPH_SEARCH_TOOL_NAME,
    GRAPH_STATUS_TOOL_NAME, GRAPH_TRACE_TOOL_NAME, GraphArchitectureInput, GraphArchitectureOutput,
    GraphEvidenceInput, GraphEvidenceOutput, GraphImpactInput, GraphImpactOutput,
    GraphReadServiceOutput, GraphReadServiceRequest, GraphSearchInput, GraphSearchOutput,
    GraphStatusInput, GraphStatusOutput, GraphTraceInput, GraphTraceOutput,
    IMPACT_ANALYZE_TOOL_NAME, McpGraphArchitectureCount, McpGraphCandidate, McpGraphCardinality,
    McpGraphContext, McpGraphDefinition, McpGraphEdge, McpGraphEvidence, McpGraphImpact,
    McpGraphPublication, McpGraphSite, McpGraphTrace, McpGraphTraceCoverage,
    McpGraphTraceTruncation,
};
pub use memory_manage::{
    MEMORY_MANAGE_SCHEMA_VERSION, MemoryManageDatabaseIdentityStatus,
    MemoryManageFileIdentityStatus, MemoryManageInput, MemoryManageMaintenanceStatus,
    MemoryManageMaintenanceStepStatus, MemoryManageOperation, MemoryManageOutput,
    MemoryManagePublicationStatus, MemoryManagePublicationStepStatus, MemoryManageReceipt,
    MemoryManageReviewDecision, MemoryManageServiceRequest,
};
pub use memory_mutation::{MemoryMutationOperation, MemoryMutationRequestScope};
pub use memory_recall::{
    McpMemoryCandidate, McpMemoryCoverage, McpMemoryEvidence, McpMemoryOccurrence,
    McpMemoryProducer, McpMemoryRecord, McpMemoryTarget, McpSelectedMemory, MemoryRecallInput,
    MemoryRecallOutput, MemoryRecallServiceRequest, MemoryRecallServiceSelection,
};
pub use outbound_sites::{
    McpOutboundSitesDeclaration, McpOutboundSyntaxSite, OUTBOUND_SITES_TOOL_NAME,
    OutboundSitesInput, OutboundSitesOutput, OutboundSitesSelectorOutput,
    OutboundSitesServiceRequest,
};
pub use relevant_paths::{
    McpRelevantPath, RELEVANT_PATHS_TOOL_NAME, RelevantPathsInput, RelevantPathsOutput,
    RelevantPathsServiceRequest,
};
pub use repository_service_error::RepositoryServiceError;
pub use repository_topology::{
    McpRepositoryTopologyCategory, McpRepositoryTopologyCoverage, McpRepositoryTopologyEntry,
    REPOSITORY_TOPOLOGY_TOOL_NAME, RepositoryTopologyInput, RepositoryTopologyOutput,
    RepositoryTopologyServiceRequest,
};
pub use scip_evidence::{
    McpScipOccurrence, McpScipOverlay, McpScipRelationship, SCIP_EVIDENCE_SCHEMA_VERSION,
    ScipEvidenceInput, ScipEvidenceOutput, ScipEvidenceServiceRequest,
};
pub use scip_relationship_trace::{
    McpScipRelationshipTraceEdge, McpScipRelationshipTraceOverlay,
    SCIP_RELATIONSHIP_TRACE_SCHEMA_VERSION, SCIP_RELATIONSHIP_TRACE_TOOL_NAME,
    ScipRelationshipTraceInput, ScipRelationshipTraceOutput, ScipRelationshipTraceServiceRequest,
};
pub use scip_symbol_resolve::{
    SCIP_SYMBOL_RESOLVE_TOOL_NAME, ScipSymbolResolveInput, ScipSymbolResolveOutput,
    ScipSymbolResolveServiceRequest,
};
pub use symbol_search::{
    SYMBOL_SEARCH_TOOL_NAME, SymbolSearchInput, SymbolSearchOutput, SymbolSearchServiceRequest,
};
pub use syntax_site_search::{
    SYNTAX_SITE_SEARCH_TOOL_NAME, SyntaxSiteSearchInput, SyntaxSiteSearchOutput,
    SyntaxSiteSearchServiceRequest,
};
pub use test_markers::{
    McpTestMarkerLanguageCoverage, TestMarkersInput, TestMarkersOutput, TestMarkersServiceRequest,
};

/// MCP tool name for bounded lexical supported-language symbol search.
pub const CODE_SEARCH_TOOL_NAME: &str = "code_search";
/// MCP tool name for deterministic bounded evidence-balanced context compilation.
pub const CONTEXT_BUILD_TOOL_NAME: &str = "context_build";
/// MCP tool name for transactionally pinned read-only repository diagnostics.
pub const DIAGNOSTICS_TOOL_NAME: &str = "diagnostics";
/// MCP tool name for generation-pinned engineering-memory retrieval.
pub const MEMORY_RECALL_TOOL_NAME: &str = "memory_recall";
/// MCP tool name for explicitly authorized local engineering-memory mutation.
pub const MEMORY_MANAGE_TOOL_NAME: &str = "memory_manage";
/// MCP tool name for immutable package-scoped SCIP symbol evidence.
pub const SCIP_EVIDENCE_TOOL_NAME: &str = "scip_evidence";
/// MCP tool name for the bounded repository catalog.
/// MCP tool name for exact verified declaration retrieval.
pub const SYMBOL_GET_TOOL_NAME: &str = "symbol_get";

pub(crate) const DEFAULT_MCP_TIMEOUT_MS: u64 = 5_000;
pub(crate) const MAX_MCP_TIMEOUT_MS: u64 = 30_000;
const MAX_PATH_BYTES: u64 = 1_048_576;
const MAX_PATH_COMPONENTS: u64 = 1_048_576;
const MAX_PATH_TEXT_BYTES: u64 = 7 + (MAX_PATH_BYTES * 2);
/// Largest integer that is exact in every supported MCP JSON implementation.
pub const MAX_MCP_INTEROPERABLE_INTEGER: u64 = 9_007_199_254_740_991;
pub(crate) const MAX_MCP_SEARCH_OUTPUT_BYTES: usize = 3 * 1024 * 1024;
pub(crate) const MAX_MCP_RELEVANT_PATHS_OUTPUT_BYTES: usize = 3 * 1024 * 1024;
pub(crate) const MAX_MCP_ARCHITECTURE_MAP_OUTPUT_BYTES: usize = 10 * 1024 * 1024;
pub(crate) const MAX_MCP_ARCHITECTURE_OVERVIEW_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_MCP_REPOSITORY_TOPOLOGY_OUTPUT_BYTES: usize = 10 * 1024 * 1024;
pub(crate) const MAX_MCP_CONTEXT_OUTPUT_BYTES: usize = 24 * 1024 * 1024;
pub(crate) const MAX_MCP_EVIDENCE_CONTEXT_OUTPUT_BYTES: usize = 24 * 1024 * 1024;
pub(crate) const MAX_MCP_DIAGNOSTICS_OUTPUT_BYTES: usize = 256 * 1024;
pub(crate) const MAX_MCP_GRAPH_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_MCP_MEMORY_RECALL_OUTPUT_BYTES: usize = 20 * 1024 * 1024;
pub(crate) const MAX_MCP_MEMORY_MANAGE_OUTPUT_BYTES: usize = 64 * 1024;
// The MCP SDK includes both structured JSON and a compatibility text copy.
// A bounded 10 MiB application payload can therefore require almost 60 MiB
// after exact source representation and nested JSON escaping.
pub(crate) const MAX_MCP_SYMBOL_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_MCP_OUTBOUND_SITES_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_MCP_SYNTAX_SITE_SEARCH_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_MCP_CODE_GRAPH_QUERY_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_MCP_SCIP_EVIDENCE_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_MCP_SCIP_RELATIONSHIP_TRACE_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

/// Version-1 wire input for `code_search`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodeSearchInput {
    /// Literal Rust, Go, TypeScript, TSX, or Python symbol terms. FTS syntax is never accepted.
    pub query: String,
    /// Maximum returned candidates, from 1 through 100.
    pub max_results: Option<u16>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for CodeSearchInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodeSearchInput")
            .field("query", &"<redacted-query>")
            .field("max_results", &self.max_results)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl CodeSearchInput {
    pub(crate) fn validate(self) -> Result<CodeSearchServiceRequest, &'static str> {
        let query = CodeSearchQuery::try_new(&self.query)
            .map_err(|_| "query does not satisfy the bounded literal search profile")?;
        let max_results = self.max_results.unwrap_or(DEFAULT_CODE_SEARCH_RESULTS);
        if !(1..=MAX_CODE_SEARCH_RESULTS).contains(&max_results) {
            return Err("max_results must be between 1 and 100");
        }
        let timeout = validate_timeout(self.timeout_ms)?;
        Ok(CodeSearchServiceRequest {
            query: query.as_str().to_owned(),
            max_results,
            timeout,
        })
    }
}

/// Validated, owned request passed from the MCP adapter to the composition root.
pub struct CodeSearchServiceRequest {
    query: String,
    max_results: u16,
    timeout: Duration,
}

impl CodeSearchServiceRequest {
    pub(crate) fn new(query: String, max_results: u16, timeout: Duration) -> Self {
        Self {
            query,
            max_results,
            timeout,
        }
    }

    /// Returns the canonical literal query.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Returns the inclusive result-count bound.
    #[must_use]
    pub const fn max_results(&self) -> u16 {
        self.max_results
    }

    /// Returns the remaining end-to-end deadline duration.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl fmt::Debug for CodeSearchServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodeSearchServiceRequest")
            .field("query", &"<redacted-query>")
            .field("max_results", &self.max_results)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Version-1 wire input for `symbol_get`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolGetInput {
    /// Exact snapshot SHA-256 from a `code_search` result.
    pub snapshot_sha256: String,
    /// Exact positive active-generation identifier.
    pub generation: i64,
    /// Canonical byte-preserving repository path from a search match.
    pub path: String,
    /// Exact source-content SHA-256 from a search match.
    pub content_sha256: String,
    /// Exact analysis-artifact SHA-256 from a search match.
    pub artifact_sha256: String,
    /// Exact fact ordinal from a search match.
    pub fact_ordinal: u64,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for SymbolGetInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SymbolGetInput")
            .field("snapshot_sha256", &"<redacted-digest>")
            .field("generation", &self.generation)
            .field("path", &"<redacted-path>")
            .field("content_sha256", &"<redacted-digest>")
            .field("artifact_sha256", &"<redacted-digest>")
            .field("fact_ordinal", &self.fact_ordinal)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl SymbolGetInput {
    pub(crate) fn validate(self) -> Result<SymbolGetServiceRequest, &'static str> {
        if self.generation <= 0 {
            return Err("generation must be a positive identifier");
        }
        if !is_lowercase_sha256(&self.snapshot_sha256)
            || !is_lowercase_sha256(&self.content_sha256)
            || !is_lowercase_sha256(&self.artifact_sha256)
        {
            return Err("digest fields must be lowercase SHA-256 text");
        }
        if !is_canonical_path_text(&self.path) {
            return Err("path must be bounded canonical rwp1:h: text");
        }
        if self.fact_ordinal > MAX_MCP_INTEROPERABLE_INTEGER {
            return Err("fact_ordinal exceeds the interoperable integer range");
        }
        let timeout = validate_timeout(self.timeout_ms)?;
        Ok(SymbolGetServiceRequest {
            snapshot_sha256: self.snapshot_sha256,
            generation: self.generation,
            path: self.path,
            content_sha256: self.content_sha256,
            artifact_sha256: self.artifact_sha256,
            fact_ordinal: self.fact_ordinal,
            timeout,
        })
    }
}

/// Validated, owned exact-symbol request passed to the composition root.
pub struct SymbolGetServiceRequest {
    snapshot_sha256: String,
    generation: i64,
    path: String,
    content_sha256: String,
    artifact_sha256: String,
    fact_ordinal: u64,
    timeout: Duration,
}

impl SymbolGetServiceRequest {
    /// Returns the exact snapshot SHA-256 text.
    #[must_use]
    pub fn snapshot_sha256(&self) -> &str {
        &self.snapshot_sha256
    }

    /// Returns the exact positive generation identifier.
    #[must_use]
    pub const fn generation(&self) -> i64 {
        self.generation
    }

    /// Returns the canonical exact repository path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the exact source-content SHA-256 text.
    #[must_use]
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }

    /// Returns the exact analysis-artifact SHA-256 text.
    #[must_use]
    pub fn artifact_sha256(&self) -> &str {
        &self.artifact_sha256
    }

    /// Returns the exact generation-local fact ordinal.
    #[must_use]
    pub const fn fact_ordinal(&self) -> u64 {
        self.fact_ordinal
    }

    /// Returns the remaining end-to-end deadline duration.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl fmt::Debug for SymbolGetServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SymbolGetServiceRequest")
            .field("snapshot_sha256", &"<redacted-digest>")
            .field("generation", &self.generation)
            .field("path", &"<redacted-path>")
            .field("content_sha256", &"<redacted-digest>")
            .field("artifact_sha256", &"<redacted-digest>")
            .field("fact_ordinal", &self.fact_ordinal)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Synchronous repository operations injected by the CLI composition root.
///
/// Implementations must honor both the request timeout and cancellation flag.
/// They must return only bounded output DTOs and stable, redacted errors.
pub trait RepositoryService: Send + Sync + 'static {
    /// Runs one bounded lexical search.
    fn code_search(
        &self,
        request: CodeSearchServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<CodeSearchOutput, RepositoryServiceError>;

    /// Groups one bounded lexical search receipt into directly supported source paths.
    fn relevant_paths(
        &self,
        _request: RelevantPathsServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<RelevantPathsOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::RelevantPaths)
    }

    /// Finds bounded direct declaration facts with exact/prefix and typed filters.
    fn symbol_search(
        &self,
        _request: SymbolSearchServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<SymbolSearchOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::SymbolSearch)
    }

    /// Reads exact parser-attributed raw sites inside one selected declaration.
    fn outbound_sites(
        &self,
        _request: OutboundSitesServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<OutboundSitesOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::OutboundSites)
    }

    /// Searches immutable raw syntax observations by one exact target spelling.
    fn syntax_site_search(
        &self,
        _request: SyntaxSiteSearchServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<SyntaxSiteSearchOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::SyntaxSiteSearch)
    }

    /// Runs exactly one closed bounded code-discovery operation.
    fn code_graph_query(
        &self,
        _request: CodeGraphQueryServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<CodeGraphQueryOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::CodeGraphQuery)
    }

    /// Maps exact indexed source files across all Phase 0 languages without inferring relationships.
    fn architecture_map(
        &self,
        _request: ArchitectureMapServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<ArchitectureMapOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::ArchitectureMap)
    }

    /// Summarizes bounded source-only repository orientation without inferring relationships.
    fn architecture_overview(
        &self,
        _request: ArchitectureOverviewServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<ArchitectureOverviewOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::ArchitectureOverview)
    }

    /// Returns a bounded path-only topology inventory without reading content.
    fn repository_topology(
        &self,
        _request: RepositoryTopologyServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<RepositoryTopologyOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::RepositoryTopology)
    }

    /// Compiles one bounded evidence-balanced context pack.
    fn context_build(
        &self,
        request: EvidenceContextBuildServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<EvidenceContextBuildOutput, RepositoryServiceError>;

    /// Builds one bounded, source-fenced revision-pinned review receipt.
    fn change_review(
        &self,
        _request: ChangeReviewServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<ChangeReviewOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::ChangeReview)
    }

    /// Reads one transactionally pinned active repository state.
    fn diagnostics(
        &self,
        request: DiagnosticsServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<DiagnosticsOutput, RepositoryServiceError>;

    /// Runs one native immutable-view-pinned Rust graph operation.
    fn graph_read(
        &self,
        _request: GraphReadServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<GraphReadServiceOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::GraphRead)
    }

    /// Reads bounded package-scoped SCIP evidence from one immutable overlay.
    fn scip_evidence(
        &self,
        _request: ScipEvidenceServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<ScipEvidenceOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::ScipEvidence)
    }

    /// Traces bounded producer-declared and exact enclosed-reference SCIP relationships from one exact symbol.
    fn scip_relationship_trace(
        &self,
        _request: ScipRelationshipTraceServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<ScipRelationshipTraceOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::ScipRelationshipTrace)
    }

    /// Resolves an exact source identifier span to an opaque SCIP symbol.
    fn scip_symbol_resolve(
        &self,
        _request: ScipSymbolResolveServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<ScipSymbolResolveOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::ScipSymbolResolve)
    }

    /// Recalls bounded records from the complete active memory projection.
    fn memory_recall(
        &self,
        request: MemoryRecallServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<MemoryRecallOutput, RepositoryServiceError>;

    /// Performs one explicitly authorized, path-confined memory mutation.
    fn memory_manage(
        &self,
        _request: MemoryManageServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<MemoryManageOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::MemoryManage)
    }

    /// Retrieves one exact, verified source declaration.
    fn symbol_get(
        &self,
        request: SymbolGetServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<SymbolGetOutput, RepositoryServiceError>;
}

/// Versioned categorical coverage counts.
#[derive(Clone, Copy, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpCoverage {
    /// Work completed within the selected scope.
    pub searched: u64,
    /// Work intentionally omitted by the index profile.
    pub skipped: u64,
    /// Work that could not be resolved.
    pub unresolved: u64,
    /// Work omitted by an explicit result bound.
    pub truncated: u64,
}

/// Versioned half-open byte span.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSpan {
    /// Inclusive starting byte offset.
    pub start: u64,
    /// Exclusive ending byte offset.
    pub end: u64,
}

/// One attributed supported-language symbol match in a `code_search` response.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSearchMatch {
    /// Canonical byte-preserving repository path.
    pub path: String,
    /// Exact generation-local fact ordinal.
    pub fact_ordinal: u64,
    /// Exact source-content SHA-256.
    pub content_sha256: String,
    /// Exact analysis-artifact SHA-256.
    pub artifact_sha256: String,
    /// Producer-manifest SHA-256.
    pub producer_manifest_sha256: String,
    /// Evidence strength; currently `syntax`.
    pub evidence_tier: String,
    /// Persisted source language: `rust`, `go`, `typescript`, `tsx`, or `python`.
    pub language: String,
    /// Language-specific declaration kind.
    pub kind: String,
    /// Unqualified declaration name.
    pub name: String,
    /// Deterministic lexical qualified name.
    pub qualified_name: String,
    /// Exact declaration-name byte span.
    pub name_span: McpSpan,
    /// Exact complete declaration byte span.
    pub declaration_span: McpSpan,
}

/// Version-3 structured response for `code_search`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CodeSearchOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Search-profile version.
    pub query_profile: u16,
    /// Concrete source snapshot SHA-256.
    pub snapshot_sha256: String,
    /// Opaque active-generation identifier.
    pub generation: i64,
    /// Categorical material-result resolution.
    pub resolution: String,
    /// Domain-separated canonical query SHA-256.
    pub query_sha256: String,
    /// Number of returned matches.
    pub matches_returned: u64,
    /// Exact number of matches before result truncation.
    pub matches_total: u64,
    /// Explicit coverage categories.
    pub coverage: McpCoverage,
    /// Explicit result limitation.
    pub limitation: String,
    /// Deterministically ordered attributed matches.
    pub matches: Vec<McpSearchMatch>,
}

/// Exact selector echoed in a `symbol_get` response.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolSelectorOutput {
    /// Canonical byte-preserving repository path.
    pub path: String,
    /// Exact source-content SHA-256.
    pub content_sha256: String,
    /// Exact analysis-artifact SHA-256.
    pub artifact_sha256: String,
    /// Exact generation-local fact ordinal.
    pub fact_ordinal: u64,
}

/// One exact verified supported-language declaration.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSymbol {
    /// Producer-manifest SHA-256.
    pub producer_manifest_sha256: String,
    /// Evidence strength; currently `syntax`.
    pub evidence_tier: String,
    /// Persisted source language: `rust`, `go`, `typescript`, `tsx`, or `python`.
    pub language: String,
    /// Language-specific declaration kind.
    pub kind: String,
    /// Unqualified declaration name.
    pub name: String,
    /// Deterministic lexical qualified name.
    pub qualified_name: String,
    /// Exact declaration-name byte span.
    pub name_span: McpSpan,
    /// Exact complete declaration byte span.
    pub declaration_span: McpSpan,
    /// Exact declaration representation: `utf8` or `lowercase_hex`.
    pub declaration_encoding: String,
    /// Exact untrusted declaration bytes in the declared representation.
    pub declaration: String,
}

/// Version-4 structured response for `symbol_get`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolGetOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Exact-symbol profile version.
    pub symbol_profile: u16,
    /// Concrete source snapshot SHA-256.
    pub snapshot_sha256: String,
    /// Opaque active-generation identifier.
    pub generation: i64,
    /// Categorical material-result resolution.
    pub resolution: String,
    /// Exact requested occurrence selector.
    pub selector: SymbolSelectorOutput,
    /// Explicit coverage categories.
    pub coverage: McpCoverage,
    /// Explicit result limitation.
    pub limitation: String,
    /// Exact verified symbol, or `null` for an unresolved occurrence.
    pub symbol: Option<McpSymbol>,
}

fn validate_timeout(timeout_ms: Option<u64>) -> Result<Duration, &'static str> {
    let timeout_ms = timeout_ms.unwrap_or(DEFAULT_MCP_TIMEOUT_MS);
    if !(1..=MAX_MCP_TIMEOUT_MS).contains(&timeout_ms) {
        return Err("timeout_ms must be between 1 and 30000");
    }
    Ok(Duration::from_millis(timeout_ms))
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_canonical_path_text(value: &str) -> bool {
    RepositoryPathTextV1::decode(
        value,
        RepositoryPathTextByteLimit::new(MAX_PATH_TEXT_BYTES),
        RepositoryPathLimits::new(MAX_PATH_BYTES, MAX_PATH_COMPONENTS),
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_are_bounded_canonical_and_redacted() {
        let search: CodeSearchInput =
            serde_json::from_str(r#"{"query":"  alpha   beta  "}"#).expect("valid input");
        let request = search.validate().expect("valid request");
        assert_eq!(request.query(), "alpha beta");
        assert_eq!(request.max_results(), DEFAULT_CODE_SEARCH_RESULTS);
        assert_eq!(
            format!("{request:?}"),
            "CodeSearchServiceRequest { query: \"<redacted-query>\", max_results: 20, timeout: 5s }"
        );

        let digest = "ab".repeat(32);
        let symbol: SymbolGetInput = serde_json::from_value(serde_json::json!({
            "snapshot_sha256": digest,
            "generation": 7,
            "path": "rwp1:h:7372632F6C69622E7273",
            "content_sha256": "cd".repeat(32),
            "artifact_sha256": "ef".repeat(32),
            "fact_ordinal": 3,
        }))
        .expect("valid input");
        let request = symbol.validate().expect("valid selector");
        assert_eq!(request.generation(), 7);
        assert_eq!(request.fact_ordinal(), 3);
        let debug = format!("{request:?}");
        assert!(!debug.contains("737263"));
        assert!(!debug.contains(&digest));
    }

    #[test]
    fn unknown_fields_and_invalid_bounds_fail_before_service_construction() {
        assert!(
            serde_json::from_str::<CodeSearchInput>(r#"{"query":"run","repository":"/private"}"#)
                .is_err()
        );
        for value in [
            serde_json::json!({"query": ""}),
            serde_json::json!({"query": "run", "max_results": 0}),
            serde_json::json!({"query": "run", "max_results": 101}),
            serde_json::json!({"query": "run", "timeout_ms": 0}),
            serde_json::json!({"query": "run", "timeout_ms": 30001}),
        ] {
            let input: CodeSearchInput = serde_json::from_value(value).expect("wire shape");
            assert!(input.validate().is_err());
        }
    }

    #[test]
    fn symbol_selector_rejects_noncanonical_text() {
        let valid = || SymbolGetInput {
            snapshot_sha256: "11".repeat(32),
            generation: 1,
            path: "rwp1:h:7372632F6C69622E7273".to_owned(),
            content_sha256: "22".repeat(32),
            artifact_sha256: "aa".repeat(32),
            fact_ordinal: 0,
            timeout_ms: None,
        };

        let mut input = valid();
        input.generation = 0;
        assert!(input.validate().is_err());
        let mut input = valid();
        input.snapshot_sha256 = "AA".repeat(32);
        assert!(input.validate().is_err());
        let mut input = valid();
        input.path = "rwp1:h:aa".to_owned();
        assert!(input.validate().is_err());
        let mut input = valid();
        input.path = "rwp1:h:A".to_owned();
        assert!(input.validate().is_err());
        for path in [
            "rwp1:h:00",
            "rwp1:h:2F737263",
            "rwp1:h:7372632F2E2E2F6C69622E7273",
            "rwp1:h:7372632F2E6769742F636F6E666967",
        ] {
            let mut input = valid();
            input.path = path.to_owned();
            assert!(input.validate().is_err());
        }
        let mut input = valid();
        input.fact_ordinal = MAX_MCP_INTEROPERABLE_INTEGER + 1;
        assert!(input.validate().is_err());
    }
}
