//! Independently authored, bounded compatibility aliases over native use cases.

mod input;
mod output;

pub use input::{
    GetArchitectureInput, GetCodeSnippetInput, GetGraphSchemaInput, IndexStatusInput,
    SearchCodeInput, SearchGraphInput, TracePathInput,
};
pub(crate) use output::{CompatibilityAlias, compatibility_output, graph_schema_output};
pub use output::{
    CompatibilityGraphSchema, CompatibilityGraphSchemaLimits, CompatibilityLevels,
    CompatibilityNamespace, CompatibilityOutput, CompatibilityReceipt,
};

/// Version of the independently authored compatibility request/receipt profile.
pub const COMPATIBILITY_PROFILE_VERSION: u16 = 1;
/// Opt-in resolved configuration spelling.
pub const INCUMBENT_COMPATIBLE_PROFILE: &str = "incumbent-compatible";
/// Fixed MCP surface identifier for the first compatibility subset.
pub const INCUMBENT_COMPATIBLE_SURFACE: &str = "native-v1-plus-incumbent-subset-v1";

/// Bounded literal source-search alias.
pub const SEARCH_CODE_ALIAS_TOOL_NAME: &str = "search_code";
/// Exact digest-verified source retrieval alias.
pub const GET_CODE_SNIPPET_ALIAS_TOOL_NAME: &str = "get_code_snippet";
/// Exact Rust graph-definition search alias.
pub const SEARCH_GRAPH_ALIAS_TOOL_NAME: &str = "search_graph";
/// Exact-selector bounded Rust graph traversal alias.
pub const TRACE_PATH_ALIAS_TOOL_NAME: &str = "trace_path";
/// Versioned Rust graph capability/schema alias.
pub const GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME: &str = "get_graph_schema";
/// Count-only Rust graph architecture alias.
pub const GET_ARCHITECTURE_ALIAS_TOOL_NAME: &str = "get_architecture";
/// Active immutable repository diagnostics alias.
pub const INDEX_STATUS_ALIAS_TOOL_NAME: &str = "index_status";

#[cfg(test)]
mod tests;
