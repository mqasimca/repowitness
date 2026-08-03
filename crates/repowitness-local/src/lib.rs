//! Local SQLite, Git, filesystem, virtual-filesystem, watcher reconciliation,
//! configuration, and bounded-execution adapters.
//!
//! Concrete I/O is kept outside the domain, analysis, and application rules.

mod bounded_file;
mod configuration;
mod connected_workspace_manifest;
mod contained_source;
mod git_memory;
mod git_paths;
mod local_architecture_map;
mod local_architecture_overview;
mod local_code_graph_query;
mod local_connected_workspace;
mod local_context_build;
mod local_diagnostics;
mod local_doctor;
mod local_graph_index;
mod local_graph_read;
mod local_identity;
mod local_index;
mod local_known_at_history;
mod local_memory_recall;
mod local_personal_memory;
mod local_phase2_context_build;
mod local_relevant_paths;
mod local_repository_topology;
mod local_retention;
mod local_scip_evidence_read;
mod local_scip_overlay_import;
mod local_scip_relationship_trace;
mod local_scip_symbol_resolve;
mod local_search;
mod local_symbol_get;
mod local_symbol_search;
mod local_syntax_site_search;
mod local_task;
mod local_test_markers;
mod local_watch;
mod memory_format;
mod memory_import;
mod memory_management;
mod memory_revalidation;
mod package_scope;
mod repository_topology;
mod rust_index;
mod source_selector;
mod source_state;
mod sqlite;
mod watch_reconciliation;

pub use bounded_file::{
    AdmittedFileParent, BoundedFileContents, BoundedFileReadError, MAX_BOUNDED_CONTROL_FILE_BYTES,
    MAX_BOUNDED_CONTROL_FILE_COMPONENTS, MAX_BOUNDED_CONTROL_FILE_PATH_BYTES,
    MAX_BOUNDED_FILE_BYTES, read_bounded_regular_file, read_bounded_regular_file_with_parent,
};
pub use configuration::{
    ConfigurationFileError, ConfigurationFileLayer, MAX_CONFIGURATION_FILE_BYTES,
    MAX_CONFIGURATION_TEXT_BYTES, parse_configuration_file,
};
pub use contained_source::{
    ContainedSourceError, ContainedSourceRoot, DEFAULT_SOURCE_FILE_BYTES,
    DEFAULT_SOURCE_READ_CHUNK_BYTES, DEFAULT_SOURCE_READ_DEADLINE, MAX_EXACT_DIRECTORY_ENTRIES,
    MAX_SOURCE_FILE_BYTES, MAX_SOURCE_READ_CHUNK_BYTES, SourceReadLimitError, SourceReadLimits,
};
pub use git_memory::{
    GitMemoryQueries, GitMemoryQueryError, GitMemoryQueryLimits, GitPathContinuityOutcome,
};
pub use git_paths::{
    DiscoveredRepositoryPaths, GitPathDiscoveryError, GitPathDiscoveryLimits,
    GitPathDiscoveryStats, discover_repository_paths, discover_repository_paths_with_cancel,
};
pub use local_architecture_map::{
    DEFAULT_LOCAL_ARCHITECTURE_MAP_DEADLINE, LocalArchitectureMapError,
    LocalArchitectureMapRequest, LocalArchitectureMapResult, map_local_architecture,
};
pub use local_architecture_overview::{
    DEFAULT_LOCAL_ARCHITECTURE_OVERVIEW_DEADLINE, LocalArchitectureOverviewError,
    LocalArchitectureOverviewRequest, LocalArchitectureOverviewResult, overview_local_architecture,
};
pub use local_code_graph_query::{
    DEFAULT_LOCAL_CODE_GRAPH_QUERY_DEADLINE, LocalCodeGraphQueryError, LocalCodeGraphQueryRequest,
    LocalCodeGraphQueryResult, read_local_code_graph_query,
};
pub use local_connected_workspace::{
    DEFAULT_LOCAL_CONNECTED_WORKSPACE_DEADLINE, DEFAULT_LOCAL_CONNECTED_WORKSPACE_SOURCE_DEADLINE,
    LOCAL_CONNECTED_WORKSPACE_REPORT_VERSION, LocalConnectedWorkspaceCoverage,
    LocalConnectedWorkspaceIndexError, LocalConnectedWorkspaceIndexReport,
    LocalConnectedWorkspaceIndexRequest, LocalConnectedWorkspaceMaintenance,
    LocalConnectedWorkspaceManifestErrorKind, LocalConnectedWorkspaceOutcome,
    LocalConnectedWorkspaceParentErrorKind, LocalConnectedWorkspacePhase,
    LocalConnectedWorkspaceRequestErrorKind, LocalConnectedWorkspaceSourceLimits,
    LocalConnectedWorkspaceViewDigest, MAX_LOCAL_CONNECTED_WORKSPACE_MANIFEST_BYTES,
    index_local_connected_workspace,
};
pub use local_context_build::{
    DEFAULT_LOCAL_CONTEXT_BUILD_DEADLINE, DEFAULT_LOCAL_CONTEXT_PROVIDER_RESULTS,
    LocalContextBuildError, LocalContextBuildRequest, LocalContextBuildResult, build_local_context,
};
pub use local_diagnostics::{
    DEFAULT_LOCAL_DIAGNOSTICS_DEADLINE, LocalRepositoryDiagnosticsError,
    LocalRepositoryDiagnosticsRequest, LocalRepositoryDiagnosticsResult, diagnose_local_repository,
};
pub use local_doctor::{
    DoctorCheckStatus, DoctorDatabaseState, DoctorOverallStatus, LocalDoctorReport,
    LocalDoctorTargets, inspect_local_doctor,
};
pub use local_graph_read::{
    DEFAULT_LOCAL_RUST_GRAPH_READ_DEADLINE, LocalRustGraphEvidenceRead, LocalRustGraphPortError,
    LocalRustGraphReadError, LocalRustGraphReadOutput, LocalRustGraphReadRequest,
    LocalRustGraphReadResult, LocalRustGraphWorkspace, read_local_rust_graph,
};
pub use local_identity::{
    GeneratedLocalIdentity, LocalIdentityGenerationError, LocalIdentityKind,
    generate_local_identity,
};
pub use local_index::{
    LocalIndexError, LocalIndexMutation, LocalIndexReport, LocalIndexRequest,
    index_local_repository, index_local_rust_repository,
};
pub use local_known_at_history::{
    DEFAULT_LOCAL_KNOWN_AT_HISTORY_DEADLINE, LocalKnownAtHistoryError, LocalKnownAtHistoryRequest,
    read_local_known_at_history,
};
pub use local_memory_recall::{
    DEFAULT_LOCAL_MEMORY_RECALL_DEADLINE, LocalMemoryRecallError, LocalMemoryRecallRequest,
    LocalMemoryRecallResult, LocalMemoryRecallSelection, recall_local_memory,
};
pub use local_personal_memory::{
    DEFAULT_LOCAL_PERSONAL_MEMORY_READ_DEADLINE, DEFAULT_LOCAL_PERSONAL_MEMORY_WRITE_DEADLINE,
    LocalPersonalMemoryAppendRequest, LocalPersonalMemoryError, LocalPersonalMemoryReadRequest,
    append_local_personal_memory, read_local_personal_memory,
};
pub use local_phase2_context_build::{
    DEFAULT_LOCAL_PHASE2_CONTEXT_BUILD_DEADLINE, LocalPhase2ContextBuildError,
    LocalPhase2ContextBuildRequest, LocalPhase2ContextBuildResult, LocalPhase2ContextItem,
    LocalPhase2ContextWorkspace, LocalPhase2HistoryItem, build_local_phase2_context,
};
pub use local_relevant_paths::{
    DEFAULT_LOCAL_RELEVANT_PATHS_DEADLINE, LocalRelevantPathsError, LocalRelevantPathsRequest,
    LocalRelevantPathsResult, locate_local_relevant_paths,
};
pub use local_repository_topology::{
    DEFAULT_LOCAL_REPOSITORY_TOPOLOGY_DEADLINE, LocalRepositoryTopologyError,
    LocalRepositoryTopologyRequest, LocalRepositoryTopologyResult, read_local_repository_topology,
};
pub use local_retention::{
    DEFAULT_LOCAL_RETENTION_TIMEOUT, LOCAL_RETENTION_PROFILE_VERSION, LocalRetentionApplyReport,
    LocalRetentionApplyRequest, LocalRetentionError, LocalRetentionErrorKind, LocalRetentionPins,
    LocalRetentionPlanReport, LocalRetentionPlanRequest, LocalRetentionPolicySummary,
    LocalRetentionRequestError, MAX_LOCAL_RETENTION_TIMEOUT, apply_local_retention,
    plan_local_retention,
};
pub use local_scip_evidence_read::{
    DEFAULT_LOCAL_SCIP_EVIDENCE_READ_DEADLINE, LocalScipEvidencePortError,
    LocalScipEvidenceReadError, LocalScipEvidenceReadRequest, LocalScipEvidenceReadResult,
    LocalScipEvidenceWorkspace, read_local_scip_evidence,
};
pub use local_scip_overlay_import::{
    DEFAULT_LOCAL_SCIP_IMPORT_DEADLINE, LocalScipOverlayImportError,
    LocalScipOverlayImportFailureCategory, LocalScipOverlayImportRequest,
    LocalScipOverlayImportResult, MAX_LOCAL_SCIP_IMPORT_DEADLINE,
    MAX_LOCAL_SCIP_IMPORT_INPUT_BYTES, import_local_scip_overlay,
};
pub use local_scip_relationship_trace::{
    DEFAULT_LOCAL_SCIP_RELATIONSHIP_TRACE_DEADLINE, LocalScipRelationshipTraceError,
    LocalScipRelationshipTracePortError, LocalScipRelationshipTraceRequest,
    LocalScipRelationshipTraceResult, trace_local_scip_relationships,
};
pub use local_scip_symbol_resolve::{
    LocalScipSymbolResolveError, LocalScipSymbolResolvePortError, LocalScipSymbolResolveRequest,
    LocalScipSymbolResolveResult, LocalScipSymbolResolveSelectorText, resolve_local_scip_symbol,
};
pub use local_search::{
    DEFAULT_LOCAL_CODE_SEARCH_DEADLINE, LocalCodeSearchError, LocalCodeSearchRequest,
    LocalCodeSearchResult, search_local_index, search_local_rust_index,
};
pub use local_symbol_get::{
    DEFAULT_LOCAL_SYMBOL_GET_DEADLINE, LocalOutboundSitesError, LocalOutboundSitesRequest,
    LocalOutboundSitesResult, LocalSymbolGetError, LocalSymbolGetRequest, LocalSymbolGetResult,
    LocalSymbolPortError, LocalSymbolSelectorText, Sha256TextError, get_local_outbound_sites,
    get_local_rust_symbol, get_local_symbol,
};
pub use local_symbol_search::{
    DEFAULT_LOCAL_SYMBOL_SEARCH_DEADLINE, LocalSymbolSearchError, LocalSymbolSearchRequest,
    LocalSymbolSearchResult, search_local_symbols,
};
pub use local_syntax_site_search::{
    DEFAULT_LOCAL_SYNTAX_SITE_SEARCH_DEADLINE, LocalSyntaxSiteSearchError,
    LocalSyntaxSiteSearchRequest, LocalSyntaxSiteSearchResult, search_local_syntax_sites,
};
pub use local_task::{
    DEFAULT_LOCAL_TASK_POLL_DEADLINE, DEFAULT_LOCAL_TASK_WRITE_DEADLINE, LocalTaskCheckpointError,
    LocalTaskCheckpointRequest, LocalTaskListRequest, LocalTaskPollError, LocalTaskPollRequest,
    MAX_LOCAL_TASK_LIST_RESULTS, append_local_task_checkpoint, list_local_tasks, poll_local_task,
};
pub use local_test_markers::{
    DEFAULT_LOCAL_TEST_MARKERS_DEADLINE, LocalTestMarkersError, LocalTestMarkersRequest,
    LocalTestMarkersResult, read_local_test_markers,
};
pub use local_watch::{
    LOCAL_WATCH_PROFILE_VERSION, LocalWatchError, LocalWatchExit, LocalWatchReconciliation,
    LocalWatchReport, LocalWatchRequest, LocalWatchRequestError, MAX_LOCAL_WATCH_RUNTIME,
    watch_local_repository,
};
pub use memory_format::{
    MAX_CANONICAL_MEMORY_BYTES, MAX_MEMORY_SCALAR_BYTES, MAX_MEMORY_YAML_BYTES,
    MemoryFormatControl, MemoryFormatError, ParsedMemoryRecord, canonical_memory_digest,
    canonical_memory_json, generate_memory_yaml, parse_memory_record,
};
pub use memory_import::{LoadedMemoryRecord, MemoryFileImportError, MemoryRecordFiles};
pub use memory_management::{
    DEFAULT_LOCAL_MEMORY_MANAGE_DEADLINE, LocalMemoryApprovalReceipt, LocalMemoryApprovalRequest,
    LocalMemoryCorrespondenceReviewReceipt, LocalMemoryCorrespondenceReviewRequest,
    LocalMemoryDatabaseIdentity, LocalMemoryFilePublicationStatus, LocalMemoryHistoryImportLimits,
    LocalMemoryHistoryImportReport, LocalMemoryHistoryImportRequest, LocalMemoryMaintenance,
    LocalMemoryMaintenanceStep, LocalMemoryManageError, LocalMemoryMutation,
    LocalMemoryWriteReceipt, LocalMemoryWriteRequest, LocalTeamMemorySyncReceipt,
    LocalTeamMemorySyncRequest, MemoryFileIdentityStatus, MemoryFilePublicationStepStatus,
    approve_local_memory, import_local_memory_history, review_local_memory_correspondence,
    sync_local_team_memory, validate_local_memory_actor, write_local_memory,
};
pub use memory_revalidation::{
    DEFAULT_LOCAL_MEMORY_CANONICAL_BYTES, DEFAULT_LOCAL_MEMORY_GIT_QUERIES,
    DEFAULT_LOCAL_MEMORY_RESULT_CANDIDATES, DEFAULT_LOCAL_MEMORY_REVALIDATION_DEADLINE,
    LocalMemoryRevalidationError, LocalMemoryRevalidationLimits, LocalMemoryRevalidationMutation,
    LocalMemoryRevalidationReport, LocalMemoryRevalidationRequest, MAX_LOCAL_MEMORY_GIT_QUERIES,
    revalidate_local_memory,
};
pub use repository_topology::{
    PreparedRepositoryTopology, RepositoryTopologyPreparationError, prepare_repository_topology,
};
pub use repowitness_application::ScipRelationshipTraceDirection;
pub use repowitness_application::{
    ARCHITECTURE_MAP_PROFILE_VERSION, ARCHITECTURE_OVERVIEW_PROFILE_VERSION, ArchitectureMapFile,
    ArchitectureOverviewEntryPointCandidate, ArchitectureOverviewSourceRoot,
    CODE_SEARCH_PROFILE_VERSION, CONFIGURATION_DIGEST_VERSION, CONFIGURATION_RESOLVER_VERSION,
    CONFIGURATION_SCHEMA_VERSION, CONNECTED_WORKSPACE_ID_TEXT_BYTES, CONTEXT_BUILD_RRF_K,
    CodeSearchNotice, CodeSearchProducer, ConfigurationField, ConfigurationLayer,
    ConfigurationLayerKind, ConfigurationPolicyOverrides, ConfigurationPreferenceOverrides,
    ConfigurationProfile, ConfigurationResolutionError, ConfigurationValidationError,
    ConnectedWorkspaceIdTextV1, ContextItem, ContextOmission, ContextProvider,
    DEFAULT_ARCHITECTURE_MAP_FILES, DEFAULT_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES,
    DEFAULT_ARCHITECTURE_OVERVIEW_FILES, DEFAULT_ARCHITECTURE_OVERVIEW_ROOTS,
    DEFAULT_CONFIGURATION_RETAINED_GENERATIONS_PER_SOURCE_SLOT,
    DEFAULT_CONFIGURATION_RETENTION_BYTES, DEFAULT_CONFIGURATION_RETENTION_GENERATION_CANDIDATES,
    DEFAULT_CONFIGURATION_RETENTION_ROWS, DEFAULT_CONTEXT_BUILD_BUDGET_UNITS,
    EffectiveConfigurationPolicy, EffectiveConfigurationPreferences,
    EffectiveRetentionConfiguration, MAX_ARCHITECTURE_MAP_FILES,
    MAX_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES, MAX_ARCHITECTURE_OVERVIEW_FILES,
    MAX_ARCHITECTURE_OVERVIEW_ROOTS, MAX_CONFIGURATION_CONTEXT_BYTES,
    MAX_CONFIGURATION_FILE_LAYERS, MAX_CONFIGURATION_GRAPH_DEPTH, MAX_CONFIGURATION_GRAPH_RESULTS,
    MAX_CONFIGURATION_QUERY_RESULTS, MAX_CONFIGURATION_RETAINED_GENERATIONS_PER_SOURCE_SLOT,
    MAX_CONFIGURATION_RETENTION_BYTES, MAX_CONFIGURATION_RETENTION_GENERATION_CANDIDATES,
    MAX_CONFIGURATION_RETENTION_ROWS, MAX_CONFIGURATION_SOURCE_FILE_BYTES,
    MAX_CONFIGURATION_SOURCE_FILES, MAX_CONFIGURATION_WATCHER_POLL_INTERVAL_MS,
    MAX_CONTEXT_BUILD_BUDGET_UNITS, MEMORY_RECALL_PROFILE_VERSION,
    MIN_CONFIGURATION_WATCHER_POLL_INTERVAL_MS, McpToolProfile, MemoryEffectiveState,
    MemoryProjectionValidityState, MemoryRecallCandidate, MemoryRecallCandidateRelation,
    MemoryRecallEvidence, MemoryRecallEvidenceAssurance, MemoryRecallEvidenceOutcome,
    MemoryRecallEvidenceState, MemoryRecallLimits, MemoryRecallOccurrence, MemoryRecallProducer,
    MemoryRecallProjectionCoverage, MemoryRecallQueryDigest, MemoryRecallReason,
    MemoryRecallRecord, MemoryRecordIdTextV1, PolicyValue, RELEVANT_PATHS_PROFILE_VERSION,
    REPOSITORY_DIAGNOSTICS_PROFILE_VERSION, RepositoryDiagnosticCapability,
    RepositoryDiagnosticLimitation, RepositoryDiagnosticsMemoryProjection,
    RepositoryIdentityTextV1, RepositoryPathTextByteLimit, RepositoryPathTextV1,
    ResolvedConfiguration, ResolvedPreference, ResolvedToolProfilePreference,
    RetentionConfigurationOverrides, RetrievedSymbol, RustGraphDefinitionSelector,
    RustGraphReadOperation, RustGraphReadSelection, RustGraphSelectorError, RustGraphSiteKind,
    RustGraphSiteSelector as ApplicationRustGraphSiteSelector, RustGraphSymbolQuery,
    RustGraphSymbolQueryError, RustGraphTraceDirection, RustGraphTraceLimits,
    RustGraphTraceStartSelector, RustSymbolKind, RustSymbolOccurrence, SOURCE_SLOT_ID_TEXT_BYTES,
    SYMBOL_GET_PROFILE_VERSION, SYMBOL_SEARCH_PROFILE_VERSION, SourceLanguage, SourceSlotIdTextV1,
    SymbolSearchNameMatch, WorkspaceIdentityTextError, resolve_configuration,
};
pub use repowitness_application::{
    ArchitectureMapLimits, ArchitectureOverviewLimits, CodeGraphQueryOperation,
    CodeGraphQueryResult, CodeSearchLimits, CodeSearchQuery, RelevantPathsLimits,
    SymbolSearchQuery, TestMarkersLimits, TestMarkersQuery,
};
pub use repowitness_application::{
    OUTBOUND_SITES_PROFILE_VERSION, OutboundSitesAvailability, OutboundSyntaxSite,
    SYNTAX_SITE_SEARCH_PROFILE_VERSION, SyntaxSiteSearchLimits, SyntaxSiteSearchQuery,
};
pub use repowitness_application::{Phase2ContextCandidate, Phase2ContextTier};
pub use repowitness_application::{TEST_MARKERS_PROFILE_VERSION, TestMarkersAvailability};
pub use repowitness_domain::{
    ConfigurationDigest, ConnectedWorkspaceId, EvidenceLocation, MemoryAssurance, MemoryCommitId,
    MemoryCorrespondenceReviewOperation, MemoryKind, MemoryLifecycle, MemoryObjectFormat,
    MemoryObservationSource, MemoryRevalidationTarget, PersonalMemoryId, PersonalMemoryKind,
    PersonalMemoryProfileId, PersonalMemoryRecord, PersonalMemoryRevision,
    Phase2ContextProviderAvailability, ResolutionStatus, SourceSlotId, SourceSnapshotDigest,
    TaskId, TaskState, TaskStatus,
};
pub use rust_index::{
    DEFAULT_LOCAL_RUST_INDEX_DEADLINE, LocalRustIndexError, LocalRustIndexLimits,
    LocalRustIndexPreparation, LocalSourceSnapshotFenceError, prepare_local_rust_index,
    prepare_local_source_index,
};
pub use source_state::{
    CapturedSourceState, GIT_STATE_VERSION, GIT_STATUS_PROFILE_VERSION,
    RUST_WORKTREE_STATE_VERSION, SUPPORTED_LANGUAGES_WORKTREE_STATE_VERSION, SourceStateError,
    capture_source_state, capture_source_state_with_cancel,
};
pub use sqlite::{
    BackupIdentityStatus, BackupLimits, BackupMaintenanceStatus, BackupOutcome,
    BackupPublicationStatus, CheckpointOutcome, DEFAULT_RETAINED_GENERATIONS_PER_SOURCE_SLOT,
    GenerationCoverage, GenerationId, GenerationRetentionPolicy, GitHistoryEvidence,
    IndexStoreStartup, KnownAtApplicability, KnownAtEvidenceBasis, KnownAtHistoryCoverage,
    KnownAtHistoryReceipt, KnownAtObservationEvidence, MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS,
    MAX_RETAINED_GENERATIONS_PER_SOURCE_SLOT, MAX_RETENTION_BYTES,
    MAX_RETENTION_GENERATION_CANDIDATES, MAX_RETENTION_GENERATION_PINS, MAX_RETENTION_ROWS,
    MAX_RETENTION_VIEW_PINS, MAX_SCIP_OVERLAY_DOCUMENTS, OwnedSqliteIndex, OwnedSqliteReader,
    PersonalMemoryReceipt, PinnedWorkspaceView, PinnedWorkspaceViewMember,
    PreparedRawSyntaxArtifact, PreparedRawSyntaxGeneration, PreparedRustGraphArtifact,
    PreparedRustGraphGeneration, PreparedScipOverlay, ProjectionRebuildLimits,
    ProjectionRebuildOutcome, RETENTION_POLICY_VERSION, RawSyntaxPreparationControl,
    RawSyntaxPreparationError, RawSyntaxSiteProjectionAvailability, RawSyntaxSiteReadLimits,
    RawSyntaxSiteRecord, RawSyntaxSitesReadResult, RetentionApplyOutcome, RetentionApplyRequest,
    RetentionLimits, RetentionPins, RetentionPlan, RetentionPlanDigest, RetentionPlanRequest,
    RetentionPolicyDigest, RustGraphArchitectureSummary, RustGraphAvailability,
    RustGraphCandidateRecord, RustGraphDefinitionRecord, RustGraphDirection, RustGraphEdgeKind,
    RustGraphEdgeKinds, RustGraphEdgeRecord, RustGraphEvidenceResult, RustGraphImpactClass,
    RustGraphImpactResult, RustGraphImpactedDefinition, RustGraphOutcomeRecord,
    RustGraphPreparationControl, RustGraphPreparationError, RustGraphPublicationSummary,
    RustGraphReadError, RustGraphReadLimits, RustGraphRelationshipCardinality,
    RustGraphSiteSelector, RustGraphSource, RustGraphSymbolSearchResult, RustGraphTraceCoverage,
    RustGraphTraceResult, RustGraphTraceStart, RustGraphTraceTruncation, ScipEvidenceReadLimits,
    ScipEvidenceReadLimitsError, ScipOccurrenceEvidence, ScipOverlayAvailability,
    ScipOverlayImportScope, ScipOverlayPreparationError, ScipOverlaySummary,
    ScipRelationshipDirection, ScipRelationshipEvidence, ScipRelationshipTrace,
    ScipRelationshipTraceEdge, ScipRelationshipTraceNoRelationships, ScipRelationshipTraceResult,
    ScipSymbolEvidence, ScipSymbolEvidenceResult, ScipSyntaxSymbolResolution, SearchHit,
    SearchLimits, SearchResults, SourceSlotEpoch, SourceSlotGeneration, SourceSlotState,
    SqliteStoreError, SymbolLookupResults, TaskCheckpointReceipt, TaskVerificationReceipt,
    WorkspaceSourceSlot, WorkspaceViewId, WorkspaceViewMember, create_online_backup,
    prepare_raw_syntax_generation, prepare_rust_graph_generation,
};
pub use watch_reconciliation::{
    CompleteReconciliationWork, DEFAULT_WATCHER_DEBOUNCE_MS, DEFAULT_WATCHER_HINT_PATH_BYTES,
    DEFAULT_WATCHER_HINT_PATHS, DEFAULT_WATCHER_MAX_RETRIES, DEFAULT_WATCHER_PERIODIC_MS,
    DEFAULT_WATCHER_POLL_INTERVAL_MS, DEFAULT_WATCHER_RETRY_DELAY_MS, MAX_WATCHER_DEBOUNCE_MS,
    MAX_WATCHER_HINT_PATH_BYTES, MAX_WATCHER_HINT_PATHS, MAX_WATCHER_PERIODIC_MS,
    MAX_WATCHER_RETRIES, MAX_WATCHER_RETRY_DELAY_MS, PollingHintObservation,
    PollingReconciliationRequest, PollingReconciliationSupervisor,
    WATCH_RECONCILIATION_PROFILE_VERSION, WatcherCompletion, WatcherCompletionOutcome,
    WatcherDurationMillis, WatcherFullReconciliationCauses, WatcherHintAccumulator,
    WatcherHintAdmission, WatcherHintBatch, WatcherHintCounters, WatcherHintLimitError,
    WatcherHintLimits, WatcherMonotonicTimestamp, WatcherObservationOutcome, WatcherPathByteCount,
    WatcherPathCount, WatcherPollDecision, WatcherPollIntervalMillis, WatcherPollingState,
    WatcherReconciliationReason, WatcherRetryAttempt, WatcherScheduleLimitError,
    WatcherScheduleLimits, WatcherStateCounters, WatcherStateError,
};
