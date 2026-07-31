//! Versioned Phase 2 evidence-ranking vocabulary.
//!
//! These values carry no storage, wire, parser, or ranking behavior. They are
//! the stable validated vocabulary shared by provider adapters and the pure
//! analysis allocator.

use std::{error::Error, fmt};

use crate::{
    ConnectedWorkspaceId, RepositoryIdentityDigest, SourceManifestDigest, SourceSlotId,
    SourceSnapshotDigest,
};

/// Stable identifier for the first Phase 2 evidence-ranking profile.
pub const PHASE2_EVIDENCE_BALANCED_PROFILE_ID: &str = "phase2-evidence-balanced-v1";
/// Version of the first Phase 2 evidence-ranking profile.
pub const PHASE2_EVIDENCE_BALANCED_PROFILE_VERSION: u16 = 1;

/// A named immutable Phase 2 evidence-ranking profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase2ContextProfile {
    /// Tiered evidence-balanced allocation defined by ADR-0036.
    EvidenceBalancedV1,
}

impl Phase2ContextProfile {
    /// Returns the stable profile ID carried by requests, results, and receipts.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::EvidenceBalancedV1 => PHASE2_EVIDENCE_BALANCED_PROFILE_ID,
        }
    }

    /// Returns the stable profile version.
    #[must_use]
    pub const fn version(self) -> u16 {
        match self {
            Self::EvidenceBalancedV1 => PHASE2_EVIDENCE_BALANCED_PROFILE_VERSION,
        }
    }
}

/// Ordered evidence tiers for one Phase 2 context request.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Phase2ContextTier {
    /// The exact user-request anchor, admitted before all evidence tiers.
    Anchor,
    /// Applicable validated immutable SCIP precision-overlay evidence.
    PreciseOverlay,
    /// Exact syntax-derived declarations and coverage.
    Syntax,
    /// Bounded structural dependency or containment evidence.
    Structural,
    /// Bounded reference, caller, callee, or impact evidence.
    References,
    /// Current engineering-memory evidence.
    Memory,
    /// Trusted attributed Git-history evidence.
    History,
    /// Explicit unresolved or incomplete supporting context.
    Unresolved,
}

/// Categorical availability reported by one Phase 2 provider before allocation.
///
/// Availability is intentionally distinct from a budget omission: an unavailable
/// provider did not contribute an admissible complete candidate, while an
/// omitted provider did and could not fit the selected context budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase2ContextProviderAvailability {
    /// At least one bounded, source-scoped candidate was available.
    Available,
    /// The provider did not produce an admissible candidate for this request.
    Unavailable,
}

/// Complete provider-level coverage for one Phase 2 evidence tier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Phase2ContextProviderCoverage {
    tier: Phase2ContextTier,
    availability: Phase2ContextProviderAvailability,
    candidate_count: u64,
}

impl Phase2ContextProviderCoverage {
    /// Validates one categorical provider outcome before ranking.
    pub const fn try_new(
        tier: Phase2ContextTier,
        availability: Phase2ContextProviderAvailability,
        candidate_count: u64,
    ) -> Result<Self, Phase2ContextProviderCoverageError> {
        if matches!(tier, Phase2ContextTier::Anchor)
            || matches!(availability, Phase2ContextProviderAvailability::Available)
                != (candidate_count != 0)
        {
            return Err(Phase2ContextProviderCoverageError::InvalidOutcome);
        }
        Ok(Self {
            tier,
            availability,
            candidate_count,
        })
    }

    /// Returns the covered evidence tier.
    #[must_use]
    pub const fn tier(self) -> Phase2ContextTier {
        self.tier
    }

    /// Returns whether the provider yielded admissible candidates.
    #[must_use]
    pub const fn availability(self) -> Phase2ContextProviderAvailability {
        self.availability
    }

    /// Returns the number of candidates before duplicate grouping and allocation.
    #[must_use]
    pub const fn candidate_count(self) -> u64 {
        self.candidate_count
    }
}

/// Invalid Phase 2 provider coverage outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase2ContextProviderCoverageError {
    /// Anchor coverage or an availability/count mismatch was supplied.
    InvalidOutcome,
}

impl fmt::Display for Phase2ContextProviderCoverageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid Phase 2 provider coverage outcome")
    }
}

impl Error for Phase2ContextProviderCoverageError {}

/// Stable typed identity supplied by the selected evidence provider.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Phase2ContextCandidateId([u8; 32]);

impl Phase2ContextCandidateId {
    /// Wraps a fixed-width canonical evidence identity.
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the canonical fixed-width identity bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Stable typed identity for one concrete evidence provider invocation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Phase2ContextProviderId([u8; 32]);

impl Phase2ContextProviderId {
    /// Wraps a fixed-width canonical provider identity.
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the canonical fixed-width identity bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Attribution retained for every provider that proved one exact candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Phase2ContextProviderAttribution {
    provider: Phase2ContextProviderId,
    tier: Phase2ContextTier,
    provider_rank: u32,
}

/// Exact immutable source member shared by all candidates in one context pack.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Phase2ContextScope {
    repository: RepositoryIdentityDigest,
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: i64,
    source_slot: SourceSlotId,
    source_epoch: u64,
    generation: i64,
    snapshot: SourceSnapshotDigest,
    manifest: SourceManifestDigest,
}

impl Phase2ContextScope {
    /// Constructs one fully pinned source-member scope.
    #[allow(
        clippy::too_many_arguments,
        reason = "every identity protects a distinct cross-provider consistency boundary"
    )]
    pub const fn try_new(
        repository: RepositoryIdentityDigest,
        connected_workspace: ConnectedWorkspaceId,
        workspace_view: i64,
        source_slot: SourceSlotId,
        source_epoch: u64,
        generation: i64,
        snapshot: SourceSnapshotDigest,
        manifest: SourceManifestDigest,
    ) -> Result<Self, Phase2ContextScopeError> {
        if workspace_view <= 0 || source_epoch == 0 || generation <= 0 {
            return Err(Phase2ContextScopeError::InvalidIdentity);
        }
        Ok(Self {
            repository,
            connected_workspace,
            workspace_view,
            source_slot,
            source_epoch,
            generation,
            snapshot,
            manifest,
        })
    }

    /// Returns the selected logical repository.
    #[must_use]
    pub const fn repository(self) -> RepositoryIdentityDigest {
        self.repository
    }

    /// Returns the selected connected workspace.
    #[must_use]
    pub const fn connected_workspace(self) -> ConnectedWorkspaceId {
        self.connected_workspace
    }

    /// Returns the selected immutable workspace view.
    #[must_use]
    pub const fn workspace_view(self) -> i64 {
        self.workspace_view
    }

    /// Returns the selected source slot.
    #[must_use]
    pub const fn source_slot(self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the selected source-slot epoch.
    #[must_use]
    pub const fn source_epoch(self) -> u64 {
        self.source_epoch
    }

    /// Returns the selected index generation.
    #[must_use]
    pub const fn generation(self) -> i64 {
        self.generation
    }

    /// Returns the selected exact source snapshot.
    #[must_use]
    pub const fn snapshot(self) -> SourceSnapshotDigest {
        self.snapshot
    }

    /// Returns the selected source manifest.
    #[must_use]
    pub const fn manifest(self) -> SourceManifestDigest {
        self.manifest
    }
}

/// One non-positive identity in an immutable Phase 2 source scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase2ContextScopeError {
    /// Workspace view, source epoch, or generation was non-positive.
    InvalidIdentity,
}

impl fmt::Display for Phase2ContextScopeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Phase 2 context scope contains an invalid immutable identity")
    }
}

impl Error for Phase2ContextScopeError {}

impl Phase2ContextProviderAttribution {
    /// Constructs one independently attributable provider result.
    #[must_use]
    pub const fn new(
        provider: Phase2ContextProviderId,
        tier: Phase2ContextTier,
        provider_rank: u32,
    ) -> Self {
        Self {
            provider,
            tier,
            provider_rank,
        }
    }

    /// Returns the concrete provider identity.
    #[must_use]
    pub const fn provider(self) -> Phase2ContextProviderId {
        self.provider
    }

    /// Returns the provider's evidence tier for this exact candidate.
    #[must_use]
    pub const fn tier(self) -> Phase2ContextTier {
        self.tier
    }

    /// Returns the provider-local relevance rank.
    #[must_use]
    pub const fn provider_rank(self) -> u32 {
        self.provider_rank
    }
}
