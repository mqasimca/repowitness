//! Storage-neutral requests for generation-pinned Rust graph reads.

mod model;
mod use_case;

pub use model::{
    MAX_RUST_GRAPH_QUERY_BYTES, RustGraphDefinitionSelector, RustGraphReadOperation,
    RustGraphReadSelection, RustGraphSelectorError, RustGraphSiteSelector, RustGraphSymbolQuery,
    RustGraphSymbolQueryError, RustGraphTraceStartSelector,
};
pub use use_case::{
    RustGraphReadError, RustGraphReadPort, RustGraphReadPortResult, RustGraphReadRequest,
    RustGraphReadResult, RustGraphReadSelectionError, rust_graph_read,
};

pub use repowitness_analysis::{
    RustGraphEdgeKinds, RustGraphSiteEvidence, RustGraphSiteKind, RustGraphTraceDirection,
    RustGraphTraceLimits, RustSymbolKind,
};
pub use repowitness_domain::{
    AnalysisArtifactDigest, ByteOffset, ByteSpan, RepositoryPath, SourceContentDigest, SourceSlotId,
};

#[cfg(test)]
mod tests;
