//! Application use cases, request context, policy enforcement, task
//! supervision, and narrow I/O ports.
//!
//! CLI and MCP adapters call the same use cases through this package.

mod canonical_digest;
mod code_search;
mod configuration;
mod context_build;
mod go_profile;
mod index_publication;
mod memory_import;
mod memory_projection;
mod memory_recall;
mod memory_record_id_text;
mod package_scope;
mod python_profile;
mod repository_diagnostics;
mod repository_identity_text;
mod repository_path_text;
mod rust_graph_profile;
mod rust_graph_read;
mod rust_index;
mod rust_profile;
mod source_profile;
mod source_slot_publication;
mod source_snapshot;
mod symbol_get;
mod typescript_profile;
mod workspace_identity_text;

pub use canonical_digest::{
    ANALYSIS_ARTIFACT_PAYLOAD_VERSION, CanonicalAnalysisArtifactKey, CanonicalSourceManifest,
    hash_analysis_artifact_key, hash_analysis_artifact_payload, hash_source_content,
    hash_source_manifest,
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
pub use package_scope::{
    MAX_PACKAGE_SCOPE_ROOTS, PACKAGE_SCOPE_VERSION, PackageRootCount, PackageRootOrdinal,
    PackageScope, PackageScopeDigest, PackageScopeError,
};
pub use python_profile::{
    PHASE0_PYTHON_ANALYSIS_SCHEMA_VERSION, PHASE0_PYTHON_CANONICALIZATION_VERSION,
    PHASE0_PYTHON_CONFIGURATION_VERSION, PHASE0_PYTHON_PRODUCER_MANIFEST_VERSION,
    phase0_python_artifact_identity,
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
pub use typescript_profile::{
    PHASE0_TYPESCRIPT_ANALYSIS_SCHEMA_VERSION, PHASE0_TYPESCRIPT_CANONICALIZATION_VERSION,
    PHASE0_TYPESCRIPT_CONFIGURATION_VERSION, PHASE0_TYPESCRIPT_PRODUCER_MANIFEST_VERSION,
    phase0_tsx_artifact_identity, phase0_typescript_artifact_identity,
};
pub use workspace_identity_text::{
    CONNECTED_WORKSPACE_ID_TEXT_BYTES, ConnectedWorkspaceIdTextV1, SOURCE_SLOT_ID_TEXT_BYTES,
    SourceSlotIdTextV1, WorkspaceIdentityTextError,
};
