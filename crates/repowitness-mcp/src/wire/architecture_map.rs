//! Versioned wire types for the bounded multi-language source architecture map.

use std::{fmt, time::Duration};

use repowitness_application::{DEFAULT_ARCHITECTURE_MAP_FILES, MAX_ARCHITECTURE_MAP_FILES};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{McpCoverage, validate_timeout};

/// Native tool name for the bounded multi-language file architecture map.
pub const ARCHITECTURE_MAP_TOOL_NAME: &str = "architecture_map";

/// Version-1 wire input for `architecture_map`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureMapInput {
    /// Maximum exact file receipts to return, from 1 through 1,000.
    pub max_files: Option<u16>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl ArchitectureMapInput {
    pub(crate) fn validate(self) -> Result<ArchitectureMapServiceRequest, &'static str> {
        let max_files = self.max_files.unwrap_or(DEFAULT_ARCHITECTURE_MAP_FILES);
        if !(1..=MAX_ARCHITECTURE_MAP_FILES).contains(&max_files) {
            return Err("max_files must be between 1 and 1000");
        }
        Ok(ArchitectureMapServiceRequest {
            max_files,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

/// Validated, owned architecture-map request passed to the composition root.
pub struct ArchitectureMapServiceRequest {
    max_files: u16,
    timeout: Duration,
}

impl ArchitectureMapServiceRequest {
    /// Returns the inclusive returned-file ceiling.
    #[must_use]
    pub const fn max_files(&self) -> u16 {
        self.max_files
    }

    /// Returns the remaining end-to-end operation deadline.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl fmt::Debug for ArchitectureMapServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ArchitectureMapServiceRequest")
            .field("max_files", &self.max_files)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Complete all-file totals for one stable language.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpArchitectureMapLanguage {
    /// `rust`, `go`, `typescript`, `tsx`, or `python`.
    pub language: String,
    /// Complete indexed-file count before returned-file truncation.
    pub files: u64,
    /// Complete persisted declaration count before returned-file truncation.
    pub declarations: u64,
}

/// Exact indexed source-file receipt without source bytes.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpArchitectureMapFile {
    /// Canonical byte-preserving repository path.
    pub path: String,
    /// Persisted source adapter language.
    pub language: String,
    /// Exact source-content SHA-256.
    pub content_sha256: String,
    /// Exact analysis-artifact SHA-256.
    pub artifact_sha256: String,
    /// Exact analysis producer-manifest SHA-256.
    pub producer_manifest_sha256: String,
    /// Exact persisted declaration count for the analysis artifact.
    pub declaration_count: u64,
}

/// Version-1 structured response for `architecture_map`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureMapOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Architecture-map profile version.
    pub map_profile: u16,
    /// Concrete source snapshot SHA-256.
    pub snapshot_sha256: String,
    /// Opaque active-generation identifier.
    pub generation: i64,
    /// Explicit indexing coverage established before activation.
    pub coverage: McpCoverage,
    /// Complete indexed-file count before entry truncation.
    pub total_files: u64,
    /// Complete persisted declaration count before entry truncation.
    pub total_declarations: u64,
    /// Number of exact file receipts retained in this response.
    pub files_returned: u64,
    /// Whether any indexed file receipt was omitted by an explicit bound.
    pub truncated: bool,
    /// Conservative encoded application-output bytes.
    pub output_bytes: u64,
    /// Explicit scope boundary: `file_inventory_only_no_relationship_inference`.
    pub limitation: String,
    /// Complete indexed-language totals in stable language order.
    pub languages: Vec<McpArchitectureMapLanguage>,
    /// Exact file receipts in canonical byte-path order.
    pub files: Vec<McpArchitectureMapFile>,
}

#[cfg(test)]
mod tests {
    use super::{ArchitectureMapInput, DEFAULT_ARCHITECTURE_MAP_FILES};

    #[test]
    fn defaults_and_bounds_are_explicit() {
        let request = ArchitectureMapInput {
            max_files: None,
            timeout_ms: None,
        }
        .validate()
        .expect("default request should be valid");
        assert_eq!(request.max_files(), DEFAULT_ARCHITECTURE_MAP_FILES);
        assert!(
            ArchitectureMapInput {
                max_files: Some(0),
                timeout_ms: None,
            }
            .validate()
            .is_err()
        );
    }
}
