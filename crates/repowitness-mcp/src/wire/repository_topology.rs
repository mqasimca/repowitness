//! Versioned wire types for the bounded path-only repository topology inventory.

use std::{fmt, time::Duration};

use repowitness_application::{DEFAULT_REPOSITORY_TOPOLOGY_PATHS, MAX_REPOSITORY_TOPOLOGY_PATHS};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::validate_timeout;

/// Native tool name for the bounded repository topology inventory.
pub const REPOSITORY_TOPOLOGY_TOOL_NAME: &str = "repository_topology";

/// Version-1 input for `repository_topology`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepositoryTopologyInput {
    /// Maximum exact path receipts to return, from 1 through 1,000.
    pub max_paths: Option<u16>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl RepositoryTopologyInput {
    pub(crate) fn validate(self) -> Result<RepositoryTopologyServiceRequest, &'static str> {
        let max_paths = self.max_paths.unwrap_or(DEFAULT_REPOSITORY_TOPOLOGY_PATHS);
        if !(1..=MAX_REPOSITORY_TOPOLOGY_PATHS).contains(&max_paths) {
            return Err("max_paths must be between 1 and 1000");
        }
        Ok(RepositoryTopologyServiceRequest {
            max_paths,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

/// Validated owned topology request passed to the composition root.
pub struct RepositoryTopologyServiceRequest {
    max_paths: u16,
    timeout: Duration,
}

impl RepositoryTopologyServiceRequest {
    /// Returns the returned-path ceiling.
    #[must_use]
    pub const fn max_paths(&self) -> u16 {
        self.max_paths
    }
    /// Returns the end-to-end deadline.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl fmt::Debug for RepositoryTopologyServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryTopologyServiceRequest")
            .field("max_paths", &self.max_paths)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Complete topology category total.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpRepositoryTopologyCategory {
    /// Fixed path-only category.
    pub category: String,
    /// Complete path count before returned-entry truncation.
    pub paths: u64,
}

/// One exact path-only topology entry.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpRepositoryTopologyEntry {
    /// Canonical byte-preserving repository path.
    pub path: String,
    /// Fixed path-only category.
    pub category: String,
}

/// Explicit path-discovery coverage.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpRepositoryTopologyCoverage {
    /// Complete Git-discovered path count.
    pub discovered_paths: u64,
    /// Paths omitted before publication; version 1 is always zero.
    pub omitted_paths: u64,
}

/// Version-1 structured response for `repository_topology`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryTopologyOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Path-only topology profile version.
    pub topology_profile: u16,
    /// Source snapshot paired with the topology receipt.
    pub snapshot_sha256: String,
    /// Opaque active-generation identifier.
    pub generation: i64,
    /// Separate path-only topology digest.
    pub topology_sha256: String,
    /// Explicit discovery coverage.
    pub coverage: McpRepositoryTopologyCoverage,
    /// Complete path count before returned-entry truncation.
    pub total_paths: u64,
    /// Number of exact entries returned.
    pub paths_returned: u64,
    /// Whether entries were bounded before all paths were returned.
    pub truncated: bool,
    /// Conservative encoded application-output bytes.
    pub output_bytes: u64,
    /// Fixed scope boundary: `inventory_only_no_semantic_relationship_inference`.
    pub limitation: String,
    /// Complete category totals in stable category order.
    pub categories: Vec<McpRepositoryTopologyCategory>,
    /// Exact topology entries in canonical byte-path order.
    pub entries: Vec<McpRepositoryTopologyEntry>,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{DEFAULT_REPOSITORY_TOPOLOGY_PATHS, RepositoryTopologyInput};

    #[test]
    fn topology_input_uses_the_fixed_default_and_accepts_the_compiled_ceiling() {
        let default = RepositoryTopologyInput {
            max_paths: None,
            timeout_ms: None,
        }
        .validate()
        .expect("default topology input should validate");
        assert_eq!(default.max_paths(), DEFAULT_REPOSITORY_TOPOLOGY_PATHS);

        let maximum = RepositoryTopologyInput {
            max_paths: Some(1_000),
            timeout_ms: Some(1),
        }
        .validate()
        .expect("maximum topology input should validate");
        assert_eq!(maximum.max_paths(), 1_000);
        assert_eq!(maximum.timeout(), Duration::from_millis(1));
    }

    #[test]
    fn topology_input_rejects_zero_and_out_of_range_path_bounds() {
        for max_paths in [0, 1_001] {
            assert!(
                RepositoryTopologyInput {
                    max_paths: Some(max_paths),
                    timeout_ms: None,
                }
                .validate()
                .is_err()
            );
        }
    }
}
