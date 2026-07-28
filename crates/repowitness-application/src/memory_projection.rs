use std::{collections::BTreeSet, error::Error, fmt};

use repowitness_domain::{
    CanonicalMemoryDigest, MAX_MEMORY_EVIDENCE, MemoryLifecycle, MemoryProjectValidity,
    MemoryRecord,
};

/// Maximum immutable memory versions considered by one Phase 0 projection.
pub const MAX_MEMORY_PROJECTION_VERSIONS: usize = 4_096;

/// One immutable journal version and its trusted local-approval state.
#[derive(Clone, Copy)]
pub struct MemoryVersionHeadInput<'record> {
    revision: CanonicalMemoryDigest,
    record: &'record MemoryRecord,
    locally_approved: bool,
}

impl<'record> MemoryVersionHeadInput<'record> {
    /// Creates one exact head-selection input.
    #[must_use]
    pub const fn new(
        revision: CanonicalMemoryDigest,
        record: &'record MemoryRecord,
        locally_approved: bool,
    ) -> Self {
        Self {
            revision,
            record,
            locally_approved,
        }
    }

    /// Returns the exact canonical version identity.
    #[must_use]
    pub const fn revision(self) -> CanonicalMemoryDigest {
        self.revision
    }

    /// Returns the immutable validated record.
    #[must_use]
    pub const fn record(self) -> &'record MemoryRecord {
        self.record
    }

    /// Returns whether trusted local audit approved this exact version.
    #[must_use]
    pub const fn locally_approved(self) -> bool {
        self.locally_approved
    }
}

impl fmt::Debug for MemoryVersionHeadInput<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryVersionHeadInput")
            .field("revision", &self.revision)
            .field("record", &self.record)
            .field("locally_approved", &self.locally_approved)
            .finish()
    }
}

/// Categorical result of approved-version head selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryHeadState {
    /// The journal contains no trusted locally approved version.
    NoApprovedVersion,
    /// Exactly one complete approved head was selected.
    Selected,
    /// More than one complete approved head exists.
    Conflicted,
    /// Missing parents or an invalid head graph prevent selection.
    Indeterminate,
}

/// Complete bounded head-selection result for one memory record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryHeadSelection {
    state: MemoryHeadState,
    selected_revision: Option<CanonicalMemoryDigest>,
    approved_version_count: u32,
    head_count: u32,
    missing_parent_count: u32,
}

impl MemoryHeadSelection {
    /// Returns the categorical selection result.
    #[must_use]
    pub const fn state(self) -> MemoryHeadState {
        self.state
    }

    /// Returns the selected revision only when exactly one usable head exists.
    #[must_use]
    pub const fn selected_revision(self) -> Option<CanonicalMemoryDigest> {
        self.selected_revision
    }

    /// Returns the number of trusted approved versions considered.
    #[must_use]
    pub const fn approved_version_count(self) -> u32 {
        self.approved_version_count
    }

    /// Returns the number of approved heads discovered.
    #[must_use]
    pub const fn head_count(self) -> u32 {
        self.head_count
    }

    /// Returns the number of distinct unavailable parent identities.
    #[must_use]
    pub const fn missing_parent_count(self) -> u32 {
        self.missing_parent_count
    }
}

/// Stable categorical source-evidence result consumed by effective-state policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryEvidenceOutcome {
    /// The exact cited occurrence remains present.
    Exact,
    /// A reviewed or high-assurance rule established correspondence.
    Corresponded,
    /// The exact descriptor remains but declaration semantics changed.
    Changed,
    /// Complete bounded candidates require trusted review.
    NeedsReview,
    /// Complete coverage found no plausible occurrence.
    Missing,
    /// Incomplete evidence or coverage prevents a conclusion.
    Indeterminate,
}

/// Effective state exposed by one immutable memory projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryEffectiveState {
    /// Project validity and all evidence support current use.
    Current,
    /// Project validity excludes the memory at this target.
    NotApplicable,
    /// Evidence no longer supports the authored claim.
    Stale,
    /// Trusted manual review is required.
    NeedsReview,
    /// Complete evaluation was unavailable.
    Indeterminate,
    /// Multiple approved heads exist.
    Conflicted,
    /// The authored version is contradicted.
    Contradicted,
    /// The authored version is superseded.
    Superseded,
    /// The authored version is quarantined.
    Quarantined,
    /// The authored version is an immutable deletion marker.
    Tombstoned,
}

/// Project-validity state persisted by a projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryProjectionValidityState {
    /// The selected version applies to the concrete target.
    Valid,
    /// The selected version does not apply to the concrete target.
    Invalid,
    /// Required Git or snapshot evidence was unavailable.
    Indeterminate,
    /// Authored lifecycle policy prevented validity evaluation.
    NotEvaluated,
}

/// Aggregate evidence state persisted by a projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryProjectionEvidenceState {
    /// Every cited occurrence remained exact.
    Exact,
    /// Every citation resolved and at least one used correspondence.
    Corresponded,
    /// At least one exact descriptor changed.
    Changed,
    /// At least one citation requires trusted review.
    Ambiguous,
    /// At least one citation is absent under complete coverage.
    Missing,
    /// Required source or correspondence evidence was unavailable.
    Indeterminate,
    /// Evidence was intentionally not evaluated.
    NotEvaluated,
}

/// Stable reason for one effective-state decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryProjectionReason {
    /// Every citation remained exact.
    EvidenceExact,
    /// At least one citation resolved through correspondence.
    EvidenceCorresponded,
    /// At least one exact descriptor changed.
    EvidenceChanged,
    /// At least one citation requires review.
    EvidenceAmbiguous,
    /// At least one citation is absent.
    EvidenceMissing,
    /// Evidence coverage was incomplete.
    EvidenceIndeterminate,
    /// Git-DAG or snapshot validity excludes the version.
    ProjectNotApplicable,
    /// Project validity could not be established.
    ProjectIndeterminate,
    /// The authored lifecycle requires review.
    AuthoredNeedsReview,
    /// The authored lifecycle is stale.
    AuthoredStale,
    /// The authored lifecycle is contradicted.
    AuthoredContradicted,
    /// The authored lifecycle is superseded.
    AuthoredSuperseded,
    /// The authored lifecycle is quarantined.
    AuthoredQuarantined,
    /// The authored lifecycle is tombstoned.
    AuthoredTombstoned,
}

/// Effective-state result for exactly one selected approved version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryProjectionDecision {
    effective_state: MemoryEffectiveState,
    validity_state: MemoryProjectionValidityState,
    evidence_state: MemoryProjectionEvidenceState,
    reason: MemoryProjectionReason,
    evidence_count: u32,
    resolved_count: u32,
    review_count: u32,
    indeterminate_count: u32,
}

impl MemoryProjectionDecision {
    /// Returns the effective state.
    #[must_use]
    pub const fn effective_state(self) -> MemoryEffectiveState {
        self.effective_state
    }

    /// Returns the project-validity state.
    #[must_use]
    pub const fn validity_state(self) -> MemoryProjectionValidityState {
        self.validity_state
    }

    /// Returns the aggregate evidence state.
    #[must_use]
    pub const fn evidence_state(self) -> MemoryProjectionEvidenceState {
        self.evidence_state
    }

    /// Returns the stable categorical reason.
    #[must_use]
    pub const fn reason(self) -> MemoryProjectionReason {
        self.reason
    }

    /// Returns the selected version's evidence count.
    #[must_use]
    pub const fn evidence_count(self) -> u32 {
        self.evidence_count
    }

    /// Returns the number of exact or corresponded evidence results.
    #[must_use]
    pub const fn resolved_count(self) -> u32 {
        self.resolved_count
    }

    /// Returns the number of evidence results requiring review.
    #[must_use]
    pub const fn review_count(self) -> u32 {
        self.review_count
    }

    /// Returns the number of indeterminate evidence results.
    #[must_use]
    pub const fn indeterminate_count(self) -> u32 {
        self.indeterminate_count
    }
}

/// Stable, content-redacted projection policy failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryProjectionError {
    /// The per-rebuild version bound was exceeded.
    TooManyVersions,
    /// Inputs mixed record IDs or repeated a revision identity.
    InvalidVersionSet,
    /// A count cannot be represented by the projection contract.
    CountNotRepresentable,
    /// Validity or evidence inputs do not match the selected lifecycle.
    InvalidEvaluation,
}

impl fmt::Display for MemoryProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TooManyVersions => "memory projection version limit exceeded",
            Self::InvalidVersionSet => "memory projection version set is invalid",
            Self::CountNotRepresentable => "memory projection count is not representable",
            Self::InvalidEvaluation => "memory projection evaluation input is invalid",
        })
    }
}

impl Error for MemoryProjectionError {}

/// Selects the one complete approved head for a single record without using
/// display revisions, timestamps, row IDs, or lexical digest ordering.
pub fn select_memory_head(
    versions: &[MemoryVersionHeadInput<'_>],
) -> Result<MemoryHeadSelection, MemoryProjectionError> {
    if versions.len() > MAX_MEMORY_PROJECTION_VERSIONS {
        return Err(MemoryProjectionError::TooManyVersions);
    }
    let Some(first) = versions.first() else {
        return head_selection(MemoryHeadState::NoApprovedVersion, None, 0, 0, 0);
    };
    let record_id = first.record.header().record_id();
    let mut all_revisions = BTreeSet::new();
    for version in versions {
        if version.record.header().record_id() != record_id
            || !all_revisions.insert(version.revision)
        {
            return Err(MemoryProjectionError::InvalidVersionSet);
        }
    }

    let approved = versions
        .iter()
        .filter(|version| version.locally_approved)
        .collect::<Vec<_>>();
    if approved.is_empty() {
        return head_selection(MemoryHeadState::NoApprovedVersion, None, 0, 0, 0);
    }

    let approved_revisions = approved
        .iter()
        .map(|version| version.revision)
        .collect::<BTreeSet<_>>();
    let mut referenced_approved = BTreeSet::new();
    let mut missing_parents = BTreeSet::new();
    for version in &approved {
        for parent in version.record.header().parents() {
            if approved_revisions.contains(parent) {
                referenced_approved.insert(*parent);
            }
            if !all_revisions.contains(parent) {
                missing_parents.insert(*parent);
            }
        }
    }
    let heads = approved_revisions
        .difference(&referenced_approved)
        .copied()
        .collect::<Vec<_>>();
    let approved_count = approved.len();
    let head_count = heads.len();
    let missing_count = missing_parents.len();
    if missing_count > 0 || heads.is_empty() {
        return head_selection(
            MemoryHeadState::Indeterminate,
            (heads.len() == 1).then_some(heads[0]),
            approved_count,
            head_count,
            missing_count,
        );
    }
    if heads.len() > 1 {
        return head_selection(
            MemoryHeadState::Conflicted,
            None,
            approved_count,
            head_count,
            0,
        );
    }
    head_selection(
        MemoryHeadState::Selected,
        Some(heads[0]),
        approved_count,
        1,
        0,
    )
}

/// Applies authored lifecycle, project validity, and evidence policy to one
/// selected approved memory version.
pub fn evaluate_memory_projection(
    record: &MemoryRecord,
    project_validity: Option<MemoryProjectValidity>,
    evidence: &[MemoryEvidenceOutcome],
) -> Result<MemoryProjectionDecision, MemoryProjectionError> {
    if record.evidence().len() > MAX_MEMORY_EVIDENCE {
        return Err(MemoryProjectionError::InvalidEvaluation);
    }
    if record.lifecycle() != MemoryLifecycle::Active {
        if project_validity.is_some() || !evidence.is_empty() {
            return Err(MemoryProjectionError::InvalidEvaluation);
        }
        return authored_lifecycle_decision(record.lifecycle(), record.evidence().len());
    }

    match project_validity {
        Some(MemoryProjectValidity::NotApplicable) if evidence.is_empty() => Ok(decision(
            MemoryEffectiveState::NotApplicable,
            MemoryProjectionValidityState::Invalid,
            MemoryProjectionEvidenceState::NotEvaluated,
            MemoryProjectionReason::ProjectNotApplicable,
            record.evidence().len(),
            0,
            0,
            0,
        )?),
        Some(MemoryProjectValidity::Indeterminate) if evidence.is_empty() => Ok(decision(
            MemoryEffectiveState::Indeterminate,
            MemoryProjectionValidityState::Indeterminate,
            MemoryProjectionEvidenceState::NotEvaluated,
            MemoryProjectionReason::ProjectIndeterminate,
            record.evidence().len(),
            0,
            0,
            0,
        )?),
        Some(MemoryProjectValidity::Valid) if evidence.len() == record.evidence().len() => {
            evaluate_valid_evidence(evidence)
        }
        _ => Err(MemoryProjectionError::InvalidEvaluation),
    }
}

fn evaluate_valid_evidence(
    evidence: &[MemoryEvidenceOutcome],
) -> Result<MemoryProjectionDecision, MemoryProjectionError> {
    let evidence_count = evidence.len();
    let resolved = evidence
        .iter()
        .filter(|outcome| {
            matches!(
                outcome,
                MemoryEvidenceOutcome::Exact | MemoryEvidenceOutcome::Corresponded
            )
        })
        .count();
    let review = evidence
        .iter()
        .filter(|outcome| **outcome == MemoryEvidenceOutcome::NeedsReview)
        .count();
    let indeterminate = evidence
        .iter()
        .filter(|outcome| **outcome == MemoryEvidenceOutcome::Indeterminate)
        .count();

    let (effective, aggregate, reason) = if evidence.contains(&MemoryEvidenceOutcome::Indeterminate)
    {
        (
            MemoryEffectiveState::Indeterminate,
            MemoryProjectionEvidenceState::Indeterminate,
            MemoryProjectionReason::EvidenceIndeterminate,
        )
    } else if evidence.contains(&MemoryEvidenceOutcome::NeedsReview) {
        (
            MemoryEffectiveState::NeedsReview,
            MemoryProjectionEvidenceState::Ambiguous,
            MemoryProjectionReason::EvidenceAmbiguous,
        )
    } else if evidence.contains(&MemoryEvidenceOutcome::Changed) {
        (
            MemoryEffectiveState::Stale,
            MemoryProjectionEvidenceState::Changed,
            MemoryProjectionReason::EvidenceChanged,
        )
    } else if evidence.contains(&MemoryEvidenceOutcome::Missing) {
        (
            MemoryEffectiveState::Stale,
            MemoryProjectionEvidenceState::Missing,
            MemoryProjectionReason::EvidenceMissing,
        )
    } else if evidence.contains(&MemoryEvidenceOutcome::Corresponded) {
        (
            MemoryEffectiveState::Current,
            MemoryProjectionEvidenceState::Corresponded,
            MemoryProjectionReason::EvidenceCorresponded,
        )
    } else {
        (
            MemoryEffectiveState::Current,
            MemoryProjectionEvidenceState::Exact,
            MemoryProjectionReason::EvidenceExact,
        )
    };
    decision(
        effective,
        MemoryProjectionValidityState::Valid,
        aggregate,
        reason,
        evidence_count,
        resolved,
        review,
        indeterminate,
    )
}

fn authored_lifecycle_decision(
    lifecycle: MemoryLifecycle,
    evidence_count: usize,
) -> Result<MemoryProjectionDecision, MemoryProjectionError> {
    let (effective_state, reason) = match lifecycle {
        MemoryLifecycle::NeedsReview => (
            MemoryEffectiveState::NeedsReview,
            MemoryProjectionReason::AuthoredNeedsReview,
        ),
        MemoryLifecycle::Stale => (
            MemoryEffectiveState::Stale,
            MemoryProjectionReason::AuthoredStale,
        ),
        MemoryLifecycle::Contradicted => (
            MemoryEffectiveState::Contradicted,
            MemoryProjectionReason::AuthoredContradicted,
        ),
        MemoryLifecycle::Superseded => (
            MemoryEffectiveState::Superseded,
            MemoryProjectionReason::AuthoredSuperseded,
        ),
        MemoryLifecycle::Quarantined => (
            MemoryEffectiveState::Quarantined,
            MemoryProjectionReason::AuthoredQuarantined,
        ),
        MemoryLifecycle::Tombstoned => (
            MemoryEffectiveState::Tombstoned,
            MemoryProjectionReason::AuthoredTombstoned,
        ),
        MemoryLifecycle::Active => unreachable!("active lifecycle is evaluated separately"),
    };
    Ok(MemoryProjectionDecision {
        effective_state,
        validity_state: MemoryProjectionValidityState::NotEvaluated,
        evidence_state: MemoryProjectionEvidenceState::NotEvaluated,
        reason,
        evidence_count: count(evidence_count)?,
        resolved_count: 0,
        review_count: 0,
        indeterminate_count: 0,
    })
}

fn head_selection(
    state: MemoryHeadState,
    selected_revision: Option<CanonicalMemoryDigest>,
    approved_version_count: usize,
    head_count: usize,
    missing_parent_count: usize,
) -> Result<MemoryHeadSelection, MemoryProjectionError> {
    Ok(MemoryHeadSelection {
        state,
        selected_revision,
        approved_version_count: count(approved_version_count)?,
        head_count: count(head_count)?,
        missing_parent_count: count(missing_parent_count)?,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "the persisted aggregate counts are explicit projection semantics"
)]
fn decision(
    effective_state: MemoryEffectiveState,
    validity_state: MemoryProjectionValidityState,
    evidence_state: MemoryProjectionEvidenceState,
    reason: MemoryProjectionReason,
    evidence_count: usize,
    resolved_count: usize,
    review_count: usize,
    indeterminate_count: usize,
) -> Result<MemoryProjectionDecision, MemoryProjectionError> {
    Ok(MemoryProjectionDecision {
        effective_state,
        validity_state,
        evidence_state,
        reason,
        evidence_count: count(evidence_count)?,
        resolved_count: count(resolved_count)?,
        review_count: count(review_count)?,
        indeterminate_count: count(indeterminate_count)?,
    })
}

fn count(value: usize) -> Result<u32, MemoryProjectionError> {
    u32::try_from(value).map_err(|_| MemoryProjectionError::CountNotRepresentable)
}

#[cfg(test)]
mod tests;
