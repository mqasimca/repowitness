//! Application use cases, request context, policy enforcement, task
//! supervision, and narrow I/O ports.
//!
//! CLI and MCP adapters call the same use cases through this package.

mod architecture_map;
mod architecture_overview;
mod canonical_digest;
mod change_receipt;
mod code_graph_query;
mod code_search;
mod configuration;
mod context_build;
mod go_profile;
mod index_publication;
mod memory_import;
mod memory_projection;
mod memory_recall;
mod memory_record_id_text;
mod outbound_sites;
mod package_scope;
mod personal_memory;
mod python_profile;
mod raw_syntax_profile;
mod relevant_paths;
mod repository_diagnostics;
mod repository_identity_text;
mod repository_path_text;
mod repository_topology;
mod rust_graph_profile;
mod rust_graph_read;
mod rust_index;
mod rust_profile;
mod scip_evidence_read;
mod scip_overlay_identity;
mod scip_overlay_import;
mod scip_relationship_trace;
mod scip_symbol_resolve;
mod source_profile;
mod source_slot_publication;
mod source_snapshot;
mod symbol_get;
mod symbol_search;
mod syntax_site_search;
mod task_checkpoint;
mod test_markers;
mod typescript_profile;
mod workspace_identity_text;

pub use architecture_map::{
    ARCHITECTURE_MAP_PROFILE_VERSION, ArchitectureMapError, ArchitectureMapFile,
    ArchitectureMapLanguageSummary, ArchitectureMapLimitError, ArchitectureMapLimits,
    ArchitectureMapPort, ArchitectureMapPortOutputError, ArchitectureMapPortResult,
    ArchitectureMapRequest, ArchitectureMapResult, DEFAULT_ARCHITECTURE_MAP_FILES,
    DEFAULT_ARCHITECTURE_MAP_OUTPUT_BYTES, MAX_ARCHITECTURE_MAP_FILES,
    MAX_ARCHITECTURE_MAP_OUTPUT_BYTES, architecture_map,
};
pub use architecture_overview::{
    ARCHITECTURE_OVERVIEW_PROFILE_VERSION, ArchitectureOverviewEntryPointCandidate,
    ArchitectureOverviewError, ArchitectureOverviewKindSummary, ArchitectureOverviewLimitError,
    ArchitectureOverviewLimits, ArchitectureOverviewPort, ArchitectureOverviewPortOutputError,
    ArchitectureOverviewPortResult, ArchitectureOverviewRequest, ArchitectureOverviewResult,
    ArchitectureOverviewSourceRoot, ArchitectureOverviewSourceRootSummary,
    DEFAULT_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES, DEFAULT_ARCHITECTURE_OVERVIEW_FILES,
    DEFAULT_ARCHITECTURE_OVERVIEW_OUTPUT_BYTES, DEFAULT_ARCHITECTURE_OVERVIEW_ROOTS,
    MAX_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES, MAX_ARCHITECTURE_OVERVIEW_FILES,
    MAX_ARCHITECTURE_OVERVIEW_OUTPUT_BYTES, MAX_ARCHITECTURE_OVERVIEW_ROOTS, architecture_overview,
};
pub use canonical_digest::{
    ANALYSIS_ARTIFACT_PAYLOAD_VERSION, CanonicalAnalysisArtifactKey, CanonicalSourceManifest,
    hash_analysis_artifact_key, hash_analysis_artifact_payload, hash_source_content,
    hash_source_manifest,
};
pub use change_receipt::{
    CHANGE_MANIFEST_PROFILE_VERSION, ChangeKind, ChangeManifest, ChangeManifestEntry,
    ChangeManifestError, ChangeReviewReceipt, IndexedContext, IndexedContextUnavailableReason,
};
pub use code_graph_query::{
    CODE_GRAPH_QUERY_PROFILE_VERSION, CodeGraphQueryError, CodeGraphQueryOperation,
    CodeGraphQueryRequest, CodeGraphQueryResult, code_graph_query,
};
pub use code_search::{
    CODE_SEARCH_PROFILE_VERSION, CodeSearchCandidate, CodeSearchClaim, CodeSearchError,
    CodeSearchEvidenceIdentity, CodeSearchLimitError, CodeSearchLimits, CodeSearchNotice,
    CodeSearchPort, CodeSearchPortOutputError, CodeSearchPortResult, CodeSearchProducer,
    CodeSearchProducerIdentity, CodeSearchQuery, CodeSearchQueryDigest, CodeSearchQueryError,
    CodeSearchRequest, CodeSearchResult, DEFAULT_CODE_SEARCH_OUTPUT_BYTES,
    DEFAULT_CODE_SEARCH_RESULTS, MAX_CODE_SEARCH_OUTPUT_BYTES, MAX_CODE_SEARCH_RESULTS,
    RustSymbolOccurrence, SourceArtifactEvidence, SourceSymbolOccurrence, code_search,
};
pub use configuration::{
    CONFIGURATION_DIGEST_VERSION, CONFIGURATION_RESOLVER_VERSION, CONFIGURATION_SCHEMA_VERSION,
    ConfigurationField, ConfigurationLayer, ConfigurationLayerKind, ConfigurationPolicyOverrides,
    ConfigurationPreferenceOverrides, ConfigurationProfile, ConfigurationResolutionError,
    ConfigurationValidationError, DEFAULT_CONFIGURATION_RETAINED_GENERATIONS_PER_SOURCE_SLOT,
    DEFAULT_CONFIGURATION_RETENTION_BYTES, DEFAULT_CONFIGURATION_RETENTION_GENERATION_CANDIDATES,
    DEFAULT_CONFIGURATION_RETENTION_ROWS, EffectiveConfigurationPolicy,
    EffectiveConfigurationPreferences, EffectiveRetentionConfiguration,
    MAX_CONFIGURATION_CONTEXT_BYTES, MAX_CONFIGURATION_FILE_LAYERS, MAX_CONFIGURATION_GRAPH_DEPTH,
    MAX_CONFIGURATION_GRAPH_RESULTS, MAX_CONFIGURATION_QUERY_RESULTS,
    MAX_CONFIGURATION_RETAINED_GENERATIONS_PER_SOURCE_SLOT, MAX_CONFIGURATION_RETENTION_BYTES,
    MAX_CONFIGURATION_RETENTION_GENERATION_CANDIDATES, MAX_CONFIGURATION_RETENTION_ROWS,
    MAX_CONFIGURATION_SOURCE_FILE_BYTES, MAX_CONFIGURATION_SOURCE_FILES,
    MAX_CONFIGURATION_WATCHER_POLL_INTERVAL_MS, MIN_CONFIGURATION_WATCHER_POLL_INTERVAL_MS,
    McpToolProfile, PolicyValue, ResolvedConfiguration, ResolvedPreference,
    ResolvedToolProfilePreference, RetentionConfigurationOverrides, resolve_configuration,
};
pub use context_build::{
    CONTEXT_BUILD_PROFILE_VERSION, CONTEXT_BUILD_RRF_K, ContextBudgetEstimator, ContextBuildBudget,
    ContextBuildCoverage, ContextBuildError, ContextBuildResult, ContextItem, ContextMemoryItem,
    ContextMemoryProjection, ContextOmission, ContextProvider, ContextRank, ContextSourceCandidate,
    ContextSourceInput, ContextSourceItem, DEFAULT_CONTEXT_BUILD_BUDGET_UNITS,
    MAX_CONTEXT_BUILD_BUDGET_UNITS, compile_context,
};
pub use go_profile::{
    PHASE0_GO_ANALYSIS_SCHEMA_VERSION, PHASE0_GO_CANONICALIZATION_VERSION,
    PHASE0_GO_CONFIGURATION_VERSION, PHASE0_GO_PRODUCER_MANIFEST_VERSION,
    phase0_go_artifact_identity,
};
pub use index_publication::{
    PublishRustIndexError, PublishRustIndexRequest, PublishedRustIndex, RustIndexCoverage,
    RustIndexPublicationPort, publish_rust_index,
};
pub use memory_import::{
    ImportMemoryRecordError, ImportMemoryRecordRequest, MemoryImportApproval, MemoryImportReceipt,
    MemoryVersionImportPort, import_memory_record,
};
pub use memory_projection::{
    MAX_MEMORY_PROJECTION_VERSIONS, MemoryEffectiveState, MemoryEvidenceOutcome,
    MemoryHeadSelection, MemoryHeadState, MemoryProjectionDecision, MemoryProjectionError,
    MemoryProjectionEvidenceState, MemoryProjectionReason, MemoryProjectionValidityState,
    MemoryVersionHeadInput, evaluate_memory_projection, select_memory_head,
};
pub use memory_recall::{
    DEFAULT_MEMORY_RECALL_OUTPUT_BYTES, DEFAULT_MEMORY_RECALL_RESULTS,
    DEFAULT_MEMORY_RECALL_SCAN_BYTES, MAX_MEMORY_RECALL_OUTPUT_BYTES, MAX_MEMORY_RECALL_RESULTS,
    MAX_MEMORY_RECALL_SCAN_BYTES, MEMORY_RECALL_PROFILE_VERSION, MemoryRecallCandidate,
    MemoryRecallCandidateRelation, MemoryRecallError, MemoryRecallEvidence,
    MemoryRecallEvidenceAssurance, MemoryRecallEvidenceOutcome, MemoryRecallEvidenceState,
    MemoryRecallLimitError, MemoryRecallLimits, MemoryRecallOccurrence, MemoryRecallPort,
    MemoryRecallPortOutputError, MemoryRecallPortResult, MemoryRecallProducer,
    MemoryRecallProjectionCoverage, MemoryRecallQuery, MemoryRecallQueryDigest,
    MemoryRecallQueryError, MemoryRecallReason, MemoryRecallRecord, MemoryRecallRequest,
    MemoryRecallResult, MemoryRecallUseCaseResult, memory_recall,
};
pub use memory_record_id_text::{
    MEMORY_RECORD_ID_TEXT_BYTES, MemoryRecordIdTextError, MemoryRecordIdTextV1,
};
pub use outbound_sites::{
    DEFAULT_OUTBOUND_SITES_OUTPUT_BYTES, DEFAULT_OUTBOUND_SITES_RESULTS,
    MAX_OUTBOUND_SITES_OUTPUT_BYTES, MAX_OUTBOUND_SITES_RESULTS, OUTBOUND_SITES_PROFILE_VERSION,
    OutboundSitesAvailability, OutboundSitesDeclaration, OutboundSitesError,
    OutboundSitesLimitError, OutboundSitesLimits, OutboundSitesNotice, OutboundSitesPort,
    OutboundSitesPortOutputError, OutboundSitesPortRequest, OutboundSitesPortResult,
    OutboundSitesRequest, OutboundSitesResult, OutboundSyntaxSite, outbound_sites,
};
pub use package_scope::{
    MAX_PACKAGE_SCOPE_ROOTS, PACKAGE_SCOPE_VERSION, PackageRootCount, PackageRootOrdinal,
    PackageScope, PackageScopeDigest, PackageScopeError,
};
pub use personal_memory::{
    MAX_PERSONAL_MEMORY_READ_RESULTS, PersonalMemoryError, PersonalMemoryPort,
    PersonalMemoryReadPort, PersonalMemoryReceipt, append_personal_memory, read_personal_memory,
};
pub use python_profile::{
    PHASE0_PYTHON_ANALYSIS_SCHEMA_VERSION, PHASE0_PYTHON_CANONICALIZATION_VERSION,
    PHASE0_PYTHON_CONFIGURATION_VERSION, PHASE0_PYTHON_PRODUCER_MANIFEST_VERSION,
    phase0_python_artifact_identity,
};
pub use raw_syntax_profile::{
    RAW_SYNTAX_ANALYSIS_SCHEMA_VERSION, RAW_SYNTAX_CANONICALIZATION_VERSION,
    RAW_SYNTAX_CONFIGURATION_VERSION, RAW_SYNTAX_PRODUCER_MANIFEST_VERSION,
    raw_syntax_artifact_identities, raw_syntax_artifact_identity,
};
pub use relevant_paths::{
    DEFAULT_RELEVANT_PATHS, MAX_RELEVANT_PATHS, RELEVANT_PATHS_CANDIDATES_PER_PATH,
    RELEVANT_PATHS_PROFILE_VERSION, RelevantPath, RelevantPathsError, RelevantPathsLimitError,
    RelevantPathsLimits, RelevantPathsResult, locate_relevant_paths,
};
pub use repository_diagnostics::{
    REPOSITORY_DIAGNOSTICS_PROFILE_VERSION, RepositoryDiagnosticCapability,
    RepositoryDiagnosticLimitation, RepositoryDiagnosticsError,
    RepositoryDiagnosticsMemoryProjection, RepositoryDiagnosticsPort,
    RepositoryDiagnosticsPortOutputError, RepositoryDiagnosticsPortResult,
    RepositoryDiagnosticsRequest, RepositoryDiagnosticsResult, RepositoryDiagnosticsUseCaseResult,
    RepositoryParserDiagnostics, repository_diagnostics,
};
pub use repository_identity_text::{
    REPOSITORY_IDENTITY_TEXT_BYTES, RepositoryIdentityTextError, RepositoryIdentityTextV1,
};
pub use repository_path_text::{
    RepositoryPathLimits, RepositoryPathTextByteCount, RepositoryPathTextByteLimit,
    RepositoryPathTextError, RepositoryPathTextV1, RepositoryPathTextVersion,
};
pub use repository_topology::{
    DEFAULT_REPOSITORY_TOPOLOGY_OUTPUT_BYTES, DEFAULT_REPOSITORY_TOPOLOGY_PATHS,
    MAX_REPOSITORY_TOPOLOGY_OUTPUT_BYTES, MAX_REPOSITORY_TOPOLOGY_PATHS,
    REPOSITORY_TOPOLOGY_PROFILE_VERSION, RepositoryTopologyCategory,
    RepositoryTopologyCategorySummary, RepositoryTopologyCoverage, RepositoryTopologyEntry,
    RepositoryTopologyError, RepositoryTopologyLimitError, RepositoryTopologyLimits,
    RepositoryTopologyPort, RepositoryTopologyPortOutputError, RepositoryTopologyPortResult,
    RepositoryTopologyReceipt, RepositoryTopologyRequest, RepositoryTopologyResult,
    repository_topology,
};
pub use repowitness_analysis::{
    MAX_PHASE2_CONTEXT_BUDGET_UNITS, MAX_PHASE2_CONTEXT_CANDIDATES,
    PHASE2_EVIDENCE_BALANCED_PROFILE_ID, PHASE2_EVIDENCE_BALANCED_PROFILE_VERSION,
    Phase2ContextBudget, Phase2ContextCandidate, Phase2ContextCandidateId, Phase2ContextError,
    Phase2ContextInput, Phase2ContextOmission, Phase2ContextProfile,
    Phase2ContextProviderAttribution, Phase2ContextProviderId, Phase2ContextResult,
    Phase2ContextScope, Phase2ContextScopeError, Phase2ContextTier, compile_phase2_context,
};
pub use rust_graph_profile::{
    PHASE1_RUST_GRAPH_ANALYSIS_SCHEMA_VERSION, PHASE1_RUST_GRAPH_CANONICALIZATION_VERSION,
    PHASE1_RUST_GRAPH_CONFIGURATION_VERSION, PHASE1_RUST_GRAPH_PRODUCER_MANIFEST_VERSION,
    phase1_rust_graph_artifact_identity,
};
pub use rust_graph_read::{
    AnalysisArtifactDigest, ByteOffset, ByteSpan, MAX_RUST_GRAPH_QUERY_BYTES, RepositoryPath,
    RustGraphDefinitionSelector, RustGraphEdgeKinds, RustGraphReadError, RustGraphReadOperation,
    RustGraphReadPort, RustGraphReadPortResult, RustGraphReadRequest, RustGraphReadResult,
    RustGraphReadSelection, RustGraphReadSelectionError, RustGraphSelectorError,
    RustGraphSiteEvidence, RustGraphSiteKind, RustGraphSiteSelector, RustGraphSymbolQuery,
    RustGraphSymbolQueryError, RustGraphTraceDirection, RustGraphTraceLimits,
    RustGraphTraceStartSelector, RustSymbolKind, SourceContentDigest, SourceSlotId,
    rust_graph_read,
};
pub use rust_index::{
    DEFAULT_RUST_INDEX_FACTS, DEFAULT_RUST_INDEX_FILES, DEFAULT_RUST_INDEX_SOURCE_BYTES,
    ImmutableRustSource, MAX_RUST_INDEX_FACTS, MAX_RUST_INDEX_FILES, MAX_RUST_INDEX_SOURCE_BYTES,
    PreparedRustFile, PreparedRustIndex, RustArtifactIdentity, RustIndexLimitError,
    RustIndexLimits, RustIndexPreparationError, SourceArtifactIdentities, SourceLanguage,
    prepare_rust_index, prepare_rust_index_with_reuse, prepare_source_index,
    prepare_source_index_with_reuse,
};
pub use rust_profile::{
    PHASE0_RUST_ANALYSIS_SCHEMA_VERSION, PHASE0_RUST_CANONICALIZATION_VERSION,
    PHASE0_RUST_CONFIGURATION_VERSION, PHASE0_RUST_PRODUCER_MANIFEST_VERSION,
    phase0_rust_artifact_identity, phase0_rust_correspondence_profile_digest,
};
pub use scip_evidence_read::{
    ScipEvidenceReadError, ScipEvidenceReadPort, ScipEvidenceReadPortResult,
    ScipEvidenceReadRequest, ScipEvidenceReadResult, ScipEvidenceReadSelection,
    ScipEvidenceReadSelectionError, ScipSymbol, scip_evidence_read,
};
pub use scip_overlay_identity::{
    SCIP_OVERLAY_IDENTITY_VERSION, ScipOverlayIdentityInput, ScipOverlayScopeIdentity,
    ScipOverlayScopeIdentityError, bounded_scip_importer_digest, hash_scip_input,
    hash_scip_overlay_identity, reviewed_scip_schema_digest,
};
pub use scip_overlay_import::{
    PreparedScipOverlayImport, ScipOverlayImportError, ScipOverlayImportRequest,
    prepare_scip_overlay_import,
};
pub use scip_relationship_trace::{
    DEFAULT_SCIP_RELATIONSHIP_TRACE_DEPTH, DEFAULT_SCIP_RELATIONSHIP_TRACE_EDGES,
    MAX_SCIP_RELATIONSHIP_TRACE_DEPTH, MAX_SCIP_RELATIONSHIP_TRACE_EDGES,
    ScipRelationshipTraceDepth, ScipRelationshipTraceDepthError, ScipRelationshipTraceDirection,
    ScipRelationshipTraceError, ScipRelationshipTraceMaxEdges, ScipRelationshipTraceMaxEdgesError,
    ScipRelationshipTracePort, ScipRelationshipTracePortResult, ScipRelationshipTraceRequest,
    ScipRelationshipTraceResult, ScipRelationshipTraceSelectionError, scip_relationship_trace,
};
pub use scip_symbol_resolve::{
    ScipSymbolResolveError, ScipSymbolResolvePort, ScipSymbolResolvePortResult,
    ScipSymbolResolveRequest, ScipSymbolResolveResult, ScipSymbolResolveSelectionError,
    scip_symbol_resolve,
};
pub use source_profile::{
    PHASE0_SOURCE_CANONICALIZATION_VERSION, PHASE0_SOURCE_SNAPSHOT_PROFILE_VERSION,
    SourceSnapshotProfile, phase0_source_artifact_identities, phase0_source_snapshot_profile,
};
pub use source_slot_publication::{
    CompleteStagedSourceSlotIndexError, CompleteStagedSourceSlotIndexResult,
    CompletedSourceSlotIndex, MAX_SOURCE_SLOT_EPOCH, PublishSourceSlotIndexError,
    PublishSourceSlotIndexRequest, PublishSourceSlotIndexResult, SourceSlotEpoch,
    SourceSlotEpochError, SourceSlotFinalFence, SourceSlotPublicationPort,
    StageSourceSlotIndexRequest, StagedSourceSlotIndex, complete_staged_source_slot_index,
    publish_source_slot_index, stage_source_slot_index,
};
pub use source_snapshot::{
    GO_AND_RUST_SOURCE_SNAPSHOT_VERSION, RUST_SOURCE_SNAPSHOT_VERSION, RustSourceSnapshotIdentity,
    SUPPORTED_LANGUAGES_SOURCE_SNAPSHOT_VERSION, SourceSnapshotIdentity, hash_rust_source_snapshot,
    hash_source_snapshot,
};
pub use symbol_get::{
    MAX_SYMBOL_GET_DECLARATION_BYTES, MAX_SYMBOL_GET_OUTPUT_BYTES, RetrievedSymbol,
    SYMBOL_GET_PROFILE_VERSION, SymbolGetCandidate, SymbolGetClaim, SymbolGetError,
    SymbolGetEvidenceIdentity, SymbolGetLimitError, SymbolGetLimits, SymbolGetNotice,
    SymbolGetPort, SymbolGetPortOutputError, SymbolGetPortRequest, SymbolGetPortResult,
    SymbolGetProducer, SymbolGetProducerIdentity, SymbolGetRequest, SymbolGetResult,
    SymbolGetSelector, symbol_get,
};
pub use symbol_search::{
    MAX_SYMBOL_SEARCH_NAME_BYTES, SYMBOL_SEARCH_PROFILE_VERSION, SymbolSearchClaim,
    SymbolSearchError, SymbolSearchEvidenceIdentity, SymbolSearchNameMatch, SymbolSearchNotice,
    SymbolSearchPort, SymbolSearchPortResult, SymbolSearchProducerIdentity, SymbolSearchQuery,
    SymbolSearchQueryDigest, SymbolSearchQueryError, SymbolSearchRequest, SymbolSearchResult,
    symbol_search,
};
pub use syntax_site_search::{
    DEFAULT_SYNTAX_SITE_SEARCH_OUTPUT_BYTES, DEFAULT_SYNTAX_SITE_SEARCH_RESULTS,
    MAX_SYNTAX_SITE_SEARCH_OUTPUT_BYTES, MAX_SYNTAX_SITE_SEARCH_RESULTS,
    MAX_SYNTAX_SITE_SEARCH_TARGET_BYTES, SYNTAX_SITE_SEARCH_PROFILE_VERSION, SyntaxSiteSearchClaim,
    SyntaxSiteSearchError, SyntaxSiteSearchLimitError, SyntaxSiteSearchLimits,
    SyntaxSiteSearchNotice, SyntaxSiteSearchPort, SyntaxSiteSearchPortOutputError,
    SyntaxSiteSearchPortRequest, SyntaxSiteSearchPortResult, SyntaxSiteSearchQuery,
    SyntaxSiteSearchQueryDigest, SyntaxSiteSearchQueryError, SyntaxSiteSearchRequest,
    SyntaxSiteSearchResult, syntax_site_search,
};
pub use task_checkpoint::{
    EngineeringTaskError, EngineeringTaskPort, TaskCheckpointReceipt, TaskStatusPort,
    TaskVerificationReceipt, append_task_checkpoint, append_task_verification, poll_task,
};
pub use test_markers::{
    DEFAULT_TEST_MARKER_OUTPUT_BYTES, DEFAULT_TEST_MARKER_RESULTS, FIXED_TEST_MARKER_OUTPUT_BYTES,
    MAX_TEST_MARKER_OUTPUT_BYTES, MAX_TEST_MARKER_RESULTS, TEST_MARKERS_PROFILE_VERSION,
    TestMarkerLanguageCoverage, TestMarkersAvailability, TestMarkersError, TestMarkersLimitError,
    TestMarkersLimits, TestMarkersPort, TestMarkersPortOutputError, TestMarkersPortPayload,
    TestMarkersPortRequest, TestMarkersPortResult, TestMarkersQuery, TestMarkersQueryError,
    TestMarkersRequest, TestMarkersResult, test_marker_record_output_bytes, test_markers,
};
pub use typescript_profile::{
    PHASE0_TYPESCRIPT_ANALYSIS_SCHEMA_VERSION, PHASE0_TYPESCRIPT_CANONICALIZATION_VERSION,
    PHASE0_TYPESCRIPT_CONFIGURATION_VERSION, PHASE0_TYPESCRIPT_PRODUCER_MANIFEST_VERSION,
    phase0_tsx_artifact_identity, phase0_typescript_artifact_identity,
};
pub use workspace_identity_text::{
    CONNECTED_WORKSPACE_ID_TEXT_BYTES, ConnectedWorkspaceIdTextV1, SOURCE_SLOT_ID_TEXT_BYTES,
    SourceSlotIdTextV1, WorkspaceIdentityTextError,
};
