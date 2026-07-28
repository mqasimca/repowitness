use std::{fmt, time::Duration};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{McpCoverage, McpMemoryCoverage, validate_timeout};

/// Version-1 wire input for read-only repository diagnostics.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsInput {
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl DiagnosticsInput {
    pub(crate) fn validate(self) -> Result<DiagnosticsServiceRequest, &'static str> {
        Ok(DiagnosticsServiceRequest {
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

impl fmt::Debug for DiagnosticsInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DiagnosticsInput")
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

/// Validated diagnostics request passed to the composition root.
pub struct DiagnosticsServiceRequest {
    timeout: Duration,
}

impl DiagnosticsServiceRequest {
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

impl fmt::Debug for DiagnosticsServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DiagnosticsServiceRequest")
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Complete active memory projection matching the diagnostic source state.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpDiagnosticsMemoryProjection {
    /// Immutable SQLite projection identity.
    pub projection: i64,
    /// Exact source epoch revalidated by the projection.
    pub source_epoch: u64,
    /// Exact source snapshot SHA-256 revalidated by the projection.
    pub snapshot_sha256: String,
    /// Complete projection coverage and effective-state counts.
    pub coverage: McpMemoryCoverage,
}

/// Version-1 structured response for read-only repository diagnostics.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Repository-diagnostics profile version.
    pub diagnostics_profile: u16,
    /// Exact active source snapshot SHA-256.
    pub snapshot_sha256: String,
    /// Exact active generation.
    pub generation: i64,
    /// Active workspace source epoch.
    pub source_epoch: u64,
    /// Producer-manifest SHA-256 for the active source snapshot.
    pub producer_manifest_sha256: String,
    /// Complete active-index coverage.
    pub index_coverage: McpCoverage,
    /// Matching complete memory projection, or `null` when none exists.
    pub memory_projection: Option<McpDiagnosticsMemoryProjection>,
    /// Supported source languages in stable order.
    pub supported_languages: Vec<String>,
    /// Implemented evidence capabilities in stable order.
    pub capabilities: Vec<String>,
    /// Explicit Phase 0 limitations in stable order.
    pub limitations: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_defaults_and_timeout_bounds_are_exact() {
        let request = DiagnosticsInput { timeout_ms: None }
            .validate()
            .expect("default timeout");
        assert_eq!(request.timeout(), Duration::from_secs(5));
        assert_eq!(
            format!("{request:?}"),
            "DiagnosticsServiceRequest { timeout: 5s }"
        );

        assert!(
            DiagnosticsInput {
                timeout_ms: Some(0)
            }
            .validate()
            .is_err()
        );
        assert!(
            DiagnosticsInput {
                timeout_ms: Some(30_001)
            }
            .validate()
            .is_err()
        );
        assert!(serde_json::from_str::<DiagnosticsInput>(r#"{"repository":"/private"}"#).is_err());
    }
}
