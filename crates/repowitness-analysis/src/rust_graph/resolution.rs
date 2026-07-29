//! Pure, bounded generation-local resolution for validated Rust graph sites.

mod model;
mod outcome;
mod resolver;
mod support;

pub use model::{
    RUST_GRAPH_RESOLVER_PROFILE_VERSION, RustGraphDefinitionIdentity,
    RustGraphDefinitionOccurrence, RustGraphResolutionControl, RustGraphResolutionError,
    RustGraphResolutionLimits, RustGraphSiteIdentity, RustGraphSiteOccurrence,
};
pub use outcome::{
    RustGraphResolution, RustGraphResolutionCandidate, RustGraphResolutionCoverage,
    RustGraphResolutionEvidence, RustGraphResolutionOutcome, RustGraphSiteResolution,
    RustGraphUnresolvedReason,
};
pub use resolver::resolve_rust_graph_sites;

#[cfg(test)]
mod tests;
