use std::{fmt, time::Duration};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::validate_timeout;

/// Schema version for the explicit profile-pinned personal-memory tool.
pub const PERSONAL_MEMORY_SCHEMA_VERSION: u16 = 1;

const DEFAULT_PERSONAL_MEMORY_RESULTS: u16 = 20;
const MAX_PERSONAL_MEMORY_RESULTS: u16 = 100;

/// Explicit personal-memory operation. The local profile is fixed at MCP startup
/// and is intentionally not accepted from the caller.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonalMemoryOperation {
    /// Read the exact startup-profile and startup-repository partition.
    Read,
    /// Append one immutable local-only revision to that partition.
    Append,
}

/// Allowed Phase 3 local-only memory kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonalMemoryKind {
    /// A non-source-derivable fact.
    Fact,
    /// A decision and its rationale.
    Decision,
    /// A procedure, which still requires independent verification before guidance use.
    Procedure,
    /// A bounded historical event.
    Episode,
    /// A local preference.
    Preference,
    /// A local policy or guardrail.
    Policy,
    /// A failed approach.
    Failure,
}

/// Explicit personal-memory lifecycle.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonalMemoryLifecycle {
    /// Currently usable local memory.
    Active,
    /// Requires an explicit local review.
    NeedsReview,
    /// Retained but stale.
    Stale,
    /// Retained but contradicted.
    Contradicted,
    /// Retained as superseded history.
    Superseded,
    /// Retained but quarantined.
    Quarantined,
    /// Retained tombstone.
    Tombstoned,
}

/// Version-1 input for the explicitly enabled `personal_memory` tool.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PersonalMemoryInput {
    /// Read or append within the single local profile fixed at server startup.
    pub operation: PersonalMemoryOperation,
    /// Required only by `append`.
    pub kind: Option<PersonalMemoryKind>,
    /// Required only by `append`; it is locally bounded and secret-scanned.
    pub title: Option<String>,
    /// Required only by `append`; it is locally bounded and secret-scanned.
    pub body: Option<String>,
    /// Required only by `append`.
    pub lifecycle: Option<PersonalMemoryLifecycle>,
    /// Returned records for `read`, from 1 through 100.
    pub max_results: Option<u16>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for PersonalMemoryInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PersonalMemoryInput")
            .field("operation", &self.operation)
            .field("kind", &self.kind)
            .field("lifecycle", &self.lifecycle)
            .field("max_results", &self.max_results)
            .field("timeout_ms", &self.timeout_ms)
            .finish_non_exhaustive()
    }
}

impl PersonalMemoryInput {
    pub(crate) fn validate(self) -> Result<PersonalMemoryServiceRequest, &'static str> {
        let timeout = validate_timeout(self.timeout_ms)?;
        match self.operation {
            PersonalMemoryOperation::Read => {
                if self.kind.is_some()
                    || self.title.is_some()
                    || self.body.is_some()
                    || self.lifecycle.is_some()
                {
                    return Err("read accepts only operation, max_results, and timeout_ms");
                }
                let max_results = self.max_results.unwrap_or(DEFAULT_PERSONAL_MEMORY_RESULTS);
                if !(1..=MAX_PERSONAL_MEMORY_RESULTS).contains(&max_results) {
                    return Err("read max_results must be between 1 and 100");
                }
                Ok(PersonalMemoryServiceRequest::Read {
                    max_results,
                    timeout,
                })
            }
            PersonalMemoryOperation::Append => {
                if self.max_results.is_some() {
                    return Err("append does not accept max_results");
                }
                let kind = self.kind.ok_or("append requires kind")?;
                let title = self.title.ok_or("append requires title")?;
                let body = self.body.ok_or("append requires body")?;
                let lifecycle = self.lifecycle.ok_or("append requires lifecycle")?;
                Ok(PersonalMemoryServiceRequest::Append {
                    kind,
                    title,
                    body,
                    lifecycle,
                    timeout,
                })
            }
        }
    }
}

/// Validated owned request passed to the fixed-profile local composition root.
pub enum PersonalMemoryServiceRequest {
    /// Read one exact profile/repository partition.
    Read {
        /// Inclusive bounded result count.
        max_results: u16,
        /// Remaining end-to-end deadline.
        timeout: Duration,
    },
    /// Append one immutable local-only revision.
    Append {
        /// Requested kind.
        kind: PersonalMemoryKind,
        /// Private bounded title.
        title: String,
        /// Private bounded body.
        body: String,
        /// Requested lifecycle.
        lifecycle: PersonalMemoryLifecycle,
        /// Remaining end-to-end deadline.
        timeout: Duration,
    },
}

impl PersonalMemoryServiceRequest {
    /// Returns the remaining end-to-end deadline duration.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        match self {
            Self::Read { timeout, .. } | Self::Append { timeout, .. } => *timeout,
        }
    }

    pub(crate) fn with_timeout(mut self, timeout: Duration) -> Self {
        match &mut self {
            Self::Read {
                timeout: current, ..
            }
            | Self::Append {
                timeout: current, ..
            } => *current = timeout,
        }
        self
    }
}

impl fmt::Debug for PersonalMemoryServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read {
                max_results,
                timeout,
            } => formatter
                .debug_struct("PersonalMemoryServiceRequest::Read")
                .field("max_results", max_results)
                .field("timeout", timeout)
                .finish(),
            Self::Append {
                kind,
                lifecycle,
                timeout,
                ..
            } => formatter
                .debug_struct("PersonalMemoryServiceRequest::Append")
                .field("kind", kind)
                .field("lifecycle", lifecycle)
                .field("timeout", timeout)
                .finish_non_exhaustive(),
        }
    }
}

/// Bounded local-only record returned only through an explicitly enabled profile.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PersonalMemoryRecordOutput {
    /// Opaque local record identifier.
    pub record_id: String,
    /// Immutable SHA-256 revision digest.
    pub revision_sha256: String,
    /// Versioned local-only kind.
    pub kind: PersonalMemoryKind,
    /// Private title, returned only by an explicit `read` to the enabled local profile.
    /// Append acknowledgements deliberately omit it.
    pub title: Option<String>,
    /// Private body, returned only by an explicit `read` to the enabled local profile.
    /// Append acknowledgements deliberately omit it.
    pub body: Option<String>,
    /// Explicit lifecycle state.
    pub lifecycle: PersonalMemoryLifecycle,
    /// Trusted local record timestamp.
    pub recorded_at_unix_ms: u64,
}

/// Versioned explicit-profile result. It never represents team memory.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PersonalMemoryOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Always `personal`; profile identity is intentionally omitted.
    pub scope: String,
    /// Requested operation.
    pub operation: PersonalMemoryOperation,
    /// Exact bounded local records, in deterministic storage order.
    pub records: Vec<PersonalMemoryRecordOutput>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personal_memory_wire_requires_explicit_bounded_operation_fields() {
        assert!(matches!(
            PersonalMemoryInput {
                operation: PersonalMemoryOperation::Read,
                kind: None,
                title: None,
                body: None,
                lifecycle: None,
                max_results: Some(1),
                timeout_ms: None,
            }
            .validate(),
            Ok(PersonalMemoryServiceRequest::Read { max_results: 1, .. })
        ));
        assert!(
            PersonalMemoryInput {
                operation: PersonalMemoryOperation::Read,
                kind: Some(PersonalMemoryKind::Fact),
                title: None,
                body: None,
                lifecycle: None,
                max_results: None,
                timeout_ms: None,
            }
            .validate()
            .is_err()
        );
        assert!(matches!(
            PersonalMemoryInput {
                operation: PersonalMemoryOperation::Append,
                kind: Some(PersonalMemoryKind::Preference),
                title: Some("prefer local evidence".to_owned()),
                body: Some("never publish it by default".to_owned()),
                lifecycle: Some(PersonalMemoryLifecycle::Active),
                max_results: None,
                timeout_ms: None,
            }
            .validate(),
            Ok(PersonalMemoryServiceRequest::Append { .. })
        ));
    }
}
