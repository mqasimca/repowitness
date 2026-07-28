use std::{fmt, time::Duration};

use repowitness_application::{
    DEFAULT_MEMORY_RECALL_RESULTS, MAX_MEMORY_RECALL_RESULTS, MemoryRecallQuery,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::validate_timeout;

/// Version-1 wire input for `memory_recall`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecallInput {
    /// Literal memory title/body terms. Omit only when `all_records` is true.
    pub query: Option<String>,
    /// Explicitly select all projected records instead of a literal query.
    #[serde(default)]
    pub all_records: bool,
    /// Maximum returned projected records, from 1 through 100.
    pub max_results: Option<u16>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for MemoryRecallInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryRecallInput")
            .field("query", &self.query.as_ref().map(|_| "<redacted-query>"))
            .field("all_records", &self.all_records)
            .field("max_results", &self.max_results)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl MemoryRecallInput {
    pub(crate) fn validate(self) -> Result<MemoryRecallServiceRequest, &'static str> {
        let selection = match (self.query, self.all_records) {
            (Some(query), false) => {
                let query = MemoryRecallQuery::try_new(&query)
                    .map_err(|_| "query does not satisfy the bounded literal memory profile")?;
                MemoryRecallServiceSelection::Query(
                    query
                        .as_str()
                        .expect("validated literal memory query has canonical text")
                        .to_owned(),
                )
            }
            (None, true) => MemoryRecallServiceSelection::All,
            _ => return Err("select exactly one of query or all_records=true"),
        };
        let max_results = self.max_results.unwrap_or(DEFAULT_MEMORY_RECALL_RESULTS);
        if !(1..=MAX_MEMORY_RECALL_RESULTS).contains(&max_results) {
            return Err("max_results must be between 1 and 100");
        }
        Ok(MemoryRecallServiceRequest {
            selection,
            max_results,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

/// Validated owned selection passed to the composition root.
#[derive(Clone, Eq, PartialEq)]
pub enum MemoryRecallServiceSelection {
    /// Return all projected records subject to result bounds.
    All,
    /// Match canonical literal title/body terms.
    Query(String),
}

impl fmt::Debug for MemoryRecallServiceSelection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::All => "All",
            Self::Query(_) => "Query(<redacted-query>)",
        })
    }
}

/// Validated owned memory-recall request passed to the composition root.
pub struct MemoryRecallServiceRequest {
    selection: MemoryRecallServiceSelection,
    max_results: u16,
    timeout: Duration,
}

impl MemoryRecallServiceRequest {
    /// Returns the explicit all-records or canonical literal selection.
    #[must_use]
    pub const fn selection(&self) -> &MemoryRecallServiceSelection {
        &self.selection
    }

    /// Returns the inclusive result-count bound.
    #[must_use]
    pub const fn max_results(&self) -> u16 {
        self.max_results
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

impl fmt::Debug for MemoryRecallServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryRecallServiceRequest")
            .field("selection", &self.selection)
            .field("max_results", &self.max_results)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Exact source target used for the active memory projection.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpMemoryTarget {
    /// `git` or `worktree`.
    pub kind: String,
    /// Exact source snapshot when `kind` is `worktree`.
    pub source_snapshot_sha256: Option<String>,
    /// Git object format for a Git target or available worktree HEAD.
    pub commit_object_format: Option<String>,
    /// Lowercase Git target or worktree HEAD object ID.
    pub commit_hex: Option<String>,
}

/// Correspondence producer attribution for one projection.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpMemoryProducer {
    /// Printable correspondence-profile identifier.
    pub id: String,
    /// Positive profile version.
    pub version: u32,
    /// Complete profile SHA-256.
    pub profile_sha256: String,
}

/// Exact projection coverage and effective-state counts.
#[derive(Clone, Copy, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpMemoryCoverage {
    /// Projected rows evaluated.
    pub searched: u64,
    /// Journal records omitted by policy.
    pub skipped: u64,
    /// Rows requiring review or more evidence.
    pub unresolved: u64,
    /// Journal records omitted by a projection bound.
    pub truncated: u64,
    /// Complete projected rows.
    pub total: u64,
    /// Records currently supported by exact or trusted correspondence evidence.
    pub current: u64,
    /// Records excluded by project validity.
    pub not_applicable: u64,
    /// Records whose authored or evidence state is stale.
    pub stale: u64,
    /// Records requiring explicit review.
    pub needs_review: u64,
    /// Records lacking enough evidence for a categorical result.
    pub indeterminate: u64,
    /// Records with multiple approved immutable heads.
    pub conflicted: u64,
    /// Records explicitly contradicted.
    pub contradicted: u64,
    /// Records superseded by another record.
    pub superseded: u64,
    /// Records quarantined from normal use.
    pub quarantined: u64,
    /// Tombstoned records retained for audit history.
    pub tombstoned: u64,
}

/// Selected immutable semantic memory content.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSelectedMemory {
    /// Semantic memory schema version.
    pub schema_version: u32,
    /// Human-facing monotonic display revision.
    pub display_revision: u32,
    /// Categorical claim kind.
    pub kind: String,
    /// Selected memory title.
    pub title: String,
    /// Selected memory body.
    pub body: String,
    /// Authored assurance category.
    pub assurance: String,
    /// Authored lifecycle category.
    pub lifecycle: String,
    /// Whether the selected immutable version is a tombstone.
    pub tombstone: bool,
}

/// Exact current occurrence established by correspondence.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpMemoryOccurrence {
    /// Canonical byte-preserving repository path.
    pub path: String,
    /// Exact target source-content SHA-256.
    pub content_sha256: String,
    /// Exact target analysis-artifact SHA-256.
    pub artifact_sha256: String,
    /// Exact generation-local fact ordinal.
    pub fact_ordinal: u64,
    /// Exact declaration SHA-256.
    pub declaration_sha256: String,
    /// Exact name-elided correspondence SHA-256.
    pub name_elided_sha256: String,
}

/// One bounded ambiguous correspondence candidate.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpMemoryCandidate {
    /// Proposed categorical correspondence relation.
    pub relation: String,
    /// Exact candidate occurrence identity.
    pub occurrence: McpMemoryOccurrence,
}

/// One projected citation outcome.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpMemoryEvidence {
    /// Categorical correspondence outcome.
    pub outcome: String,
    /// Automatic, reviewed, or absent correspondence assurance.
    pub assurance: String,
    /// Resolved current occurrence, when established.
    pub target: Option<McpMemoryOccurrence>,
    /// Whether candidate enumeration was complete.
    pub candidate_coverage_complete: bool,
    /// Candidates observed before any adapter limit.
    pub candidate_count_before_limit: u64,
    /// Deterministically ordered review candidates.
    pub candidates: Vec<McpMemoryCandidate>,
}

/// One projected logical memory record.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpMemoryRecord {
    /// Canonical logical memory-record identity.
    pub record_id: String,
    /// Selected immutable revision SHA-256, absent on head conflict/failure.
    pub revision_sha256: Option<String>,
    /// Selected integrity-checked semantic memory, when head selection succeeded.
    pub selected: Option<McpSelectedMemory>,
    /// Effective freshness and eligibility state.
    pub effective_state: String,
    /// Project-validity evaluation state.
    pub validity_state: String,
    /// Aggregate correspondence evidence state.
    pub evidence_state: String,
    /// Stable categorical explanation for the effective state.
    pub reason: String,
    /// Selected version's authored citation count.
    pub evidence_count: u32,
    /// Exact or corresponded citation count.
    pub resolved_count: u32,
    /// Review-required citation count.
    pub review_count: u32,
    /// Indeterminate citation count.
    pub indeterminate_count: u32,
    /// Approved immutable heads considered.
    pub head_count: u32,
    /// Unavailable parent revision count.
    pub missing_parent_count: u32,
    /// Projected citation outcomes in authored order.
    pub evidence: Vec<McpMemoryEvidence>,
}

/// Version-1 structured response for `memory_recall`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecallOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Deterministic application recall-profile version.
    pub recall_profile: u16,
    /// Canonical literal query SHA-256, absent for all-records mode.
    pub query_sha256: Option<String>,
    /// Exact active source snapshot SHA-256.
    pub snapshot_sha256: String,
    /// Exact active index-generation identity.
    pub generation: i64,
    /// Immutable database-local memory-projection identity.
    pub projection: i64,
    /// Workspace source epoch fenced by projection publication.
    pub source_epoch: u64,
    /// Exact Git or worktree target used for revalidation.
    pub target: McpMemoryTarget,
    /// Correspondence producer attribution.
    pub producer: McpMemoryProducer,
    /// Number of records returned.
    pub matches_returned: u64,
    /// Number of matching projection rows before the result limit.
    pub matches_total: u64,
    /// Matching rows omitted by the result limit.
    pub matches_omitted: u64,
    /// Complete projection coverage and state counts.
    pub coverage: McpMemoryCoverage,
    /// Explicit Phase 0 capability limitation.
    pub limitation: String,
    /// Deterministically ordered projected records.
    pub records: Vec<McpMemoryRecord>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_and_bounds_are_explicit_and_redacted() {
        let request = MemoryRecallInput {
            query: Some("Private Decision".to_owned()),
            all_records: false,
            max_results: Some(7),
            timeout_ms: Some(100),
        }
        .validate()
        .expect("valid literal request");
        assert_eq!(request.max_results(), 7);
        assert_eq!(
            request.selection(),
            &MemoryRecallServiceSelection::Query("private decision".to_owned())
        );
        assert!(!format!("{request:?}").contains("private"));

        let all = MemoryRecallInput {
            query: None,
            all_records: true,
            max_results: None,
            timeout_ms: None,
        }
        .validate()
        .expect("explicit all-records request");
        assert_eq!(all.selection(), &MemoryRecallServiceSelection::All);

        for input in [
            MemoryRecallInput {
                query: None,
                all_records: false,
                max_results: None,
                timeout_ms: None,
            },
            MemoryRecallInput {
                query: Some("term".to_owned()),
                all_records: true,
                max_results: None,
                timeout_ms: None,
            },
            MemoryRecallInput {
                query: Some(String::new()),
                all_records: false,
                max_results: None,
                timeout_ms: None,
            },
            MemoryRecallInput {
                query: Some("term".to_owned()),
                all_records: false,
                max_results: Some(0),
                timeout_ms: None,
            },
        ] {
            assert!(input.validate().is_err());
        }
    }
}
