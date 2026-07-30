use std::{fmt, time::Duration};

use repowitness_application::{
    MemoryRecordIdTextV1, RepositoryPathLimits, RepositoryPathTextByteLimit, RepositoryPathTextV1,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    MAX_MCP_INTEROPERABLE_INTEGER, MemoryMutationRequestScope, is_lowercase_sha256,
    validate_timeout,
};

const MAX_INLINE_MEMORY_YAML_BYTES: usize = 64 * 1024;
const MAX_MEMORY_EVIDENCE_ORDINAL: u8 = 15;
const MAX_REVIEW_PATH_TEXT_BYTES: u64 = 65_535;
const MAX_REVIEW_PATH_BYTES: u64 = 32_764;
const MAX_REVIEW_PATH_COMPONENTS: u64 = 16_382;

/// Current `memory_manage` receipt schema with explicit maintenance truth.
pub const MEMORY_MANAGE_SCHEMA_VERSION: u16 = 2;

/// Allow-listed mutation selected by the opt-in `memory_manage` tool.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryManageOperation {
    /// Validate and publish one complete inline record.
    Write,
    /// Locally approve one exact current worktree record.
    Approve,
    /// Append one exact trusted correspondence decision.
    Review,
    /// Observe bounded memory versions reachable from repository HEAD.
    ImportHistory,
}

/// Allow-listed trusted correspondence-review decision.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryManageReviewDecision {
    /// Approve the exact target correspondence.
    Approve,
    /// Reject the exact target correspondence.
    Reject,
    /// Assert an explicit manual link to the exact target.
    ManualLink,
}

/// Version-1 input for the explicitly enabled `memory_manage` tool.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryManageInput {
    /// Exact mutation to perform.
    pub operation: MemoryManageOperation,
    /// Complete strict version-1 YAML record for `write`.
    pub record_yaml: Option<String>,
    /// Canonical logical record ID for `approve` or `review`.
    pub record_id: Option<String>,
    /// Exact canonical semantic revision SHA-256 for `review`.
    pub revision_sha256: Option<String>,
    /// Authored evidence ordinal, from 0 through 15, for `review`.
    pub evidence_ordinal: Option<u8>,
    /// Approve, reject, or manually link the exact review target.
    pub review_decision: Option<MemoryManageReviewDecision>,
    /// Canonical byte-preserving current target path for `review`.
    pub target_path: Option<String>,
    /// Exact current target artifact SHA-256 for `review`.
    pub target_artifact_sha256: Option<String>,
    /// Exact current target fact ordinal for `review`.
    pub target_fact_ordinal: Option<u64>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for MemoryManageInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryManageInput")
            .field("operation", &self.operation)
            .field("evidence_ordinal", &self.evidence_ordinal)
            .field("review_decision", &self.review_decision)
            .field("target_fact_ordinal", &self.target_fact_ordinal)
            .field("timeout_ms", &self.timeout_ms)
            .finish_non_exhaustive()
    }
}

impl MemoryManageInput {
    pub(crate) fn validate(self) -> Result<MemoryManageServiceRequest, &'static str> {
        let timeout = validate_timeout(self.timeout_ms)?;
        match self.operation {
            MemoryManageOperation::Write => {
                reject_non_write_fields(&self)?;
                let record_yaml = self
                    .record_yaml
                    .filter(|yaml| !yaml.is_empty() && yaml.len() <= MAX_INLINE_MEMORY_YAML_BYTES)
                    .ok_or("write requires record_yaml between 1 and 65536 UTF-8 bytes")?;
                Ok(MemoryManageServiceRequest::Write {
                    record_yaml,
                    timeout,
                })
            }
            MemoryManageOperation::Approve => {
                reject_non_approve_fields(&self)?;
                let record_id = validated_record_id(self.record_id)?;
                Ok(MemoryManageServiceRequest::Approve { record_id, timeout })
            }
            MemoryManageOperation::Review => {
                reject_non_review_fields(&self)?;
                let record_id = validated_record_id(self.record_id)?;
                let revision_sha256 = self
                    .revision_sha256
                    .filter(|digest| is_lowercase_sha256(digest))
                    .ok_or("review requires a lowercase revision_sha256")?;
                let evidence_ordinal = self
                    .evidence_ordinal
                    .filter(|ordinal| *ordinal <= MAX_MEMORY_EVIDENCE_ORDINAL)
                    .ok_or("review evidence_ordinal must be between 0 and 15")?;
                let decision = self
                    .review_decision
                    .ok_or("review requires review_decision")?;
                let target_path = self
                    .target_path
                    .filter(|path| is_canonical_review_path_text(path))
                    .ok_or("review requires a canonical target_path")?;
                let target_artifact_sha256 = self
                    .target_artifact_sha256
                    .filter(|digest| is_lowercase_sha256(digest))
                    .ok_or("review requires a lowercase target_artifact_sha256")?;
                let target_fact_ordinal = self
                    .target_fact_ordinal
                    .filter(|ordinal| *ordinal <= MAX_MCP_INTEROPERABLE_INTEGER)
                    .ok_or("review target_fact_ordinal exceeds the interoperable range")?;
                Ok(MemoryManageServiceRequest::Review {
                    record_id,
                    revision_sha256,
                    evidence_ordinal,
                    decision,
                    target_path,
                    target_artifact_sha256,
                    target_fact_ordinal,
                    timeout,
                })
            }
            MemoryManageOperation::ImportHistory => {
                reject_non_history_fields(&self)?;
                Ok(MemoryManageServiceRequest::ImportHistory { timeout })
            }
        }
    }
}

/// Validated, owned memory-management request passed to the composition root.
pub enum MemoryManageServiceRequest {
    /// Publish a complete inline record.
    Write {
        /// Complete strict version-1 YAML record.
        record_yaml: String,
        /// Remaining end-to-end deadline.
        timeout: Duration,
    },
    /// Approve an exact current worktree record.
    Approve {
        /// Canonical logical record identity.
        record_id: String,
        /// Remaining end-to-end deadline.
        timeout: Duration,
    },
    /// Append one exact trusted correspondence decision.
    Review {
        /// Canonical logical record identity.
        record_id: String,
        /// Exact selected canonical semantic revision SHA-256.
        revision_sha256: String,
        /// Authored evidence ordinal.
        evidence_ordinal: u8,
        /// Trusted review decision.
        decision: MemoryManageReviewDecision,
        /// Canonical byte-preserving target path.
        target_path: String,
        /// Exact target analysis-artifact SHA-256.
        target_artifact_sha256: String,
        /// Exact target fact ordinal.
        target_fact_ordinal: u64,
        /// Remaining end-to-end deadline.
        timeout: Duration,
    },
    /// Observe bounded memory versions reachable from repository HEAD.
    ImportHistory {
        /// Remaining end-to-end deadline.
        timeout: Duration,
    },
}

impl MemoryManageServiceRequest {
    /// Returns the remaining end-to-end deadline duration.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        match self {
            Self::Write { timeout, .. }
            | Self::Approve { timeout, .. }
            | Self::Review { timeout, .. }
            | Self::ImportHistory { timeout } => *timeout,
        }
    }

    /// Returns the request-level durable operation for timeout reconciliation.
    #[must_use]
    pub const fn mutation_request_scope(&self) -> MemoryMutationRequestScope {
        match self {
            Self::Write { .. } => MemoryMutationRequestScope::Write,
            Self::Approve { .. } => MemoryMutationRequestScope::Approve,
            Self::Review { .. } => MemoryMutationRequestScope::Review,
            Self::ImportHistory { .. } => MemoryMutationRequestScope::ImportHistory,
        }
    }

    pub(crate) fn with_timeout(mut self, timeout: Duration) -> Self {
        match &mut self {
            Self::Write {
                timeout: current, ..
            }
            | Self::Approve {
                timeout: current, ..
            }
            | Self::Review {
                timeout: current, ..
            }
            | Self::ImportHistory { timeout: current } => *current = timeout,
        }
        self
    }
}

impl fmt::Debug for MemoryManageServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Write { timeout, .. } => formatter
                .debug_struct("MemoryManageServiceRequest::Write")
                .field("timeout", timeout)
                .finish_non_exhaustive(),
            Self::Approve { timeout, .. } => formatter
                .debug_struct("MemoryManageServiceRequest::Approve")
                .field("timeout", timeout)
                .finish_non_exhaustive(),
            Self::Review {
                evidence_ordinal,
                decision,
                target_fact_ordinal,
                timeout,
                ..
            } => formatter
                .debug_struct("MemoryManageServiceRequest::Review")
                .field("evidence_ordinal", evidence_ordinal)
                .field("decision", decision)
                .field("target_fact_ordinal", target_fact_ordinal)
                .field("timeout", timeout)
                .finish_non_exhaustive(),
            Self::ImportHistory { timeout } => formatter
                .debug_struct("MemoryManageServiceRequest::ImportHistory")
                .field("timeout", timeout)
                .finish(),
        }
    }
}

/// Version-2 redacted receipt from one authorized memory mutation.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryManageOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Exact operation-specific redacted receipt.
    pub receipt: MemoryManageReceipt,
}

/// Exact operation-specific result nested under [`MemoryManageOutput`].
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryManageReceipt {
    /// Canonical record-publication receipt.
    Write {
        /// Published canonical semantic revision SHA-256.
        revision_sha256: String,
        /// Whether the record was newly created.
        created: bool,
        /// Exact canonical YAML byte count.
        canonical_bytes: u64,
        /// Categorical post-publication facts observed after the atomic write.
        publication: MemoryManagePublicationStatus,
    },
    /// Local approval and observation receipt.
    Approve {
        /// Approved canonical semantic revision SHA-256.
        revision_sha256: String,
        /// Whether the immutable version was newly inserted.
        version_inserted: bool,
        /// Whether the worktree observation was newly appended.
        observation_inserted: bool,
        /// Whether the trusted approval was newly appended.
        approval_inserted: bool,
        /// Truthful post-commit SQLite maintenance status.
        maintenance: MemoryManageMaintenanceStatus,
    },
    /// Correspondence-review append receipt.
    Review {
        /// Whether a new semantic review event was appended.
        inserted: bool,
        /// Truthful post-commit SQLite maintenance status.
        maintenance: MemoryManageMaintenanceStatus,
    },
    /// Observation-only Git-history import report.
    ImportHistory {
        /// Admitted commits inspected.
        commits_inspected: u32,
        /// Record blobs inspected.
        records_inspected: u32,
        /// Immutable versions newly imported.
        imported_versions: u32,
        /// Observation events newly appended.
        appended_observations: u32,
        /// Exact admitted record-byte count.
        total_record_bytes: u64,
        /// Sanitized Git processes executed.
        git_processes: u32,
        /// Whether the bounded reachable-history coverage was complete.
        history_complete: bool,
        /// Truthful post-commit SQLite maintenance status.
        maintenance: MemoryManageMaintenanceStatus,
    },
}

/// Categorical state of one post-commit SQLite maintenance step.
#[derive(Clone, Copy, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryManageMaintenanceStepStatus {
    /// The step completed at its named final fence.
    Complete,
    /// The durable receipt is known, but the step was not confirmed.
    Deferred,
}

/// Database-path identity evidence observed after a committed SQLite mutation.
#[derive(Clone, Copy, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryManageDatabaseIdentityStatus {
    /// The canonical path named the exact unique writer-opened file.
    ConfirmedAtFinalFence,
    /// The path, file type, link policy, or identity changed after commit.
    ChangedAfterCommit,
    /// The final identity could not be determined safely.
    Unconfirmed,
}

/// Path-free finalization evidence for one committed SQLite memory mutation.
#[derive(Clone, Copy, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryManageMaintenanceStatus {
    /// Whether every maintenance step and the database identity fence completed.
    pub complete: bool,
    /// Exact number of deferred or unconfirmed finalization facts.
    pub warning_count: u8,
    /// Terminal WAL-checkpoint status.
    pub checkpoint: MemoryManageMaintenanceStepStatus,
    /// Writer-shutdown status.
    pub shutdown: MemoryManageMaintenanceStepStatus,
    /// Database-path evidence at the final fence.
    pub database_identity: MemoryManageDatabaseIdentityStatus,
}

impl MemoryManageMaintenanceStatus {
    /// Constructs internally consistent finalization evidence.
    #[must_use]
    pub const fn from_evidence(
        checkpoint: MemoryManageMaintenanceStepStatus,
        shutdown: MemoryManageMaintenanceStepStatus,
        database_identity: MemoryManageDatabaseIdentityStatus,
    ) -> Self {
        let checkpoint_warning = match checkpoint {
            MemoryManageMaintenanceStepStatus::Complete => 0,
            MemoryManageMaintenanceStepStatus::Deferred => 1,
        };
        let shutdown_warning = match shutdown {
            MemoryManageMaintenanceStepStatus::Complete => 0,
            MemoryManageMaintenanceStepStatus::Deferred => 1,
        };
        let identity_warning = match database_identity {
            MemoryManageDatabaseIdentityStatus::ConfirmedAtFinalFence => 0,
            MemoryManageDatabaseIdentityStatus::ChangedAfterCommit
            | MemoryManageDatabaseIdentityStatus::Unconfirmed => 1,
        };
        let warning_count = checkpoint_warning + shutdown_warning + identity_warning;
        Self {
            complete: warning_count == 0,
            warning_count,
            checkpoint,
            shutdown,
            database_identity,
        }
    }
}

/// Identity confirmation observed at one memory-file final fence.
#[derive(Clone, Copy, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryManageFileIdentityStatus {
    /// The path named the authorized target at the final fence.
    ConfirmedAtFinalFence,
    /// Publication committed but the path no longer had confirmed identity.
    ChangedAfterCommit,
}

/// Categorical state of one post-publication maintenance step.
#[derive(Clone, Copy, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryManagePublicationStepStatus {
    /// The step did not apply to this publication mode.
    NotRequired,
    /// The step completed at its final fence.
    Complete,
    /// Publication committed but the step was not confirmed.
    Deferred,
}

/// Path-free post-publication facts for one canonical memory write.
#[derive(Clone, Copy, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryManagePublicationStatus {
    /// Whether every applicable post-publication step was confirmed.
    pub complete: bool,
    /// Number of categorical warnings observed after publication.
    pub warning_count: u8,
    /// Private temporary-file cleanup state.
    pub temporary_cleanup: MemoryManagePublicationStepStatus,
    /// Canonical target identity at its final fence.
    pub target_identity: MemoryManageFileIdentityStatus,
    /// Records-directory identity at its final fence.
    pub records_directory_identity: MemoryManageFileIdentityStatus,
    /// Records-directory synchronization state.
    pub directory_sync: MemoryManagePublicationStepStatus,
}

impl MemoryManageOutput {
    /// Constructs a version-2 canonical record-publication receipt with its
    /// exact post-commit status.
    #[must_use]
    pub fn write_with_publication(
        revision_sha256: String,
        created: bool,
        canonical_bytes: u64,
        publication: MemoryManagePublicationStatus,
    ) -> Self {
        Self {
            schema_version: MEMORY_MANAGE_SCHEMA_VERSION,
            receipt: MemoryManageReceipt::Write {
                revision_sha256,
                created,
                canonical_bytes,
                publication,
            },
        }
    }

    /// Constructs a version-2 local approval receipt with truthful maintenance.
    #[must_use]
    pub fn approve_with_maintenance(
        revision_sha256: String,
        version_inserted: bool,
        observation_inserted: bool,
        approval_inserted: bool,
        maintenance: MemoryManageMaintenanceStatus,
    ) -> Self {
        Self {
            schema_version: MEMORY_MANAGE_SCHEMA_VERSION,
            receipt: MemoryManageReceipt::Approve {
                revision_sha256,
                version_inserted,
                observation_inserted,
                approval_inserted,
                maintenance,
            },
        }
    }

    /// Constructs a version-2 correspondence-review receipt with truthful maintenance.
    #[must_use]
    pub const fn review_with_maintenance(
        inserted: bool,
        maintenance: MemoryManageMaintenanceStatus,
    ) -> Self {
        Self {
            schema_version: MEMORY_MANAGE_SCHEMA_VERSION,
            receipt: MemoryManageReceipt::Review {
                inserted,
                maintenance,
            },
        }
    }

    /// Constructs a version-2 history-import report with truthful maintenance.
    #[allow(
        clippy::too_many_arguments,
        reason = "the bounded import receipt and maintenance state remain explicit"
    )]
    #[must_use]
    pub const fn import_history_with_maintenance(
        commits_inspected: u32,
        records_inspected: u32,
        imported_versions: u32,
        appended_observations: u32,
        total_record_bytes: u64,
        git_processes: u32,
        history_complete: bool,
        maintenance: MemoryManageMaintenanceStatus,
    ) -> Self {
        Self {
            schema_version: MEMORY_MANAGE_SCHEMA_VERSION,
            receipt: MemoryManageReceipt::ImportHistory {
                commits_inspected,
                records_inspected,
                imported_versions,
                appended_observations,
                total_record_bytes,
                git_processes,
                history_complete,
                maintenance,
            },
        }
    }
}

fn validated_record_id(record_id: Option<String>) -> Result<String, &'static str> {
    let record_id = record_id.ok_or("operation requires record_id")?;
    let decoded =
        MemoryRecordIdTextV1::decode(&record_id).map_err(|_| "record_id is not canonical")?;
    if MemoryRecordIdTextV1::encode(decoded).as_str() != record_id {
        return Err("record_id is not canonical");
    }
    Ok(record_id)
}

fn is_canonical_review_path_text(value: &str) -> bool {
    RepositoryPathTextV1::decode(
        value,
        RepositoryPathTextByteLimit::new(MAX_REVIEW_PATH_TEXT_BYTES),
        RepositoryPathLimits::new(MAX_REVIEW_PATH_BYTES, MAX_REVIEW_PATH_COMPONENTS),
    )
    .is_ok()
}

fn reject_non_write_fields(input: &MemoryManageInput) -> Result<(), &'static str> {
    if input.record_id.is_some()
        || input.revision_sha256.is_some()
        || input.evidence_ordinal.is_some()
        || input.review_decision.is_some()
        || input.target_path.is_some()
        || input.target_artifact_sha256.is_some()
        || input.target_fact_ordinal.is_some()
    {
        Err("write accepts only record_yaml and timeout_ms")
    } else {
        Ok(())
    }
}

fn reject_non_approve_fields(input: &MemoryManageInput) -> Result<(), &'static str> {
    if input.record_yaml.is_some()
        || input.revision_sha256.is_some()
        || input.evidence_ordinal.is_some()
        || input.review_decision.is_some()
        || input.target_path.is_some()
        || input.target_artifact_sha256.is_some()
        || input.target_fact_ordinal.is_some()
    {
        Err("approve accepts only record_id and timeout_ms")
    } else {
        Ok(())
    }
}

fn reject_non_review_fields(input: &MemoryManageInput) -> Result<(), &'static str> {
    if input.record_yaml.is_some() {
        Err("review does not accept record_yaml")
    } else {
        Ok(())
    }
}

fn reject_non_history_fields(input: &MemoryManageInput) -> Result<(), &'static str> {
    if input.record_yaml.is_some()
        || input.record_id.is_some()
        || input.revision_sha256.is_some()
        || input.evidence_ordinal.is_some()
        || input.review_decision.is_some()
        || input.target_path.is_some()
        || input.target_artifact_sha256.is_some()
        || input.target_fact_ordinal.is_some()
    {
        Err("import_history accepts only timeout_ms")
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "memory_manage_tests.rs"]
mod tests;
