use std::{fmt, time::Duration};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::validate_timeout;

/// Versioned receipt schema for a bounded exact historical applicability read.
pub const HISTORICAL_MEMORY_SCHEMA_VERSION: u16 = 1;
const DEFAULT_RESULTS: u16 = 32;
const MAX_RESULTS: u16 = 100;

/// Exact target type. Branches are deliberately excluded because callers must
/// resolve them to an immutable object before asking a historical question.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalMemoryTargetKind {
    /// A lowercase SHA-1 or SHA-256 Git commit object.
    GitCommit,
    /// A lowercase SHA-256 retained worktree snapshot digest.
    WorktreeSnapshot,
}

/// Public input for the read-only `historical_memory` tool.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoricalMemoryInput {
    /// Inclusive recorded-time cutoff in Unix milliseconds.
    pub known_at_unix_ms: u64,
    /// Exact immutable target kind.
    pub target_kind: HistoricalMemoryTargetKind,
    /// Lowercase hexadecimal exact target identity; never a branch name.
    pub target: String,
    /// Bounded number of redacted evidence rows, from 1 through 100.
    pub max_results: Option<u16>,
    /// End-to-end deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl HistoricalMemoryInput {
    pub(crate) fn validate(self) -> Result<HistoricalMemoryServiceRequest, &'static str> {
        let max_results = self.max_results.unwrap_or(DEFAULT_RESULTS);
        if !(1..=MAX_RESULTS).contains(&max_results) {
            return Err("max_results must be between 1 and 100");
        }
        let target = match self.target_kind {
            HistoricalMemoryTargetKind::GitCommit => {
                if !matches!(self.target.len(), 40 | 64) || !lower_hex(&self.target) {
                    return Err("git_commit target must be lowercase SHA-1 or SHA-256");
                }
                HistoricalMemoryTarget::GitCommit(self.target)
            }
            HistoricalMemoryTargetKind::WorktreeSnapshot => {
                if self.target.len() != 64 || !lower_hex(&self.target) {
                    return Err("worktree_snapshot target must be lowercase SHA-256");
                }
                HistoricalMemoryTarget::WorktreeSnapshot(self.target)
            }
        };
        Ok(HistoricalMemoryServiceRequest {
            known_at_unix_ms: self.known_at_unix_ms,
            target,
            max_results,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

fn lower_hex(value: &str) -> bool {
    value
        .as_bytes()
        .iter()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Validated exact target passed to the local composition root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HistoricalMemoryTarget {
    /// Exact existing-or-pruned Git object identity.
    GitCommit(String),
    /// Exact retained source snapshot digest.
    WorktreeSnapshot(String),
}

/// Validated bounded historical read request.
#[derive(Clone, Debug)]
pub struct HistoricalMemoryServiceRequest {
    known_at_unix_ms: u64,
    target: HistoricalMemoryTarget,
    max_results: u16,
    timeout: Duration,
}

impl HistoricalMemoryServiceRequest {
    /// Returns the inclusive recorded-time cutoff.
    #[must_use]
    pub const fn known_at_unix_ms(&self) -> u64 {
        self.known_at_unix_ms
    }

    /// Returns the exact immutable target.
    #[must_use]
    pub fn target(&self) -> &HistoricalMemoryTarget {
        &self.target
    }

    /// Returns the bounded evidence result count.
    #[must_use]
    pub const fn max_results(&self) -> u16 {
        self.max_results
    }

    /// Returns the remaining operation timeout.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Retained coverage category returned independently of applicability.
#[derive(Clone, Copy, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalMemoryCoverage {
    /// All matching durable evidence fit the selected bound.
    Complete,
    /// At least one matching durable evidence row was omitted by the bound.
    Truncated,
}

/// Fail-closed concrete-target applicability category.
#[derive(Clone, Copy, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalMemoryApplicability {
    /// The target could not be checked from retained source or Git objects.
    Unavailable,
    /// The target was available but no pre-cutoff approved evidence applied.
    NotApplicable,
    /// At least one pre-cutoff approved evidence relation applied.
    Applicable,
}

/// Redacted basis for a historical evidence relation.
#[derive(Clone, Copy, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalMemoryEvidenceBasis {
    /// Exact approved source observation.
    Observation,
    /// Exact non-conflicted archival correspondence review.
    ReviewedCorrespondence,
}

/// One redacted immutable evidence identity.
#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoricalMemoryEvidence {
    /// Canonical record identifier.
    pub record_id: String,
    /// Canonical immutable revision SHA-256.
    pub revision_sha256: String,
    /// Evidence relation that applied to the requested exact target.
    pub basis: HistoricalMemoryEvidenceBasis,
}

/// Bounded path-free historical read receipt.
#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoricalMemoryOutput {
    /// Fixed output schema version.
    pub schema_version: u16,
    /// Independent retained-audit coverage category.
    pub coverage: HistoricalMemoryCoverage,
    /// Fail-closed exact target applicability category.
    pub applicability: HistoricalMemoryApplicability,
    /// Bounded redacted exact evidence identities.
    pub evidence: Vec<HistoricalMemoryEvidence>,
}

impl HistoricalMemoryOutput {
    /// Constructs an adapter-owned validated receipt.
    #[must_use]
    pub const fn new(
        coverage: HistoricalMemoryCoverage,
        applicability: HistoricalMemoryApplicability,
        evidence: Vec<HistoricalMemoryEvidence>,
    ) -> Self {
        Self {
            schema_version: HISTORICAL_MEMORY_SCHEMA_VERSION,
            coverage,
            applicability,
            evidence,
        }
    }
}

impl fmt::Display for HistoricalMemoryTargetKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::GitCommit => "git_commit",
            Self::WorktreeSnapshot => "worktree_snapshot",
        })
    }
}
