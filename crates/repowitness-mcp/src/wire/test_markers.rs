//! Versioned wire types for repository-scoped raw test-marker observations.

use std::{fmt, time::Duration};

use repowitness_application::{DEFAULT_TEST_MARKER_RESULTS, SourceLanguage, TestMarkersQuery};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{McpCoverage, McpOutboundSyntaxSite, validate_timeout};

/// Maximum retained marker records accepted at the transport boundary.
pub const MAX_MCP_TEST_MARKERS: u16 = 250;

/// Version-1 wire input for repository-scoped raw marker navigation.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TestMarkersInput {
    /// Optional exact built-in syntax language.
    pub language: Option<String>,
    /// Optional safe repository-relative byte prefix.
    pub path_prefix: Option<String>,
    /// Maximum returned raw marker observations, from 1 through 250.
    pub max_results: Option<u16>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for TestMarkersInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TestMarkersInput")
            .field("language", &self.language)
            .field(
                "path_prefix",
                &self.path_prefix.as_ref().map(|_| "<redacted-path>"),
            )
            .field("max_results", &self.max_results)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl TestMarkersInput {
    pub(crate) fn validate(self) -> Result<TestMarkersServiceRequest, &'static str> {
        let language = match self.language {
            Some(language) => SourceLanguage::from_stable_str(&language)
                .ok_or("language must be rust, go, typescript, tsx, or python")
                .map(Some)?,
            None => None,
        };
        let path_prefix = self.path_prefix;
        TestMarkersQuery::try_new(language, path_prefix.as_deref())
            .map_err(|_| "path_prefix must be a bounded safe repository-relative prefix")?;
        let max_results = self.max_results.unwrap_or(DEFAULT_TEST_MARKER_RESULTS);
        if !(1..=MAX_MCP_TEST_MARKERS).contains(&max_results) {
            return Err("max_results must be between 1 and 250");
        }
        Ok(TestMarkersServiceRequest {
            language,
            path_prefix,
            max_results,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

/// Validated, owned marker request passed to the local composition root.
pub struct TestMarkersServiceRequest {
    language: Option<SourceLanguage>,
    path_prefix: Option<String>,
    max_results: u16,
    timeout: Duration,
}

impl TestMarkersServiceRequest {
    /// Returns the optional exact built-in syntax-language filter.
    #[must_use]
    pub const fn language(&self) -> Option<SourceLanguage> {
        self.language
    }
    /// Returns the optional validated repository-relative byte prefix.
    #[must_use]
    pub fn path_prefix(&self) -> Option<&str> {
        self.path_prefix.as_deref()
    }
    /// Returns the inclusive retained marker ceiling.
    #[must_use]
    pub const fn max_results(&self) -> u16 {
        self.max_results
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

impl fmt::Debug for TestMarkersServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TestMarkersServiceRequest")
            .field("language", &self.language)
            .field(
                "path_prefix",
                &self.path_prefix.as_ref().map(|_| "<redacted-path>"),
            )
            .field("max_results", &self.max_results)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Exact selected-language support and emission receipt for test markers.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpTestMarkerLanguageCoverage {
    /// Persisted built-in syntax language.
    pub language: String,
    /// Indexed source files within the selected language/path scope.
    pub indexed_files: u64,
    /// Files whose raw extractor supports test-marker observations.
    pub supported_files: u64,
    /// Files whose raw extractor explicitly does not support test markers.
    pub unsupported_files: u64,
    /// Exact emitted raw marker observations before response truncation.
    pub emitted_markers: u64,
}

/// Version-1 structured response for repository-scoped raw marker observations.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TestMarkersOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Raw test-marker extraction profile version.
    pub test_markers_profile: u16,
    /// Concrete source snapshot SHA-256.
    pub snapshot_sha256: String,
    /// Opaque immutable generation identifier.
    pub generation: i64,
    /// `complete` or `not_produced`.
    pub availability: String,
    /// Recorded source-index coverage.
    pub coverage: McpCoverage,
    /// Exact language-specific support and emission receipts.
    pub language_coverage: Vec<McpTestMarkerLanguageCoverage>,
    /// Number of marker observations retained in this response.
    pub markers_returned: u64,
    /// Exact number before the explicit returned-marker bound.
    pub markers_total: u64,
    /// Whether an explicit marker bound omitted observations.
    pub truncated: bool,
    /// Conservative application output-byte accounting.
    pub output_bytes: u64,
    /// Explicit v1 scope boundary.
    pub limitation: String,
    /// Parser-attributed observations in deterministic source order.
    pub markers: Vec<McpOutboundSyntaxSite>,
}
