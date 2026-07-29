use std::{
    error::Error,
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use repowitness_domain::{
    AnalysisArtifactDigest, ByteSpan, MAX_MEMORY_INTEROPERABLE_INTEGER, RepositoryPath,
    SourceSlotId,
};

use crate::{
    RustGraphSite, RustGraphSiteKind, RustGraphSiteOrdinal, RustSymbolFact, RustSymbolKind,
};

/// Version of the pure generation-local Rust resolver contract.
pub const RUST_GRAPH_RESOLVER_PROFILE_VERSION: u32 = 1;

const MAX_DEFINITIONS: u32 = 250_000;
const MAX_SITES: u32 = 250_000;
const MAX_CANDIDATES_PER_SITE: u32 = 4_096;
const MAX_TOTAL_CANDIDATES: u64 = 1_000_000;
const MAX_INPUT_TEXT_BYTES: u64 = 256 * 1024 * 1024;
const MAX_OUTPUT_BYTES: u64 = 256 * 1024 * 1024;

/// Independent admission and output limits for one complete resolver run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RustGraphResolutionLimits {
    max_definitions: u32,
    max_sites: u32,
    max_candidates_per_site: u32,
    max_total_candidates: u64,
    max_input_text_bytes: u64,
    max_output_bytes: u64,
}

impl RustGraphResolutionLimits {
    /// Conservative limits bounded by compiled hard ceilings.
    pub const DEFAULT: Self = Self {
        max_definitions: 100_000,
        max_sites: 250_000,
        max_candidates_per_site: 64,
        max_total_candidates: 500_000,
        max_input_text_bytes: 128 * 1024 * 1024,
        max_output_bytes: 128 * 1024 * 1024,
    };

    /// Creates positive limits no larger than the compiled hard ceilings.
    pub fn try_new(
        max_definitions: u32,
        max_sites: u32,
        max_candidates_per_site: u32,
        max_total_candidates: u64,
        max_input_text_bytes: u64,
        max_output_bytes: u64,
    ) -> Result<Self, RustGraphResolutionError> {
        let limits = Self {
            max_definitions,
            max_sites,
            max_candidates_per_site,
            max_total_candidates,
            max_input_text_bytes,
            max_output_bytes,
        };
        if limits.is_valid() {
            Ok(limits)
        } else {
            Err(RustGraphResolutionError::InvalidLimits)
        }
    }

    const fn is_valid(self) -> bool {
        self.max_definitions != 0
            && self.max_definitions <= MAX_DEFINITIONS
            && self.max_sites != 0
            && self.max_sites <= MAX_SITES
            && self.max_candidates_per_site >= 2
            && self.max_candidates_per_site <= MAX_CANDIDATES_PER_SITE
            && self.max_total_candidates != 0
            && self.max_total_candidates <= MAX_TOTAL_CANDIDATES
            && self.max_input_text_bytes != 0
            && self.max_input_text_bytes <= MAX_INPUT_TEXT_BYTES
            && self.max_output_bytes != 0
            && self.max_output_bytes <= MAX_OUTPUT_BYTES
    }

    /// Returns the admitted definition-occurrence limit.
    #[must_use]
    pub const fn max_definitions(self) -> u32 {
        self.max_definitions
    }

    /// Returns the admitted raw-site occurrence limit.
    #[must_use]
    pub const fn max_sites(self) -> u32 {
        self.max_sites
    }

    /// Returns the retained candidates-per-site limit.
    #[must_use]
    pub const fn max_candidates_per_site(self) -> u32 {
        self.max_candidates_per_site
    }

    /// Returns the aggregate retained-candidate limit.
    #[must_use]
    pub const fn max_total_candidates(self) -> u64 {
        self.max_total_candidates
    }

    /// Returns the aggregate input text-byte limit.
    #[must_use]
    pub const fn max_input_text_bytes(self) -> u64 {
        self.max_input_text_bytes
    }

    /// Returns the accounted immutable output-byte limit.
    #[must_use]
    pub const fn max_output_bytes(self) -> u64 {
        self.max_output_bytes
    }
}

impl Default for RustGraphResolutionLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Cooperative cancellation and absolute monotonic deadline.
#[derive(Clone, Copy)]
pub struct RustGraphResolutionControl<'a> {
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

impl<'a> RustGraphResolutionControl<'a> {
    /// Creates control state for one all-or-nothing resolver run.
    #[must_use]
    pub const fn new(cancelled: &'a AtomicBool, deadline: Instant) -> Self {
        Self {
            cancelled,
            deadline,
        }
    }

    pub(super) fn outcome(self) -> Option<RustGraphResolutionError> {
        if self.cancelled.load(Ordering::Acquire) {
            Some(RustGraphResolutionError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Some(RustGraphResolutionError::DeadlineExceeded)
        } else {
            None
        }
    }
}

impl fmt::Debug for RustGraphResolutionControl<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphResolutionControl")
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Stable content-redacted resolver failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustGraphResolutionError {
    /// One configured limit is invalid.
    InvalidLimits,
    /// One supplied occurrence violates the Rust occurrence contract.
    InvalidOccurrence,
    /// The definition input bound was exceeded.
    DefinitionLimitExceeded,
    /// The site input bound was exceeded.
    SiteLimitExceeded,
    /// The aggregate input text bound was exceeded.
    InputTextLimitExceeded,
    /// The aggregate retained-candidate bound was exceeded.
    CandidateLimitExceeded,
    /// The accounted output bound was exceeded.
    OutputLimitExceeded,
    /// An exact definition identity was supplied more than once.
    DuplicateDefinition,
    /// An exact site identity was supplied more than once.
    DuplicateSite,
    /// Fixed-width accounting overflowed.
    CountOverflow,
    /// Cancellation was observed before complete output existed.
    Cancelled,
    /// The absolute monotonic deadline elapsed.
    DeadlineExceeded,
}

impl fmt::Display for RustGraphResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLimits => "Rust graph resolution limits are invalid",
            Self::InvalidOccurrence => "Rust graph resolution occurrence is invalid",
            Self::DefinitionLimitExceeded => "Rust graph definition limit exceeded",
            Self::SiteLimitExceeded => "Rust graph site limit exceeded",
            Self::InputTextLimitExceeded => "Rust graph input text limit exceeded",
            Self::CandidateLimitExceeded => "Rust graph candidate limit exceeded",
            Self::OutputLimitExceeded => "Rust graph output limit exceeded",
            Self::DuplicateDefinition => "Rust graph definition identity is duplicated",
            Self::DuplicateSite => "Rust graph site identity is duplicated",
            Self::CountOverflow => "Rust graph resolution count overflowed",
            Self::Cancelled => "Rust graph resolution cancelled",
            Self::DeadlineExceeded => "Rust graph resolution deadline exceeded",
        })
    }
}

impl Error for RustGraphResolutionError {}

/// One validated declaration occurrence in the immutable resolver input.
#[derive(Clone, Eq, PartialEq)]
pub struct RustGraphDefinitionOccurrence {
    source_slot: SourceSlotId,
    path: RepositoryPath,
    artifact: AnalysisArtifactDigest,
    fact_ordinal: u64,
    fact: RustSymbolFact,
}

impl RustGraphDefinitionOccurrence {
    /// Constructs one exact target occurrence from validated artifact facts.
    pub fn try_new(
        source_slot: SourceSlotId,
        path: RepositoryPath,
        artifact: AnalysisArtifactDigest,
        fact_ordinal: u64,
        fact: RustSymbolFact,
    ) -> Result<Self, RustGraphResolutionError> {
        if fact_ordinal > MAX_MEMORY_INTEROPERABLE_INTEGER || !is_rust_path(&path) {
            return Err(RustGraphResolutionError::InvalidOccurrence);
        }
        Ok(Self {
            source_slot,
            path,
            artifact,
            fact_ordinal,
            fact,
        })
    }

    /// Returns the connected-workspace source slot.
    #[must_use]
    pub const fn source_slot(&self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the exact repository-relative source path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the immutable declaration artifact identity.
    #[must_use]
    pub const fn artifact(&self) -> AnalysisArtifactDigest {
        self.artifact
    }

    /// Returns the source-order fact ordinal.
    #[must_use]
    pub const fn fact_ordinal(&self) -> u64 {
        self.fact_ordinal
    }

    /// Returns the already validated declaration fact.
    #[must_use]
    pub const fn fact(&self) -> &RustSymbolFact {
        &self.fact
    }

    pub(super) fn identity(&self) -> RustGraphDefinitionIdentity {
        RustGraphDefinitionIdentity {
            source_slot: self.source_slot,
            path: self.path.clone(),
            artifact: self.artifact,
            fact_ordinal: self.fact_ordinal,
            kind: self.fact.kind(),
            name_span: self.fact.name_span(),
            declaration_span: self.fact.declaration_span(),
        }
    }
}

impl fmt::Debug for RustGraphDefinitionOccurrence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphDefinitionOccurrence")
            .field("source_slot", &self.source_slot)
            .field("path", &self.path)
            .field("artifact", &self.artifact)
            .field("fact_ordinal", &self.fact_ordinal)
            .field("kind", &self.fact.kind())
            .field("name", &"<redacted>")
            .field("qualified_name", &"<redacted>")
            .field("name_span", &self.fact.name_span())
            .field("declaration_span", &self.fact.declaration_span())
            .finish()
    }
}

/// One validated raw-site occurrence in the immutable resolver input.
#[derive(Clone, Eq, PartialEq)]
pub struct RustGraphSiteOccurrence {
    source_slot: SourceSlotId,
    path: RepositoryPath,
    artifact: AnalysisArtifactDigest,
    site: RustGraphSite,
}

impl RustGraphSiteOccurrence {
    /// Constructs an exact occurrence from one validated graph-site artifact.
    pub fn try_new(
        source_slot: SourceSlotId,
        path: RepositoryPath,
        artifact: AnalysisArtifactDigest,
        site: RustGraphSite,
    ) -> Result<Self, RustGraphResolutionError> {
        if !is_rust_path(&path) {
            return Err(RustGraphResolutionError::InvalidOccurrence);
        }
        Ok(Self {
            source_slot,
            path,
            artifact,
            site,
        })
    }

    /// Returns the connected-workspace source slot.
    #[must_use]
    pub const fn source_slot(&self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the exact repository-relative source path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the immutable graph-site artifact identity.
    #[must_use]
    pub const fn artifact(&self) -> AnalysisArtifactDigest {
        self.artifact
    }

    /// Returns the validated raw graph site.
    #[must_use]
    pub const fn site(&self) -> &RustGraphSite {
        &self.site
    }

    pub(super) fn identity(&self) -> RustGraphSiteIdentity {
        RustGraphSiteIdentity {
            source_slot: self.source_slot,
            path: self.path.clone(),
            artifact: self.artifact,
            ordinal: self.site.ordinal(),
            kind: self.site.kind(),
            occurrence_span: self.site.occurrence_span(),
            target_span: self.site.target_span(),
        }
    }
}

impl fmt::Debug for RustGraphSiteOccurrence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphSiteOccurrence")
            .field("source_slot", &self.source_slot)
            .field("path", &self.path)
            .field("artifact", &self.artifact)
            .field("ordinal", &self.site.ordinal())
            .field("kind", &self.site.kind())
            .field("target", &"<redacted>")
            .finish()
    }
}

/// Exact generation-local identity of one declaration target.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct RustGraphDefinitionIdentity {
    source_slot: SourceSlotId,
    path: RepositoryPath,
    artifact: AnalysisArtifactDigest,
    fact_ordinal: u64,
    kind: RustSymbolKind,
    name_span: ByteSpan,
    declaration_span: ByteSpan,
}

impl RustGraphDefinitionIdentity {
    /// Constructs one exact generation-local declaration identity.
    ///
    /// Source adapters must separately prove that both spans fit the immutable
    /// source blob and that the name bytes match the declared fact.
    pub fn try_new(
        source_slot: SourceSlotId,
        path: RepositoryPath,
        artifact: AnalysisArtifactDigest,
        fact_ordinal: u64,
        kind: RustSymbolKind,
        name_span: ByteSpan,
        declaration_span: ByteSpan,
    ) -> Result<Self, RustGraphResolutionError> {
        if fact_ordinal > MAX_MEMORY_INTEROPERABLE_INTEGER
            || !is_rust_path(&path)
            || name_span.is_empty()
            || !span_contains(declaration_span, name_span)
        {
            return Err(RustGraphResolutionError::InvalidOccurrence);
        }
        Ok(Self {
            source_slot,
            path,
            artifact,
            fact_ordinal,
            kind,
            name_span,
            declaration_span,
        })
    }

    /// Returns the source slot.
    #[must_use]
    pub const fn source_slot(&self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the exact repository path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the immutable declaration artifact identity.
    #[must_use]
    pub const fn artifact(&self) -> AnalysisArtifactDigest {
        self.artifact
    }

    /// Returns the exact fact ordinal.
    #[must_use]
    pub const fn fact_ordinal(&self) -> u64 {
        self.fact_ordinal
    }

    /// Returns the declaration category.
    #[must_use]
    pub const fn kind(&self) -> RustSymbolKind {
        self.kind
    }

    /// Returns the exact name span.
    #[must_use]
    pub const fn name_span(&self) -> ByteSpan {
        self.name_span
    }

    /// Returns the exact declaration span.
    #[must_use]
    pub const fn declaration_span(&self) -> ByteSpan {
        self.declaration_span
    }
}

impl fmt::Debug for RustGraphDefinitionIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphDefinitionIdentity")
            .field("source_slot", &self.source_slot)
            .field("path", &self.path)
            .field("artifact", &self.artifact)
            .field("fact_ordinal", &self.fact_ordinal)
            .field("kind", &self.kind)
            .field("name_span", &self.name_span)
            .field("declaration_span", &self.declaration_span)
            .finish()
    }
}

/// Exact generation-local identity of one raw site.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct RustGraphSiteIdentity {
    source_slot: SourceSlotId,
    path: RepositoryPath,
    artifact: AnalysisArtifactDigest,
    ordinal: RustGraphSiteOrdinal,
    kind: RustGraphSiteKind,
    occurrence_span: ByteSpan,
    target_span: ByteSpan,
}

impl RustGraphSiteIdentity {
    /// Constructs one exact generation-local raw-site identity.
    ///
    /// Source adapters must separately prove that both spans fit the immutable
    /// source blob and that the target bytes match the raw-site spelling.
    pub fn try_new(
        source_slot: SourceSlotId,
        path: RepositoryPath,
        artifact: AnalysisArtifactDigest,
        ordinal: RustGraphSiteOrdinal,
        kind: RustGraphSiteKind,
        occurrence_span: ByteSpan,
        target_span: ByteSpan,
    ) -> Result<Self, RustGraphResolutionError> {
        if !is_rust_path(&path)
            || target_span.is_empty()
            || !span_contains(occurrence_span, target_span)
        {
            return Err(RustGraphResolutionError::InvalidOccurrence);
        }
        Ok(Self {
            source_slot,
            path,
            artifact,
            ordinal,
            kind,
            occurrence_span,
            target_span,
        })
    }

    /// Returns the source slot.
    #[must_use]
    pub const fn source_slot(&self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the exact repository path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the graph-site artifact identity.
    #[must_use]
    pub const fn artifact(&self) -> AnalysisArtifactDigest {
        self.artifact
    }

    /// Returns the artifact-local ordinal.
    #[must_use]
    pub const fn ordinal(&self) -> RustGraphSiteOrdinal {
        self.ordinal
    }

    /// Returns the raw-site category.
    #[must_use]
    pub const fn kind(&self) -> RustGraphSiteKind {
        self.kind
    }

    /// Returns the complete construct span.
    #[must_use]
    pub const fn occurrence_span(&self) -> ByteSpan {
        self.occurrence_span
    }

    /// Returns the raw-target span.
    #[must_use]
    pub const fn target_span(&self) -> ByteSpan {
        self.target_span
    }
}

impl fmt::Debug for RustGraphSiteIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphSiteIdentity")
            .field("source_slot", &self.source_slot)
            .field("path", &self.path)
            .field("artifact", &self.artifact)
            .field("ordinal", &self.ordinal)
            .field("kind", &self.kind)
            .field("occurrence_span", &self.occurrence_span)
            .field("target_span", &self.target_span)
            .finish()
    }
}

fn is_rust_path(path: &RepositoryPath) -> bool {
    path.components()
        .next_back()
        .is_some_and(|component| component.ends_with(b".rs") && component.len() > 3)
}

fn span_contains(outer: ByteSpan, inner: ByteSpan) -> bool {
    outer.start() <= inner.start() && inner.end() <= outer.end()
}
