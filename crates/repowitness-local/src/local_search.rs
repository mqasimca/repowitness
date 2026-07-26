//! One-shot local composition for the shared Phase 0 code-search use case.

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
    CodeSearchError, CodeSearchLimitError, CodeSearchLimits, CodeSearchQuery, CodeSearchQueryError,
    CodeSearchRequest, CodeSearchResult, RepositoryIdentityTextError, RepositoryIdentityTextV1,
    code_search,
};

use crate::{GenerationId, OwnedSqliteReader, SqliteStoreError};

/// Default end-to-end deadline for one local lexical query.
pub const DEFAULT_LOCAL_CODE_SEARCH_DEADLINE: Duration = Duration::from_secs(5);

/// Proof-carrying local search result pinned to one SQLite generation.
pub type LocalCodeSearchResult = CodeSearchResult<GenerationId>;

/// Explicit inputs for one local database query.
#[derive(Clone, Copy)]
pub struct LocalCodeSearchRequest<'a> {
    database: &'a Path,
    repository_identity: &'a str,
    query: &'a str,
    limits: CodeSearchLimits,
    deadline: Duration,
}

impl<'a> LocalCodeSearchRequest<'a> {
    /// Constructs a request with conservative Phase 0 limits.
    #[must_use]
    pub fn new(database: &'a Path, repository_identity: &'a str, query: &'a str) -> Self {
        Self {
            database,
            repository_identity,
            query,
            limits: CodeSearchLimits::default(),
            deadline: DEFAULT_LOCAL_CODE_SEARCH_DEADLINE,
        }
    }

    /// Applies an explicit candidate limit while preserving the default byte ceiling.
    pub fn with_max_results(mut self, max_results: u16) -> Result<Self, CodeSearchLimitError> {
        self.limits = CodeSearchLimits::try_new(max_results, self.limits.max_output_bytes())?;
        Ok(self)
    }

    /// Applies an explicit end-to-end deadline duration.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalCodeSearchRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalCodeSearchRequest")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("query", &"<redacted-query>")
            .field("limits", &self.limits)
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Stable one-shot local search failure.
#[derive(Debug)]
pub enum LocalCodeSearchError {
    /// The repository identity text is malformed or non-canonical.
    RepositoryIdentity {
        /// Stable boundary validation failure.
        source: RepositoryIdentityTextError,
    },
    /// The query violates the shared literal profile.
    Query {
        /// Stable query validation failure.
        source: CodeSearchQueryError,
    },
    /// The absolute deadline cannot be represented.
    DeadlineNotRepresentable,
    /// The owned read connection could not start.
    ReaderStart {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// The shared application query failed.
    Search {
        /// Stable application or SQLite boundary failure.
        source: CodeSearchError<SqliteStoreError>,
    },
    /// The owned read connection did not shut down cleanly.
    Shutdown {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
}

impl fmt::Display for LocalCodeSearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity { .. } => "repository identity is invalid",
            Self::Query { .. } => "code-search query is invalid",
            Self::DeadlineNotRepresentable => "code-search deadline cannot be represented",
            Self::ReaderStart { .. } => "local index reader could not start",
            Self::Search { .. } => "local code search failed",
            Self::Shutdown { .. } => "local index reader could not shut down",
        })
    }
}

impl Error for LocalCodeSearchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity { source } => Some(source),
            Self::Query { source } => Some(source),
            Self::ReaderStart { source } => Some(source),
            Self::Search { source } => Some(source),
            Self::Shutdown { source } => Some(source),
            Self::DeadlineNotRepresentable => None,
        }
    }
}

/// Opens one owned reader, runs the shared application search, and shuts it down.
pub fn search_local_rust_index(
    request: LocalCodeSearchRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalCodeSearchResult, LocalCodeSearchError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(|source| LocalCodeSearchError::RepositoryIdentity { source })?;
    let query = CodeSearchQuery::try_new(request.query)
        .map_err(|source| LocalCodeSearchError::Query { source })?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalCodeSearchError::DeadlineNotRepresentable)?;
    check_facade_control(&cancelled, deadline)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(|source| LocalCodeSearchError::ReaderStart { source })?;
    let result = code_search(
        &reader,
        CodeSearchRequest::new(repository, query, request.limits, cancelled, deadline),
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(source), _) => Err(LocalCodeSearchError::Search { source }),
        (Ok(_), Err(source)) => Err(LocalCodeSearchError::Shutdown { source }),
    }
}

fn check_facade_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalCodeSearchError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalCodeSearchError::Search {
            source: CodeSearchError::Cancelled,
        })
    } else if Instant::now() >= deadline {
        Err(LocalCodeSearchError::Search {
            source: CodeSearchError::DeadlineExceeded,
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };

    use super::{LocalCodeSearchError, LocalCodeSearchRequest, search_local_rust_index};

    const REPOSITORY_ID: &str = concat!(
        "rwi1:h:",
        "0101010101010101010101010101010101010101010101010101010101010101"
    );

    #[test]
    fn request_limits_and_debug_output_are_explicit_and_redacted() {
        let request = LocalCodeSearchRequest::new(
            Path::new("/private/index.sqlite3"),
            REPOSITORY_ID,
            "private_symbol",
        )
        .with_max_results(100)
        .expect("inclusive result ceiling should be valid")
        .with_deadline(Duration::from_secs(1));
        let debug = format!("{request:?}");
        assert!(!debug.contains("/private"));
        assert!(!debug.contains(REPOSITORY_ID));
        assert!(!debug.contains("private_symbol"));
        assert!(
            LocalCodeSearchRequest::new(Path::new("index"), REPOSITORY_ID, "x")
                .with_max_results(0)
                .is_err()
        );
    }

    #[test]
    fn invalid_boundary_inputs_fail_before_database_io() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let invalid_identity = search_local_rust_index(
            LocalCodeSearchRequest::new(
                Path::new("/missing/private-index.sqlite3"),
                "private-invalid-identity",
                "symbol",
            ),
            Arc::clone(&cancelled),
        )
        .expect_err("invalid identity should fail");
        assert!(matches!(
            invalid_identity,
            LocalCodeSearchError::RepositoryIdentity { .. }
        ));

        let invalid_query = search_local_rust_index(
            LocalCodeSearchRequest::new(
                Path::new("/missing/private-index.sqlite3"),
                REPOSITORY_ID,
                "",
            ),
            cancelled,
        )
        .expect_err("invalid query should fail");
        assert!(matches!(invalid_query, LocalCodeSearchError::Query { .. }));
    }

    #[test]
    fn cancellation_and_zero_deadline_stop_before_database_io() {
        let cancelled = Arc::new(AtomicBool::new(true));
        let cancelled_error = search_local_rust_index(
            LocalCodeSearchRequest::new(
                Path::new("/missing/private-index.sqlite3"),
                REPOSITORY_ID,
                "symbol",
            ),
            Arc::clone(&cancelled),
        )
        .expect_err("pre-cancelled search should fail before opening SQLite");
        assert!(matches!(
            cancelled_error,
            LocalCodeSearchError::Search {
                source: repowitness_application::CodeSearchError::Cancelled
            }
        ));

        cancelled.store(false, Ordering::Release);
        let deadline_error = search_local_rust_index(
            LocalCodeSearchRequest::new(
                Path::new("/missing/private-index.sqlite3"),
                REPOSITORY_ID,
                "symbol",
            )
            .with_deadline(Duration::ZERO),
            cancelled,
        )
        .expect_err("zero-deadline search should fail before opening SQLite");
        assert!(matches!(
            deadline_error,
            LocalCodeSearchError::Search {
                source: repowitness_application::CodeSearchError::DeadlineExceeded
            }
        ));
    }
}
