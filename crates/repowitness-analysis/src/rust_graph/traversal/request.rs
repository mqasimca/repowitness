use std::{
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use super::model::{
    RustGraphEdgeKinds, RustGraphTraceDirection, RustGraphTraceError, RustGraphTraceStart,
    RustGraphTraversalEdge,
};

const MAX_INPUT_EDGES: u64 = 4_000_000;
const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DEPTH: u32 = 256;
const MAX_RESULTS: u32 = 100_000;
const MAX_VISITED_NODES: u64 = 1_000_000;
const MAX_VISITED_EDGES: u64 = 4_000_000;
const MAX_FRONTIER: u64 = 1_000_000;
const MAX_OUTPUT_BYTES: u64 = 256 * 1024 * 1024;

/// Caller and policy bounds for one all-or-nothing traversal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RustGraphTraceLimits {
    max_input_edges: u64,
    max_input_bytes: u64,
    max_depth: u32,
    max_results: u32,
    max_visited_nodes: u64,
    max_visited_edges: u64,
    max_frontier: u64,
    max_output_bytes: u64,
}

impl RustGraphTraceLimits {
    /// Conservative default bounds below every compiled ceiling.
    pub const DEFAULT: Self = Self {
        max_input_edges: 500_000,
        max_input_bytes: 256 * 1024 * 1024,
        max_depth: 16,
        max_results: 10_000,
        max_visited_nodes: 100_000,
        max_visited_edges: 500_000,
        max_frontier: 100_000,
        max_output_bytes: 64 * 1024 * 1024,
    };

    /// Constructs positive limits no larger than compiled hard ceilings.
    #[allow(
        clippy::too_many_arguments,
        reason = "each independent traversal resource has an explicit bound"
    )]
    pub const fn try_new(
        max_input_edges: u64,
        max_input_bytes: u64,
        max_depth: u32,
        max_results: u32,
        max_visited_nodes: u64,
        max_visited_edges: u64,
        max_frontier: u64,
        max_output_bytes: u64,
    ) -> Result<Self, RustGraphTraceError> {
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
            Err(RustGraphTraceError::InvalidLimits)
        }
    }

    const fn is_valid(self) -> bool {
        self.max_input_edges != 0
            && self.max_input_edges <= MAX_INPUT_EDGES
            && self.max_input_bytes != 0
            && self.max_input_bytes <= MAX_INPUT_BYTES
            && self.max_depth != 0
            && self.max_depth <= MAX_DEPTH
            && self.max_results != 0
            && self.max_results <= MAX_RESULTS
            && self.max_visited_nodes != 0
            && self.max_visited_nodes <= MAX_VISITED_NODES
            && self.max_visited_edges != 0
            && self.max_visited_edges <= MAX_VISITED_EDGES
            && self.max_frontier != 0
            && self.max_frontier <= MAX_FRONTIER
            && self.max_output_bytes != 0
            && self.max_output_bytes <= MAX_OUTPUT_BYTES
    }

    /// Returns the admitted relationship count.
    #[must_use]
    pub const fn max_input_edges(self) -> u64 {
        self.max_input_edges
    }

    /// Returns aggregate encoded input bytes.
    #[must_use]
    pub const fn max_input_bytes(self) -> u64 {
        self.max_input_bytes
    }

    /// Returns maximum traversal depth.
    #[must_use]
    pub const fn max_depth(self) -> u32 {
        self.max_depth
    }

    /// Returns maximum emitted relationship count.
    #[must_use]
    pub const fn max_results(self) -> u32 {
        self.max_results
    }

    /// Returns maximum distinct definition count.
    #[must_use]
    pub const fn max_visited_nodes(self) -> u64 {
        self.max_visited_nodes
    }

    /// Returns maximum examined relationship count.
    #[must_use]
    pub const fn max_visited_edges(self) -> u64 {
        self.max_visited_edges
    }

    /// Returns maximum pending definition count.
    #[must_use]
    pub const fn max_frontier(self) -> u64 {
        self.max_frontier
    }

    /// Returns maximum conservatively encoded output bytes.
    #[must_use]
    pub const fn max_output_bytes(self) -> u64 {
        self.max_output_bytes
    }
}

impl Default for RustGraphTraceLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Cooperative cancellation and absolute monotonic deadline.
#[derive(Clone, Copy)]
pub struct RustGraphTraceControl<'a> {
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

impl<'a> RustGraphTraceControl<'a> {
    /// Constructs one traversal control.
    #[must_use]
    pub const fn new(cancelled: &'a AtomicBool, deadline: Instant) -> Self {
        Self {
            cancelled,
            deadline,
        }
    }

    pub(super) fn outcome(self) -> Result<(), RustGraphTraceError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(RustGraphTraceError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(RustGraphTraceError::DeadlineExceeded)
        } else {
            Ok(())
        }
    }
}

impl fmt::Debug for RustGraphTraceControl<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphTraceControl")
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Generation-level coverage that relationship rows cannot represent alone.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RustGraphTraceCoverage {
    unresolved_sites: u64,
    unsupported_sites: u64,
    ambiguous_sites: u64,
    truncated_sites: u64,
    unlinked_sites: u64,
    macro_sites: u64,
    conditional_sites: u64,
    heuristic_sites: u64,
}

impl RustGraphTraceCoverage {
    /// Constructs explicit generation-level graph limitations.
    #[allow(
        clippy::too_many_arguments,
        reason = "all fixed coverage categories remain independently inspectable"
    )]
    #[must_use]
    pub const fn new(
        unresolved_sites: u64,
        unsupported_sites: u64,
        ambiguous_sites: u64,
        truncated_sites: u64,
        unlinked_sites: u64,
        macro_sites: u64,
        conditional_sites: u64,
        heuristic_sites: u64,
    ) -> Self {
        Self {
            unresolved_sites,
            unsupported_sites,
            ambiguous_sites,
            truncated_sites,
            unlinked_sites,
            macro_sites,
            conditional_sites,
            heuristic_sites,
        }
    }

    /// Reports whether unsupported or incomplete work limits impact certainty.
    #[must_use]
    pub const fn has_unknown_impact(self) -> bool {
        self.unresolved_sites != 0
            || self.unsupported_sites != 0
            || self.truncated_sites != 0
            || self.unlinked_sites != 0
            || self.macro_sites != 0
            || self.conditional_sites != 0
            || self.heuristic_sites != 0
    }

    /// Returns unresolved site count.
    #[must_use]
    pub const fn unresolved_sites(self) -> u64 {
        self.unresolved_sites
    }

    /// Returns unsupported site count.
    #[must_use]
    pub const fn unsupported_sites(self) -> u64 {
        self.unsupported_sites
    }

    /// Returns ambiguous site count.
    #[must_use]
    pub const fn ambiguous_sites(self) -> u64 {
        self.ambiguous_sites
    }

    /// Returns candidate-truncated site count.
    #[must_use]
    pub const fn truncated_sites(self) -> u64 {
        self.truncated_sites
    }

    /// Returns sites that could not be joined to an enclosing declaration.
    #[must_use]
    pub const fn unlinked_sites(self) -> u64 {
        self.unlinked_sites
    }

    /// Returns macro-call site count.
    #[must_use]
    pub const fn macro_sites(self) -> u64 {
        self.macro_sites
    }

    /// Returns conditional-source marker count.
    #[must_use]
    pub const fn conditional_sites(self) -> u64 {
        self.conditional_sites
    }

    /// Returns syntax-heuristic site count.
    #[must_use]
    pub const fn heuristic_sites(self) -> u64 {
        self.heuristic_sites
    }
}

/// Complete request for one immutable relationship slice.
#[derive(Clone, Debug)]
pub struct RustGraphTraceRequest<'a> {
    edges: &'a [RustGraphTraversalEdge],
    start: RustGraphTraceStart,
    direction: RustGraphTraceDirection,
    edge_kinds: RustGraphEdgeKinds,
    limits: RustGraphTraceLimits,
    coverage: RustGraphTraceCoverage,
    control: RustGraphTraceControl<'a>,
}

impl<'a> RustGraphTraceRequest<'a> {
    /// Constructs one explicit trace request.
    #[must_use]
    pub const fn new(
        edges: &'a [RustGraphTraversalEdge],
        start: RustGraphTraceStart,
        direction: RustGraphTraceDirection,
        edge_kinds: RustGraphEdgeKinds,
        limits: RustGraphTraceLimits,
        coverage: RustGraphTraceCoverage,
        control: RustGraphTraceControl<'a>,
    ) -> Self {
        Self {
            edges,
            start,
            direction,
            edge_kinds,
            limits,
            coverage,
            control,
        }
    }

    pub(super) const fn edges(&self) -> &'a [RustGraphTraversalEdge] {
        self.edges
    }

    pub(super) const fn start(&self) -> &RustGraphTraceStart {
        &self.start
    }

    pub(super) const fn direction(&self) -> RustGraphTraceDirection {
        self.direction
    }

    pub(super) const fn edge_kinds(&self) -> RustGraphEdgeKinds {
        self.edge_kinds
    }

    pub(super) const fn limits(&self) -> RustGraphTraceLimits {
        self.limits
    }

    pub(super) const fn coverage(&self) -> RustGraphTraceCoverage {
        self.coverage
    }

    pub(super) const fn control(&self) -> RustGraphTraceControl<'a> {
        self.control
    }
}
