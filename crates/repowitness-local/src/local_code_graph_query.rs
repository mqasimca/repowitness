//! One-shot local composition for the finite code-discovery operation algebra.

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
    CodeGraphQueryError, CodeGraphQueryOperation, CodeGraphQueryRequest, CodeGraphQueryResult,
    CodeSearchLimitError, RepositoryIdentityTextError, RepositoryIdentityTextV1,
    ResolvedConfiguration, SyntaxSiteSearchLimitError, SyntaxSiteSearchLimits, code_graph_query,
};

use crate::{
    GenerationId, OwnedSqliteReader, SqliteStoreError, local_search::effective_code_search_limits,
};

/// Default end-to-end deadline for one finite code-discovery operation.
pub const DEFAULT_LOCAL_CODE_GRAPH_QUERY_DEADLINE: Duration = Duration::from_secs(30);

/// Proof-carrying result from exactly one active-generation discovery operation.
pub type LocalCodeGraphQueryResult = CodeGraphQueryResult<GenerationId>;

/// Explicit local inputs for one finite discovery operation.
pub struct LocalCodeGraphQueryRequest<'a> {
    database: &'a Path,
    repository_identity: &'a str,
    operation: CodeGraphQueryOperation<GenerationId>,
    configuration: Option<&'a ResolvedConfiguration>,
    deadline: Duration,
}

impl<'a> LocalCodeGraphQueryRequest<'a> {
    /// Constructs a request with a finite already-validated operation.
    #[must_use]
    pub const fn new(
        database: &'a Path,
        repository_identity: &'a str,
        operation: CodeGraphQueryOperation<GenerationId>,
    ) -> Self {
        Self {
            database,
            repository_identity,
            operation,
            configuration: None,
            deadline: DEFAULT_LOCAL_CODE_GRAPH_QUERY_DEADLINE,
        }
    }

    /// Replaces the end-to-end deadline duration.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// Applies the resolved configuration as a ceiling for embedded code search.
    #[must_use]
    pub const fn with_configuration(mut self, configuration: &'a ResolvedConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }
}

impl fmt::Debug for LocalCodeGraphQueryRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalCodeGraphQueryRequest")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("operation", &self.operation)
            .field(
                "configuration_digest",
                &self.configuration.map(ResolvedConfiguration::digest),
            )
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Stable content-redacted local finite-discovery failure.
#[derive(Debug)]
pub enum LocalCodeGraphQueryError {
    /// The repository identity text was malformed or non-canonical.
    RepositoryIdentity(RepositoryIdentityTextError),
    /// The absolute deadline could not be represented.
    DeadlineNotRepresentable,
    /// Cancellation was observed before the owned reader started.
    Cancelled,
    /// The deadline elapsed before the owned reader started.
    DeadlineExceeded,
    /// The owned reader could not start.
    ReaderStart(SqliteStoreError),
    /// The resolved configuration could not produce a valid embedded search bound.
    Limits(LocalCodeGraphQueryLimitError),
    /// The shared application finite operation failed.
    Query(CodeGraphQueryError<SqliteStoreError>),
    /// The owned reader did not shut down cleanly.
    Shutdown(SqliteStoreError),
}

/// Stable limit-validation class for finite operations with different profiles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalCodeGraphQueryLimitError {
    /// Embedded lexical declaration limits were invalid.
    CodeSearch(CodeSearchLimitError),
    /// Embedded raw target observation limits were invalid.
    SyntaxSiteSearch(SyntaxSiteSearchLimitError),
}

impl fmt::Display for LocalCodeGraphQueryLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CodeSearch(error) => error.fmt(formatter),
            Self::SyntaxSiteSearch(error) => error.fmt(formatter),
        }
    }
}

impl Error for LocalCodeGraphQueryLimitError {}

impl fmt::Display for LocalCodeGraphQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity(_) => "repository identity is invalid",
            Self::DeadlineNotRepresentable => "code graph query deadline cannot be represented",
            Self::Cancelled => "code graph query cancelled",
            Self::DeadlineExceeded => "code graph query deadline exceeded",
            Self::ReaderStart(_) => "local index reader could not start",
            Self::Limits(_) => "code graph query limits are invalid",
            Self::Query(_) => "local code graph query failed",
            Self::Shutdown(_) => "local index reader could not shut down",
        })
    }
}

impl Error for LocalCodeGraphQueryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity(error) => Some(error),
            Self::ReaderStart(error) | Self::Shutdown(error) => Some(error),
            Self::Limits(error) => Some(error),
            Self::Query(error) => Some(error),
            Self::DeadlineNotRepresentable | Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Opens one owned reader, dispatches exactly one application operation, and shuts it down.
pub fn read_local_code_graph_query(
    request: LocalCodeGraphQueryRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalCodeGraphQueryResult, LocalCodeGraphQueryError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(LocalCodeGraphQueryError::RepositoryIdentity)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalCodeGraphQueryError::DeadlineNotRepresentable)?;
    if cancelled.load(Ordering::Acquire) {
        return Err(LocalCodeGraphQueryError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(LocalCodeGraphQueryError::DeadlineExceeded);
    }
    let operation = effective_operation(request.operation, request.configuration)
        .map_err(LocalCodeGraphQueryError::Limits)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalCodeGraphQueryError::ReaderStart)?;
    let result = code_graph_query(
        &reader,
        CodeGraphQueryRequest::new(repository, operation, Arc::clone(&cancelled), deadline),
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(LocalCodeGraphQueryError::Query(error)),
        (Ok(_), Err(error)) => Err(LocalCodeGraphQueryError::Shutdown(error)),
    }
}

fn effective_operation(
    operation: CodeGraphQueryOperation<GenerationId>,
    configuration: Option<&ResolvedConfiguration>,
) -> Result<CodeGraphQueryOperation<GenerationId>, LocalCodeGraphQueryLimitError> {
    match operation {
        CodeGraphQueryOperation::RelevantPaths {
            query,
            search_limits,
            path_limits,
        } => Ok(CodeGraphQueryOperation::RelevantPaths {
            query,
            search_limits: effective_code_search_limits(search_limits, configuration)
                .map_err(LocalCodeGraphQueryLimitError::CodeSearch)?,
            path_limits,
        }),
        CodeGraphQueryOperation::SyntaxSiteSearch { query, limits } => {
            let configured_max = configuration
                .map_or(u64::from(limits.max_results()), |configuration| {
                    *configuration.preferences().query_results().effective()
                });
            let max_results = u64::from(limits.max_results()).min(configured_max);
            let max_results = u16::try_from(max_results).map_err(|_| {
                LocalCodeGraphQueryLimitError::SyntaxSiteSearch(SyntaxSiteSearchLimitError)
            })?;
            let limits = SyntaxSiteSearchLimits::try_new(max_results, limits.max_output_bytes())
                .map_err(LocalCodeGraphQueryLimitError::SyntaxSiteSearch)?;
            Ok(CodeGraphQueryOperation::SyntaxSiteSearch { query, limits })
        }
        operation => Ok(operation),
    }
}

#[cfg(test)]
mod tests {
    use repowitness_application::{
        CodeSearchLimits, CodeSearchQuery, RelevantPathsLimits, SyntaxSiteSearchLimits,
        SyntaxSiteSearchQuery, resolve_configuration,
    };

    use crate::{ConfigurationFileLayer, GenerationId, parse_configuration_file};

    use super::{CodeGraphQueryOperation, effective_operation};

    #[test]
    fn resolved_configuration_caps_embedded_relevant_path_search() {
        let layer = parse_configuration_file(
            b"schema_version = 1\n[preferences]\nquery_results = 3\n",
            ConfigurationFileLayer::User,
        )
        .expect("configuration should parse");
        let configuration = resolve_configuration(&[layer]).expect("configuration should resolve");
        let defaults = CodeSearchLimits::default();
        let operation: CodeGraphQueryOperation<GenerationId> =
            CodeGraphQueryOperation::RelevantPaths {
                query: CodeSearchQuery::try_new("Widget").expect("query should be valid"),
                search_limits: CodeSearchLimits::try_new(48, defaults.max_output_bytes())
                    .expect("search limits should be valid"),
                path_limits: RelevantPathsLimits::try_new(12).expect("path limits should be valid"),
            };
        let operation = effective_operation(operation, Some(&configuration))
            .expect("configuration must only tighten the embedded search");
        let CodeGraphQueryOperation::RelevantPaths {
            search_limits,
            path_limits,
            ..
        } = operation
        else {
            panic!("operation variant must be preserved");
        };
        assert_eq!(search_limits.max_results(), 3);
        assert_eq!(path_limits.max_paths(), 12);
    }

    #[test]
    fn resolved_configuration_caps_embedded_raw_target_search() {
        let layer = parse_configuration_file(
            b"schema_version = 1\n[preferences]\nquery_results = 3\n",
            ConfigurationFileLayer::User,
        )
        .expect("configuration should parse");
        let configuration = resolve_configuration(&[layer]).expect("configuration should resolve");
        let defaults = SyntaxSiteSearchLimits::default();
        let operation: CodeGraphQueryOperation<GenerationId> =
            CodeGraphQueryOperation::SyntaxSiteSearch {
                query: SyntaxSiteSearchQuery::try_new("run").expect("query should be valid"),
                limits: SyntaxSiteSearchLimits::try_new(48, defaults.max_output_bytes())
                    .expect("search limits should be valid"),
            };
        let operation = effective_operation(operation, Some(&configuration))
            .expect("configuration must only tighten the embedded raw target search");
        let CodeGraphQueryOperation::SyntaxSiteSearch { limits, .. } = operation else {
            panic!("operation variant must be preserved");
        };
        assert_eq!(limits.max_results(), 3);
    }
}
