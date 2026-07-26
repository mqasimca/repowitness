//! Deterministic content-to-facts analysis, resolution, correspondence,
//! retrieval, and context selection.
//!
//! Analysis consumes immutable content and snapshot inputs and performs no
//! filesystem or database I/O.

mod artifact_reuse;
mod reconciliation;
mod rust_source;

pub use artifact_reuse::{
    ArtifactKeySemantics, ArtifactPlanAction, ArtifactPlanCount, ArtifactPlanningError,
    ArtifactReusePlan, PlannedAnalysisArtifact, plan_artifact_reuse,
};
pub use reconciliation::{
    DEFAULT_RECONCILIATION_CHANGES, DEFAULT_RECONCILIATION_HINTS, MAX_RECONCILIATION_CHANGES,
    MAX_RECONCILIATION_HINTS, ManifestChange, ManifestChangeKind, ManifestReconciliation,
    ManifestReconciliationError, ManifestReconciliationLimits, ReconciliationCount,
    reconcile_source_manifests,
};
pub use rust_source::{
    RUST_ANALYSIS_PROFILE_VERSION, RustAnalysisControl, RustAnalysisError, RustAnalysisLimits,
    RustSourceAnalysis, RustSourceAnalyzer, RustSymbolFact, RustSymbolKind,
    TREE_SITTER_RUNTIME_VERSION, TREE_SITTER_RUST_GRAMMAR_VERSION,
    rust_analyzer_implementation_fingerprint_input, rust_grammar_fingerprint_input,
};
