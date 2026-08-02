//! Versioned wire types for exact unresolved raw syntax observations.

use std::{fmt, time::Duration};

use repowitness_application::DEFAULT_OUTBOUND_SITES_RESULTS;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    MAX_MCP_INTEROPERABLE_INTEGER, McpCoverage, McpSpan, is_canonical_path_text,
    is_lowercase_sha256, validate_timeout,
};

/// Native tool name for exact declaration-contained raw syntax observations.
pub const OUTBOUND_SITES_TOOL_NAME: &str = "outbound_sites";
/// Transport ceiling kept below the application hard ceiling for predictable MCP payloads.
pub const MAX_MCP_OUTBOUND_SITES: u16 = 250;

/// Version-1 wire input for `outbound_sites`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutboundSitesInput {
    /// Exact snapshot SHA-256 from a declaration-search result.
    pub snapshot_sha256: String,
    /// Exact positive immutable generation identifier.
    pub generation: i64,
    /// Canonical byte-preserving repository path from that result.
    pub path: String,
    /// Exact source-content SHA-256 from that result.
    pub content_sha256: String,
    /// Exact analysis-artifact SHA-256 from that result.
    pub artifact_sha256: String,
    /// Exact declaration fact ordinal from that result.
    pub fact_ordinal: u64,
    /// Maximum returned raw observations, from 1 through 250.
    pub max_sites: Option<u16>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for OutboundSitesInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OutboundSitesInput")
            .field("snapshot_sha256", &"<redacted-digest>")
            .field("generation", &self.generation)
            .field("path", &"<redacted-path>")
            .field("content_sha256", &"<redacted-digest>")
            .field("artifact_sha256", &"<redacted-digest>")
            .field("fact_ordinal", &self.fact_ordinal)
            .field("max_sites", &self.max_sites)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl OutboundSitesInput {
    pub(crate) fn validate(self) -> Result<OutboundSitesServiceRequest, &'static str> {
        if self.generation <= 0 {
            return Err("generation must be a positive identifier");
        }
        if !is_lowercase_sha256(&self.snapshot_sha256)
            || !is_lowercase_sha256(&self.content_sha256)
            || !is_lowercase_sha256(&self.artifact_sha256)
        {
            return Err("digest fields must be lowercase SHA-256 text");
        }
        if !is_canonical_path_text(&self.path) {
            return Err("path must be bounded canonical rwp1:h: text");
        }
        if self.fact_ordinal > MAX_MCP_INTEROPERABLE_INTEGER {
            return Err("fact_ordinal exceeds the interoperable integer range");
        }
        let max_sites = self.max_sites.unwrap_or(DEFAULT_OUTBOUND_SITES_RESULTS);
        if !(1..=MAX_MCP_OUTBOUND_SITES).contains(&max_sites) {
            return Err("max_sites must be between 1 and 250");
        }
        Ok(OutboundSitesServiceRequest {
            snapshot_sha256: self.snapshot_sha256,
            generation: self.generation,
            path: self.path,
            content_sha256: self.content_sha256,
            artifact_sha256: self.artifact_sha256,
            fact_ordinal: self.fact_ordinal,
            max_sites,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

/// Validated, owned raw-site request passed to the local composition root.
pub struct OutboundSitesServiceRequest {
    snapshot_sha256: String,
    generation: i64,
    path: String,
    content_sha256: String,
    artifact_sha256: String,
    fact_ordinal: u64,
    max_sites: u16,
    timeout: Duration,
}

impl OutboundSitesServiceRequest {
    /// Returns the exact selected source snapshot digest text.
    #[must_use]
    pub fn snapshot_sha256(&self) -> &str {
        &self.snapshot_sha256
    }
    /// Returns the exact selected immutable generation.
    #[must_use]
    pub const fn generation(&self) -> i64 {
        self.generation
    }
    /// Returns the exact selected canonical repository path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Returns the exact selected source-content digest text.
    #[must_use]
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }
    /// Returns the exact selected source artifact digest text.
    #[must_use]
    pub fn artifact_sha256(&self) -> &str {
        &self.artifact_sha256
    }
    /// Returns the exact selected declaration fact ordinal.
    #[must_use]
    pub const fn fact_ordinal(&self) -> u64 {
        self.fact_ordinal
    }
    /// Returns the inclusive returned-site ceiling.
    #[must_use]
    pub const fn max_sites(&self) -> u16 {
        self.max_sites
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

impl fmt::Debug for OutboundSitesServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OutboundSitesServiceRequest")
            .field("snapshot_sha256", &"<redacted-digest>")
            .field("generation", &self.generation)
            .field("path", &"<redacted-path>")
            .field("content_sha256", &"<redacted-digest>")
            .field("artifact_sha256", &"<redacted-digest>")
            .field("fact_ordinal", &self.fact_ordinal)
            .field("max_sites", &self.max_sites)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Exact declaration context that physically contains every returned site.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpOutboundSitesDeclaration {
    /// Persisted adapter language.
    pub language: String,
    /// Exact declaration byte span.
    pub declaration_span: McpSpan,
}

/// One parser-attributed raw observation; target resolution is deliberately absent.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpOutboundSyntaxSite {
    /// Canonical byte-preserving repository path.
    pub path: String,
    /// Exact source-content SHA-256.
    pub content_sha256: String,
    /// Exact raw-site artifact SHA-256.
    pub artifact_sha256: String,
    /// Persisted adapter language.
    pub language: String,
    /// Artifact-local source-order ordinal.
    pub ordinal: u32,
    /// `import`, `reference`, `call`, or `test_marker`.
    pub kind: String,
    /// `direct_syntax` or `syntax_heuristic`.
    pub evidence: String,
    /// Exact enclosing occurrence byte span.
    pub occurrence_span: McpSpan,
    /// Exact raw-target byte span.
    pub target_span: McpSpan,
    /// Bounded UTF-8 target spelling only; it is not a resolved symbol.
    pub raw_target: String,
    /// Always `not_attempted_no_resolution_profile` in v1.
    pub target_resolution: String,
}

/// Version-1 structured response for `outbound_sites`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutboundSitesOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Raw-site extraction profile version.
    pub outbound_sites_profile: u16,
    /// Concrete source snapshot SHA-256.
    pub snapshot_sha256: String,
    /// Opaque immutable generation identifier.
    pub generation: i64,
    /// Exact selector echoed without a source root or mutable source read.
    pub selector: OutboundSitesSelectorOutput,
    /// `complete` or `not_produced`.
    pub availability: String,
    /// Exact declaration context, absent when the selector has no declaration.
    pub declaration: Option<McpOutboundSitesDeclaration>,
    /// Recorded source-index coverage.
    pub coverage: McpCoverage,
    /// Number of raw sites retained in this response.
    pub sites_returned: u64,
    /// Exact number before the explicit returned-site bound.
    pub sites_total: u64,
    /// Whether an explicit site bound omitted observations.
    pub truncated: bool,
    /// Conservative application output-byte accounting.
    pub output_bytes: u64,
    /// Explicit v1 scope boundary.
    pub limitation: String,
    /// Raw parser observations in deterministic source order.
    pub sites: Vec<McpOutboundSyntaxSite>,
}

/// Exact declaration selector echoed by `outbound_sites`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutboundSitesSelectorOutput {
    /// Canonical byte-preserving repository path.
    pub path: String,
    /// Exact source-content SHA-256.
    pub content_sha256: String,
    /// Exact analysis-artifact SHA-256.
    pub artifact_sha256: String,
    /// Exact selected declaration fact ordinal.
    pub fact_ordinal: u64,
}

#[cfg(test)]
mod tests {
    use super::{MAX_MCP_OUTBOUND_SITES, OutboundSitesInput};

    #[test]
    fn exact_selector_and_bound_are_checked_before_service_access() {
        let digest = "ab".repeat(32);
        let input = OutboundSitesInput {
            snapshot_sha256: digest.clone(),
            generation: 1,
            path: format!("rwp1:h:{:02x}", b'a'),
            content_sha256: digest.clone(),
            artifact_sha256: digest,
            fact_ordinal: 0,
            max_sites: Some(MAX_MCP_OUTBOUND_SITES),
            timeout_ms: Some(1),
        }
        .validate()
        .expect("bounded exact selector should be accepted");
        assert_eq!(input.max_sites(), MAX_MCP_OUTBOUND_SITES);
        assert!(
            OutboundSitesInput {
                snapshot_sha256: "00".repeat(32),
                generation: 0,
                path: "rwp1:h:61".to_owned(),
                content_sha256: "00".repeat(32),
                artifact_sha256: "00".repeat(32),
                fact_ordinal: 0,
                max_sites: None,
                timeout_ms: None,
            }
            .validate()
            .is_err()
        );
    }
}
