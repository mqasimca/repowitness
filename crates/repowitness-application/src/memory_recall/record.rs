use std::fmt;

use repowitness_domain::{CanonicalMemoryDigest, MemoryRecord, MemoryRecordId, RepositoryPath};

use crate::{MemoryEffectiveState, MemoryProjectionValidityState};

use super::{
    evidence::{
        MemoryRecallEvidence, MemoryRecallEvidenceOutcome, MemoryRecallEvidenceState,
        MemoryRecallReason,
    },
    port::MemoryRecallPortOutputError,
};

const MAX_EVIDENCE_PER_RECORD: usize = 16;
const FIXED_RECORD_OUTPUT_BYTES: u64 = 512;
const FIXED_EVIDENCE_OUTPUT_BYTES: u64 = 384;
const FIXED_CANDIDATE_OUTPUT_BYTES: u64 = 320;

/// One projected memory record and its optional selected immutable version.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryRecallRecord {
    pub(super) record_id: MemoryRecordId,
    revision: Option<CanonicalMemoryDigest>,
    pub(super) record: Option<MemoryRecord>,
    effective_state: MemoryEffectiveState,
    validity_state: MemoryProjectionValidityState,
    evidence_state: MemoryRecallEvidenceState,
    reason: MemoryRecallReason,
    evidence_count: u32,
    resolved_count: u32,
    review_count: u32,
    indeterminate_count: u32,
    head_count: u32,
    missing_parent_count: u32,
    evidence: Box<[MemoryRecallEvidence]>,
}

impl MemoryRecallRecord {
    /// Validates one complete projected row before application use.
    #[allow(
        clippy::too_many_arguments,
        reason = "the immutable projection row contract keeps all categorical counts explicit"
    )]
    pub fn try_new(
        record_id: MemoryRecordId,
        revision: Option<CanonicalMemoryDigest>,
        record: Option<MemoryRecord>,
        effective_state: MemoryEffectiveState,
        validity_state: MemoryProjectionValidityState,
        evidence_state: MemoryRecallEvidenceState,
        reason: MemoryRecallReason,
        evidence_count: u32,
        resolved_count: u32,
        review_count: u32,
        indeterminate_count: u32,
        head_count: u32,
        missing_parent_count: u32,
        evidence: Vec<MemoryRecallEvidence>,
    ) -> Result<Self, MemoryRecallPortOutputError> {
        let selected_consistent = match (revision, record.as_ref()) {
            (Some(_), Some(record)) => record.header().record_id() == record_id,
            (None, None) => matches!(
                effective_state,
                MemoryEffectiveState::Conflicted | MemoryEffectiveState::Indeterminate
            ),
            _ => false,
        };
        let count_consistent = resolved_count
            .checked_add(review_count)
            .and_then(|count| count.checked_add(indeterminate_count))
            .is_some_and(|count| count <= evidence_count)
            && evidence_count <= u32::try_from(MAX_EVIDENCE_PER_RECORD).unwrap_or(u32::MAX)
            && evidence.len() <= MAX_EVIDENCE_PER_RECORD
            && (evidence.is_empty()
                || usize::try_from(evidence_count).is_ok_and(|count| count == evidence.len()));
        let projected_counts = evidence.iter().try_fold(
            (0_u32, 0_u32, 0_u32),
            |(resolved, review, indeterminate), result| match result.outcome() {
                MemoryRecallEvidenceOutcome::Exact
                | MemoryRecallEvidenceOutcome::SamePathRename
                | MemoryRecallEvidenceOutcome::GitExactMove
                | MemoryRecallEvidenceOutcome::ReviewedLink => {
                    Some((resolved.checked_add(1)?, review, indeterminate))
                }
                MemoryRecallEvidenceOutcome::Ambiguous => {
                    Some((resolved, review.checked_add(1)?, indeterminate))
                }
                MemoryRecallEvidenceOutcome::Indeterminate => {
                    Some((resolved, review, indeterminate.checked_add(1)?))
                }
                MemoryRecallEvidenceOutcome::Changed | MemoryRecallEvidenceOutcome::Missing => {
                    Some((resolved, review, indeterminate))
                }
            },
        );
        let projected_counts_consistent = evidence.is_empty()
            || projected_counts == Some((resolved_count, review_count, indeterminate_count));
        let head_state_consistent = match reason {
            MemoryRecallReason::ApprovedHeadConflict => {
                effective_state == MemoryEffectiveState::Conflicted
                    && revision.is_none()
                    && evidence.is_empty()
            }
            MemoryRecallReason::MissingParent | MemoryRecallReason::InvalidHeadGraph => {
                effective_state == MemoryEffectiveState::Indeterminate
                    && validity_state == MemoryProjectionValidityState::NotEvaluated
                    && evidence_state == MemoryRecallEvidenceState::NotEvaluated
                    && evidence.is_empty()
            }
            _ => revision.is_some() && effective_state != MemoryEffectiveState::Conflicted,
        };
        if !selected_consistent
            || !count_consistent
            || !projected_counts_consistent
            || !head_state_consistent
            || head_count == 0
            || (effective_state == MemoryEffectiveState::Conflicted
                && (head_count < 2
                    || revision.is_some()
                    || evidence_state != MemoryRecallEvidenceState::Conflicted
                    || validity_state != MemoryProjectionValidityState::NotEvaluated
                    || reason != MemoryRecallReason::ApprovedHeadConflict))
        {
            return Err(MemoryRecallPortOutputError::InvalidRecord);
        }
        Ok(Self {
            record_id,
            revision,
            record,
            effective_state,
            validity_state,
            evidence_state,
            reason,
            evidence_count,
            resolved_count,
            review_count,
            indeterminate_count,
            head_count,
            missing_parent_count,
            evidence: evidence.into_boxed_slice(),
        })
    }

    /// Returns the logical record identity.
    #[must_use]
    pub const fn record_id(&self) -> MemoryRecordId {
        self.record_id
    }

    /// Returns the exact selected immutable revision, when head selection succeeded.
    #[must_use]
    pub const fn revision(&self) -> Option<CanonicalMemoryDigest> {
        self.revision
    }

    /// Returns the selected integrity-checked semantic record.
    #[must_use]
    pub const fn record(&self) -> Option<&MemoryRecord> {
        self.record.as_ref()
    }

    /// Returns the effective freshness and eligibility state.
    #[must_use]
    pub const fn effective_state(&self) -> MemoryEffectiveState {
        self.effective_state
    }

    /// Returns the project-validity state.
    #[must_use]
    pub const fn validity_state(&self) -> MemoryProjectionValidityState {
        self.validity_state
    }

    /// Returns aggregate evidence state.
    #[must_use]
    pub const fn evidence_state(&self) -> MemoryRecallEvidenceState {
        self.evidence_state
    }

    /// Returns the stable effective-state reason.
    #[must_use]
    pub const fn reason(&self) -> MemoryRecallReason {
        self.reason
    }

    /// Returns the selected version's citation count.
    #[must_use]
    pub const fn evidence_count(&self) -> u32 {
        self.evidence_count
    }

    /// Returns the exact or corresponded citation count.
    #[must_use]
    pub const fn resolved_count(&self) -> u32 {
        self.resolved_count
    }

    /// Returns the review-required citation count.
    #[must_use]
    pub const fn review_count(&self) -> u32 {
        self.review_count
    }

    /// Returns the indeterminate citation count.
    #[must_use]
    pub const fn indeterminate_count(&self) -> u32 {
        self.indeterminate_count
    }

    /// Returns the approved head count considered.
    #[must_use]
    pub const fn head_count(&self) -> u32 {
        self.head_count
    }

    /// Returns the unavailable parent count.
    #[must_use]
    pub const fn missing_parent_count(&self) -> u32 {
        self.missing_parent_count
    }

    /// Returns projected citation outcomes in authored evidence order.
    #[must_use]
    pub const fn evidence(&self) -> &[MemoryRecallEvidence] {
        &self.evidence
    }

    /// Returns a conservative upper bound for JSON/line encoding this record.
    pub fn encoded_output_bytes(&self) -> Result<u64, MemoryRecallPortOutputError> {
        let mut total = FIXED_RECORD_OUTPUT_BYTES;
        if let Some(record) = &self.record {
            total = add_escaped_text(total, record.claim().title().as_str())?;
            total = add_escaped_text(total, record.claim().body().as_str())?;
        }
        for result in &self.evidence {
            total = total
                .checked_add(FIXED_EVIDENCE_OUTPUT_BYTES)
                .ok_or(MemoryRecallPortOutputError::CountNotRepresentable)?;
            if let Some(target) = result.target() {
                total = add_path_output(total, target.path())?;
            }
            for candidate in result.candidates() {
                total = total
                    .checked_add(FIXED_CANDIDATE_OUTPUT_BYTES)
                    .ok_or(MemoryRecallPortOutputError::CountNotRepresentable)?;
                total = add_path_output(total, candidate.occurrence().path())?;
            }
        }
        Ok(total)
    }
}

impl fmt::Debug for MemoryRecallRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryRecallRecord")
            .field("record_id", &self.record_id)
            .field("revision", &self.revision)
            .field("has_selected_record", &self.record.is_some())
            .field("effective_state", &self.effective_state)
            .field("validity_state", &self.validity_state)
            .field("evidence_state", &self.evidence_state)
            .field("reason", &self.reason)
            .field("evidence_count", &self.evidence_count)
            .field("returned_evidence", &self.evidence.len())
            .finish()
    }
}

/// Exact coverage and state counts of one complete projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryRecallProjectionCoverage {
    searched: u64,
    skipped: u64,
    unresolved: u64,
    truncated: u64,
    total: u64,
    current: u64,
    not_applicable: u64,
    stale: u64,
    needs_review: u64,
    indeterminate: u64,
    conflicted: u64,
    contradicted: u64,
    superseded: u64,
    quarantined: u64,
    tombstoned: u64,
}

impl MemoryRecallProjectionCoverage {
    /// Constructs exact persisted projection coverage for application validation.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "every effective state has an independent persisted count"
    )]
    pub const fn new(
        searched: u64,
        skipped: u64,
        unresolved: u64,
        truncated: u64,
        total: u64,
        current: u64,
        not_applicable: u64,
        stale: u64,
        needs_review: u64,
        indeterminate: u64,
        conflicted: u64,
        contradicted: u64,
        superseded: u64,
        quarantined: u64,
        tombstoned: u64,
    ) -> Self {
        Self {
            searched,
            skipped,
            unresolved,
            truncated,
            total,
            current,
            not_applicable,
            stale,
            needs_review,
            indeterminate,
            conflicted,
            contradicted,
            superseded,
            quarantined,
            tombstoned,
        }
    }

    /// Returns records evaluated into projection rows.
    #[must_use]
    pub const fn searched(self) -> u64 {
        self.searched
    }

    /// Returns journal records omitted by policy.
    #[must_use]
    pub const fn skipped(self) -> u64 {
        self.skipped
    }

    /// Returns projected records requiring review or more evidence.
    #[must_use]
    pub const fn unresolved(self) -> u64 {
        self.unresolved
    }

    /// Returns journal records omitted because a projection bound was reached.
    #[must_use]
    pub const fn truncated(self) -> u64 {
        self.truncated
    }

    /// Returns complete projected rows.
    #[must_use]
    pub const fn total(self) -> u64 {
        self.total
    }

    /// Returns the count for one effective state.
    #[must_use]
    pub const fn state_count(self, state: MemoryEffectiveState) -> u64 {
        match state {
            MemoryEffectiveState::Current => self.current,
            MemoryEffectiveState::NotApplicable => self.not_applicable,
            MemoryEffectiveState::Stale => self.stale,
            MemoryEffectiveState::NeedsReview => self.needs_review,
            MemoryEffectiveState::Indeterminate => self.indeterminate,
            MemoryEffectiveState::Conflicted => self.conflicted,
            MemoryEffectiveState::Contradicted => self.contradicted,
            MemoryEffectiveState::Superseded => self.superseded,
            MemoryEffectiveState::Quarantined => self.quarantined,
            MemoryEffectiveState::Tombstoned => self.tombstoned,
        }
    }

    pub(super) fn valid(self) -> bool {
        let state_sum = [
            self.current,
            self.not_applicable,
            self.stale,
            self.needs_review,
            self.indeterminate,
            self.conflicted,
            self.contradicted,
            self.superseded,
            self.quarantined,
            self.tombstoned,
        ]
        .into_iter()
        .try_fold(0_u64, u64::checked_add);
        let unresolved_sum = self
            .needs_review
            .checked_add(self.indeterminate)
            .and_then(|count| count.checked_add(self.conflicted));
        self.searched == self.total
            && state_sum == Some(self.total)
            && unresolved_sum == Some(self.unresolved)
    }
}

fn add_escaped_text(total: u64, value: &str) -> Result<u64, MemoryRecallPortOutputError> {
    let bytes = u64::try_from(value.len())
        .map_err(|_| MemoryRecallPortOutputError::CountNotRepresentable)?;
    total
        .checked_add(
            bytes
                .checked_mul(6)
                .ok_or(MemoryRecallPortOutputError::CountNotRepresentable)?,
        )
        .ok_or(MemoryRecallPortOutputError::CountNotRepresentable)
}

fn add_path_output(total: u64, path: &RepositoryPath) -> Result<u64, MemoryRecallPortOutputError> {
    let bytes = u64::try_from(path.as_bytes().len())
        .map_err(|_| MemoryRecallPortOutputError::CountNotRepresentable)?;
    total
        .checked_add(
            bytes
                .checked_mul(2)
                .and_then(|value| value.checked_add(16))
                .ok_or(MemoryRecallPortOutputError::CountNotRepresentable)?,
        )
        .ok_or(MemoryRecallPortOutputError::CountNotRepresentable)
}
