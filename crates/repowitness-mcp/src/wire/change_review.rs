use std::{fmt, time::Duration};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::validate_timeout;

/// Native read-only MCP tool name for revision-pinned change review.
pub const CHANGE_REVIEW_TOOL_NAME: &str = "verify";
/// Stable JSON output schema version for `verify`.
pub const CHANGE_REVIEW_SCHEMA_VERSION: u16 = 2;

/// Bounded MCP input for one revision-pinned change review.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangeReviewInput {
    /// Complete canonical lower-case SHA-1 or SHA-256 base object identifier.
    pub base: String,
    /// Bounded literal review intent for indexed source and memory context.
    pub intent: String,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for ChangeReviewInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChangeReviewInput")
            .field("base", &"<redacted-object-id>")
            .field("intent", &"<redacted-intent>")
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl ChangeReviewInput {
    pub(crate) fn validate(self) -> Result<ChangeReviewServiceRequest, &'static str> {
        if !is_canonical_full_object_id(&self.base) {
            return Err("base must be a complete lower-case SHA-1 or SHA-256 object identifier");
        }
        repowitness_application::CodeSearchQuery::try_new(&self.intent)
            .map_err(|_| "intent does not satisfy the bounded literal source profile")?;
        repowitness_application::MemoryRecallQuery::try_new(&self.intent)
            .map_err(|_| "intent does not satisfy the bounded literal memory profile")?;
        Ok(ChangeReviewServiceRequest {
            base: self.base,
            intent: self.intent,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

/// Validated owned change-review request passed to the composition root.
pub struct ChangeReviewServiceRequest {
    base: String,
    intent: String,
    timeout: Duration,
}

impl ChangeReviewServiceRequest {
    /// Returns the canonical explicit base object identifier.
    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }
    /// Returns the bounded literal review intent.
    #[must_use]
    pub fn intent(&self) -> &str {
        &self.intent
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

impl fmt::Debug for ChangeReviewServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChangeReviewServiceRequest")
            .field("base", &"<redacted-object-id>")
            .field("intent", &"<redacted-intent>")
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// One bounded exact changed path in a review receipt.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpChangeReviewPath {
    /// Stable categorical change kind.
    pub kind: String,
    /// Canonical encoded repository-relative path.
    pub path: String,
    /// Human-readable path; the canonical encoded path remains the identity.
    pub display_path: String,
}

/// Bounded source-fenced, revision-pinned change-review receipt.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangeReviewOutput {
    /// Version of this JSON output schema.
    pub schema_version: u16,
    /// Version of the derived change-manifest profile.
    pub change_manifest_profile: u16,
    /// Exact verified base object identifier.
    pub base: String,
    /// Opaque fenced current-worktree Git-state digest.
    pub worktree_git_state_sha256: String,
    /// Exact current-worktree path changes.
    pub changes: Vec<McpChangeReviewPath>,
    /// `available` or `unavailable`; unavailable context is never replaced with stale source.
    pub indexed_context_availability: String,
    /// Stable reason when indexed context is unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_context_reason: Option<String>,
    /// Snapshot identity of the separately pinned indexed context when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_snapshot_sha256: Option<String>,
    /// Immutable generation of the separately pinned indexed context when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_generation: Option<u64>,
    /// Number of retained context items when indexed context is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_context_items: Option<u64>,
    /// Number of explicit context omissions when indexed context is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_context_omissions: Option<u64>,
    /// `verified`, `mismatch`, or `unavailable` based on an exact local source comparison.
    pub index_worktree_alignment: String,
    /// Always `not_provided`; this tool never approves or rejects a change.
    pub verdict: String,
}

fn is_canonical_full_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::ChangeReviewInput;
    #[test]
    fn input_accepts_only_full_canonical_base_ids() {
        assert!(
            ChangeReviewInput {
                base: "ab".repeat(20),
                intent: "review parser".to_owned(),
                timeout_ms: None
            }
            .validate()
            .is_ok()
        );
        assert!(
            ChangeReviewInput {
                base: "HEAD".to_owned(),
                intent: "review parser".to_owned(),
                timeout_ms: None
            }
            .validate()
            .is_err()
        );
    }
}
