//! One-shot local composition for repository-scoped raw test-marker navigation.

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
    RepositoryIdentityTextError, RepositoryIdentityTextV1, SourceLanguage, TestMarkersError,
    TestMarkersLimitError, TestMarkersLimits, TestMarkersQuery, TestMarkersQueryError,
    TestMarkersRequest, TestMarkersResult, test_markers,
};

use crate::{GenerationId, OwnedSqliteReader, SqliteStoreError};

/// Default end-to-end deadline for one repository-scoped marker read.
pub const DEFAULT_LOCAL_TEST_MARKERS_DEADLINE: Duration = Duration::from_secs(5);

/// Proof-carrying local marker result pinned to one SQLite generation.
pub type LocalTestMarkersResult = TestMarkersResult<GenerationId>;

/// Explicit inputs for one bounded marker read.
#[derive(Clone, Copy)]
pub struct LocalTestMarkersRequest<'a> {
    database: &'a Path,
    repository_identity: &'a str,
    language: Option<SourceLanguage>,
    path_prefix: Option<&'a str>,
    limits: TestMarkersLimits,
    deadline: Duration,
}

impl<'a> LocalTestMarkersRequest<'a> {
    /// Constructs a repository-scoped raw marker request with conservative defaults.
    #[must_use]
    pub fn new(database: &'a Path, repository_identity: &'a str) -> Self {
        Self {
            database,
            repository_identity,
            language: None,
            path_prefix: None,
            limits: TestMarkersLimits::default(),
            deadline: DEFAULT_LOCAL_TEST_MARKERS_DEADLINE,
        }
    }

    /// Restricts output to direct persisted language and path facts only.
    #[must_use]
    pub const fn with_filters(
        mut self,
        language: Option<SourceLanguage>,
        path_prefix: Option<&'a str>,
    ) -> Self {
        self.language = language;
        self.path_prefix = path_prefix;
        self
    }

    /// Applies an explicit retained marker limit while retaining the byte ceiling.
    pub fn with_max_results(mut self, max_results: u16) -> Result<Self, TestMarkersLimitError> {
        self.limits = TestMarkersLimits::try_new(max_results, self.limits.max_output_bytes())?;
        Ok(self)
    }

    /// Replaces the end-to-end deadline duration.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalTestMarkersRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalTestMarkersRequest")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("language", &self.language)
            .field("path_prefix", &self.path_prefix.map(|_| "<redacted-path>"))
            .field("limits", &self.limits)
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Stable content-redacted local marker-read failure.
#[derive(Debug)]
pub enum LocalTestMarkersError {
    /// The repository identity text was malformed or non-canonical.
    RepositoryIdentity(RepositoryIdentityTextError),
    /// The optional direct-fact filter was not admitted.
    Query(TestMarkersQueryError),
    /// The requested retained-result bound was invalid.
    Limits(TestMarkersLimitError),
    /// The absolute deadline could not be represented.
    DeadlineNotRepresentable,
    /// The owned reader could not start.
    ReaderStart(SqliteStoreError),
    /// The shared application marker read failed.
    TestMarkers(TestMarkersError<SqliteStoreError>),
    /// The owned reader did not shut down cleanly.
    Shutdown(SqliteStoreError),
}

impl fmt::Display for LocalTestMarkersError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity(_) => "repository identity is invalid",
            Self::Query(_) => "test-marker query is invalid",
            Self::Limits(_) => "test-marker limits are invalid",
            Self::DeadlineNotRepresentable => "test-marker deadline cannot be represented",
            Self::ReaderStart(_) => "local index reader could not start",
            Self::TestMarkers(_) => "local test-marker read failed",
            Self::Shutdown(_) => "local index reader could not shut down",
        })
    }
}

impl Error for LocalTestMarkersError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity(error) => Some(error),
            Self::Query(error) => Some(error),
            Self::Limits(error) => Some(error),
            Self::ReaderStart(error) | Self::Shutdown(error) => Some(error),
            Self::TestMarkers(error) => Some(error),
            Self::DeadlineNotRepresentable => None,
        }
    }
}

/// Opens one owned reader, reads exact marker observations, then shuts it down.
pub fn read_local_test_markers(
    request: LocalTestMarkersRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalTestMarkersResult, LocalTestMarkersError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(LocalTestMarkersError::RepositoryIdentity)?;
    let query = TestMarkersQuery::try_new(request.language, request.path_prefix)
        .map_err(LocalTestMarkersError::Query)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalTestMarkersError::DeadlineNotRepresentable)?;
    check_facade_control(&cancelled, deadline)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalTestMarkersError::ReaderStart)?;
    let result = test_markers(
        &reader,
        TestMarkersRequest::new(
            repository,
            query,
            request.limits,
            Arc::clone(&cancelled),
            deadline,
        ),
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(LocalTestMarkersError::TestMarkers(error)),
        (Ok(_), Err(error)) => Err(LocalTestMarkersError::Shutdown(error)),
    }
}

fn check_facade_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalTestMarkersError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalTestMarkersError::TestMarkers(
            TestMarkersError::Cancelled,
        ))
    } else if Instant::now() >= deadline {
        Err(LocalTestMarkersError::TestMarkers(
            TestMarkersError::DeadlineExceeded,
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::{Arc, atomic::AtomicBool},
        time::Duration,
    };

    use super::{LocalTestMarkersRequest, read_local_test_markers};

    const REPOSITORY_ID: &str = concat!(
        "rwi1:h:",
        "0101010101010101010101010101010101010101010101010101010101010101"
    );

    #[test]
    fn malformed_boundary_values_fail_before_reader_start() {
        assert!(
            read_local_test_markers(
                LocalTestMarkersRequest::new(Path::new("/not/opened.sqlite3"), "invalid"),
                Arc::new(AtomicBool::new(false)),
            )
            .is_err()
        );
        assert!(
            LocalTestMarkersRequest::new(Path::new("index"), REPOSITORY_ID)
                .with_max_results(0)
                .is_err()
        );
    }

    #[test]
    fn debug_output_redacts_private_boundaries() {
        let debug = format!(
            "{:?}",
            LocalTestMarkersRequest::new(Path::new("/private/index.sqlite3"), REPOSITORY_ID,)
                .with_filters(None, Some("private/"))
                .with_deadline(Duration::from_secs(1))
        );
        assert!(debug.contains("<redacted-path>"));
        assert!(!debug.contains("/private"));
    }
}
