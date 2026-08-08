use std::{fmt, time::Duration};

use repowitness_application::{ContextBuildBudget, DEFAULT_CODE_SEARCH_RESULTS, MemoryRecallQuery};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    McpCoverage, McpMemoryCoverage, McpMemoryProducer, McpMemoryRecord, McpSpan, validate_timeout,
};

/// Version-1 wire input for `context_build`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextBuildInput {
    /// Bounded literal engineering intent used by source and memory providers.
    ///
    /// Prefer `intent`; the agent-oriented `query` spelling is accepted as a
    /// compatibility alias.
    #[serde(alias = "query")]
    pub intent: String,
    /// Conservative `utf8_bytes_upper_bound_v1` content budget.
    ///
    /// Prefer `budget_units`; `max_chars` is accepted as a compatibility alias
    /// and has the same conservative byte-budget semantics, not an exact
    /// Unicode-character limit.
    #[serde(alias = "max_chars")]
    pub budget_units: Option<u64>,
    /// Maximum independently returned candidates from each implemented provider.
    pub max_provider_results: Option<u16>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for ContextBuildInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextBuildInput")
            .field("intent", &"<redacted-intent>")
            .field("budget_units", &self.budget_units)
            .field("max_provider_results", &self.max_provider_results)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl ContextBuildInput {
    pub(crate) fn validate(self) -> Result<ContextBuildServiceRequest, &'static str> {
        let source = repowitness_application::CodeSearchQuery::try_new(&self.intent)
            .map_err(|_| "intent does not satisfy the bounded literal source profile")?;
        MemoryRecallQuery::try_new(&self.intent)
            .map_err(|_| "intent does not satisfy the bounded literal memory profile")?;
        let budget = match self.budget_units {
            Some(units) => ContextBuildBudget::try_new(units)
                .map_err(|_| "budget_units is outside the Phase 0 context bounds")?,
            None => ContextBuildBudget::default(),
        };
        let max_provider_results = self
            .max_provider_results
            .unwrap_or(DEFAULT_CODE_SEARCH_RESULTS);
        if !(1..=repowitness_application::MAX_CODE_SEARCH_RESULTS).contains(&max_provider_results) {
            return Err("max_provider_results must be between 1 and 100");
        }
        Ok(ContextBuildServiceRequest {
            intent: source.as_str().to_owned(),
            budget_units: budget.units(),
            max_provider_results,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

/// Validated owned context request passed to the composition root.
pub struct ContextBuildServiceRequest {
    intent: String,
    budget_units: u64,
    max_provider_results: u16,
    timeout: Duration,
}

impl ContextBuildServiceRequest {
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

impl fmt::Debug for ContextBuildServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextBuildServiceRequest")
            .field("intent", &"<redacted-intent>")
            .field("budget_units", &self.budget_units)
            .field("max_provider_results", &self.max_provider_results)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Exact active memory projection used by a context pack.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpContextMemoryProjection {
    /// Immutable SQLite projection identity.
    pub projection: i64,
    /// Exact source epoch revalidated by the projection.
    pub source_epoch: u64,
    /// Correspondence-producer attribution.
    pub producer: McpMemoryProducer,
    /// Complete projection coverage and state counts.
    pub coverage: McpMemoryCoverage,
}

/// Exact provider retrieval and final admission coverage.
#[derive(Clone, Copy, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpContextCoverage {
    /// Underlying source-index coverage.
    pub source_index: McpCoverage,
    /// Source matches before the lexical result bound.
    pub source_total_matches: u64,
    /// Source matches returned by lexical retrieval.
    pub source_returned_matches: u64,
    /// Source declarations not expanded because they exceeded the hard pack ceiling.
    pub source_expansion_omitted: u64,
    /// Expanded source declarations omitted by the final budget.
    pub source_budget_omitted: u64,
    /// Source declarations admitted to the pack.
    pub source_included: u64,
    /// Memory matches before the recall result bound.
    pub memory_total_matches: u64,
    /// Memory rows returned by recall.
    pub memory_returned_matches: u64,
    /// Returned non-current memory rows excluded from context.
    pub memory_non_current_omitted: u64,
    /// Current memory rows omitted by the final budget.
    pub memory_budget_omitted: u64,
    /// Current memory rows admitted to the pack.
    pub memory_included: u64,
}

/// One explicit provider, state, or budget omission.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpContextOmission {
    /// Stable omission category.
    pub kind: String,
    /// Provider when the omission is provider-specific.
    pub provider: Option<String>,
    /// Exact omitted count when one is available.
    pub count: Option<u64>,
}

/// One admitted exact source declaration.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpContextSourceItem {
    /// Original lexical-provider rank.
    pub provider_rank: u16,
    /// Pre-budget fused rank.
    pub fused_rank: u16,
    /// Denominator of `1 / (k + provider_rank)`.
    pub reciprocal_rank_denominator: u16,
    /// Conservative content units consumed.
    pub estimated_units: u64,
    /// Canonical byte-preserving repository path.
    pub path: String,
    /// Exact source-content SHA-256.
    pub content_sha256: String,
    /// Exact analysis-artifact SHA-256.
    pub artifact_sha256: String,
    /// Exact generation-local fact ordinal.
    pub fact_ordinal: u64,
    /// Syntax producer-manifest SHA-256.
    pub producer_manifest_sha256: String,
    /// Persisted source language: `rust`, `go`, `typescript`, `tsx`, or `python`.
    pub language: String,
    /// Language-specific declaration kind.
    pub declaration_kind: String,
    /// Unqualified declaration name.
    pub name: String,
    /// Deterministic lexical qualified name.
    pub qualified_name: String,
    /// Exact declaration-name span.
    pub name_span: McpSpan,
    /// Exact complete declaration span.
    pub declaration_span: McpSpan,
    /// Exact declaration representation: `utf8` or `lowercase_hex`.
    pub declaration_encoding: String,
    /// Exact untrusted declaration bytes in the declared representation.
    pub declaration: String,
}

/// One admitted current engineering-memory record.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpContextMemoryItem {
    /// Original memory-provider rank.
    pub provider_rank: u16,
    /// Pre-budget fused rank.
    pub fused_rank: u16,
    /// Denominator of `1 / (k + provider_rank)`.
    pub reciprocal_rank_denominator: u16,
    /// Conservative content units consumed.
    pub estimated_units: u64,
    /// Complete selected projected memory and correspondence evidence.
    pub record: McpMemoryRecord,
}

/// One heterogeneous item in deterministic fusion order.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpContextItem {
    /// Current engineering memory.
    Memory(McpContextMemoryItem),
    /// Exact verified source declaration.
    Source(McpContextSourceItem),
}

/// Version-2 structured response for `context_build`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextBuildOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Context fusion/admission profile version.
    pub context_profile: u16,
    /// Fixed reciprocal-rank-fusion constant.
    pub reciprocal_rank_k: u16,
    /// Stable estimator label; never an exact model-token claim.
    pub budget_estimator: String,
    /// Admitted conservative content budget.
    pub budget_units: u64,
    /// Conservative content units consumed.
    pub used_units: u64,
    /// Canonical intent SHA-256.
    pub query_sha256: String,
    /// Exact active source snapshot SHA-256.
    pub snapshot_sha256: String,
    /// Exact active generation.
    pub generation: i64,
    /// Active memory projection when available.
    pub memory: Option<McpContextMemoryProjection>,
    /// Exact retrieval and admission coverage.
    pub coverage: McpContextCoverage,
    /// Explicit omissions and unavailable providers.
    pub omissions: Vec<McpContextOmission>,
    /// Admitted context in deterministic fusion order.
    pub items: Vec<McpContextItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_is_bounded_canonical_and_redacted() {
        let request = ContextBuildInput {
            intent: "  Publish\tAtomic ".to_owned(),
            budget_units: Some(4096),
            max_provider_results: Some(7),
            timeout_ms: Some(1000),
        };
        let debug = format!("{request:?}");
        assert!(!debug.contains("Publish"));
        let request = request.validate().expect("request");
        assert_eq!(request.intent(), "Publish Atomic");
        assert_eq!(request.budget_units(), 4096);
        assert_eq!(request.max_provider_results(), 7);
    }

    #[test]
    fn agent_oriented_aliases_map_to_the_canonical_context_request() {
        let input: ContextBuildInput = serde_json::from_value(serde_json::json!({
            "query": "  Trace Dispatcher  ",
            "max_chars": 12_000,
            "max_provider_results": 7,
        }))
        .expect("compatibility aliases deserialize");

        let request = input.validate().expect("request");
        assert_eq!(request.intent(), "Trace Dispatcher");
        assert_eq!(request.budget_units(), 12_000);
        assert_eq!(request.max_provider_results(), 7);
    }

    #[test]
    fn aliases_do_not_allow_ambiguous_or_unknown_context_fields() {
        for input in [
            serde_json::json!({"intent": "run", "query": "different"}),
            serde_json::json!({"budget_units": 4096, "max_chars": 8192, "intent": "run"}),
            serde_json::json!({"intent": "run", "unknown": true}),
        ] {
            assert!(serde_json::from_value::<ContextBuildInput>(input).is_err());
        }
    }

    #[test]
    fn invalid_budget_results_and_intent_fail_at_the_wire_boundary() {
        for input in [
            ContextBuildInput {
                intent: String::new(),
                budget_units: None,
                max_provider_results: None,
                timeout_ms: None,
            },
            ContextBuildInput {
                intent: "x".to_owned(),
                budget_units: Some(0),
                max_provider_results: None,
                timeout_ms: None,
            },
            ContextBuildInput {
                intent: "x".to_owned(),
                budget_units: None,
                max_provider_results: Some(101),
                timeout_ms: None,
            },
        ] {
            assert!(input.validate().is_err());
        }
    }
}
