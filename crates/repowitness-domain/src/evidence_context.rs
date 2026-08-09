//! Versioned evidence-balanced evidence-ranking vocabulary.
//!
//! These values carry no storage, wire, parser, or ranking behavior. They are
//! the stable validated vocabulary shared by provider adapters and the pure
//! analysis allocator.

use std::{error::Error, fmt};

use crate::{
    ConnectedWorkspaceId, RepositoryIdentityDigest, SourceManifestDigest, SourceSlotId,
    SourceSnapshotDigest,
};

/// Stable identifier for the first evidence-balanced evidence-ranking profile.
pub const EVIDENCE_BALANCED_PROFILE_ID: &str = "evidence-balanced-v1";
/// Version of the first evidence-balanced evidence-ranking profile.
pub const EVIDENCE_BALANCED_PROFILE_VERSION: u16 = 1;

/// A named immutable evidence-balanced evidence-ranking profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceContextProfile {
    /// Tiered evidence-balanced allocation defined by ADR-0036.
    EvidenceBalancedV1,
}

impl EvidenceContextProfile {
    /// Returns the stable profile ID carried by requests, results, and receipts.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::EvidenceBalancedV1 => EVIDENCE_BALANCED_PROFILE_ID,
        }
    }

    /// Returns the stable profile version.
    #[must_use]
    pub const fn version(self) -> u16 {
        match self {
            Self::EvidenceBalancedV1 => EVIDENCE_BALANCED_PROFILE_VERSION,
        }
    }
}

/// Ordered evidence tiers for one evidence-balanced context request.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum EvidenceContextTier {
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

/// Categorical availability reported by one evidence-balanced provider before allocation.
///
/// Availability is intentionally distinct from a budget omission: an unavailable
/// provider did not contribute an admissible complete candidate, while an
/// omitted provider did and could not fit the selected context budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceContextProviderAvailability {
    /// At least one bounded, source-scoped candidate was available.
    Available,
    /// The provider did not produce an admissible candidate for this request.
    Unavailable,
}

/// Complete provider-level coverage for one evidence-balanced evidence tier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceContextProviderCoverage {
    tier: EvidenceContextTier,
    availability: EvidenceContextProviderAvailability,
    candidate_count: u64,
}

impl EvidenceContextProviderCoverage {
    /// Validates one categorical provider outcome before ranking.
    pub const fn try_new(
        tier: EvidenceContextTier,
        availability: EvidenceContextProviderAvailability,
        candidate_count: u64,
    ) -> Result<Self, EvidenceContextProviderCoverageError> {
        if matches!(tier, EvidenceContextTier::Anchor)
            || matches!(availability, EvidenceContextProviderAvailability::Available)
                != (candidate_count != 0)
        {
            return Err(EvidenceContextProviderCoverageError::InvalidOutcome);
        }
        Ok(Self {
            tier,
            availability,
            candidate_count,
        })
    }

    /// Returns the covered evidence tier.
    #[must_use]
    pub const fn tier(self) -> EvidenceContextTier {
        self.tier
    }

    /// Returns whether the provider yielded admissible candidates.
    #[must_use]
    pub const fn availability(self) -> EvidenceContextProviderAvailability {
        self.availability
    }

    /// Returns the number of candidates before duplicate grouping and allocation.
    #[must_use]
    pub const fn candidate_count(self) -> u64 {
        self.candidate_count
    }
}

/// Invalid evidence-balanced provider coverage outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceContextProviderCoverageError {
    /// Anchor coverage or an availability/count mismatch was supplied.
    InvalidOutcome,
}

impl fmt::Display for EvidenceContextProviderCoverageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid evidence-balanced provider coverage outcome")
    }
}

impl Error for EvidenceContextProviderCoverageError {}

/// Stable typed identity supplied by the selected evidence provider.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EvidenceContextCandidateId([u8; 32]);

impl EvidenceContextCandidateId {
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
pub struct EvidenceContextProviderId([u8; 32]);

impl EvidenceContextProviderId {
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
pub struct EvidenceContextProviderAttribution {
    provider: EvidenceContextProviderId,
    tier: EvidenceContextTier,
    provider_rank: u32,
}

/// Exact immutable source member shared by all candidates in one context pack.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceContextScope {
    repository: RepositoryIdentityDigest,
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: i64,
    source_slot: SourceSlotId,
    source_epoch: u64,
    generation: i64,
    snapshot: SourceSnapshotDigest,
    manifest: SourceManifestDigest,
}

impl EvidenceContextScope {
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
    ) -> Result<Self, EvidenceContextScopeError> {
        if workspace_view <= 0 || source_epoch == 0 || generation <= 0 {
            return Err(EvidenceContextScopeError::InvalidIdentity);
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

/// One non-positive identity in an immutable evidence-balanced source scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceContextScopeError {
    /// Workspace view, source epoch, or generation was non-positive.
    InvalidIdentity,
}

impl fmt::Display for EvidenceContextScopeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .write_str("evidence-balanced context scope contains an invalid immutable identity")
    }
}

impl Error for EvidenceContextScopeError {}

impl EvidenceContextProviderAttribution {
    /// Constructs one independently attributable provider result.
    #[must_use]
    pub const fn new(
        provider: EvidenceContextProviderId,
        tier: EvidenceContextTier,
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
    pub const fn provider(self) -> EvidenceContextProviderId {
        self.provider
    }

    /// Returns the provider's evidence tier for this exact candidate.
    #[must_use]
    pub const fn tier(self) -> EvidenceContextTier {
        self.tier
    }

    /// Returns the provider-local relevance rank.
    #[must_use]
    pub const fn provider_rank(self) -> u32 {
        self.provider_rank
    }
}
