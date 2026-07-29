//! Deterministic bounded trace and conservative impact over one Rust graph.

mod engine;
mod impact;
mod model;
mod request;
mod result;

pub use engine::trace_rust_graph;
pub use impact::analyze_rust_graph_impact;
pub use model::{
    RUST_GRAPH_TRAVERSAL_PROFILE_VERSION, RustGraphEdgeKind, RustGraphEdgeKinds,
    RustGraphImpactClass, RustGraphRelationshipCardinality, RustGraphTraceDirection,
    RustGraphTraceError, RustGraphTraceStart, RustGraphTraversalEdge,
};
pub use request::{
    RustGraphTraceControl, RustGraphTraceCoverage, RustGraphTraceLimits, RustGraphTraceRequest,
};
pub use result::{
    RustGraphImpact, RustGraphImpactRequest, RustGraphImpactResult, RustGraphTraceEdge,
    RustGraphTraceResult, RustGraphTraceTruncation,
};

#[cfg(test)]
mod tests;
