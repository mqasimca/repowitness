use repowitness_analysis::{
    RustGraphRelationshipCardinality, RustGraphResolutionEvidence, RustGraphSiteEvidence,
    RustGraphTraceCoverage, RustGraphUnresolvedReason, RustSymbolKind,
};
use repowitness_domain::SourceContentDigest;

use super::read_model::{
    RustGraphDefinitionRecord, RustGraphPublicationSummary, RustGraphSiteSelector,
};

/// One retained exact candidate with its attributed resolution evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphCandidateRecord {
    pub(crate) target: RustGraphDefinitionRecord,
    pub(crate) evidence: RustGraphResolutionEvidence,
}

impl RustGraphCandidateRecord {
    /// Returns the exact target declaration.
    #[must_use]
    pub const fn target(&self) -> &RustGraphDefinitionRecord {
        &self.target
    }

    /// Returns the evidence class without confidence upgrading.
    #[must_use]
    pub const fn evidence(&self) -> RustGraphResolutionEvidence {
        self.evidence
    }
}

/// Decoded categorical outcome for one exact site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RustGraphOutcomeRecord {
    /// Complete work found no supported candidate.
    Unresolved(RustGraphUnresolvedReason),
    /// Exactly one retained candidate.
    Unique(Box<RustGraphCandidateRecord>),
    /// Two or more deterministic candidates, possibly truncated.
    Ambiguous(Box<[RustGraphCandidateRecord]>),
}

/// Complete exact evidence lookup, excluding source bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphEvidenceResult {
    pub(crate) publication: RustGraphPublicationSummary,
    pub(crate) site: RustGraphSiteSelector,
    pub(crate) content_digest: SourceContentDigest,
    pub(crate) extraction_evidence: RustGraphSiteEvidence,
    pub(crate) outcome: RustGraphOutcomeRecord,
    pub(crate) candidate_count: u32,
    pub(crate) candidates_truncated: bool,
}

impl RustGraphEvidenceResult {
    /// Returns the immutable graph receipt used by the lookup.
    #[must_use]
    pub const fn publication(&self) -> &RustGraphPublicationSummary {
        &self.publication
    }

    /// Returns the exact originating site selector.
    #[must_use]
    pub const fn site(&self) -> &RustGraphSiteSelector {
        &self.site
    }

    /// Returns the exact source-content digest for capability-backed retrieval.
    #[must_use]
    pub const fn content_digest(&self) -> SourceContentDigest {
        self.content_digest
    }

    /// Returns the extraction evidence without confidence upgrading.
    #[must_use]
    pub const fn extraction_evidence(&self) -> RustGraphSiteEvidence {
        self.extraction_evidence
    }

    /// Returns the categorical zero/one/many outcome.
    #[must_use]
    pub const fn outcome(&self) -> &RustGraphOutcomeRecord {
        &self.outcome
    }

    /// Returns the exact candidate count before retention truncation.
    #[must_use]
    pub const fn candidate_count(&self) -> u32 {
        self.candidate_count
    }

    /// Returns whether candidate retention was truncated.
    #[must_use]
    pub const fn candidates_truncated(&self) -> bool {
        self.candidates_truncated
    }
}

/// Bounded exact symbol search inside one graph projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphSymbolSearchResult {
    pub(crate) publication: RustGraphPublicationSummary,
    pub(crate) definitions: Box<[RustGraphDefinitionRecord]>,
    pub(crate) total_matches: u64,
    pub(crate) output_bytes: u64,
}

impl RustGraphSymbolSearchResult {
    /// Returns the immutable graph receipt used by the search.
    #[must_use]
    pub const fn publication(&self) -> &RustGraphPublicationSummary {
        &self.publication
    }

    /// Returns exact declarations in deterministic identity order.
    #[must_use]
    pub const fn definitions(&self) -> &[RustGraphDefinitionRecord] {
        &self.definitions
    }

    /// Returns exact matches before the caller result limit.
    #[must_use]
    pub const fn total_matches(&self) -> u64 {
        self.total_matches
    }

    /// Returns conservatively encoded output bytes.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
}

/// Stable stored unique edge categories.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RustGraphEdgeKind {
    /// Supported import target.
    Import,
    /// Supported reference target.
    Reference,
    /// Supported free-call target.
    Call,
}

impl RustGraphEdgeKind {
    /// Returns the stable persistence spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Reference => "reference",
            Self::Call => "call",
        }
    }

    pub(crate) fn from_stable_str(value: &str) -> Option<Self> {
        match value {
            "import" => Some(Self::Import),
            "reference" => Some(Self::Reference),
            "call" => Some(Self::Call),
            _ => None,
        }
    }
}

/// One explicit traversal direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustGraphDirection {
    /// Follow enclosing definition to unique target.
    Outbound,
    /// Follow unique target back to enclosing definitions.
    Inbound,
}

/// Exact declaration or raw-site start for one generation-pinned trace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RustGraphTraceStart {
    /// Begin from one exact declaration returned by graph symbol search.
    Definition(RustGraphDefinitionRecord),
    /// Begin from one exact raw site returned by graph evidence lookup.
    Site(RustGraphSiteSelector),
}

/// One deterministic trace step between exact declarations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphEdgeRecord {
    pub(crate) depth: u32,
    pub(crate) kind: RustGraphEdgeKind,
    pub(crate) extraction_evidence: RustGraphSiteEvidence,
    pub(crate) resolution_evidence: RustGraphResolutionEvidence,
    pub(crate) cardinality: RustGraphRelationshipCardinality,
    pub(crate) site: RustGraphSiteSelector,
    pub(crate) source: RustGraphDefinitionRecord,
    pub(crate) target: RustGraphDefinitionRecord,
}

impl RustGraphEdgeRecord {
    /// Returns the one-based traversal depth of this edge.
    #[must_use]
    pub const fn depth(&self) -> u32 {
        self.depth
    }

    /// Returns the exact stored edge category.
    #[must_use]
    pub const fn kind(&self) -> RustGraphEdgeKind {
        self.kind
    }

    /// Returns attributed raw-site extraction evidence.
    #[must_use]
    pub const fn extraction_evidence(&self) -> RustGraphSiteEvidence {
        self.extraction_evidence
    }

    /// Returns attributed target-resolution evidence.
    #[must_use]
    pub const fn resolution_evidence(&self) -> RustGraphResolutionEvidence {
        self.resolution_evidence
    }

    /// Returns the complete candidate cardinality for the originating site.
    #[must_use]
    pub const fn cardinality(&self) -> RustGraphRelationshipCardinality {
        self.cardinality
    }

    /// Returns the exact originating raw site.
    #[must_use]
    pub const fn site(&self) -> &RustGraphSiteSelector {
        &self.site
    }

    /// Returns the exact enclosing source declaration.
    #[must_use]
    pub const fn source(&self) -> &RustGraphDefinitionRecord {
        &self.source
    }

    /// Returns the exact target declaration.
    #[must_use]
    pub const fn target(&self) -> &RustGraphDefinitionRecord {
        &self.target
    }
}

/// Complete bounded traversal result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphTraceResult {
    pub(crate) publication: RustGraphPublicationSummary,
    pub(crate) edges: Box<[RustGraphEdgeRecord]>,
    pub(crate) visited_nodes: u64,
    pub(crate) visited_edges: u64,
    pub(crate) maximum_completed_depth: u32,
    pub(crate) truncation: RustGraphTraceTruncation,
    pub(crate) coverage: RustGraphTraceCoverage,
    pub(crate) input_bytes: u64,
    pub(crate) output_bytes: u64,
}

/// Exact traversal bounds that stopped otherwise pending work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RustGraphTraceTruncation {
    pub(crate) depth: bool,
    pub(crate) visited_nodes: bool,
    pub(crate) visited_edges: bool,
    pub(crate) frontier: bool,
    pub(crate) results: bool,
}

impl RustGraphTraceTruncation {
    /// Returns whether the depth bound left a non-empty next frontier.
    #[must_use]
    pub const fn depth(self) -> bool {
        self.depth
    }

    /// Returns whether the distinct-node bound stopped traversal.
    #[must_use]
    pub const fn visited_nodes(self) -> bool {
        self.visited_nodes
    }

    /// Returns whether the examined-edge bound stopped traversal.
    #[must_use]
    pub const fn visited_edges(self) -> bool {
        self.visited_edges
    }

    /// Returns whether the pending-frontier bound stopped traversal.
    #[must_use]
    pub const fn frontier(self) -> bool {
        self.frontier
    }

    /// Returns whether the emitted-result bound stopped traversal.
    #[must_use]
    pub const fn results(self) -> bool {
        self.results
    }

    /// Returns whether any declared traversal bound truncated work.
    #[must_use]
    pub const fn any(self) -> bool {
        self.depth || self.visited_nodes || self.visited_edges || self.frontier || self.results
    }
}

impl RustGraphTraceResult {
    /// Returns the immutable graph receipt used by traversal.
    #[must_use]
    pub const fn publication(&self) -> &RustGraphPublicationSummary {
        &self.publication
    }

    /// Returns deterministic emitted trace edges.
    #[must_use]
    pub const fn edges(&self) -> &[RustGraphEdgeRecord] {
        &self.edges
    }

    /// Returns distinct visited declaration count.
    #[must_use]
    pub const fn visited_nodes(&self) -> u64 {
        self.visited_nodes
    }

    /// Returns examined unique edge count.
    #[must_use]
    pub const fn visited_edges(&self) -> u64 {
        self.visited_edges
    }

    /// Returns the greatest fully completed traversal depth.
    #[must_use]
    pub const fn maximum_completed_depth(&self) -> u32 {
        self.maximum_completed_depth
    }

    /// Returns the exact set of traversal bounds that stopped pending work.
    #[must_use]
    pub const fn truncation(&self) -> RustGraphTraceTruncation {
        self.truncation
    }

    /// Returns generation-level limitations not representable as relationships.
    #[must_use]
    pub const fn coverage(&self) -> RustGraphTraceCoverage {
        self.coverage
    }

    /// Returns conservatively encoded admitted relationship bytes.
    #[must_use]
    pub const fn input_bytes(&self) -> u64 {
        self.input_bytes
    }

    /// Returns conservatively encoded output bytes.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
}

/// Count-only architecture summary over one immutable graph projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphArchitectureSummary {
    pub(crate) publication: RustGraphPublicationSummary,
    pub(crate) definitions_by_kind: Box<[(RustSymbolKind, u64)]>,
    pub(crate) edges_by_kind: Box<[(RustGraphEdgeKind, u64)]>,
}

impl RustGraphArchitectureSummary {
    /// Returns the immutable graph receipt used by the summary.
    #[must_use]
    pub const fn publication(&self) -> &RustGraphPublicationSummary {
        &self.publication
    }

    /// Returns deterministic declaration counts by kind.
    #[must_use]
    pub const fn definitions_by_kind(&self) -> &[(RustSymbolKind, u64)] {
        &self.definitions_by_kind
    }

    /// Returns deterministic unique edge counts by kind.
    #[must_use]
    pub const fn edges_by_kind(&self) -> &[(RustGraphEdgeKind, u64)] {
        &self.edges_by_kind
    }
}

/// Conservative impact strength.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RustGraphImpactClass {
    /// A unique non-heuristic inbound relationship.
    DirectlyConnected,
    /// An inbound relationship uses heuristic evidence.
    Possible,
    /// Unsupported, macro, truncated, or ambiguous coverage remains.
    Unknown,
}

/// One impacted declaration and the shortest supported inbound path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphImpactedDefinition {
    pub(crate) class: RustGraphImpactClass,
    pub(crate) definition: RustGraphDefinitionRecord,
    pub(crate) minimum_depth: u32,
}

impl RustGraphImpactedDefinition {
    /// Returns conservative impact strength.
    #[must_use]
    pub const fn class(&self) -> RustGraphImpactClass {
        self.class
    }

    /// Returns the exact impacted declaration.
    #[must_use]
    pub const fn definition(&self) -> &RustGraphDefinitionRecord {
        &self.definition
    }

    /// Returns the shortest observed inbound path.
    #[must_use]
    pub const fn minimum_depth(&self) -> u32 {
        self.minimum_depth
    }
}

/// Conservative inbound impact projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphImpactResult {
    pub(crate) trace: RustGraphTraceResult,
    pub(crate) impacted: Box<[RustGraphImpactedDefinition]>,
    pub(crate) unknown_coverage: bool,
    pub(crate) output_bytes: u64,
}

impl RustGraphImpactResult {
    /// Returns the complete bounded inbound trace.
    #[must_use]
    pub const fn trace(&self) -> &RustGraphTraceResult {
        &self.trace
    }

    /// Returns deduplicated impacted declarations and conservative classes.
    #[must_use]
    pub const fn impacted(&self) -> &[RustGraphImpactedDefinition] {
        &self.impacted
    }

    /// Returns whether incomplete graph coverage prevents a closed-world claim.
    #[must_use]
    pub const fn unknown_coverage(&self) -> bool {
        self.unknown_coverage
    }

    /// Returns conservatively encoded output bytes including the trace.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
}
