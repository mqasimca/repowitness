use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::Instant,
};

use repowitness_analysis::{
    RUST_CORRESPONDENCE_PROFILE_ID, RUST_CORRESPONDENCE_PROFILE_VERSION, RustAnalysisLimits,
    RustOccurrenceFingerprint, RustSourceAnalysis, RustSymbolFact, RustSymbolKind,
};
use repowitness_application::{
    CodeSearchCandidate, CodeSearchLimits, CodeSearchPort, CodeSearchPortResult, CodeSearchQuery,
    MemoryRecallLimits, MemoryRecallPort, MemoryRecallPortResult, MemoryRecallQuery,
    RepositoryDiagnosticsPort, RepositoryDiagnosticsPortResult, RustArtifactIdentity,
    RustIndexCoverage, RustIndexLimits, RustSymbolOccurrence, SourceArtifactEvidence,
    SourceLanguage, SymbolGetSelector, hash_analysis_artifact_key, hash_analysis_artifact_payload,
};
use repowitness_domain::{
    AnalysisArtifactDigest, AnalysisArtifactKey, AnalysisArtifactPayloadDigest, ByteOffset,
    ByteSpan, CorrespondenceFingerprintDigest, DeclarationDigest, ProducerManifestDigest,
    RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits, SourceContentDigest,
    SourceSnapshotDigest,
};
use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params,
};

use super::{
    GenerationId, SqliteStoreError, memory_reader::recall_active_memory, open_index_reader,
};

const MAX_QUERY_BYTES: usize = 256;
const MAX_QUERY_TERMS: usize = 8;
const MAX_TERM_BYTES: usize = 64;
const MAX_RESULTS: u16 = 100;
const MAX_RESULT_BYTES: u64 = 1024 * 1024;
const FIXED_SEARCH_HIT_OUTPUT_BYTES: u64 = 136;
const MAX_REUSABLE_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
const PROGRESS_OPCODES: i32 = 1_000;
const PERSISTED_PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(1_048_576, 1_048_576);

type SearchReply = SyncSender<Result<SearchResults, SqliteStoreError>>;
type SymbolReply = SyncSender<Result<SymbolLookupResults, SqliteStoreError>>;
type ArtifactReply =
    SyncSender<Result<BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>, SqliteStoreError>>;
type MemoryRecallReply =
    SyncSender<Result<MemoryRecallPortResult<GenerationId, i64>, SqliteStoreError>>;
type DiagnosticsReply =
    SyncSender<Result<RepositoryDiagnosticsPortResult<GenerationId, i64>, SqliteStoreError>>;

enum ReaderCommand {
    Search(Box<SearchCommand>),
    GetSymbol(Box<SymbolCommand>),
    LoadArtifacts(Box<ArtifactCommand>),
    RecallMemory(Box<MemoryRecallCommand>),
    Diagnostics(Box<DiagnosticsCommand>),
    Shutdown {
        reply: SyncSender<Result<(), SqliteStoreError>>,
    },
}

struct SearchCommand {
    repository: RepositoryIdentityDigest,
    query: String,
    limits: SearchLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: SearchReply,
}

struct SymbolCommand {
    repository: RepositoryIdentityDigest,
    expected_snapshot: SourceSnapshotDigest,
    expected_generation: GenerationId,
    selector: SymbolGetSelector,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: SymbolReply,
}

struct ArtifactCommand {
    requested: Box<[AnalysisArtifactDigest]>,
    language: SourceLanguage,
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: ArtifactReply,
}

struct MemoryRecallCommand {
    repository: RepositoryIdentityDigest,
    query: Option<String>,
    limits: MemoryRecallLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: MemoryRecallReply,
}

struct DiagnosticsCommand {
    repository: RepositoryIdentityDigest,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: DiagnosticsReply,
}

/// Inclusive row and encoded-output limits for one lexical search.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SearchLimits {
    max_results: u16,
    max_output_bytes: u64,
}

impl SearchLimits {
    /// Constructs bounded Phase 0 lexical search limits.
    pub const fn try_new(
        max_results: u16,
        max_output_bytes: u64,
    ) -> Result<Self, SqliteStoreError> {
        if max_results == 0
            || max_results > MAX_RESULTS
            || max_output_bytes == 0
            || max_output_bytes > MAX_RESULT_BYTES
        {
            return Err(SqliteStoreError::InvalidSearchLimits);
        }
        Ok(Self {
            max_results,
            max_output_bytes,
        })
    }

    /// Returns the inclusive row limit.
    #[must_use]
    pub const fn max_results(self) -> u16 {
        self.max_results
    }

    /// Returns the inclusive aggregate encoded-output byte limit.
    #[must_use]
    pub const fn max_output_bytes(self) -> u64 {
        self.max_output_bytes
    }
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            max_results: 20,
            max_output_bytes: 256 * 1024,
        }
    }
}

/// One generation-scoped lexical symbol candidate.
#[derive(Clone, Eq, PartialEq)]
pub struct SearchHit {
    path: RepositoryPath,
    language: SourceLanguage,
    fact_ordinal: u64,
    content_digest: SourceContentDigest,
    artifact_digest: AnalysisArtifactDigest,
    producer_manifest: ProducerManifestDigest,
    kind: RustSymbolKind,
    name: String,
    qualified_name: String,
    name_span: ByteSpan,
    declaration_span: ByteSpan,
}

impl SearchHit {
    /// Returns the exact repository path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the persisted syntax-adapter language.
    #[must_use]
    pub const fn language(&self) -> SourceLanguage {
        self.language
    }

    /// Returns the deterministic source-order fact ordinal within the file.
    #[must_use]
    pub const fn fact_ordinal(&self) -> u64 {
        self.fact_ordinal
    }

    /// Returns the exact source-content identity.
    #[must_use]
    pub const fn content_digest(&self) -> SourceContentDigest {
        self.content_digest
    }

    /// Returns the semantics-complete analysis-artifact identity.
    #[must_use]
    pub const fn artifact_digest(&self) -> AnalysisArtifactDigest {
        self.artifact_digest
    }

    /// Returns the exact syntax producer that created the persisted artifact.
    #[must_use]
    pub const fn producer_manifest(&self) -> ProducerManifestDigest {
        self.producer_manifest
    }

    /// Returns the declaration category.
    #[must_use]
    pub const fn kind(&self) -> RustSymbolKind {
        self.kind
    }

    /// Returns the exact symbol name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the deterministic syntax-qualified name.
    #[must_use]
    pub fn qualified_name(&self) -> &str {
        &self.qualified_name
    }

    /// Returns the identifier's half-open source byte span.
    #[must_use]
    pub const fn name_span(&self) -> ByteSpan {
        self.name_span
    }

    /// Returns the declaration's half-open source byte span.
    #[must_use]
    pub const fn declaration_span(&self) -> ByteSpan {
        self.declaration_span
    }
}

impl fmt::Debug for SearchHit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchHit")
            .field("path", &self.path)
            .field("language", &self.language)
            .field("fact_ordinal", &self.fact_ordinal)
            .field("content_digest", &self.content_digest)
            .field("artifact_digest", &self.artifact_digest)
            .field("producer_manifest", &self.producer_manifest)
            .field("kind", &self.kind)
            .field("name", &"<redacted-symbol>")
            .field("qualified_name", &"<redacted-symbol>")
            .finish()
    }
}

/// Complete bounded lexical result pinned to one immutable generation.
#[derive(Eq, PartialEq)]
pub struct SearchResults {
    snapshot: SourceSnapshotDigest,
    generation: GenerationId,
    producer_manifest: ProducerManifestDigest,
    index_coverage: RustIndexCoverage,
    hits: Box<[SearchHit]>,
    total_matches: u64,
    output_bytes: u64,
}

impl SearchResults {
    /// Returns the concrete source snapshot used for the complete query.
    #[must_use]
    pub const fn snapshot(&self) -> SourceSnapshotDigest {
        self.snapshot
    }

    /// Returns the immutable generation used for the complete query.
    #[must_use]
    pub const fn generation(&self) -> GenerationId {
        self.generation
    }

    /// Returns the exact syntax-producer manifest for the active snapshot.
    #[must_use]
    pub const fn producer_manifest(&self) -> ProducerManifestDigest {
        self.producer_manifest
    }

    /// Returns indexing coverage stored before generation activation.
    #[must_use]
    pub const fn index_coverage(&self) -> RustIndexCoverage {
        self.index_coverage
    }

    /// Returns candidates in deterministic rank, path, and ordinal order.
    #[must_use]
    pub const fn hits(&self) -> &[SearchHit] {
        &self.hits
    }

    /// Returns the exact match count before the result-row limit.
    #[must_use]
    pub const fn total_matches(&self) -> u64 {
        self.total_matches
    }

    /// Returns the bounded aggregate encoded result bytes.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
}

impl fmt::Debug for SearchResults {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchResults")
            .field("snapshot", &self.snapshot)
            .field("generation", &self.generation)
            .field("hit_count", &self.hits.len())
            .field("total_matches", &self.total_matches)
            .field("output_bytes", &self.output_bytes)
            .finish()
    }
}

/// One exact occurrence lookup pinned to an expected active generation.
#[derive(Eq, PartialEq)]
pub struct SymbolLookupResults {
    snapshot: SourceSnapshotDigest,
    generation: GenerationId,
    producer_manifest: ProducerManifestDigest,
    index_coverage: RustIndexCoverage,
    hit: Option<SearchHit>,
}

impl SymbolLookupResults {
    /// Returns the concrete source snapshot used for the lookup.
    #[must_use]
    pub const fn snapshot(&self) -> SourceSnapshotDigest {
        self.snapshot
    }

    /// Returns the immutable generation used for the lookup.
    #[must_use]
    pub const fn generation(&self) -> GenerationId {
        self.generation
    }

    /// Returns the exact syntax-producer manifest for the snapshot.
    #[must_use]
    pub const fn producer_manifest(&self) -> ProducerManifestDigest {
        self.producer_manifest
    }

    /// Returns indexing coverage stored before generation activation.
    #[must_use]
    pub const fn index_coverage(&self) -> RustIndexCoverage {
        self.index_coverage
    }

    /// Returns the exact occurrence when it exists in the expected context.
    #[must_use]
    pub const fn hit(&self) -> Option<&SearchHit> {
        self.hit.as_ref()
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        SourceSnapshotDigest,
        GenerationId,
        ProducerManifestDigest,
        RustIndexCoverage,
        Option<SearchHit>,
    ) {
        (
            self.snapshot,
            self.generation,
            self.producer_manifest,
            self.index_coverage,
            self.hit,
        )
    }
}

impl fmt::Debug for SymbolLookupResults {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SymbolLookupResults")
            .field("snapshot", &self.snapshot)
            .field("generation", &self.generation)
            .field("found", &self.hit.is_some())
            .finish()
    }
}

/// Capacity-one command client for one SQLite read-connection owner thread.
pub struct OwnedSqliteReader {
    commands: SyncSender<ReaderCommand>,
    worker: Option<JoinHandle<()>>,
}

impl OwnedSqliteReader {
    /// Opens and validates one read-only owner connection.
    pub fn start(path: &Path, deadline: Instant) -> Result<Self, SqliteStoreError> {
        if Instant::now() >= deadline {
            return Err(SqliteStoreError::DeadlineExceeded);
        }
        let (commands, receiver) = mpsc::sync_channel(1);
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let database_path = PathBuf::from(path);
        let worker = thread::Builder::new()
            .name("repowitness-sqlite-reader".to_owned())
            .spawn(move || {
                let result = open_index_reader(&database_path);
                let Ok(mut connection) = result else {
                    let error = result.err().unwrap_or(SqliteStoreError::WorkerUnavailable);
                    let _ = startup_sender.send(Err(error));
                    return;
                };
                if startup_sender.send(Ok(())).is_err() {
                    return;
                }
                run_reader(&mut connection, receiver);
            })
            .map_err(|_| SqliteStoreError::WorkerUnavailable)?;
        receive_reply(&startup_receiver, deadline)?;
        Ok(Self {
            commands,
            worker: Some(worker),
        })
    }

    /// Searches only the active generation using literal bounded query terms.
    pub fn search(
        &self,
        repository: RepositoryIdentityDigest,
        query: &str,
        limits: SearchLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<SearchResults, SqliteStoreError> {
        let canonical_query = literal_fts_query(query)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::Search(Box::new(SearchCommand {
                repository,
                query: canonical_query,
                limits,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(results) => Ok(results),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Resolves one exact occurrence only if the requested context remains active.
    pub fn get_symbol(
        &self,
        repository: RepositoryIdentityDigest,
        expected_snapshot: SourceSnapshotDigest,
        expected_generation: GenerationId,
        selector: SymbolGetSelector,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<SymbolLookupResults, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::GetSymbol(Box::new(SymbolCommand {
                repository,
                expected_snapshot,
                expected_generation,
                selector,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(results) => Ok(results),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Loads only exact, complete, integrity-checked artifacts requested by preparation.
    #[cfg(test)]
    pub(crate) fn load_reusable_artifacts(
        &self,
        requested: &[AnalysisArtifactDigest],
        identity: RustArtifactIdentity,
        limits: RustIndexLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>, SqliteStoreError> {
        self.load_reusable_artifacts_for_language(
            requested,
            SourceLanguage::Rust,
            identity,
            limits,
            cancelled,
            deadline,
        )
    }

    pub(crate) fn load_reusable_artifacts_for_language(
        &self,
        requested: &[AnalysisArtifactDigest],
        language: SourceLanguage,
        identity: RustArtifactIdentity,
        limits: RustIndexLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>, SqliteStoreError> {
        let requested_count =
            u64::try_from(requested.len()).map_err(|_| SqliteStoreError::CountNotRepresentable)?;
        if requested_count > limits.max_files()
            || requested.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::LoadArtifacts(Box::new(ArtifactCommand {
                requested: requested.to_vec().into_boxed_slice(),
                language,
                identity,
                limits,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(results) => Ok(results),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Recalls records only from a complete projection of the current active source.
    pub fn recall_memory(
        &self,
        repository: RepositoryIdentityDigest,
        query: &MemoryRecallQuery,
        limits: MemoryRecallLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<MemoryRecallPortResult<GenerationId, i64>, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::RecallMemory(Box::new(MemoryRecallCommand {
                repository,
                query: query.as_str().map(str::to_owned),
                limits,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(results) => Ok(results),
            Err(SqliteStoreError::MemoryProjectionUnavailable) => {
                Err(SqliteStoreError::MemoryProjectionUnavailable)
            }
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Reads one transactionally pinned active source and optional memory projection.
    pub fn diagnostics(
        &self,
        repository: RepositoryIdentityDigest,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RepositoryDiagnosticsPortResult<GenerationId, i64>, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::Diagnostics(Box::new(DiagnosticsCommand {
                repository,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(results) => Ok(results),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Stops and joins the owned reader thread.
    pub fn shutdown(mut self, deadline: Instant) -> Result<(), SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(ReaderCommand::Shutdown { reply }, deadline)?;
        receive_reply(&receiver, deadline)?;
        self.join_worker()
    }

    fn send(&self, command: ReaderCommand, deadline: Instant) -> Result<(), SqliteStoreError> {
        if Instant::now() >= deadline {
            return Err(SqliteStoreError::DeadlineExceeded);
        }
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(_) => SqliteStoreError::QueueFull,
                TrySendError::Disconnected(_) => SqliteStoreError::WorkerUnavailable,
            })
    }

    fn join_worker(&mut self) -> Result<(), SqliteStoreError> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker.join().map_err(|_| SqliteStoreError::WorkerPanicked)
    }
}

include!("reader/adapters.rs");
include!("reader/artifacts.rs");
include!("reader/diagnostics.rs");
include!("reader/query.rs");

#[cfg(test)]
mod tests;
