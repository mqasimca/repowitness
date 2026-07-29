//! Strict native Rust graph tool contracts.

mod input;
mod output;

pub use input::{
    GraphArchitectureInput, GraphEvidenceInput, GraphImpactInput, GraphReadServiceRequest,
    GraphSearchInput, GraphStatusInput, GraphTraceInput, McpGraphDefinition, McpGraphSite,
};
pub use output::{
    GraphArchitectureOutput, GraphEvidenceOutput, GraphImpactOutput, GraphSearchOutput,
    GraphStatusOutput, GraphTraceOutput, McpGraphArchitectureCount, McpGraphCandidate,
    McpGraphCardinality, McpGraphContext, McpGraphEdge, McpGraphEvidence, McpGraphImpact,
    McpGraphPublication, McpGraphTrace, McpGraphTraceCoverage, McpGraphTraceTruncation,
};

/// Native graph publication-status tool.
pub const GRAPH_STATUS_TOOL_NAME: &str = "graph_status";
/// Native exact graph-definition search tool.
pub const GRAPH_SEARCH_TOOL_NAME: &str = "graph_search";
/// Native exact raw-site evidence tool.
pub const GRAPH_EVIDENCE_TOOL_NAME: &str = "graph_evidence";
/// Native count-only graph architecture tool.
pub const GRAPH_ARCHITECTURE_TOOL_NAME: &str = "graph_architecture";
/// Native deterministic bounded graph traversal tool.
pub const GRAPH_TRACE_TOOL_NAME: &str = "graph_trace";
/// Native conservative inbound impact tool.
pub const IMPACT_ANALYZE_TOOL_NAME: &str = "impact_analyze";

/// One operation-specific native graph response.
pub enum GraphReadServiceOutput {
    /// Graph publication status.
    Status(GraphStatusOutput),
    /// Exact symbol search.
    Search(GraphSearchOutput),
    /// Exact site evidence.
    Evidence(GraphEvidenceOutput),
    /// Count-only architecture summary.
    Architecture(GraphArchitectureOutput),
    /// Deterministic graph traversal.
    Trace(GraphTraceOutput),
    /// Conservative inbound impact.
    Impact(GraphImpactOutput),
}
