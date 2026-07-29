use std::{error::Error, fmt};

use super::super::{
    RustGraphDefinitionIdentity, RustGraphResolutionEvidence, RustGraphSiteEvidence,
    RustGraphSiteIdentity, RustGraphSiteKind,
};

/// Version of the deterministic Rust graph traversal contract.
pub const RUST_GRAPH_TRAVERSAL_PROFILE_VERSION: u32 = 1;

/// Stable relationship categories supported by traversal profile 1.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RustGraphEdgeKind {
    /// A supported import relationship.
    Import,
    /// A supported reference relationship.
    Reference,
    /// A supported free-call relationship.
    Call,
}

impl RustGraphEdgeKind {
    /// Returns the stable persistence and wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Reference => "reference",
            Self::Call => "call",
        }
    }

    pub(super) const fn from_site_kind(kind: RustGraphSiteKind) -> Option<Self> {
        match kind {
            RustGraphSiteKind::Import => Some(Self::Import),
            RustGraphSiteKind::Reference => Some(Self::Reference),
            RustGraphSiteKind::Call => Some(Self::Call),
            RustGraphSiteKind::MacroCall | RustGraphSiteKind::TestMarker => None,
        }
    }
}

/// Non-empty allow-list of relationship categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RustGraphEdgeKinds {
    import: bool,
    reference: bool,
    call: bool,
}

impl RustGraphEdgeKinds {
    /// Allows every relationship category in traversal profile 1.
    pub const ALL: Self = Self {
        import: true,
        reference: true,
        call: true,
    };

    /// Constructs a non-empty category allow-list.
    pub const fn try_new(
        import: bool,
        reference: bool,
        call: bool,
    ) -> Result<Self, RustGraphTraceError> {
        if import || reference || call {
            Ok(Self {
                import,
                reference,
                call,
            })
        } else {
            Err(RustGraphTraceError::InvalidEdgeKinds)
        }
    }

    /// Reports whether one category is allowed.
    #[must_use]
    pub const fn allows(self, kind: RustGraphEdgeKind) -> bool {
        match kind {
            RustGraphEdgeKind::Import => self.import,
            RustGraphEdgeKind::Reference => self.reference,
            RustGraphEdgeKind::Call => self.call,
        }
    }
}

impl Default for RustGraphEdgeKinds {
    fn default() -> Self {
        Self::ALL
    }
}

/// Candidate cardinality retained from the categorical resolver outcome.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RustGraphRelationshipCardinality {
    /// The site has exactly one complete candidate.
    Unique,
    /// The site has two or more candidates and this edge is one retained option.
    Ambiguous {
        /// Complete candidate count before deterministic retention.
        candidate_count: u32,
        /// Number of candidates retained for this site.
        retained_candidates: u32,
        /// Whether one or more candidates were omitted by the configured bound.
        candidates_truncated: bool,
    },
}

impl RustGraphRelationshipCardinality {
    /// Constructs consistent ambiguous-candidate accounting.
    pub const fn try_ambiguous(
        candidate_count: u32,
        retained_candidates: u32,
        candidates_truncated: bool,
    ) -> Result<Self, RustGraphTraceError> {
        if candidate_count < 2
            || retained_candidates < 2
            || retained_candidates > candidate_count
            || candidates_truncated != (retained_candidates < candidate_count)
        {
            return Err(RustGraphTraceError::InvalidEdge);
        }
        Ok(Self::Ambiguous {
            candidate_count,
            retained_candidates,
            candidates_truncated,
        })
    }

    /// Returns complete candidate count.
    #[must_use]
    pub const fn candidate_count(self) -> u32 {
        match self {
            Self::Unique => 1,
            Self::Ambiguous {
                candidate_count, ..
            } => candidate_count,
        }
    }

    /// Returns retained candidate count.
    #[must_use]
    pub const fn retained_candidates(self) -> u32 {
        match self {
            Self::Unique => 1,
            Self::Ambiguous {
                retained_candidates,
                ..
            } => retained_candidates,
        }
    }

    /// Reports candidate truncation.
    #[must_use]
    pub const fn candidates_truncated(self) -> bool {
        match self {
            Self::Unique => false,
            Self::Ambiguous {
                candidates_truncated,
                ..
            } => candidates_truncated,
        }
    }

    /// Reports whether the relationship remains one ambiguous option.
    #[must_use]
    pub const fn is_ambiguous(self) -> bool {
        matches!(self, Self::Ambiguous { .. })
    }
}

/// One exact generation-local relationship with its originating site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphTraversalEdge {
    source: RustGraphDefinitionIdentity,
    site: RustGraphSiteIdentity,
    target: RustGraphDefinitionIdentity,
    extraction_evidence: RustGraphSiteEvidence,
    resolution_evidence: RustGraphResolutionEvidence,
    cardinality: RustGraphRelationshipCardinality,
}

impl RustGraphTraversalEdge {
    /// Constructs one relationship after validating its exact source-site join.
    pub fn try_new(
        source: RustGraphDefinitionIdentity,
        site: RustGraphSiteIdentity,
        target: RustGraphDefinitionIdentity,
        extraction_evidence: RustGraphSiteEvidence,
        resolution_evidence: RustGraphResolutionEvidence,
        cardinality: RustGraphRelationshipCardinality,
    ) -> Result<Self, RustGraphTraceError> {
        if RustGraphEdgeKind::from_site_kind(site.kind()).is_none()
            || source.source_slot() != site.source_slot()
            || source.path() != site.path()
            || !span_contains(source.declaration_span(), site.occurrence_span())
        {
            return Err(RustGraphTraceError::InvalidEdge);
        }
        Ok(Self {
            source,
            site,
            target,
            extraction_evidence,
            resolution_evidence,
            cardinality,
        })
    }

    /// Returns the exact enclosing declaration.
    #[must_use]
    pub const fn source(&self) -> &RustGraphDefinitionIdentity {
        &self.source
    }

    /// Returns the exact originating site.
    #[must_use]
    pub const fn site(&self) -> &RustGraphSiteIdentity {
        &self.site
    }

    /// Returns the exact candidate target.
    #[must_use]
    pub const fn target(&self) -> &RustGraphDefinitionIdentity {
        &self.target
    }

    /// Returns the relationship category.
    #[must_use]
    pub fn kind(&self) -> RustGraphEdgeKind {
        RustGraphEdgeKind::from_site_kind(self.site.kind()).expect("validated traversal edge kind")
    }

    /// Returns raw-site extraction evidence.
    #[must_use]
    pub const fn extraction_evidence(&self) -> RustGraphSiteEvidence {
        self.extraction_evidence
    }

    /// Returns target-resolution evidence.
    #[must_use]
    pub const fn resolution_evidence(&self) -> RustGraphResolutionEvidence {
        self.resolution_evidence
    }

    /// Returns categorical candidate accounting.
    #[must_use]
    pub const fn cardinality(&self) -> RustGraphRelationshipCardinality {
        self.cardinality
    }
}

/// Explicit graph traversal direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustGraphTraceDirection {
    /// Follow enclosing definitions to candidate targets.
    Outbound,
    /// Follow candidate targets back to enclosing definitions.
    Inbound,
}

/// Exact starting declaration or raw-site occurrence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RustGraphTraceStart {
    /// Begin at one exact declaration.
    Definition(RustGraphDefinitionIdentity),
    /// Begin with only the relationships emitted by one exact site.
    Site(RustGraphSiteIdentity),
}

/// Conservative impact classification.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RustGraphImpactClass {
    /// At least one complete unique non-heuristic path exists.
    DirectlyConnected,
    /// Only ambiguous or heuristic paths are available.
    Possible,
    /// Unsupported or truncated work prevents a target claim.
    Unknown,
}

/// Stable all-or-nothing traversal failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustGraphTraceError {
    /// One resource limit is zero or exceeds a compiled ceiling.
    InvalidLimits,
    /// No relationship category was allowed.
    InvalidEdgeKinds,
    /// One relationship violates exact identity or cardinality invariants.
    InvalidEdge,
    /// Input contains the same exact relationship more than once.
    DuplicateEdge,
    /// Input relationship count exceeds the request bound.
    InputEdgeLimitExceeded,
    /// Aggregate encoded input exceeds the request bound.
    InputByteLimitExceeded,
    /// The exact requested raw-site occurrence has no retained relationship.
    StartUnavailable,
    /// A fixed-width count or byte total overflowed.
    CountOverflow,
    /// Cooperative cancellation was observed.
    Cancelled,
    /// The absolute monotonic deadline elapsed.
    DeadlineExceeded,
    /// Complete encoded output would exceed its bound.
    OutputLimitExceeded,
}

impl fmt::Display for RustGraphTraceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLimits => "Rust graph traversal limits are invalid",
            Self::InvalidEdgeKinds => "Rust graph traversal edge kinds are invalid",
            Self::InvalidEdge => "Rust graph traversal relationship is invalid",
            Self::DuplicateEdge => "Rust graph traversal contains a duplicate relationship",
            Self::InputEdgeLimitExceeded => "Rust graph traversal input edge limit exceeded",
            Self::InputByteLimitExceeded => "Rust graph traversal input byte limit exceeded",
            Self::StartUnavailable => "Rust graph traversal start is unavailable",
            Self::CountOverflow => "Rust graph traversal accounting overflowed",
            Self::Cancelled => "Rust graph traversal was cancelled",
            Self::DeadlineExceeded => "Rust graph traversal deadline exceeded",
            Self::OutputLimitExceeded => "Rust graph traversal output byte limit exceeded",
        })
    }
}

impl Error for RustGraphTraceError {}

fn span_contains(outer: repowitness_domain::ByteSpan, inner: repowitness_domain::ByteSpan) -> bool {
    outer.start() <= inner.start() && inner.end() <= outer.end()
}
