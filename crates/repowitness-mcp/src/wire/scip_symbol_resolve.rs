//! Strict exact-source-span to opaque SCIP-symbol navigation contract.

#![allow(
    missing_docs,
    reason = "public field names and enclosing comments form the versioned JSON schema"
)]

use std::{fmt, time::Duration};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    MAX_MCP_INTEROPERABLE_INTEGER, McpSpan, is_canonical_path_text, is_lowercase_sha256,
    validate_timeout,
};

/// Native tool name for exact syntax-span to opaque SCIP-symbol navigation.
pub const SCIP_SYMBOL_RESOLVE_TOOL_NAME: &str = "scip_symbol_resolve";

/// Version-1 wire input for `scip_symbol_resolve`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScipSymbolResolveInput {
    /// Exact source snapshot SHA-256 from the declaration-search receipt.
    pub snapshot_sha256: String,
    /// Exact positive generation from that declaration-search receipt.
    pub generation: i64,
    /// Canonical byte-preserving repository path from an exact declaration receipt.
    pub path: String,
    /// Exact source-content SHA-256 from that declaration receipt.
    pub content_sha256: String,
    /// Exact analysis-artifact SHA-256 from that declaration receipt.
    pub artifact_sha256: String,
    /// Exact stable fact ordinal from that declaration receipt.
    pub fact_ordinal: u64,
    /// Exact declaration-name byte span from that receipt.
    pub name_span: McpSpan,
    /// Exact immutable workspace view from a prior result. Omit for the active view.
    pub workspace_view: Option<i64>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for ScipSymbolResolveInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScipSymbolResolveInput")
            .field("snapshot_sha256", &"<redacted-digest>")
            .field("generation", &self.generation)
            .field("path", &"<redacted-path>")
            .field("content_sha256", &"<redacted-digest>")
            .field("artifact_sha256", &"<redacted-digest>")
            .field("fact_ordinal", &self.fact_ordinal)
            .field("name_span", &self.name_span)
            .field("workspace_view", &self.workspace_view)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl ScipSymbolResolveInput {
    /// Validates untrusted wire input into the composition-root request.
    pub fn validate(self) -> Result<ScipSymbolResolveServiceRequest, &'static str> {
        if !is_lowercase_sha256(&self.snapshot_sha256) {
            return Err("snapshot_sha256 must be lowercase SHA-256 text");
        }
        if self.generation <= 0 {
            return Err("generation must be a positive interoperable identifier");
        }
        if u64::try_from(self.generation).ok() > Some(MAX_MCP_INTEROPERABLE_INTEGER) {
            return Err("generation exceeds the interoperable integer range");
        }
        if !is_canonical_path_text(&self.path) {
            return Err("path must be bounded canonical rwp1:h: text");
        }
        if !is_lowercase_sha256(&self.content_sha256) {
            return Err("content_sha256 must be lowercase SHA-256 text");
        }
        if !is_lowercase_sha256(&self.artifact_sha256) {
            return Err("artifact_sha256 must be lowercase SHA-256 text");
        }
        if self.fact_ordinal > MAX_MCP_INTEROPERABLE_INTEGER {
            return Err("fact_ordinal exceeds the interoperable integer range");
        }
        if self.name_span.start >= self.name_span.end {
            return Err("name_span must be a non-empty half-open byte span");
        }
        if self.name_span.end > MAX_MCP_INTEROPERABLE_INTEGER {
            return Err("name_span exceeds the interoperable integer range");
        }
        if self.workspace_view.is_some_and(|view| view <= 0) {
            return Err("workspace_view must be a positive interoperable identifier");
        }
        if self
            .workspace_view
            .is_some_and(|view| u64::try_from(view).ok() > Some(MAX_MCP_INTEROPERABLE_INTEGER))
        {
            return Err("workspace_view exceeds the interoperable integer range");
        }
        Ok(ScipSymbolResolveServiceRequest {
            snapshot_sha256: self.snapshot_sha256,
            generation: self.generation,
            path: self.path,
            content_sha256: self.content_sha256,
            artifact_sha256: self.artifact_sha256,
            fact_ordinal: self.fact_ordinal,
            name_span: self.name_span,
            workspace_view: self.workspace_view,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

/// Validated owned exact-span navigation request passed to the composition root.
pub struct ScipSymbolResolveServiceRequest {
    snapshot_sha256: String,
    generation: i64,
    path: String,
    content_sha256: String,
    artifact_sha256: String,
    fact_ordinal: u64,
    name_span: McpSpan,
    workspace_view: Option<i64>,
    timeout: Duration,
}

impl ScipSymbolResolveServiceRequest {
    /// Returns the exact source-snapshot digest from the declaration receipt.
    #[must_use]
    pub fn snapshot_sha256(&self) -> &str {
        &self.snapshot_sha256
    }
    /// Returns the exact generation from the declaration receipt.
    #[must_use]
    pub const fn generation(&self) -> i64 {
        self.generation
    }
    /// Returns the exact canonical source path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Returns the exact source-content digest text.
    #[must_use]
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }
    /// Returns the exact analysis-artifact digest from the declaration receipt.
    #[must_use]
    pub fn artifact_sha256(&self) -> &str {
        &self.artifact_sha256
    }
    /// Returns the exact stable declaration fact ordinal.
    #[must_use]
    pub const fn fact_ordinal(&self) -> u64 {
        self.fact_ordinal
    }
    /// Returns the exact identifier span.
    #[must_use]
    pub const fn name_span(&self) -> McpSpan {
        self.name_span
    }
    /// Returns an optional immutable workspace-view pin.
    #[must_use]
    pub const fn workspace_view(&self) -> Option<i64> {
        self.workspace_view
    }
    /// Returns the remaining end-to-end deadline.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }
    pub(crate) const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl fmt::Debug for ScipSymbolResolveServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScipSymbolResolveServiceRequest")
            .field("snapshot_sha256", &"<redacted-digest>")
            .field("generation", &self.generation)
            .field("path", &"<redacted-path>")
            .field("content_sha256", &"<redacted-digest>")
            .field("artifact_sha256", &"<redacted-digest>")
            .field("fact_ordinal", &self.fact_ordinal)
            .field("name_span", &self.name_span)
            .field("workspace_view", &self.workspace_view)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Version-1 categorical result for an exact SCIP syntax-span resolution.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScipSymbolResolveOutput {
    pub schema_version: u16,
    pub connected_workspace: String,
    pub workspace_view: i64,
    pub source_slot: String,
    /// `not_produced`, `no_exact_match`, `ambiguous`, or `exact`.
    pub resolution: String,
    /// Exact opaque provider symbol only when `resolution` is `exact`.
    pub symbol: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::ScipSymbolResolveInput;
    use crate::McpSpan;

    #[test]
    fn input_is_strict_bounded_and_redacted() {
        let request = ScipSymbolResolveInput {
            snapshot_sha256: "cd".repeat(32),
            generation: 3,
            path: "rwp1:h:7372632F6C69622E7273".to_owned(),
            content_sha256: "ab".repeat(32),
            artifact_sha256: "ef".repeat(32),
            fact_ordinal: 7,
            name_span: McpSpan { start: 5, end: 9 },
            workspace_view: Some(7),
            timeout_ms: Some(8),
        }
        .validate()
        .expect("valid exact span should be accepted");
        assert_eq!(request.name_span(), McpSpan { start: 5, end: 9 });
        assert_eq!(request.generation(), 3);
        assert_eq!(request.fact_ordinal(), 7);
        assert_eq!(request.workspace_view(), Some(7));
        let debug = format!("{request:?}");
        assert!(!debug.contains("737263"));
        assert!(!debug.contains("abab"));
    }

    #[test]
    fn malformed_exact_inputs_fail_before_service_access() {
        for input in [
            ScipSymbolResolveInput {
                snapshot_sha256: "cd".repeat(32),
                generation: 3,
                path: "not-a-path".to_owned(),
                content_sha256: "ab".repeat(32),
                artifact_sha256: "ef".repeat(32),
                fact_ordinal: 0,
                name_span: McpSpan { start: 1, end: 2 },
                workspace_view: None,
                timeout_ms: None,
            },
            ScipSymbolResolveInput {
                snapshot_sha256: "cd".repeat(32),
                generation: i64::try_from(super::MAX_MCP_INTEROPERABLE_INTEGER + 1)
                    .expect("interoperable overflow fits i64"),
                path: "rwp1:h:737263".to_owned(),
                content_sha256: "ab".repeat(32),
                artifact_sha256: "ef".repeat(32),
                fact_ordinal: 0,
                name_span: McpSpan { start: 2, end: 3 },
                workspace_view: Some(
                    i64::try_from(super::MAX_MCP_INTEROPERABLE_INTEGER + 1)
                        .expect("interoperable overflow fits i64"),
                ),
                timeout_ms: None,
            },
            ScipSymbolResolveInput {
                snapshot_sha256: "not-a-digest".to_owned(),
                generation: 3,
                path: "rwp1:h:737263".to_owned(),
                content_sha256: "not-a-digest".to_owned(),
                artifact_sha256: "ef".repeat(32),
                fact_ordinal: 0,
                name_span: McpSpan { start: 1, end: 2 },
                workspace_view: None,
                timeout_ms: None,
            },
            ScipSymbolResolveInput {
                snapshot_sha256: "cd".repeat(32),
                generation: 0,
                path: "rwp1:h:737263".to_owned(),
                content_sha256: "ab".repeat(32),
                artifact_sha256: "ef".repeat(32),
                fact_ordinal: 0,
                name_span: McpSpan { start: 2, end: 2 },
                workspace_view: None,
                timeout_ms: None,
            },
            ScipSymbolResolveInput {
                snapshot_sha256: "cd".repeat(32),
                generation: 3,
                path: "rwp1:h:737263".to_owned(),
                content_sha256: "ab".repeat(32),
                artifact_sha256: "ef".repeat(32),
                fact_ordinal: super::MAX_MCP_INTEROPERABLE_INTEGER + 1,
                name_span: McpSpan { start: 2, end: 3 },
                workspace_view: None,
                timeout_ms: None,
            },
        ] {
            assert!(input.validate().is_err());
        }
    }
}
