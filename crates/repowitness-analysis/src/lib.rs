//! Deterministic content-to-facts analysis, resolution, correspondence,
//! retrieval, and context selection.
//!
//! Analysis consumes immutable content and snapshot inputs and performs no
//! filesystem or database I/O.

mod artifact_reuse;
mod evidence_context;
mod go_source;
mod python_source;
mod raw_syntax;
mod reconciliation;
mod rust_correspondence;
mod rust_graph;
mod rust_source;
mod scip_overlay;
mod scip_wire;
mod typescript_source;

/// Maximum admitted bytes in one bounded SCIP overlay input.
///
/// This is the outer-wire framing limit and must also bound local file
/// admission before any SCIP decoder retains input bytes.
pub const MAX_SCIP_OVERLAY_INPUT_BYTES: usize = 64 * 1024 * 1024;

pub use artifact_reuse::{
    ArtifactKeySemantics, ArtifactPlanAction, ArtifactPlanCount, ArtifactPlanningError,
    ArtifactReusePlan, PlannedAnalysisArtifact, plan_artifact_reuse,
};
pub use evidence_context::{
    DEFAULT_EVIDENCE_CONTEXT_BUDGET_UNITS, EvidenceContextBudget, EvidenceContextCandidate,
    EvidenceContextError, EvidenceContextInput, EvidenceContextOmission, EvidenceContextResult,
    MAX_EVIDENCE_CONTEXT_BUDGET_UNITS, MAX_EVIDENCE_CONTEXT_CANDIDATES, compile_evidence_context,
};
pub use go_source::{
    GO_ANALYSIS_PROFILE_VERSION, GoSourceAnalyzer, TREE_SITTER_GO_GRAMMAR_VERSION,
    go_analyzer_implementation_fingerprint_input, go_grammar_fingerprint_input,
};
pub use python_source::{
    PYTHON_ANALYSIS_PROFILE_VERSION, PythonSourceAnalyzer, TREE_SITTER_PYTHON_GRAMMAR_VERSION,
    python_analyzer_implementation_fingerprint_input, python_grammar_fingerprint_input,
};
pub use raw_syntax::{
    RAW_SYNTAX_SITE_PROFILE_VERSION, RawSyntaxLanguage, RawSyntaxSite, RawSyntaxSiteAnalysis,
    RawSyntaxSiteAnalysisControl, RawSyntaxSiteAnalysisError, RawSyntaxSiteAnalysisLimits,
    RawSyntaxSiteAnalyzer, RawSyntaxSiteCoverage, RawSyntaxSiteEvidence, RawSyntaxSiteKind,
    RawSyntaxSiteKindCoverage, RawSyntaxSiteOrdinal, RawSyntaxSiteSupport,
    raw_syntax_grammar_fingerprint_input, raw_syntax_site_implementation_fingerprint_input,
};
pub use reconciliation::{
    DEFAULT_RECONCILIATION_CHANGES, DEFAULT_RECONCILIATION_HINTS, MAX_RECONCILIATION_CHANGES,
    MAX_RECONCILIATION_HINTS, ManifestChange, ManifestChangeKind, ManifestReconciliation,
    ManifestReconciliationError, ManifestReconciliationLimits, ReconciliationCount,
    reconcile_source_manifests,
};
pub use repowitness_domain::{
    EVIDENCE_BALANCED_PROFILE_ID, EVIDENCE_BALANCED_PROFILE_VERSION, EvidenceContextCandidateId,
    EvidenceContextProfile, EvidenceContextProviderAttribution,
    EvidenceContextProviderAvailability, EvidenceContextProviderCoverage,
    EvidenceContextProviderCoverageError, EvidenceContextProviderId, EvidenceContextScope,
    EvidenceContextScopeError, EvidenceContextTier,
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
