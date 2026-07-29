//! Immutable Rust graph persistence inputs and generation-pinned read models.

mod preparation;
mod query_results;
mod read_model;

use std::{
    error::Error,
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use repowitness_analysis::{
    RustGraphDefinitionOccurrence, RustGraphResolution, RustGraphSiteAnalysis,
};
pub use repowitness_analysis::{
    RustGraphEdgeKinds, RustGraphRelationshipCardinality, RustGraphTraceCoverage,
};
use repowitness_application::{CanonicalAnalysisArtifactKey, ResolvedConfiguration};
use repowitness_domain::{
    AnalysisArtifactDigest, ConnectedWorkspaceId, RepositoryPath, SourceSlotId,
};

use super::GenerationId;

pub(super) use preparation::artifact_payload_digest_with_control;
pub use preparation::prepare_rust_graph_generation;
pub use query_results::{
    RustGraphArchitectureSummary, RustGraphCandidateRecord, RustGraphDirection, RustGraphEdgeKind,
    RustGraphEdgeRecord, RustGraphEvidenceResult, RustGraphImpactClass, RustGraphImpactResult,
    RustGraphImpactedDefinition, RustGraphOutcomeRecord, RustGraphSymbolSearchResult,
    RustGraphTraceResult, RustGraphTraceStart, RustGraphTraceTruncation,
};
pub use read_model::{
    RustGraphAvailability, RustGraphDefinitionRecord, RustGraphPublicationSummary,
    RustGraphSiteSelector,
};

/// Failure to assemble a complete graph projection before persistence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustGraphPreparationError {
    /// The connected source set is empty, too large, duplicated, or inconsistent.
    InvalidSources,
    /// An artifact occurrence is duplicated or does not belong to its source.
    InvalidArtifacts,
    /// A definition occurrence is duplicated or does not belong to its source.
    InvalidDefinitions,
    /// Resolver sites, outcomes, candidates, or coverage are inconsistent.
    InvalidResolution,
    /// Fixed-width accounting overflowed.
    CountOverflow,
    /// Cancellation was observed before complete output existed.
    Cancelled,
    /// The absolute monotonic deadline elapsed.
    DeadlineExceeded,
}

impl fmt::Display for RustGraphPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidSources => "Rust graph source set is invalid",
            Self::InvalidArtifacts => "Rust graph artifact set is invalid",
            Self::InvalidDefinitions => "Rust graph definition set is invalid",
            Self::InvalidResolution => "Rust graph resolution is invalid",
            Self::CountOverflow => "Rust graph preparation count overflowed",
            Self::Cancelled => "Rust graph preparation cancelled",
            Self::DeadlineExceeded => "Rust graph preparation deadline exceeded",
        })
    }
}

impl Error for RustGraphPreparationError {}

/// Cooperative cancellation and monotonic deadline for graph preparation.
#[derive(Clone, Copy)]
pub struct RustGraphPreparationControl<'a> {
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

impl<'a> RustGraphPreparationControl<'a> {
    /// Creates control state for one complete preparation.
    #[must_use]
    pub const fn new(cancelled: &'a AtomicBool, deadline: Instant) -> Self {
        Self {
            cancelled,
            deadline,
        }
    }

    fn check(self) -> Result<(), RustGraphPreparationError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(RustGraphPreparationError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(RustGraphPreparationError::DeadlineExceeded)
        } else {
            Ok(())
        }
    }
}

impl fmt::Debug for RustGraphPreparationControl<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphPreparationControl")
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// One source slot and concrete generation used by a graph projection.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RustGraphSource {
    source_slot: SourceSlotId,
    generation: GenerationId,
}

impl RustGraphSource {
    /// Binds one source slot to one immutable repository generation.
    #[must_use]
    pub const fn new(source_slot: SourceSlotId, generation: GenerationId) -> Self {
        Self {
            source_slot,
            generation,
        }
    }

    /// Returns the connected-workspace source slot.
    #[must_use]
    pub const fn source_slot(self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the selected repository generation.
    #[must_use]
    pub const fn generation(self) -> GenerationId {
        self.generation
    }
}

/// One reusable graph-site artifact at an exact source occurrence.
#[derive(Clone, Eq, PartialEq)]
pub struct PreparedRustGraphArtifact {
    source_slot: SourceSlotId,
    path: RepositoryPath,
    key: CanonicalAnalysisArtifactKey,
    artifact_digest: AnalysisArtifactDigest,
    payload_digest: [u8; 32],
    analysis: RustGraphSiteAnalysis,
}

impl PreparedRustGraphArtifact {
    /// Returns the source slot containing this artifact occurrence.
    #[must_use]
    pub const fn source_slot(&self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the exact repository-relative source path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns every semantics-affecting graph-artifact input.
    #[must_use]
    pub const fn key(&self) -> CanonicalAnalysisArtifactKey {
        self.key
    }

    /// Returns the canonical graph-artifact key digest.
    #[must_use]
    pub const fn artifact_digest(&self) -> AnalysisArtifactDigest {
        self.artifact_digest
    }

    /// Returns the canonical immutable site-payload digest.
    #[must_use]
    pub const fn payload_digest(&self) -> &[u8; 32] {
        &self.payload_digest
    }

    /// Returns complete bounded raw graph sites.
    #[must_use]
    pub const fn analysis(&self) -> &RustGraphSiteAnalysis {
        &self.analysis
    }
}

impl fmt::Debug for PreparedRustGraphArtifact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedRustGraphArtifact")
            .field("source_slot", &self.source_slot)
            .field("path", &self.path)
            .field("artifact_digest", &self.artifact_digest)
            .field("payload_digest", &"<redacted-digest>")
            .field("site_count", &self.analysis.sites().len())
            .field("syntax_error_nodes", &self.analysis.syntax_error_nodes())
            .finish()
    }
}

/// Complete deterministic graph projection ready for generation-owned staging.
#[derive(Clone, Eq, PartialEq)]
pub struct PreparedRustGraphGeneration {
    connected_workspace: ConnectedWorkspaceId,
    sources: Box<[RustGraphSource]>,
    artifacts: Box<[PreparedRustGraphArtifact]>,
    definitions: Box<[RustGraphDefinitionOccurrence]>,
    resolution: RustGraphResolution,
    input_digest: [u8; 32],
    output_digest: [u8; 32],
    edge_count: u64,
    syntax_error_nodes: u64,
    macro_sites: u64,
    test_marker_sites: u64,
    heuristic_sites: u64,
}

impl PreparedRustGraphGeneration {
    /// Returns the connected workspace whose exact source set was resolved.
    #[must_use]
    pub const fn connected_workspace(&self) -> ConnectedWorkspaceId {
        self.connected_workspace
    }

    /// Returns source slots in canonical identity order.
    #[must_use]
    pub const fn sources(&self) -> &[RustGraphSource] {
        &self.sources
    }

    /// Returns artifact occurrences in canonical slot/path order.
    #[must_use]
    pub const fn artifacts(&self) -> &[PreparedRustGraphArtifact] {
        &self.artifacts
    }

    /// Returns exact definition occurrences in canonical identity order.
    #[must_use]
    pub const fn definitions(&self) -> &[RustGraphDefinitionOccurrence] {
        &self.definitions
    }

    /// Returns the complete categorical resolver output.
    #[must_use]
    pub const fn resolution(&self) -> &RustGraphResolution {
        &self.resolution
    }

    /// Returns the canonical complete graph-input digest.
    #[must_use]
    pub const fn input_digest(&self) -> &[u8; 32] {
        &self.input_digest
    }

    /// Returns the canonical complete graph-output digest.
    #[must_use]
    pub const fn output_digest(&self) -> &[u8; 32] {
        &self.output_digest
    }

    /// Returns the number of published unique typed edges.
    #[must_use]
    pub const fn edge_count(&self) -> u64 {
        self.edge_count
    }

    /// Returns aggregate graph-artifact syntax error nodes.
    #[must_use]
    pub const fn syntax_error_nodes(&self) -> u64 {
        self.syntax_error_nodes
    }

    /// Returns raw macro-call site coverage.
    #[must_use]
    pub const fn macro_sites(&self) -> u64 {
        self.macro_sites
    }

    /// Returns raw test-marker site coverage.
    #[must_use]
    pub const fn test_marker_sites(&self) -> u64 {
        self.test_marker_sites
    }

    /// Returns raw syntax-heuristic site coverage.
    #[must_use]
    pub const fn heuristic_sites(&self) -> u64 {
        self.heuristic_sites
    }
}

impl fmt::Debug for PreparedRustGraphGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedRustGraphGeneration")
            .field("connected_workspace", &self.connected_workspace)
            .field("source_count", &self.sources.len())
            .field("artifact_count", &self.artifacts.len())
            .field("definition_count", &self.definitions.len())
            .field("resolution", &self.resolution)
            .field("input_digest", &"<redacted-digest>")
            .field("output_digest", &"<redacted-digest>")
            .field("edge_count", &self.edge_count)
            .finish()
    }
}

/// Caller bounds for one deterministic generation/view-pinned graph read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RustGraphReadLimits {
    max_input_edges: u64,
    max_input_bytes: u64,
    max_depth: u32,
    max_results: u32,
    max_visited_nodes: u64,
    max_visited_edges: u64,
    max_frontier: u64,
    max_output_bytes: u64,
}

impl RustGraphReadLimits {
    /// Constructs positive limits no larger than compiled hard ceilings.
    pub const fn try_new(
        max_depth: u32,
        max_results: u32,
        max_visited_nodes: u64,
        max_visited_edges: u64,
        max_frontier: u64,
        max_output_bytes: u64,
    ) -> Result<Self, RustGraphReadError> {
        Self::try_new_with_input(
            max_visited_edges,
            64 * 1024 * 1024,
            max_depth,
            max_results,
            max_visited_nodes,
            max_visited_edges,
            max_frontier,
            max_output_bytes,
        )
    }

    /// Constructs explicit input and traversal bounds below compiled ceilings.
    #[allow(
        clippy::too_many_arguments,
        reason = "each independent graph-read resource has an explicit bound"
    )]
    pub const fn try_new_with_input(
        max_input_edges: u64,
        max_input_bytes: u64,
        max_depth: u32,
        max_results: u32,
        max_visited_nodes: u64,
        max_visited_edges: u64,
        max_frontier: u64,
        max_output_bytes: u64,
    ) -> Result<Self, RustGraphReadError> {
        let limits = Self {
            max_input_edges,
            max_input_bytes,
            max_depth,
            max_results,
            max_visited_nodes,
            max_visited_edges,
            max_frontier,
            max_output_bytes,
        };
        if limits.is_valid() {
            Ok(limits)
        } else {
            Err(RustGraphReadError::InvalidLimits)
        }
    }

    const fn is_valid(self) -> bool {
        self.max_input_edges != 0
            && self.max_input_edges <= 4_000_000
            && self.max_input_bytes != 0
            && self.max_input_bytes <= 512 * 1024 * 1024
            && self.max_depth != 0
            && self.max_depth <= 256
            && self.max_results != 0
            && self.max_results <= 100_000
            && self.max_visited_nodes != 0
            && self.max_visited_nodes <= 1_000_000
            && self.max_visited_edges != 0
            && self.max_visited_edges <= 4_000_000
            && self.max_frontier != 0
            && self.max_frontier <= 1_000_000
            && self.max_output_bytes != 0
            && self.max_output_bytes <= 256 * 1024 * 1024
    }

    /// Returns the maximum complete relationship input count.
    #[must_use]
    pub const fn max_input_edges(self) -> u64 {
        self.max_input_edges
    }

    /// Returns the maximum conservatively encoded relationship input bytes.
    #[must_use]
    pub const fn max_input_bytes(self) -> u64 {
        self.max_input_bytes
    }

    /// Applies already policy-capped graph preferences without widening caller bounds.
    #[must_use]
    pub fn constrained_by(self, configuration: Option<&ResolvedConfiguration>) -> Self {
        let Some(configuration) = configuration else {
            return self;
        };
        let depth = *configuration.preferences().graph_depth().effective();
        let results = *configuration.preferences().graph_results().effective();
        Self {
            max_depth: self.max_depth.min(u32::try_from(depth).unwrap_or(u32::MAX)),
            max_results: self
                .max_results
                .min(u32::try_from(results).unwrap_or(u32::MAX)),
            ..self
        }
    }

    /// Returns the maximum completed traversal depth.
    #[must_use]
    pub const fn max_depth(self) -> u32 {
        self.max_depth
    }

    /// Returns the maximum emitted result count.
    #[must_use]
    pub const fn max_results(self) -> u32 {
        self.max_results
    }

    /// Returns the maximum distinct visited node count.
    #[must_use]
    pub const fn max_visited_nodes(self) -> u64 {
        self.max_visited_nodes
    }

    /// Returns the maximum examined edge count.
    #[must_use]
    pub const fn max_visited_edges(self) -> u64 {
        self.max_visited_edges
    }

    /// Returns the maximum pending frontier count.
    #[must_use]
    pub const fn max_frontier(self) -> u64 {
        self.max_frontier
    }

    /// Returns the maximum conservatively encoded output byte count.
    #[must_use]
    pub const fn max_output_bytes(self) -> u64 {
        self.max_output_bytes
    }
}

impl Default for RustGraphReadLimits {
    fn default() -> Self {
        Self {
            max_input_edges: 50_000,
            max_input_bytes: 64 * 1024 * 1024,
            max_depth: 8,
            max_results: 100,
            max_visited_nodes: 10_000,
            max_visited_edges: 50_000,
            max_frontier: 10_000,
            max_output_bytes: 4 * 1024 * 1024,
        }
    }
}

/// Stable failure from graph status or bounded graph reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustGraphReadError {
    /// One requested bound is zero or above its compiled ceiling.
    InvalidLimits,
    /// An exact symbol query is empty, oversized, or contains control bytes.
    InvalidQuery,
    /// An exact site selector cannot be represented by the persisted format.
    InvalidSelector,
    /// The relationship category allow-list is empty.
    InvalidEdgeKinds,
    /// The pinned view or graph-owning generation is unavailable.
    GenerationUnavailable,
    /// A complete graph was not produced for this legacy generation.
    GraphNotProduced,
    /// The exact declaration or retained raw-site relationship is unavailable.
    StartUnavailable,
    /// Persisted graph rows violate the immutable graph contract.
    CorruptGraph,
    /// The operation was cancelled.
    Cancelled,
    /// The absolute deadline elapsed.
    DeadlineExceeded,
    /// Complete relationship input exceeds caller count or byte bounds.
    InputLimitExceeded,
    /// The result exceeded its encoded-output budget.
    OutputLimitExceeded,
    /// SQLite failed without exposing raw database text.
    Store,
}

impl fmt::Display for RustGraphReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLimits => "Rust graph read limits are invalid",
            Self::InvalidQuery => "Rust graph query is invalid",
            Self::InvalidSelector => "Rust graph site selector is invalid",
            Self::InvalidEdgeKinds => "Rust graph edge kinds are invalid",
            Self::GenerationUnavailable => "Rust graph generation is unavailable",
            Self::GraphNotProduced => "Rust graph was not produced for this generation",
            Self::StartUnavailable => "Rust graph trace start is unavailable",
            Self::CorruptGraph => "Rust graph persistence is inconsistent",
            Self::Cancelled => "Rust graph read was cancelled",
            Self::DeadlineExceeded => "Rust graph read deadline exceeded",
            Self::InputLimitExceeded => "Rust graph relationship input limit exceeded",
            Self::OutputLimitExceeded => "Rust graph read output limit exceeded",
            Self::Store => "Rust graph store operation failed",
        })
    }
}

impl Error for RustGraphReadError {}

#[cfg(test)]
mod tests;
