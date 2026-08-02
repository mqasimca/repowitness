use std::{fmt, time::Duration};

use repowitness_application::{CodeSearchQuery, DEFAULT_RELEVANT_PATHS, RelevantPathsLimits};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{McpCoverage, McpSearchMatch, validate_timeout};

/// MCP tool name for bounded lexical source-path navigation.
pub const RELEVANT_PATHS_TOOL_NAME: &str = "locate_relevant_paths";

/// Version-1 wire input for `locate_relevant_paths`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RelevantPathsInput {
    /// Literal Rust, Go, TypeScript, TSX, or Python symbol terms. FTS syntax is never accepted.
    pub query: String,
    /// Maximum returned paths, from 1 through 50.
    pub max_paths: Option<u16>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for RelevantPathsInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelevantPathsInput")
            .field("query", &"<redacted-query>")
            .field("max_paths", &self.max_paths)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl RelevantPathsInput {
    pub(crate) fn validate(self) -> Result<RelevantPathsServiceRequest, &'static str> {
        let query = CodeSearchQuery::try_new(&self.query)
            .map_err(|_| "query does not satisfy the bounded literal search profile")?;
        let max_paths = self.max_paths.unwrap_or(DEFAULT_RELEVANT_PATHS);
        RelevantPathsLimits::try_new(max_paths)
            .map_err(|_| "max_paths must be between 1 and 50")?;
        let timeout = validate_timeout(self.timeout_ms)?;
        Ok(RelevantPathsServiceRequest {
            query: query.as_str().to_owned(),
            max_paths,
            timeout,
        })
    }
}

/// Validated, owned request passed from the MCP adapter to the composition root.
pub struct RelevantPathsServiceRequest {
    query: String,
    max_paths: u16,
    timeout: Duration,
}

impl RelevantPathsServiceRequest {
    /// Returns the canonical literal query.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Returns the inclusive path-output bound.
    #[must_use]
    pub const fn max_paths(&self) -> u16 {
        self.max_paths
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

impl fmt::Debug for RelevantPathsServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelevantPathsServiceRequest")
            .field("query", &"<redacted-query>")
            .field("max_paths", &self.max_paths)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// One direct lexical path summary in a `locate_relevant_paths` response.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpRelevantPath {
    /// Canonical byte-preserving repository path.
    pub path: String,
    /// Exact source-content SHA-256 shared by this path's matched declarations.
    pub content_sha256: String,
    /// Number of returned direct declaration matches in this path.
    pub matching_declarations: u16,
    /// Smallest matching generation-local fact ordinal in this path.
    pub first_fact_ordinal: u64,
}

/// Version-1 structured response for `locate_relevant_paths`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RelevantPathsOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Path-presentation profile version.
    pub path_ranking_profile: u16,
    /// Concrete source snapshot SHA-256.
    pub snapshot_sha256: String,
    /// Opaque active-generation identifier.
    pub generation: i64,
    /// Categorical material-result resolution from the underlying lexical receipt.
    pub resolution: String,
    /// Domain-separated canonical query SHA-256.
    pub query_sha256: String,
    /// Number of returned declaration matches used by the path projection.
    pub matches_returned: u64,
    /// Exact number of declaration matches before candidate truncation.
    pub matches_total: u64,
    /// Number of returned path summaries.
    pub paths_returned: u64,
    /// Exact unique-path count among returned declaration matches before the path limit.
    ///
    /// It does not include paths that might occur only in candidates omitted by
    /// the underlying bounded lexical search.
    pub returned_match_paths_total: u64,
    /// Whether the path bound omitted paths from the returned-match surface.
    pub returned_match_paths_truncated: bool,
    /// Explicit coverage categories from the underlying lexical receipt.
    pub coverage: McpCoverage,
    /// Explicit limitations on path interpretation and ranking.
    pub limitations: Vec<String>,
    /// Paths ordered by returned declaration-match count then canonical path.
    pub paths: Vec<McpRelevantPath>,
    /// Deterministically ordered attributed declaration evidence supporting paths.
    pub matches: Vec<McpSearchMatch>,
}

#[cfg(test)]
mod tests {
    use super::RelevantPathsInput;

    #[test]
    fn input_is_bounded_and_redacted() {
        let input: RelevantPathsInput =
            serde_json::from_str(r#"{"query":"  Widget   run ","max_paths":12,"timeout_ms":1000}"#)
                .expect("valid input shape");
        let request = input.validate().expect("valid bounded input");
        assert_eq!(request.query(), "Widget run");
        assert_eq!(request.max_paths(), 12);
        let debug = format!("{request:?}");
        assert!(debug.contains("<redacted-query>"));
        assert!(!debug.contains("Widget"));
    }

    #[test]
    fn invalid_input_fails_before_service_access() {
        for value in [
            serde_json::json!({"query":""}),
            serde_json::json!({"query":"run", "max_paths":0}),
            serde_json::json!({"query":"run", "max_paths":51}),
            serde_json::json!({"query":"run", "timeout_ms":0}),
        ] {
            let input: RelevantPathsInput =
                serde_json::from_value(value).expect("bounded object shape should deserialize");
            assert!(input.validate().is_err());
        }
        assert!(
            serde_json::from_value::<RelevantPathsInput>(
                serde_json::json!({"query":"run", "unknown":true}),
            )
            .is_err()
        );
    }
}
