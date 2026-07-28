//! Deterministic content-to-facts analysis, resolution, correspondence,
//! retrieval, and context selection.
//!
//! Analysis consumes immutable content and snapshot inputs and performs no
//! filesystem or database I/O.

mod artifact_reuse;
mod go_source;
mod python_source;
mod reconciliation;
mod rust_correspondence;
mod rust_source;
mod typescript_source;

pub use artifact_reuse::{
    ArtifactKeySemantics, ArtifactPlanAction, ArtifactPlanCount, ArtifactPlanningError,
    ArtifactReusePlan, PlannedAnalysisArtifact, plan_artifact_reuse,
};
pub use go_source::{
    GO_ANALYSIS_PROFILE_VERSION, GoSourceAnalyzer, TREE_SITTER_GO_GRAMMAR_VERSION,
    go_analyzer_implementation_fingerprint_input, go_grammar_fingerprint_input,
};
pub use python_source::{
    PYTHON_ANALYSIS_PROFILE_VERSION, PythonSourceAnalyzer, TREE_SITTER_PYTHON_GRAMMAR_VERSION,
    python_analyzer_implementation_fingerprint_input, python_grammar_fingerprint_input,
};
pub use reconciliation::{
    DEFAULT_RECONCILIATION_CHANGES, DEFAULT_RECONCILIATION_HINTS, MAX_RECONCILIATION_CHANGES,
    MAX_RECONCILIATION_HINTS, ManifestChange, ManifestChangeKind, ManifestReconciliation,
    ManifestReconciliationError, ManifestReconciliationLimits, ReconciliationCount,
    reconcile_source_manifests,
};
pub use rust_correspondence::{
    MAX_RUST_CORRESPONDENCE_CANDIDATES, RUST_CORRESPONDENCE_PROFILE_ID,
    RUST_CORRESPONDENCE_PROFILE_VERSION, RustAutomaticCorrespondence, RustCorrespondenceCandidate,
    RustCorrespondenceError, RustCorrespondenceIndeterminateReason, RustCorrespondenceResolution,
    RustCorrespondenceSubject, RustOccurrenceFingerprint, RustPathContinuity,
    fingerprint_rust_occurrence, resolve_rust_correspondence,
    rust_correspondence_implementation_fingerprint_input,
};
pub use rust_source::{
    RUST_ANALYSIS_PROFILE_VERSION, RustAnalysisControl, RustAnalysisError, RustAnalysisLimits,
    RustSourceAnalysis, RustSourceAnalyzer, RustSymbolFact, RustSymbolKind, SourceAnalysis,
    SourceAnalysisControl, SourceAnalysisError, SourceAnalysisLimits, SymbolFact, SymbolKind,
    TREE_SITTER_RUNTIME_VERSION, TREE_SITTER_RUST_GRAMMAR_VERSION,
    rust_analyzer_implementation_fingerprint_input, rust_analyzer_traversal_fingerprint_input,
    rust_grammar_fingerprint_input,
};
pub use typescript_source::{
    TREE_SITTER_TYPESCRIPT_GRAMMAR_VERSION, TYPESCRIPT_ANALYSIS_PROFILE_VERSION, TypeScriptDialect,
    TypeScriptSourceAnalyzer, typescript_analyzer_implementation_fingerprint_input,
    typescript_grammar_fingerprint_input,
};

#[cfg(test)]
mod adversarial_tests;
