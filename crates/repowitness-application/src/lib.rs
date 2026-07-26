//! Application use cases, request context, policy enforcement, task
//! supervision, and narrow I/O ports.
//!
//! CLI and MCP adapters call the same use cases through this package.

mod canonical_digest;
mod code_search;
mod index_publication;
mod repository_identity_text;
mod repository_path_text;
mod rust_index;
mod rust_profile;
mod source_snapshot;
mod symbol_get;

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
    RustSymbolOccurrence, code_search,
};
pub use index_publication::{
    PublishRustIndexError, PublishRustIndexRequest, PublishedRustIndex, RustIndexCoverage,
    RustIndexPublicationPort, publish_rust_index,
};
pub use repository_identity_text::{
    REPOSITORY_IDENTITY_TEXT_BYTES, RepositoryIdentityTextError, RepositoryIdentityTextV1,
};
pub use repository_path_text::{
    RepositoryPathTextByteCount, RepositoryPathTextByteLimit, RepositoryPathTextError,
    RepositoryPathTextV1, RepositoryPathTextVersion,
};
pub use rust_index::{
    DEFAULT_RUST_INDEX_FACTS, DEFAULT_RUST_INDEX_FILES, DEFAULT_RUST_INDEX_SOURCE_BYTES,
    ImmutableRustSource, MAX_RUST_INDEX_FACTS, MAX_RUST_INDEX_FILES, MAX_RUST_INDEX_SOURCE_BYTES,
    PreparedRustFile, PreparedRustIndex, RustArtifactIdentity, RustIndexLimitError,
    RustIndexLimits, RustIndexPreparationError, prepare_rust_index, prepare_rust_index_with_reuse,
};
pub use rust_profile::{
    PHASE0_RUST_ANALYSIS_SCHEMA_VERSION, PHASE0_RUST_CANONICALIZATION_VERSION,
    PHASE0_RUST_CONFIGURATION_VERSION, PHASE0_RUST_PRODUCER_MANIFEST_VERSION,
    phase0_rust_artifact_identity,
};
pub use source_snapshot::{
    RUST_SOURCE_SNAPSHOT_VERSION, RustSourceSnapshotIdentity, hash_rust_source_snapshot,
};
pub use symbol_get::{
    MAX_SYMBOL_GET_DECLARATION_BYTES, MAX_SYMBOL_GET_OUTPUT_BYTES, RetrievedSymbol,
    SYMBOL_GET_PROFILE_VERSION, SymbolGetCandidate, SymbolGetClaim, SymbolGetError,
    SymbolGetEvidenceIdentity, SymbolGetLimitError, SymbolGetLimits, SymbolGetNotice,
    SymbolGetPort, SymbolGetPortOutputError, SymbolGetPortRequest, SymbolGetPortResult,
    SymbolGetProducer, SymbolGetProducerIdentity, SymbolGetRequest, SymbolGetResult,
    SymbolGetSelector, symbol_get,
};
