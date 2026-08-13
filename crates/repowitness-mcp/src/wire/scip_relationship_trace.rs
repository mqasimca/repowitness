//! Strict bounded SCIP relationship-trace contract.

#![allow(
    missing_docs,
    reason = "public field names and enclosing comments form the versioned JSON schema"
)]

use std::{fmt, time::Duration};

use repowitness_application::{
    DEFAULT_SCIP_RELATIONSHIP_TRACE_DEPTH, DEFAULT_SCIP_RELATIONSHIP_TRACE_EDGES, PackageScope,
    RepositoryPathLimits, RepositoryPathTextByteLimit, RepositoryPathTextV1,
    ScipRelationshipTraceDepth, ScipRelationshipTraceDirection, ScipRelationshipTraceMaxEdges,
    ScipSymbol,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    MAX_MCP_INTEROPERABLE_INTEGER, MAX_PATH_BYTES, MAX_PATH_COMPONENTS, MAX_PATH_TEXT_BYTES,
    validate_timeout,
};

/// MCP tool name for bounded SCIP relationship traversal.
pub const SCIP_RELATIONSHIP_TRACE_TOOL_NAME: &str = "scip_relationship_trace";
/// Versioned JSON output schema for `scip_relationship_trace`.
pub const SCIP_RELATIONSHIP_TRACE_SCHEMA_VERSION: u16 = 2;

/// Version-1 wire input for `scip_relationship_trace`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScipRelationshipTraceInput {
    /// Exact bounded opaque SCIP symbol from an already imported overlay.
    pub symbol: String,
    /// Canonical byte-preserving repository package roots. Omit for the whole source slot.
    pub package_roots: Option<Vec<String>>,
    /// Exact immutable workspace view from a prior result. Omit for the active view.
    pub workspace_view: Option<i64>,
    /// Follow producer `source -> target` or `target -> source` rows.
    pub direction: String,
    /// Inclusive breadth-first traversal depth, from one through four.
    pub max_depth: Option<u8>,
    /// Maximum retained relationship rows.
    pub max_edges: Option<u16>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for ScipRelationshipTraceInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScipRelationshipTraceInput")
            .field("symbol", &"<redacted-symbol>")
            .field("package_roots", &self.package_roots.as_ref().map(Vec::len))
            .field("workspace_view", &self.workspace_view)
            .field("direction", &self.direction)
            .field("max_depth", &self.max_depth)
            .field("max_edges", &self.max_edges)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl ScipRelationshipTraceInput {
    /// Validates untrusted wire values into one bounded storage-neutral request.
    pub fn validate(self) -> Result<ScipRelationshipTraceServiceRequest, &'static str> {
        let symbol = ScipSymbol::try_new(self.symbol)
            .map_err(|_| "symbol does not satisfy the bounded SCIP symbol contract")?;
        let package_scope = match self.package_roots {
            None => PackageScope::whole_repository(),
            Some(roots) => {
                let roots = roots
                    .iter()
                    .map(|root| {
                        RepositoryPathTextV1::decode(
                            root,
                            RepositoryPathTextByteLimit::new(MAX_PATH_TEXT_BYTES),
                            RepositoryPathLimits::new(MAX_PATH_BYTES, MAX_PATH_COMPONENTS),
                        )
                        .map_err(|_| "package_roots must contain bounded canonical rwp1:h: text")
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                PackageScope::try_explicit_roots(roots)
                    .map_err(|_| "package_roots are empty, overlapping, duplicated, or invalid")?
            }
        };
        if self.workspace_view.is_some_and(|view| {
            view <= 0 || u64::try_from(view).ok() > Some(MAX_MCP_INTEROPERABLE_INTEGER)
        }) {
            return Err("workspace_view must be a positive interoperable identifier");
        }
        let direction = match self.direction.as_str() {
            "outgoing" => ScipRelationshipTraceDirection::Outgoing,
            "incoming" => ScipRelationshipTraceDirection::Incoming,
            _ => return Err("direction must be outgoing or incoming"),
        };
        let max_depth = ScipRelationshipTraceDepth::try_new(
            self.max_depth
                .unwrap_or(DEFAULT_SCIP_RELATIONSHIP_TRACE_DEPTH),
        )
        .map_err(|_| "max_depth must be between 1 and 4")?;
        let max_edges = ScipRelationshipTraceMaxEdges::try_new(
            self.max_edges
                .unwrap_or(DEFAULT_SCIP_RELATIONSHIP_TRACE_EDGES),
        )
        .map_err(|_| "max_edges must be between 1 and 256")?;
        Ok(ScipRelationshipTraceServiceRequest {
            package_scope,
            symbol,
            workspace_view: self.workspace_view,
            direction,
            max_depth,
            max_edges,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

/// Validated owned SCIP relationship-trace service request.
pub struct ScipRelationshipTraceServiceRequest {
    package_scope: PackageScope,
    symbol: ScipSymbol,
    workspace_view: Option<i64>,
    direction: ScipRelationshipTraceDirection,
    max_depth: ScipRelationshipTraceDepth,
    max_edges: ScipRelationshipTraceMaxEdges,
    timeout: Duration,
}

impl ScipRelationshipTraceServiceRequest {
    /// Returns the explicit package scope.
    #[must_use]
    pub const fn package_scope(&self) -> &PackageScope {
        &self.package_scope
    }
    /// Returns the opaque exact SCIP root symbol.
    #[must_use]
    pub const fn symbol(&self) -> &ScipSymbol {
        &self.symbol
    }
    /// Returns the optional immutable workspace-view pin.
    #[must_use]
    pub const fn workspace_view(&self) -> Option<i64> {
        self.workspace_view
    }
    /// Returns the explicit traversal direction.
    #[must_use]
    pub const fn direction(&self) -> ScipRelationshipTraceDirection {
        self.direction
    }
    /// Returns the inclusive bounded traversal depth.
    #[must_use]
    pub const fn max_depth(&self) -> ScipRelationshipTraceDepth {
        self.max_depth
    }
    /// Returns the retained relationship-edge ceiling.
    #[must_use]
    pub const fn max_edges(&self) -> ScipRelationshipTraceMaxEdges {
        self.max_edges
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

impl fmt::Debug for ScipRelationshipTraceServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScipRelationshipTraceServiceRequest")
            .field("package_scope", &self.package_scope)
            .field("symbol", &"<redacted-symbol>")
            .field("workspace_view", &self.workspace_view)
            .field("direction", &self.direction)
            .field("max_depth", &self.max_depth)
            .field("max_edges", &self.max_edges)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Version-1 categorical result for one exact SCIP relationship traversal.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScipRelationshipTraceOutput {
    pub schema_version: u16,
    pub connected_workspace: String,
    pub workspace_view: i64,
    pub source_slot: String,
    /// `not_produced`, `no_relationships`, or `found`.
    pub resolution: String,
    /// Complete selected overlay receipt, absent only for `not_produced`.
    pub overlay: Option<McpScipRelationshipTraceOverlay>,
    /// Semantic SHA-256 identity of the requested package scope, absent only for `not_produced`.
    pub package_scope_sha256: Option<String>,
    /// `outgoing` or `incoming` as requested.
    pub direction: String,
    pub max_depth: u8,
    pub max_edges: u16,
    pub visited_symbols: u16,
    /// Known discovered symbols that could not be completely expanded; a lower bound when
    /// an edge or output ceiling stops further relationship-row discovery.
    pub unexpanded_frontier_symbols: u16,
    pub depth_limit_reached: bool,
    pub edge_limit_reached: bool,
    pub symbol_limit_reached: bool,
    pub output_limit_reached: bool,
    pub truncated: bool,
    pub output_bytes: u64,
    pub edges: Vec<McpScipRelationshipTraceEdge>,
}

/// Count-only immutable SCIP overlay receipt.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpScipRelationshipTraceOverlay {
    pub overlay_sha256: String,
    pub documents: u64,
    pub occurrences: u64,
    pub relationships: u64,
}

/// One exact relationship edge from a bounded trace.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpScipRelationshipTraceEdge {
    pub document_ordinal: u32,
    pub relationship_ordinal: u32,
    pub depth: u8,
    pub path: String,
    pub content_sha256: String,
    pub source: String,
    pub target: String,
    pub is_reference: bool,
    pub is_implementation: bool,
    pub is_type_definition: bool,
    pub is_definition: bool,
    /// `producer_declared` or `enclosed_reference`.
    pub evidence: String,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn input_is_strict_bounded_and_redacted() {
        let input: ScipRelationshipTraceInput = serde_json::from_str(
            r#"{
                "symbol":"scip-rust pkg 1 Root.",
                "package_roots":["rwp1:h:737263"],
                "workspace_view":7,
                "direction":"outgoing",
                "max_depth":2,
                "max_edges":8,
                "timeout_ms":8
            }"#,
        )
        .expect("valid wire input");
        let request = input.validate().expect("validated request");
        assert_eq!(request.symbol().as_str(), "scip-rust pkg 1 Root.");
        assert_eq!(request.workspace_view(), Some(7));
        assert_eq!(request.max_depth().get(), 2);
        assert_eq!(request.max_edges().get(), 8);
        assert_eq!(request.timeout(), Duration::from_millis(8));
        let debug = format!("{request:?}");
        assert!(!debug.contains("scip-rust"));
        assert!(!debug.contains("737263"));
    }

    #[test]
    fn malformed_inputs_and_unknown_fields_fail_closed() {
        for input in [
            r#"{"symbol":"","direction":"outgoing"}"#,
            r#"{"symbol":"scip-rust pkg 1 Root.","direction":"both"}"#,
            r#"{"symbol":"scip-rust pkg 1 Root.","direction":"incoming","max_depth":0}"#,
            r#"{"symbol":"scip-rust pkg 1 Root.","direction":"incoming","max_edges":257}"#,
            r#"{"symbol":"scip-rust pkg 1 Root.","direction":"incoming","package_roots":[]}"#,
        ] {
            let input: ScipRelationshipTraceInput =
                serde_json::from_str(input).expect("wire shape");
            assert!(input.validate().is_err());
        }
        assert!(serde_json::from_str::<ScipRelationshipTraceInput>(
            r#"{"symbol":"scip-rust pkg 1 Root.","direction":"incoming","host_path":"/private"}"#
        )
        .is_err());
    }
}
