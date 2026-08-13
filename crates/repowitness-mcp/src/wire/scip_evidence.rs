//! Strict package-scoped SCIP evidence read contract.

#![allow(
    missing_docs,
    reason = "public field names and enclosing comments form the versioned JSON schema"
)]

use std::{fmt, time::Duration};

use repowitness_application::{
    PackageScope, RepositoryPathLimits, RepositoryPathTextByteLimit, RepositoryPathTextV1,
    ScipSymbol,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    MAX_MCP_INTEROPERABLE_INTEGER, MAX_PATH_BYTES, MAX_PATH_COMPONENTS, MAX_PATH_TEXT_BYTES,
    validate_timeout,
};

/// Versioned JSON output schema for `scip_evidence`.
pub const SCIP_EVIDENCE_SCHEMA_VERSION: u16 = 2;

/// Version-1 wire input for `scip_evidence`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScipEvidenceInput {
    /// Exact bounded opaque SCIP symbol from an already imported overlay.
    pub symbol: String,
    /// Canonical byte-preserving repository package roots. Omit for the whole source slot.
    pub package_roots: Option<Vec<String>>,
    /// Exact immutable workspace view from a prior result. Omit for the active view.
    pub workspace_view: Option<i64>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for ScipEvidenceInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScipEvidenceInput")
            .field("symbol", &"<redacted-symbol>")
            .field("package_roots", &self.package_roots.as_ref().map(Vec::len))
            .field("workspace_view", &self.workspace_view)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl ScipEvidenceInput {
    /// Validates untrusted wire values into one bounded, storage-neutral request.
    pub fn validate(self) -> Result<ScipEvidenceServiceRequest, &'static str> {
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
        Ok(ScipEvidenceServiceRequest {
            package_scope,
            symbol,
            workspace_view: self.workspace_view,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

/// Validated owned SCIP evidence service request.
pub struct ScipEvidenceServiceRequest {
    package_scope: PackageScope,
    symbol: ScipSymbol,
    workspace_view: Option<i64>,
    timeout: Duration,
}

impl ScipEvidenceServiceRequest {
    /// Returns the explicit package scope.
    #[must_use]
    pub const fn package_scope(&self) -> &PackageScope {
        &self.package_scope
    }
    /// Returns the opaque exact SCIP symbol.
    #[must_use]
    pub const fn symbol(&self) -> &ScipSymbol {
        &self.symbol
    }
    /// Returns the optional immutable workspace-view pin.
    #[must_use]
    pub const fn workspace_view(&self) -> Option<i64> {
        self.workspace_view
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

impl fmt::Debug for ScipEvidenceServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScipEvidenceServiceRequest")
            .field("package_scope", &self.package_scope)
            .field("symbol", &"<redacted-symbol>")
            .field("workspace_view", &self.workspace_view)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Version-1 categorical result for an exact SCIP evidence lookup.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScipEvidenceOutput {
    pub schema_version: u16,
    pub connected_workspace: String,
    pub workspace_view: i64,
    pub source_slot: String,
    /// `not_produced`, `no_match`, or `found`.
    pub resolution: String,
    /// Complete selected overlay receipt, absent only for `not_produced`.
    pub overlay: Option<McpScipOverlay>,
    /// Semantic SHA-256 identity of the requested package scope, absent for `not_produced`.
    pub package_scope_sha256: Option<String>,
    pub occurrences_truncated: bool,
    pub relationships_truncated: bool,
    pub output_bytes: u64,
    pub occurrences: Vec<McpScipOccurrence>,
    pub relationships: Vec<McpScipRelationship>,
}

/// Count-only immutable SCIP overlay receipt.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpScipOverlay {
    pub overlay_sha256: String,
    pub documents: u64,
    pub occurrences: u64,
    pub relationships: u64,
}

/// Exact bounded SCIP occurrence evidence.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpScipOccurrence {
    pub path: String,
    pub content_sha256: String,
    pub span_start: u64,
    pub span_end: u64,
    pub definition: bool,
    pub import: bool,
    pub read_access: bool,
    pub write_access: bool,
}

/// Exact bounded SCIP relationship evidence.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpScipRelationship {
    pub path: String,
    pub content_sha256: String,
    /// `outgoing` or `incoming` relative to the requested symbol.
    pub direction: String,
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
    use super::*;

    #[test]
    fn input_is_strict_bounded_and_redacted() {
        let input: ScipEvidenceInput = serde_json::from_str(
            r#"{
                "symbol":"scip-rust pkg 1 Symbol.",
                "package_roots":["rwp1:h:737263"],
                "workspace_view":7,
                "timeout_ms":8
            }"#,
        )
        .expect("valid wire input");
        let request = input.validate().expect("validated request");
        assert_eq!(request.symbol().as_str(), "scip-rust pkg 1 Symbol.");
        assert_eq!(request.workspace_view(), Some(7));
        assert_eq!(request.timeout(), Duration::from_millis(8));
        let debug = format!("{request:?}");
        assert!(!debug.contains("scip-rust"));
        assert!(!debug.contains("737263"));
    }

    #[test]
    fn malformed_scope_selection_and_unknown_fields_fail_closed() {
        for input in [
            r#"{"symbol":""}"#,
            r#"{"symbol":"scip-rust pkg 1 Symbol.","package_roots":[]}"#,
            r#"{"symbol":"scip-rust pkg 1 Symbol.","package_roots":["not-a-path"]}"#,
            r#"{"symbol":"scip-rust pkg 1 Symbol.","workspace_view":0}"#,
            r#"{"symbol":"scip-rust pkg 1 Symbol.","timeout_ms":0}"#,
        ] {
            let input: ScipEvidenceInput = serde_json::from_str(input).expect("wire shape");
            assert!(input.validate().is_err());
        }
        assert!(
            serde_json::from_str::<ScipEvidenceInput>(
                r#"{"symbol":"scip-rust pkg 1 Symbol.","host_path":"/private"}"#,
            )
            .is_err()
        );
    }
}
