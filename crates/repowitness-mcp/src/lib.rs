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
};
pub use transport::{BoundedLineReader, BoundedLineReaderLimitError, MAX_MCP_INPUT_LINE_BYTES};
pub use wire::{
    CODE_SEARCH_TOOL_NAME, CodeSearchInput, CodeSearchOutput, CodeSearchServiceRequest,
    McpCoverage, McpSearchMatch, McpSpan, McpSymbol, RepositoryService, RepositoryServiceError,
    SYMBOL_GET_TOOL_NAME, SymbolGetInput, SymbolGetOutput, SymbolGetServiceRequest,
    SymbolSelectorOutput,
};
