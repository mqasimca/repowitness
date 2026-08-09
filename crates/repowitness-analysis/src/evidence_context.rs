//! Deterministic evidence-balanced evidence-balanced context admission.
//!
//! This profile is deliberately separate from the accepted baseline RRF
//! compiler. Callers supply already-validated, generation-pinned evidence
//! candidates; this module performs no storage, filesystem, or graph I/O.

use repowitness_domain::{
    EvidenceContextCandidateId, EvidenceContextProfile, EvidenceContextProviderAttribution,
    EvidenceContextProviderCoverage, EvidenceContextProviderId, EvidenceContextScope,
    EvidenceContextTier,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    error::Error,
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

/// Maximum candidate items admitted to one bounded profile invocation.
pub const MAX_EVIDENCE_CONTEXT_CANDIDATES: usize = 10_000;
/// Largest complete-item budget accepted by the first evidence-balanced profile.
pub const MAX_EVIDENCE_CONTEXT_BUDGET_UNITS: u64 = 1_048_576;
/// Default conservative complete-item budget for context compilation.
pub const DEFAULT_EVIDENCE_CONTEXT_BUDGET_UNITS: u64 = 64 * 1024;

/// A validated whole-item allocation budget for one evidence-balanced profile run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceContextBudget(u64);

impl EvidenceContextBudget {
    /// Creates a bounded positive allocation budget.
    pub const fn try_new(units: u64) -> Result<Self, EvidenceContextError> {
        if units == 0 || units > MAX_EVIDENCE_CONTEXT_BUDGET_UNITS {
            return Err(EvidenceContextError::InvalidBudget);
        }
        Ok(Self(units))
    }

    /// Returns the conservative whole-item allocation budget.
    #[must_use]
    pub const fn units(self) -> u64 {
        self.0
    }
}

impl Default for EvidenceContextBudget {
    fn default() -> Self {
        Self(DEFAULT_EVIDENCE_CONTEXT_BUDGET_UNITS)
    }
}

/// One complete candidate supplied by a single evidence provider.
pub struct EvidenceContextCandidate<T> {
    scope: EvidenceContextScope,
    tier: EvidenceContextTier,
    provider_rank: u32,
    estimated_units: u64,
    identity: EvidenceContextCandidateId,
    attributions: Box<[EvidenceContextProviderAttribution]>,
    payload: T,
}

impl<T> EvidenceContextCandidate<T> {
    /// Validates one whole-item evidence candidate.
    pub fn try_new(
        scope: EvidenceContextScope,
        tier: EvidenceContextTier,
        provider_rank: u32,
        estimated_units: u64,
        identity: EvidenceContextCandidateId,
        provider: EvidenceContextProviderId,
        payload: T,
    ) -> Result<Self, EvidenceContextError> {
        if provider_rank == 0 || estimated_units == 0 {
            return Err(EvidenceContextError::InvalidCandidate);
        }
        Ok(Self {
            scope,
            tier,
            provider_rank,
            estimated_units,
            identity,
            attributions: vec![EvidenceContextProviderAttribution::new(
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
    pub const fn tier(&self) -> EvidenceContextTier {
        self.tier
    }

    /// Returns the exact immutable source scope independently validated for this item.
    #[must_use]
    pub const fn scope(&self) -> EvidenceContextScope {
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
    pub const fn identity(&self) -> EvidenceContextCandidateId {
        self.identity
    }

    /// Returns every independently attributable provider of this exact item.
    #[must_use]
    pub fn attributions(&self) -> &[EvidenceContextProviderAttribution] {
        &self.attributions
    }

    /// Returns the provider-owned validated payload.
    #[must_use]
    pub const fn payload(&self) -> &T {
        &self.payload
    }
}

const EVIDENCE_TIERS: [EvidenceContextTier; 7] = [
    EvidenceContextTier::PreciseOverlay,
    EvidenceContextTier::Syntax,
    EvidenceContextTier::Structural,
    EvidenceContextTier::References,
    EvidenceContextTier::Memory,
    EvidenceContextTier::History,
    EvidenceContextTier::Unresolved,
];

const fn tier_index(tier: EvidenceContextTier) -> usize {
    match tier {
        EvidenceContextTier::Anchor => 0,
        EvidenceContextTier::PreciseOverlay => 1,
        EvidenceContextTier::Syntax => 2,
        EvidenceContextTier::Structural => 3,
        EvidenceContextTier::References => 4,
        EvidenceContextTier::Memory => 5,
        EvidenceContextTier::History => 6,
        EvidenceContextTier::Unresolved => 7,
    }
}

const fn tier_lane(tier: EvidenceContextTier) -> Option<usize> {
    match tier {
        EvidenceContextTier::Anchor => None,
        EvidenceContextTier::PreciseOverlay => Some(0),
        EvidenceContextTier::Syntax => Some(1),
        EvidenceContextTier::Structural => Some(2),
        EvidenceContextTier::References => Some(3),
        EvidenceContextTier::Memory => Some(4),
        EvidenceContextTier::History => Some(5),
        EvidenceContextTier::Unresolved => Some(6),
    }
}

impl<T> fmt::Debug for EvidenceContextCandidate<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EvidenceContextCandidate")
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

/// Stable failure from evidence-balanced deterministic allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceContextError {
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

impl fmt::Display for EvidenceContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidBudget => "invalid evidence-balanced context budget",
            Self::InvalidInput => "invalid evidence-balanced context input",
            Self::InvalidCandidate => "invalid or duplicate evidence-balanced context candidate",
            Self::CountNotRepresentable => {
                "evidence-balanced context count is not representable safely"
            }
            Self::Cancelled => "evidence-balanced context allocation cancelled",
            Self::DeadlineExceeded => "evidence-balanced context allocation deadline exceeded",
        })
    }
}

impl Error for EvidenceContextError {}

/// Complete evidence inputs for one named evidence-balanced allocation request.
pub struct EvidenceContextInput<T> {
    scope: EvidenceContextScope,
    anchor: Option<EvidenceContextCandidate<T>>,
    candidates: Vec<EvidenceContextCandidate<T>>,
    provider_coverage: Box<[EvidenceContextProviderCoverage]>,
}

impl<T> EvidenceContextInput<T> {
    /// Groups exact duplicates while retaining every independently attributable provider.
    pub fn try_new(
        scope: EvidenceContextScope,
        anchor: Option<EvidenceContextCandidate<T>>,
        candidates: Vec<EvidenceContextCandidate<T>>,
    ) -> Result<Self, EvidenceContextError> {
        if anchor.as_ref().is_some_and(|candidate| {
            candidate.tier() != EvidenceContextTier::Anchor || candidate.scope() != scope
        }) || candidates.len() > MAX_EVIDENCE_CONTEXT_CANDIDATES
            || candidates.iter().any(|candidate| {
                candidate.tier() == EvidenceContextTier::Anchor || candidate.scope() != scope
            })
        {
            return Err(EvidenceContextError::InvalidInput);
        }
        let mut identities = BTreeSet::new();
        if let Some(anchor) = &anchor
            && !identities.insert(anchor.identity())
        {
            return Err(EvidenceContextError::InvalidCandidate);
        }
        let mut grouped = BTreeMap::new();
        for candidate in candidates {
            if !identities.insert(candidate.identity())
                && !grouped.contains_key(&candidate.identity())
            {
                return Err(EvidenceContextError::InvalidCandidate);
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
        coverage: Vec<EvidenceContextProviderCoverage>,
    ) -> Result<Self, EvidenceContextError> {
        let mut tiers = BTreeSet::new();
        if coverage
            .iter()
            .any(|coverage| !tiers.insert(coverage.tier()))
        {
            return Err(EvidenceContextError::InvalidInput);
        }
        self.provider_coverage = coverage.into_boxed_slice();
        Ok(self)
    }

    /// Returns the one immutable source scope shared by every candidate.
    #[must_use]
    pub const fn scope(&self) -> EvidenceContextScope {
        self.scope
    }
}

impl<T> EvidenceContextCandidate<T> {
    fn merge_duplicate(&mut self, duplicate: Self) -> Result<(), EvidenceContextError> {
        if self.estimated_units != duplicate.estimated_units
            || self.attributions.iter().any(|existing| {
                duplicate
                    .attributions
                    .iter()
                    .any(|incoming| existing.provider() == incoming.provider())
            })
        {
            return Err(EvidenceContextError::InvalidCandidate);
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

    fn primary_provider(&self) -> EvidenceContextProviderId {
        self.attributions
            .iter()
            .map(|attribution| attribution.provider())
            .min()
            .expect("evidence-balanced candidate always has one provider attribution")
    }
}

/// Categorical omission of complete candidates from one evidence tier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceContextOmission {
    tier: EvidenceContextTier,
    count: u64,
}

impl EvidenceContextOmission {
    /// Returns the evidence tier whose complete candidates did not fit.
    #[must_use]
    pub const fn tier(self) -> EvidenceContextTier {
        self.tier
    }

    /// Returns the number of omitted complete candidates.
    #[must_use]
    pub const fn count(self) -> u64 {
        self.count
    }
}

/// Complete allocation result in deterministic admission order.
pub struct EvidenceContextResult<T> {
    profile: EvidenceContextProfile,
    scope: EvidenceContextScope,
    budget: EvidenceContextBudget,
    used_units: u64,
    items: Box<[EvidenceContextCandidate<T>]>,
    provider_coverage: Box<[EvidenceContextProviderCoverage]>,
    omissions: Box<[EvidenceContextOmission]>,
}

impl<T> EvidenceContextResult<T> {
    /// Returns the named immutable allocation profile.
    #[must_use]
    pub const fn profile(&self) -> EvidenceContextProfile {
        self.profile
    }

    /// Returns the immutable source member shared by every admitted item.
    #[must_use]
    pub const fn scope(&self) -> EvidenceContextScope {
        self.scope
    }

    /// Returns the admitted whole-item budget.
    #[must_use]
    pub const fn budget(&self) -> EvidenceContextBudget {
        self.budget
    }

    /// Returns the sum of costs of every admitted complete item.
    #[must_use]
    pub const fn used_units(&self) -> u64 {
        self.used_units
    }

    /// Returns admitted candidates in deterministic allocation order.
    #[must_use]
    pub fn items(&self) -> &[EvidenceContextCandidate<T>] {
        &self.items
    }

    /// Returns categorical provider availability before allocation.
    #[must_use]
    pub fn provider_coverage(&self) -> &[EvidenceContextProviderCoverage] {
        &self.provider_coverage
    }

    /// Returns categorical complete-item budget omissions by tier.
    #[must_use]
    pub fn omissions(&self) -> &[EvidenceContextOmission] {
        &self.omissions
    }
}

/// Allocates exact evidence through a named deterministic evidence-balanced profile.
pub fn compile_evidence_context<T>(
    profile: EvidenceContextProfile,
    input: EvidenceContextInput<T>,
    budget: EvidenceContextBudget,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<EvidenceContextResult<T>, EvidenceContextError> {
    check_control(cancelled, deadline)?;
    let scope = input.scope;
    let mut lanes: [Vec<EvidenceContextCandidate<T>>; 7] = std::array::from_fn(|_| Vec::new());
    for candidate in input.candidates {
        let lane = tier_lane(candidate.tier()).ok_or(EvidenceContextError::InvalidInput)?;
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
    let mut allocation = EvidenceAllocation::new(profile, scope, budget, input.provider_coverage);
    if let Some(anchor) = input.anchor {
        allocation.try_admit(anchor, cancelled, deadline)?;
    }
    while allocation.admit_round(&mut lanes, cancelled, deadline)? {}
    allocation.finish()
}

struct EvidenceAllocation<T> {
    profile: EvidenceContextProfile,
    scope: EvidenceContextScope,
    budget: EvidenceContextBudget,
    used_units: u64,
    items: Vec<EvidenceContextCandidate<T>>,
    provider_coverage: Box<[EvidenceContextProviderCoverage]>,
    omissions: [u64; 8],
}

impl<T> EvidenceAllocation<T> {
    fn new(
        profile: EvidenceContextProfile,
        scope: EvidenceContextScope,
        budget: EvidenceContextBudget,
        provider_coverage: Box<[EvidenceContextProviderCoverage]>,
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
        candidate: EvidenceContextCandidate<T>,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<bool, EvidenceContextError> {
        check_control(cancelled, deadline)?;
        let next = self
            .used_units
            .checked_add(candidate.estimated_units())
            .ok_or(EvidenceContextError::CountNotRepresentable)?;
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
        lanes: &mut [VecDeque<EvidenceContextCandidate<T>>; 7],
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<bool, EvidenceContextError> {
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

    fn omit(&mut self, tier: EvidenceContextTier) -> Result<(), EvidenceContextError> {
        let count = &mut self.omissions[tier_index(tier)];
        *count = count
            .checked_add(1)
            .ok_or(EvidenceContextError::CountNotRepresentable)?;
        Ok(())
    }

    fn finish(self) -> Result<EvidenceContextResult<T>, EvidenceContextError> {
        let tiers = [
            EvidenceContextTier::Anchor,
            EvidenceContextTier::PreciseOverlay,
            EvidenceContextTier::Syntax,
            EvidenceContextTier::Structural,
            EvidenceContextTier::References,
            EvidenceContextTier::Memory,
            EvidenceContextTier::History,
            EvidenceContextTier::Unresolved,
        ];
        let omissions = tiers
            .into_iter()
            .zip(self.omissions)
            .filter_map(|(tier, count)| {
                (count != 0).then_some(EvidenceContextOmission { tier, count })
            })
            .collect();
        Ok(EvidenceContextResult {
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

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), EvidenceContextError> {
    if cancelled.load(Ordering::Acquire) {
        Err(EvidenceContextError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(EvidenceContextError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::atomic::AtomicBool, time::Duration};

    use repowitness_domain::{
        ConnectedWorkspaceId, EVIDENCE_BALANCED_PROFILE_ID, EvidenceContextProviderAvailability,
        EvidenceContextProviderCoverage, RepositoryIdentityDigest, SourceManifestDigest,
        SourceSlotId, SourceSnapshotDigest,
    };

    use super::*;

    fn scope() -> EvidenceContextScope {
        EvidenceContextScope::try_new(
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
        tier: EvidenceContextTier,
        rank: u32,
        units: u64,
        id: u8,
    ) -> EvidenceContextCandidate<&'static str> {
        EvidenceContextCandidate::try_new(
            scope(),
            tier,
            rank,
            units,
            EvidenceContextCandidateId::new([id; 32]),
            EvidenceContextProviderId::new([id; 32]),
            "payload",
        )
        .expect("candidate")
    }

    #[test]
    fn named_profile_admits_anchor_then_one_item_per_evidence_tier_per_round() {
        let cancelled = AtomicBool::new(false);
        let input = EvidenceContextInput::try_new(
            scope(),
            Some(candidate(EvidenceContextTier::Anchor, 1, 2, 1)),
            vec![
                candidate(EvidenceContextTier::Memory, 2, 2, 7),
                candidate(EvidenceContextTier::PreciseOverlay, 2, 2, 3),
                candidate(EvidenceContextTier::Syntax, 1, 2, 5),
                candidate(EvidenceContextTier::Memory, 1, 2, 6),
                candidate(EvidenceContextTier::PreciseOverlay, 1, 2, 2),
            ],
        )
        .expect("input");
        let result = compile_evidence_context(
            EvidenceContextProfile::EvidenceBalancedV1,
            input,
            EvidenceContextBudget::try_new(12).expect("budget"),
            &cancelled,
            Instant::now() + Duration::from_secs(1),
        )
        .expect("result");

        assert_eq!(result.profile().id(), EVIDENCE_BALANCED_PROFILE_ID);
        assert_eq!(result.profile().version(), 1);
        assert_eq!(result.used_units(), 12);
        assert_eq!(
            result
                .items()
                .iter()
                .map(EvidenceContextCandidate::tier)
                .collect::<Vec<_>>(),
            vec![
                EvidenceContextTier::Anchor,
                EvidenceContextTier::PreciseOverlay,
                EvidenceContextTier::Syntax,
                EvidenceContextTier::Memory,
                EvidenceContextTier::PreciseOverlay,
                EvidenceContextTier::Memory,
            ]
        );
    }

    #[test]
    fn provider_coverage_is_retained_independently_of_budget_omissions() {
        let cancelled = AtomicBool::new(false);
        let input = EvidenceContextInput::try_new(
            scope(),
            None,
            vec![candidate(EvidenceContextTier::Syntax, 1, 4, 8)],
        )
        .expect("input")
        .with_provider_coverage(vec![
            EvidenceContextProviderCoverage::try_new(
                EvidenceContextTier::PreciseOverlay,
                EvidenceContextProviderAvailability::Unavailable,
                0,
            )
            .expect("unavailable coverage"),
            EvidenceContextProviderCoverage::try_new(
                EvidenceContextTier::Syntax,
                EvidenceContextProviderAvailability::Available,
                1,
            )
            .expect("available coverage"),
        ])
        .expect("coverage");
        let result = compile_evidence_context(
            EvidenceContextProfile::EvidenceBalancedV1,
            input,
            EvidenceContextBudget::try_new(2).expect("budget"),
            &cancelled,
            Instant::now() + Duration::from_secs(1),
        )
        .expect("result");
        assert_eq!(result.provider_coverage().len(), 2);
        assert_eq!(
            result.provider_coverage()[0].availability(),
            EvidenceContextProviderAvailability::Unavailable
        );
        assert_eq!(result.omissions()[0].tier(), EvidenceContextTier::Syntax);
    }

    #[test]
    fn whole_item_admission_skips_an_oversize_tier_item_without_starving_a_later_tier() {
        let cancelled = AtomicBool::new(false);
        let input = EvidenceContextInput::try_new(
            scope(),
            None,
            vec![
                candidate(EvidenceContextTier::Syntax, 1, 6, 1),
                candidate(EvidenceContextTier::History, 1, 5, 2),
            ],
        )
        .expect("input");
        let result = compile_evidence_context(
            EvidenceContextProfile::EvidenceBalancedV1,
            input,
            EvidenceContextBudget::try_new(5).expect("budget"),
            &cancelled,
            Instant::now() + Duration::from_secs(1),
        )
        .expect("result");

        assert_eq!(result.items().len(), 1);
        assert_eq!(result.items()[0].tier(), EvidenceContextTier::History);
        assert_eq!(
            result.omissions(),
            &[EvidenceContextOmission {
                tier: EvidenceContextTier::Syntax,
                count: 1
            }]
        );
    }

    #[test]
    fn duplicate_candidates_retain_attribution_and_cancelled_work_fails_closed() {
        let duplicate = EvidenceContextCandidateId::new([9; 32]);
        let input = EvidenceContextInput::try_new(
            scope(),
            None,
            vec![
                EvidenceContextCandidate::try_new(
                    scope(),
                    EvidenceContextTier::Syntax,
                    2,
                    1,
                    duplicate,
                    EvidenceContextProviderId::new([2; 32]),
                    "syntax",
                )
                .expect("candidate"),
                EvidenceContextCandidate::try_new(
                    scope(),
                    EvidenceContextTier::PreciseOverlay,
                    1,
                    1,
                    duplicate,
                    EvidenceContextProviderId::new([1; 32]),
                    "overlay",
                )
                .expect("candidate"),
            ],
        )
        .expect("exact duplicates should group");
        let result = compile_evidence_context(
            EvidenceContextProfile::EvidenceBalancedV1,
            input,
            EvidenceContextBudget::try_new(1).expect("budget"),
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(1),
        )
        .expect("result");
        assert_eq!(result.items().len(), 1);
        assert_eq!(
            result.items()[0].tier(),
            EvidenceContextTier::PreciseOverlay
        );
        assert_eq!(result.items()[0].attributions().len(), 2);
        assert_eq!(result.items()[0].payload(), &"overlay");

        let cancelled = AtomicBool::new(true);
        let input = EvidenceContextInput::try_new(
            scope(),
            None,
            vec![candidate(EvidenceContextTier::Syntax, 1, 1, 1)],
        )
        .expect("input");
        assert!(matches!(
            compile_evidence_context(
                EvidenceContextProfile::EvidenceBalancedV1,
                input,
                EvidenceContextBudget::try_new(1).expect("budget"),
                &cancelled,
                Instant::now() + Duration::from_secs(1),
            ),
            Err(EvidenceContextError::Cancelled)
        ));
    }

    #[test]
    fn duplicate_group_selection_and_attribution_order_are_input_permutation_independent() {
        let duplicate = EvidenceContextCandidateId::new([9; 32]);
        let provider = |tier, rank, provider, payload| {
            EvidenceContextCandidate::try_new(
                scope(),
                tier,
                rank,
                2,
                duplicate,
                EvidenceContextProviderId::new([provider; 32]),
                payload,
            )
            .expect("candidate")
        };
        let compile = |candidates| {
            compile_evidence_context(
                EvidenceContextProfile::EvidenceBalancedV1,
                EvidenceContextInput::try_new(scope(), None, candidates).expect("input"),
                EvidenceContextBudget::try_new(2).expect("budget"),
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(1),
            )
            .expect("result")
        };
        let forward = compile(vec![
            provider(EvidenceContextTier::Syntax, 1, 2, "syntax"),
            provider(EvidenceContextTier::PreciseOverlay, 2, 3, "overlay-high"),
            provider(EvidenceContextTier::PreciseOverlay, 1, 1, "overlay-low"),
        ]);
        let reverse = compile(vec![
            provider(EvidenceContextTier::PreciseOverlay, 1, 1, "overlay-low"),
            provider(EvidenceContextTier::PreciseOverlay, 2, 3, "overlay-high"),
            provider(EvidenceContextTier::Syntax, 1, 2, "syntax"),
        ]);
        for result in [&forward, &reverse] {
            let [item] = result.items() else {
                panic!("one exact duplicate group should be admitted");
            };
            assert_eq!(item.tier(), EvidenceContextTier::PreciseOverlay);
            assert_eq!(item.provider_rank(), 1);
            assert_eq!(item.payload(), &"overlay-low");
            assert_eq!(
                item.attributions()
                    .iter()
                    .map(|attribution| attribution.provider())
                    .collect::<Vec<_>>(),
                vec![
                    EvidenceContextProviderId::new([1; 32]),
                    EvidenceContextProviderId::new([2; 32]),
                    EvidenceContextProviderId::new([3; 32]),
                ]
            );
        }
    }

    #[test]
    fn conflicting_duplicate_cost_or_provider_attribution_fails_closed() {
        let duplicate = EvidenceContextCandidateId::new([7; 32]);
        let candidate = |units, provider| {
            EvidenceContextCandidate::try_new(
                scope(),
                EvidenceContextTier::Syntax,
                1,
                units,
                duplicate,
                EvidenceContextProviderId::new([provider; 32]),
                "payload",
            )
            .expect("candidate")
        };
        assert!(matches!(
            EvidenceContextInput::try_new(scope(), None, vec![candidate(1, 1), candidate(2, 2)]),
            Err(EvidenceContextError::InvalidCandidate)
        ));
        assert!(matches!(
            EvidenceContextInput::try_new(scope(), None, vec![candidate(1, 1), candidate(1, 1)]),
            Err(EvidenceContextError::InvalidCandidate)
        ));
    }

    #[test]
    fn budget_boundaries_are_inclusive_and_invalid_before_allocation() {
        assert!(matches!(
            EvidenceContextBudget::try_new(0),
            Err(EvidenceContextError::InvalidBudget)
        ));
        assert_eq!(
            EvidenceContextBudget::try_new(MAX_EVIDENCE_CONTEXT_BUDGET_UNITS)
                .expect("maximum budget should be admitted")
                .units(),
            MAX_EVIDENCE_CONTEXT_BUDGET_UNITS
        );
        assert!(matches!(
            EvidenceContextBudget::try_new(MAX_EVIDENCE_CONTEXT_BUDGET_UNITS + 1),
            Err(EvidenceContextError::InvalidBudget)
        ));
    }

    #[test]
    fn a_mismatched_source_scope_fails_before_ranking_or_publication() {
        let expected = scope();
        let mismatched = EvidenceContextScope::try_new(
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
        let candidate = EvidenceContextCandidate::try_new(
            mismatched,
            EvidenceContextTier::Syntax,
            1,
            1,
            EvidenceContextCandidateId::new([7; 32]),
            EvidenceContextProviderId::new([8; 32]),
            "payload",
        )
        .expect("candidate");
        assert!(matches!(
            EvidenceContextInput::try_new(expected, None, vec![candidate]),
            Err(EvidenceContextError::InvalidInput)
        ));
    }
}
