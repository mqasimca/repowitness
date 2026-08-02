//! Versioned wire types for exact raw syntax-target discovery.

use std::{fmt, time::Duration};

use repowitness_application::{DEFAULT_SYNTAX_SITE_SEARCH_RESULTS, SyntaxSiteSearchQuery};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{McpCoverage, McpOutboundSyntaxSite, validate_timeout};

/// Native tool name for exact raw syntax-target discovery.
pub const SYNTAX_SITE_SEARCH_TOOL_NAME: &str = "syntax_site_search";
/// Transport ceiling kept below the application hard ceiling for predictable payloads.
pub const MAX_MCP_SYNTAX_SITE_SEARCH: u16 = 250;

/// Version-1 wire input for `syntax_site_search`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SyntaxSiteSearchInput {
    /// Exact parser-emitted UTF-8 target spelling; this is not a symbol identity.
    pub target: String,
    /// Maximum returned raw observations, from 1 through 250.
    pub max_sites: Option<u16>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for SyntaxSiteSearchInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyntaxSiteSearchInput")
            .field("target", &"<redacted-raw-target>")
            .field("max_sites", &self.max_sites)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl SyntaxSiteSearchInput {
    pub(crate) fn validate(self) -> Result<SyntaxSiteSearchServiceRequest, &'static str> {
        SyntaxSiteSearchQuery::try_new(&self.target)
            .map_err(|_| "target does not satisfy the bounded exact raw-syntax profile")?;
        let max_sites = self.max_sites.unwrap_or(DEFAULT_SYNTAX_SITE_SEARCH_RESULTS);
        if !(1..=MAX_MCP_SYNTAX_SITE_SEARCH).contains(&max_sites) {
            return Err("max_sites must be between 1 and 250");
        }
        Ok(SyntaxSiteSearchServiceRequest {
            target: self.target,
            max_sites,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

/// Validated, owned request passed to the local composition root.
pub struct SyntaxSiteSearchServiceRequest {
    target: String,
    max_sites: u16,
    timeout: Duration,
}

impl SyntaxSiteSearchServiceRequest {
    /// Returns the exact raw target spelling.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }
    /// Returns the inclusive retained-observation ceiling.
    #[must_use]
    pub const fn max_sites(&self) -> u16 {
        self.max_sites
    }
    /// Returns the remaining end-to-end timeout.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }
    pub(crate) const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl fmt::Debug for SyntaxSiteSearchServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyntaxSiteSearchServiceRequest")
            .field("target", &"<redacted-raw-target>")
            .field("max_sites", &self.max_sites)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Version-1 structured response for `syntax_site_search`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SyntaxSiteSearchOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Raw-target search profile version.
    pub syntax_site_search_profile: u16,
    /// SHA-256 identity of the exact target input.
    pub target_sha256: String,
    /// Concrete source snapshot SHA-256.
    pub snapshot_sha256: String,
    /// Opaque immutable generation identifier.
    pub generation: i64,
    /// `complete` or `not_produced`.
    pub availability: String,
    /// Recorded source-index coverage.
    pub coverage: McpCoverage,
    /// Number of raw observations retained in this response.
    pub sites_returned: u64,
    /// Exact number before the explicit returned-site bound.
    pub sites_total: u64,
    /// Whether an explicit retained-site bound omitted observations.
    pub truncated: bool,
    /// Conservative application output-byte accounting.
    pub output_bytes: u64,
    /// Explicit v1 scope boundary.
    pub limitation: String,
    /// Exact parser observations in canonical path then source order.
    pub sites: Vec<McpOutboundSyntaxSite>,
}

#[cfg(test)]
mod tests {
    use super::{MAX_MCP_SYNTAX_SITE_SEARCH, SyntaxSiteSearchInput};

    #[test]
    fn target_and_bound_are_checked_before_service_access() {
        let request = SyntaxSiteSearchInput {
            target: "target".to_owned(),
            max_sites: Some(MAX_MCP_SYNTAX_SITE_SEARCH),
            timeout_ms: Some(1),
        }
        .validate()
        .expect("bounded target should be accepted");
        assert_eq!(request.max_sites(), MAX_MCP_SYNTAX_SITE_SEARCH);
        assert!(
            SyntaxSiteSearchInput {
                target: String::new(),
                max_sites: None,
                timeout_ms: None,
            }
            .validate()
            .is_err()
        );
    }
}
