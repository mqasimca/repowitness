use super::super::RustGraphDefinitionIdentity;
use super::model::{
    RUST_GRAPH_TRAVERSAL_PROFILE_VERSION, RustGraphEdgeKinds, RustGraphImpactClass,
    RustGraphTraceDirection, RustGraphTraceStart, RustGraphTraversalEdge,
};
use super::request::{
    RustGraphTraceControl, RustGraphTraceCoverage, RustGraphTraceLimits, RustGraphTraceRequest,
};

/// Independent traversal-bound outcomes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RustGraphTraceTruncation {
    depth: bool,
    visited_nodes: bool,
    visited_edges: bool,
    frontier: bool,
    results: bool,
}

impl RustGraphTraceTruncation {
    pub(super) fn set_depth(&mut self) {
        self.depth = true;
    }

    pub(super) fn set_visited_nodes(&mut self) {
        self.visited_nodes = true;
    }

    pub(super) fn set_visited_edges(&mut self) {
        self.visited_edges = true;
    }

    pub(super) fn set_frontier(&mut self) {
        self.frontier = true;
    }

    pub(super) fn set_results(&mut self) {
        self.results = true;
    }

    /// Reports depth truncation.
    #[must_use]
    pub const fn depth(self) -> bool {
        self.depth
    }

    /// Reports distinct-node truncation.
    #[must_use]
    pub const fn visited_nodes(self) -> bool {
        self.visited_nodes
    }

    /// Reports examined-edge truncation.
    #[must_use]
    pub const fn visited_edges(self) -> bool {
        self.visited_edges
    }

    /// Reports pending-frontier truncation.
    #[must_use]
    pub const fn frontier(self) -> bool {
        self.frontier
    }

    /// Reports returned-result truncation.
    #[must_use]
    pub const fn results(self) -> bool {
        self.results
    }

    /// Reports whether any traversal work was omitted.
    #[must_use]
    pub const fn any(self) -> bool {
        self.depth || self.visited_nodes || self.visited_edges || self.frontier || self.results
    }
}

/// One deterministic result edge at its shortest observed traversal depth.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphTraceEdge {
    depth: u32,
    relationship: RustGraphTraversalEdge,
}

impl RustGraphTraceEdge {
    pub(super) const fn new(depth: u32, relationship: RustGraphTraversalEdge) -> Self {
        Self {
            depth,
            relationship,
        }
    }

    /// Returns the one-based traversal depth.
    #[must_use]
    pub const fn depth(&self) -> u32 {
        self.depth
    }

    /// Returns the exact relationship and originating site.
    #[must_use]
    pub const fn relationship(&self) -> &RustGraphTraversalEdge {
        &self.relationship
    }
}

/// Complete bounded trace output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphTraceResult {
    edges: Box<[RustGraphTraceEdge]>,
    visited_nodes: u64,
    visited_edges: u64,
    maximum_completed_depth: u32,
    truncation: RustGraphTraceTruncation,
    coverage: RustGraphTraceCoverage,
    input_bytes: u64,
    output_bytes: u64,
}

impl RustGraphTraceResult {
    #[allow(
        clippy::too_many_arguments,
        reason = "the complete trace receipt keeps independent accounting explicit"
    )]
    pub(super) fn new(
        edges: Box<[RustGraphTraceEdge]>,
        visited_nodes: u64,
        visited_edges: u64,
        maximum_completed_depth: u32,
        truncation: RustGraphTraceTruncation,
        coverage: RustGraphTraceCoverage,
        input_bytes: u64,
        output_bytes: u64,
    ) -> Self {
        Self {
            edges,
            visited_nodes,
            visited_edges,
            maximum_completed_depth,
            truncation,
            coverage,
            input_bytes,
            output_bytes,
        }
    }

    /// Returns the traversal profile.
    #[must_use]
    pub const fn profile_version(&self) -> u32 {
        RUST_GRAPH_TRAVERSAL_PROFILE_VERSION
    }

    /// Returns deterministic shortest-depth relationship results.
    #[must_use]
    pub const fn edges(&self) -> &[RustGraphTraceEdge] {
        &self.edges
    }

    /// Returns distinct admitted definitions.
    #[must_use]
    pub const fn visited_nodes(&self) -> u64 {
        self.visited_nodes
    }

    /// Returns examined allowed relationships.
    #[must_use]
    pub const fn visited_edges(&self) -> u64 {
        self.visited_edges
    }

    /// Returns the deepest fully processed frontier.
    #[must_use]
    pub const fn maximum_completed_depth(&self) -> u32 {
        self.maximum_completed_depth
    }

    /// Returns independent traversal truncation flags.
    #[must_use]
    pub const fn truncation(&self) -> RustGraphTraceTruncation {
        self.truncation
    }

    /// Returns generation-level graph limitations.
    #[must_use]
    pub const fn coverage(&self) -> RustGraphTraceCoverage {
        self.coverage
    }

    /// Returns conservative admitted input bytes.
    #[must_use]
    pub const fn input_bytes(&self) -> u64 {
        self.input_bytes
    }

    /// Returns conservative encoded output bytes.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
}

/// One impacted declaration and its strongest supported path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphImpact {
    class: RustGraphImpactClass,
    definition: RustGraphDefinitionIdentity,
    minimum_depth: u32,
}

impl RustGraphImpact {
    pub(super) const fn new(
        class: RustGraphImpactClass,
        definition: RustGraphDefinitionIdentity,
        minimum_depth: u32,
    ) -> Self {
        Self {
            class,
            definition,
            minimum_depth,
        }
    }

    /// Returns conservative impact strength.
    #[must_use]
    pub const fn class(&self) -> RustGraphImpactClass {
        self.class
    }

    /// Returns the exact impacted declaration.
    #[must_use]
    pub const fn definition(&self) -> &RustGraphDefinitionIdentity {
        &self.definition
    }

    /// Returns the shortest observed inbound path.
    #[must_use]
    pub const fn minimum_depth(&self) -> u32 {
        self.minimum_depth
    }
}

/// Complete conservative inbound-impact request.
#[derive(Clone, Debug)]
pub struct RustGraphImpactRequest<'a> {
    edges: &'a [RustGraphTraversalEdge],
    start: RustGraphDefinitionIdentity,
    edge_kinds: RustGraphEdgeKinds,
    limits: RustGraphTraceLimits,
    coverage: RustGraphTraceCoverage,
    control: RustGraphTraceControl<'a>,
}

impl<'a> RustGraphImpactRequest<'a> {
    /// Constructs one impact request pinned by its caller to one generation.
    #[must_use]
    pub const fn new(
        edges: &'a [RustGraphTraversalEdge],
        start: RustGraphDefinitionIdentity,
        edge_kinds: RustGraphEdgeKinds,
        limits: RustGraphTraceLimits,
        coverage: RustGraphTraceCoverage,
        control: RustGraphTraceControl<'a>,
    ) -> Self {
        Self {
            edges,
            start,
            edge_kinds,
            limits,
            coverage,
            control,
        }
    }

    pub(super) const fn start(&self) -> &RustGraphDefinitionIdentity {
        &self.start
    }

    pub(super) const fn limits(&self) -> RustGraphTraceLimits {
        self.limits
    }

    pub(super) fn trace_request(&self) -> RustGraphTraceRequest<'a> {
        RustGraphTraceRequest::new(
            self.edges,
            RustGraphTraceStart::Definition(self.start.clone()),
            RustGraphTraceDirection::Inbound,
            self.edge_kinds,
            self.limits,
            self.coverage,
            self.control,
        )
    }
}

/// Complete conservative inbound impact projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphImpactResult {
    trace: RustGraphTraceResult,
    impacted: Box<[RustGraphImpact]>,
    unknown_coverage: bool,
    output_bytes: u64,
}

impl RustGraphImpactResult {
    pub(super) const fn new(
        trace: RustGraphTraceResult,
        impacted: Box<[RustGraphImpact]>,
        unknown_coverage: bool,
        output_bytes: u64,
    ) -> Self {
        Self {
            trace,
            impacted,
            unknown_coverage,
            output_bytes,
        }
    }

    /// Returns the exact inbound trace.
    #[must_use]
    pub const fn trace(&self) -> &RustGraphTraceResult {
        &self.trace
    }

    /// Returns deterministic impacted declarations.
    #[must_use]
    pub const fn impacted(&self) -> &[RustGraphImpact] {
        &self.impacted
    }

    /// Reports whether unsupported or truncated work remains.
    #[must_use]
    pub const fn unknown_coverage(&self) -> bool {
        self.unknown_coverage
    }

    /// Returns conservative encoded output bytes including the trace.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
}
