//! One-shot local composition for immutable exact raw syntax-target discovery.

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
    RepositoryIdentityTextError, RepositoryIdentityTextV1, ResolvedConfiguration,
    SyntaxSiteSearchError, SyntaxSiteSearchLimitError, SyntaxSiteSearchLimits,
    SyntaxSiteSearchQuery, SyntaxSiteSearchQueryError, SyntaxSiteSearchRequest,
    SyntaxSiteSearchResult, syntax_site_search,
};

use crate::{GenerationId, OwnedSqliteReader, SqliteStoreError};

/// Default end-to-end deadline for one local raw syntax-target search.
pub const DEFAULT_LOCAL_SYNTAX_SITE_SEARCH_DEADLINE: Duration = Duration::from_secs(5);

/// Proof-carrying local result pinned to one SQLite generation.
pub type LocalSyntaxSiteSearchResult = SyntaxSiteSearchResult<GenerationId>;

/// Explicit inputs for one local immutable syntax-site search.
#[derive(Clone, Copy)]
pub struct LocalSyntaxSiteSearchRequest<'a> {
    database: &'a Path,
    repository_identity: &'a str,
    target: &'a str,
    limits: SyntaxSiteSearchLimits,
    configuration: Option<&'a ResolvedConfiguration>,
    deadline: Duration,
}

impl<'a> LocalSyntaxSiteSearchRequest<'a> {
    /// Constructs a request with conservative profile limits.
    #[must_use]
    pub fn new(database: &'a Path, repository_identity: &'a str, target: &'a str) -> Self {
        Self {
            database,
            repository_identity,
            target,
            limits: SyntaxSiteSearchLimits::default(),
            configuration: None,
            deadline: DEFAULT_LOCAL_SYNTAX_SITE_SEARCH_DEADLINE,
        }
    }

    /// Applies an explicit retained-observation bound.
    pub fn with_max_results(
        mut self,
        max_results: u16,
    ) -> Result<Self, SyntaxSiteSearchLimitError> {
        self.limits = SyntaxSiteSearchLimits::try_new(max_results, self.limits.max_output_bytes())?;
        Ok(self)
    }

    /// Applies the resolved local query-result ceiling.
    #[must_use]
    pub const fn with_configuration(mut self, configuration: &'a ResolvedConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }

    /// Applies an explicit end-to-end deadline duration.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalSyntaxSiteSearchRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalSyntaxSiteSearchRequest")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("target", &"<redacted-raw-target>")
            .field("limits", &self.limits)
            .field(
                "configuration_digest",
                &self.configuration.map(ResolvedConfiguration::digest),
            )
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Stable local raw syntax-target search failure.
#[derive(Debug)]
pub enum LocalSyntaxSiteSearchError {
    /// The repository identity text is malformed or non-canonical.
    RepositoryIdentity(RepositoryIdentityTextError),
    /// The exact raw target violates the shared boundary profile.
    Query(SyntaxSiteSearchQueryError),
    /// The effective configured result bounds are invalid.
    Limits(SyntaxSiteSearchLimitError),
    /// The absolute deadline cannot be represented.
    DeadlineNotRepresentable,
    /// The owned read connection could not start.
    ReaderStart(SqliteStoreError),
    /// The shared application search failed.
    Search(SyntaxSiteSearchError<SqliteStoreError>),
    /// The owned read connection did not shut down cleanly.
    Shutdown(SqliteStoreError),
}

impl fmt::Display for LocalSyntaxSiteSearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity(_) => "repository identity is invalid",
            Self::Query(_) => "syntax-site search target is invalid",
            Self::Limits(_) => "syntax-site search limits are invalid",
            Self::DeadlineNotRepresentable => "syntax-site search deadline cannot be represented",
            Self::ReaderStart(_) => "local index reader could not start",
            Self::Search(_) => "local syntax-site search failed",
            Self::Shutdown(_) => "local index reader could not shut down",
        })
    }
}

impl Error for LocalSyntaxSiteSearchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity(source) => Some(source),
            Self::Query(source) => Some(source),
            Self::Limits(source) => Some(source),
            Self::ReaderStart(source) => Some(source),
            Self::Search(source) => Some(source),
            Self::Shutdown(source) => Some(source),
            Self::DeadlineNotRepresentable => None,
        }
    }
}

/// Opens one owned reader, searches its active raw-site projection, and shuts it down.
pub fn search_local_syntax_sites(
    request: LocalSyntaxSiteSearchRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalSyntaxSiteSearchResult, LocalSyntaxSiteSearchError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(LocalSyntaxSiteSearchError::RepositoryIdentity)?;
    let query = SyntaxSiteSearchQuery::try_new(request.target)
        .map_err(LocalSyntaxSiteSearchError::Query)?;
    let configured_max = request
        .configuration
        .map_or(u64::from(request.limits.max_results()), |configuration| {
            *configuration.preferences().query_results().effective()
        });
    let max_results = u64::from(request.limits.max_results()).min(configured_max);
    let max_results = u16::try_from(max_results)
        .map_err(|_| LocalSyntaxSiteSearchError::Limits(SyntaxSiteSearchLimitError))?;
    let limits = SyntaxSiteSearchLimits::try_new(max_results, request.limits.max_output_bytes())
        .map_err(LocalSyntaxSiteSearchError::Limits)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalSyntaxSiteSearchError::DeadlineNotRepresentable)?;
    check_control(&cancelled, deadline)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalSyntaxSiteSearchError::ReaderStart)?;
    let result = syntax_site_search(
        &reader,
        SyntaxSiteSearchRequest::new(repository, query, limits, cancelled, deadline),
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(source), _) => Err(LocalSyntaxSiteSearchError::Search(source)),
        (Ok(_), Err(source)) => Err(LocalSyntaxSiteSearchError::Shutdown(source)),
    }
}

fn check_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalSyntaxSiteSearchError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalSyntaxSiteSearchError::Search(
            SyntaxSiteSearchError::Cancelled,
        ))
    } else if Instant::now() >= deadline {
        Err(LocalSyntaxSiteSearchError::Search(
            SyntaxSiteSearchError::DeadlineExceeded,
        ))
    } else {
        Ok(())
    }
}
