//! One-shot local composition for the bounded multi-language architecture map.

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
    ArchitectureMapError, ArchitectureMapLimitError, ArchitectureMapLimits, ArchitectureMapRequest,
    ArchitectureMapResult, RepositoryIdentityTextError, RepositoryIdentityTextV1, architecture_map,
};

use crate::{GenerationId, OwnedSqliteReader, SqliteStoreError};

/// Default end-to-end deadline for one bounded architecture map.
pub const DEFAULT_LOCAL_ARCHITECTURE_MAP_DEADLINE: Duration = Duration::from_secs(30);

/// Exact multi-language source map pinned to one immutable local generation.
pub type LocalArchitectureMapResult = ArchitectureMapResult<GenerationId>;

/// Explicit inputs for one local active-generation map.
#[derive(Clone, Copy)]
pub struct LocalArchitectureMapRequest<'a> {
    database: &'a Path,
    repository_identity: &'a str,
    limits: ArchitectureMapLimits,
    deadline: Duration,
}

impl<'a> LocalArchitectureMapRequest<'a> {
    /// Constructs a request with the conservative public map bounds.
    #[must_use]
    pub fn new(database: &'a Path, repository_identity: &'a str) -> Self {
        Self {
            database,
            repository_identity,
            limits: ArchitectureMapLimits::default(),
            deadline: DEFAULT_LOCAL_ARCHITECTURE_MAP_DEADLINE,
        }
    }

    /// Applies an explicit retained-file limit while preserving the byte ceiling.
    pub fn with_max_files(mut self, max_files: u16) -> Result<Self, ArchitectureMapLimitError> {
        self.limits = ArchitectureMapLimits::try_new(max_files, self.limits.max_output_bytes())?;
        Ok(self)
    }

    /// Replaces the end-to-end deadline duration.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalArchitectureMapRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalArchitectureMapRequest")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("limits", &self.limits)
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Stable content-redacted local architecture-map failure.
#[derive(Debug)]
pub enum LocalArchitectureMapError {
    /// The repository identity text was malformed or non-canonical.
    RepositoryIdentity(RepositoryIdentityTextError),
    /// The requested bound was invalid.
    Limits(ArchitectureMapLimitError),
    /// The absolute deadline could not be represented.
    DeadlineNotRepresentable,
    /// The owned reader could not start.
    ReaderStart(SqliteStoreError),
    /// The shared application map failed.
    Map(ArchitectureMapError<SqliteStoreError>),
    /// The owned reader could not shut down cleanly.
    Shutdown(SqliteStoreError),
}

impl fmt::Display for LocalArchitectureMapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity(_) => "repository identity is invalid",
            Self::Limits(_) => "architecture-map limits are invalid",
            Self::DeadlineNotRepresentable => "architecture-map deadline cannot be represented",
            Self::ReaderStart(_) => "local index reader could not start",
            Self::Map(_) => "local architecture map failed",
            Self::Shutdown(_) => "local index reader could not shut down",
        })
    }
}

impl Error for LocalArchitectureMapError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity(error) => Some(error),
            Self::Limits(error) => Some(error),
            Self::ReaderStart(error) | Self::Shutdown(error) => Some(error),
            Self::Map(error) => Some(error),
            Self::DeadlineNotRepresentable => None,
        }
    }
}

/// Opens one owned reader, maps the active index, then shuts the reader down.
pub fn map_local_architecture(
    request: LocalArchitectureMapRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalArchitectureMapResult, LocalArchitectureMapError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(LocalArchitectureMapError::RepositoryIdentity)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalArchitectureMapError::DeadlineNotRepresentable)?;
    check_facade_control(&cancelled, deadline)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalArchitectureMapError::ReaderStart)?;
    let result = architecture_map(
        &reader,
        ArchitectureMapRequest::new(repository, request.limits, Arc::clone(&cancelled), deadline),
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(LocalArchitectureMapError::Map(error)),
        (Ok(_), Err(error)) => Err(LocalArchitectureMapError::Shutdown(error)),
    }
}

fn check_facade_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalArchitectureMapError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalArchitectureMapError::Map(
            ArchitectureMapError::Cancelled,
        ))
    } else if Instant::now() >= deadline {
        Err(LocalArchitectureMapError::Map(
            ArchitectureMapError::DeadlineExceeded,
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
        DEFAULT_LOCAL_ARCHITECTURE_MAP_DEADLINE, LocalArchitectureMapRequest,
        map_local_architecture,
    };

    const REPOSITORY_ID: &str = concat!(
        "rwi1:h:",
        "0101010101010101010101010101010101010101010101010101010101010101"
    );

    #[test]
    fn request_bounds_and_debug_output_are_explicit_and_redacted() {
        let request =
            LocalArchitectureMapRequest::new(Path::new("/private/index.sqlite3"), REPOSITORY_ID)
                .with_max_files(100)
                .expect("inclusive file ceiling should be valid")
                .with_deadline(Duration::from_secs(1));
        let debug = format!("{request:?}");
        assert!(!debug.contains("/private"));
        assert!(!debug.contains(REPOSITORY_ID));
        assert_eq!(
            DEFAULT_LOCAL_ARCHITECTURE_MAP_DEADLINE,
            Duration::from_secs(30)
        );
        assert!(
            LocalArchitectureMapRequest::new(Path::new("index"), REPOSITORY_ID)
                .with_max_files(0)
                .is_err()
        );
    }

    #[test]
    fn malformed_identity_fails_before_opening_the_database() {
        assert!(
            map_local_architecture(
                LocalArchitectureMapRequest::new(Path::new("/not/opened.sqlite3"), "invalid"),
                Arc::new(AtomicBool::new(false)),
            )
            .is_err()
        );
    }
}
