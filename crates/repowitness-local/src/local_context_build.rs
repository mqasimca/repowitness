//! One-shot local composition for deterministic Phase 0 context compilation.

use std::{
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
    CodeSearchError, CodeSearchLimits, CodeSearchQuery, CodeSearchQueryError, CodeSearchRequest,
    ContextBuildBudget, ContextBuildError, ContextBuildResult, ContextSourceCandidate,
    ContextSourceInput, DEFAULT_CODE_SEARCH_OUTPUT_BYTES, DEFAULT_MEMORY_RECALL_OUTPUT_BYTES,
    DEFAULT_MEMORY_RECALL_SCAN_BYTES, MemoryRecallError, MemoryRecallLimits, MemoryRecallQuery,
    MemoryRecallQueryError, MemoryRecallRequest, RepositoryIdentityTextError,
    RepositoryIdentityTextV1, ResolvedConfiguration, SymbolGetError, SymbolGetLimits,
    SymbolGetRequest, SymbolGetSelector, code_search, compile_context, memory_recall, symbol_get,
};
use repowitness_domain::EvidenceLocation;

use crate::{
    ContainedSourceError, ContainedSourceRoot, GenerationId, OwnedSqliteReader, SqliteStoreError,
    local_symbol_get::{LocalSymbolPort, LocalSymbolPortError},
};

/// Default end-to-end deadline for one local context build.
pub const DEFAULT_LOCAL_CONTEXT_BUILD_DEADLINE: Duration = Duration::from_secs(10);
/// Default candidates requested independently from source and memory providers.
pub const DEFAULT_LOCAL_CONTEXT_PROVIDER_RESULTS: u16 = 20;

/// Complete local input for one context build.
#[derive(Clone, Copy)]
pub struct LocalContextBuildRequest<'a> {
    root: &'a Path,
    database: &'a Path,
    repository_identity: &'a str,
    intent: &'a str,
    budget: ContextBuildBudget,
    max_provider_results: u16,
    configuration: Option<&'a ResolvedConfiguration>,
    deadline: Duration,
}

impl<'a> LocalContextBuildRequest<'a> {
    /// Constructs a request with conservative Phase 0 bounds.
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
            intent,
            budget: ContextBuildBudget::default(),
            max_provider_results: DEFAULT_LOCAL_CONTEXT_PROVIDER_RESULTS,
            configuration: None,
            deadline: DEFAULT_LOCAL_CONTEXT_BUILD_DEADLINE,
        }
    }

    /// Replaces the conservative content budget.
    pub fn with_budget_units(mut self, units: u64) -> Result<Self, ContextBuildError> {
        self.budget = ContextBuildBudget::try_new(units)?;
        Ok(self)
    }

    /// Replaces the per-provider result count.
    pub fn with_max_provider_results(
        mut self,
        max_results: u16,
    ) -> Result<Self, ContextBuildError> {
        if max_results == 0
            || CodeSearchLimits::try_new(max_results, DEFAULT_CODE_SEARCH_OUTPUT_BYTES).is_err()
            || MemoryRecallLimits::try_new(
                max_results,
                DEFAULT_MEMORY_RECALL_OUTPUT_BYTES,
                DEFAULT_MEMORY_RECALL_SCAN_BYTES,
            )
            .is_err()
        {
            return Err(ContextBuildError::InvalidSourceInput);
        }
        self.max_provider_results = max_results;
        Ok(self)
    }

    /// Applies resolved query and context limits as additional ceilings.
    #[must_use]
    pub const fn with_configuration(mut self, configuration: &'a ResolvedConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }

    /// Replaces the end-to-end monotonic deadline duration.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalContextBuildRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalContextBuildRequest")
            .field("root", &"<redacted-path>")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
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

/// Complete generation-pinned local context result.
pub type LocalContextBuildResult = ContextBuildResult<GenerationId, i64>;

/// Stable, content-redacted local context failure.
#[derive(Debug)]
pub enum LocalContextBuildError {
    /// Repository identity text was malformed or non-canonical.
    RepositoryIdentity(RepositoryIdentityTextError),
    /// The literal source intent was invalid.
    SourceQuery(CodeSearchQueryError),
    /// The literal memory intent was invalid.
    MemoryQuery(MemoryRecallQueryError),
    /// The absolute deadline could not be represented.
    DeadlineNotRepresentable,
    /// Cancellation was visible before I/O.
    Cancelled,
    /// The deadline elapsed before I/O.
    DeadlineExceeded,
    /// The contained source root could not open.
    RootOpen(ContainedSourceError),
    /// The owned reader could not start.
    ReaderStart(SqliteStoreError),
    /// Lexical source search failed.
    Search(CodeSearchError<SqliteStoreError>),
    /// Active memory recall failed.
    Memory(MemoryRecallError<SqliteStoreError>),
    /// Exact source expansion failed.
    Symbol(SymbolGetError<LocalSymbolPortError>),
    /// A source-search result violated the context composition contract.
    InvalidSearchResult,
    /// Deterministic context compilation failed.
    Compile(ContextBuildError),
    /// The owned reader did not shut down cleanly.
    Shutdown(SqliteStoreError),
}

impl fmt::Display for LocalContextBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity(_) => "repository identity is invalid",
            Self::SourceQuery(_) | Self::MemoryQuery(_) => "context intent is invalid",
            Self::DeadlineNotRepresentable => "context deadline is not representable",
            Self::Cancelled => "context build was cancelled",
            Self::DeadlineExceeded => "context build deadline elapsed",
            Self::RootOpen(_) => "repository source root could not open",
            Self::ReaderStart(_) => "context reader startup failed",
            Self::Search(_) => "context source search failed",
            Self::Memory(_) => "context memory recall failed",
            Self::Symbol(_) => "context source expansion failed",
            Self::InvalidSearchResult => "context source evidence is inconsistent",
            Self::Compile(_) => "context compilation failed",
            Self::Shutdown(_) => "context reader shutdown failed",
        })
    }
}

impl Error for LocalContextBuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity(source) => Some(source),
            Self::SourceQuery(source) => Some(source),
            Self::MemoryQuery(source) => Some(source),
            Self::RootOpen(source) => Some(source),
            Self::ReaderStart(source) | Self::Shutdown(source) => Some(source),
            Self::Search(source) => Some(source),
            Self::Memory(source) => Some(source),
            Self::Symbol(source) => Some(source),
            Self::Compile(source) => Some(source),
            Self::DeadlineNotRepresentable
            | Self::Cancelled
            | Self::DeadlineExceeded
            | Self::InvalidSearchResult => None,
        }
    }
}

/// Opens one exact local context, retrieves providers, compiles, and shuts down.
pub fn build_local_context(
    request: LocalContextBuildRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalContextBuildResult, LocalContextBuildError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(LocalContextBuildError::RepositoryIdentity)?;
    let source_query =
        CodeSearchQuery::try_new(request.intent).map_err(LocalContextBuildError::SourceQuery)?;
    let memory_query =
        MemoryRecallQuery::try_new(request.intent).map_err(LocalContextBuildError::MemoryQuery)?;
    let request = effective_context_request(request).map_err(LocalContextBuildError::Compile)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalContextBuildError::DeadlineNotRepresentable)?;
    check_control(cancelled.as_ref(), deadline)?;
    let root = ContainedSourceRoot::open(request.root).map_err(LocalContextBuildError::RootOpen)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalContextBuildError::ReaderStart)?;
    let result = build_with_reader(
        &reader,
        &root,
        repository,
        source_query,
        memory_query,
        request,
        Arc::clone(&cancelled),
        deadline,
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(source), _) => Err(source),
        (Ok(_), Err(source)) => Err(LocalContextBuildError::Shutdown(source)),
    }
}

fn effective_context_request<'a>(
    mut request: LocalContextBuildRequest<'a>,
) -> Result<LocalContextBuildRequest<'a>, ContextBuildError> {
    let Some(configuration) = request.configuration else {
        return Ok(request);
    };
    let configured_budget = *configuration.preferences().context_bytes().effective();
    request.budget = ContextBuildBudget::try_new(request.budget.units().min(configured_budget))?;

    let configured_results = *configuration.preferences().query_results().effective();
    let configured_results =
        u16::try_from(configured_results).map_err(|_| ContextBuildError::InvalidSourceInput)?;
    request.max_provider_results = request.max_provider_results.min(configured_results);
    Ok(request)
}

#[allow(
    clippy::too_many_arguments,
    reason = "the local composition keeps identity, providers, controls, and bounds explicit"
)]
fn build_with_reader(
    reader: &OwnedSqliteReader,
    root: &ContainedSourceRoot,
    repository: repowitness_domain::RepositoryIdentityDigest,
    source_query: CodeSearchQuery,
    memory_query: MemoryRecallQuery,
    request: LocalContextBuildRequest<'_>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<LocalContextBuildResult, LocalContextBuildError> {
    let search_limits = CodeSearchLimits::try_new(
        request.max_provider_results,
        DEFAULT_CODE_SEARCH_OUTPUT_BYTES,
    )
    .map_err(|_| LocalContextBuildError::Compile(ContextBuildError::InvalidSourceInput))?;
    let search = code_search(
        reader,
        CodeSearchRequest::new(
            repository,
            source_query,
            search_limits,
            Arc::clone(&cancelled),
            deadline,
        ),
    )
    .map_err(LocalContextBuildError::Search)?;
    let memory_limits = MemoryRecallLimits::try_new(
        request.max_provider_results,
        DEFAULT_MEMORY_RECALL_OUTPUT_BYTES,
        DEFAULT_MEMORY_RECALL_SCAN_BYTES,
    )
    .map_err(|_| LocalContextBuildError::Compile(ContextBuildError::InvalidSourceInput))?;
    let memory = match memory_recall(
        reader,
        MemoryRecallRequest::new(
            repository,
            memory_query,
            memory_limits,
            Arc::clone(&cancelled),
            deadline,
        ),
    ) {
        Ok(memory) => Some(memory),
        Err(MemoryRecallError::Port(SqliteStoreError::MemoryProjectionUnavailable)) => None,
        Err(error) => return Err(LocalContextBuildError::Memory(error)),
    };
    let candidates = expand_source_candidates(
        reader,
        root,
        repository,
        &search,
        request.budget,
        Arc::clone(&cancelled),
        deadline,
    )?;
    let source = ContextSourceInput::try_new(
        repository,
        search.claim().query(),
        *search.snapshot(),
        *search.generation(),
        search.coverage(),
        search.claim().total_matches(),
        search.claim().returned_matches(),
        candidates,
    )
    .map_err(LocalContextBuildError::Compile)?;
    compile_context(
        source,
        memory.as_ref(),
        request.budget,
        cancelled.as_ref(),
        deadline,
    )
    .map_err(LocalContextBuildError::Compile)
}

#[allow(
    clippy::too_many_arguments,
    reason = "exact expansion retains repository, generation, controls, and source ownership"
)]
pub(crate) fn expand_source_candidates(
    reader: &OwnedSqliteReader,
    root: &ContainedSourceRoot,
    repository: repowitness_domain::RepositoryIdentityDigest,
    search: &crate::LocalCodeSearchResult,
    budget: ContextBuildBudget,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<Vec<ContextSourceCandidate>, LocalContextBuildError> {
    let port = LocalSymbolPort { reader, root };
    let output_limit = budget
        .units()
        .checked_add(1024)
        .ok_or(LocalContextBuildError::Compile(
            ContextBuildError::CountNotRepresentable,
        ))?;
    let symbol_limits = SymbolGetLimits::try_new(budget.units(), output_limit)
        .map_err(|_| LocalContextBuildError::Compile(ContextBuildError::InvalidBudget))?;
    let mut candidates = Vec::with_capacity(search.evidence().as_slice().len());
    for (index, evidence) in search.evidence().as_slice().iter().enumerate() {
        check_control(cancelled.as_ref(), deadline)?;
        let EvidenceLocation::SymbolOccurrence(occurrence) = evidence.identity().location() else {
            return Err(LocalContextBuildError::InvalidSearchResult);
        };
        if occurrence.declaration_span().len().get() > budget.units() {
            continue;
        }
        let selector = SymbolGetSelector::new(
            evidence.identity().path().clone(),
            *evidence.identity().content_digest(),
            occurrence.artifact_digest(),
            occurrence.fact_ordinal(),
        );
        let result = symbol_get(
            &port,
            SymbolGetRequest::new(
                repository,
                *search.snapshot(),
                *search.generation(),
                selector,
                symbol_limits,
                Arc::clone(&cancelled),
                deadline,
            ),
        )
        .map_err(LocalContextBuildError::Symbol)?;
        let symbol = result
            .claim()
            .symbol()
            .ok_or(LocalContextBuildError::InvalidSearchResult)?;
        candidates.push(
            ContextSourceCandidate::try_new(
                u16::try_from(index + 1)
                    .map_err(|_| LocalContextBuildError::InvalidSearchResult)?,
                result.claim().selector().clone(),
                symbol.occurrence().clone(),
                symbol.declaration().to_vec().into_boxed_slice(),
            )
            .map_err(LocalContextBuildError::Compile)?,
        );
    }
    Ok(candidates)
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), LocalContextBuildError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalContextBuildError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(LocalContextBuildError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
