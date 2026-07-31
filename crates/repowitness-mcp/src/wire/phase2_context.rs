use std::{fmt, time::Duration};

use repowitness_application::{
    ContextBuildBudget, DEFAULT_CODE_SEARCH_RESULTS, MemoryRecallQuery, Phase2ContextBudget,
    ScipSymbol,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::validate_timeout;

/// Version-1 wire input for the separately versioned Phase 2 context profile.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Phase2ContextBuildInput {
    /// Bounded literal engineering intent used by the implemented providers.
    pub intent: String,
    /// Conservative `utf8_bytes_upper_bound_v1` content budget.
    pub budget_units: Option<u64>,
    /// Maximum independently returned candidates from each implemented provider.
    pub max_provider_results: Option<u16>,
    /// Optional exact opaque SCIP symbol for the precision-overlay provider.
    pub scip_symbol: Option<String>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for Phase2ContextBuildInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Phase2ContextBuildInput")
            .field("intent", &"<redacted-intent>")
            .field("budget_units", &self.budget_units)
            .field("max_provider_results", &self.max_provider_results)
            .field(
                "scip_symbol",
                &self.scip_symbol.as_ref().map(|_| "<redacted-scip-symbol>"),
            )
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl Phase2ContextBuildInput {
    pub(crate) fn validate(self) -> Result<Phase2ContextBuildServiceRequest, &'static str> {
        let source = repowitness_application::CodeSearchQuery::try_new(&self.intent)
            .map_err(|_| "intent does not satisfy the bounded literal source profile")?;
        MemoryRecallQuery::try_new(&self.intent)
            .map_err(|_| "intent does not satisfy the bounded literal memory profile")?;
        let budget = match self.budget_units {
            Some(units) => Phase2ContextBudget::try_new(units)
                .map_err(|_| "budget_units is outside the Phase 2 context bounds")?,
            None => Phase2ContextBudget::try_new(ContextBuildBudget::default().units())
                .expect("the Phase 0 default budget is valid for Phase 2"),
        };
        let max_provider_results = self
            .max_provider_results
            .unwrap_or(DEFAULT_CODE_SEARCH_RESULTS);
        if !(1..=repowitness_application::MAX_CODE_SEARCH_RESULTS).contains(&max_provider_results) {
            return Err("max_provider_results must be between 1 and 100");
        }
        let scip_symbol = self
            .scip_symbol
            .map(ScipSymbol::try_new)
            .transpose()
            .map_err(|_| "scip_symbol does not satisfy the bounded opaque-symbol profile")?
            .map(ScipSymbol::into_string);
        Ok(Phase2ContextBuildServiceRequest {
            intent: source.as_str().to_owned(),
            budget_units: budget.units(),
            max_provider_results,
            scip_symbol,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

/// Validated owned Phase 2 context request passed to the composition root.
pub struct Phase2ContextBuildServiceRequest {
    intent: String,
    budget_units: u64,
    max_provider_results: u16,
    scip_symbol: Option<String>,
    timeout: Duration,
}

impl Phase2ContextBuildServiceRequest {
    /// Returns canonical literal source intent.
    #[must_use]
    pub fn intent(&self) -> &str {
        &self.intent
    }

    /// Returns conservative content-budget units.
    #[must_use]
    pub const fn budget_units(&self) -> u64 {
        self.budget_units
    }

    /// Returns the independent provider result ceiling.
    #[must_use]
    pub const fn max_provider_results(&self) -> u16 {
        self.max_provider_results
    }

    /// Returns the optional validated opaque SCIP symbol.
    #[must_use]
    pub fn scip_symbol(&self) -> Option<&str> {
        self.scip_symbol.as_deref()
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

impl fmt::Debug for Phase2ContextBuildServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Phase2ContextBuildServiceRequest")
            .field("intent", &"<redacted-intent>")
            .field("budget_units", &self.budget_units)
            .field("max_provider_results", &self.max_provider_results)
            .field(
                "scip_symbol",
                &self.scip_symbol.as_ref().map(|_| "<redacted-scip-symbol>"),
            )
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// One immutable source member used by every included Phase 2 candidate.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpPhase2ContextScope {
    /// Repository identity SHA-256.
    pub repository_sha256: String,
    /// Connected workspace identity SHA-256.
    pub connected_workspace_sha256: String,
    /// Immutable workspace-view identifier.
    pub workspace_view: i64,
    /// Source-slot identity SHA-256.
    pub source_slot_sha256: String,
    /// Exact source epoch.
    pub source_epoch: u64,
    /// Exact source generation.
    pub generation: i64,
    /// Exact source snapshot SHA-256.
    pub snapshot_sha256: String,
    /// Exact source manifest SHA-256.
    pub manifest_sha256: String,
}

/// One provider attribution retained after exact duplicate grouping.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpPhase2ContextAttribution {
    /// Stable provider identity SHA-256.
    pub provider_sha256: String,
    /// Evidence category for this attribution.
    pub tier: String,
    /// Provider-local deterministic relevance rank.
    pub provider_rank: u32,
}

/// One categorical whole-item omission.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpPhase2ContextOmission {
    /// Evidence category whose complete items did not fit.
    pub tier: String,
    /// Exact number of whole items omitted in this category.
    pub count: u64,
}

/// Categorical availability for one provider before context allocation.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpPhase2ContextProviderCoverage {
    /// Evidence tier supplied by this provider.
    pub tier: String,
    /// `available` when complete candidates were admissible; otherwise `unavailable`.
    pub availability: String,
    /// Candidate count before duplicate grouping and allocation.
    pub candidate_count: u64,
}

/// Exact bounded Phase 2 item payload.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpPhase2ContextPayload {
    /// Exact verified syntax declaration.
    Syntax {
        /// Canonical byte-preserving repository path.
        path: String,
        /// Exact source-content SHA-256.
        content_sha256: String,
        /// Exact analysis-artifact SHA-256.
        artifact_sha256: String,
        /// Exact generation-local fact ordinal.
        fact_ordinal: u64,
        /// Exact declaration representation: `utf8` or `lowercase_hex`.
        declaration_encoding: String,
        /// Exact untrusted declaration bytes in the declared representation.
        declaration: String,
    },
    /// Current evidence-backed engineering-memory claim.
    Memory {
        /// Stable memory record identity SHA-256.
        record_id_sha256: String,
        /// Complete selected projected memory and correspondence evidence.
        record: Box<super::McpMemoryRecord>,
    },
    /// Current approved memory with one immutable historical Git observation receipt.
    History {
        /// Stable memory record identity SHA-256.
        record_id_sha256: String,
        /// Git object format carried by the immutable observation.
        commit_object_format: String,
        /// Exact Git object identifier, encoded as lowercase hexadecimal.
        commit_object_id_hex: String,
        /// Complete selected projected memory and correspondence evidence.
        record: Box<super::McpMemoryRecord>,
    },
    /// Exact source-verified occurrence from an immutable SCIP precision overlay.
    PreciseOverlay {
        /// Immutable overlay receipt SHA-256.
        overlay_sha256: String,
        /// Canonical byte-preserving repository path.
        path: String,
        /// Exact source-content SHA-256.
        content_sha256: String,
        /// Exact half-open source-byte span start.
        span_start: u64,
        /// Exact half-open source-byte span end.
        span_end: u64,
        /// Preserved producer occurrence role bits.
        roles: u32,
        /// Number of complete validated relationships retained for the exact symbol.
        relationship_count: u64,
        /// Exact source-span representation: `utf8` or `lowercase_hex`.
        source_encoding: String,
        /// Exact untrusted source bytes in the declared representation.
        source: String,
    },
    /// Exact source declaration reached through a unique pinned syntax-graph edge.
    GraphRelation {
        /// Retained graph relationship category.
        edge_kind: String,
        /// One-based graph traversal depth.
        depth: u32,
        /// Canonical byte-preserving repository path.
        path: String,
        /// Exact source-content SHA-256.
        content_sha256: String,
        /// Exact analysis-artifact SHA-256.
        artifact_sha256: String,
        /// Exact generation-local fact ordinal.
        fact_ordinal: u64,
        /// Exact declaration representation: `utf8` or `lowercase_hex`.
        declaration_encoding: String,
        /// Exact untrusted declaration bytes in the declared representation.
        declaration: String,
    },
}

/// One deterministic selected Phase 2 candidate.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpPhase2ContextItem {
    /// Evidence tier after versioned profile selection.
    pub tier: String,
    /// Provider-local deterministic rank.
    pub provider_rank: u32,
    /// Conservative whole-item content cost.
    pub estimated_units: u64,
    /// Stable candidate identity SHA-256.
    pub identity_sha256: String,
    /// Complete retained contributing providers.
    pub providers: Vec<McpPhase2ContextAttribution>,
    /// Exact bounded item payload.
    pub payload: McpPhase2ContextPayload,
}

/// Version-1 structured response for `phase2_context_build`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Phase2ContextBuildOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Immutable named profile identifier.
    pub profile_id: String,
    /// Immutable named profile version.
    pub profile_version: u16,
    /// Stable estimator label; never an exact model-token claim.
    pub budget_estimator: String,
    /// Admitted conservative content budget.
    pub budget_units: u64,
    /// Conservative content units consumed.
    pub used_units: u64,
    /// One common pinned source scope for every selected item.
    pub scope: McpPhase2ContextScope,
    /// Provider availability before allocation.
    pub provider_coverage: Vec<McpPhase2ContextProviderCoverage>,
    /// Whole-item budget omissions by evidence tier.
    pub omissions: Vec<McpPhase2ContextOmission>,
    /// Selected context in deterministic profile order.
    pub items: Vec<McpPhase2ContextItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_is_bounded_canonical_and_redacted() {
        let input = Phase2ContextBuildInput {
            intent: "  Publish\tAtomic ".to_owned(),
            budget_units: Some(4096),
            max_provider_results: Some(7),
            scip_symbol: Some("scip-rust pkg 0/Widget#".to_owned()),
            timeout_ms: Some(1000),
        };
        assert!(!format!("{input:?}").contains("Publish"));
        let request = input.validate().expect("valid request");
        assert_eq!(request.intent(), "Publish Atomic");
        assert_eq!(request.budget_units(), 4096);
        assert_eq!(request.max_provider_results(), 7);
        assert_eq!(request.scip_symbol(), Some("scip-rust pkg 0/Widget#"));
    }

    #[test]
    fn invalid_phase2_input_fails_at_the_wire_boundary() {
        for input in [
            Phase2ContextBuildInput {
                intent: String::new(),
                budget_units: None,
                max_provider_results: None,
                scip_symbol: None,
                timeout_ms: None,
            },
            Phase2ContextBuildInput {
                intent: "x".to_owned(),
                budget_units: Some(0),
                max_provider_results: None,
                scip_symbol: None,
                timeout_ms: None,
            },
            Phase2ContextBuildInput {
                intent: "x".to_owned(),
                budget_units: None,
                max_provider_results: Some(101),
                scip_symbol: None,
                timeout_ms: None,
            },
        ] {
            assert!(input.validate().is_err());
        }
    }
}
