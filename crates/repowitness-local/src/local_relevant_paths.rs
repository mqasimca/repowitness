//! One-shot local composition for bounded lexical path navigation.

use std::{
    error::Error,
    fmt,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use repowitness_application::{
    CodeSearchLimitError, RelevantPathsError, RelevantPathsLimitError, RelevantPathsLimits,
    RelevantPathsResult, ResolvedConfiguration, locate_relevant_paths,
};

use crate::{GenerationId, LocalCodeSearchError, LocalCodeSearchRequest, search_local_index};

/// Default end-to-end deadline for one local lexical path-navigation request.
pub const DEFAULT_LOCAL_RELEVANT_PATHS_DEADLINE: Duration = Duration::from_secs(5);

/// Evidence-bearing local path result pinned to one SQLite generation.
pub type LocalRelevantPathsResult = RelevantPathsResult<GenerationId>;

/// Explicit inputs for one local path-navigation projection.
#[derive(Clone, Copy)]
pub struct LocalRelevantPathsRequest<'a> {
    database: &'a Path,
    repository_identity: &'a str,
    query: &'a str,
    limits: RelevantPathsLimits,
    configuration: Option<&'a ResolvedConfiguration>,
    deadline: Duration,
}

impl<'a> LocalRelevantPathsRequest<'a> {
    /// Constructs a request with a conservative path-output bound.
    #[must_use]
    pub fn new(database: &'a Path, repository_identity: &'a str, query: &'a str) -> Self {
        Self {
            database,
            repository_identity,
            query,
            limits: RelevantPathsLimits::default(),
            configuration: None,
            deadline: DEFAULT_LOCAL_RELEVANT_PATHS_DEADLINE,
        }
    }

    /// Applies an explicit bounded path-output limit.
    pub fn with_max_paths(mut self, max_paths: u16) -> Result<Self, RelevantPathsLimitError> {
        self.limits = RelevantPathsLimits::try_new(max_paths)?;
        Ok(self)
    }

    /// Applies a resolved configuration as a ceiling for the underlying search.
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

    fn candidate_limit(self) -> u16 {
        self.limits.candidate_limit()
    }
}

impl fmt::Debug for LocalRelevantPathsRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalRelevantPathsRequest")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("query", &"<redacted-query>")
            .field("limits", &self.limits)
            .field(
                "configuration_digest",
                &self.configuration.map(ResolvedConfiguration::digest),
            )
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Stable one-shot local path-navigation failure.
#[derive(Debug)]
pub enum LocalRelevantPathsError {
    /// The underlying bounded lexical search did not complete.
    Search {
        /// Stable local search failure.
        source: LocalCodeSearchError,
    },
    /// The completed lexical receipt could not be projected safely.
    Projection {
        /// Stable application projection failure.
        source: RelevantPathsError,
    },
    /// The derived candidate count violated the shared search limit contract.
    CandidateLimit {
        /// Stable bounded-search limit failure.
        source: CodeSearchLimitError,
    },
}

impl fmt::Display for LocalRelevantPathsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Search { .. } => "local lexical path search failed",
            Self::Projection { .. } => "local lexical path projection failed",
            Self::CandidateLimit { .. } => "local lexical path candidate limit is invalid",
        })
    }
}

impl Error for LocalRelevantPathsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Search { source } => Some(source),
            Self::Projection { source } => Some(source),
            Self::CandidateLimit { source } => Some(source),
        }
    }
}

/// Runs one bounded lexical search and projects its immutable evidence receipt into paths.
pub fn locate_local_relevant_paths(
    request: LocalRelevantPathsRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalRelevantPathsResult, LocalRelevantPathsError> {
    let search =
        LocalCodeSearchRequest::new(request.database, request.repository_identity, request.query)
            .with_max_results(request.candidate_limit())
            .map_err(|source| LocalRelevantPathsError::CandidateLimit { source })?
            .with_deadline(request.deadline);
    let search = match request.configuration {
        Some(configuration) => search.with_configuration(configuration),
        None => search,
    };
    let search = search_local_index(search, cancelled)
        .map_err(|source| LocalRelevantPathsError::Search { source })?;
    locate_relevant_paths(search, request.limits)
        .map_err(|source| LocalRelevantPathsError::Projection { source })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::LocalRelevantPathsRequest;
    use repowitness_application::RELEVANT_PATHS_CANDIDATES_PER_PATH;

    const REPOSITORY_ID: &str = concat!(
        "rwi1:h:",
        "0101010101010101010101010101010101010101010101010101010101010101"
    );

    #[test]
    fn request_is_redacted_and_derives_a_bounded_candidate_surface() {
        let request = LocalRelevantPathsRequest::new(
            Path::new("/private/index.sqlite3"),
            REPOSITORY_ID,
            "private_symbol",
        )
        .with_max_paths(12)
        .expect("valid path limit");
        assert_eq!(
            request.candidate_limit(),
            12 * RELEVANT_PATHS_CANDIDATES_PER_PATH
        );
        let debug = format!("{request:?}");
        assert!(!debug.contains("/private"));
        assert!(!debug.contains(REPOSITORY_ID));
        assert!(!debug.contains("private_symbol"));
        assert!(
            LocalRelevantPathsRequest::new(Path::new("index"), REPOSITORY_ID, "symbol")
                .with_max_paths(0)
                .is_err()
        );
    }
}
