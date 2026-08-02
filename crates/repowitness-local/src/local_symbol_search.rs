//! One-shot local composition for bounded typed declaration discovery.

use std::{
    error::Error,
    fmt,
    ops::Deref,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use repowitness_analysis::RustSymbolKind;
use repowitness_application::{
    CodeSearchLimitError, CodeSearchLimits, ConnectedWorkspaceIdTextV1,
    RepositoryIdentityTextError, RepositoryIdentityTextV1, ResolvedConfiguration, SourceLanguage,
    SourceSlotIdTextV1, SymbolSearchError, SymbolSearchNameMatch, SymbolSearchPort,
    SymbolSearchPortResult, SymbolSearchQuery, SymbolSearchQueryError, SymbolSearchRequest,
    SymbolSearchResult, WorkspaceIdentityTextError, symbol_search,
};
use repowitness_domain::{ConnectedWorkspaceId, RepositoryIdentityDigest, SourceSlotId};

use crate::{GenerationId, OwnedSqliteReader, PinnedWorkspaceView, SearchLimits, SqliteStoreError};

/// Default end-to-end deadline for one local typed declaration search.
pub const DEFAULT_LOCAL_SYMBOL_SEARCH_DEADLINE: Duration = Duration::from_secs(5);

/// Proof-carrying local declaration result pinned to one workspace view and generation.
pub struct LocalSymbolSearchResult {
    result: SymbolSearchResult<GenerationId>,
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: i64,
    source_slot: SourceSlotId,
}

impl LocalSymbolSearchResult {
    /// Returns the underlying exact typed-declaration result.
    #[must_use]
    pub const fn result(&self) -> &SymbolSearchResult<GenerationId> {
        &self.result
    }

    /// Returns the selected connected workspace.
    #[must_use]
    pub const fn connected_workspace(&self) -> ConnectedWorkspaceId {
        self.connected_workspace
    }

    /// Returns the exact immutable workspace view used to select the source slot.
    #[must_use]
    pub const fn workspace_view(&self) -> i64 {
        self.workspace_view
    }

    /// Returns the selected source slot.
    #[must_use]
    pub const fn source_slot(&self) -> SourceSlotId {
        self.source_slot
    }

    /// Consumes the local result into its typed result and immutable workspace receipt.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        SymbolSearchResult<GenerationId>,
        ConnectedWorkspaceId,
        i64,
        SourceSlotId,
    ) {
        (
            self.result,
            self.connected_workspace,
            self.workspace_view,
            self.source_slot,
        )
    }
}

impl Deref for LocalSymbolSearchResult {
    type Target = SymbolSearchResult<GenerationId>;

    fn deref(&self) -> &Self::Target {
        &self.result
    }
}

/// Explicit single-repository or connected-source-slot search context.
#[derive(Clone, Copy)]
pub enum LocalSymbolSearchWorkspace<'a> {
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

/// Explicit inputs for one local declaration discovery request.
#[derive(Clone, Copy)]
pub struct LocalSymbolSearchRequest<'a> {
    database: &'a Path,
    workspace: LocalSymbolSearchWorkspace<'a>,
    name: &'a str,
    name_match: SymbolSearchNameMatch,
    language: Option<SourceLanguage>,
    kind: Option<RustSymbolKind>,
    path_prefix: Option<&'a str>,
    limits: CodeSearchLimits,
    configuration: Option<&'a ResolvedConfiguration>,
    deadline: Duration,
}

impl<'a> LocalSymbolSearchRequest<'a> {
    /// Constructs an all-language exact declaration search with default bounds.
    #[must_use]
    pub fn new(
        database: &'a Path,
        repository_identity: &'a str,
        name: &'a str,
        name_match: SymbolSearchNameMatch,
    ) -> Self {
        Self {
            database,
            workspace: LocalSymbolSearchWorkspace::SingleRepository {
                repository_identity,
            },
            name,
            name_match,
            language: None,
            kind: None,
            path_prefix: None,
            limits: CodeSearchLimits::default(),
            configuration: None,
            deadline: DEFAULT_LOCAL_SYMBOL_SEARCH_DEADLINE,
        }
    }

    /// Constructs an all-language exact declaration search for one connected source slot.
    #[must_use]
    pub fn for_connected_workspace(
        database: &'a Path,
        connected_workspace: &'a str,
        source_slot: &'a str,
        name: &'a str,
        name_match: SymbolSearchNameMatch,
    ) -> Self {
        Self {
            database,
            workspace: LocalSymbolSearchWorkspace::ConnectedWorkspace {
                connected_workspace,
                source_slot,
            },
            name,
            name_match,
            language: None,
            kind: None,
            path_prefix: None,
            limits: CodeSearchLimits::default(),
            configuration: None,
            deadline: DEFAULT_LOCAL_SYMBOL_SEARCH_DEADLINE,
        }
    }

    /// Applies only direct-fact filters that are persisted by the index.
    #[must_use]
    pub const fn with_filters(
        mut self,
        language: Option<SourceLanguage>,
        kind: Option<RustSymbolKind>,
        path_prefix: Option<&'a str>,
    ) -> Self {
        self.language = language;
        self.kind = kind;
        self.path_prefix = path_prefix;
        self
    }

    /// Applies an explicit result limit while preserving the default byte limit.
    pub fn with_max_results(mut self, max_results: u16) -> Result<Self, CodeSearchLimitError> {
        self.limits = CodeSearchLimits::try_new(max_results, self.limits.max_output_bytes())?;
        Ok(self)
    }

    /// Applies a resolved configuration as an additional result ceiling.
    #[must_use]
    pub const fn with_configuration(mut self, configuration: &'a ResolvedConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }

    /// Replaces the end-to-end deadline duration.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalSymbolSearchRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalSymbolSearchRequest")
            .field("database", &"<redacted-path>")
            .field("workspace", &self.workspace)
            .field("name", &"<redacted-symbol>")
            .field("name_match", &self.name_match)
            .field("language", &self.language)
            .field("kind", &self.kind)
            .field("path_prefix", &self.path_prefix.map(|_| "<redacted-path>"))
            .field("limits", &self.limits)
            .field(
                "configuration_digest",
                &self.configuration.map(ResolvedConfiguration::digest),
            )
            .field("deadline", &self.deadline)
            .finish()
    }
}

impl fmt::Debug for LocalSymbolSearchWorkspace<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::SingleRepository { .. } => "single_repository",
            Self::ConnectedWorkspace { .. } => "connected_workspace",
        };
        formatter
            .debug_struct("LocalSymbolSearchWorkspace")
            .field("kind", &kind)
            .finish_non_exhaustive()
    }
}

/// Stable content-redacted local typed declaration-search failure.
#[derive(Debug)]
pub enum LocalSymbolSearchError {
    /// The repository identity text was malformed or non-canonical.
    RepositoryIdentity(RepositoryIdentityTextError),
    /// The connected-workspace identity text was malformed or non-canonical.
    ConnectedWorkspaceIdentity(WorkspaceIdentityTextError),
    /// The source-slot identity text was malformed or non-canonical.
    SourceSlotIdentity(WorkspaceIdentityTextError),
    /// The typed selector was not admitted.
    Query(SymbolSearchQueryError),
    /// The requested bounds were invalid.
    Limits(CodeSearchLimitError),
    /// The absolute deadline cannot be represented.
    DeadlineNotRepresentable,
    /// The owned reader could not start.
    ReaderStart(SqliteStoreError),
    /// The selected workspace view or source slot was unavailable.
    WorkspaceUnavailable,
    /// Reading the selected workspace view failed.
    Workspace(SqliteStoreError),
    /// The source changed after workspace-view selection.
    WorkspaceGenerationChanged,
    /// The shared application search failed.
    Search(SymbolSearchError<SqliteStoreError>),
    /// The owned reader did not shut down cleanly.
    Shutdown(SqliteStoreError),
}

impl fmt::Display for LocalSymbolSearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity(_) => "repository identity is invalid",
            Self::ConnectedWorkspaceIdentity(_) => "connected workspace identity is invalid",
            Self::SourceSlotIdentity(_) => "source slot identity is invalid",
            Self::Query(_) => "symbol-search query is invalid",
            Self::Limits(_) => "symbol-search limits are invalid",
            Self::DeadlineNotRepresentable => "symbol-search deadline cannot be represented",
            Self::ReaderStart(_) => "local index reader could not start",
            Self::WorkspaceUnavailable => "symbol-search workspace view is unavailable",
            Self::Workspace(_) => "symbol-search workspace view read failed",
            Self::WorkspaceGenerationChanged => {
                "symbol-search source changed after workspace-view selection"
            }
            Self::Search(_) => "local symbol search failed",
            Self::Shutdown(_) => "local index reader could not shut down",
        })
    }
}

impl Error for LocalSymbolSearchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity(error) => Some(error),
            Self::ConnectedWorkspaceIdentity(error) | Self::SourceSlotIdentity(error) => {
                Some(error)
            }
            Self::Query(error) => Some(error),
            Self::Limits(error) => Some(error),
            Self::ReaderStart(error) | Self::Shutdown(error) => Some(error),
            Self::Workspace(error) => Some(error),
            Self::Search(error) => Some(error),
            Self::DeadlineNotRepresentable
            | Self::WorkspaceUnavailable
            | Self::WorkspaceGenerationChanged => None,
        }
    }
}

/// Opens one owned reader, discovers typed direct declarations, and shuts it down.
pub fn search_local_symbols(
    request: LocalSymbolSearchRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalSymbolSearchResult, LocalSymbolSearchError> {
    validate_workspace_identity(request.workspace)?;
    let query = SymbolSearchQuery::try_new_with_filters(
        request.name,
        request.name_match,
        request.language,
        request.kind,
        request.path_prefix,
    )
    .map_err(LocalSymbolSearchError::Query)?;
    let limits = effective_search_limits(&request).map_err(LocalSymbolSearchError::Limits)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalSymbolSearchError::DeadlineNotRepresentable)?;
    check_facade_control(&cancelled, deadline)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalSymbolSearchError::ReaderStart)?;
    let workspace =
        match selected_workspace(&reader, request.workspace, Arc::clone(&cancelled), deadline) {
            Ok(workspace) => workspace,
            Err(error) => {
                let shutdown = reader.shutdown(deadline);
                return match shutdown {
                    Ok(()) => Err(error),
                    Err(shutdown) => Err(LocalSymbolSearchError::Shutdown(shutdown)),
                };
            }
        };
    let result = match workspace.view.as_ref() {
        None => symbol_search(
            &reader,
            SymbolSearchRequest::new(
                workspace.repository,
                query,
                limits,
                Arc::clone(&cancelled),
                deadline,
            ),
        ),
        Some(view) => symbol_search(
            &ConnectedWorkspaceSymbolSearchPort {
                reader: &reader,
                view,
                source_slot: workspace.source_slot,
            },
            SymbolSearchRequest::new(
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
        (Ok(result), Ok(())) => {
            if *result.generation() != workspace.generation {
                return Err(LocalSymbolSearchError::WorkspaceGenerationChanged);
            }
            Ok(LocalSymbolSearchResult {
                result,
                connected_workspace: workspace.connected_workspace,
                workspace_view: workspace.workspace_view,
                source_slot: workspace.source_slot,
            })
        }
        (Err(error), _) => Err(LocalSymbolSearchError::Search(error)),
        (Ok(_), Err(error)) => Err(LocalSymbolSearchError::Shutdown(error)),
    }
}

struct SelectedWorkspace {
    repository: RepositoryIdentityDigest,
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: i64,
    source_slot: SourceSlotId,
    generation: GenerationId,
    view: Option<PinnedWorkspaceView>,
}

fn validate_workspace_identity(
    workspace: LocalSymbolSearchWorkspace<'_>,
) -> Result<(), LocalSymbolSearchError> {
    match workspace {
        LocalSymbolSearchWorkspace::SingleRepository {
            repository_identity,
        } => {
            RepositoryIdentityTextV1::decode(repository_identity)
                .map_err(LocalSymbolSearchError::RepositoryIdentity)?;
        }
        LocalSymbolSearchWorkspace::ConnectedWorkspace {
            connected_workspace,
            source_slot,
        } => {
            ConnectedWorkspaceIdTextV1::decode(connected_workspace)
                .map_err(LocalSymbolSearchError::ConnectedWorkspaceIdentity)?;
            SourceSlotIdTextV1::decode(source_slot)
                .map_err(LocalSymbolSearchError::SourceSlotIdentity)?;
        }
    }
    Ok(())
}

fn selected_workspace(
    reader: &OwnedSqliteReader,
    workspace: LocalSymbolSearchWorkspace<'_>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<SelectedWorkspace, LocalSymbolSearchError> {
    let (connected_workspace, requested_slot) = match workspace {
        LocalSymbolSearchWorkspace::SingleRepository {
            repository_identity,
        } => {
            let repository = RepositoryIdentityTextV1::decode(repository_identity)
                .map_err(LocalSymbolSearchError::RepositoryIdentity)?;
            (
                ConnectedWorkspaceId::for_single_repository(repository),
                None,
            )
        }
        LocalSymbolSearchWorkspace::ConnectedWorkspace {
            connected_workspace,
            source_slot,
        } => (
            ConnectedWorkspaceIdTextV1::decode(connected_workspace)
                .map_err(LocalSymbolSearchError::ConnectedWorkspaceIdentity)?,
            Some(
                SourceSlotIdTextV1::decode(source_slot)
                    .map_err(LocalSymbolSearchError::SourceSlotIdentity)?,
            ),
        ),
    };
    let view = reader
        .pin_workspace_view(connected_workspace, None, cancelled, deadline)
        .map_err(LocalSymbolSearchError::Workspace)?
        .ok_or(LocalSymbolSearchError::WorkspaceUnavailable)?;
    let is_connected_workspace = requested_slot.is_some();
    let member = match requested_slot {
        Some(source_slot) => view
            .members()
            .iter()
            .find(|member| member.source_slot() == source_slot)
            .ok_or(LocalSymbolSearchError::WorkspaceUnavailable)?,
        None => {
            let [member] = view.members() else {
                return Err(LocalSymbolSearchError::WorkspaceUnavailable);
            };
            member
        }
    };
    Ok(SelectedWorkspace {
        repository: member.repository(),
        connected_workspace: view.connected_workspace(),
        workspace_view: view.view().get(),
        source_slot: member.source_slot(),
        generation: member.generation(),
        view: is_connected_workspace.then_some(view),
    })
}

struct ConnectedWorkspaceSymbolSearchPort<'a> {
    reader: &'a OwnedSqliteReader,
    view: &'a PinnedWorkspaceView,
    source_slot: SourceSlotId,
}

impl SymbolSearchPort for ConnectedWorkspaceSymbolSearchPort<'_> {
    type Generation = GenerationId;
    type Error = SqliteStoreError;

    fn search_symbols(
        &self,
        _repository: RepositoryIdentityDigest,
        query: &SymbolSearchQuery,
        limits: CodeSearchLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<SymbolSearchPortResult<Self::Generation>, Self::Error> {
        let storage_limits =
            SearchLimits::try_new(limits.max_results(), limits.max_output_bytes())?;
        let result = self.reader.search_workspace_member_symbols(
            self.view,
            self.source_slot,
            query.clone(),
            storage_limits,
            cancelled,
            deadline,
        )?;
        crate::sqlite::code_search_port_result_from_search_results(result)
    }
}

fn effective_search_limits(
    request: &LocalSymbolSearchRequest<'_>,
) -> Result<CodeSearchLimits, CodeSearchLimitError> {
    let configured_max = request
        .configuration
        .map_or(u64::from(request.limits.max_results()), |configuration| {
            *configuration.preferences().query_results().effective()
        });
    let effective_max = u64::from(request.limits.max_results()).min(configured_max);
    let effective_max = u16::try_from(effective_max).map_err(|_| CodeSearchLimitError)?;
    CodeSearchLimits::try_new(effective_max, request.limits.max_output_bytes())
}

fn check_facade_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalSymbolSearchError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalSymbolSearchError::Search(SymbolSearchError::Cancelled))
    } else if Instant::now() >= deadline {
        Err(LocalSymbolSearchError::Search(
            SymbolSearchError::DeadlineExceeded,
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

    use repowitness_application::SymbolSearchNameMatch;

    use super::{
        DEFAULT_LOCAL_SYMBOL_SEARCH_DEADLINE, LocalSymbolSearchError, LocalSymbolSearchRequest,
        search_local_symbols,
    };

    const REPOSITORY_ID: &str = concat!(
        "rwi1:h:",
        "0101010101010101010101010101010101010101010101010101010101010101"
    );

    #[test]
    fn request_debug_output_is_redacted_and_bounds_are_explicit() {
        let request = LocalSymbolSearchRequest::new(
            Path::new("/private/index.sqlite3"),
            REPOSITORY_ID,
            "private_symbol",
            SymbolSearchNameMatch::Prefix,
        )
        .with_max_results(100)
        .expect("inclusive result ceiling should be valid")
        .with_deadline(Duration::from_secs(1));
        let debug = format!("{request:?}");
        assert!(!debug.contains("/private"));
        assert!(!debug.contains(REPOSITORY_ID));
        assert!(!debug.contains("private_symbol"));
        assert_eq!(DEFAULT_LOCAL_SYMBOL_SEARCH_DEADLINE, Duration::from_secs(5));
    }

    #[test]
    fn malformed_identity_fails_before_opening_the_database() {
        assert!(matches!(
            search_local_symbols(
                LocalSymbolSearchRequest::for_connected_workspace(
                    Path::new("/not/opened.sqlite3"),
                    "invalid",
                    "invalid",
                    "name",
                    SymbolSearchNameMatch::Exact,
                ),
                Arc::new(AtomicBool::new(false)),
            ),
            Err(LocalSymbolSearchError::ConnectedWorkspaceIdentity(_))
        ));
    }
}
