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

mod compatibility;
mod context_build;
mod diagnostics;
mod graph;
mod historical_memory;
mod memory_manage;
mod memory_mutation;
mod memory_recall;
mod personal_memory;
mod phase2_context;
mod repository_service_error;
mod scip_evidence;

pub use compatibility::{
    COMPATIBILITY_PROFILE_VERSION, CompatibilityGraphSchema, CompatibilityGraphSchemaLimits,
    CompatibilityLevels, CompatibilityNamespace, CompatibilityObservation, CompatibilityOutput,
    CompatibilityReceipt, GET_ARCHITECTURE_ALIAS_TOOL_NAME, GET_CODE_SNIPPET_ALIAS_TOOL_NAME,
    GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME, GetArchitectureInput, GetCodeSnippetInput,
    GetGraphSchemaInput, INCUMBENT_COMPATIBLE_PROFILE, INCUMBENT_COMPATIBLE_SURFACE,
    INDEX_STATUS_ALIAS_TOOL_NAME, IndexStatusInput, SEARCH_CODE_ALIAS_TOOL_NAME,
    SEARCH_GRAPH_ALIAS_TOOL_NAME, SearchCodeInput, SearchGraphInput, TRACE_PATH_ALIAS_TOOL_NAME,
    TracePathInput,
};
pub(crate) use compatibility::{CompatibilityAlias, compatibility_output, graph_schema_output};
pub use context_build::{
    ContextBuildInput, ContextBuildOutput, ContextBuildServiceRequest, McpContextCoverage,
    McpContextItem, McpContextMemoryItem, McpContextMemoryProjection, McpContextOmission,
    McpContextSourceItem,
};
pub use diagnostics::{
    DiagnosticsInput, DiagnosticsOutput, DiagnosticsServiceRequest, McpConfigurationIdentity,
    McpDiagnosticsMemoryProjection,
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
pub use historical_memory::{
    HISTORICAL_MEMORY_SCHEMA_VERSION, HistoricalMemoryApplicability, HistoricalMemoryCoverage,
    HistoricalMemoryEvidence, HistoricalMemoryEvidenceBasis, HistoricalMemoryInput,
    HistoricalMemoryOutput, HistoricalMemoryServiceRequest, HistoricalMemoryTarget,
    HistoricalMemoryTargetKind,
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
pub use personal_memory::{
    PERSONAL_MEMORY_SCHEMA_VERSION, PersonalMemoryInput, PersonalMemoryKind,
    PersonalMemoryLifecycle, PersonalMemoryOperation, PersonalMemoryOutput,
    PersonalMemoryRecordOutput, PersonalMemoryServiceRequest,
};
pub use phase2_context::{
    McpPhase2ContextAttribution, McpPhase2ContextItem, McpPhase2ContextOmission,
    McpPhase2ContextPayload, McpPhase2ContextProviderCoverage, McpPhase2ContextScope,
    Phase2ContextBuildInput, Phase2ContextBuildOutput, Phase2ContextBuildServiceRequest,
};
pub use repository_service_error::RepositoryServiceError;
pub use scip_evidence::{
    McpScipOccurrence, McpScipOverlay, McpScipRelationship, ScipEvidenceInput, ScipEvidenceOutput,
    ScipEvidenceServiceRequest,
};

/// MCP tool name for bounded lexical supported-language symbol search.
pub const CODE_SEARCH_TOOL_NAME: &str = "code_search";
/// MCP tool name for deterministic bounded context compilation.
pub const CONTEXT_BUILD_TOOL_NAME: &str = "context_build";
/// MCP tool name for the separate evidence-balanced Phase 2 context profile.
pub const PHASE2_CONTEXT_BUILD_TOOL_NAME: &str = "phase2_context_build";
/// MCP tool name for transactionally pinned read-only repository diagnostics.
pub const DIAGNOSTICS_TOOL_NAME: &str = "diagnostics";
/// MCP tool name for generation-pinned engineering-memory retrieval.
pub const MEMORY_RECALL_TOOL_NAME: &str = "memory_recall";
/// MCP tool name for explicitly authorized local engineering-memory mutation.
pub const MEMORY_MANAGE_TOOL_NAME: &str = "memory_manage";
/// MCP tool name for explicitly enabled local-profile personal memory.
pub const PERSONAL_MEMORY_TOOL_NAME: &str = "personal_memory";
/// MCP tool name for a bounded exact historical memory applicability receipt.
pub const HISTORICAL_MEMORY_TOOL_NAME: &str = "historical_memory";
/// MCP tool name for exact verified declaration retrieval.
pub const SYMBOL_GET_TOOL_NAME: &str = "symbol_get";
/// MCP tool name for immutable package-scoped SCIP symbol evidence.
pub const SCIP_EVIDENCE_TOOL_NAME: &str = "scip_evidence";

pub(crate) const DEFAULT_MCP_TIMEOUT_MS: u64 = 5_000;
pub(crate) const MAX_MCP_TIMEOUT_MS: u64 = 30_000;
const MAX_PATH_BYTES: u64 = 1_048_576;
const MAX_PATH_COMPONENTS: u64 = 1_048_576;
const MAX_PATH_TEXT_BYTES: u64 = 7 + (MAX_PATH_BYTES * 2);
/// Largest integer that is exact in every supported MCP JSON implementation.
pub const MAX_MCP_INTEROPERABLE_INTEGER: u64 = 9_007_199_254_740_991;
pub(crate) const MAX_MCP_SEARCH_OUTPUT_BYTES: usize = 3 * 1024 * 1024;
pub(crate) const MAX_MCP_CONTEXT_OUTPUT_BYTES: usize = 24 * 1024 * 1024;
pub(crate) const MAX_MCP_PHASE2_CONTEXT_OUTPUT_BYTES: usize = 24 * 1024 * 1024;
pub(crate) const MAX_MCP_DIAGNOSTICS_OUTPUT_BYTES: usize = 256 * 1024;
pub(crate) const MAX_MCP_GRAPH_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_MCP_MEMORY_RECALL_OUTPUT_BYTES: usize = 20 * 1024 * 1024;
pub(crate) const MAX_MCP_HISTORICAL_MEMORY_OUTPUT_BYTES: usize = 128 * 1024;
pub(crate) const MAX_MCP_MEMORY_MANAGE_OUTPUT_BYTES: usize = 64 * 1024;
/// Personal records are bounded to 4 KiB fields and reads to 100 records.
pub(crate) const MAX_MCP_PERSONAL_MEMORY_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
// The MCP SDK includes both structured JSON and a compatibility text copy.
// A bounded 10 MiB application payload can therefore require almost 60 MiB
// after exact source representation and nested JSON escaping.
pub(crate) const MAX_MCP_SYMBOL_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_MCP_SCIP_EVIDENCE_OUTPUT_BYTES: usize = 4 * 1024 * 1024;

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

/// Durable lifecycle state projected through negotiated native MCP Tasks.
///
/// The transport never derives this state from an ephemeral result payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeTaskState {
    /// Work remains active.
    Working,
    /// The operation completed successfully.
    Completed,
    /// The operation needs follow-up after a bounded failure.
    Failed,
    /// The caller cancelled the operation.
    Cancelled,
}

/// Polling-safe durable task projection for the MCP transport.
///
/// It excludes persisted task text and all captured output. `task_id` is an
/// opaque canonical durable task identity, not a user-controlled path or key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeTaskStatus {
    task_id: String,
    state: NativeTaskState,
    checkpoint_sequence: u32,
    verification_count: u32,
}

impl NativeTaskStatus {
    /// Creates one validated adapter-owned status projection.
    #[must_use]
    pub fn new(
        task_id: String,
        state: NativeTaskState,
        checkpoint_sequence: u32,
        verification_count: u32,
    ) -> Self {
        Self {
            task_id,
            state,
            checkpoint_sequence,
            verification_count,
        }
    }

    /// Returns the canonical opaque durable task identity.
    #[must_use]
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    /// Returns the lifecycle state supported by the MCP task protocol.
    #[must_use]
    pub const fn state(&self) -> NativeTaskState {
        self.state
    }

    /// Returns the last committed immutable checkpoint sequence.
    #[must_use]
    pub const fn checkpoint_sequence(&self) -> u32 {
        self.checkpoint_sequence
    }

    /// Returns the bounded number of verification receipts.
    #[must_use]
    pub const fn verification_count(&self) -> u32 {
        self.verification_count
    }
}

/// Synchronous repository operations injected by the CLI composition root.
///
/// Implementations must honor both the request timeout and cancellation flag.
/// They must return only bounded output DTOs and stable, redacted errors.
pub trait RepositoryService: Send + Sync + 'static {
    /// Creates the canonical durable engineering-task record backing one
    /// explicitly negotiated native MCP task. The returned opaque identifier
    /// is both the transport handle and the durable task identity text.
    fn native_task_start(
        &self,
        _objective: &str,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<NativeTaskStatus, RepositoryServiceError> {
        Err(RepositoryServiceError::NativeTask)
    }

    /// Appends one bounded lifecycle checkpoint to a durable native task.
    ///
    /// This is intentionally separate from the retained MCP result payload:
    /// the payload is ephemeral transport data, while this state survives a
    /// reconnect and remains subject to the engineering-task audit rules.
    fn native_task_transition(
        &self,
        _task_id: &str,
        _state: NativeTaskState,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<NativeTaskStatus, RepositoryServiceError> {
        Err(RepositoryServiceError::NativeTask)
    }

    /// Returns one durable native task in the configured repository scope.
    fn native_task_status(
        &self,
        _task_id: &str,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<Option<NativeTaskStatus>, RepositoryServiceError> {
        Err(RepositoryServiceError::NativeTask)
    }

    /// Lists the bounded most-recent durable native task records in the
    /// configured repository scope.
    fn native_task_list(
        &self,
        _limit: u16,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<Box<[NativeTaskStatus]>, RepositoryServiceError> {
        Err(RepositoryServiceError::NativeTask)
    }

    /// Runs one bounded lexical search.
    fn code_search(
        &self,
        request: CodeSearchServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<CodeSearchOutput, RepositoryServiceError>;

    /// Compiles one bounded evidence-bearing context pack.
    fn context_build(
        &self,
        request: ContextBuildServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<ContextBuildOutput, RepositoryServiceError>;

    /// Compiles one bounded evidence-balanced Phase 2 context pack.
    fn phase2_context_build(
        &self,
        _request: Phase2ContextBuildServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<Phase2ContextBuildOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::Phase2ContextBuild)
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

    /// Recalls bounded records from the complete active memory projection.
    fn memory_recall(
        &self,
        request: MemoryRecallServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<MemoryRecallOutput, RepositoryServiceError>;

    /// Reads a bounded exact historical applicability receipt. The repository
    /// scope is fixed at MCP startup and target paths are never accepted.
    fn historical_memory(
        &self,
        _request: HistoricalMemoryServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<HistoricalMemoryOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::HistoricalMemory)
    }

    /// Performs one explicitly authorized, path-confined memory mutation.
    fn memory_manage(
        &self,
        _request: MemoryManageServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<MemoryManageOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::MemoryManage)
    }

    /// Reads or appends local-only memory for the fixed startup profile.
    ///
    /// This method is unavailable unless the composition root opted into a
    /// single opaque local profile before the MCP runtime started.
    fn personal_memory(
        &self,
        _request: PersonalMemoryServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<PersonalMemoryOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::PersonalMemory)
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
