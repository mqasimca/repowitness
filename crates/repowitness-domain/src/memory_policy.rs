//! Deterministic lifecycle and retention rules shared by memory adapters.

use crate::{MemoryKind, MemoryLifecycle};

/// Version of the deterministic Phase 3 lifecycle policy profile.
pub const MEMORY_LIFECYCLE_POLICY_VERSION: u32 = 1;

/// Auditable reason for one lifecycle transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryLifecycleReason {
    /// A trusted reviewer accepted the authored revision.
    ReviewApproved,
    /// Current source or applicability evidence no longer supports the revision.
    SourceInvalidated,
    /// Attributed evidence conflicts with the revision.
    Contradicted,
    /// An explicit successor replaced the revision.
    Superseded,
    /// Validation or policy isolated the revision from normal use.
    TrustConcern,
    /// A retention schedule reached its disposition point.
    RetentionDue,
}

/// Stable failure for an invalid lifecycle transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryLifecycleTransitionError {
    /// The requested reason cannot transition the current state.
    InvalidTransition,
    /// Supersession requires an explicit successor relationship outside this pure function.
    SuccessorRequired,
}

/// Retention disposition evaluated from persisted timestamps and policy input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRetentionDisposition {
    /// Keep the revision eligible for its normal lifecycle rules.
    Retain,
    /// Keep the revision but require review before current use.
    Review,
    /// Keep an immutable observation outside current-use projections.
    Archive,
    /// Append an explicit tombstone revision; never delete implicitly.
    Tombstone,
}

/// Bounded retention policy. A missing age means that disposition is disabled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryLifecyclePolicy {
    review_after_ms: Option<u64>,
    archive_after_ms: Option<u64>,
    tombstone_after_ms: Option<u64>,
}

impl MemoryLifecyclePolicy {
    /// Creates a policy, rejecting contradictory ordering.
    pub fn try_new(
        review_after_ms: Option<u64>,
        archive_after_ms: Option<u64>,
        tombstone_after_ms: Option<u64>,
    ) -> Option<Self> {
        if let Some(review) = review_after_ms
            && (archive_after_ms.is_some_and(|archive| archive < review)
                || tombstone_after_ms.is_some_and(|tombstone| tombstone < review))
        {
            return None;
        }
        if let Some(archive) = archive_after_ms
            && tombstone_after_ms.is_some_and(|tombstone| tombstone < archive)
        {
            return None;
        }
        Some(Self {
            review_after_ms,
            archive_after_ms,
            tombstone_after_ms,
        })
    }

    /// Returns the configured review threshold.
    #[must_use]
    pub const fn review_after_ms(self) -> Option<u64> {
        self.review_after_ms
    }

    /// Returns the configured archive threshold.
    #[must_use]
    pub const fn archive_after_ms(self) -> Option<u64> {
        self.archive_after_ms
    }

    /// Returns the configured tombstone threshold.
    #[must_use]
    pub const fn tombstone_after_ms(self) -> Option<u64> {
        self.tombstone_after_ms
    }
}

/// Determines the retention action without reading a clock or deleting data.
#[must_use]
pub fn evaluate_retention(
    policy: MemoryLifecyclePolicy,
    current: MemoryLifecycle,
    age_ms: u64,
) -> MemoryRetentionDisposition {
    if current == MemoryLifecycle::Tombstoned {
        return MemoryRetentionDisposition::Tombstone;
    }
    if policy
        .tombstone_after_ms
        .is_some_and(|threshold| age_ms >= threshold)
    {
        return MemoryRetentionDisposition::Tombstone;
    }
    if policy
        .archive_after_ms
        .is_some_and(|threshold| age_ms >= threshold)
    {
        return MemoryRetentionDisposition::Archive;
    }
    if policy
        .review_after_ms
        .is_some_and(|threshold| age_ms >= threshold)
    {
        return MemoryRetentionDisposition::Review;
    }
    MemoryRetentionDisposition::Retain
}

/// Returns the deterministic next state for one audited policy event.
///
/// `successor_present` must be true when supersession is requested; callers
/// are responsible for proving that the successor is the exact attributed
/// relationship before invoking this pure state transition.
pub fn transition_memory_lifecycle(
    current: MemoryLifecycle,
    reason: MemoryLifecycleReason,
    successor_present: bool,
) -> Result<MemoryLifecycle, MemoryLifecycleTransitionError> {
    use MemoryLifecycle::{
        Active, Contradicted, NeedsReview, Quarantined, Stale, Superseded, Tombstoned,
    };
    match (current, reason) {
        (NeedsReview, MemoryLifecycleReason::ReviewApproved) => Ok(Active),
        (Active, MemoryLifecycleReason::SourceInvalidated) => Ok(Stale),
        (Active, MemoryLifecycleReason::Contradicted) => Ok(Contradicted),
        (Active, MemoryLifecycleReason::Superseded) if successor_present => Ok(Superseded),
        (Active | NeedsReview, MemoryLifecycleReason::TrustConcern) => Ok(Quarantined),
        (
            Active | NeedsReview | Stale | Contradicted | Superseded | Quarantined,
            MemoryLifecycleReason::RetentionDue,
        ) => Ok(Tombstoned),
        (Tombstoned, _) => Err(MemoryLifecycleTransitionError::InvalidTransition),
        (_, MemoryLifecycleReason::Superseded) if !successor_present => {
            Err(MemoryLifecycleTransitionError::SuccessorRequired)
        }
        (_, MemoryLifecycleReason::Superseded) => {
            Err(MemoryLifecycleTransitionError::InvalidTransition)
        }
        _ => Err(MemoryLifecycleTransitionError::InvalidTransition),
    }
}

/// Retrieval eligibility separate from authored lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryUseEligibility {
    /// Safe for the current-use provider under the supplied evidence state.
    Current,
    /// Exposed only to explicit review/history consumers.
    ReviewOnly,
    /// Excluded from memory guidance and review results by default.
    Excluded,
}

/// Evaluates current-use eligibility without inventing confidence.
#[must_use]
pub fn evaluate_memory_use(
    kind: MemoryKind,
    lifecycle: MemoryLifecycle,
    independently_verified: bool,
) -> MemoryUseEligibility {
    match lifecycle {
        MemoryLifecycle::Active if kind == MemoryKind::Procedure && independently_verified => {
            MemoryUseEligibility::Current
        }
        MemoryLifecycle::Active if kind != MemoryKind::Procedure => MemoryUseEligibility::Current,
        MemoryLifecycle::NeedsReview
        | MemoryLifecycle::Stale
        | MemoryLifecycle::Contradicted
        | MemoryLifecycle::Superseded => MemoryUseEligibility::ReviewOnly,
        MemoryLifecycle::Active => MemoryUseEligibility::ReviewOnly,
        MemoryLifecycle::Quarantined | MemoryLifecycle::Tombstoned => {
            MemoryUseEligibility::Excluded
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_order_is_explicit_and_non_destructive() {
        let policy =
            MemoryLifecyclePolicy::try_new(Some(10), Some(20), Some(30)).expect("ordered policy");
        assert_eq!(
            evaluate_retention(policy, MemoryLifecycle::Active, 9),
            MemoryRetentionDisposition::Retain
        );
        assert_eq!(
            evaluate_retention(policy, MemoryLifecycle::Active, 10),
            MemoryRetentionDisposition::Review
        );
        assert_eq!(
            evaluate_retention(policy, MemoryLifecycle::Active, 20),
            MemoryRetentionDisposition::Archive
        );
        assert_eq!(
            evaluate_retention(policy, MemoryLifecycle::Active, 30),
            MemoryRetentionDisposition::Tombstone
        );
        assert!(MemoryLifecyclePolicy::try_new(Some(20), Some(10), None).is_none());
    }

    #[test]
    fn transitions_reject_silent_resurrection_and_require_successors() {
        assert_eq!(
            transition_memory_lifecycle(
                MemoryLifecycle::NeedsReview,
                MemoryLifecycleReason::ReviewApproved,
                false,
            ),
            Ok(MemoryLifecycle::Active)
        );
        assert_eq!(
            transition_memory_lifecycle(
                MemoryLifecycle::Active,
                MemoryLifecycleReason::Superseded,
                false,
            ),
            Err(MemoryLifecycleTransitionError::SuccessorRequired)
        );
        assert_eq!(
            transition_memory_lifecycle(
                MemoryLifecycle::Active,
                MemoryLifecycleReason::Superseded,
                true,
            ),
            Ok(MemoryLifecycle::Superseded)
        );
        assert_eq!(
            transition_memory_lifecycle(
                MemoryLifecycle::Stale,
                MemoryLifecycleReason::Superseded,
                false,
            ),
            Err(MemoryLifecycleTransitionError::SuccessorRequired)
        );
        assert_eq!(
            transition_memory_lifecycle(
                MemoryLifecycle::Tombstoned,
                MemoryLifecycleReason::ReviewApproved,
                false,
            ),
            Err(MemoryLifecycleTransitionError::InvalidTransition)
        );
    }

    #[test]
    fn procedures_need_independent_verification() {
        assert_eq!(
            evaluate_memory_use(MemoryKind::Procedure, MemoryLifecycle::Active, false),
            MemoryUseEligibility::ReviewOnly
        );
        assert_eq!(
            evaluate_memory_use(MemoryKind::Procedure, MemoryLifecycle::Active, true),
            MemoryUseEligibility::Current
        );
        assert_eq!(
            evaluate_memory_use(MemoryKind::Policy, MemoryLifecycle::Quarantined, true),
            MemoryUseEligibility::Excluded
        );
    }
}
