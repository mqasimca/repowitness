use std::{fmt, time::Duration};

use repowitness_application::{
    AnalysisArtifactDigest, ByteOffset, ByteSpan, RepositoryPathLimits,
    RepositoryPathTextByteLimit, RepositoryPathTextV1, RustGraphDefinitionSelector,
    RustGraphEdgeKinds, RustGraphReadOperation, RustGraphSiteKind, RustGraphSiteSelector,
    RustGraphSymbolQuery, RustGraphTraceDirection, RustGraphTraceLimits,
    RustGraphTraceStartSelector, RustSymbolKind, SourceContentDigest, SourceSlotIdTextV1,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::super::{
    MAX_MCP_INTEROPERABLE_INTEGER, MAX_MCP_TIMEOUT_MS, MAX_PATH_BYTES, MAX_PATH_COMPONENTS,
    MAX_PATH_TEXT_BYTES,
};

// Traversal loads the complete immutable relationship set before applying its
// output and frontier bounds. Keep the default high enough for RepoWitness's
// own complete graph, otherwise a small requested trace fails before the
// traversal can report its bounded result.
const DEFAULT_INPUT_EDGES: u64 = 200_000;
const DEFAULT_INPUT_BYTES: u64 = 256 * 1024 * 1024;
const DEFAULT_DEPTH: u32 = 8;
const DEFAULT_RESULTS: u32 = 100;
const DEFAULT_VISITED_NODES: u64 = 10_000;
const DEFAULT_VISITED_EDGES: u64 = 200_000;
const DEFAULT_FRONTIER: u64 = 10_000;
const DEFAULT_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_REQUESTED_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
const DEFAULT_GRAPH_TIMEOUT_MS: u64 = 30_000;

#[derive(Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GraphSelectionInput {
    /// Exact workspace view from a prior graph response; pair with `graph_generation`.
    workspace_view: Option<i64>,
    /// Exact graph generation from a prior graph response; pair with `workspace_view`.
    graph_generation: Option<i64>,
}

impl GraphSelectionInput {
    fn validate(&self) -> Result<Option<(i64, i64)>, &'static str> {
        match (self.workspace_view, self.graph_generation) {
            (None, None) => Ok(None),
            (Some(view), Some(generation))
                if view > 0
                    && generation > 0
                    && u64::try_from(view).ok() <= Some(MAX_MCP_INTEROPERABLE_INTEGER)
                    && u64::try_from(generation).ok() <= Some(MAX_MCP_INTEROPERABLE_INTEGER) =>
            {
                Ok(Some((view, generation)))
            }
            _ => Err(
                "workspace_view and graph_generation must be supplied together as positive identifiers",
            ),
        }
    }
}

#[derive(Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GraphLimitsInput {
    /// Maximum complete relationship rows admitted before traversal.
    max_input_edges: Option<u64>,
    /// Maximum conservative relationship input bytes.
    max_input_bytes: Option<u64>,
    /// Maximum traversal depth.
    max_depth: Option<u32>,
    /// Maximum returned definitions, relationships, or impacts.
    max_results: Option<u32>,
    /// Maximum distinct visited definitions.
    max_visited_nodes: Option<u64>,
    /// Maximum examined relationships.
    max_visited_edges: Option<u64>,
    /// Maximum pending traversal frontier.
    max_frontier: Option<u64>,
    /// Maximum conservative operation output bytes.
    max_output_bytes: Option<u64>,
}

impl GraphLimitsInput {
    fn validate(&self) -> Result<RustGraphTraceLimits, &'static str> {
        let values = [
            self.max_input_edges.unwrap_or(DEFAULT_INPUT_EDGES),
            self.max_input_bytes.unwrap_or(DEFAULT_INPUT_BYTES),
            u64::from(self.max_depth.unwrap_or(DEFAULT_DEPTH)),
            u64::from(self.max_results.unwrap_or(DEFAULT_RESULTS)),
            self.max_visited_nodes.unwrap_or(DEFAULT_VISITED_NODES),
            self.max_visited_edges.unwrap_or(DEFAULT_VISITED_EDGES),
            self.max_frontier.unwrap_or(DEFAULT_FRONTIER),
            self.max_output_bytes.unwrap_or(DEFAULT_OUTPUT_BYTES),
        ];
        if values
            .into_iter()
            .any(|value| value > MAX_MCP_INTEROPERABLE_INTEGER)
        {
            return Err("graph limits exceed the interoperable integer range");
        }
        if values[7] > MAX_REQUESTED_OUTPUT_BYTES {
            return Err("max_output_bytes exceeds the MCP graph response budget");
        }
        RustGraphTraceLimits::try_new(
            values[0],
            values[1],
            u32::try_from(values[2]).map_err(|_| "graph limits are invalid")?,
            u32::try_from(values[3]).map_err(|_| "graph limits are invalid")?,
            values[4],
            values[5],
            values[6],
            values[7],
        )
        .map_err(|_| "graph limits are zero or exceed compiled ceilings")
    }
}

/// Exact declaration selector echoed by graph search and traversal.
#[derive(Clone, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGraphDefinition {
    /// Canonical source-slot identity.
    pub source_slot: String,
    /// Positive immutable generation for that source slot.
    pub source_generation: i64,
    /// Canonical byte-preserving repository path.
    pub path: String,
    /// Exact source-content SHA-256.
    pub content_sha256: String,
    /// Exact graph/declaration artifact SHA-256.
    pub artifact_sha256: String,
    /// Exact artifact-local declaration ordinal.
    pub fact_ordinal: u64,
    /// Stable declaration kind.
    pub symbol_kind: String,
    /// Exact unqualified declaration name.
    pub name: String,
    /// Deterministic syntax-qualified name.
    pub qualified_name: String,
    /// Exact declaration-name span.
    pub name_span: super::super::McpSpan,
    /// Exact complete declaration span.
    pub declaration_span: super::super::McpSpan,
}

impl fmt::Debug for McpGraphDefinition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("McpGraphDefinition")
            .field("source_slot", &"<redacted-identity>")
            .field("source_generation", &self.source_generation)
            .field("path", &"<redacted-path>")
            .field("content_sha256", &"<redacted-digest>")
            .field("artifact_sha256", &"<redacted-digest>")
            .field("fact_ordinal", &self.fact_ordinal)
            .field("symbol_kind", &self.symbol_kind)
            .field("name", &"<redacted>")
            .field("qualified_name", &"<redacted>")
            .field("name_span", &self.name_span)
            .field("declaration_span", &self.declaration_span)
            .finish()
    }
}

impl McpGraphDefinition {
    fn validate(self) -> Result<RustGraphDefinitionSelector, &'static str> {
        if self.fact_ordinal > MAX_MCP_INTEROPERABLE_INTEGER
            || u64::try_from(self.source_generation).ok() > Some(MAX_MCP_INTEROPERABLE_INTEGER)
        {
            return Err("definition identity exceeds the interoperable integer range");
        }
        let source_slot = SourceSlotIdTextV1::decode(&self.source_slot)
            .map_err(|_| "source_slot must be canonical ssi1:h: text")?;
        let path = decode_path(&self.path)?;
        let content = SourceContentDigest::new(decode_sha256(&self.content_sha256)?);
        let artifact = AnalysisArtifactDigest::new(decode_sha256(&self.artifact_sha256)?);
        let kind = RustSymbolKind::from_stable_str(&self.symbol_kind)
            .ok_or("symbol_kind is unsupported")?;
        RustGraphDefinitionSelector::try_new(
            source_slot,
            self.source_generation,
            path,
            content,
            artifact,
            self.fact_ordinal,
            kind,
            self.name,
            self.qualified_name,
            span(self.name_span)?,
            span(self.declaration_span)?,
        )
        .map_err(|_| "definition selector is inconsistent")
    }
}

/// Exact raw graph-site selector echoed by evidence and traversal.
#[derive(Clone, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGraphSite {
    /// Canonical source-slot identity.
    pub source_slot: String,
    /// Canonical byte-preserving repository path.
    pub path: String,
    /// Exact graph-site artifact SHA-256.
    pub artifact_sha256: String,
    /// Exact artifact-local site ordinal.
    pub ordinal: u32,
    /// Stable raw-site kind.
    pub site_kind: String,
    /// Exact enclosing construct span.
    pub occurrence_span: super::super::McpSpan,
    /// Exact target spelling span.
    pub target_span: super::super::McpSpan,
}

impl fmt::Debug for McpGraphSite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("McpGraphSite")
            .field("source_slot", &"<redacted-identity>")
            .field("path", &"<redacted-path>")
            .field("artifact_sha256", &"<redacted-digest>")
            .field("ordinal", &self.ordinal)
            .field("site_kind", &self.site_kind)
            .field("occurrence_span", &self.occurrence_span)
            .field("target_span", &self.target_span)
            .finish()
    }
}

impl McpGraphSite {
    fn validate(self) -> Result<RustGraphSiteSelector, &'static str> {
        let source_slot = SourceSlotIdTextV1::decode(&self.source_slot)
            .map_err(|_| "source_slot must be canonical ssi1:h: text")?;
        let path = decode_path(&self.path)?;
        let artifact = AnalysisArtifactDigest::new(decode_sha256(&self.artifact_sha256)?);
        let kind = RustGraphSiteKind::from_stable_str(&self.site_kind)
            .ok_or("site_kind is unsupported")?;
        RustGraphSiteSelector::try_new(
            source_slot,
            path,
            artifact,
            self.ordinal,
            kind,
            span(self.occurrence_span)?,
            span(self.target_span)?,
        )
        .map_err(|_| "site selector is inconsistent")
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum GraphTraceStartInput {
    Definition { definition: McpGraphDefinition },
    Site { site: McpGraphSite },
}

impl GraphTraceStartInput {
    fn validate(self) -> Result<RustGraphTraceStartSelector, &'static str> {
        match self {
            Self::Definition { definition } => definition
                .validate()
                .map(RustGraphTraceStartSelector::Definition),
            Self::Site { site } => site.validate().map(RustGraphTraceStartSelector::Site),
        }
    }
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum GraphDirectionInput {
    Outbound,
    Inbound,
}

impl GraphDirectionInput {
    const fn into_application(self) -> RustGraphTraceDirection {
        match self {
            Self::Outbound => RustGraphTraceDirection::Outbound,
            Self::Inbound => RustGraphTraceDirection::Inbound,
        }
    }
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum GraphEdgeKindInput {
    Import,
    Reference,
    Call,
}

fn edge_kinds(values: Vec<GraphEdgeKindInput>) -> Result<RustGraphEdgeKinds, &'static str> {
    let mut import = false;
    let mut reference = false;
    let mut call = false;
    for value in values {
        let slot = match value {
            GraphEdgeKindInput::Import => &mut import,
            GraphEdgeKindInput::Reference => &mut reference,
            GraphEdgeKindInput::Call => &mut call,
        };
        if *slot {
            return Err("edge_kinds must not contain duplicates");
        }
        *slot = true;
    }
    RustGraphEdgeKinds::try_new(import, reference, call)
        .map_err(|_| "edge_kinds must contain at least one supported kind")
}

/// Input for `graph_status`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphStatusInput {
    #[serde(flatten)]
    selection: GraphSelectionInput,
    /// End-to-end deadline in milliseconds.
    timeout_ms: Option<u64>,
}

/// Input for `graph_search`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphSearchInput {
    #[serde(flatten)]
    selection: GraphSelectionInput,
    /// Literal exact-name or qualified-name text.
    query: String,
    #[serde(flatten)]
    limits: GraphLimitsInput,
    /// End-to-end deadline in milliseconds.
    timeout_ms: Option<u64>,
}

/// Input for `graph_evidence`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphEvidenceInput {
    #[serde(flatten)]
    selection: GraphSelectionInput,
    /// Exact site returned by graph evidence or traversal.
    site: McpGraphSite,
    #[serde(flatten)]
    limits: GraphLimitsInput,
    /// End-to-end deadline in milliseconds.
    timeout_ms: Option<u64>,
}

/// Input for `graph_architecture`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphArchitectureInput {
    #[serde(flatten)]
    selection: GraphSelectionInput,
    #[serde(flatten)]
    limits: GraphLimitsInput,
    /// End-to-end deadline in milliseconds.
    timeout_ms: Option<u64>,
}

/// Input for `graph_trace`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphTraceInput {
    #[serde(flatten)]
    selection: GraphSelectionInput,
    /// Exact declaration or raw-site start.
    start: GraphTraceStartInput,
    /// Explicit traversal direction.
    direction: GraphDirectionInput,
    /// Non-empty allow-list using `import`, `reference`, and `call`.
    edge_kinds: Vec<GraphEdgeKindInput>,
    #[serde(flatten)]
    limits: GraphLimitsInput,
    /// End-to-end deadline in milliseconds.
    timeout_ms: Option<u64>,
}

/// Input for `impact_analyze`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphImpactInput {
    #[serde(flatten)]
    selection: GraphSelectionInput,
    /// Exact declaration whose inbound impact is requested.
    start: McpGraphDefinition,
    /// Non-empty allow-list using `import`, `reference`, and `call`.
    edge_kinds: Vec<GraphEdgeKindInput>,
    #[serde(flatten)]
    limits: GraphLimitsInput,
    /// End-to-end deadline in milliseconds.
    timeout_ms: Option<u64>,
}

/// Validated graph request passed to the fixed local composition root.
pub struct GraphReadServiceRequest {
    exact_pin: Option<(i64, i64)>,
    operation: RustGraphReadOperation,
    timeout: Duration,
}

impl GraphReadServiceRequest {
    /// Returns the exact immutable pin when the caller supplied one.
    #[must_use]
    pub const fn exact_pin(&self) -> Option<(i64, i64)> {
        self.exact_pin
    }

    /// Returns the end-to-end deadline duration.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Consumes the request and returns the validated operation.
    #[must_use]
    pub fn into_operation(self) -> RustGraphReadOperation {
        self.operation
    }

    pub(crate) const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl fmt::Debug for GraphReadServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GraphReadServiceRequest")
            .field("exact_pin", &self.exact_pin)
            .field("operation", &operation_label(&self.operation))
            .field("timeout", &self.timeout)
            .finish()
    }
}

fn validate_graph_timeout(timeout_ms: Option<u64>) -> Result<Duration, &'static str> {
    let timeout_ms = timeout_ms.unwrap_or(DEFAULT_GRAPH_TIMEOUT_MS);
    if !(1..=MAX_MCP_TIMEOUT_MS).contains(&timeout_ms) {
        return Err("timeout_ms must be between 1 and 30000");
    }
    Ok(Duration::from_millis(timeout_ms))
}

macro_rules! validate_input {
    ($name:ident, $operation:expr) => {
        impl $name {
            /// Validates the strict wire input into one canonical graph operation.
            pub fn validate(self) -> Result<GraphReadServiceRequest, &'static str> {
                let exact_pin = self.selection.validate()?;
                let timeout = validate_graph_timeout(self.timeout_ms)?;
                let operation = $operation(self)?;
                Ok(GraphReadServiceRequest {
                    exact_pin,
                    operation,
                    timeout,
                })
            }
        }
    };
}

validate_input!(GraphStatusInput, |_input: GraphStatusInput| {
    Ok::<RustGraphReadOperation, &'static str>(RustGraphReadOperation::Status)
});
validate_input!(GraphSearchInput, |input: GraphSearchInput| {
    Ok::<RustGraphReadOperation, &'static str>(RustGraphReadOperation::Search {
        query: RustGraphSymbolQuery::try_new(&input.query)
            .map_err(|_| "query violates the bounded literal graph profile")?,
        limits: input.limits.validate()?,
    })
});
validate_input!(GraphEvidenceInput, |input: GraphEvidenceInput| {
    Ok::<RustGraphReadOperation, &'static str>(RustGraphReadOperation::Evidence {
        site: input.site.validate()?,
        limits: input.limits.validate()?,
    })
});
validate_input!(GraphArchitectureInput, |input: GraphArchitectureInput| {
    Ok::<RustGraphReadOperation, &'static str>(RustGraphReadOperation::Architecture {
        limits: input.limits.validate()?,
    })
});
validate_input!(GraphTraceInput, |input: GraphTraceInput| {
    Ok::<RustGraphReadOperation, &'static str>(RustGraphReadOperation::Trace {
        start: input.start.validate()?,
        direction: input.direction.into_application(),
        edge_kinds: edge_kinds(input.edge_kinds)?,
        limits: input.limits.validate()?,
    })
});
validate_input!(GraphImpactInput, |input: GraphImpactInput| {
    Ok::<RustGraphReadOperation, &'static str>(RustGraphReadOperation::Impact {
        start: input.start.validate()?,
        edge_kinds: edge_kinds(input.edge_kinds)?,
        limits: input.limits.validate()?,
    })
});

fn span(value: super::super::McpSpan) -> Result<ByteSpan, &'static str> {
    if value.start > MAX_MCP_INTEROPERABLE_INTEGER || value.end > MAX_MCP_INTEROPERABLE_INTEGER {
        return Err("span exceeds the interoperable integer range");
    }
    ByteSpan::try_new(ByteOffset::new(value.start), ByteOffset::new(value.end))
        .map_err(|_| "span end precedes its start")
}

fn decode_path(value: &str) -> Result<repowitness_application::RepositoryPath, &'static str> {
    RepositoryPathTextV1::decode(
        value,
        RepositoryPathTextByteLimit::new(MAX_PATH_TEXT_BYTES),
        RepositoryPathLimits::new(MAX_PATH_BYTES, MAX_PATH_COMPONENTS),
    )
    .map_err(|_| "path must be bounded canonical rwp1:h: text")
}

fn decode_sha256(value: &str) -> Result<[u8; 32], &'static str> {
    if value.len() != 64 {
        return Err("digest fields must be lowercase SHA-256 text");
    }
    let mut output = [0_u8; 32];
    for (target, pair) in output.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        let high = hex_nibble(pair[0]).ok_or("digest fields must be lowercase SHA-256 text")?;
        let low = hex_nibble(pair[1]).ok_or("digest fields must be lowercase SHA-256 text")?;
        *target = (high << 4) | low;
    }
    Ok(output)
}

const fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

const fn operation_label(operation: &RustGraphReadOperation) -> &'static str {
    match operation {
        RustGraphReadOperation::Status => "status",
        RustGraphReadOperation::Search { .. } => "search",
        RustGraphReadOperation::Evidence { .. } => "evidence",
        RustGraphReadOperation::Architecture { .. } => "architecture",
        RustGraphReadOperation::Trace { .. } => "trace",
        RustGraphReadOperation::Impact { .. } => "impact",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition() -> serde_json::Value {
        serde_json::json!({
            "source_slot": format!("ssi1:h:{}", "AB".repeat(32)),
            "source_generation": 9,
            "path": "rwp1:h:7372632F6C69622E7273",
            "content_sha256": "22".repeat(32),
            "artifact_sha256": "33".repeat(32),
            "fact_ordinal": 7,
            "symbol_kind": "function",
            "name": "run",
            "qualified_name": "fixture::run",
            "name_span": {"start": 7, "end": 10},
            "declaration_span": {"start": 0, "end": 13},
        })
    }

    fn site() -> serde_json::Value {
        serde_json::json!({
            "source_slot": format!("ssi1:h:{}", "AB".repeat(32)),
            "path": "rwp1:h:7372632F6C69622E7273",
            "artifact_sha256": "33".repeat(32),
            "ordinal": 1,
            "site_kind": "call",
            "occurrence_span": {"start": 0, "end": 13},
            "target_span": {"start": 7, "end": 10},
        })
    }

    #[test]
    fn selection_limits_and_unknown_fields_fail_closed() {
        let active: GraphStatusInput =
            serde_json::from_value(serde_json::json!({})).expect("empty input selects active view");
        let active_request = active.validate().expect("valid");
        assert_eq!(active_request.exact_pin(), None);
        assert_eq!(active_request.timeout(), Duration::from_secs(30));

        let exact: GraphStatusInput = serde_json::from_value(serde_json::json!({
            "workspace_view": 4,
            "graph_generation": 9,
        }))
        .expect("wire shape");
        assert_eq!(exact.validate().expect("valid").exact_pin(), Some((4, 9)));

        let incomplete: GraphStatusInput = serde_json::from_value(serde_json::json!({
            "workspace_view": 4,
        }))
        .expect("wire shape");
        assert!(incomplete.validate().is_err());
        let oversized_pin: GraphStatusInput = serde_json::from_value(serde_json::json!({
            "workspace_view": MAX_MCP_INTEROPERABLE_INTEGER + 1,
            "graph_generation": 9,
        }))
        .expect("wire shape");
        assert!(oversized_pin.validate().is_err());
        assert!(
            serde_json::from_value::<GraphStatusInput>(
                serde_json::json!({"repository": "/private"})
            )
            .is_err()
        );

        let invalid_limit: GraphArchitectureInput =
            serde_json::from_value(serde_json::json!({"max_depth": 0})).expect("wire shape");
        assert!(invalid_limit.validate().is_err());

        let oversized_output: GraphArchitectureInput = serde_json::from_value(
            serde_json::json!({"max_output_bytes": MAX_REQUESTED_OUTPUT_BYTES + 1}),
        )
        .expect("wire shape");
        assert!(oversized_output.validate().is_err());

        let defaults = GraphLimitsInput::default()
            .validate()
            .expect("compiled graph defaults");
        assert_eq!(defaults.max_input_edges(), DEFAULT_INPUT_EDGES);
        assert_eq!(defaults.max_input_bytes(), DEFAULT_INPUT_BYTES);
        assert_eq!(defaults.max_visited_edges(), DEFAULT_VISITED_EDGES);
    }

    #[test]
    fn exact_selectors_and_edge_kinds_are_strict() {
        let trace: GraphTraceInput = serde_json::from_value(serde_json::json!({
            "start": {"type": "definition", "definition": definition()},
            "direction": "outbound",
            "edge_kinds": ["call", "reference"],
        }))
        .expect("valid trace input");
        let request = trace.validate().expect("valid trace request");
        assert_eq!(operation_label(&request.operation), "trace");

        let duplicate: GraphTraceInput = serde_json::from_value(serde_json::json!({
            "start": {"type": "site", "site": site()},
            "direction": "inbound",
            "edge_kinds": ["call", "call"],
        }))
        .expect("wire shape");
        assert!(duplicate.validate().is_err());

        let empty: GraphImpactInput = serde_json::from_value(serde_json::json!({
            "start": definition(),
            "edge_kinds": [],
        }))
        .expect("wire shape");
        assert!(empty.validate().is_err());
    }

    #[test]
    fn query_and_selector_debug_never_expose_untrusted_text() {
        let search: GraphSearchInput = serde_json::from_value(serde_json::json!({
            "query": "private_customer_symbol",
        }))
        .expect("wire shape");
        let request = search.validate().expect("valid search");
        let debug = format!("{request:?}");
        assert!(debug.contains("search"));
        assert!(!debug.contains("private_customer_symbol"));

        let definition: McpGraphDefinition =
            serde_json::from_value(definition()).expect("valid definition shape");
        let debug = format!("{definition:?}");
        assert!(!debug.contains("fixture::run"));
        assert!(!debug.contains("737263"));
        assert!(!debug.contains(&"22".repeat(32)));
    }

    #[test]
    fn malformed_digest_path_span_and_kind_are_rejected() {
        let mut invalid = definition();
        invalid["content_sha256"] = serde_json::Value::String("AA".repeat(32));
        let input: GraphImpactInput = serde_json::from_value(serde_json::json!({
            "start": invalid,
            "edge_kinds": ["call"],
        }))
        .expect("wire shape");
        assert!(input.validate().is_err());

        let mut invalid = site();
        invalid["target_span"] = serde_json::json!({"start": 20, "end": 21});
        let input: GraphEvidenceInput =
            serde_json::from_value(serde_json::json!({"site": invalid})).expect("wire shape");
        assert!(input.validate().is_err());
    }
}
