//! One-shot local composition for pinned package-scoped SCIP evidence reads.

use std::{
    error::Error,
    fmt,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use repowitness_application::{
    ConnectedWorkspaceIdTextV1, PackageScope, RepositoryIdentityTextError,
    RepositoryIdentityTextV1, ScipEvidenceReadError as ApplicationScipEvidenceReadError,
    ScipEvidenceReadPort, ScipEvidenceReadPortResult, ScipEvidenceReadRequest,
    ScipEvidenceReadResult, ScipEvidenceReadSelection, SourceSlotIdTextV1,
    WorkspaceIdentityTextError, scip_evidence_read,
};
use repowitness_domain::{ConnectedWorkspaceId, ScipSymbol};

use crate::{
    OwnedSqliteReader, ScipEvidenceReadLimits, ScipSymbolEvidenceResult, SqliteStoreError,
};

/// Default end-to-end deadline for one local SCIP evidence read.
pub const DEFAULT_LOCAL_SCIP_EVIDENCE_READ_DEADLINE: Duration = Duration::from_secs(5);

/// Validated local result with the concrete immutable workspace/view/slot.
pub type LocalScipEvidenceReadResult = ScipEvidenceReadResult<ScipSymbolEvidenceResult>;

/// Explicit inputs for one package-scoped local precision-evidence read.
pub struct LocalScipEvidenceReadRequest<'a> {
    database: &'a Path,
    workspace: LocalScipEvidenceWorkspace<'a>,
    exact_view: Option<i64>,
    package_scope: PackageScope,
    symbol: ScipSymbol,
    limits: ScipEvidenceReadLimits,
    deadline: Duration,
}

/// One explicitly selected evidence workspace context.
pub enum LocalScipEvidenceWorkspace<'a> {
    /// The compatible default workspace derived from one repository identity.
    SingleRepository {
        /// Canonical repository identity text.
        repository_identity: &'a str,
    },
    /// One explicitly selected source slot in a connected workspace.
    ConnectedWorkspace {
        /// Canonical connected-workspace identity text.
        connected_workspace: &'a str,
        /// Canonical source-slot identity text.
        source_slot: &'a str,
    },
}

impl<'a> LocalScipEvidenceReadRequest<'a> {
    /// Constructs an active-view request for one default repository workspace.
    #[must_use]
    pub fn new(
        database: &'a Path,
        repository_identity: &'a str,
        package_scope: PackageScope,
        symbol: ScipSymbol,
    ) -> Self {
        Self {
            database,
            workspace: LocalScipEvidenceWorkspace::SingleRepository {
                repository_identity,
            },
            exact_view: None,
            package_scope,
            symbol,
            limits: ScipEvidenceReadLimits::default(),
            deadline: DEFAULT_LOCAL_SCIP_EVIDENCE_READ_DEADLINE,
        }
    }

    /// Constructs an active-view request for one explicit connected source slot.
    #[must_use]
    pub fn for_connected_workspace(
        database: &'a Path,
        connected_workspace: &'a str,
        source_slot: &'a str,
        package_scope: PackageScope,
        symbol: ScipSymbol,
    ) -> Self {
        Self {
            database,
            workspace: LocalScipEvidenceWorkspace::ConnectedWorkspace {
                connected_workspace,
                source_slot,
            },
            exact_view: None,
            package_scope,
            symbol,
            limits: ScipEvidenceReadLimits::default(),
            deadline: DEFAULT_LOCAL_SCIP_EVIDENCE_READ_DEADLINE,
        }
    }

    /// Pins one exact immutable view.
    pub fn with_exact_view(
        mut self,
        workspace_view: i64,
    ) -> Result<Self, LocalScipEvidenceReadError> {
        if workspace_view <= 0 {
            return Err(LocalScipEvidenceReadError::InvalidSelection);
        }
        self.exact_view = Some(workspace_view);
        Ok(self)
    }

    /// Replaces the independent local evidence-read bounds.
    #[must_use]
    pub const fn with_limits(mut self, limits: ScipEvidenceReadLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Replaces the end-to-end deadline duration.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalScipEvidenceReadRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalScipEvidenceReadRequest")
            .field("database", &"<redacted-path>")
            .field("workspace", &self.workspace)
            .field("exact_view", &self.exact_view)
            .field("package_scope", &self.package_scope)
            .field("symbol", &self.symbol)
            .field("limits", &self.limits)
            .field("deadline", &self.deadline)
            .finish()
    }
}

impl fmt::Debug for LocalScipEvidenceWorkspace<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::SingleRepository { .. } => "single_repository",
            Self::ConnectedWorkspace { .. } => "connected_workspace",
        };
        formatter
            .debug_struct("LocalScipEvidenceWorkspace")
            .field("kind", &kind)
            .finish_non_exhaustive()
    }
}

/// Stable content-redacted local precision-evidence read failure.
#[derive(Debug)]
pub enum LocalScipEvidenceReadError {
    /// The repository identity text was malformed or non-canonical.
    RepositoryIdentity(RepositoryIdentityTextError),
    /// The connected-workspace identity text was malformed or non-canonical.
    ConnectedWorkspaceIdentity(WorkspaceIdentityTextError),
    /// The source-slot identity text was malformed or non-canonical.
    SourceSlotIdentity(WorkspaceIdentityTextError),
    /// The exact immutable view selection was invalid.
    InvalidSelection,
    /// The absolute deadline could not be represented.
    DeadlineNotRepresentable,
    /// The read-only SQLite owner could not start.
    ReaderStart(SqliteStoreError),
    /// The shared application use case failed.
    Read(ApplicationScipEvidenceReadError<LocalScipEvidencePortError>),
    /// The read-only SQLite owner did not shut down cleanly.
    Shutdown(SqliteStoreError),
}

impl fmt::Display for LocalScipEvidenceReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity(_) => "repository identity is invalid",
            Self::ConnectedWorkspaceIdentity(_) => "connected workspace identity is invalid",
            Self::SourceSlotIdentity(_) => "source slot identity is invalid",
            Self::InvalidSelection => "SCIP evidence immutable context selection is invalid",
            Self::DeadlineNotRepresentable => "SCIP evidence deadline cannot be represented",
            Self::ReaderStart(_) => "SCIP evidence reader startup failed",
            Self::Read(_) => "local SCIP evidence read failed",
            Self::Shutdown(_) => "SCIP evidence reader shutdown failed",
        })
    }
}

impl Error for LocalScipEvidenceReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity(error) => Some(error),
            Self::ConnectedWorkspaceIdentity(error) | Self::SourceSlotIdentity(error) => {
                Some(error)
            }
            Self::ReaderStart(error) | Self::Shutdown(error) => Some(error),
            Self::Read(error) => Some(error),
            Self::InvalidSelection | Self::DeadlineNotRepresentable => None,
        }
    }
}

/// Stable local adapter failure behind the application precision-evidence port.
#[derive(Debug)]
pub enum LocalScipEvidencePortError {
    /// The selected workspace view or requested source slot is unavailable.
    ViewUnavailable,
    /// Immutable view pinning failed.
    View(SqliteStoreError),
    /// The bounded overlay reader failed.
    Evidence(SqliteStoreError),
}

impl fmt::Display for LocalScipEvidencePortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ViewUnavailable => "SCIP evidence workspace view is unavailable",
            Self::View(_) => "SCIP evidence workspace view read failed",
            Self::Evidence(_) => "SCIP evidence read failed",
        })
    }
}

impl Error for LocalScipEvidencePortError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::View(error) | Self::Evidence(error) => Some(error),
            Self::ViewUnavailable => None,
        }
    }
}

struct LocalScipEvidencePort<'a> {
    reader: &'a OwnedSqliteReader,
    limits: ScipEvidenceReadLimits,
}

impl ScipEvidenceReadPort for LocalScipEvidencePort<'_> {
    type Output = ScipSymbolEvidenceResult;
    type Error = LocalScipEvidencePortError;

    fn read(
        &self,
        selection: ScipEvidenceReadSelection,
        package_scope: &PackageScope,
        symbol: &ScipSymbol,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ScipEvidenceReadPortResult<Self::Output>, Self::Error> {
        let view = self
            .reader
            .pin_workspace_view(
                selection.connected_workspace(),
                selection.workspace_view(),
                Arc::clone(&cancelled),
                deadline,
            )
            .map_err(LocalScipEvidencePortError::View)?
            .ok_or(LocalScipEvidencePortError::ViewUnavailable)?;
        let source_slot = match selection.source_slot() {
            Some(source_slot) => view
                .members()
                .iter()
                .find(|member| member.source_slot() == source_slot)
                .map(|member| member.source_slot())
                .ok_or(LocalScipEvidencePortError::ViewUnavailable)?,
            None => {
                let [member] = view.members() else {
                    return Err(LocalScipEvidencePortError::ViewUnavailable);
                };
                member.source_slot()
            }
        };
        let output = self
            .reader
            .scip_symbol_evidence(
                &view,
                source_slot,
                package_scope.clone(),
                symbol.clone(),
                self.limits,
                cancelled,
                deadline,
            )
            .map_err(LocalScipEvidencePortError::Evidence)?;
        Ok(ScipEvidenceReadPortResult::new(
            view.connected_workspace(),
            view.view().get(),
            source_slot,
            output,
        ))
    }
}

/// Opens one reader, executes the shared SCIP evidence use case, and shuts down.
pub fn read_local_scip_evidence(
    request: LocalScipEvidenceReadRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalScipEvidenceReadResult, LocalScipEvidenceReadError> {
    let selection = resolve_selection(&request)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalScipEvidenceReadError::DeadlineNotRepresentable)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalScipEvidenceReadError::ReaderStart)?;
    let port = LocalScipEvidencePort {
        reader: &reader,
        limits: request.limits,
    };
    let result = scip_evidence_read(
        &port,
        ScipEvidenceReadRequest::new(
            selection,
            request.package_scope,
            request.symbol,
            cancelled,
            deadline,
        ),
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(LocalScipEvidenceReadError::Read(error)),
        (Ok(_), Err(error)) => Err(LocalScipEvidenceReadError::Shutdown(error)),
    }
}

fn resolve_selection(
    request: &LocalScipEvidenceReadRequest<'_>,
) -> Result<ScipEvidenceReadSelection, LocalScipEvidenceReadError> {
    match request.workspace {
        LocalScipEvidenceWorkspace::SingleRepository {
            repository_identity,
        } => {
            let repository = RepositoryIdentityTextV1::decode(repository_identity)
                .map_err(LocalScipEvidenceReadError::RepositoryIdentity)?;
            let workspace = ConnectedWorkspaceId::for_single_repository(repository);
            match request.exact_view {
                Some(view) => ScipEvidenceReadSelection::exact(workspace, view)
                    .map_err(|_| LocalScipEvidenceReadError::InvalidSelection),
                None => Ok(ScipEvidenceReadSelection::active(workspace)),
            }
        }
        LocalScipEvidenceWorkspace::ConnectedWorkspace {
            connected_workspace,
            source_slot,
        } => {
            let workspace = ConnectedWorkspaceIdTextV1::decode(connected_workspace)
                .map_err(LocalScipEvidenceReadError::ConnectedWorkspaceIdentity)?;
            let source_slot = SourceSlotIdTextV1::decode(source_slot)
                .map_err(LocalScipEvidenceReadError::SourceSlotIdentity)?;
            match request.exact_view {
                Some(view) => {
                    ScipEvidenceReadSelection::exact_source_slot(workspace, source_slot, view)
                        .map_err(|_| LocalScipEvidenceReadError::InvalidSelection)
                }
                None => Ok(ScipEvidenceReadSelection::active_source_slot(
                    workspace,
                    source_slot,
                )),
            }
        }
    }
}
