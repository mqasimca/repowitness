//! Deterministic Phase 2 evidence-balanced context admission.
//!
//! This profile is deliberately separate from the accepted Phase 0 RRF
//! compiler. Callers supply already-validated, generation-pinned evidence
//! candidates; this module performs no storage, filesystem, or graph I/O.

use repowitness_domain::{
    Phase2ContextCandidateId, Phase2ContextProfile, Phase2ContextProviderAttribution,
    Phase2ContextProviderCoverage, Phase2ContextProviderId, Phase2ContextScope, Phase2ContextTier,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    error::Error,
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

/// Maximum candidate items admitted to one bounded profile invocation.
pub const MAX_PHASE2_CONTEXT_CANDIDATES: usize = 10_000;
/// Largest complete-item budget accepted by the first Phase 2 profile.
pub const MAX_PHASE2_CONTEXT_BUDGET_UNITS: u64 = 1_048_576;

/// A validated whole-item allocation budget for one Phase 2 profile run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Phase2ContextBudget(u64);

impl Phase2ContextBudget {
    /// Creates a bounded positive allocation budget.
    pub const fn try_new(units: u64) -> Result<Self, Phase2ContextError> {
        if units == 0 || units > MAX_PHASE2_CONTEXT_BUDGET_UNITS {
            return Err(Phase2ContextError::InvalidBudget);
        }
        Ok(Self(units))
    }

    /// Returns the conservative whole-item allocation budget.
    #[must_use]
    pub const fn units(self) -> u64 {
        self.0
    }
}

/// One complete candidate supplied by a single evidence provider.
pub struct Phase2ContextCandidate<T> {
    scope: Phase2ContextScope,
    tier: Phase2ContextTier,
    provider_rank: u32,
    estimated_units: u64,
    identity: Phase2ContextCandidateId,
    attributions: Box<[Phase2ContextProviderAttribution]>,
    payload: T,
}

impl<T> Phase2ContextCandidate<T> {
    /// Validates one whole-item evidence candidate.
    pub fn try_new(
        scope: Phase2ContextScope,
        tier: Phase2ContextTier,
        provider_rank: u32,
        estimated_units: u64,
        identity: Phase2ContextCandidateId,
        provider: Phase2ContextProviderId,
        payload: T,
    ) -> Result<Self, Phase2ContextError> {
        if provider_rank == 0 || estimated_units == 0 {
            return Err(Phase2ContextError::InvalidCandidate);
        }
        Ok(Self {
            scope,
            tier,
            provider_rank,
            estimated_units,
            identity,
            attributions: vec![Phase2ContextProviderAttribution::new(
                provider,
                tier,
                provider_rank,
            )]
            .into_boxed_slice(),
            payload,
        })
    }

    /// Returns the candidate's explicit evidence tier.
    #[must_use]
    pub const fn tier(&self) -> Phase2ContextTier {
        self.tier
    }

    /// Returns the exact immutable source scope independently validated for this item.
    #[must_use]
    pub const fn scope(&self) -> Phase2ContextScope {
        self.scope
    }

    /// Returns the provider-local relevance rank.
    #[must_use]
    pub const fn provider_rank(&self) -> u32 {
        self.provider_rank
    }

    /// Returns the complete-item budget cost.
    #[must_use]
    pub const fn estimated_units(&self) -> u64 {
        self.estimated_units
    }

    /// Returns the stable evidence identity used for deterministic ties.
    #[must_use]
    pub const fn identity(&self) -> Phase2ContextCandidateId {
        self.identity
    }

    /// Returns every independently attributable provider of this exact item.
    #[must_use]
    pub fn attributions(&self) -> &[Phase2ContextProviderAttribution] {
        &self.attributions
    }

    /// Returns the provider-owned validated payload.
    #[must_use]
    pub const fn payload(&self) -> &T {
        &self.payload
    }
}

const EVIDENCE_TIERS: [Phase2ContextTier; 7] = [
    Phase2ContextTier::PreciseOverlay,
    Phase2ContextTier::Syntax,
    Phase2ContextTier::Structural,
    Phase2ContextTier::References,
    Phase2ContextTier::Memory,
    Phase2ContextTier::History,
    Phase2ContextTier::Unresolved,
];

const fn tier_index(tier: Phase2ContextTier) -> usize {
    match tier {
        Phase2ContextTier::Anchor => 0,
        Phase2ContextTier::PreciseOverlay => 1,
        Phase2ContextTier::Syntax => 2,
        Phase2ContextTier::Structural => 3,
        Phase2ContextTier::References => 4,
        Phase2ContextTier::Memory => 5,
        Phase2ContextTier::History => 6,
        Phase2ContextTier::Unresolved => 7,
    }
}

const fn tier_lane(tier: Phase2ContextTier) -> Option<usize> {
    match tier {
        Phase2ContextTier::Anchor => None,
        Phase2ContextTier::PreciseOverlay => Some(0),
        Phase2ContextTier::Syntax => Some(1),
        Phase2ContextTier::Structural => Some(2),
        Phase2ContextTier::References => Some(3),
        Phase2ContextTier::Memory => Some(4),
        Phase2ContextTier::History => Some(5),
        Phase2ContextTier::Unresolved => Some(6),
    }
}

impl<T> fmt::Debug for Phase2ContextCandidate<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Phase2ContextCandidate")
            .field("scope", &self.scope)
            .field("tier", &self.tier)
            .field("provider_rank", &self.provider_rank)
            .field("estimated_units", &self.estimated_units)
            .field("identity", &self.identity)
            .field("attributions", &self.attributions)
            .field("payload", &"<redacted-provider-payload>")
            .finish()
    }
}

/// Stable failure from Phase 2 deterministic allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase2ContextError {
    /// The requested whole-item allocation budget was outside the fixed profile bound.
    InvalidBudget,
    /// The request supplied an anchor in the wrong tier or too many candidates.
    InvalidInput,
    /// A provider-local rank, item cost, or attribution was invalid.
    InvalidCandidate,
    /// Fixed-width candidate or budget arithmetic overflowed.
    CountNotRepresentable,
    /// Cancellation was observed before a complete allocation existed.
    Cancelled,
    /// The monotonic deadline elapsed before a complete allocation existed.
    DeadlineExceeded,
}

impl fmt::Display for Phase2ContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidBudget => "invalid Phase 2 context budget",
            Self::InvalidInput => "invalid Phase 2 context input",
            Self::InvalidCandidate => "invalid or duplicate Phase 2 context candidate",
            Self::CountNotRepresentable => "Phase 2 context count is not representable safely",
            Self::Cancelled => "Phase 2 context allocation cancelled",
            Self::DeadlineExceeded => "Phase 2 context allocation deadline exceeded",
        })
    }
}

impl Error for Phase2ContextError {}

/// Complete evidence inputs for one named Phase 2 allocation request.
pub struct Phase2ContextInput<T> {
    scope: Phase2ContextScope,
    anchor: Option<Phase2ContextCandidate<T>>,
    candidates: Vec<Phase2ContextCandidate<T>>,
    provider_coverage: Box<[Phase2ContextProviderCoverage]>,
}

impl<T> Phase2ContextInput<T> {
    /// Groups exact duplicates while retaining every independently attributable provider.
    pub fn try_new(
        scope: Phase2ContextScope,
        anchor: Option<Phase2ContextCandidate<T>>,
        candidates: Vec<Phase2ContextCandidate<T>>,
    ) -> Result<Self, Phase2ContextError> {
        if anchor.as_ref().is_some_and(|candidate| {
            candidate.tier() != Phase2ContextTier::Anchor || candidate.scope() != scope
        }) || candidates.len() > MAX_PHASE2_CONTEXT_CANDIDATES
            || candidates.iter().any(|candidate| {
                candidate.tier() == Phase2ContextTier::Anchor || candidate.scope() != scope
            })
        {
            return Err(Phase2ContextError::InvalidInput);
        }
        let mut identities = BTreeSet::new();
        if let Some(anchor) = &anchor
            && !identities.insert(anchor.identity())
        {
            return Err(Phase2ContextError::InvalidCandidate);
        }
        let mut grouped = BTreeMap::new();
        for candidate in candidates {
            if !identities.insert(candidate.identity())
                && !grouped.contains_key(&candidate.identity())
            {
                return Err(Phase2ContextError::InvalidCandidate);
            }
            match grouped.entry(candidate.identity()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(candidate);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().merge_duplicate(candidate)?;
                }
            }
        }
        Ok(Self {
            scope,
            anchor,
            candidates: grouped.into_values().collect(),
            provider_coverage: Box::new([]),
        })
    }

    /// Attaches independently collected provider availability to this request.
    ///
    /// A provider may be unavailable even when another tier has candidates;
    /// coverage is therefore supplied separately from allocation candidates.
    pub fn with_provider_coverage(
        mut self,
        coverage: Vec<Phase2ContextProviderCoverage>,
    ) -> Result<Self, Phase2ContextError> {
        let mut tiers = BTreeSet::new();
        if coverage
            .iter()
            .any(|coverage| !tiers.insert(coverage.tier()))
        {
            return Err(Phase2ContextError::InvalidInput);
        }
        self.provider_coverage = coverage.into_boxed_slice();
        Ok(self)
    }

    /// Returns the one immutable source scope shared by every candidate.
    #[must_use]
    pub const fn scope(&self) -> Phase2ContextScope {
        self.scope
    }
}

impl<T> Phase2ContextCandidate<T> {
    fn merge_duplicate(&mut self, duplicate: Self) -> Result<(), Phase2ContextError> {
        if self.estimated_units != duplicate.estimated_units
            || self.attributions.iter().any(|existing| {
                duplicate
                    .attributions
                    .iter()
                    .any(|incoming| existing.provider() == incoming.provider())
            })
        {
            return Err(Phase2ContextError::InvalidCandidate);
        }
        let old_tier = self.tier;
        let duplicate_tier = duplicate.tier;
        if duplicate_tier < old_tier
            || (duplicate_tier == old_tier
                && duplicate.primary_provider() < self.primary_provider())
        {
            self.tier = duplicate_tier;
            self.provider_rank = duplicate.provider_rank;
            self.payload = duplicate.payload;
        } else if duplicate_tier == old_tier {
            self.provider_rank = self.provider_rank.min(duplicate.provider_rank);
        }
        let mut attributions = self.attributions.to_vec();
        attributions.append(&mut duplicate.attributions.into_vec());
        attributions.sort_by(|left, right| {
            left.provider()
                .cmp(&right.provider())
                .then_with(|| left.tier().cmp(&right.tier()))
                .then_with(|| left.provider_rank().cmp(&right.provider_rank()))
        });
        self.attributions = attributions.into_boxed_slice();
        Ok(())
    }

    fn primary_provider(&self) -> Phase2ContextProviderId {
        self.attributions
            .iter()
            .map(|attribution| attribution.provider())
            .min()
            .expect("Phase 2 candidate always has one provider attribution")
    }
}

/// Categorical omission of complete candidates from one evidence tier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Phase2ContextOmission {
    tier: Phase2ContextTier,
    count: u64,
}

impl Phase2ContextOmission {
    /// Returns the evidence tier whose complete candidates did not fit.
    #[must_use]
    pub const fn tier(self) -> Phase2ContextTier {
        self.tier
    }

    /// Returns the number of omitted complete candidates.
    #[must_use]
    pub const fn count(self) -> u64 {
        self.count
    }
}

/// Complete allocation result in deterministic admission order.
pub struct Phase2ContextResult<T> {
    profile: Phase2ContextProfile,
    scope: Phase2ContextScope,
    budget: Phase2ContextBudget,
    used_units: u64,
    items: Box<[Phase2ContextCandidate<T>]>,
    provider_coverage: Box<[Phase2ContextProviderCoverage]>,
    omissions: Box<[Phase2ContextOmission]>,
}

impl<T> Phase2ContextResult<T> {
    /// Returns the named immutable allocation profile.
    #[must_use]
    pub const fn profile(&self) -> Phase2ContextProfile {
        self.profile
    }

    /// Returns the immutable source member shared by every admitted item.
    #[must_use]
    pub const fn scope(&self) -> Phase2ContextScope {
        self.scope
    }

    /// Returns the admitted whole-item budget.
    #[must_use]
    pub const fn budget(&self) -> Phase2ContextBudget {
        self.budget
    }

    /// Returns the sum of costs of every admitted complete item.
    #[must_use]
    pub const fn used_units(&self) -> u64 {
        self.used_units
    }

    /// Returns admitted candidates in deterministic allocation order.
    #[must_use]
    pub fn items(&self) -> &[Phase2ContextCandidate<T>] {
        &self.items
    }

    /// Returns categorical provider availability before allocation.
    #[must_use]
    pub fn provider_coverage(&self) -> &[Phase2ContextProviderCoverage] {
        &self.provider_coverage
    }

    /// Returns categorical complete-item budget omissions by tier.
    #[must_use]
    pub fn omissions(&self) -> &[Phase2ContextOmission] {
        &self.omissions
    }
}

/// Allocates exact evidence through a named deterministic Phase 2 profile.
pub fn compile_phase2_context<T>(
    profile: Phase2ContextProfile,
    input: Phase2ContextInput<T>,
    budget: Phase2ContextBudget,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Phase2ContextResult<T>, Phase2ContextError> {
    check_control(cancelled, deadline)?;
    let scope = input.scope;
    let mut lanes: [Vec<Phase2ContextCandidate<T>>; 7] = std::array::from_fn(|_| Vec::new());
    for candidate in input.candidates {
        let lane = tier_lane(candidate.tier()).ok_or(Phase2ContextError::InvalidInput)?;
        lanes[lane].push(candidate);
    }
    let mut lanes = lanes.map(|mut lane| {
        lane.sort_by(|left, right| {
            left.provider_rank()
                .cmp(&right.provider_rank())
                .then_with(|| left.estimated_units().cmp(&right.estimated_units()))
                .then_with(|| left.identity().cmp(&right.identity()))
        });
        VecDeque::from(lane)
    });
    let mut allocation = Phase2Allocation::new(profile, scope, budget, input.provider_coverage);
    if let Some(anchor) = input.anchor {
        allocation.try_admit(anchor, cancelled, deadline)?;
    }
    while allocation.admit_round(&mut lanes, cancelled, deadline)? {}
    allocation.finish()
}

struct Phase2Allocation<T> {
    profile: Phase2ContextProfile,
    scope: Phase2ContextScope,
    budget: Phase2ContextBudget,
    used_units: u64,
    items: Vec<Phase2ContextCandidate<T>>,
    provider_coverage: Box<[Phase2ContextProviderCoverage]>,
    omissions: [u64; 8],
}

impl<T> Phase2Allocation<T> {
    fn new(
        profile: Phase2ContextProfile,
        scope: Phase2ContextScope,
        budget: Phase2ContextBudget,
        provider_coverage: Box<[Phase2ContextProviderCoverage]>,
    ) -> Self {
        Self {
            profile,
            scope,
            budget,
            used_units: 0,
            items: Vec::new(),
            provider_coverage,
            omissions: [0; 8],
        }
    }

    fn try_admit(
        &mut self,
        candidate: Phase2ContextCandidate<T>,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<bool, Phase2ContextError> {
        check_control(cancelled, deadline)?;
        let next = self
            .used_units
            .checked_add(candidate.estimated_units())
            .ok_or(Phase2ContextError::CountNotRepresentable)?;
        if next > self.budget.units() {
            self.omit(candidate.tier())?;
            return Ok(false);
        }
        self.used_units = next;
        self.items.push(candidate);
        Ok(true)
    }

    fn admit_round(
        &mut self,
        lanes: &mut [VecDeque<Phase2ContextCandidate<T>>; 7],
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<bool, Phase2ContextError> {
        let mut admitted = false;
        for tier in EVIDENCE_TIERS {
            let lane = &mut lanes[tier_lane(tier).expect("evidence tier")];
            while let Some(candidate) = lane.pop_front() {
                if self.try_admit(candidate, cancelled, deadline)? {
                    admitted = true;
                    break;
                }
            }
        }
        Ok(admitted)
    }

    fn omit(&mut self, tier: Phase2ContextTier) -> Result<(), Phase2ContextError> {
        let count = &mut self.omissions[tier_index(tier)];
        *count = count
            .checked_add(1)
            .ok_or(Phase2ContextError::CountNotRepresentable)?;
        Ok(())
    }

    fn finish(self) -> Result<Phase2ContextResult<T>, Phase2ContextError> {
        let tiers = [
            Phase2ContextTier::Anchor,
            Phase2ContextTier::PreciseOverlay,
            Phase2ContextTier::Syntax,
            Phase2ContextTier::Structural,
            Phase2ContextTier::References,
            Phase2ContextTier::Memory,
            Phase2ContextTier::History,
            Phase2ContextTier::Unresolved,
        ];
        let omissions = tiers
            .into_iter()
            .zip(self.omissions)
            .filter_map(|(tier, count)| {
                (count != 0).then_some(Phase2ContextOmission { tier, count })
            })
            .collect();
        Ok(Phase2ContextResult {
            profile: self.profile,
            scope: self.scope,
            budget: self.budget,
            used_units: self.used_units,
            items: self.items.into_boxed_slice(),
            provider_coverage: self.provider_coverage,
            omissions,
        })
    }
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), Phase2ContextError> {
    if cancelled.load(Ordering::Acquire) {
        Err(Phase2ContextError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(Phase2ContextError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::atomic::AtomicBool, time::Duration};

    use repowitness_domain::{
        ConnectedWorkspaceId, PHASE2_EVIDENCE_BALANCED_PROFILE_ID,
        Phase2ContextProviderAvailability, Phase2ContextProviderCoverage, RepositoryIdentityDigest,
        SourceManifestDigest, SourceSlotId, SourceSnapshotDigest,
    };

    use super::*;

    fn scope() -> Phase2ContextScope {
        Phase2ContextScope::try_new(
            RepositoryIdentityDigest::new([1; 32]),
            ConnectedWorkspaceId::new([2; 32]),
            1,
            SourceSlotId::new([3; 32]),
            1,
            1,
            SourceSnapshotDigest::new([4; 32]),
            SourceManifestDigest::new([5; 32]),
        )
        .expect("test scope")
    }

    fn candidate(
        tier: Phase2ContextTier,
        rank: u32,
        units: u64,
        id: u8,
    ) -> Phase2ContextCandidate<&'static str> {
        Phase2ContextCandidate::try_new(
            scope(),
            tier,
            rank,
            units,
            Phase2ContextCandidateId::new([id; 32]),
            Phase2ContextProviderId::new([id; 32]),
            "payload",
        )
        .expect("candidate")
    }

    #[test]
    fn named_profile_admits_anchor_then_one_item_per_evidence_tier_per_round() {
        let cancelled = AtomicBool::new(false);
        let input = Phase2ContextInput::try_new(
            scope(),
            Some(candidate(Phase2ContextTier::Anchor, 1, 2, 1)),
            vec![
                candidate(Phase2ContextTier::Memory, 2, 2, 7),
                candidate(Phase2ContextTier::PreciseOverlay, 2, 2, 3),
                candidate(Phase2ContextTier::Syntax, 1, 2, 5),
                candidate(Phase2ContextTier::Memory, 1, 2, 6),
                candidate(Phase2ContextTier::PreciseOverlay, 1, 2, 2),
            ],
        )
        .expect("input");
        let result = compile_phase2_context(
            Phase2ContextProfile::EvidenceBalancedV1,
            input,
            Phase2ContextBudget::try_new(12).expect("budget"),
            &cancelled,
            Instant::now() + Duration::from_secs(1),
        )
        .expect("result");

        assert_eq!(result.profile().id(), PHASE2_EVIDENCE_BALANCED_PROFILE_ID);
        assert_eq!(result.profile().version(), 1);
        assert_eq!(result.used_units(), 12);
        assert_eq!(
            result
                .items()
                .iter()
                .map(Phase2ContextCandidate::tier)
                .collect::<Vec<_>>(),
            vec![
                Phase2ContextTier::Anchor,
                Phase2ContextTier::PreciseOverlay,
                Phase2ContextTier::Syntax,
                Phase2ContextTier::Memory,
                Phase2ContextTier::PreciseOverlay,
                Phase2ContextTier::Memory,
            ]
        );
    }

    #[test]
    fn provider_coverage_is_retained_independently_of_budget_omissions() {
        let cancelled = AtomicBool::new(false);
        let input = Phase2ContextInput::try_new(
            scope(),
            None,
            vec![candidate(Phase2ContextTier::Syntax, 1, 4, 8)],
        )
        .expect("input")
        .with_provider_coverage(vec![
            Phase2ContextProviderCoverage::try_new(
                Phase2ContextTier::PreciseOverlay,
                Phase2ContextProviderAvailability::Unavailable,
                0,
            )
            .expect("unavailable coverage"),
            Phase2ContextProviderCoverage::try_new(
                Phase2ContextTier::Syntax,
                Phase2ContextProviderAvailability::Available,
                1,
            )
            .expect("available coverage"),
        ])
        .expect("coverage");
        let result = compile_phase2_context(
            Phase2ContextProfile::EvidenceBalancedV1,
            input,
            Phase2ContextBudget::try_new(2).expect("budget"),
            &cancelled,
            Instant::now() + Duration::from_secs(1),
        )
        .expect("result");
        assert_eq!(result.provider_coverage().len(), 2);
        assert_eq!(
            result.provider_coverage()[0].availability(),
            Phase2ContextProviderAvailability::Unavailable
        );
        assert_eq!(result.omissions()[0].tier(), Phase2ContextTier::Syntax);
    }

    #[test]
    fn whole_item_admission_skips_an_oversize_tier_item_without_starving_a_later_tier() {
        let cancelled = AtomicBool::new(false);
        let input = Phase2ContextInput::try_new(
            scope(),
            None,
            vec![
                candidate(Phase2ContextTier::Syntax, 1, 6, 1),
                candidate(Phase2ContextTier::History, 1, 5, 2),
            ],
        )
        .expect("input");
        let result = compile_phase2_context(
            Phase2ContextProfile::EvidenceBalancedV1,
            input,
            Phase2ContextBudget::try_new(5).expect("budget"),
            &cancelled,
            Instant::now() + Duration::from_secs(1),
        )
        .expect("result");

        assert_eq!(result.items().len(), 1);
        assert_eq!(result.items()[0].tier(), Phase2ContextTier::History);
        assert_eq!(
            result.omissions(),
            &[Phase2ContextOmission {
                tier: Phase2ContextTier::Syntax,
                count: 1
            }]
        );
    }

    #[test]
    fn duplicate_candidates_retain_attribution_and_cancelled_work_fails_closed() {
        let duplicate = Phase2ContextCandidateId::new([9; 32]);
        let input = Phase2ContextInput::try_new(
            scope(),
            None,
            vec![
                Phase2ContextCandidate::try_new(
                    scope(),
                    Phase2ContextTier::Syntax,
                    2,
                    1,
                    duplicate,
                    Phase2ContextProviderId::new([2; 32]),
                    "syntax",
                )
                .expect("candidate"),
                Phase2ContextCandidate::try_new(
                    scope(),
                    Phase2ContextTier::PreciseOverlay,
                    1,
                    1,
                    duplicate,
                    Phase2ContextProviderId::new([1; 32]),
                    "overlay",
                )
                .expect("candidate"),
            ],
        )
        .expect("exact duplicates should group");
        let result = compile_phase2_context(
            Phase2ContextProfile::EvidenceBalancedV1,
            input,
            Phase2ContextBudget::try_new(1).expect("budget"),
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(1),
        )
        .expect("result");
        assert_eq!(result.items().len(), 1);
        assert_eq!(result.items()[0].tier(), Phase2ContextTier::PreciseOverlay);
        assert_eq!(result.items()[0].attributions().len(), 2);
        assert_eq!(result.items()[0].payload(), &"overlay");

        let cancelled = AtomicBool::new(true);
        let input = Phase2ContextInput::try_new(
            scope(),
            None,
            vec![candidate(Phase2ContextTier::Syntax, 1, 1, 1)],
        )
        .expect("input");
        assert!(matches!(
            compile_phase2_context(
                Phase2ContextProfile::EvidenceBalancedV1,
                input,
                Phase2ContextBudget::try_new(1).expect("budget"),
                &cancelled,
                Instant::now() + Duration::from_secs(1),
            ),
            Err(Phase2ContextError::Cancelled)
        ));
    }

    #[test]
    fn duplicate_group_selection_and_attribution_order_are_input_permutation_independent() {
        let duplicate = Phase2ContextCandidateId::new([9; 32]);
        let provider = |tier, rank, provider, payload| {
            Phase2ContextCandidate::try_new(
                scope(),
                tier,
                rank,
                2,
                duplicate,
                Phase2ContextProviderId::new([provider; 32]),
                payload,
            )
            .expect("candidate")
        };
        let compile = |candidates| {
            compile_phase2_context(
                Phase2ContextProfile::EvidenceBalancedV1,
                Phase2ContextInput::try_new(scope(), None, candidates).expect("input"),
                Phase2ContextBudget::try_new(2).expect("budget"),
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(1),
            )
            .expect("result")
        };
        let forward = compile(vec![
            provider(Phase2ContextTier::Syntax, 1, 2, "syntax"),
            provider(Phase2ContextTier::PreciseOverlay, 2, 3, "overlay-high"),
            provider(Phase2ContextTier::PreciseOverlay, 1, 1, "overlay-low"),
        ]);
        let reverse = compile(vec![
            provider(Phase2ContextTier::PreciseOverlay, 1, 1, "overlay-low"),
            provider(Phase2ContextTier::PreciseOverlay, 2, 3, "overlay-high"),
            provider(Phase2ContextTier::Syntax, 1, 2, "syntax"),
        ]);
        for result in [&forward, &reverse] {
            let [item] = result.items() else {
                panic!("one exact duplicate group should be admitted");
            };
            assert_eq!(item.tier(), Phase2ContextTier::PreciseOverlay);
            assert_eq!(item.provider_rank(), 1);
            assert_eq!(item.payload(), &"overlay-low");
            assert_eq!(
                item.attributions()
                    .iter()
                    .map(|attribution| attribution.provider())
                    .collect::<Vec<_>>(),
                vec![
                    Phase2ContextProviderId::new([1; 32]),
                    Phase2ContextProviderId::new([2; 32]),
                    Phase2ContextProviderId::new([3; 32]),
                ]
            );
        }
    }

    #[test]
    fn conflicting_duplicate_cost_or_provider_attribution_fails_closed() {
        let duplicate = Phase2ContextCandidateId::new([7; 32]);
        let candidate = |units, provider| {
            Phase2ContextCandidate::try_new(
                scope(),
                Phase2ContextTier::Syntax,
                1,
                units,
                duplicate,
                Phase2ContextProviderId::new([provider; 32]),
                "payload",
            )
            .expect("candidate")
        };
        assert!(matches!(
            Phase2ContextInput::try_new(scope(), None, vec![candidate(1, 1), candidate(2, 2)]),
            Err(Phase2ContextError::InvalidCandidate)
        ));
        assert!(matches!(
            Phase2ContextInput::try_new(scope(), None, vec![candidate(1, 1), candidate(1, 1)]),
            Err(Phase2ContextError::InvalidCandidate)
        ));
    }

    #[test]
    fn budget_boundaries_are_inclusive_and_invalid_before_allocation() {
        assert!(matches!(
            Phase2ContextBudget::try_new(0),
            Err(Phase2ContextError::InvalidBudget)
        ));
        assert_eq!(
            Phase2ContextBudget::try_new(MAX_PHASE2_CONTEXT_BUDGET_UNITS)
                .expect("maximum budget should be admitted")
                .units(),
            MAX_PHASE2_CONTEXT_BUDGET_UNITS
        );
        assert!(matches!(
            Phase2ContextBudget::try_new(MAX_PHASE2_CONTEXT_BUDGET_UNITS + 1),
            Err(Phase2ContextError::InvalidBudget)
        ));
    }

    #[test]
    fn a_mismatched_source_scope_fails_before_ranking_or_publication() {
        let expected = scope();
        let mismatched = Phase2ContextScope::try_new(
            expected.repository(),
            expected.connected_workspace(),
            expected.workspace_view(),
            expected.source_slot(),
            expected.source_epoch(),
            expected.generation(),
            SourceSnapshotDigest::new([6; 32]),
            expected.manifest(),
        )
        .expect("mismatched scope remains structurally valid");
        let candidate = Phase2ContextCandidate::try_new(
            mismatched,
            Phase2ContextTier::Syntax,
            1,
            1,
            Phase2ContextCandidateId::new([7; 32]),
            Phase2ContextProviderId::new([8; 32]),
            "payload",
        )
        .expect("candidate");
        assert!(matches!(
            Phase2ContextInput::try_new(expected, None, vec![candidate]),
            Err(Phase2ContextError::InvalidInput)
        ));
    }
}
