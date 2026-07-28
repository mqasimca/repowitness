//! Local SQLite, Git, filesystem, virtual-filesystem, watcher reconciliation,
//! configuration, and bounded-execution adapters.
//!
//! Concrete I/O is kept outside the domain, analysis, and application rules.

mod contained_source;
mod git_memory;
mod git_paths;
mod local_context_build;
mod local_diagnostics;
mod local_index;
mod local_memory_recall;
mod local_search;
mod local_symbol_get;
mod memory_format;
mod memory_import;
mod memory_management;
mod memory_revalidation;
mod rust_index;
mod source_state;
mod sqlite;

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
pub use local_context_build::{
    DEFAULT_LOCAL_CONTEXT_BUILD_DEADLINE, DEFAULT_LOCAL_CONTEXT_PROVIDER_RESULTS,
    LocalContextBuildError, LocalContextBuildRequest, LocalContextBuildResult, build_local_context,
};
pub use local_diagnostics::{
    DEFAULT_LOCAL_DIAGNOSTICS_DEADLINE, LocalRepositoryDiagnosticsError,
    LocalRepositoryDiagnosticsRequest, LocalRepositoryDiagnosticsResult, diagnose_local_repository,
};
pub use local_index::{
    LocalIndexError, LocalIndexReport, LocalIndexRequest, index_local_repository,
    index_local_rust_repository,
};
pub use local_memory_recall::{
    DEFAULT_LOCAL_MEMORY_RECALL_DEADLINE, LocalMemoryRecallError, LocalMemoryRecallRequest,
    LocalMemoryRecallResult, LocalMemoryRecallSelection, recall_local_memory,
};
pub use local_search::{
    DEFAULT_LOCAL_CODE_SEARCH_DEADLINE, LocalCodeSearchError, LocalCodeSearchRequest,
    LocalCodeSearchResult, search_local_index, search_local_rust_index,
};
pub use local_symbol_get::{
    DEFAULT_LOCAL_SYMBOL_GET_DEADLINE, LocalSymbolGetError, LocalSymbolGetRequest,
    LocalSymbolGetResult, LocalSymbolPortError, LocalSymbolSelectorText, Sha256TextError,
    get_local_rust_symbol, get_local_symbol,
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
    LocalMemoryHistoryImportLimits, LocalMemoryHistoryImportReport,
    LocalMemoryHistoryImportRequest, LocalMemoryManageError, LocalMemoryWriteReceipt,
    LocalMemoryWriteRequest, approve_local_memory, import_local_memory_history,
    review_local_memory_correspondence, validate_local_memory_actor, write_local_memory,
};
pub use memory_revalidation::{
    DEFAULT_LOCAL_MEMORY_CANONICAL_BYTES, DEFAULT_LOCAL_MEMORY_GIT_QUERIES,
    DEFAULT_LOCAL_MEMORY_RESULT_CANDIDATES, DEFAULT_LOCAL_MEMORY_REVALIDATION_DEADLINE,
    LocalMemoryRevalidationError, LocalMemoryRevalidationLimits, LocalMemoryRevalidationReport,
    LocalMemoryRevalidationRequest, MAX_LOCAL_MEMORY_GIT_QUERIES, revalidate_local_memory,
};
pub use repowitness_application::{
    CODE_SEARCH_PROFILE_VERSION, CONTEXT_BUILD_RRF_K, CodeSearchNotice, CodeSearchProducer,
    ContextItem, ContextOmission, ContextProvider, DEFAULT_CONTEXT_BUILD_BUDGET_UNITS,
    MAX_CONTEXT_BUILD_BUDGET_UNITS, MEMORY_RECALL_PROFILE_VERSION, MemoryEffectiveState,
    MemoryProjectionValidityState, MemoryRecallCandidate, MemoryRecallCandidateRelation,
    MemoryRecallEvidence, MemoryRecallEvidenceAssurance, MemoryRecallEvidenceOutcome,
    MemoryRecallEvidenceState, MemoryRecallLimits, MemoryRecallOccurrence, MemoryRecallProducer,
    MemoryRecallProjectionCoverage, MemoryRecallQueryDigest, MemoryRecallReason,
    MemoryRecallRecord, MemoryRecordIdTextV1, REPOSITORY_DIAGNOSTICS_PROFILE_VERSION,
    RepositoryDiagnosticCapability, RepositoryDiagnosticLimitation,
    RepositoryDiagnosticsMemoryProjection, RepositoryIdentityTextV1, RepositoryPathTextByteLimit,
    RepositoryPathTextV1, RetrievedSymbol, RustSymbolOccurrence, SYMBOL_GET_PROFILE_VERSION,
};
pub use repowitness_domain::{
    EvidenceLocation, MemoryAssurance, MemoryCommitId, MemoryCorrespondenceReviewOperation,
    MemoryKind, MemoryLifecycle, MemoryObjectFormat, MemoryRevalidationTarget, ResolutionStatus,
};
pub use rust_index::{
    DEFAULT_LOCAL_RUST_INDEX_DEADLINE, LocalRustIndexError, LocalRustIndexLimits,
    LocalRustIndexPreparation, prepare_local_rust_index, prepare_local_source_index,
};
pub use source_state::{
    CapturedSourceState, GIT_STATE_VERSION, GIT_STATUS_PROFILE_VERSION,
    RUST_WORKTREE_STATE_VERSION, SUPPORTED_LANGUAGES_WORKTREE_STATE_VERSION, SourceStateError,
    capture_source_state, capture_source_state_with_cancel,
};
pub use sqlite::{
    BackupLimits, BackupOutcome, CheckpointOutcome, GenerationCoverage, GenerationId,
    IndexStoreStartup, OwnedSqliteIndex, OwnedSqliteReader, ProjectionRebuildLimits,
    ProjectionRebuildOutcome, SearchHit, SearchLimits, SearchResults, SqliteStoreError,
    SymbolLookupResults, create_online_backup,
};
