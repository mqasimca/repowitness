//! Bounded local MCP transport, versioned wire DTOs, capability negotiation,
//! and protocol-error mapping over RepoWitness application use cases.
//!
//! This package deliberately has no Git, filesystem, or SQLite dependency.
//! The CLI composition root injects those concrete operations through
//! [`RepositoryService`].

mod server;
mod transport;
mod wire;

pub use server::{
    DEFAULT_MCP_OPERATION_CONCURRENCY, McpServeError, McpToolSurface, RepoWitnessMcpServer,
    serve_stdio, serve_stdio_with_memory_writes, serve_stdio_with_surface,
};
pub use transport::{BoundedLineReader, BoundedLineReaderLimitError, MAX_MCP_INPUT_LINE_BYTES};
pub use wire::{
    CODE_SEARCH_TOOL_NAME, COMPATIBILITY_PROFILE_VERSION, CONTEXT_BUILD_TOOL_NAME, CodeSearchInput,
    CodeSearchOutput, CodeSearchServiceRequest, CompatibilityGraphSchema,
    CompatibilityGraphSchemaLimits, CompatibilityLevels, CompatibilityNamespace,
    CompatibilityOutput, CompatibilityReceipt, ContextBuildInput, ContextBuildOutput,
    ContextBuildServiceRequest, DIAGNOSTICS_TOOL_NAME, DiagnosticsInput, DiagnosticsOutput,
    DiagnosticsServiceRequest, GET_ARCHITECTURE_ALIAS_TOOL_NAME, GET_CODE_SNIPPET_ALIAS_TOOL_NAME,
    GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME, GRAPH_ARCHITECTURE_TOOL_NAME, GRAPH_EVIDENCE_TOOL_NAME,
    GRAPH_SEARCH_TOOL_NAME, GRAPH_STATUS_TOOL_NAME, GRAPH_TRACE_TOOL_NAME, GetArchitectureInput,
    GetCodeSnippetInput, GetGraphSchemaInput, GraphArchitectureInput, GraphArchitectureOutput,
    GraphEvidenceInput, GraphEvidenceOutput, GraphImpactInput, GraphImpactOutput,
    GraphReadServiceOutput, GraphReadServiceRequest, GraphSearchInput, GraphSearchOutput,
    GraphStatusInput, GraphStatusOutput, GraphTraceInput, GraphTraceOutput,
    IMPACT_ANALYZE_TOOL_NAME, INCUMBENT_COMPATIBLE_PROFILE, INCUMBENT_COMPATIBLE_SURFACE,
    INDEX_STATUS_ALIAS_TOOL_NAME, IndexStatusInput, MAX_MCP_INTEROPERABLE_INTEGER,
    MEMORY_MANAGE_TOOL_NAME, MEMORY_RECALL_TOOL_NAME, McpConfigurationIdentity, McpContextCoverage,
    McpContextItem, McpContextMemoryItem, McpContextMemoryProjection, McpContextOmission,
    McpContextSourceItem, McpCoverage, McpDiagnosticsMemoryProjection, McpGraphArchitectureCount,
    McpGraphCandidate, McpGraphCardinality, McpGraphContext, McpGraphDefinition, McpGraphEdge,
    McpGraphEvidence, McpGraphImpact, McpGraphPublication, McpGraphSite, McpGraphTrace,
    McpGraphTraceCoverage, McpGraphTraceTruncation, McpMemoryCandidate, McpMemoryCoverage,
    McpMemoryEvidence, McpMemoryOccurrence, McpMemoryProducer, McpMemoryRecord, McpMemoryTarget,
    McpSearchMatch, McpSelectedMemory, McpSpan, McpSymbol, MemoryManageFileIdentityStatus,
    MemoryManageInput, MemoryManageOperation, MemoryManageOutput, MemoryManagePublicationStatus,
    MemoryManagePublicationStepStatus, MemoryManageReceipt, MemoryManageReviewDecision,
    MemoryManageServiceRequest, MemoryRecallInput, MemoryRecallOutput, MemoryRecallServiceRequest,
    MemoryRecallServiceSelection, RepositoryService, RepositoryServiceError,
    SEARCH_CODE_ALIAS_TOOL_NAME, SEARCH_GRAPH_ALIAS_TOOL_NAME, SYMBOL_GET_TOOL_NAME,
    SearchCodeInput, SearchGraphInput, SymbolGetInput, SymbolGetOutput, SymbolGetServiceRequest,
    SymbolSelectorOutput, TRACE_PATH_ALIAS_TOOL_NAME, TracePathInput,
};
