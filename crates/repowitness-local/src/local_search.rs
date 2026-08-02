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
    CodeSearchError, CodeSearchLimitError, CodeSearchLimits, CodeSearchPort, CodeSearchPortResult,
    CodeSearchQuery, CodeSearchQueryError, CodeSearchRequest, CodeSearchResult,
    ConnectedWorkspaceIdTextV1, RepositoryIdentityTextError, RepositoryIdentityTextV1,
    ResolvedConfiguration, SourceSlotIdTextV1, WorkspaceIdentityTextError, code_search,
};
use repowitness_domain::{ConnectedWorkspaceId, RepositoryIdentityDigest, SourceSlotId};

use crate::{GenerationId, OwnedSqliteReader, PinnedWorkspaceView, SearchLimits, SqliteStoreError};

/// Default end-to-end deadline for one local lexical query.
pub const DEFAULT_LOCAL_CODE_SEARCH_DEADLINE: Duration = Duration::from_secs(5);

/// Proof-carrying local search result pinned to one SQLite generation.
pub type LocalCodeSearchResult = CodeSearchResult<GenerationId>;

/// Explicit single-repository or connected-source-slot query context.
#[derive(Clone, Copy)]
pub enum LocalCodeSearchWorkspace<'a> {
    /// One repository's compatible single-source workspace.
    SingleRepository {
        /// Canonical repository identity text.
        repository_identity: &'a str,
    },
    /// One selected member of a connected workspace.
    ConnectedWorkspace {
        /// Canonical connected-workspace identity text.
        connected_workspace: &'a str,
        /// Canonical source-slot identity text.
        source_slot: &'a str,
    },
}

/// Explicit inputs for one local database query.
#[derive(Clone, Copy)]
pub struct LocalCodeSearchRequest<'a> {
    database: &'a Path,
    workspace: LocalCodeSearchWorkspace<'a>,
    query: &'a str,
    limits: CodeSearchLimits,
    configuration: Option<&'a ResolvedConfiguration>,
    deadline: Duration,
}

impl<'a> LocalCodeSearchRequest<'a> {
    /// Constructs a request with conservative Phase 0 limits.
    #[must_use]
    pub fn new(database: &'a Path, repository_identity: &'a str, query: &'a str) -> Self {
        Self {
            database,
            workspace: LocalCodeSearchWorkspace::SingleRepository {
                repository_identity,
            },
            query,
            limits: CodeSearchLimits::default(),
            configuration: None,
            deadline: DEFAULT_LOCAL_CODE_SEARCH_DEADLINE,
        }
    }

    /// Constructs a search pinned to one selected source slot of a connected workspace.
    #[must_use]
    pub fn for_connected_workspace(
        database: &'a Path,
        connected_workspace: &'a str,
        source_slot: &'a str,
        query: &'a str,
    ) -> Self {
        Self {
            database,
            workspace: LocalCodeSearchWorkspace::ConnectedWorkspace {
                connected_workspace,
                source_slot,
            },
            query,
            limits: CodeSearchLimits::default(),
            configuration: None,
            deadline: DEFAULT_LOCAL_CODE_SEARCH_DEADLINE,
        }
    }

    /// Applies an explicit candidate limit while preserving the default byte ceiling.
    pub fn with_max_results(mut self, max_results: u16) -> Result<Self, CodeSearchLimitError> {
        self.limits = CodeSearchLimits::try_new(max_results, self.limits.max_output_bytes())?;
        Ok(self)
    }

    /// Applies a resolved configuration as an additional query-result ceiling.
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

impl fmt::Debug for LocalCodeSearchRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalCodeSearchRequest")
            .field("database", &"<redacted-path>")
            .field("workspace", &self.workspace)
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

impl fmt::Debug for LocalCodeSearchWorkspace<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::SingleRepository { .. } => "single_repository",
            Self::ConnectedWorkspace { .. } => "connected_workspace",
        };
        formatter
            .debug_struct("LocalCodeSearchWorkspace")
            .field("kind", &kind)
            .finish_non_exhaustive()
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
    /// The connected-workspace identity text is malformed or non-canonical.
    ConnectedWorkspaceIdentity {
        /// Stable boundary validation failure.
        source: WorkspaceIdentityTextError,
    },
    /// The source-slot identity text is malformed or non-canonical.
    SourceSlotIdentity {
        /// Stable boundary validation failure.
        source: WorkspaceIdentityTextError,
    },
    /// The query violates the shared literal profile.
    Query {
        /// Stable query validation failure.
        source: CodeSearchQueryError,
    },
    /// Resolved configuration could not produce a valid effective query bound.
    Limits {
        /// Stable limit validation failure.
        source: CodeSearchLimitError,
    },
    /// The absolute deadline cannot be represented.
    DeadlineNotRepresentable,
    /// The owned read connection could not start.
    ReaderStart {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// The selected connected workspace or source slot is unavailable.
    WorkspaceUnavailable,
    /// Pinning the selected workspace view failed.
    Workspace {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// The selected source slot changed after view selection.
    WorkspaceGenerationChanged,
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
            Self::ConnectedWorkspaceIdentity { .. } => "connected workspace identity is invalid",
            Self::SourceSlotIdentity { .. } => "source slot identity is invalid",
            Self::Query { .. } => "code-search query is invalid",
            Self::Limits { .. } => "code-search limits are invalid",
            Self::DeadlineNotRepresentable => "code-search deadline cannot be represented",
            Self::ReaderStart { .. } => "local index reader could not start",
            Self::WorkspaceUnavailable => "code-search workspace view is unavailable",
            Self::Workspace { .. } => "code-search workspace view read failed",
            Self::WorkspaceGenerationChanged => {
                "code-search source changed after workspace-view selection"
            }
            Self::Search { .. } => "local code search failed",
            Self::Shutdown { .. } => "local index reader could not shut down",
        })
    }
}

impl Error for LocalCodeSearchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity { source } => Some(source),
            Self::ConnectedWorkspaceIdentity { source } | Self::SourceSlotIdentity { source } => {
                Some(source)
            }
            Self::Query { source } => Some(source),
            Self::Limits { source } => Some(source),
            Self::ReaderStart { source } => Some(source),
            Self::Workspace { source } => Some(source),
            Self::Search { source } => Some(source),
            Self::Shutdown { source } => Some(source),
            Self::DeadlineNotRepresentable
            | Self::WorkspaceUnavailable
            | Self::WorkspaceGenerationChanged => None,
        }
    }
}

/// Opens one owned reader, runs the shared application search, and shuts it down.
pub fn search_local_rust_index(
    request: LocalCodeSearchRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalCodeSearchResult, LocalCodeSearchError> {
    validate_workspace_identity(request.workspace)?;
    let query = CodeSearchQuery::try_new(request.query)
        .map_err(|source| LocalCodeSearchError::Query { source })?;
    let limits = effective_search_limits(&request)
        .map_err(|source| LocalCodeSearchError::Limits { source })?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalCodeSearchError::DeadlineNotRepresentable)?;
    check_facade_control(&cancelled, deadline)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(|source| LocalCodeSearchError::ReaderStart { source })?;
    let workspace =
        match selected_workspace(&reader, request.workspace, Arc::clone(&cancelled), deadline) {
            Ok(workspace) => workspace,
            Err(error) => {
                let shutdown = reader.shutdown(deadline);
                return match shutdown {
                    Ok(()) => Err(error),
                    Err(source) => Err(LocalCodeSearchError::Shutdown { source }),
                };
            }
        };
    let result = match workspace.view.as_ref() {
        None => code_search(
            &reader,
            CodeSearchRequest::new(
                workspace.repository,
                query,
                limits,
                Arc::clone(&cancelled),
                deadline,
            ),
        ),
        Some(view) => code_search(
            &ConnectedWorkspaceCodeSearchPort {
                reader: &reader,
                view,
                source_slot: workspace.source_slot,
            },
            CodeSearchRequest::new(
                workspace.repository,
                query,
                limits,
                Arc::clone(&cancelled),
                deadline,
            ),
        ),
    };
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) if *result.generation() == workspace.generation => Ok(result),
        (Ok(_), Ok(())) => Err(LocalCodeSearchError::WorkspaceGenerationChanged),
        (Err(source), _) => Err(LocalCodeSearchError::Search { source }),
        (Ok(_), Err(source)) => Err(LocalCodeSearchError::Shutdown { source }),
    }
}

fn effective_search_limits(
    request: &LocalCodeSearchRequest<'_>,
) -> Result<CodeSearchLimits, CodeSearchLimitError> {
    effective_code_search_limits(request.limits, request.configuration)
}

/// Applies the resolved local query-result ceiling to an already validated search bound.
///
/// This shared helper keeps composite read operations from bypassing the same
/// configuration policy enforced by the direct code-search facade.
pub(crate) fn effective_code_search_limits(
    limits: CodeSearchLimits,
    configuration: Option<&ResolvedConfiguration>,
) -> Result<CodeSearchLimits, CodeSearchLimitError> {
    let configured_max = configuration.map_or(u64::from(limits.max_results()), |configuration| {
        *configuration.preferences().query_results().effective()
    });
    let effective_max = u64::from(limits.max_results()).min(configured_max);
    let effective_max = u16::try_from(effective_max).map_err(|_| CodeSearchLimitError)?;
    CodeSearchLimits::try_new(effective_max, limits.max_output_bytes())
}

/// Searches the active local supported-language index.
pub fn search_local_index(
    request: LocalCodeSearchRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalCodeSearchResult, LocalCodeSearchError> {
    search_local_rust_index(request, cancelled)
}

struct SelectedWorkspace {
    repository: RepositoryIdentityDigest,
    generation: GenerationId,
    source_slot: SourceSlotId,
    view: Option<PinnedWorkspaceView>,
}

fn validate_workspace_identity(
    workspace: LocalCodeSearchWorkspace<'_>,
) -> Result<(), LocalCodeSearchError> {
    match workspace {
        LocalCodeSearchWorkspace::SingleRepository {
            repository_identity,
        } => {
            RepositoryIdentityTextV1::decode(repository_identity)
                .map_err(|source| LocalCodeSearchError::RepositoryIdentity { source })?;
        }
        LocalCodeSearchWorkspace::ConnectedWorkspace {
            connected_workspace,
            source_slot,
        } => {
            ConnectedWorkspaceIdTextV1::decode(connected_workspace)
                .map_err(|source| LocalCodeSearchError::ConnectedWorkspaceIdentity { source })?;
            SourceSlotIdTextV1::decode(source_slot)
                .map_err(|source| LocalCodeSearchError::SourceSlotIdentity { source })?;
        }
    }
    Ok(())
}

fn selected_workspace(
    reader: &OwnedSqliteReader,
    workspace: LocalCodeSearchWorkspace<'_>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<SelectedWorkspace, LocalCodeSearchError> {
    let (connected_workspace, requested_slot) = match workspace {
        LocalCodeSearchWorkspace::SingleRepository {
            repository_identity,
        } => {
            let repository = RepositoryIdentityTextV1::decode(repository_identity)
                .map_err(|source| LocalCodeSearchError::RepositoryIdentity { source })?;
            (
                ConnectedWorkspaceId::for_single_repository(repository),
                None,
            )
        }
        LocalCodeSearchWorkspace::ConnectedWorkspace {
            connected_workspace,
            source_slot,
        } => (
            ConnectedWorkspaceIdTextV1::decode(connected_workspace)
                .map_err(|source| LocalCodeSearchError::ConnectedWorkspaceIdentity { source })?,
            Some(
                SourceSlotIdTextV1::decode(source_slot)
                    .map_err(|source| LocalCodeSearchError::SourceSlotIdentity { source })?,
            ),
        ),
    };
    let view = reader
        .pin_workspace_view(connected_workspace, None, cancelled, deadline)
        .map_err(|source| LocalCodeSearchError::Workspace { source })?
        .ok_or(LocalCodeSearchError::WorkspaceUnavailable)?;
    let member = match requested_slot {
        Some(source_slot) => view
            .members()
            .iter()
            .find(|member| member.source_slot() == source_slot)
            .ok_or(LocalCodeSearchError::WorkspaceUnavailable)?,
        None => {
            let [member] = view.members() else {
                return Err(LocalCodeSearchError::WorkspaceUnavailable);
            };
            member
        }
    };
    Ok(SelectedWorkspace {
        repository: member.repository(),
        generation: member.generation(),
        source_slot: member.source_slot(),
        view: requested_slot.map(|_| view),
    })
}

struct ConnectedWorkspaceCodeSearchPort<'a> {
    reader: &'a OwnedSqliteReader,
    view: &'a PinnedWorkspaceView,
    source_slot: SourceSlotId,
}

impl CodeSearchPort for ConnectedWorkspaceCodeSearchPort<'_> {
    type Generation = GenerationId;
    type Error = SqliteStoreError;

    fn search(
        &self,
        _repository: RepositoryIdentityDigest,
        query: &CodeSearchQuery,
        limits: CodeSearchLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<CodeSearchPortResult<Self::Generation>, Self::Error> {
        let storage_limits =
            SearchLimits::try_new(limits.max_results(), limits.max_output_bytes())?;
        let result = self.reader.search_workspace_member(
            self.view,
            self.source_slot,
            query.as_str(),
            storage_limits,
            cancelled,
            deadline,
        )?;
        crate::sqlite::code_search_port_result_from_search_results(result)
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

    use repowitness_application::resolve_configuration;

    use crate::{ConfigurationFileLayer, parse_configuration_file};

    use super::{
        LocalCodeSearchError, LocalCodeSearchRequest, effective_search_limits,
        search_local_rust_index,
    };

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
    fn resolved_configuration_can_only_tighten_query_results() {
        let layer = parse_configuration_file(
            b"schema_version = 1\n[preferences]\nquery_results = 3\n",
            ConfigurationFileLayer::User,
        )
        .expect("configuration should parse");
        let configuration = resolve_configuration(&[layer]).expect("configuration should resolve");
        let configured = LocalCodeSearchRequest::new(Path::new("index"), REPOSITORY_ID, "symbol")
            .with_max_results(100)
            .expect("explicit limit should be valid")
            .with_configuration(&configuration);
        let tightened =
            effective_search_limits(&configured).expect("effective limit should be valid");
        assert_eq!(tightened.max_results(), 3);

        let narrower = LocalCodeSearchRequest::new(Path::new("index"), REPOSITORY_ID, "symbol")
            .with_max_results(2)
            .expect("explicit limit should be valid")
            .with_configuration(&configuration);
        let preserved =
            effective_search_limits(&narrower).expect("effective limit should be valid");
        assert_eq!(preserved.max_results(), 2);
        assert!(format!("{configured:?}").contains("configuration_digest"));
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
