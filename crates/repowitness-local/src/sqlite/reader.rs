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

use repowitness_analysis::{RustSourceAnalysis, RustSymbolFact, RustSymbolKind};
use repowitness_application::{
    CodeSearchCandidate, CodeSearchLimits, CodeSearchPort, CodeSearchPortResult, CodeSearchQuery,
    RustArtifactIdentity, RustIndexCoverage, RustIndexLimits, RustSymbolOccurrence,
    SymbolGetSelector, hash_analysis_artifact_key, hash_analysis_artifact_payload,
};
use repowitness_domain::{
    AnalysisArtifactDigest, AnalysisArtifactKey, AnalysisArtifactPayloadDigest, ByteOffset,
    ByteSpan, ProducerManifestDigest, RepositoryIdentityDigest, RepositoryPath,
    RepositoryPathLimits, SourceContentDigest, SourceSnapshotDigest,
};
use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params,
};

use super::{GenerationId, SqliteStoreError, open_index_reader};

const MAX_QUERY_BYTES: usize = 256;
const MAX_QUERY_TERMS: usize = 8;
const MAX_TERM_BYTES: usize = 64;
const MAX_RESULTS: u16 = 100;
const MAX_RESULT_BYTES: u64 = 1024 * 1024;
const MAX_REUSABLE_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
const PROGRESS_OPCODES: i32 = 1_000;
const PERSISTED_PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(1_048_576, 1_048_576);

type SearchReply = SyncSender<Result<SearchResults, SqliteStoreError>>;
type SymbolReply = SyncSender<Result<SymbolLookupResults, SqliteStoreError>>;
type ArtifactReply =
    SyncSender<Result<BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>, SqliteStoreError>>;

enum ReaderCommand {
    Search(Box<SearchCommand>),
    GetSymbol(Box<SymbolCommand>),
    LoadArtifacts(Box<ArtifactCommand>),
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
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: ArtifactReply,
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
    fact_ordinal: u64,
    content_digest: SourceContentDigest,
    artifact_digest: AnalysisArtifactDigest,
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
            .field("fact_ordinal", &self.fact_ordinal)
            .field("content_digest", &self.content_digest)
            .field("artifact_digest", &self.artifact_digest)
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
    pub(crate) fn load_reusable_artifacts(
        &self,
        requested: &[AnalysisArtifactDigest],
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

impl CodeSearchPort for OwnedSqliteReader {
    type Generation = GenerationId;
    type Error = SqliteStoreError;

    fn search(
        &self,
        repository: RepositoryIdentityDigest,
        query: &CodeSearchQuery,
        limits: CodeSearchLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<CodeSearchPortResult<Self::Generation>, Self::Error> {
        let storage_limits =
            SearchLimits::try_new(limits.max_results(), limits.max_output_bytes())?;
        let results = OwnedSqliteReader::search(
            self,
            repository,
            query.as_str(),
            storage_limits,
            cancelled,
            deadline,
        )?;
        let SearchResults {
            snapshot,
            generation,
            producer_manifest,
            index_coverage,
            hits,
            total_matches,
            output_bytes,
        } = results;
        let mut candidates = Vec::with_capacity(hits.len());
        for hit in hits.into_vec() {
            let occurrence = RustSymbolOccurrence::try_new(
                hit.fact_ordinal,
                hit.artifact_digest,
                hit.kind,
                hit.name,
                hit.qualified_name,
                hit.name_span,
                hit.declaration_span,
            )
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            candidates.push(CodeSearchCandidate::new(
                hit.path,
                hit.content_digest,
                occurrence,
            ));
        }
        Ok(CodeSearchPortResult::new(
            snapshot,
            generation,
            producer_manifest,
            index_coverage,
            candidates,
            total_matches,
            output_bytes,
        ))
    }
}

impl Drop for OwnedSqliteReader {
    fn drop(&mut self) {
        if self.worker.is_none() {
            return;
        }
        let (reply, _receiver) = mpsc::sync_channel(1);
        if self
            .commands
            .try_send(ReaderCommand::Shutdown { reply })
            .is_ok()
        {
            let _ = self.join_worker();
        } else {
            // Dropping the sender disconnects the worker after any queued
            // command. Do not wait without having delivered a shutdown.
            let _ = self.worker.take();
        }
    }
}

fn run_reader(connection: &mut Connection, receiver: Receiver<ReaderCommand>) {
    while let Ok(command) = receiver.recv() {
        match command {
            ReaderCommand::Search(command) => {
                let SearchCommand {
                    repository,
                    query,
                    limits,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result =
                    search_active(connection, repository, &query, limits, cancelled, deadline);
                let _ = reply.try_send(result);
            }
            ReaderCommand::GetSymbol(command) => {
                let SymbolCommand {
                    repository,
                    expected_snapshot,
                    expected_generation,
                    selector,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = get_active_symbol(
                    connection,
                    repository,
                    expected_snapshot,
                    expected_generation,
                    &selector,
                    cancelled,
                    deadline,
                );
                let _ = reply.try_send(result);
            }
            ReaderCommand::LoadArtifacts(command) => {
                let ArtifactCommand {
                    requested,
                    identity,
                    limits,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = load_reusable_artifacts(
                    connection, &requested, identity, limits, cancelled, deadline,
                );
                let _ = reply.try_send(result);
            }
            ReaderCommand::Shutdown { reply } => {
                let _ = reply.try_send(Ok(()));
                break;
            }
        }
    }
}

fn search_active(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    query: &str,
    limits: SearchLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<SearchResults, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = search_transaction(connection, repository, query, limits);
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    match result {
        Ok(results) => {
            check_control(&cancelled, deadline)?;
            Ok(results)
        }
        Err(SearchFailure::Sqlite(error)) if is_interrupted(&error) => {
            check_control(&cancelled, deadline)?;
            Err(SqliteStoreError::DatabaseOperationFailed)
        }
        Err(SearchFailure::Sqlite(_)) => Err(SqliteStoreError::DatabaseOperationFailed),
        Err(SearchFailure::Store(error)) => Err(error),
    }
}

fn get_active_symbol(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    expected_snapshot: SourceSnapshotDigest,
    expected_generation: GenerationId,
    selector: &SymbolGetSelector,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<SymbolLookupResults, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = symbol_transaction(
        connection,
        repository,
        expected_snapshot,
        expected_generation,
        selector,
    );
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    match result {
        Ok(results) => {
            check_control(&cancelled, deadline)?;
            Ok(results)
        }
        Err(SearchFailure::Sqlite(error)) if is_interrupted(&error) => {
            check_control(&cancelled, deadline)?;
            Err(SqliteStoreError::DatabaseOperationFailed)
        }
        Err(SearchFailure::Sqlite(_)) => Err(SqliteStoreError::DatabaseOperationFailed),
        Err(SearchFailure::Store(error)) => Err(error),
    }
}

fn load_reusable_artifacts(
    connection: &mut Connection,
    requested: &[AnalysisArtifactDigest],
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = artifact_transaction(
        connection, requested, identity, limits, &cancelled, deadline,
    );
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    match result {
        Ok(artifacts) => {
            check_control(&cancelled, deadline)?;
            Ok(artifacts)
        }
        Err(SearchFailure::Sqlite(error)) if is_interrupted(&error) => {
            check_control(&cancelled, deadline)?;
            Err(SqliteStoreError::DatabaseOperationFailed)
        }
        Err(SearchFailure::Sqlite(_)) => Err(SqliteStoreError::DatabaseOperationFailed),
        Err(SearchFailure::Store(error)) => Err(error),
    }
}

enum SearchFailure {
    Sqlite(rusqlite::Error),
    Store(SqliteStoreError),
}

impl From<rusqlite::Error> for SearchFailure {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

fn artifact_transaction(
    connection: &mut Connection,
    requested: &[AnalysisArtifactDigest],
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let mut artifacts = BTreeMap::new();
    let mut budget = ArtifactLoadBudget { facts: 0, bytes: 0 };
    let context = ArtifactReadContext {
        identity,
        limits,
        cancelled,
        deadline,
    };
    for requested_digest in requested {
        check_control(cancelled, deadline).map_err(SearchFailure::Store)?;
        let Some(artifact) =
            read_reusable_artifact(&transaction, *requested_digest, context, &mut budget)?
        else {
            continue;
        };
        if artifacts.insert(*requested_digest, artifact).is_some() {
            return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
        }
    }
    transaction.commit()?;
    Ok(artifacts)
}

type PersistedArtifactMetadata = (
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    i64,
    i64,
    i64,
    i64,
    Vec<u8>,
);

#[derive(Clone, Copy)]
struct ArtifactReadContext<'a> {
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

struct ArtifactLoadBudget {
    facts: u64,
    bytes: u64,
}

fn read_reusable_artifact(
    transaction: &Transaction<'_>,
    requested_digest: AnalysisArtifactDigest,
    context: ArtifactReadContext<'_>,
    budget: &mut ArtifactLoadBudget,
) -> Result<Option<RustSourceAnalysis>, SearchFailure> {
    check_control(context.cancelled, context.deadline).map_err(SearchFailure::Store)?;
    let persisted: Option<PersistedArtifactMetadata> = transaction
        .query_row(
            "SELECT source_content_digest, producer_manifest_digest,
                    configuration_digest, analysis_schema_digest,
                    canonicalization_version, fact_count, visited_nodes,
                    syntax_error_nodes, payload_digest
             FROM analysis_artifacts
             WHERE artifact_digest = ?1
               AND lifecycle_state = 'complete'
               AND payload_digest IS NOT NULL
               AND length(source_content_digest) = 32
               AND length(producer_manifest_digest) = 32
               AND length(configuration_digest) = 32
               AND length(analysis_schema_digest) = 32
               AND length(payload_digest) = 32",
            [requested_digest.as_bytes().as_slice()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )
        .optional()?;
    let Some(persisted) = persisted else {
        let eligible = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM analysis_artifacts
                 WHERE artifact_digest = ?1
                   AND lifecycle_state = 'complete'
                   AND payload_digest IS NOT NULL
             )",
            [requested_digest.as_bytes().as_slice()],
            |row| row.get::<_, i64>(0),
        )?;
        return if eligible == 0 {
            Ok(None)
        } else {
            Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
        };
    };
    let content_digest = SourceContentDigest::try_from_slice(&persisted.0)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    if persisted.1.as_slice() != context.identity.producer_manifest().as_bytes()
        || persisted.2.as_slice() != context.identity.configuration().as_bytes()
        || persisted.3.as_slice() != context.identity.schema().as_bytes()
        || u32::try_from(persisted.4).ok() != Some(context.identity.canonicalization_version())
    {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    let artifact_key = AnalysisArtifactKey::new(
        content_digest,
        context.identity.producer_manifest(),
        context.identity.configuration(),
        context.identity.schema(),
        context.identity.canonicalization_version(),
    );
    if hash_analysis_artifact_key(&artifact_key) != requested_digest {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    let expected_fact_count = persisted_nonnegative_u32(persisted.5)?;
    let visited_nodes = persisted_nonnegative_u32(persisted.6)?;
    let syntax_error_nodes = persisted_nonnegative_u32(persisted.7)?;
    if expected_fact_count > context.limits.per_file().max_symbol_facts()
        || visited_nodes == 0
        || visited_nodes > context.limits.per_file().max_syntax_nodes()
        || syntax_error_nodes > visited_nodes
    {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    let payload_digest = AnalysisArtifactPayloadDigest::try_from_slice(&persisted.8)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let facts = read_reusable_facts(
        transaction,
        requested_digest,
        expected_fact_count,
        context,
        budget,
    )?;
    let analysis = RustSourceAnalysis::try_from_parts(
        facts,
        visited_nodes,
        syntax_error_nodes,
        context.limits.per_file(),
    )
    .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    if hash_analysis_artifact_payload(&analysis) != payload_digest {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    Ok(Some(analysis))
}

fn read_reusable_facts(
    transaction: &Transaction<'_>,
    artifact_digest: AnalysisArtifactDigest,
    expected_count: u32,
    context: ArtifactReadContext<'_>,
    budget: &mut ArtifactLoadBudget,
) -> Result<Vec<RustSymbolFact>, SearchFailure> {
    let projected_total_facts =
        budget
            .facts
            .checked_add(u64::from(expected_count))
            .ok_or(SearchFailure::Store(
                SqliteStoreError::CountNotRepresentable,
            ))?;
    if projected_total_facts > context.limits.max_total_facts() {
        return Err(SearchFailure::Store(
            SqliteStoreError::ArtifactReuseLimitExceeded,
        ));
    }
    let capacity = usize::try_from(expected_count)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let mut statement = transaction.prepare(
        "SELECT ordinal, kind, name, qualified_name, name_start, name_end,
                declaration_start, declaration_end
         FROM artifact_facts
         WHERE artifact_digest = ?1
           AND length(CAST(kind AS BLOB)) BETWEEN 1 AND 16
           AND length(CAST(name AS BLOB)) BETWEEN 1 AND ?2
           AND length(CAST(qualified_name AS BLOB)) BETWEEN 1 AND ?3
         ORDER BY ordinal",
    )?;
    let mut rows = statement.query(params![
        artifact_digest.as_bytes().as_slice(),
        i64::from(context.limits.per_file().max_symbol_name_bytes()),
        i64::from(context.limits.per_file().max_qualified_name_bytes()),
    ])?;
    let mut facts = Vec::with_capacity(capacity);
    while let Some(row) = rows.next()? {
        check_control(context.cancelled, context.deadline).map_err(SearchFailure::Store)?;
        if facts.len() >= capacity {
            return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
        }
        let ordinal: i64 = row.get(0)?;
        let kind: String = row.get(1)?;
        let name: String = row.get(2)?;
        let qualified_name: String = row.get(3)?;
        let name_start: i64 = row.get(4)?;
        let name_end: i64 = row.get(5)?;
        let declaration_start: i64 = row.get(6)?;
        let declaration_end: i64 = row.get(7)?;
        let expected_ordinal = i64::try_from(facts.len())
            .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        if ordinal != expected_ordinal {
            return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
        }
        budget.bytes = checked_artifact_bytes(budget.bytes, &kind, &name, &qualified_name)
            .map_err(SearchFailure::Store)?;
        let kind = RustSymbolKind::from_stable_str(&kind)
            .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        let fact = RustSymbolFact::try_new(
            kind,
            name,
            qualified_name,
            persisted_span(name_start, name_end).map_err(SearchFailure::Store)?,
            persisted_span(declaration_start, declaration_end).map_err(SearchFailure::Store)?,
            context.limits.per_file(),
        )
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        facts.push(fact);
    }
    if facts.len() != capacity {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    budget.facts = projected_total_facts;
    Ok(facts)
}

fn persisted_nonnegative_u32(value: i64) -> Result<u32, SearchFailure> {
    u32::try_from(value).map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
}

fn checked_artifact_bytes(
    current: u64,
    kind: &str,
    name: &str,
    qualified_name: &str,
) -> Result<u64, SqliteStoreError> {
    let row_bytes = 72_u64
        .checked_add(u64::try_from(kind.len()).unwrap_or(u64::MAX))
        .and_then(|value| value.checked_add(u64::try_from(name.len()).unwrap_or(u64::MAX)))
        .and_then(|value| {
            value.checked_add(u64::try_from(qualified_name.len()).unwrap_or(u64::MAX))
        })
        .ok_or(SqliteStoreError::CountNotRepresentable)?;
    let total = current
        .checked_add(row_bytes)
        .ok_or(SqliteStoreError::CountNotRepresentable)?;
    if total > MAX_REUSABLE_ARTIFACT_BYTES {
        return Err(SqliteStoreError::ArtifactReuseLimitExceeded);
    }
    Ok(total)
}

fn search_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    query: &str,
    limits: SearchLimits,
) -> Result<SearchResults, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let state = active_search_state(&transaction, repository)?;
    let total_matches = transaction.query_row(
        state.sql.count,
        params![query, state.generation.get()],
        |row| row.get::<_, i64>(0),
    )?;
    let total_matches = persisted_count(total_matches)?;
    let (hits, output_bytes) = search_hits(
        &transaction,
        state.sql.search,
        query,
        state.generation,
        limits,
    )?;
    transaction.commit()?;
    Ok(SearchResults {
        snapshot: state.snapshot,
        generation: state.generation,
        producer_manifest: state.producer_manifest,
        index_coverage: state.index_coverage,
        hits,
        total_matches,
        output_bytes,
    })
}

fn symbol_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    expected_snapshot: SourceSnapshotDigest,
    expected_generation: GenerationId,
    selector: &SymbolGetSelector,
) -> Result<SymbolLookupResults, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let state = active_generation_state(&transaction, repository)?;
    if state.snapshot != expected_snapshot || state.generation != expected_generation {
        return Err(SearchFailure::Store(
            SqliteStoreError::GenerationUnavailable,
        ));
    }
    let hit = exact_symbol_hit(&transaction, state.generation, selector)?;
    transaction.commit()?;
    Ok(SymbolLookupResults {
        snapshot: state.snapshot,
        generation: state.generation,
        producer_manifest: state.producer_manifest,
        index_coverage: state.index_coverage,
        hit,
    })
}

#[derive(Clone, Copy)]
struct SearchProjectionSql {
    search: &'static str,
    count: &'static str,
}

#[derive(Clone, Copy)]
struct ActiveSearchState {
    snapshot: SourceSnapshotDigest,
    generation: GenerationId,
    producer_manifest: ProducerManifestDigest,
    index_coverage: RustIndexCoverage,
    sql: SearchProjectionSql,
}

#[derive(Clone, Copy)]
struct ActiveGenerationState {
    snapshot: SourceSnapshotDigest,
    generation: GenerationId,
    producer_manifest: ProducerManifestDigest,
    index_coverage: RustIndexCoverage,
}

fn active_search_state(
    transaction: &Transaction<'_>,
    repository: RepositoryIdentityDigest,
) -> Result<ActiveSearchState, SearchFailure> {
    let state = active_generation_state(transaction, repository)?;
    let projection_slot = transaction
        .query_row(
            "SELECT active_slot
             FROM search_projection_state
             WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let sql = match projection_slot {
        0 => SearchProjectionSql {
            search: PRIMARY_SEARCH_SQL,
            count: PRIMARY_COUNT_SQL,
        },
        1 => SearchProjectionSql {
            search: REBUILD_SEARCH_SQL,
            count: REBUILD_COUNT_SQL,
        },
        _ => {
            return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
        }
    };
    Ok(ActiveSearchState {
        snapshot: state.snapshot,
        generation: state.generation,
        producer_manifest: state.producer_manifest,
        index_coverage: state.index_coverage,
        sql,
    })
}

fn active_generation_state(
    transaction: &Transaction<'_>,
    repository: RepositoryIdentityDigest,
) -> Result<ActiveGenerationState, SearchFailure> {
    let persisted = transaction
        .query_row(
            "SELECT generation.generation_id, generation.snapshot_digest,
                    snapshot.producer_manifest_digest,
                    generation.searched_count, generation.skipped_count,
                    generation.unresolved_count, generation.truncated_count
             FROM workspaces AS workspace
             JOIN index_generations AS generation
               ON generation.generation_id = workspace.active_generation_id
              AND generation.workspace_id = workspace.workspace_id
              AND generation.lifecycle_state = 'active'
             JOIN source_snapshots AS snapshot
               ON snapshot.snapshot_digest = generation.snapshot_digest
              AND snapshot.lifecycle_state = 'complete'
             WHERE workspace.repository_identity = ?1
            ",
            [repository.as_bytes().as_slice()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )
        .optional()?
        .ok_or(SearchFailure::Store(
            SqliteStoreError::GenerationUnavailable,
        ))?;
    let (generation, snapshot, producer_manifest, searched, skipped, unresolved, truncated) =
        persisted;
    let snapshot = SourceSnapshotDigest::try_from_slice(&snapshot)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let producer_manifest = ProducerManifestDigest::try_from_slice(&producer_manifest)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    Ok(ActiveGenerationState {
        snapshot,
        generation: GenerationId::from_database(generation),
        producer_manifest,
        index_coverage: RustIndexCoverage::new(
            persisted_count(searched)?,
            persisted_count(skipped)?,
            persisted_count(unresolved)?,
            persisted_count(truncated)?,
        ),
    })
}

fn search_hits(
    transaction: &Transaction<'_>,
    search_sql: &str,
    query: &str,
    generation: GenerationId,
    limits: SearchLimits,
) -> Result<(Box<[SearchHit]>, u64), SearchFailure> {
    let mut statement = transaction.prepare(search_sql)?;
    let mut rows = statement.query(params![
        query,
        generation.get(),
        i64::from(limits.max_results())
    ])?;
    let mut hits = Vec::with_capacity(usize::from(limits.max_results()));
    let mut output_bytes = 0_u64;
    while let Some(row) = rows.next()? {
        let hit = read_search_hit(row)?;
        output_bytes = checked_output_bytes(
            output_bytes,
            &hit.path,
            hit.kind,
            &hit.name,
            &hit.qualified_name,
            limits.max_output_bytes(),
        )
        .map_err(SearchFailure::Store)?;
        hits.push(hit);
    }
    drop(rows);
    drop(statement);
    Ok((hits.into_boxed_slice(), output_bytes))
}

fn exact_symbol_hit(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    selector: &SymbolGetSelector,
) -> Result<Option<SearchHit>, SearchFailure> {
    let mut statement = transaction.prepare(
        "SELECT file.repository_path, fact.ordinal, fact.kind, fact.name,
                fact.qualified_name, file.content_digest, file.artifact_digest,
                fact.name_start, fact.name_end,
                fact.declaration_start, fact.declaration_end
         FROM generation_files AS file
         JOIN artifact_facts AS fact
           ON fact.artifact_digest = file.artifact_digest
         WHERE file.generation_id = ?1
           AND file.repository_path = ?2
           AND file.content_digest = ?3
           AND file.artifact_digest = ?4
           AND fact.ordinal = ?5",
    )?;
    let mut rows = statement.query(params![
        generation.get(),
        selector.path().as_bytes(),
        selector.content_digest().as_bytes().as_slice(),
        selector.artifact_digest().as_bytes().as_slice(),
        persisted_ordinal(selector.fact_ordinal())?,
    ])?;
    let hit = rows.next()?.map(read_search_hit).transpose()?;
    if rows.next()?.is_some() {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    drop(rows);
    drop(statement);
    Ok(hit)
}

fn read_search_hit(row: &rusqlite::Row<'_>) -> Result<SearchHit, SearchFailure> {
    let path_bytes: Vec<u8> = row.get(0)?;
    let fact_ordinal: i64 = row.get(1)?;
    let kind: String = row.get(2)?;
    let name: String = row.get(3)?;
    let qualified_name: String = row.get(4)?;
    let content_digest: Vec<u8> = row.get(5)?;
    let artifact_digest: Vec<u8> = row.get(6)?;
    let name_start: i64 = row.get(7)?;
    let name_end: i64 = row.get(8)?;
    let declaration_start: i64 = row.get(9)?;
    let declaration_end: i64 = row.get(10)?;
    let path = RepositoryPath::try_from_bytes(&path_bytes, PERSISTED_PATH_LIMITS)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let fact_ordinal = u64::try_from(fact_ordinal)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let kind = parse_symbol_kind(&kind)
        .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let content_digest = SourceContentDigest::try_from_slice(&content_digest)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let artifact_digest = AnalysisArtifactDigest::try_from_slice(&artifact_digest)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let name_span = persisted_span(name_start, name_end).map_err(SearchFailure::Store)?;
    let declaration_span =
        persisted_span(declaration_start, declaration_end).map_err(SearchFailure::Store)?;
    Ok(SearchHit {
        path,
        fact_ordinal,
        content_digest,
        artifact_digest,
        kind,
        name,
        qualified_name,
        name_span,
        declaration_span,
    })
}

fn persisted_ordinal(ordinal: u64) -> Result<i64, SearchFailure> {
    i64::try_from(ordinal).map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
}

const PRIMARY_SEARCH_SQL: &str = "SELECT repository_path, fact_ordinal, kind, name, qualified_name,
       content_digest, artifact_digest, name_start, name_end,
       declaration_start, declaration_end,
       bm25(
           generation_search,
           0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
           0.0, 0.0, 0.0, 1.0, 3.0, 2.0
       ) AS rank
FROM generation_search
WHERE generation_search MATCH ?1 AND generation_id = ?2
ORDER BY rank ASC, repository_path ASC, fact_ordinal ASC
LIMIT ?3";

const REBUILD_SEARCH_SQL: &str = "SELECT repository_path, fact_ordinal, kind, name, qualified_name,
            content_digest, artifact_digest, name_start, name_end,
            declaration_start, declaration_end,
            bm25(
                generation_search_rebuild,
                0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 1.0, 3.0, 2.0
            ) AS rank
     FROM generation_search_rebuild
     WHERE generation_search_rebuild MATCH ?1 AND generation_id = ?2
     ORDER BY rank ASC, repository_path ASC, fact_ordinal ASC
     LIMIT ?3";

const PRIMARY_COUNT_SQL: &str = "SELECT COUNT(*)
FROM generation_search
WHERE generation_search MATCH ?1 AND generation_id = ?2";

const REBUILD_COUNT_SQL: &str = "SELECT COUNT(*)
FROM generation_search_rebuild
WHERE generation_search_rebuild MATCH ?1 AND generation_id = ?2";

fn literal_fts_query(query: &str) -> Result<String, SqliteStoreError> {
    if query.is_empty() || query.len() > MAX_QUERY_BYTES {
        return Err(SqliteStoreError::InvalidSearchQuery);
    }
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() || terms.len() > MAX_QUERY_TERMS {
        return Err(SqliteStoreError::InvalidSearchQuery);
    }
    let mut output = String::with_capacity(query.len().saturating_mul(2).saturating_add(16));
    for (index, term) in terms.into_iter().enumerate() {
        if term.len() > MAX_TERM_BYTES {
            return Err(SqliteStoreError::InvalidSearchQuery);
        }
        if index != 0 {
            output.push(' ');
        }
        output.push('"');
        for character in term.chars() {
            if character == '"' {
                output.push('"');
            }
            output.push(character);
        }
        output.push('"');
    }
    Ok(output)
}

fn checked_output_bytes(
    current: u64,
    path: &RepositoryPath,
    kind: RustSymbolKind,
    name: &str,
    qualified_name: &str,
    limit: u64,
) -> Result<u64, SqliteStoreError> {
    let row_bytes = path
        .byte_count()
        .get()
        .checked_add(104)
        .and_then(|value| value.checked_add(u64::try_from(kind.as_str().len()).unwrap_or(u64::MAX)))
        .and_then(|value| value.checked_add(u64::try_from(name.len()).unwrap_or(u64::MAX)))
        .and_then(|value| {
            value.checked_add(u64::try_from(qualified_name.len()).unwrap_or(u64::MAX))
        })
        .ok_or(SqliteStoreError::CountNotRepresentable)?;
    let total = current
        .checked_add(row_bytes)
        .ok_or(SqliteStoreError::CountNotRepresentable)?;
    if total > limit {
        return Err(SqliteStoreError::SearchOutputLimitExceeded);
    }
    Ok(total)
}

fn persisted_span(start: i64, end: i64) -> Result<ByteSpan, SqliteStoreError> {
    let start = u64::try_from(start).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let end = u64::try_from(end).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    ByteSpan::try_new(ByteOffset::new(start), ByteOffset::new(end))
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)
}

fn persisted_count(value: i64) -> Result<u64, SearchFailure> {
    u64::try_from(value).map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
}

fn parse_symbol_kind(kind: &str) -> Option<RustSymbolKind> {
    RustSymbolKind::from_stable_str(kind)
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), SqliteStoreError> {
    if cancelled.load(Ordering::Acquire) {
        Err(SqliteStoreError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(SqliteStoreError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

fn is_interrupted(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(code, _) if code.code == ErrorCode::OperationInterrupted
    )
}

fn receive_reply<T>(
    receiver: &Receiver<Result<T, SqliteStoreError>>,
    deadline: Instant,
) -> Result<T, SqliteStoreError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(SqliteStoreError::DeadlineExceeded);
    }
    receiver
        .recv_timeout(remaining)
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => SqliteStoreError::ReplyTimeout,
            mpsc::RecvTimeoutError::Disconnected => SqliteStoreError::WorkerUnavailable,
        })?
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant},
    };

    use repowitness_application::{
        CodeSearchLimits, CodeSearchQuery, CodeSearchRequest, ImmutableRustSource,
        PreparedRustIndex, RustArtifactIdentity, RustIndexLimits, RustSourceSnapshotIdentity,
        SymbolGetSelector, code_search, hash_rust_source_snapshot, prepare_rust_index,
    };
    use repowitness_domain::{
        AnalysisSchemaDigest, ConfigurationDigest, GitStateDigest, ProducerManifestDigest,
        RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits, WorktreeStateDigest,
    };
    use rusqlite::{Connection, params};

    use crate::{GenerationCoverage, OwnedSqliteIndex};

    use super::{OwnedSqliteReader, ReaderCommand, SearchLimits, SqliteStoreError};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
    const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(4096, 256);

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "repowitness-owned-reader-{}-{ordinal}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("fixture directory should be created");
            Self(path)
        }

        fn database(&self) -> PathBuf {
            self.0.join("index.sqlite3")
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn deadline() -> Instant {
        Instant::now()
            .checked_add(Duration::from_secs(5))
            .expect("test deadline should be representable")
    }

    #[test]
    fn saturated_drop_detaches_instead_of_waiting_without_shutdown() {
        let (commands, _receiver) = mpsc::sync_channel(1);
        let (reply, _reply_receiver) = mpsc::sync_channel(1);
        commands
            .send(ReaderCommand::Shutdown { reply })
            .expect("fixture queue should accept one command");
        let worker = thread::spawn(|| thread::sleep(Duration::from_millis(500)));
        let reader = OwnedSqliteReader {
            commands,
            worker: Some(worker),
        };

        let started = Instant::now();
        drop(reader);

        assert!(started.elapsed() < Duration::from_millis(250));
    }

    fn identity() -> RustSourceSnapshotIdentity {
        RustSourceSnapshotIdentity::new(
            RepositoryIdentityDigest::new([1; 32]),
            GitStateDigest::new([2; 32]),
            WorktreeStateDigest::new([3; 32]),
            ConfigurationDigest::new([4; 32]),
            ProducerManifestDigest::new([5; 32]),
            AnalysisSchemaDigest::new([6; 32]),
            7,
        )
    }

    fn artifact_identity() -> RustArtifactIdentity {
        RustArtifactIdentity::new(
            identity().producer_manifest(),
            identity().configuration(),
            identity().analysis_schema(),
            identity().canonicalization_version(),
        )
    }

    fn prepared(version: u8) -> PreparedRustIndex {
        let first = if version == 1 {
            b"pub fn old_generation_only() {}\npub fn shared_token() {}\n".as_slice()
        } else {
            b"pub fn new_generation_only() {}\npub fn shared_token() {}\n".as_slice()
        };
        let cancelled = AtomicBool::new(false);
        prepare_rust_index(
            vec![
                ImmutableRustSource::new(
                    RepositoryPath::try_from_bytes(b"src/a.rs", PATH_LIMITS)
                        .expect("fixture path should be valid"),
                    first.to_vec().into_boxed_slice(),
                ),
                ImmutableRustSource::new(
                    RepositoryPath::try_from_bytes(b"src/b.rs", PATH_LIMITS)
                        .expect("fixture path should be valid"),
                    b"pub fn shared_token() {}\n".to_vec().into_boxed_slice(),
                ),
            ],
            artifact_identity(),
            RustIndexLimits::default(),
            &cancelled,
            deadline(),
        )
        .expect("fixture index should prepare")
    }

    fn publish(
        writer: &OwnedSqliteIndex,
        epoch: u64,
        prepared: PreparedRustIndex,
    ) -> super::GenerationId {
        let generation = writer
            .stage(
                epoch,
                identity(),
                prepared,
                GenerationCoverage::new(2, 0, 0, 0),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("generation should stage");
        writer
            .activate(generation, epoch, deadline())
            .expect("generation should activate");
        generation
    }

    #[test]
    fn reusable_artifacts_are_exact_bounded_and_cancelled_without_partial_output() {
        let directory = TempDirectory::new();
        let database = directory.database();
        let prepared = prepared(1);
        let expected = prepared
            .files()
            .iter()
            .map(|file| (file.artifact_digest(), file.analysis().clone()))
            .collect::<BTreeMap<_, _>>();
        let requested = expected.keys().copied().collect::<Vec<_>>();
        let (writer, _) =
            OwnedSqliteIndex::start(&database, 123, deadline()).expect("writer should start");
        writer
            .register_workspace(identity().repository(), 0, deadline())
            .expect("workspace should register");
        publish(&writer, 0, prepared);
        writer.shutdown(deadline()).expect("writer should stop");

        let reader = OwnedSqliteReader::start(&database, deadline()).expect("reader should start");
        let actual = reader
            .load_reusable_artifacts(
                &requested,
                artifact_identity(),
                RustIndexLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("exact artifacts should load");
        assert_eq!(actual, expected);

        assert_eq!(
            reader.load_reusable_artifacts(
                &requested,
                artifact_identity(),
                RustIndexLimits::default(),
                Arc::new(AtomicBool::new(true)),
                deadline(),
            ),
            Err(SqliteStoreError::Cancelled)
        );
        assert_eq!(
            reader.load_reusable_artifacts(
                &requested,
                artifact_identity(),
                RustIndexLimits::default(),
                Arc::new(AtomicBool::new(false)),
                Instant::now(),
            ),
            Err(SqliteStoreError::DeadlineExceeded)
        );

        let duplicate = requested
            .first()
            .copied()
            .map(|digest| [digest, digest])
            .expect("fixture must contain an artifact");
        assert_eq!(
            reader.load_reusable_artifacts(
                &duplicate,
                artifact_identity(),
                RustIndexLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            ),
            Err(SqliteStoreError::IntegrityCheckFailed)
        );
        reader.shutdown(deadline()).expect("reader should stop");
    }

    #[test]
    fn reusable_artifact_payload_corruption_fails_closed() {
        let directory = TempDirectory::new();
        let database = directory.database();
        let prepared = prepared(1);
        let requested = prepared
            .files()
            .iter()
            .map(|file| file.artifact_digest())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let corrupted = requested[0];
        let (writer, _) =
            OwnedSqliteIndex::start(&database, 123, deadline()).expect("writer should start");
        writer
            .register_workspace(identity().repository(), 0, deadline())
            .expect("workspace should register");
        publish(&writer, 0, prepared);
        writer.shutdown(deadline()).expect("writer should stop");

        let connection = Connection::open(&database).expect("fixture database should open");
        connection
            .execute_batch("DROP TRIGGER artifact_facts_no_update")
            .expect("fixture immutability trigger should be removed");
        connection
            .execute(
                "UPDATE artifact_facts SET name = 'corrupt'
                 WHERE artifact_digest = ?1 AND ordinal = 0",
                params![corrupted.as_bytes().as_slice()],
            )
            .expect("fixture fact should be corrupted");
        drop(connection);

        let reader = OwnedSqliteReader::start(&database, deadline()).expect("reader should start");
        assert_eq!(
            reader.load_reusable_artifacts(
                &requested,
                artifact_identity(),
                RustIndexLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            ),
            Err(SqliteStoreError::IntegrityCheckFailed)
        );
        reader.shutdown(deadline()).expect("reader should stop");
    }

    #[test]
    fn reusable_artifact_metadata_is_bounded_before_allocation() {
        let directory = TempDirectory::new();
        let database = directory.database();
        let prepared = prepared(1);
        let requested = prepared
            .files()
            .iter()
            .map(|file| file.artifact_digest())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let corrupted = requested[0];
        let (writer, _) =
            OwnedSqliteIndex::start(&database, 123, deadline()).expect("writer should start");
        writer
            .register_workspace(identity().repository(), 0, deadline())
            .expect("workspace should register");
        publish(&writer, 0, prepared);
        writer.shutdown(deadline()).expect("writer should stop");

        let connection = Connection::open(&database).expect("fixture database should open");
        connection
            .execute_batch("DROP TRIGGER analysis_artifacts_no_semantic_update")
            .expect("fixture immutability trigger should be removed");
        connection
            .execute(
                "UPDATE analysis_artifacts SET fact_count = 2147483647
                 WHERE artifact_digest = ?1",
                params![corrupted.as_bytes().as_slice()],
            )
            .expect("fixture fact count should be corrupted");
        drop(connection);

        let reader = OwnedSqliteReader::start(&database, deadline()).expect("reader should start");
        assert_eq!(
            reader.load_reusable_artifacts(
                &requested,
                artifact_identity(),
                RustIndexLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            ),
            Err(SqliteStoreError::IntegrityCheckFailed)
        );
        reader.shutdown(deadline()).expect("reader should stop");
    }

    #[test]
    fn reader_pins_active_generation_and_orders_equal_hits_by_exact_path() {
        let directory = TempDirectory::new();
        let (writer, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
            .expect("writer should start");
        writer
            .register_workspace(identity().repository(), 0, deadline())
            .expect("workspace should register");
        let first_prepared = prepared(1);
        let first_snapshot =
            hash_rust_source_snapshot(identity(), first_prepared.manifest_digest());
        let first = publish(&writer, 0, first_prepared);
        let reader = OwnedSqliteReader::start(&directory.database(), deadline())
            .expect("reader should start");

        let results = reader
            .search(
                identity().repository(),
                "shared_token",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("active generation should be searchable");
        assert_eq!(results.generation(), first);
        assert_eq!(results.snapshot(), first_snapshot);
        assert_eq!(results.producer_manifest(), identity().producer_manifest());
        assert_eq!(
            results.index_coverage(),
            GenerationCoverage::new(2, 0, 0, 0)
        );
        assert_eq!(results.hits().len(), 2);
        assert_eq!(results.total_matches(), 2);
        assert_eq!(results.hits()[0].path().as_bytes(), b"src/a.rs");
        assert_eq!(results.hits()[1].path().as_bytes(), b"src/b.rs");

        let material = code_search(
            &reader,
            CodeSearchRequest::new(
                identity().repository(),
                CodeSearchQuery::try_new("shared_token").expect("query should be valid"),
                CodeSearchLimits::try_new(1, 64 * 1024).expect("limits should be valid"),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            ),
        )
        .expect("shared application search should succeed");
        assert_eq!(material.claim().returned_matches(), 1);
        assert_eq!(material.claim().total_matches(), 2);
        assert_eq!(material.coverage().truncated().get(), 1);
        assert_eq!(material.snapshot(), &first_snapshot);
        assert_eq!(material.generation(), &first);

        writer
            .advance_source_epoch(identity().repository(), 0, 1, deadline())
            .expect("source epoch should advance");
        let second = publish(&writer, 1, prepared(2));
        let current = reader
            .search(
                identity().repository(),
                "new_generation_only",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("new active generation should be searchable");
        assert_eq!(current.generation(), second);
        assert_eq!(current.hits().len(), 1);
        let old = reader
            .search(
                identity().repository(),
                "old_generation_only",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("retained generations should not leak into search");
        assert_eq!(old.generation(), second);
        assert!(old.hits().is_empty());

        reader.shutdown(deadline()).expect("reader should stop");
        writer.shutdown(deadline()).expect("writer should stop");
    }

    #[test]
    fn exact_lookup_requires_active_context_and_missing_occurrences_remain_explicit() {
        let directory = TempDirectory::new();
        let (writer, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
            .expect("writer should start");
        writer
            .register_workspace(identity().repository(), 0, deadline())
            .expect("workspace should register");
        let first_prepared = prepared(1);
        let first_snapshot =
            hash_rust_source_snapshot(identity(), first_prepared.manifest_digest());
        let first = publish(&writer, 0, first_prepared);
        let reader = OwnedSqliteReader::start(&directory.database(), deadline())
            .expect("reader should start");
        let search = reader
            .search(
                identity().repository(),
                "shared_token",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("fixture occurrence should be searchable");
        let first_hit = &search.hits()[0];
        let selector = SymbolGetSelector::new(
            first_hit.path().clone(),
            first_hit.content_digest(),
            first_hit.artifact_digest(),
            first_hit.fact_ordinal(),
        );
        let exact = reader
            .get_symbol(
                identity().repository(),
                first_snapshot,
                first,
                selector.clone(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("exact active occurrence should resolve");
        assert_eq!(exact.snapshot(), first_snapshot);
        assert_eq!(exact.generation(), first);
        assert_eq!(exact.producer_manifest(), identity().producer_manifest());
        assert_eq!(
            exact
                .hit()
                .expect("exact occurrence should exist")
                .qualified_name(),
            "shared_token"
        );

        writer
            .advance_source_epoch(identity().repository(), 0, 1, deadline())
            .expect("source epoch should advance");
        let second_prepared = prepared(2);
        let second_snapshot =
            hash_rust_source_snapshot(identity(), second_prepared.manifest_digest());
        let second = publish(&writer, 1, second_prepared);
        assert_eq!(
            reader
                .get_symbol(
                    identity().repository(),
                    first_snapshot,
                    first,
                    selector,
                    Arc::new(AtomicBool::new(false)),
                    deadline(),
                )
                .expect_err("stale source context must not silently retarget"),
            SqliteStoreError::GenerationUnavailable
        );
        let missing = reader
            .get_symbol(
                identity().repository(),
                second_snapshot,
                second,
                SymbolGetSelector::new(
                    RepositoryPath::try_from_bytes(b"src/a.rs", PATH_LIMITS)
                        .expect("fixture path should be valid"),
                    repowitness_domain::SourceContentDigest::new([9; 32]),
                    repowitness_domain::AnalysisArtifactDigest::new([9; 32]),
                    999,
                ),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("missing exact occurrence should be a bounded result");
        assert!(missing.hit().is_none());

        reader.shutdown(deadline()).expect("reader should stop");
        writer.shutdown(deadline()).expect("writer should stop");
    }

    #[test]
    fn hostile_syntax_is_literal_and_query_result_and_control_bounds_fail_closed() {
        let directory = TempDirectory::new();
        let (writer, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
            .expect("writer should start");
        writer
            .register_workspace(identity().repository(), 0, deadline())
            .expect("workspace should register");
        publish(&writer, 0, prepared(1));
        let reader = OwnedSqliteReader::start(&directory.database(), deadline())
            .expect("reader should start");

        let hostile = reader
            .search(
                identity().repository(),
                "shared_token OR old_generation_only*",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("hostile-looking syntax should remain a literal query");
        assert!(hostile.hits().is_empty());
        assert_eq!(
            SearchLimits::try_new(0, 1).expect_err("zero results should fail"),
            SqliteStoreError::InvalidSearchLimits
        );
        assert_eq!(
            reader
                .search(
                    identity().repository(),
                    "",
                    SearchLimits::default(),
                    Arc::new(AtomicBool::new(false)),
                    deadline(),
                )
                .expect_err("empty query should fail"),
            SqliteStoreError::InvalidSearchQuery
        );
        assert_eq!(
            reader
                .search(
                    identity().repository(),
                    "shared_token",
                    SearchLimits::try_new(10, 1).expect("tiny output bound is valid"),
                    Arc::new(AtomicBool::new(false)),
                    deadline(),
                )
                .expect_err("output should exceed one byte"),
            SqliteStoreError::SearchOutputLimitExceeded
        );
        assert_eq!(
            reader
                .search(
                    identity().repository(),
                    "shared_token",
                    SearchLimits::default(),
                    Arc::new(AtomicBool::new(true)),
                    deadline(),
                )
                .expect_err("pre-cancelled search should fail"),
            SqliteStoreError::Cancelled
        );

        reader.shutdown(deadline()).expect("reader should stop");
        writer.shutdown(deadline()).expect("writer should stop");
    }
}
