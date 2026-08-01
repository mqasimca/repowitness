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
    RUST_CORRESPONDENCE_PROFILE_ID, RUST_CORRESPONDENCE_PROFILE_VERSION,
    RUST_GRAPH_SITE_PROFILE_VERSION, RustAnalysisLimits, RustGraphAnalysisControl,
    RustGraphAnalysisError, RustGraphAnalysisLimits, RustGraphEnclosingDefinition, RustGraphSite,
    RustGraphSiteAnalysis, RustGraphSiteKind, RustGraphSiteOrdinal, RustOccurrenceFingerprint,
    RustSourceAnalysis, RustSymbolFact, RustSymbolKind,
};
use repowitness_application::{
    CodeSearchCandidate, CodeSearchLimits, CodeSearchPort, CodeSearchPortResult, CodeSearchQuery,
    MemoryRecallLimits, MemoryRecallPort, MemoryRecallPortResult, MemoryRecallQuery, PackageScope,
    RepositoryDiagnosticsPort, RepositoryDiagnosticsPortResult, RustArtifactIdentity,
    RustIndexCoverage, RustIndexLimits, RustSymbolOccurrence, SourceArtifactEvidence,
    SourceLanguage, SymbolGetSelector, hash_analysis_artifact_key, hash_analysis_artifact_payload,
};
use repowitness_domain::{
    AnalysisArtifactDigest, AnalysisArtifactKey, AnalysisArtifactPayloadDigest, ByteOffset,
    ByteSpan, CanonicalMemoryDigest, CorrespondenceFingerprintDigest, DeclarationDigest,
    MemoryCommitId, MemoryLifecycle, MemoryObservationSource, MemoryRecordId, PersonalMemoryId,
    PersonalMemoryKind, PersonalMemoryProfileId, PersonalMemoryRecord, PersonalMemoryRevision,
    ProducerManifestDigest, RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits,
    ScipSymbol, SourceContentDigest, SourceSnapshotDigest, TaskId, TaskState, TaskStatus, TaskText,
};
use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params,
};

use super::{
    GenerationId, PinnedWorkspaceView, SqliteStoreError,
    graph::{RustGraphPreparationControl, RustGraphPreparationError},
    memory_reader::recall_active_memory,
    open_index_reader,
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
type GraphArtifactReply =
    SyncSender<Result<BTreeMap<AnalysisArtifactDigest, RustGraphSiteAnalysis>, SqliteStoreError>>;
type MemoryRecallReply =
    SyncSender<Result<MemoryRecallPortResult<GenerationId, i64>, SqliteStoreError>>;
type DiagnosticsReply =
    SyncSender<Result<RepositoryDiagnosticsPortResult<GenerationId, i64>, SqliteStoreError>>;
type WorkspaceViewReply = SyncSender<Result<Option<PinnedWorkspaceView>, SqliteStoreError>>;
type ScipOverlayReply = SyncSender<Result<ScipOverlayAvailability, SqliteStoreError>>;
type ScipEvidenceReply = SyncSender<Result<ScipSymbolEvidenceResult, SqliteStoreError>>;
type ScipSyntaxSymbolReply = SyncSender<Result<ScipSyntaxSymbolResolution, SqliteStoreError>>;
type ScipImportScopeReply = SyncSender<Result<ScipOverlayImportScope, SqliteStoreError>>;
type HistoryEvidenceReply = SyncSender<Result<Vec<GitHistoryEvidence>, SqliteStoreError>>;
type KnownAtHistoryReceiptReply = SyncSender<Result<KnownAtHistoryReceipt, SqliteStoreError>>;
type TaskStatusReply = SyncSender<Result<Option<TaskStatus>, SqliteStoreError>>;
type TaskStatusesReply = SyncSender<Result<Vec<TaskStatus>, SqliteStoreError>>;
type PersonalMemoryReadReply = SyncSender<Result<Vec<PersonalMemoryRecord>, SqliteStoreError>>;

enum ReaderCommand {
    Search(Box<SearchCommand>),
    WorkspaceSearch(Box<WorkspaceSearchCommand>),
    GetSymbol(Box<SymbolCommand>),
    LoadArtifacts(Box<ArtifactCommand>),
    LoadGraphArtifacts(Box<GraphArtifactCommand>),
    RecallMemory(Box<MemoryRecallCommand>),
    HistoryEvidence(Box<HistoryEvidenceCommand>),
    KnownAtHistoryEvidence(Box<KnownAtHistoryEvidenceCommand>),
    KnownAtHistoryReceipt(Box<KnownAtHistoryReceiptCommand>),
    PersonalMemoryRead(Box<PersonalMemoryReadCommand>),
    TaskStatus(Box<TaskStatusCommand>),
    TaskStatuses(Box<TaskStatusesCommand>),
    Diagnostics(Box<DiagnosticsCommand>),
    WorkspaceView(Box<WorkspaceViewCommand>),
    ScipOverlayStatus(Box<ScipOverlayStatusCommand>),
    ScipSymbolEvidence(Box<ScipSymbolEvidenceCommand>),
    ScipSyntaxSymbol(Box<ScipSyntaxSymbolCommand>),
    ScipImportScope(Box<ScipImportScopeCommand>),
    Graph(Box<GraphCommand>),
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

struct WorkspaceSearchCommand {
    view: PinnedWorkspaceView,
    source_slot: repowitness_domain::SourceSlotId,
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

struct GraphArtifactCommand {
    requested: Box<[AnalysisArtifactDigest]>,
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    graph_limits: RustGraphAnalysisLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: GraphArtifactReply,
}

struct MemoryRecallCommand {
    repository: RepositoryIdentityDigest,
    query: Option<String>,
    limits: MemoryRecallLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: MemoryRecallReply,
}

struct HistoryEvidenceCommand {
    repository: RepositoryIdentityDigest,
    expected_snapshot: SourceSnapshotDigest,
    expected_generation: GenerationId,
    expected_source_epoch: u64,
    max_results: u16,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: HistoryEvidenceReply,
}

struct KnownAtHistoryEvidenceCommand {
    repository: RepositoryIdentityDigest,
    known_at_unix_ms: repowitness_domain::MemoryRecordedAtUnixMillis,
    max_results: u16,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: HistoryEvidenceReply,
}

struct KnownAtHistoryReceiptCommand {
    repository: RepositoryIdentityDigest,
    known_at_unix_ms: repowitness_domain::MemoryRecordedAtUnixMillis,
    target: MemoryObservationSource,
    max_results: u16,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: KnownAtHistoryReceiptReply,
}

struct PersonalMemoryReadCommand {
    profile: PersonalMemoryProfileId,
    repository: RepositoryIdentityDigest,
    limit: u16,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: PersonalMemoryReadReply,
}

struct TaskStatusCommand {
    repository: RepositoryIdentityDigest,
    task_id: TaskId,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: TaskStatusReply,
}

struct TaskStatusesCommand {
    repository: RepositoryIdentityDigest,
    limit: u16,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: TaskStatusesReply,
}

/// One immutable Git observation for a locally approved current-memory version.
///
/// The commit identifies historical provenance only; it does not assert that the
/// commit remains reachable or that its source contents have been re-read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GitHistoryEvidence {
    record_id: MemoryRecordId,
    revision: CanonicalMemoryDigest,
    commit: MemoryCommitId,
}

impl GitHistoryEvidence {
    const fn new(
        record_id: MemoryRecordId,
        revision: CanonicalMemoryDigest,
        commit: MemoryCommitId,
    ) -> Self {
        Self {
            record_id,
            revision,
            commit,
        }
    }

    /// Returns the exact locally approved memory record identity.
    #[must_use]
    pub const fn record_id(self) -> MemoryRecordId {
        self.record_id
    }

    /// Returns the exact immutable memory revision observed at the commit.
    #[must_use]
    pub const fn revision(self) -> CanonicalMemoryDigest {
        self.revision
    }

    /// Returns the immutable Git observation receipt.
    #[must_use]
    pub const fn commit(self) -> MemoryCommitId {
        self.commit
    }
}

/// Whether a bounded historical journal query returned every matching receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnownAtHistoryCoverage {
    /// Every matching immutable journal event fit within the requested bound.
    Complete,
    /// At least one matching immutable journal event was omitted by the bound.
    Truncated,
}

/// The result of applying immutable recorded-time evidence to one concrete
/// target. Worktree snapshots are checked against retained immutable source
/// snapshots; Git reachability remains an adapter-owned I/O fence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnownAtApplicability {
    /// The exact target could not be evaluated from retained local evidence.
    Unavailable,
    /// The concrete target was available, but no approved evidence applied at
    /// the recorded-time cutoff.
    NotApplicable,
    /// The concrete target was retained and at least one approved observation
    /// or non-conflicted correspondence review applied at the cutoff.
    Applicable,
}

/// The immutable evidence relation that made a historical memory revision
/// applicable to the exact target.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum KnownAtEvidenceBasis {
    /// The approved revision was observed directly at the requested target.
    Observation,
    /// A trusted archival correspondence review linked the revision to a
    /// retained target snapshot without a conflicting rejection at the cutoff.
    ReviewedCorrespondence,
}

/// One immutable observation of an approved memory version at a concrete source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KnownAtObservationEvidence {
    record_id: MemoryRecordId,
    revision: CanonicalMemoryDigest,
    source: MemoryObservationSource,
    basis: KnownAtEvidenceBasis,
}

impl KnownAtObservationEvidence {
    const fn new(
        record_id: MemoryRecordId,
        revision: CanonicalMemoryDigest,
        source: MemoryObservationSource,
        basis: KnownAtEvidenceBasis,
    ) -> Self {
        Self {
            record_id,
            revision,
            source,
            basis,
        }
    }

    /// Returns the exact memory record identity observed at the target.
    #[must_use]
    pub const fn record_id(self) -> MemoryRecordId {
        self.record_id
    }

    /// Returns the immutable memory revision observed at the target.
    #[must_use]
    pub const fn revision(self) -> CanonicalMemoryDigest {
        self.revision
    }

    /// Returns the exact committed or worktree source receipt.
    #[must_use]
    pub const fn source(self) -> MemoryObservationSource {
        self.source
    }

    /// Returns whether this exact relation was an observation or a reviewed
    /// archival correspondence link.
    #[must_use]
    pub const fn basis(self) -> KnownAtEvidenceBasis {
        self.basis
    }
}

/// Bounded, deterministic evidence of what the append-only journal and
/// retained correspondence audit knew at a recorded-time cutoff about one
/// exact Git commit or worktree snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnownAtHistoryReceipt {
    known_at_unix_ms: repowitness_domain::MemoryRecordedAtUnixMillis,
    target: MemoryObservationSource,
    evidence: Vec<KnownAtObservationEvidence>,
    coverage: KnownAtHistoryCoverage,
    applicability: KnownAtApplicability,
}

impl KnownAtHistoryReceipt {
    pub(crate) fn new(
        known_at_unix_ms: repowitness_domain::MemoryRecordedAtUnixMillis,
        target: MemoryObservationSource,
        evidence: Vec<KnownAtObservationEvidence>,
        coverage: KnownAtHistoryCoverage,
        applicability: KnownAtApplicability,
    ) -> Self {
        Self {
            known_at_unix_ms,
            target,
            evidence,
            coverage,
            applicability,
        }
    }

    /// Returns the inclusive immutable-journal recorded-time cutoff.
    #[must_use]
    pub const fn known_at_unix_ms(&self) -> repowitness_domain::MemoryRecordedAtUnixMillis {
        self.known_at_unix_ms
    }

    /// Returns the exact Git commit or source snapshot that was queried.
    #[must_use]
    pub const fn target(&self) -> MemoryObservationSource {
        self.target
    }

    /// Returns only approved observations and non-conflicted correspondence
    /// reviews recorded at or before the cutoff.
    #[must_use]
    pub fn evidence(&self) -> &[KnownAtObservationEvidence] {
        &self.evidence
    }

    /// Returns whether the requested bound omitted matching observations.
    #[must_use]
    pub const fn coverage(&self) -> KnownAtHistoryCoverage {
        self.coverage
    }

    /// Returns the independently evaluated target applicability result.
    #[must_use]
    pub const fn applicability(&self) -> KnownAtApplicability {
        self.applicability
    }

    /// Applies the result of an adapter-owned Git object fence. This is crate
    /// internal so no caller can manufacture an applicability claim without
    /// performing the bounded local check.
    pub(crate) fn with_git_object_availability(mut self, available: bool) -> Self {
        if available {
            self.applicability = if self.evidence.is_empty() {
                KnownAtApplicability::NotApplicable
            } else {
                KnownAtApplicability::Applicable
            };
        }
        self
    }
}

struct DiagnosticsCommand {
    repository: RepositoryIdentityDigest,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: DiagnosticsReply,
}

struct WorkspaceViewCommand {
    connected_workspace: repowitness_domain::ConnectedWorkspaceId,
    requested_view: Option<i64>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: WorkspaceViewReply,
}

struct ScipOverlayStatusCommand {
    view: PinnedWorkspaceView,
    source_slot: repowitness_domain::SourceSlotId,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: ScipOverlayReply,
}

struct ScipSymbolEvidenceCommand {
    view: PinnedWorkspaceView,
    source_slot: repowitness_domain::SourceSlotId,
    package_scope: PackageScope,
    symbol: ScipSymbol,
    limits: ScipEvidenceReadLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: ScipEvidenceReply,
}

struct ScipSyntaxSymbolCommand {
    view: PinnedWorkspaceView,
    source_slot: repowitness_domain::SourceSlotId,
    path: RepositoryPath,
    content: SourceContentDigest,
    name_span: ByteSpan,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: ScipSyntaxSymbolReply,
}

struct ScipImportScopeCommand {
    view: PinnedWorkspaceView,
    source_slot: repowitness_domain::SourceSlotId,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: ScipImportScopeReply,
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

    /// Searches one exact source-slot member of a pinned active workspace view.
    pub fn search_workspace_member(
        &self,
        view: &PinnedWorkspaceView,
        source_slot: repowitness_domain::SourceSlotId,
        query: &str,
        limits: SearchLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<SearchResults, SqliteStoreError> {
        if !view
            .members()
            .iter()
            .any(|member| member.source_slot() == source_slot)
        {
            return Err(SqliteStoreError::InvalidWorkspaceView);
        }
        let canonical_query = literal_fts_query(query)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::WorkspaceSearch(Box::new(WorkspaceSearchCommand {
                view: view.clone(),
                source_slot,
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

    /// Reads bounded immutable Git observations only for current memory versions
    /// that were locally approved in the active projection matching the exact
    /// requested source fence.
    #[allow(
        clippy::too_many_arguments,
        reason = "the projection source fence, bound, and controls are independent trust inputs"
    )]
    pub fn trusted_git_history_evidence(
        &self,
        repository: RepositoryIdentityDigest,
        expected_snapshot: SourceSnapshotDigest,
        expected_generation: GenerationId,
        expected_source_epoch: u64,
        max_results: u16,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Vec<GitHistoryEvidence>, SqliteStoreError> {
        if max_results == 0 || max_results > MAX_RESULTS {
            return Err(SqliteStoreError::InvalidSearchLimits);
        }
        check_control(&cancelled, deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::HistoryEvidence(Box::new(HistoryEvidenceCommand {
                repository,
                expected_snapshot,
                expected_generation,
                expected_source_epoch,
                max_results,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(result) => Ok(result),
            Err(SqliteStoreError::MemoryProjectionUnavailable) => {
                Err(SqliteStoreError::MemoryProjectionUnavailable)
            }
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Reads immutable Git observations that had both observation and local
    /// approval events at or before the supplied recorded-time cutoff.
    ///
    /// This intentionally bypasses active memory projections. It establishes
    /// only recorded-time knowledge; callers must evaluate current Git or
    /// snapshot validity separately before making an applicability claim.
    pub fn known_at_trusted_git_history_evidence(
        &self,
        repository: RepositoryIdentityDigest,
        known_at_unix_ms: repowitness_domain::MemoryRecordedAtUnixMillis,
        max_results: u16,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Vec<GitHistoryEvidence>, SqliteStoreError> {
        if max_results == 0 || max_results > MAX_RESULTS {
            return Err(SqliteStoreError::InvalidSearchLimits);
        }
        check_control(&cancelled, deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::KnownAtHistoryEvidence(Box::new(KnownAtHistoryEvidenceCommand {
                repository,
                known_at_unix_ms,
                max_results,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(result) => Ok(result),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Reads a bounded immutable receipt for one exact Git commit or worktree
    /// snapshot at an inclusive recorded-time cutoff.
    ///
    /// Retained worktree snapshots are evaluated against direct observations
    /// and non-conflicted archival correspondence reviews. Git object
    /// reachability is deliberately outside this reader and remains
    /// unavailable until a bounded local Git adapter verifies it.
    pub fn known_at_history_receipt(
        &self,
        repository: RepositoryIdentityDigest,
        known_at_unix_ms: repowitness_domain::MemoryRecordedAtUnixMillis,
        target: MemoryObservationSource,
        max_results: u16,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<KnownAtHistoryReceipt, SqliteStoreError> {
        if max_results == 0 || max_results > MAX_RESULTS {
            return Err(SqliteStoreError::InvalidSearchLimits);
        }
        check_control(&cancelled, deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::KnownAtHistoryReceipt(Box::new(KnownAtHistoryReceiptCommand {
                repository,
                known_at_unix_ms,
                target,
                max_results,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(receipt) => Ok(receipt),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Reads local-only memory only when both profile and repository scopes match exactly.
    pub fn read_personal_memory(
        &self,
        profile: PersonalMemoryProfileId,
        repository: RepositoryIdentityDigest,
        limit: u16,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Vec<PersonalMemoryRecord>, SqliteStoreError> {
        if limit == 0 || limit > 100 {
            return Err(SqliteStoreError::InvalidSearchLimits);
        }
        check_control(&cancelled, deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::PersonalMemoryRead(Box::new(PersonalMemoryReadCommand {
                profile,
                repository,
                limit,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(records) => Ok(records),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Reads a polling-safe task summary without opening a writable connection.
    pub fn task_status(
        &self,
        repository: RepositoryIdentityDigest,
        task_id: TaskId,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Option<TaskStatus>, SqliteStoreError> {
        check_control(&cancelled, deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::TaskStatus(Box::new(TaskStatusCommand {
                repository,
                task_id,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(status) => Ok(status),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Lists bounded polling-safe task summaries without opening a writable connection.
    pub fn task_statuses(
        &self,
        repository: RepositoryIdentityDigest,
        limit: u16,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Vec<TaskStatus>, SqliteStoreError> {
        if limit == 0 || limit > 100 {
            return Err(SqliteStoreError::InvalidTask);
        }
        check_control(&cancelled, deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::TaskStatuses(Box::new(TaskStatusesCommand {
                repository,
                limit,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(statuses) => Ok(statuses),
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
include!("reader/artifact_commands.rs");
include!("reader/artifacts.rs");
include!("reader/graph_artifact_reuse.rs");
include!("reader/diagnostics.rs");
include!("reader/graph_commands.rs");
include!("reader/graph_decode.rs");
include!("reader/graph_receipt.rs");
include!("reader/graph_query.rs");
include!("reader/graph_relationships.rs");
include!("reader/graph_traversal.rs");
include!("reader/query.rs");
include!("reader/history.rs");
include!("reader/task.rs");
include!("reader/personal_memory.rs");
include!("reader/scip_overlay.rs");
include!("reader/workspace.rs");

#[cfg(test)]
mod tests;
