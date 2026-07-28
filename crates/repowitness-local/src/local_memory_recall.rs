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
    MemoryRecallError, MemoryRecallLimitError, MemoryRecallLimits, MemoryRecallQuery,
    MemoryRecallQueryError, MemoryRecallRequest, MemoryRecallResult, RepositoryIdentityTextError,
    RepositoryIdentityTextV1, memory_recall,
};

use crate::{GenerationId, OwnedSqliteReader, SqliteStoreError};

/// Default end-to-end deadline for one local memory recall.
pub const DEFAULT_LOCAL_MEMORY_RECALL_DEADLINE: Duration = Duration::from_secs(5);

/// Explicit selection mode for a bounded memory recall.
#[derive(Clone, Copy)]
pub enum LocalMemoryRecallSelection<'a> {
    /// Return all projected records subject to the explicit result bounds.
    All,
    /// Match canonical literal terms against selected memory titles and bodies.
    Query(&'a str),
}

impl fmt::Debug for LocalMemoryRecallSelection<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::All => "All",
            Self::Query(_) => "Query(<redacted-query>)",
        })
    }
}

/// Complete local input for one generation-pinned memory recall.
#[derive(Clone, Copy)]
pub struct LocalMemoryRecallRequest<'a> {
    database: &'a Path,
    repository_identity: &'a str,
    selection: LocalMemoryRecallSelection<'a>,
    limits: MemoryRecallLimits,
    deadline: Duration,
}

impl<'a> LocalMemoryRecallRequest<'a> {
    /// Constructs a request using conservative result, byte, and time bounds.
    #[must_use]
    pub fn new(
        database: &'a Path,
        repository_identity: &'a str,
        selection: LocalMemoryRecallSelection<'a>,
    ) -> Self {
        Self {
            database,
            repository_identity,
            selection,
            limits: MemoryRecallLimits::default(),
            deadline: DEFAULT_LOCAL_MEMORY_RECALL_DEADLINE,
        }
    }

    /// Replaces only the inclusive result-count bound.
    pub fn with_max_results(mut self, max_results: u16) -> Result<Self, MemoryRecallLimitError> {
        self.limits = MemoryRecallLimits::try_new(
            max_results,
            self.limits.max_output_bytes(),
            self.limits.max_scan_bytes(),
        )?;
        Ok(self)
    }

    /// Replaces the complete validated recall resource policy.
    #[must_use]
    pub const fn with_limits(mut self, limits: MemoryRecallLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Replaces the end-to-end monotonic deadline duration.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalMemoryRecallRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalMemoryRecallRequest")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("selection", &self.selection)
            .field("limits", &self.limits)
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Validated memory recall pinned to one source generation and projection.
pub type LocalMemoryRecallResult = MemoryRecallResult<GenerationId, i64>;

/// Stable content-redacted failure for one local memory recall.
#[derive(Debug)]
pub enum LocalMemoryRecallError {
    /// The repository identity text was malformed or non-canonical.
    RepositoryIdentity {
        /// Stable identity-validation failure.
        source: RepositoryIdentityTextError,
    },
    /// The literal query violated the bounded recall profile.
    Query {
        /// Stable query-validation failure.
        source: MemoryRecallQueryError,
    },
    /// The absolute deadline could not be represented.
    DeadlineNotRepresentable,
    /// Cancellation was visible before database I/O.
    Cancelled,
    /// The deadline elapsed before database I/O.
    DeadlineExceeded,
    /// The owned read connection could not start.
    ReaderStart {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// The shared application use case failed.
    Recall {
        /// Stable application or SQLite boundary failure.
        source: MemoryRecallError<SqliteStoreError>,
    },
    /// The owned read connection did not shut down cleanly.
    Shutdown {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
}

impl fmt::Display for LocalMemoryRecallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity { .. } => "repository identity is invalid",
            Self::Query { .. } => "memory recall query is invalid",
            Self::DeadlineNotRepresentable => "memory recall deadline is not representable",
            Self::Cancelled => "memory recall was cancelled",
            Self::DeadlineExceeded => "memory recall deadline elapsed",
            Self::ReaderStart { .. } => "memory recall reader startup failed",
            Self::Recall { .. } => "memory recall failed",
            Self::Shutdown { .. } => "memory recall reader shutdown failed",
        })
    }
}

impl Error for LocalMemoryRecallError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity { source } => Some(source),
            Self::Query { source } => Some(source),
            Self::ReaderStart { source } | Self::Shutdown { source } => Some(source),
            Self::Recall { source } => Some(source),
            Self::DeadlineNotRepresentable | Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Opens one owned reader, recalls the active projection, and shuts it down.
pub fn recall_local_memory(
    request: LocalMemoryRecallRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalMemoryRecallResult, LocalMemoryRecallError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(|source| LocalMemoryRecallError::RepositoryIdentity { source })?;
    let query = match request.selection {
        LocalMemoryRecallSelection::All => MemoryRecallQuery::all(),
        LocalMemoryRecallSelection::Query(value) => MemoryRecallQuery::try_new(value)
            .map_err(|source| LocalMemoryRecallError::Query { source })?,
    };
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalMemoryRecallError::DeadlineNotRepresentable)?;
    check_control(cancelled.as_ref(), deadline)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(|source| LocalMemoryRecallError::ReaderStart { source })?;
    let result = memory_recall(
        &reader,
        MemoryRecallRequest::new(repository, query, request.limits, cancelled, deadline),
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(source), _) => Err(LocalMemoryRecallError::Recall { source }),
        (Ok(_), Err(source)) => Err(LocalMemoryRecallError::Shutdown { source }),
    }
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), LocalMemoryRecallError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalMemoryRecallError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(LocalMemoryRecallError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::atomic::AtomicBool};

    use super::*;

    #[test]
    fn invalid_boundaries_and_control_fail_before_database_io() {
        let missing = Path::new("must-not-be-opened.db");
        let cancelled = Arc::new(AtomicBool::new(true));
        let error = recall_local_memory(
            LocalMemoryRecallRequest::new(missing, "invalid", LocalMemoryRecallSelection::All),
            Arc::clone(&cancelled),
        )
        .expect_err("identity validation should fail first");
        assert!(matches!(
            error,
            LocalMemoryRecallError::RepositoryIdentity { .. }
        ));

        let identity = format!("rwi1:h:{}", "00".repeat(32));
        let error = recall_local_memory(
            LocalMemoryRecallRequest::new(
                missing,
                &identity,
                LocalMemoryRecallSelection::Query(""),
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .expect_err("empty query should fail before database I/O");
        assert!(matches!(error, LocalMemoryRecallError::Query { .. }));

        let error = recall_local_memory(
            LocalMemoryRecallRequest::new(missing, &identity, LocalMemoryRecallSelection::All)
                .with_deadline(Duration::ZERO),
            Arc::new(AtomicBool::new(false)),
        )
        .expect_err("zero deadline should fail before database I/O");
        assert!(matches!(error, LocalMemoryRecallError::DeadlineExceeded));
    }

    #[test]
    fn request_debug_and_limits_are_explicit_and_redacted() {
        let request = LocalMemoryRecallRequest::new(
            Path::new("private.db"),
            "private-identity",
            LocalMemoryRecallSelection::Query("private query"),
        )
        .with_max_results(7)
        .expect("valid result limit");
        let debug = format!("{request:?}");
        assert!(debug.contains("max_results: 7"));
        assert!(!debug.contains("private"));
        assert!(
            request
                .with_max_results(0)
                .expect_err("zero result limit must fail")
                .to_string()
                .contains("limits")
        );
    }
}
