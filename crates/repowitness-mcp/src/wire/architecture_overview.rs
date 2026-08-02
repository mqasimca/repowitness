//! Versioned wire types for bounded source-only repository orientation.

use std::{fmt, time::Duration};

use repowitness_application::{
    DEFAULT_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES, DEFAULT_ARCHITECTURE_OVERVIEW_FILES,
    DEFAULT_ARCHITECTURE_OVERVIEW_ROOTS, MAX_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES,
    MAX_ARCHITECTURE_OVERVIEW_FILES, MAX_ARCHITECTURE_OVERVIEW_ROOTS,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    McpArchitectureMapFile, McpArchitectureMapLanguage, McpCoverage, McpSearchMatch,
    validate_timeout,
};

/// Native tool name for bounded source-only repository orientation.
pub const ARCHITECTURE_OVERVIEW_TOOL_NAME: &str = "architecture_overview";

/// Fixed v1 limitations, in the order returned by every overview response.
pub const ARCHITECTURE_OVERVIEW_LIMITATIONS: [&str; 3] = [
    "source_fact_aggregate_only_no_relationship_inference",
    "top_level_path_buckets_are_not_package_or_ownership_boundaries",
    "function_named_main_candidates_are_not_runtime_entry_point_proof",
];

/// Version-1 wire input for `architecture_overview`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureOverviewInput {
    /// Maximum structural source-root summaries to return, from 1 through 500.
    pub max_roots: Option<u16>,
    /// Maximum direct-syntax `function main` candidates to return, from 1 through 500.
    pub max_entry_point_candidates: Option<u16>,
    /// Maximum exact per-file receipts to return, from 1 through 1,000.
    pub max_files: Option<u16>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl ArchitectureOverviewInput {
    pub(crate) fn validate(self) -> Result<ArchitectureOverviewServiceRequest, &'static str> {
        let max_roots = self
            .max_roots
            .unwrap_or(DEFAULT_ARCHITECTURE_OVERVIEW_ROOTS);
        if !(1..=MAX_ARCHITECTURE_OVERVIEW_ROOTS).contains(&max_roots) {
            return Err("max_roots must be between 1 and 500");
        }
        let max_entry_point_candidates = self
            .max_entry_point_candidates
            .unwrap_or(DEFAULT_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES);
        if !(1..=MAX_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES)
            .contains(&max_entry_point_candidates)
        {
            return Err("max_entry_point_candidates must be between 1 and 500");
        }
        let max_files = self
            .max_files
            .unwrap_or(DEFAULT_ARCHITECTURE_OVERVIEW_FILES);
        if !(1..=MAX_ARCHITECTURE_OVERVIEW_FILES).contains(&max_files) {
            return Err("max_files must be between 1 and 1000");
        }
        Ok(ArchitectureOverviewServiceRequest {
            max_roots,
            max_entry_point_candidates,
            max_files,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

/// Validated, owned architecture-overview request passed to the composition root.
pub struct ArchitectureOverviewServiceRequest {
    max_roots: u16,
    max_entry_point_candidates: u16,
    max_files: u16,
    timeout: Duration,
}

impl ArchitectureOverviewServiceRequest {
    /// Returns the inclusive source-root receipt ceiling.
    #[must_use]
    pub const fn max_roots(&self) -> u16 {
        self.max_roots
    }

    /// Returns the inclusive direct-syntax entry-point candidate ceiling.
    #[must_use]
    pub const fn max_entry_point_candidates(&self) -> u16 {
        self.max_entry_point_candidates
    }

    /// Returns the inclusive per-file receipt ceiling.
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

impl fmt::Debug for ArchitectureOverviewServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ArchitectureOverviewServiceRequest")
            .field("max_roots", &self.max_roots)
            .field(
                "max_entry_point_candidates",
                &self.max_entry_point_candidates,
            )
            .field("max_files", &self.max_files)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Complete direct-syntax declaration total for one persisted language and kind.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpArchitectureOverviewKind {
    /// Persisted source language: `rust`, `go`, `typescript`, `tsx`, or `python`.
    pub language: String,
    /// Persisted direct declaration kind.
    pub kind: String,
    /// Complete direct-declaration count for this exact language/kind pair.
    pub declarations: u64,
}

/// Exact structural source-root aggregate.
///
/// The root is either the repository root (with no `path`) or one top-level
/// canonical repository-path component. It deliberately does not assert a
/// package, module, ownership, or dependency boundary.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpArchitectureOverviewRoot {
    /// `repository_root` or `top_level_directory`.
    pub kind: String,
    /// Canonical byte-preserving first component for `top_level_directory`; otherwise `null`.
    pub path: Option<String>,
    /// Complete indexed-file count under this structural root.
    pub files: u64,
    /// Complete persisted direct-declaration count under this structural root.
    pub declarations: u64,
}

/// Version-1 structured response for `architecture_overview`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureOverviewOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Bounded source-only overview profile version.
    pub overview_profile: u16,
    /// Concrete source snapshot SHA-256.
    pub snapshot_sha256: String,
    /// Opaque active-generation identifier.
    pub generation: i64,
    /// Exact source-profile producer-manifest SHA-256 for the active snapshot.
    pub source_producer_manifest_sha256: String,
    /// Explicit indexing coverage established before activation.
    pub coverage: McpCoverage,
    /// Complete indexed-file count before per-file receipt truncation.
    pub total_files: u64,
    /// Complete persisted direct-declaration count before receipt truncation.
    pub total_declarations: u64,
    /// Complete structural source-root count before root truncation.
    pub total_source_roots: u64,
    /// Number of exact source-root aggregates retained in this response.
    pub source_roots_returned: u64,
    /// Whether source-root aggregates were omitted by their independent bound.
    pub source_roots_truncated: bool,
    /// Complete direct-syntax `function main` candidate count before candidate truncation.
    pub total_entry_point_candidates: u64,
    /// Number of exact entry-point candidates retained in this response.
    pub entry_point_candidates_returned: u64,
    /// Whether entry-point candidates were omitted by their independent bound.
    pub entry_point_candidates_truncated: bool,
    /// Number of exact per-file receipts retained in this response.
    pub files_returned: u64,
    /// Whether file receipts were omitted by their independent bound.
    pub files_truncated: bool,
    /// Conservative encoded application-output bytes.
    pub output_bytes: u64,
    /// Fixed explicit v1 scope boundaries in stable order.
    pub limitations: Vec<String>,
    /// Complete indexed-language totals in stable language order.
    pub languages: Vec<McpArchitectureMapLanguage>,
    /// Complete direct-declaration language/kind totals in stable order.
    pub kinds: Vec<McpArchitectureOverviewKind>,
    /// Exact structural source-root aggregates in stable structural-root order.
    pub source_roots: Vec<McpArchitectureOverviewRoot>,
    /// Exact direct-syntax `function main` candidates in path/fact order.
    pub entry_point_candidates: Vec<McpSearchMatch>,
    /// Exact per-file declaration receipts in canonical byte-path order.
    pub files: Vec<McpArchitectureMapFile>,
}

#[cfg(test)]
mod tests {
    use super::{
        ArchitectureOverviewInput, DEFAULT_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES,
        DEFAULT_ARCHITECTURE_OVERVIEW_FILES, DEFAULT_ARCHITECTURE_OVERVIEW_ROOTS,
    };

    #[test]
    fn defaults_and_independent_bounds_are_explicit() {
        let request = ArchitectureOverviewInput {
            max_roots: None,
            max_entry_point_candidates: None,
            max_files: None,
            timeout_ms: None,
        }
        .validate()
        .expect("default request should be valid");
        assert_eq!(request.max_roots(), DEFAULT_ARCHITECTURE_OVERVIEW_ROOTS);
        assert_eq!(
            request.max_entry_point_candidates(),
            DEFAULT_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES
        );
        assert_eq!(request.max_files(), DEFAULT_ARCHITECTURE_OVERVIEW_FILES);
        assert!(
            ArchitectureOverviewInput {
                max_roots: Some(0),
                max_entry_point_candidates: Some(1),
                max_files: Some(1),
                timeout_ms: None,
            }
            .validate()
            .is_err()
        );
        assert!(
            ArchitectureOverviewInput {
                max_roots: Some(1),
                max_entry_point_candidates: Some(501),
                max_files: Some(1),
                timeout_ms: None,
            }
            .validate()
            .is_err()
        );
        assert!(
            ArchitectureOverviewInput {
                max_roots: Some(1),
                max_entry_point_candidates: Some(1),
                max_files: Some(1_001),
                timeout_ms: None,
            }
            .validate()
            .is_err()
        );
    }
}
