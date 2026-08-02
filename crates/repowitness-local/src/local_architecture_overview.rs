//! One-shot local composition for the bounded source-only architecture overview.

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
    ArchitectureOverviewError, ArchitectureOverviewLimitError, ArchitectureOverviewLimits,
    ArchitectureOverviewRequest, ArchitectureOverviewResult, RepositoryIdentityTextError,
    RepositoryIdentityTextV1, architecture_overview,
};

use crate::{GenerationId, OwnedSqliteReader, SqliteStoreError};

/// Default end-to-end deadline for one bounded source-only architecture overview.
pub const DEFAULT_LOCAL_ARCHITECTURE_OVERVIEW_DEADLINE: Duration = Duration::from_secs(30);

/// Exact source-only architecture orientation pinned to one immutable local generation.
pub type LocalArchitectureOverviewResult = ArchitectureOverviewResult<GenerationId>;

/// Explicit inputs for one local active-generation architecture overview.
#[derive(Clone, Copy)]
pub struct LocalArchitectureOverviewRequest<'a> {
    database: &'a Path,
    repository_identity: &'a str,
    limits: ArchitectureOverviewLimits,
    deadline: Duration,
}

impl<'a> LocalArchitectureOverviewRequest<'a> {
    /// Constructs a request with conservative public overview limits.
    #[must_use]
    pub fn new(database: &'a Path, repository_identity: &'a str) -> Self {
        Self {
            database,
            repository_identity,
            limits: ArchitectureOverviewLimits::default(),
            deadline: DEFAULT_LOCAL_ARCHITECTURE_OVERVIEW_DEADLINE,
        }
    }

    /// Applies independent receipt ceilings while preserving the byte ceiling.
    pub fn with_limits(
        mut self,
        max_roots: u16,
        max_entry_point_candidates: u16,
        max_files: u16,
    ) -> Result<Self, ArchitectureOverviewLimitError> {
        self.limits = ArchitectureOverviewLimits::try_new(
            max_roots,
            max_entry_point_candidates,
            max_files,
            self.limits.max_output_bytes(),
        )?;
        Ok(self)
    }

    /// Replaces the end-to-end deadline duration.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalArchitectureOverviewRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalArchitectureOverviewRequest")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("limits", &self.limits)
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Stable content-redacted local architecture-overview failure.
#[derive(Debug)]
pub enum LocalArchitectureOverviewError {
    /// The repository identity text was malformed or non-canonical.
    RepositoryIdentity(RepositoryIdentityTextError),
    /// The requested bound was invalid.
    Limits(ArchitectureOverviewLimitError),
    /// The absolute deadline could not be represented.
    DeadlineNotRepresentable,
    /// The owned reader could not start.
    ReaderStart(SqliteStoreError),
    /// The shared application overview failed.
    Overview(ArchitectureOverviewError<SqliteStoreError>),
    /// The owned reader could not shut down cleanly.
    Shutdown(SqliteStoreError),
}

impl fmt::Display for LocalArchitectureOverviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity(_) => "repository identity is invalid",
            Self::Limits(_) => "architecture-overview limits are invalid",
            Self::DeadlineNotRepresentable => {
                "architecture-overview deadline cannot be represented"
            }
            Self::ReaderStart(_) => "local index reader could not start",
            Self::Overview(_) => "local architecture overview failed",
            Self::Shutdown(_) => "local index reader could not shut down",
        })
    }
}

impl Error for LocalArchitectureOverviewError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity(error) => Some(error),
            Self::Limits(error) => Some(error),
            Self::ReaderStart(error) | Self::Shutdown(error) => Some(error),
            Self::Overview(error) => Some(error),
            Self::DeadlineNotRepresentable => None,
        }
    }
}

/// Opens one owned reader, summarizes the active index, then shuts the reader down.
pub fn overview_local_architecture(
    request: LocalArchitectureOverviewRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalArchitectureOverviewResult, LocalArchitectureOverviewError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(LocalArchitectureOverviewError::RepositoryIdentity)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalArchitectureOverviewError::DeadlineNotRepresentable)?;
    check_facade_control(&cancelled, deadline)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalArchitectureOverviewError::ReaderStart)?;
    let result = architecture_overview(
        &reader,
        ArchitectureOverviewRequest::new(
            repository,
            request.limits,
            Arc::clone(&cancelled),
            deadline,
        ),
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(LocalArchitectureOverviewError::Overview(error)),
        (Ok(_), Err(error)) => Err(LocalArchitectureOverviewError::Shutdown(error)),
    }
}

fn check_facade_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalArchitectureOverviewError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalArchitectureOverviewError::Overview(
            ArchitectureOverviewError::Cancelled,
        ))
    } else if Instant::now() >= deadline {
        Err(LocalArchitectureOverviewError::Overview(
            ArchitectureOverviewError::DeadlineExceeded,
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

    use super::{
        DEFAULT_LOCAL_ARCHITECTURE_OVERVIEW_DEADLINE, LocalArchitectureOverviewRequest,
        overview_local_architecture,
    };

    const REPOSITORY_ID: &str = concat!(
        "rwi1:h:",
        "0101010101010101010101010101010101010101010101010101010101010101"
    );

    #[test]
    fn request_bounds_and_debug_output_are_explicit_and_redacted() {
        let request = LocalArchitectureOverviewRequest::new(
            Path::new("/private/index.sqlite3"),
            REPOSITORY_ID,
        )
        .with_limits(10, 20, 30)
        .expect("bounded limits should be valid");
        let debug = format!("{request:?}");
        assert!(debug.contains("<redacted-path>"));
        assert!(debug.contains("<redacted-identity>"));
        assert!(!debug.contains("index.sqlite3"));
        assert!(
            LocalArchitectureOverviewRequest::new(Path::new("index"), REPOSITORY_ID)
                .with_limits(0, 1, 1)
                .is_err()
        );
    }

    #[test]
    fn invalid_identity_fails_before_reader_start() {
        let result = overview_local_architecture(
            LocalArchitectureOverviewRequest::new(Path::new("/not/opened.sqlite3"), "invalid")
                .with_deadline(Duration::from_secs(1)),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(result.is_err());
        assert_eq!(
            DEFAULT_LOCAL_ARCHITECTURE_OVERVIEW_DEADLINE,
            Duration::from_secs(30)
        );
    }
}
