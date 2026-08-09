//! One-shot local composition for the canonical evidence-balanced profile.

use std::{
    collections::BTreeSet,
    error::Error,
    fmt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use repowitness_application::{
    CodeSearchCandidate, CodeSearchError, CodeSearchLimits, CodeSearchPort, CodeSearchPortResult,
    CodeSearchQuery, CodeSearchQueryError, CodeSearchRequest, ConnectedWorkspaceIdTextV1,
    ContextSourceCandidate, DEFAULT_CODE_SEARCH_OUTPUT_BYTES, DEFAULT_MEMORY_RECALL_OUTPUT_BYTES,
    DEFAULT_MEMORY_RECALL_SCAN_BYTES, EvidenceContextBudget, EvidenceContextCandidate,
    EvidenceContextCandidateId, EvidenceContextError, EvidenceContextInput, EvidenceContextProfile,
    EvidenceContextProviderId, EvidenceContextResult, EvidenceContextScope, EvidenceContextTier,
    MemoryEffectiveState, MemoryRecallError, MemoryRecallLimits, MemoryRecallQuery,
    MemoryRecallQueryError, MemoryRecallRecord, MemoryRecallRequest, PackageScope,
    RepositoryIdentityTextError, RepositoryIdentityTextV1, ResolvedConfiguration,
    RustSymbolOccurrence, ScipSymbol, SourceArtifactEvidence, SourceSlotIdTextV1,
    SymbolGetSelector, WorkspaceIdentityTextError, code_search, compile_evidence_context,
    hash_source_content, memory_recall,
};
use repowitness_domain::{
    ConnectedWorkspaceId, EvidenceContextProviderAvailability, EvidenceContextProviderCoverage,
    EvidenceLocation, ScipSymbolError, SourceSlotId,
};
use sha2::{Digest, Sha256};

use crate::{
    ContainedSourceError, ContainedSourceRoot, DEFAULT_SOURCE_FILE_BYTES,
    DEFAULT_SOURCE_READ_CHUNK_BYTES, OwnedSqliteReader, RustGraphDirection, RustGraphEdgeKind,
    RustGraphEdgeKinds, RustGraphReadError, RustGraphReadLimits, RustGraphRelationshipCardinality,
    RustGraphTraceStart, ScipEvidenceReadLimits, ScipOccurrenceEvidence, ScipOverlaySummary,
    ScipSymbolEvidenceResult, ScipSyntaxSymbolResolution, SourceReadLimits, SqliteStoreError,
};

/// Default candidates requested independently from each evidence provider.
pub const DEFAULT_LOCAL_EVIDENCE_CONTEXT_PROVIDER_RESULTS: u16 = 20;

struct PinnedWorkspaceCodeSearchPort<'a> {
    reader: &'a OwnedSqliteReader,
    view: crate::sqlite::PinnedWorkspaceView,
    source_slot: SourceSlotId,
}

impl CodeSearchPort for PinnedWorkspaceCodeSearchPort<'_> {
    type Generation = crate::sqlite::GenerationId;
    type Error = SqliteStoreError;

    fn search(
        &self,
        repository: repowitness_domain::RepositoryIdentityDigest,
        query: &CodeSearchQuery,
        limits: CodeSearchLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<CodeSearchPortResult<Self::Generation>, Self::Error> {
        let member = self
            .view
            .members()
            .iter()
            .find(|member| member.source_slot() == self.source_slot)
            .ok_or(SqliteStoreError::InvalidWorkspaceView)?;
        if member.repository() != repository {
            return Err(SqliteStoreError::InvalidWorkspaceView);
        }
        let storage_limits =
            crate::SearchLimits::try_new(limits.max_results(), limits.max_output_bytes())?;
        let results = self.reader.search_workspace_member(
            &self.view,
            self.source_slot,
            query.as_str(),
            storage_limits,
            cancelled,
            deadline,
        )?;
        let candidates = results
            .hits()
            .iter()
            .map(|hit| {
                let occurrence = RustSymbolOccurrence::try_new(
                    hit.fact_ordinal(),
                    SourceArtifactEvidence::new(hit.artifact_digest(), hit.producer_manifest()),
                    hit.kind(),
                    hit.name().to_owned(),
                    hit.qualified_name().to_owned(),
                    hit.name_span(),
                    hit.declaration_span(),
                )
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?
                .with_language(hit.language());
                Ok(CodeSearchCandidate::new(
                    hit.path().clone(),
                    hit.content_digest(),
                    occurrence,
                ))
            })
            .collect::<Result<Vec<_>, SqliteStoreError>>()?;
        Ok(CodeSearchPortResult::new(
            results.snapshot(),
            results.generation(),
            results.index_coverage(),
            candidates,
            results.total_matches(),
            results.output_bytes(),
        ))
    }
}

/// Default end-to-end deadline for one local evidence-balanced context build.
pub const DEFAULT_LOCAL_EVIDENCE_CONTEXT_BUILD_DEADLINE: Duration = Duration::from_secs(10);

/// Maximum complete Rust graph relationship input admitted while expanding one
/// bounded evidence-balanced context request.
///
/// This is intentionally independent from the traversal visit cap: the reader
/// validates its complete immutable graph input before it can retain a small
/// one-hop result set.
const EVIDENCE_CONTEXT_GRAPH_INPUT_EDGE_LIMIT: u64 = 200_000;

const PROVIDER_ID_VERSION: &[u8] = b"repowitness:evidence-provider-id:v1\0";
const CANDIDATE_ID_VERSION: &[u8] = b"repowitness:evidence-candidate-id:v1\0";

/// One locally materialized exact evidence-balanced context item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalEvidenceContextItem {
    /// An exact syntax declaration expanded from a lexical source hit.
    Syntax(ContextSourceCandidate),
    /// A current, evidence-backed engineering-memory record.
    Memory(MemoryRecallRecord),
    /// A locally approved current memory record with an immutable Git observation receipt.
    History(LocalEvidenceHistoryItem),
    /// A source-verified, unambiguous occurrence from one immutable SCIP overlay.
    PreciseOverlay(LocalEvidencePreciseOverlayItem),
    /// A source-verified target declaration reached through one unique pinned graph edge.
    GraphRelation(LocalEvidenceGraphRelationItem),
}

/// One source-verified declaration selected through a unique immutable graph relationship.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalEvidenceGraphRelationItem {
    candidate: ContextSourceCandidate,
    edge_kind: RustGraphEdgeKind,
    depth: u32,
}

/// One current memory record attributed to an immutable historical Git observation.
///
/// The receipt proves that RepoWitness observed the selected locally approved
/// revision at the commit. It does not assert the object is still reachable or
/// that its historical source bytes equal the current source view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalEvidenceHistoryItem {
    record: MemoryRecallRecord,
    commit: repowitness_domain::MemoryCommitId,
}

impl LocalEvidenceHistoryItem {
    /// Returns the currently eligible, evidence-backed memory record.
    #[must_use]
    pub const fn record(&self) -> &MemoryRecallRecord {
        &self.record
    }

    /// Returns the exact Git commit at which the immutable revision was observed.
    #[must_use]
    pub const fn commit(&self) -> repowitness_domain::MemoryCommitId {
        self.commit
    }
}

impl LocalEvidenceGraphRelationItem {
    /// Returns the exact source declaration at the relationship target.
    #[must_use]
    pub const fn candidate(&self) -> &ContextSourceCandidate {
        &self.candidate
    }

    /// Returns the retained graph relationship category.
    #[must_use]
    pub const fn edge_kind(&self) -> RustGraphEdgeKind {
        self.edge_kind
    }

    /// Returns the one-based traversal depth of the relationship.
    #[must_use]
    pub const fn depth(&self) -> u32 {
        self.depth
    }
}

/// One source-verified SCIP occurrence admitted as precise context evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalEvidencePreciseOverlayItem {
    overlay: ScipOverlaySummary,
    occurrence: ScipOccurrenceEvidence,
    source: Box<[u8]>,
    relationship_count: u64,
}

impl LocalEvidencePreciseOverlayItem {
    /// Returns the exact immutable overlay receipt used by this item.
    #[must_use]
    pub const fn overlay(&self) -> ScipOverlaySummary {
        self.overlay
    }

    /// Returns the exact source-verified overlay occurrence.
    #[must_use]
    pub const fn occurrence(&self) -> &ScipOccurrenceEvidence {
        &self.occurrence
    }

    /// Returns the exact selected source span in its original byte representation.
    #[must_use]
    pub const fn source(&self) -> &[u8] {
        &self.source
    }

    /// Returns the number of validated relationships retained for the exact SCIP symbol.
    #[must_use]
    pub const fn relationship_count(&self) -> u64 {
        self.relationship_count
    }
}

/// Complete local evidence-balanced allocation result.
pub type LocalEvidenceContextBuildResult = EvidenceContextResult<LocalEvidenceContextItem>;

/// Complete local input for one evidence-balanced context-build operation.
#[derive(Clone, Copy)]
pub struct LocalEvidenceContextBuildRequest<'a> {
    root: &'a Path,
    database: &'a Path,
    repository_identity: &'a str,
    workspace: LocalEvidenceContextWorkspace<'a>,
    scip_symbol: Option<&'a str>,
    intent: &'a str,
    budget: EvidenceContextBudget,
    max_provider_results: u16,
    configuration: Option<&'a ResolvedConfiguration>,
    deadline: Duration,
}

/// Explicit workspace source selection for one evidence-balanced context build.
#[derive(Clone, Copy)]
pub enum LocalEvidenceContextWorkspace<'a> {
    /// The compatible default workspace derived from one repository identity.
    SingleRepository,
    /// One explicitly selected source slot in a connected workspace.
    ConnectedWorkspace {
        /// Canonical connected-workspace identity text.
        connected_workspace: &'a str,
        /// Canonical source-slot identity text.
        source_slot: &'a str,
    },
}

impl<'a> LocalEvidenceContextBuildRequest<'a> {
    /// Constructs a request with the fixed evidence-balanced profile defaults.
    #[must_use]
    pub fn new(
        root: &'a Path,
        database: &'a Path,
        repository_identity: &'a str,
        intent: &'a str,
    ) -> Self {
        Self {
            root,
            database,
            repository_identity,
            workspace: LocalEvidenceContextWorkspace::SingleRepository,
            scip_symbol: None,
            intent,
            budget: EvidenceContextBudget::default(),
            max_provider_results: DEFAULT_LOCAL_EVIDENCE_CONTEXT_PROVIDER_RESULTS,
            configuration: None,
            deadline: DEFAULT_LOCAL_EVIDENCE_CONTEXT_BUILD_DEADLINE,
        }
    }

    /// Constructs a request for one explicitly selected connected source slot.
    #[must_use]
    pub fn for_connected_workspace(
        root: &'a Path,
        database: &'a Path,
        repository_identity: &'a str,
        connected_workspace: &'a str,
        source_slot: &'a str,
        intent: &'a str,
    ) -> Self {
        Self {
            root,
            database,
            repository_identity,
            workspace: LocalEvidenceContextWorkspace::ConnectedWorkspace {
                connected_workspace,
                source_slot,
            },
            scip_symbol: None,
            intent,
            budget: EvidenceContextBudget::default(),
            max_provider_results: DEFAULT_LOCAL_EVIDENCE_CONTEXT_PROVIDER_RESULTS,
            configuration: None,
            deadline: DEFAULT_LOCAL_EVIDENCE_CONTEXT_BUILD_DEADLINE,
        }
    }

    /// Replaces the conservative whole-item allocation budget.
    pub fn with_budget_units(mut self, units: u64) -> Result<Self, EvidenceContextError> {
        self.budget = EvidenceContextBudget::try_new(units)?;
        Ok(self)
    }

    /// Replaces the independent source and memory provider result ceiling.
    pub fn with_max_provider_results(
        mut self,
        max_results: u16,
    ) -> Result<Self, EvidenceContextError> {
        if max_results == 0
            || CodeSearchLimits::try_new(max_results, DEFAULT_CODE_SEARCH_OUTPUT_BYTES).is_err()
            || MemoryRecallLimits::try_new(
                max_results,
                DEFAULT_MEMORY_RECALL_OUTPUT_BYTES,
                DEFAULT_MEMORY_RECALL_SCAN_BYTES,
            )
            .is_err()
        {
            return Err(EvidenceContextError::InvalidInput);
        }
        self.max_provider_results = max_results;
        Ok(self)
    }

    /// Enables the precise-overlay provider for one exact opaque SCIP symbol.
    #[must_use]
    pub const fn with_scip_symbol(mut self, scip_symbol: &'a str) -> Self {
        self.scip_symbol = Some(scip_symbol);
        self
    }

    /// Applies resolved query and context ceilings as additional bounds.
    #[must_use]
    pub const fn with_configuration(mut self, configuration: &'a ResolvedConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }

    /// Replaces the complete operation deadline.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalEvidenceContextBuildRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalEvidenceContextBuildRequest")
            .field("root", &"<redacted-path>")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("workspace", &self.workspace)
            .field(
                "scip_symbol",
                &self.scip_symbol.map(|_| "<redacted-scip-symbol>"),
            )
            .field("intent", &"<redacted-intent>")
            .field("budget", &self.budget)
            .field("max_provider_results", &self.max_provider_results)
            .field(
                "configuration_digest",
                &self.configuration.map(ResolvedConfiguration::digest),
            )
            .field("deadline", &self.deadline)
            .finish()
    }
}

impl fmt::Debug for LocalEvidenceContextWorkspace<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SingleRepository => {
                formatter.write_str("LocalEvidenceContextWorkspace::SingleRepository")
            }
            Self::ConnectedWorkspace { .. } => {
                formatter.write_str("LocalEvidenceContextWorkspace::ConnectedWorkspace(<redacted>)")
            }
        }
    }
}

/// Stable content-redacted local evidence-balanced context failure.
#[derive(Debug)]
pub enum LocalEvidenceContextBuildError {
    /// Repository identity text was malformed or non-canonical.
    RepositoryIdentity(RepositoryIdentityTextError),
    /// Connected-workspace identity text was malformed or non-canonical.
    ConnectedWorkspaceIdentity(WorkspaceIdentityTextError),
    /// Source-slot identity text was malformed or non-canonical.
    SourceSlotIdentity(WorkspaceIdentityTextError),
    /// The literal source intent was invalid.
    SourceQuery(CodeSearchQueryError),
    /// The literal memory intent was invalid.
    MemoryQuery(MemoryRecallQueryError),
    /// The optional opaque SCIP symbol was invalid.
    ScipSymbol(ScipSymbolError),
    /// The absolute deadline could not be represented.
    DeadlineNotRepresentable,
    /// Cancellation was visible before a complete result.
    Cancelled,
    /// The deadline elapsed before a complete result.
    DeadlineExceeded,
    /// The contained source root could not open.
    RootOpen(ContainedSourceError),
    /// The read-only SQLite owner could not start.
    ReaderStart(SqliteStoreError),
    /// No current single-repository workspace view was available.
    WorkspaceUnavailable,
    /// The pinned workspace view did not contain exactly the selected repository member.
    WorkspaceMismatch,
    /// The selected immutable source member could not be read.
    SourceScope(SqliteStoreError),
    /// Lexical source search failed.
    Search(CodeSearchError<SqliteStoreError>),
    /// Current memory recall failed.
    Memory(MemoryRecallError<SqliteStoreError>),
    /// Immutable Git-history provenance could not be read.
    History(SqliteStoreError),
    /// The selected immutable SCIP overlay could not be read.
    ScipEvidence(SqliteStoreError),
    /// The selected immutable syntax graph could not be read.
    Graph(RustGraphReadError),
    /// Exact source expansion failed.
    SourceExpansion(LocalEvidenceSourceExpansionError),
    /// Independently retrieved evidence did not match the pinned source member.
    EvidenceScopeMismatch,
    /// The evidence-balanced candidate or allocation contract failed.
    Compile(EvidenceContextError),
    /// The reader did not shut down cleanly.
    Shutdown(SqliteStoreError),
}

/// Stable, content-redacted source-expansion failure for a pinned evidence-balanced scope.
#[derive(Debug)]
pub enum LocalEvidenceSourceExpansionError {
    /// The capability-contained source read failed.
    Source(ContainedSourceError),
    /// Current source bytes do not match the indexed content identity.
    StaleSource,
    /// An indexed declaration span was outside the verified source bytes.
    InvalidSourceSpan,
}

impl fmt::Display for LocalEvidenceSourceExpansionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Source(_) => "evidence-balanced declaration source read failed",
            Self::StaleSource => "evidence-balanced declaration source is stale",
            Self::InvalidSourceSpan => "evidence-balanced declaration source span is invalid",
        })
    }
}

impl Error for LocalEvidenceSourceExpansionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Source(source) => Some(source),
            Self::StaleSource | Self::InvalidSourceSpan => None,
        }
    }
}

impl fmt::Display for LocalEvidenceContextBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity(_) => "repository identity is invalid",
            Self::ConnectedWorkspaceIdentity(_) | Self::SourceSlotIdentity(_) => {
                "evidence-balanced context workspace identity is invalid"
            }
            Self::SourceQuery(_) | Self::MemoryQuery(_) => "context intent is invalid",
            Self::ScipSymbol(_) => "evidence-balanced SCIP symbol is invalid",
            Self::DeadlineNotRepresentable => {
                "evidence-balanced context deadline is not representable"
            }
            Self::Cancelled => "evidence-balanced context build was cancelled",
            Self::DeadlineExceeded => "evidence-balanced context build deadline elapsed",
            Self::RootOpen(_) => "source root could not open",
            Self::ReaderStart(_) => "evidence-balanced context reader startup failed",
            Self::WorkspaceUnavailable | Self::WorkspaceMismatch => {
                "evidence-balanced context workspace selection is unavailable"
            }
            Self::SourceScope(_) => "evidence-balanced context source scope read failed",
            Self::Search(_) => "evidence-balanced context source search failed",
            Self::Memory(_) => "evidence-balanced context memory recall failed",
            Self::History(_) => "evidence-balanced Git-history evidence read failed",
            Self::ScipEvidence(_) => "evidence-balanced SCIP evidence read failed",
            Self::Graph(_) => "evidence-balanced graph evidence read failed",
            Self::SourceExpansion(_) => "evidence-balanced context source expansion failed",
            Self::EvidenceScopeMismatch => {
                "evidence-balanced context evidence did not match the pinned source"
            }
            Self::Compile(_) => "evidence-balanced context compilation failed",
            Self::Shutdown(_) => "evidence-balanced context reader shutdown failed",
        })
    }
}

impl Error for LocalEvidenceContextBuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity(error) => Some(error),
            Self::ConnectedWorkspaceIdentity(error) | Self::SourceSlotIdentity(error) => {
                Some(error)
            }
            Self::SourceQuery(error) => Some(error),
            Self::MemoryQuery(error) => Some(error),
            Self::ScipSymbol(error) => Some(error),
            Self::RootOpen(error) => Some(error),
            Self::ReaderStart(error) | Self::SourceScope(error) | Self::Shutdown(error) => {
                Some(error)
            }
            Self::Search(error) => Some(error),
            Self::Memory(error) => Some(error),
            Self::History(error) => Some(error),
            Self::ScipEvidence(error) => Some(error),
            Self::Graph(error) => Some(error),
            Self::SourceExpansion(error) => Some(error),
            Self::Compile(error) => Some(error),
            Self::DeadlineNotRepresentable
            | Self::Cancelled
            | Self::DeadlineExceeded
            | Self::WorkspaceUnavailable
            | Self::WorkspaceMismatch
            | Self::EvidenceScopeMismatch => None,
        }
    }
}

/// Opens one immutable local view, gathers bounded syntax and memory evidence,
/// then allocates it through the evidence-balanced profile.
pub fn build_local_evidence_context(
    request: LocalEvidenceContextBuildRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalEvidenceContextBuildResult, LocalEvidenceContextBuildError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(LocalEvidenceContextBuildError::RepositoryIdentity)?;
    let request =
        effective_context_request(request).map_err(LocalEvidenceContextBuildError::Compile)?;
    let source_query = CodeSearchQuery::try_new(request.intent)
        .map_err(LocalEvidenceContextBuildError::SourceQuery)?;
    let memory_query = MemoryRecallQuery::try_new(request.intent)
        .map_err(LocalEvidenceContextBuildError::MemoryQuery)?;
    let scip_symbol = request
        .scip_symbol
        .map(|symbol| ScipSymbol::try_new(symbol.to_owned()))
        .transpose()
        .map_err(LocalEvidenceContextBuildError::ScipSymbol)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalEvidenceContextBuildError::DeadlineNotRepresentable)?;
    check_control(&cancelled, deadline)?;
    let root = ContainedSourceRoot::open(request.root)
        .map_err(LocalEvidenceContextBuildError::RootOpen)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalEvidenceContextBuildError::ReaderStart)?;
    let result = build_with_reader(
        &reader,
        &root,
        repository,
        source_query,
        memory_query,
        scip_symbol,
        request,
        Arc::clone(&cancelled),
        deadline,
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(LocalEvidenceContextBuildError::Shutdown(error)),
    }
}

fn effective_context_request<'a>(
    mut request: LocalEvidenceContextBuildRequest<'a>,
) -> Result<LocalEvidenceContextBuildRequest<'a>, EvidenceContextError> {
    let Some(configuration) = request.configuration else {
        return Ok(request);
    };
    let configured_budget = *configuration.preferences().context_bytes().effective();
    request.budget = EvidenceContextBudget::try_new(request.budget.units().min(configured_budget))?;
    let configured_results = *configuration.preferences().query_results().effective();
    let configured_results =
        u16::try_from(configured_results).map_err(|_| EvidenceContextError::InvalidInput)?;
    request.max_provider_results = request.max_provider_results.min(configured_results);
    Ok(request)
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one local fence sequence keeps source ownership, identity, provider bounds, and controls auditable without crossing an unpinned intermediate result"
)]
fn build_with_reader(
    reader: &OwnedSqliteReader,
    root: &ContainedSourceRoot,
    repository: repowitness_domain::RepositoryIdentityDigest,
    source_query: CodeSearchQuery,
    memory_query: MemoryRecallQuery,
    scip_symbol: Option<ScipSymbol>,
    request: LocalEvidenceContextBuildRequest<'_>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<LocalEvidenceContextBuildResult, LocalEvidenceContextBuildError> {
    let (workspace, source_slot, requires_single_member) = match request.workspace {
        LocalEvidenceContextWorkspace::SingleRepository => (
            ConnectedWorkspaceId::for_single_repository(repository),
            SourceSlotId::for_repository(repository),
            true,
        ),
        LocalEvidenceContextWorkspace::ConnectedWorkspace {
            connected_workspace,
            source_slot,
        } => (
            ConnectedWorkspaceIdTextV1::decode(connected_workspace)
                .map_err(LocalEvidenceContextBuildError::ConnectedWorkspaceIdentity)?,
            SourceSlotIdTextV1::decode(source_slot)
                .map_err(LocalEvidenceContextBuildError::SourceSlotIdentity)?,
            false,
        ),
    };
    let view = reader
        .pin_workspace_view(workspace, None, Arc::clone(&cancelled), deadline)
        .map_err(LocalEvidenceContextBuildError::SourceScope)?
        .ok_or(LocalEvidenceContextBuildError::WorkspaceUnavailable)?;
    if requires_single_member && view.members().len() != 1 {
        return Err(LocalEvidenceContextBuildError::WorkspaceMismatch);
    }
    let member = view
        .members()
        .iter()
        .find(|member| member.source_slot() == source_slot)
        .ok_or(LocalEvidenceContextBuildError::WorkspaceMismatch)?;
    if member.repository() != repository {
        return Err(LocalEvidenceContextBuildError::WorkspaceMismatch);
    }
    let source = reader
        .scip_import_scope(&view, source_slot, Arc::clone(&cancelled), deadline)
        .map_err(LocalEvidenceContextBuildError::SourceScope)?;
    let scope = EvidenceContextScope::try_new(
        repository,
        source.connected_workspace(),
        source.workspace_view().get(),
        source.source_slot(),
        source.source_epoch().get(),
        source.generation().get(),
        source.source_snapshot(),
        source.source_manifest(),
    )
    .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let search_limits = CodeSearchLimits::try_new(
        request.max_provider_results,
        DEFAULT_CODE_SEARCH_OUTPUT_BYTES,
    )
    .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let search_port = PinnedWorkspaceCodeSearchPort {
        reader,
        view: view.clone(),
        source_slot,
    };
    let graph_query = source_query.as_str().to_owned();
    let search = code_search(
        &search_port,
        CodeSearchRequest::new(
            repository,
            source_query,
            search_limits,
            Arc::clone(&cancelled),
            deadline,
        ),
    )
    .map_err(LocalEvidenceContextBuildError::Search)?;
    if *search.snapshot() != scope.snapshot() || search.generation().get() != scope.generation() {
        return Err(LocalEvidenceContextBuildError::EvidenceScopeMismatch);
    }
    let expansion_budget = request.budget;
    let source_candidates = expand_pinned_source_candidates(
        root,
        &search,
        expansion_budget,
        Arc::clone(&cancelled),
        deadline,
    )?;
    let mut candidates = source_candidates
        .into_iter()
        .map(|candidate| syntax_candidate(scope, candidate))
        .collect::<Result<Vec<_>, _>>()?;
    candidates.extend(graph_relation_candidates(
        reader,
        root,
        &view,
        source_slot,
        scope,
        source.source_identity().producer_manifest(),
        &graph_query,
        request.max_provider_results,
        Arc::clone(&cancelled),
        deadline,
    )?);
    let mut scip_symbols = BTreeSet::new();
    if let Some(scip_symbol) = scip_symbol {
        scip_symbols.insert(scip_symbol);
    } else {
        for evidence in search.evidence().as_slice() {
            check_control(&cancelled, deadline)?;
            let EvidenceLocation::SymbolOccurrence(occurrence) = evidence.identity().location()
            else {
                return Err(LocalEvidenceContextBuildError::EvidenceScopeMismatch);
            };
            if let ScipSyntaxSymbolResolution::Exact(symbol) = reader
                .scip_symbol_at_syntax_span(
                    &view,
                    source_slot,
                    evidence.identity().path().clone(),
                    *evidence.identity().content_digest(),
                    occurrence.name_span(),
                    Arc::clone(&cancelled),
                    deadline,
                )
                .map_err(LocalEvidenceContextBuildError::ScipEvidence)?
            {
                scip_symbols.insert(symbol);
            }
        }
    }
    for scip_symbol in scip_symbols {
        let scip_limits = ScipEvidenceReadLimits::try_new(
            request.max_provider_results,
            request.max_provider_results,
            1024 * 1024,
        )
        .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
        let evidence = reader
            .scip_symbol_evidence(
                &view,
                source_slot,
                PackageScope::whole_repository(),
                scip_symbol,
                scip_limits,
                Arc::clone(&cancelled),
                deadline,
            )
            .map_err(LocalEvidenceContextBuildError::ScipEvidence)?;
        if let ScipSymbolEvidenceResult::Found(evidence) = evidence
            && evidence.occurrences().len() == 1
            && !evidence.occurrences_truncated()
            && !evidence.relationships_truncated()
        {
            let occurrence = evidence.occurrences()[0].clone();
            let source = read_verified_pinned_span(
                root,
                occurrence.path(),
                occurrence.content(),
                occurrence.span(),
                &cancelled,
                deadline,
            )?;
            candidates.push(precise_overlay_candidate(
                scope,
                evidence.overlay(),
                occurrence,
                source,
                u64::try_from(evidence.relationships().len())
                    .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?,
            )?);
        }
    }
    let memory_limits = MemoryRecallLimits::try_new(
        request.max_provider_results,
        DEFAULT_MEMORY_RECALL_OUTPUT_BYTES,
        DEFAULT_MEMORY_RECALL_SCAN_BYTES,
    )
    .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    match memory_recall(
        reader,
        MemoryRecallRequest::new(
            repository,
            memory_query,
            memory_limits,
            Arc::clone(&cancelled),
            deadline,
        ),
    ) {
        Ok(memory) => {
            let same_pinned_member = memory.snapshot() == scope.snapshot()
                && memory.generation().get() == scope.generation()
                && memory.source_epoch() == scope.source_epoch();
            if same_pinned_member {
                for (index, record) in memory.records().iter().enumerate() {
                    check_control(&cancelled, deadline)?;
                    if record.effective_state() == MemoryEffectiveState::Current {
                        candidates.push(memory_candidate(scope, index, record.clone())?);
                    }
                }
                match reader.trusted_git_history_evidence(
                    repository,
                    scope.snapshot(),
                    crate::GenerationId::from_database(scope.generation()),
                    scope.source_epoch(),
                    request.max_provider_results,
                    Arc::clone(&cancelled),
                    deadline,
                ) {
                    Ok(history) => {
                        for (index, evidence) in history.into_iter().enumerate() {
                            check_control(&cancelled, deadline)?;
                            let Some(record) = memory.records().iter().find(|record| {
                                record.effective_state() == MemoryEffectiveState::Current
                                    && record.record_id() == evidence.record_id()
                                    && record.revision() == Some(evidence.revision())
                            }) else {
                                // History is a secondary provider over the complete current
                                // projection, while recall has already applied this request's
                                // bounded literal query. Evidence for another recalled-out
                                // record is not a scope failure and must not make the syntax
                                // result unavailable.
                                continue;
                            };
                            candidates.push(history_candidate(
                                scope,
                                index,
                                record.clone(),
                                evidence.commit(),
                            )?);
                        }
                    }
                    Err(SqliteStoreError::MemoryProjectionUnavailable) => {}
                    Err(error) => return Err(LocalEvidenceContextBuildError::History(error)),
                }
            }
        }
        Err(MemoryRecallError::Port(SqliteStoreError::MemoryProjectionUnavailable)) => {}
        Err(error) => return Err(LocalEvidenceContextBuildError::Memory(error)),
    }
    let provider_coverage = local_provider_coverage(&candidates)?;
    let input = EvidenceContextInput::try_new(scope, None, candidates)
        .map_err(LocalEvidenceContextBuildError::Compile)?
        .with_provider_coverage(provider_coverage)
        .map_err(LocalEvidenceContextBuildError::Compile)?;
    compile_evidence_context(
        EvidenceContextProfile::EvidenceBalancedV1,
        input,
        request.budget,
        &cancelled,
        deadline,
    )
    .map_err(LocalEvidenceContextBuildError::Compile)
}

fn local_provider_coverage(
    candidates: &[EvidenceContextCandidate<LocalEvidenceContextItem>],
) -> Result<Vec<EvidenceContextProviderCoverage>, LocalEvidenceContextBuildError> {
    let mut coverage = Vec::with_capacity(6);
    for tier in [
        EvidenceContextTier::PreciseOverlay,
        EvidenceContextTier::Syntax,
        EvidenceContextTier::Structural,
        EvidenceContextTier::References,
        EvidenceContextTier::Memory,
        EvidenceContextTier::History,
    ] {
        let candidate_count = u64::try_from(
            candidates
                .iter()
                .filter(|candidate| candidate.tier() == tier)
                .count(),
        )
        .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
        let availability = if candidate_count == 0 {
            EvidenceContextProviderAvailability::Unavailable
        } else {
            EvidenceContextProviderAvailability::Available
        };
        coverage.push(
            EvidenceContextProviderCoverage::try_new(tier, availability, candidate_count)
                .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?,
        );
    }
    Ok(coverage)
}

#[allow(
    clippy::too_many_arguments,
    reason = "the exact provider identity, source occurrence, and source bytes each protect the immutable overlay fence"
)]
fn precise_overlay_candidate(
    scope: EvidenceContextScope,
    overlay: ScipOverlaySummary,
    occurrence: ScipOccurrenceEvidence,
    source: Box<[u8]>,
    relationship_count: u64,
) -> Result<EvidenceContextCandidate<LocalEvidenceContextItem>, LocalEvidenceContextBuildError> {
    let units = u64::try_from(source.len())
        .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let identity = candidate_identity(scope, b"scip-overlay", |hasher| {
        hasher.update(overlay.digest().as_bytes());
        hasher.update(occurrence.path().as_bytes());
        hasher.update(occurrence.content().as_bytes());
        hasher.update(occurrence.span().start().get().to_be_bytes());
        hasher.update(occurrence.span().end().get().to_be_bytes());
        hasher.update(occurrence.roles().bits().to_be_bytes());
        hasher.update(&source);
    });
    let provider = provider_identity_with(b"scip-overlay", overlay.digest().as_bytes());
    EvidenceContextCandidate::try_new(
        scope,
        EvidenceContextTier::PreciseOverlay,
        1,
        units,
        identity,
        provider,
        LocalEvidenceContextItem::PreciseOverlay(LocalEvidencePreciseOverlayItem {
            overlay,
            occurrence,
            source,
            relationship_count,
        }),
    )
    .map_err(LocalEvidenceContextBuildError::Compile)
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "the graph provider preserves every pinned source, graph, resource, and ownership boundary in one local adapter"
)]
fn graph_relation_candidates(
    reader: &OwnedSqliteReader,
    root: &ContainedSourceRoot,
    view: &crate::PinnedWorkspaceView,
    source_slot: SourceSlotId,
    scope: EvidenceContextScope,
    producer_manifest: repowitness_domain::ProducerManifestDigest,
    query: &str,
    max_results: u16,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<Vec<EvidenceContextCandidate<LocalEvidenceContextItem>>, LocalEvidenceContextBuildError>
{
    // Context expansion is deliberately one hop from an already matched syntax
    // declaration.  The graph read API supports deeper navigation, but letting
    // a broad lexical term fan out recursively here would spend the evidence
    // budget on transitive neighbours before each independent tier receives an
    // opportunity to contribute.
    let limits = RustGraphReadLimits::try_new_with_input(
        EVIDENCE_CONTEXT_GRAPH_INPUT_EDGE_LIMIT,
        64 * 1024 * 1024,
        1,
        u32::from(max_results),
        10_000,
        50_000,
        10_000,
        4 * 1024 * 1024,
    )
    .map_err(LocalEvidenceContextBuildError::Graph)?;
    let definitions = match reader.search_rust_graph_symbols(
        view,
        crate::GenerationId::from_database(scope.generation()),
        query,
        limits,
        None,
        Arc::clone(&cancelled),
        deadline,
    ) {
        Ok(result) => result,
        Err(RustGraphReadError::GraphNotProduced) => return Ok(Vec::new()),
        Err(error) => return Err(LocalEvidenceContextBuildError::Graph(error)),
    };
    let mut candidates = Vec::new();
    let mut identities = BTreeSet::new();
    for definition in definitions.definitions() {
        check_control(&cancelled, deadline)?;
        if definition.source_slot() != source_slot
            || definition.source_generation().get() != scope.generation()
        {
            continue;
        }
        let trace = match reader.trace_rust_graph(
            view,
            crate::GenerationId::from_database(scope.generation()),
            RustGraphTraceStart::Definition(definition.clone()),
            RustGraphDirection::Outbound,
            RustGraphEdgeKinds::ALL,
            limits,
            None,
            Arc::clone(&cancelled),
            deadline,
        ) {
            Ok(trace) => trace,
            Err(RustGraphReadError::GraphNotProduced) => return Ok(candidates),
            Err(error) => return Err(LocalEvidenceContextBuildError::Graph(error)),
        };
        for edge in trace.edges() {
            check_control(&cancelled, deadline)?;
            if edge.cardinality() != RustGraphRelationshipCardinality::Unique {
                continue;
            }
            let target = edge.target();
            if target.source_slot() != source_slot
                || target.source_generation().get() != scope.generation()
            {
                continue;
            }
            let source = read_verified_pinned_span(
                root,
                target.path(),
                target.content_digest(),
                target.declaration_span(),
                &cancelled,
                deadline,
            )?;
            let occurrence = RustSymbolOccurrence::try_new(
                target.fact_ordinal(),
                SourceArtifactEvidence::new(target.artifact(), producer_manifest),
                target.kind(),
                target.name().to_owned(),
                target.qualified_name().to_owned(),
                target.name_span(),
                target.declaration_span(),
            )
            .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
            let rank = u16::try_from(candidates.len() + 1)
                .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
            let candidate = ContextSourceCandidate::try_new(
                rank,
                SymbolGetSelector::new(
                    target.path().clone(),
                    target.content_digest(),
                    target.artifact(),
                    target.fact_ordinal(),
                ),
                occurrence,
                source,
            )
            .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
            let relation = graph_relation_candidate(scope, edge.kind(), edge.depth(), candidate)?;
            if identities.insert(relation.identity()) {
                candidates.push(relation);
            }
            if candidates.len() == usize::from(max_results) {
                return Ok(candidates);
            }
        }
    }
    Ok(candidates)
}

fn graph_relation_candidate(
    scope: EvidenceContextScope,
    edge_kind: RustGraphEdgeKind,
    depth: u32,
    candidate: ContextSourceCandidate,
) -> Result<EvidenceContextCandidate<LocalEvidenceContextItem>, LocalEvidenceContextBuildError> {
    let tier = match edge_kind {
        RustGraphEdgeKind::Import => EvidenceContextTier::Structural,
        RustGraphEdgeKind::Reference | RustGraphEdgeKind::Call => EvidenceContextTier::References,
    };
    let units = u64::try_from(candidate.declaration().len())
        .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let identity = candidate_identity(scope, b"source-declaration", |hasher| {
        hasher.update(candidate.selector().path().as_bytes());
        hasher.update(candidate.selector().content_digest().as_bytes());
        hasher.update(candidate.selector().artifact_digest().as_bytes());
        hasher.update(candidate.selector().fact_ordinal().to_be_bytes());
        hasher.update(candidate.declaration());
    });
    EvidenceContextCandidate::try_new(
        scope,
        tier,
        u32::from(candidate.provider_rank()),
        units,
        identity,
        provider_identity(b"rust-graph"),
        LocalEvidenceContextItem::GraphRelation(LocalEvidenceGraphRelationItem {
            candidate,
            edge_kind,
            depth,
        }),
    )
    .map_err(LocalEvidenceContextBuildError::Compile)
}

/// Expands declarations from the already-pinned lexical evidence without consulting a
/// repository-global active generation. This is the second half of the workspace fence: the
/// source digest and declaration span come from the same selected workspace member as search.
fn expand_pinned_source_candidates(
    root: &ContainedSourceRoot,
    search: &crate::LocalCodeSearchResult,
    budget: EvidenceContextBudget,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<Vec<ContextSourceCandidate>, LocalEvidenceContextBuildError> {
    let mut candidates = Vec::with_capacity(search.evidence().as_slice().len());
    for (index, evidence) in search.evidence().as_slice().iter().enumerate() {
        check_control(&cancelled, deadline)?;
        let EvidenceLocation::SymbolOccurrence(occurrence) = evidence.identity().location() else {
            return Err(LocalEvidenceContextBuildError::EvidenceScopeMismatch);
        };
        if occurrence.declaration_span().len().get() > budget.units() {
            continue;
        }
        let read_limits = SourceReadLimits::try_new(
            deadline
                .checked_duration_since(Instant::now())
                .ok_or(LocalEvidenceContextBuildError::DeadlineExceeded)?,
            DEFAULT_SOURCE_FILE_BYTES,
            DEFAULT_SOURCE_READ_CHUNK_BYTES,
        )
        .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
        let source = root
            .read_with_cancel(evidence.identity().path(), read_limits, || {
                cancelled.load(Ordering::Acquire)
            })
            .map_err(|error| match error {
                ContainedSourceError::Cancelled => LocalEvidenceContextBuildError::Cancelled,
                ContainedSourceError::DeadlineExceeded { .. } => {
                    LocalEvidenceContextBuildError::DeadlineExceeded
                }
                error => LocalEvidenceContextBuildError::SourceExpansion(
                    LocalEvidenceSourceExpansionError::Source(error),
                ),
            })?;
        check_control(&cancelled, deadline)?;
        if hash_source_content(&source) != *evidence.identity().content_digest() {
            return Err(LocalEvidenceContextBuildError::SourceExpansion(
                LocalEvidenceSourceExpansionError::StaleSource,
            ));
        }
        let start = usize::try_from(occurrence.declaration_span().start().get()).map_err(|_| {
            LocalEvidenceContextBuildError::SourceExpansion(
                LocalEvidenceSourceExpansionError::InvalidSourceSpan,
            )
        })?;
        let end = usize::try_from(occurrence.declaration_span().end().get()).map_err(|_| {
            LocalEvidenceContextBuildError::SourceExpansion(
                LocalEvidenceSourceExpansionError::InvalidSourceSpan,
            )
        })?;
        let declaration = source
            .get(start..end)
            .map(<[u8]>::to_vec)
            .map(Vec::into_boxed_slice)
            .ok_or(LocalEvidenceContextBuildError::SourceExpansion(
                LocalEvidenceSourceExpansionError::InvalidSourceSpan,
            ))?;
        let selector = SymbolGetSelector::new(
            evidence.identity().path().clone(),
            *evidence.identity().content_digest(),
            occurrence.artifact_digest(),
            occurrence.fact_ordinal(),
        );
        candidates.push(
            ContextSourceCandidate::try_new(
                u16::try_from(index + 1)
                    .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?,
                selector,
                occurrence.clone(),
                declaration,
            )
            .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?,
        );
    }
    Ok(candidates)
}

/// Reads one exact source span only after proving that current contained bytes still match the
/// immutable provider content digest.
fn read_verified_pinned_span(
    root: &ContainedSourceRoot,
    path: &repowitness_domain::RepositoryPath,
    content: repowitness_domain::SourceContentDigest,
    span: repowitness_domain::ByteSpan,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Box<[u8]>, LocalEvidenceContextBuildError> {
    check_control(cancelled, deadline)?;
    let read_limits = SourceReadLimits::try_new(
        deadline
            .checked_duration_since(Instant::now())
            .ok_or(LocalEvidenceContextBuildError::DeadlineExceeded)?,
        DEFAULT_SOURCE_FILE_BYTES,
        DEFAULT_SOURCE_READ_CHUNK_BYTES,
    )
    .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let source = root
        .read_with_cancel(path, read_limits, || cancelled.load(Ordering::Acquire))
        .map_err(|error| match error {
            ContainedSourceError::Cancelled => LocalEvidenceContextBuildError::Cancelled,
            ContainedSourceError::DeadlineExceeded { .. } => {
                LocalEvidenceContextBuildError::DeadlineExceeded
            }
            error => LocalEvidenceContextBuildError::SourceExpansion(
                LocalEvidenceSourceExpansionError::Source(error),
            ),
        })?;
    check_control(cancelled, deadline)?;
    if hash_source_content(&source) != content {
        return Err(LocalEvidenceContextBuildError::SourceExpansion(
            LocalEvidenceSourceExpansionError::StaleSource,
        ));
    }
    let start = usize::try_from(span.start().get()).map_err(|_| {
        LocalEvidenceContextBuildError::SourceExpansion(
            LocalEvidenceSourceExpansionError::InvalidSourceSpan,
        )
    })?;
    let end = usize::try_from(span.end().get()).map_err(|_| {
        LocalEvidenceContextBuildError::SourceExpansion(
            LocalEvidenceSourceExpansionError::InvalidSourceSpan,
        )
    })?;
    source
        .get(start..end)
        .map(<[u8]>::to_vec)
        .map(Vec::into_boxed_slice)
        .ok_or(LocalEvidenceContextBuildError::SourceExpansion(
            LocalEvidenceSourceExpansionError::InvalidSourceSpan,
        ))
}

fn syntax_candidate(
    scope: EvidenceContextScope,
    candidate: ContextSourceCandidate,
) -> Result<EvidenceContextCandidate<LocalEvidenceContextItem>, LocalEvidenceContextBuildError> {
    let units = u64::try_from(candidate.declaration().len())
        .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let identity = candidate_identity(scope, b"source-declaration", |hasher| {
        hasher.update(candidate.selector().path().as_bytes());
        hasher.update(candidate.selector().content_digest().as_bytes());
        hasher.update(candidate.selector().artifact_digest().as_bytes());
        hasher.update(candidate.selector().fact_ordinal().to_be_bytes());
        hasher.update(candidate.declaration());
    });
    EvidenceContextCandidate::try_new(
        scope,
        EvidenceContextTier::Syntax,
        u32::from(candidate.provider_rank()),
        units,
        identity,
        provider_identity(b"syntax"),
        LocalEvidenceContextItem::Syntax(candidate),
    )
    .map_err(LocalEvidenceContextBuildError::Compile)
}

fn memory_candidate(
    scope: EvidenceContextScope,
    index: usize,
    record: MemoryRecallRecord,
) -> Result<EvidenceContextCandidate<LocalEvidenceContextItem>, LocalEvidenceContextBuildError> {
    let semantic = record
        .record()
        .ok_or(LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let title = u64::try_from(semantic.claim().title().as_str().len())
        .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let body = u64::try_from(semantic.claim().body().as_str().len())
        .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let units = title
        .checked_add(body)
        .ok_or(LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let rank = u32::try_from(index + 1)
        .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let identity = candidate_identity(scope, b"memory", |hasher| {
        hasher.update(record.record_id().as_bytes());
        if let Some(revision) = record.revision() {
            hasher.update(revision.as_bytes());
        }
    });
    EvidenceContextCandidate::try_new(
        scope,
        EvidenceContextTier::Memory,
        rank,
        units,
        identity,
        provider_identity(b"memory"),
        LocalEvidenceContextItem::Memory(record),
    )
    .map_err(LocalEvidenceContextBuildError::Compile)
}

fn history_candidate(
    scope: EvidenceContextScope,
    index: usize,
    record: MemoryRecallRecord,
    commit: repowitness_domain::MemoryCommitId,
) -> Result<EvidenceContextCandidate<LocalEvidenceContextItem>, LocalEvidenceContextBuildError> {
    let semantic = record
        .record()
        .ok_or(LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let title = u64::try_from(semantic.claim().title().as_str().len())
        .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let body = u64::try_from(semantic.claim().body().as_str().len())
        .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let units = title
        .checked_add(body)
        .ok_or(LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let rank = u32::try_from(index + 1)
        .map_err(|_| LocalEvidenceContextBuildError::EvidenceScopeMismatch)?;
    let identity = candidate_identity(scope, b"memory-git-history", |hasher| {
        hasher.update(record.record_id().as_bytes());
        if let Some(revision) = record.revision() {
            hasher.update(revision.as_bytes());
        }
        hasher.update(match commit.object_format() {
            repowitness_domain::MemoryObjectFormat::Sha1 => [1],
            repowitness_domain::MemoryObjectFormat::Sha256 => [2],
        });
        hasher.update(commit.as_bytes());
    });
    EvidenceContextCandidate::try_new(
        scope,
        EvidenceContextTier::History,
        rank,
        units,
        identity,
        provider_identity(b"memory-git-history"),
        LocalEvidenceContextItem::History(LocalEvidenceHistoryItem { record, commit }),
    )
    .map_err(LocalEvidenceContextBuildError::Compile)
}

fn provider_identity(label: &[u8]) -> EvidenceContextProviderId {
    provider_identity_with(label, &[])
}

fn provider_identity_with(label: &[u8], immutable_identity: &[u8]) -> EvidenceContextProviderId {
    let mut hasher = Sha256::new();
    hasher.update(PROVIDER_ID_VERSION);
    hasher.update(label);
    hasher.update(immutable_identity);
    EvidenceContextProviderId::new(hasher.finalize().into())
}

fn candidate_identity(
    scope: EvidenceContextScope,
    provider: &[u8],
    write_payload: impl FnOnce(&mut Sha256),
) -> EvidenceContextCandidateId {
    let mut hasher = Sha256::new();
    hasher.update(CANDIDATE_ID_VERSION);
    hasher.update(scope.repository().as_bytes());
    hasher.update(scope.connected_workspace().as_bytes());
    hasher.update(scope.workspace_view().to_be_bytes());
    hasher.update(scope.source_slot().as_bytes());
    hasher.update(scope.source_epoch().to_be_bytes());
    hasher.update(scope.generation().to_be_bytes());
    hasher.update(scope.snapshot().as_bytes());
    hasher.update(scope.manifest().as_bytes());
    hasher.update(provider);
    write_payload(&mut hasher);
    EvidenceContextCandidateId::new(hasher.finalize().into())
}

fn check_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalEvidenceContextBuildError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalEvidenceContextBuildError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(LocalEvidenceContextBuildError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
        time::Duration,
    };

    use repowitness_application::{
        ContextItem, RustGraphReadOperation, RustGraphSymbolQuery, RustGraphTraceLimits,
    };

    use crate::{
        LocalCodeSearchRequest, LocalContextBuildRequest, LocalIndexRequest,
        LocalRustGraphReadOutput, LocalRustGraphReadRequest, LocalRustIndexLimits,
        build_local_context, index_local_repository, read_local_rust_graph, search_local_index,
    };

    use super::*;

    const REPOSITORY_ID: &str = concat!(
        "rwi1:h:",
        "0101010101010101010101010101010101010101010101010101010101010101"
    );
    const LARGE_GRAPH_TEST_DEADLINE: Duration = Duration::from_secs(180);
    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "repowitness-local-evidence-context-{}-{ordinal}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("fixture directory");
            Self(path)
        }

        fn repository(&self) -> PathBuf {
            self.0.join("repository")
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

    fn fixture_repository(directory: &TempDirectory) -> PathBuf {
        let repository = directory.repository();
        fs::create_dir_all(repository.join("src")).expect("source directory");
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .arg(&repository)
                .status()
                .expect("git init")
                .success()
        );
        fs::write(
            repository.join("src/lib.rs"),
            "pub struct Widget;\nimpl Widget { pub fn run() {} }\n",
        )
        .expect("source fixture");
        assert!(
            Command::new("git")
                .current_dir(&repository)
                .args(["add", "--", "src/lib.rs"])
                .status()
                .expect("git add")
                .success()
        );
        repository
    }

    #[test]
    fn indexed_source_builds_an_exact_scoped_evidence_syntax_context() {
        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);
        let database = directory.database();
        let report = index_local_repository(
            LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("index");
        let result = build_local_evidence_context(
            LocalEvidenceContextBuildRequest::new(&repository, &database, REPOSITORY_ID, "Widget"),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("evidence-balanced context");
        assert_eq!(result.profile(), EvidenceContextProfile::EvidenceBalancedV1);
        assert_eq!(result.scope().generation(), report.generation().get());
        assert!(!result.items().is_empty());
        assert!(result.items().iter().all(|item| {
            item.tier() == EvidenceContextTier::Syntax
                && matches!(item.payload(), LocalEvidenceContextItem::Syntax(_))
        }));
    }

    #[test]
    fn unique_pinned_graph_edges_contribute_reference_context_without_cross_scope_leakage() {
        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);
        fs::write(
            repository.join("src/lib.rs"),
            "pub fn target() {}\npub fn Widget() { target(); }\n",
        )
        .expect("graph source fixture");
        let database = directory.database();
        index_local_repository(
            LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("index");
        let result = build_local_evidence_context(
            LocalEvidenceContextBuildRequest::new(&repository, &database, REPOSITORY_ID, "Widget"),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("evidence-balanced context");
        assert!(result.items().iter().any(|item| {
            item.tier() == EvidenceContextTier::References
                && matches!(
                    item.payload(),
                    LocalEvidenceContextItem::GraphRelation(relation)
                        if relation.edge_kind() == RustGraphEdgeKind::Call
                            && relation.depth() == 1
                            && std::str::from_utf8(relation.candidate().declaration())
                                .is_ok_and(|source| source.contains("target"))
                )
        }));
    }

    #[test]
    fn graph_expansion_accepts_complete_input_above_the_traversal_visit_cap() {
        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);
        let mut source = String::from("pub fn target() {}\npub fn Widget() {\n");
        source.push_str(&"target();\n".repeat(50_001));
        source.push_str("}\n");
        fs::write(repository.join("src/lib.rs"), source).expect("large graph fixture");
        let database = directory.database();
        let defaults = LocalRustIndexLimits::default();
        let index_limits = LocalRustIndexLimits::new(
            LARGE_GRAPH_TEST_DEADLINE,
            defaults.discovery(),
            defaults.source_read(),
            defaults.preparation(),
        );
        // This fixture validates context construction above the traversal cap,
        // not an interactive indexing service-level objective. Give the test a
        // bounded deadline that remains valid under parallel CI contention.
        index_local_repository(
            LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0)
                .with_limits(index_limits),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("index");

        let result = build_local_evidence_context(
            LocalEvidenceContextBuildRequest::new(&repository, &database, REPOSITORY_ID, "Widget")
                .with_deadline(LARGE_GRAPH_TEST_DEADLINE),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("graph input above the traversal visit cap remains usable");

        assert!(result.items().iter().any(|item| {
            item.tier() == EvidenceContextTier::References
                && matches!(item.payload(), LocalEvidenceContextItem::GraphRelation(_))
        }));
    }

    #[test]
    fn syntax_and_graph_evidence_for_one_declaration_share_one_budget_item() {
        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);
        fs::write(
            repository.join("src/lib.rs"),
            "pub fn Widget() { Widget(); }\n",
        )
        .expect("recursive graph source fixture");
        let database = directory.database();
        index_local_repository(
            LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("index");
        let result = build_local_evidence_context(
            LocalEvidenceContextBuildRequest::new(&repository, &database, REPOSITORY_ID, "Widget"),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("evidence-balanced context");
        let [item] = result.items() else {
            panic!("the duplicate source declaration should be grouped");
        };
        assert_eq!(item.tier(), EvidenceContextTier::Syntax);
        assert_eq!(item.attributions().len(), 2);
        assert!(matches!(
            item.payload(),
            LocalEvidenceContextItem::Syntax(_)
        ));
    }

    #[test]
    fn public_synthetic_call_chain_improves_navigation_and_relevant_source_density() {
        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);
        fs::write(
            repository.join("src/lib.rs"),
            concat!(
                "pub fn close_connection() {}\n",
                "pub fn Listener() { close_connection(); }\n",
                "pub fn retry_connection() {}\n",
                "pub fn Worker() { retry_connection(); }\n",
            ),
        )
        .expect("public synthetic source fixture");
        let database = directory.database();
        index_local_repository(
            LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("index");

        for (anchor, target) in [
            ("Listener", "close_connection"),
            ("Worker", "retry_connection"),
        ] {
            let lexical = search_local_index(
                LocalCodeSearchRequest::new(&database, REPOSITORY_ID, anchor),
                Arc::new(AtomicBool::new(false)),
            )
            .expect("lexical baseline");
            assert_eq!(lexical.evidence().as_slice().len(), 1);

            let graph = read_local_rust_graph(
                LocalRustGraphReadRequest::new(
                    &database,
                    REPOSITORY_ID,
                    RustGraphReadOperation::Search {
                        query: RustGraphSymbolQuery::try_new(anchor).expect("query"),
                        limits: RustGraphTraceLimits::default(),
                    },
                ),
                Arc::new(AtomicBool::new(false)),
            )
            .expect("graph-only baseline");
            assert!(matches!(
                graph.output(),
                LocalRustGraphReadOutput::Search(result) if result.definitions().len() == 1
            ));

            let incumbent = build_local_context(
                LocalContextBuildRequest::new(&repository, &database, REPOSITORY_ID, anchor),
                Arc::new(AtomicBool::new(false)),
            )
            .expect("supported incumbent context");
            assert_eq!(incumbent.items().len(), 1);
            assert!(matches!(
                incumbent.items().first(),
                Some(ContextItem::Source(item))
                    if std::str::from_utf8(item.candidate().declaration())
                        .is_ok_and(|source| source.contains(anchor))
            ));

            let evidence = build_local_evidence_context(
                LocalEvidenceContextBuildRequest::new(
                    &repository,
                    &database,
                    REPOSITORY_ID,
                    anchor,
                ),
                Arc::new(AtomicBool::new(false)),
            )
            .expect("evidence-balanced context");
            assert!(evidence.items().iter().any(|item| {
                matches!(item.payload(), LocalEvidenceContextItem::Syntax(candidate)
                    if std::str::from_utf8(candidate.declaration())
                        .is_ok_and(|source| source.contains(anchor)))
            }));
            assert!(evidence.items().iter().any(|item| {
                matches!(item.payload(), LocalEvidenceContextItem::GraphRelation(relation)
                    if relation.edge_kind() == RustGraphEdgeKind::Call
                        && std::str::from_utf8(relation.candidate().declaration())
                            .is_ok_and(|source| source.contains(target)))
            }));

            // The lexical and graph-only baselines expose only selectors, while
            // the incumbent expands one source declaration. evidence-balanced expands the
            // same anchor plus the direct call target. Both declarations are
            // one-line required task evidence, so its relevant-source density
            // must improve for each downstream navigation task.
            assert!(incumbent.used_units() > 0);
            assert!(evidence.used_units() > incumbent.used_units());
            assert!(2 * incumbent.used_units() > evidence.used_units());
        }
    }

    #[test]
    fn invalid_boundary_inputs_fail_before_filesystem_or_database_access() {
        let missing = Path::new("/missing/private-evidence-context-input");
        let invalid_identity = match build_local_evidence_context(
            LocalEvidenceContextBuildRequest::new(missing, missing, "invalid", "Widget"),
            Arc::new(AtomicBool::new(false)),
        ) {
            Ok(_) => panic!("identity validation should fail"),
            Err(error) => error,
        };
        assert!(matches!(
            invalid_identity,
            LocalEvidenceContextBuildError::RepositoryIdentity(_)
        ));
        let cancelled = match build_local_evidence_context(
            LocalEvidenceContextBuildRequest::new(missing, missing, REPOSITORY_ID, "Widget"),
            Arc::new(AtomicBool::new(true)),
        ) {
            Ok(_) => panic!("cancellation should fail"),
            Err(error) => error,
        };
        assert!(matches!(
            cancelled,
            LocalEvidenceContextBuildError::Cancelled
        ));
    }
}
