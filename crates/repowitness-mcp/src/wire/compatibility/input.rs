#![allow(
    missing_docs,
    reason = "public field names and enclosing comments form the versioned JSON schema"
)]

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::super::{
    CodeSearchInput, CodeSearchServiceRequest, DiagnosticsInput, DiagnosticsServiceRequest,
    GraphArchitectureInput, GraphReadServiceRequest, GraphSearchInput, GraphStatusInput,
    GraphTraceInput, McpGraphDefinition, McpGraphSite, SymbolGetInput, SymbolGetServiceRequest,
};

const COMPATIBILITY_MAX_GRAPH_DEPTH: u32 = 5;

#[derive(Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityGraphSelection {
    workspace_view: Option<i64>,
    graph_generation: Option<i64>,
}

#[derive(Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityGraphResultLimits {
    max_results: Option<u32>,
    max_output_bytes: Option<u64>,
}

#[derive(Default, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityTraceLimits {
    max_input_edges: Option<u64>,
    max_input_bytes: Option<u64>,
    max_depth: Option<u32>,
    max_results: Option<u32>,
    max_visited_nodes: Option<u64>,
    max_visited_edges: Option<u64>,
    max_frontier: Option<u64>,
    max_output_bytes: Option<u64>,
}

/// Strict version-1 input for `search_code`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchCodeInput {
    pub query: String,
    pub max_results: Option<u16>,
    pub timeout_ms: Option<u64>,
}

impl SearchCodeInput {
    pub(crate) fn validate(self) -> Result<CodeSearchServiceRequest, &'static str> {
        CodeSearchInput {
            query: self.query,
            max_results: self.max_results,
            timeout_ms: self.timeout_ms,
        }
        .validate()
    }
}

impl fmt::Debug for SearchCodeInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchCodeInput")
            .field("query", &"<redacted-query>")
            .field("max_results", &self.max_results)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

/// Strict exact-selector version-1 input for `get_code_snippet`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetCodeSnippetInput {
    pub snapshot_sha256: String,
    pub generation: i64,
    pub path: String,
    pub content_sha256: String,
    pub artifact_sha256: String,
    pub fact_ordinal: u64,
    pub timeout_ms: Option<u64>,
}

impl GetCodeSnippetInput {
    pub(crate) fn validate(self) -> Result<SymbolGetServiceRequest, &'static str> {
        SymbolGetInput {
            snapshot_sha256: self.snapshot_sha256,
            generation: self.generation,
            path: self.path,
            content_sha256: self.content_sha256,
            artifact_sha256: self.artifact_sha256,
            fact_ordinal: self.fact_ordinal,
            timeout_ms: self.timeout_ms,
        }
        .validate()
    }
}

impl fmt::Debug for GetCodeSnippetInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GetCodeSnippetInput")
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

/// Strict exact-name version-1 input for `search_graph`.
#[derive(Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchGraphInput {
    #[serde(flatten)]
    selection: CompatibilityGraphSelection,
    query: String,
    #[serde(flatten)]
    limits: CompatibilityGraphResultLimits,
    timeout_ms: Option<u64>,
}

impl SearchGraphInput {
    pub(crate) fn validate(self) -> Result<GraphReadServiceRequest, &'static str> {
        convert_graph_input::<_, GraphSearchInput>(self)?.validate()
    }
}

impl fmt::Debug for SearchGraphInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchGraphInput")
            .field("query", &"<redacted-query>")
            .field("workspace_view", &self.selection.workspace_view)
            .field("graph_generation", &self.selection.graph_generation)
            .field("max_results", &self.limits.max_results)
            .field("max_output_bytes", &self.limits.max_output_bytes)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

#[derive(Deserialize, JsonSchema, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum CompatibilityTraceStart {
    Definition { definition: McpGraphDefinition },
    Site { site: McpGraphSite },
}

#[derive(Clone, Copy, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
enum CompatibilityDirection {
    Outbound,
    Inbound,
}

#[derive(Clone, Copy, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
enum CompatibilityEdgeKind {
    Import,
    Reference,
    Call,
}

/// Strict exact-selector version-1 input for `trace_path`.
#[derive(Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TracePathInput {
    #[serde(flatten)]
    selection: CompatibilityGraphSelection,
    start: CompatibilityTraceStart,
    direction: CompatibilityDirection,
    edge_kinds: Vec<CompatibilityEdgeKind>,
    #[serde(flatten)]
    limits: CompatibilityTraceLimits,
    timeout_ms: Option<u64>,
}

impl TracePathInput {
    pub(crate) fn validate(mut self) -> Result<GraphReadServiceRequest, &'static str> {
        let depth = self
            .limits
            .max_depth
            .unwrap_or(COMPATIBILITY_MAX_GRAPH_DEPTH);
        if !(1..=COMPATIBILITY_MAX_GRAPH_DEPTH).contains(&depth) {
            return Err("max_depth must be between 1 and 5");
        }
        if !(1..=3).contains(&self.edge_kinds.len()) {
            return Err("edge_kinds must contain between one and three kinds");
        }
        self.limits.max_depth = Some(depth);
        convert_graph_input::<_, GraphTraceInput>(self)?.validate()
    }
}

impl fmt::Debug for TracePathInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TracePathInput")
            .field("start", &"<redacted-selector>")
            .field("workspace_view", &self.selection.workspace_view)
            .field("graph_generation", &self.selection.graph_generation)
            .field("max_depth", &self.limits.max_depth)
            .field("edge_kind_count", &self.edge_kinds.len())
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

/// Strict version-1 input for `get_graph_schema`.
#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GetGraphSchemaInput {
    #[serde(flatten)]
    selection: CompatibilityGraphSelection,
    timeout_ms: Option<u64>,
}

impl GetGraphSchemaInput {
    pub(crate) fn validate(self) -> Result<GraphReadServiceRequest, &'static str> {
        convert_graph_input::<_, GraphStatusInput>(self)?.validate()
    }
}

/// Strict count-only version-1 input for `get_architecture`.
#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GetArchitectureInput {
    #[serde(flatten)]
    selection: CompatibilityGraphSelection,
    #[serde(flatten)]
    limits: CompatibilityGraphResultLimits,
    timeout_ms: Option<u64>,
}

impl GetArchitectureInput {
    pub(crate) fn validate(self) -> Result<GraphReadServiceRequest, &'static str> {
        convert_graph_input::<_, GraphArchitectureInput>(self)?.validate()
    }
}

/// Strict version-1 input for `index_status`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IndexStatusInput {
    pub timeout_ms: Option<u64>,
}

impl IndexStatusInput {
    pub(crate) fn validate(self) -> Result<DiagnosticsServiceRequest, &'static str> {
        DiagnosticsInput {
            timeout_ms: self.timeout_ms,
        }
        .validate()
    }
}

fn convert_graph_input<T, U>(input: T) -> Result<U, &'static str>
where
    T: Serialize,
    U: DeserializeOwned,
{
    let value =
        serde_json::to_value(input).map_err(|_| "compatibility request conversion failed")?;
    serde_json::from_value(value).map_err(|_| "compatibility request is outside the shared subset")
}
