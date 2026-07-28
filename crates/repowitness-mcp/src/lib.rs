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
    DEFAULT_MCP_OPERATION_CONCURRENCY, McpServeError, RepoWitnessMcpServer, serve_stdio,
    serve_stdio_with_memory_writes,
};
pub use transport::{BoundedLineReader, BoundedLineReaderLimitError, MAX_MCP_INPUT_LINE_BYTES};
pub use wire::{
    CODE_SEARCH_TOOL_NAME, CONTEXT_BUILD_TOOL_NAME, CodeSearchInput, CodeSearchOutput,
    CodeSearchServiceRequest, ContextBuildInput, ContextBuildOutput, ContextBuildServiceRequest,
    DIAGNOSTICS_TOOL_NAME, DiagnosticsInput, DiagnosticsOutput, DiagnosticsServiceRequest,
    MAX_MCP_INTEROPERABLE_INTEGER, MEMORY_MANAGE_TOOL_NAME, MEMORY_RECALL_TOOL_NAME,
    McpContextCoverage, McpContextItem, McpContextMemoryItem, McpContextMemoryProjection,
    McpContextOmission, McpContextSourceItem, McpCoverage, McpDiagnosticsMemoryProjection,
    McpMemoryCandidate, McpMemoryCoverage, McpMemoryEvidence, McpMemoryOccurrence,
    McpMemoryProducer, McpMemoryRecord, McpMemoryTarget, McpSearchMatch, McpSelectedMemory,
    McpSpan, McpSymbol, MemoryManageInput, MemoryManageOperation, MemoryManageOutput,
    MemoryManageReceipt, MemoryManageReviewDecision, MemoryManageServiceRequest, MemoryRecallInput,
    MemoryRecallOutput, MemoryRecallServiceRequest, MemoryRecallServiceSelection,
    RepositoryService, RepositoryServiceError, SYMBOL_GET_TOOL_NAME, SymbolGetInput,
    SymbolGetOutput, SymbolGetServiceRequest, SymbolSelectorOutput,
};
