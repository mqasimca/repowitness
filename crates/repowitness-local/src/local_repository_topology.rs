//! One-shot local composition for the path-only repository topology inventory.

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
    RepositoryIdentityTextError, RepositoryIdentityTextV1, RepositoryTopologyError,
    RepositoryTopologyLimitError, RepositoryTopologyLimits, RepositoryTopologyRequest,
    RepositoryTopologyResult, repository_topology,
};

use crate::{GenerationId, OwnedSqliteReader, SqliteStoreError};

/// Default end-to-end deadline for one path-only topology read.
pub const DEFAULT_LOCAL_REPOSITORY_TOPOLOGY_DEADLINE: Duration = Duration::from_secs(30);

/// Path-only local topology pinned to one immutable generation.
pub type LocalRepositoryTopologyResult = RepositoryTopologyResult<GenerationId>;

/// Explicit inputs for one local active-generation topology read.
#[derive(Clone, Copy)]
pub struct LocalRepositoryTopologyRequest<'a> {
    database: &'a Path,
    repository_identity: &'a str,
    limits: RepositoryTopologyLimits,
    deadline: Duration,
}

impl<'a> LocalRepositoryTopologyRequest<'a> {
    /// Creates one request with the fixed default limits and deadline.
    #[must_use]
    pub fn new(database: &'a Path, repository_identity: &'a str) -> Self {
        Self {
            database,
            repository_identity,
            limits: RepositoryTopologyLimits::default(),
            deadline: DEFAULT_LOCAL_REPOSITORY_TOPOLOGY_DEADLINE,
        }
    }

    /// Replaces the exact returned-path ceiling.
    pub fn with_max_paths(mut self, max_paths: u16) -> Result<Self, RepositoryTopologyLimitError> {
        self.limits = RepositoryTopologyLimits::try_new(max_paths, self.limits.max_output_bytes())?;
        Ok(self)
    }

    /// Replaces the shared end-to-end deadline.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalRepositoryTopologyRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalRepositoryTopologyRequest")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("limits", &self.limits)
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Stable local topology-composition failure.
#[derive(Debug)]
pub enum LocalRepositoryTopologyError {
    /// Repository identity text was invalid.
    RepositoryIdentity(RepositoryIdentityTextError),
    /// The deadline could not be represented.
    DeadlineNotRepresentable,
    /// Reader startup failed.
    ReaderStart(SqliteStoreError),
    /// The topology use case failed.
    Topology(RepositoryTopologyError<SqliteStoreError>),
    /// Reader shutdown failed after the read completed.
    Shutdown(SqliteStoreError),
}

impl fmt::Display for LocalRepositoryTopologyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity(_) => "repository topology identity is invalid",
            Self::DeadlineNotRepresentable => "repository topology deadline is not representable",
            Self::ReaderStart(_) => "repository topology reader startup failed",
            Self::Topology(_) => "repository topology read failed",
            Self::Shutdown(_) => "repository topology reader shutdown failed",
        })
    }
}

impl Error for LocalRepositoryTopologyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity(source) => Some(source),
            Self::ReaderStart(source) | Self::Shutdown(source) => Some(source),
            Self::Topology(source) => Some(source),
            Self::DeadlineNotRepresentable => None,
        }
    }
}

/// Opens one owned reader, reads the active topology, then shuts it down.
pub fn read_local_repository_topology(
    request: LocalRepositoryTopologyRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalRepositoryTopologyResult, LocalRepositoryTopologyError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(LocalRepositoryTopologyError::RepositoryIdentity)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalRepositoryTopologyError::DeadlineNotRepresentable)?;
    if cancelled.load(Ordering::Acquire) {
        return Err(LocalRepositoryTopologyError::Topology(
            RepositoryTopologyError::Cancelled,
        ));
    }
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalRepositoryTopologyError::ReaderStart)?;
    let result = repository_topology(
        &reader,
        RepositoryTopologyRequest::new(
            repository,
            request.limits,
            Arc::clone(&cancelled),
            deadline,
        ),
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(LocalRepositoryTopologyError::Topology(error)),
        (Ok(_), Err(error)) => Err(LocalRepositoryTopologyError::Shutdown(error)),
    }
}
