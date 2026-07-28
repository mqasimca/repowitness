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
    RepositoryDiagnosticsError, RepositoryDiagnosticsRequest, RepositoryDiagnosticsResult,
    RepositoryIdentityTextError, RepositoryIdentityTextV1, repository_diagnostics,
};

use crate::{GenerationId, OwnedSqliteReader, SqliteStoreError};

/// Default end-to-end deadline for one local diagnostics read.
pub const DEFAULT_LOCAL_DIAGNOSTICS_DEADLINE: Duration = Duration::from_secs(5);

/// Complete local input for one read-only repository diagnostics operation.
#[derive(Clone, Copy)]
pub struct LocalRepositoryDiagnosticsRequest<'a> {
    database: &'a Path,
    repository_identity: &'a str,
    deadline: Duration,
}

impl<'a> LocalRepositoryDiagnosticsRequest<'a> {
    /// Constructs a request using the conservative default deadline.
    #[must_use]
    pub const fn new(database: &'a Path, repository_identity: &'a str) -> Self {
        Self {
            database,
            repository_identity,
            deadline: DEFAULT_LOCAL_DIAGNOSTICS_DEADLINE,
        }
    }

    /// Replaces the end-to-end monotonic deadline duration.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalRepositoryDiagnosticsRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalRepositoryDiagnosticsRequest")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Validated diagnostics pinned to one active source generation.
pub type LocalRepositoryDiagnosticsResult = RepositoryDiagnosticsResult<GenerationId, i64>;

/// Stable content-redacted failure for one local diagnostics read.
#[derive(Debug)]
pub enum LocalRepositoryDiagnosticsError {
    /// The repository identity text was malformed or non-canonical.
    RepositoryIdentity(RepositoryIdentityTextError),
    /// The absolute deadline could not be represented.
    DeadlineNotRepresentable,
    /// Cancellation was visible before database I/O.
    Cancelled,
    /// The deadline elapsed before database I/O.
    DeadlineExceeded,
    /// The owned read connection could not start.
    ReaderStart(SqliteStoreError),
    /// The shared application use case failed.
    Diagnostics(RepositoryDiagnosticsError<SqliteStoreError>),
    /// The owned read connection did not shut down cleanly.
    Shutdown(SqliteStoreError),
}

impl fmt::Display for LocalRepositoryDiagnosticsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity(_) => "repository identity is invalid",
            Self::DeadlineNotRepresentable => "diagnostics deadline is not representable",
            Self::Cancelled => "repository diagnostics was cancelled",
            Self::DeadlineExceeded => "repository diagnostics deadline elapsed",
            Self::ReaderStart(_) => "repository diagnostics reader startup failed",
            Self::Diagnostics(_) => "repository diagnostics failed",
            Self::Shutdown(_) => "repository diagnostics reader shutdown failed",
        })
    }
}

impl Error for LocalRepositoryDiagnosticsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity(source) => Some(source),
            Self::ReaderStart(source) | Self::Shutdown(source) => Some(source),
            Self::Diagnostics(source) => Some(source),
            Self::DeadlineNotRepresentable | Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Opens one owned reader, reads diagnostics, and shuts it down.
pub fn diagnose_local_repository(
    request: LocalRepositoryDiagnosticsRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalRepositoryDiagnosticsResult, LocalRepositoryDiagnosticsError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(LocalRepositoryDiagnosticsError::RepositoryIdentity)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalRepositoryDiagnosticsError::DeadlineNotRepresentable)?;
    check_control(&cancelled, deadline)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalRepositoryDiagnosticsError::ReaderStart)?;
    let result = repository_diagnostics(
        &reader,
        RepositoryDiagnosticsRequest::new(repository, cancelled, deadline),
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(source), _) => Err(LocalRepositoryDiagnosticsError::Diagnostics(source)),
        (Ok(_), Err(source)) => Err(LocalRepositoryDiagnosticsError::Shutdown(source)),
    }
}

fn check_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalRepositoryDiagnosticsError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalRepositoryDiagnosticsError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(LocalRepositoryDiagnosticsError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
