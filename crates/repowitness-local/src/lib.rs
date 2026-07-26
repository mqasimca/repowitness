//! Local SQLite, Git, filesystem, virtual-filesystem, watcher reconciliation,
//! configuration, and bounded-execution adapters.
//!
//! Concrete I/O is kept outside the domain, analysis, and application rules.

mod contained_source;
mod git_paths;
mod local_index;
mod local_search;
mod local_symbol_get;
mod rust_index;
mod source_state;
mod sqlite;

pub use contained_source::{
    ContainedSourceError, ContainedSourceRoot, DEFAULT_SOURCE_FILE_BYTES,
    DEFAULT_SOURCE_READ_CHUNK_BYTES, DEFAULT_SOURCE_READ_DEADLINE, MAX_SOURCE_FILE_BYTES,
    MAX_SOURCE_READ_CHUNK_BYTES, SourceReadLimitError, SourceReadLimits,
};
pub use git_paths::{
    DiscoveredRepositoryPaths, GitPathDiscoveryError, GitPathDiscoveryLimits,
    GitPathDiscoveryStats, discover_repository_paths, discover_repository_paths_with_cancel,
};
pub use local_index::{
    LocalIndexError, LocalIndexReport, LocalIndexRequest, index_local_rust_repository,
};
pub use local_search::{
    DEFAULT_LOCAL_CODE_SEARCH_DEADLINE, LocalCodeSearchError, LocalCodeSearchRequest,
    LocalCodeSearchResult, search_local_rust_index,
};
pub use local_symbol_get::{
    DEFAULT_LOCAL_SYMBOL_GET_DEADLINE, LocalSymbolGetError, LocalSymbolGetRequest,
    LocalSymbolGetResult, LocalSymbolPortError, LocalSymbolSelectorText, Sha256TextError,
    get_local_rust_symbol,
};
pub use repowitness_application::{
    CODE_SEARCH_PROFILE_VERSION, CodeSearchNotice, CodeSearchProducer, RepositoryIdentityTextV1,
    RepositoryPathTextByteLimit, RepositoryPathTextV1, RetrievedSymbol, RustSymbolOccurrence,
    SYMBOL_GET_PROFILE_VERSION,
};
pub use repowitness_domain::{EvidenceLocation, ResolutionStatus};
pub use rust_index::{
    DEFAULT_LOCAL_RUST_INDEX_DEADLINE, LocalRustIndexError, LocalRustIndexLimits,
    LocalRustIndexPreparation, prepare_local_rust_index,
};
pub use source_state::{
    CapturedSourceState, GIT_STATE_VERSION, GIT_STATUS_PROFILE_VERSION,
    RUST_WORKTREE_STATE_VERSION, SourceStateError, capture_source_state,
    capture_source_state_with_cancel,
};
pub use sqlite::{
    BackupLimits, BackupOutcome, CheckpointOutcome, GenerationCoverage, GenerationId,
    IndexStoreStartup, OwnedSqliteIndex, OwnedSqliteReader, ProjectionRebuildLimits,
    ProjectionRebuildOutcome, SearchHit, SearchLimits, SearchResults, SqliteStoreError,
    SymbolLookupResults, create_online_backup,
};
