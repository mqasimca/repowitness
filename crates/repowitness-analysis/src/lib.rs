//! Deterministic content-to-facts analysis, resolution, correspondence,
//! retrieval, and context selection.
//!
//! Analysis consumes immutable content and snapshot inputs and performs no
//! filesystem or database I/O.

mod artifact_reuse;
mod go_source;
mod phase2_context;
mod python_source;
mod reconciliation;
mod rust_correspondence;
mod rust_graph;
mod rust_source;
mod scip_overlay;
mod scip_wire;
mod typescript_source;

pub use artifact_reuse::{
    ArtifactKeySemantics, ArtifactPlanAction, ArtifactPlanCount, ArtifactPlanningError,
    ArtifactReusePlan, PlannedAnalysisArtifact, plan_artifact_reuse,
};
pub use go_source::{
    GO_ANALYSIS_PROFILE_VERSION, GoSourceAnalyzer, TREE_SITTER_GO_GRAMMAR_VERSION,
    go_analyzer_implementation_fingerprint_input, go_grammar_fingerprint_input,
};
pub use phase2_context::{
    MAX_PHASE2_CONTEXT_BUDGET_UNITS, MAX_PHASE2_CONTEXT_CANDIDATES, Phase2ContextBudget,
    Phase2ContextCandidate, Phase2ContextError, Phase2ContextInput, Phase2ContextOmission,
    Phase2ContextResult, compile_phase2_context,
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
pub use repowitness_domain::{
    PHASE2_EVIDENCE_BALANCED_PROFILE_ID, PHASE2_EVIDENCE_BALANCED_PROFILE_VERSION,
    Phase2ContextCandidateId, Phase2ContextProfile, Phase2ContextProviderAttribution,
    Phase2ContextProviderAvailability, Phase2ContextProviderCoverage,
    Phase2ContextProviderCoverageError, Phase2ContextProviderId, Phase2ContextScope,
    Phase2ContextScopeError, Phase2ContextTier,
};
pub use rust_correspondence::{
    MAX_RUST_CORRESPONDENCE_CANDIDATES, RUST_CORRESPONDENCE_PROFILE_ID,
    RUST_CORRESPONDENCE_PROFILE_VERSION, RustAutomaticCorrespondence, RustCorrespondenceCandidate,
    RustCorrespondenceError, RustCorrespondenceIndeterminateReason, RustCorrespondenceResolution,
    RustCorrespondenceSubject, RustOccurrenceFingerprint, RustPathContinuity,
    fingerprint_rust_occurrence, resolve_rust_correspondence,
    rust_correspondence_implementation_fingerprint_input,
};
pub use rust_graph::{
    RUST_GRAPH_RESOLVER_PROFILE_VERSION, RUST_GRAPH_SITE_PROFILE_VERSION,
    RUST_GRAPH_TRAVERSAL_PROFILE_VERSION, RustGraphAnalysisControl, RustGraphAnalysisError,
    RustGraphAnalysisLimits, RustGraphDefinitionIdentity, RustGraphDefinitionOccurrence,
    RustGraphEdgeKind, RustGraphEdgeKinds, RustGraphEnclosingDefinition, RustGraphImpact,
    RustGraphImpactClass, RustGraphImpactRequest, RustGraphImpactResult,
    RustGraphRelationshipCardinality, RustGraphResolution, RustGraphResolutionCandidate,
    RustGraphResolutionControl, RustGraphResolutionCoverage, RustGraphResolutionError,
    RustGraphResolutionEvidence, RustGraphResolutionLimits, RustGraphResolutionOutcome,
    RustGraphSite, RustGraphSiteAnalysis, RustGraphSiteAnalyzer, RustGraphSiteEvidence,
    RustGraphSiteIdentity, RustGraphSiteKind, RustGraphSiteOccurrence, RustGraphSiteOrdinal,
    RustGraphSiteResolution, RustGraphTraceControl, RustGraphTraceCoverage,
    RustGraphTraceDirection, RustGraphTraceEdge, RustGraphTraceError, RustGraphTraceLimits,
    RustGraphTraceRequest, RustGraphTraceResult, RustGraphTraceStart, RustGraphTraceTruncation,
    RustGraphTraversalEdge, RustGraphUnresolvedReason, analyze_rust_graph_impact,
    resolve_rust_graph_sites, rust_graph_site_extraction_fingerprint_input,
    rust_graph_site_implementation_fingerprint_input, rust_graph_site_traversal_fingerprint_input,
    trace_rust_graph,
};
pub use rust_source::{
    RUST_ANALYSIS_PROFILE_VERSION, RustAnalysisControl, RustAnalysisError, RustAnalysisLimits,
    RustSourceAnalysis, RustSourceAnalyzer, RustSymbolFact, RustSymbolKind, SourceAnalysis,
    SourceAnalysisControl, SourceAnalysisError, SourceAnalysisLimits, SymbolFact, SymbolKind,
    TREE_SITTER_RUNTIME_VERSION, TREE_SITTER_RUST_GRAMMAR_VERSION,
    rust_analyzer_implementation_fingerprint_input, rust_analyzer_traversal_fingerprint_input,
    rust_grammar_fingerprint_input,
};
pub use scip_overlay::{
    SCIP_OVERLAY_IMPORTER_VERSION, SCIP_SCHEMA_REVISION, SCIP_SCHEMA_SHA256,
    ScipImmutableSourceLookup, ScipOverlayDocument, ScipOverlayDocumentError,
    ScipOverlayIndexSummary, ScipSourceTextEncoding, decode_scip_overlay_document,
    decode_scip_overlay_index,
};
pub use typescript_source::{
    TREE_SITTER_TYPESCRIPT_GRAMMAR_VERSION, TYPESCRIPT_ANALYSIS_PROFILE_VERSION, TypeScriptDialect,
    TypeScriptSourceAnalyzer, typescript_analyzer_implementation_fingerprint_input,
    typescript_grammar_fingerprint_input,
};

#[cfg(test)]
mod adversarial_tests;
