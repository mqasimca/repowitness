#![allow(
    missing_docs,
    reason = "public field names and enclosing comments form the versioned JSON schema"
)]

use schemars::JsonSchema;
use serde::Serialize;

use super::{McpGraphDefinition, McpGraphSite};

/// Exact immutable context and optional complete publication receipt.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGraphContext {
    /// Canonical connected-workspace identity.
    pub connected_workspace: String,
    /// Positive immutable workspace-view identity.
    pub workspace_view: i64,
    /// Positive graph-owning generation identity.
    pub graph_generation: i64,
    /// Complete graph receipt, absent only when status is `not_produced`.
    pub publication: Option<McpGraphPublication>,
}

/// Complete immutable graph publication receipt.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGraphPublication {
    pub resolver_profile: u32,
    pub input_sha256: String,
    pub output_sha256: String,
    pub source_count: u16,
    pub artifact_count: u64,
    pub definition_count: u64,
    pub site_count: u64,
    pub unresolved_count: u64,
    pub unique_count: u64,
    pub ambiguous_count: u64,
    pub unsupported_count: u64,
    pub truncated_site_count: u64,
    pub retained_candidate_count: u64,
    pub edge_count: u64,
    pub input_text_bytes: u64,
    pub output_bytes: u64,
    pub syntax_error_nodes: u64,
    pub macro_sites: u64,
    pub test_marker_sites: u64,
    pub heuristic_sites: u64,
}

/// Version-1 output for `graph_status`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphStatusOutput {
    pub schema_version: u16,
    pub context: McpGraphContext,
    /// `complete` or `not_produced`.
    pub availability: String,
}

/// Version-1 output for `graph_search`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphSearchOutput {
    pub schema_version: u16,
    pub context: McpGraphContext,
    pub matches_returned: u64,
    pub matches_total: u64,
    pub truncated: bool,
    pub output_bytes: u64,
    pub definitions: Vec<McpGraphDefinition>,
}

/// One exact retained candidate and its attributed resolution evidence.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGraphCandidate {
    pub target: McpGraphDefinition,
    pub resolution_evidence: String,
}

/// Complete exact raw-site evidence without source or raw-target bytes.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGraphEvidence {
    pub site: McpGraphSite,
    pub content_sha256: String,
    pub extraction_evidence: String,
    /// `unresolved`, `unique`, or `ambiguous`.
    pub outcome: String,
    pub unresolved_reason: Option<String>,
    pub candidate_count: u32,
    pub candidates_truncated: bool,
    pub candidates: Vec<McpGraphCandidate>,
}

/// Version-1 output for `graph_evidence`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphEvidenceOutput {
    pub schema_version: u16,
    pub context: McpGraphContext,
    pub found: bool,
    pub evidence: Option<McpGraphEvidence>,
}

/// One stable kind/count pair in a count-only architecture summary.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGraphArchitectureCount {
    pub kind: String,
    pub count: u64,
}

/// Version-1 output for `graph_architecture`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphArchitectureOutput {
    pub schema_version: u16,
    pub context: McpGraphContext,
    pub definitions_by_kind: Vec<McpGraphArchitectureCount>,
    pub edges_by_kind: Vec<McpGraphArchitectureCount>,
}

/// Exact unique or ambiguous candidate cardinality.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGraphCardinality {
    /// `unique` or `ambiguous`.
    pub kind: String,
    pub candidate_count: u32,
    pub retained_candidates: u32,
    pub candidates_truncated: bool,
}

/// One deterministic trace relationship with complete identity and evidence.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGraphEdge {
    pub depth: u32,
    pub edge_kind: String,
    pub extraction_evidence: String,
    pub resolution_evidence: String,
    pub cardinality: McpGraphCardinality,
    pub site: McpGraphSite,
    pub source: McpGraphDefinition,
    pub target: McpGraphDefinition,
}

/// Independent resource bounds that stopped pending traversal work.
#[derive(Clone, Copy, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGraphTraceTruncation {
    pub depth: bool,
    pub visited_nodes: bool,
    pub visited_edges: bool,
    pub frontier: bool,
    pub results: bool,
}

/// Generation-level graph limitations not representable as relationships.
#[derive(Clone, Copy, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGraphTraceCoverage {
    pub unresolved_sites: u64,
    pub unsupported_sites: u64,
    pub ambiguous_sites: u64,
    pub truncated_sites: u64,
    pub unlinked_sites: u64,
    pub macro_sites: u64,
    pub conditional_sites: u64,
    pub heuristic_sites: u64,
}

/// Complete deterministic bounded traversal.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGraphTrace {
    pub edges: Vec<McpGraphEdge>,
    pub visited_nodes: u64,
    pub visited_edges: u64,
    pub maximum_completed_depth: u32,
    pub truncation: McpGraphTraceTruncation,
    pub coverage: McpGraphTraceCoverage,
    pub input_bytes: u64,
    pub output_bytes: u64,
}

/// Version-1 output for `graph_trace`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphTraceOutput {
    pub schema_version: u16,
    pub context: McpGraphContext,
    pub trace: McpGraphTrace,
}

/// One conservatively classified impacted declaration.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGraphImpact {
    /// `directly_connected`, `possible`, or `unknown`.
    pub class: String,
    pub definition: McpGraphDefinition,
    pub minimum_depth: u32,
}

/// Version-1 output for `impact_analyze`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphImpactOutput {
    pub schema_version: u16,
    pub context: McpGraphContext,
    pub trace: McpGraphTrace,
    pub impacts: Vec<McpGraphImpact>,
    pub unknown_coverage: bool,
    pub output_bytes: u64,
}
