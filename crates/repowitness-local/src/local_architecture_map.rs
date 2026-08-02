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
    ArchitectureMapError, ArchitectureMapLimitError, ArchitectureMapLimits, ArchitectureMapPort,
    ArchitectureMapRequest, ArchitectureMapResult, ConnectedWorkspaceIdTextV1,
    RepositoryIdentityTextError, RepositoryIdentityTextV1, SourceSlotIdTextV1,
    WorkspaceIdentityTextError, architecture_map,
};
use repowitness_domain::{ConnectedWorkspaceId, RepositoryIdentityDigest, SourceSlotId};

use crate::{GenerationId, OwnedSqliteReader, PinnedWorkspaceView, SqliteStoreError};

/// Default end-to-end deadline for one bounded architecture map.
pub const DEFAULT_LOCAL_ARCHITECTURE_MAP_DEADLINE: Duration = Duration::from_secs(30);

/// Exact multi-language source map pinned to one immutable local generation.
pub type LocalArchitectureMapResult = ArchitectureMapResult<GenerationId>;

/// Explicit single-repository or connected-source-slot map context.
#[derive(Clone, Copy)]
pub enum LocalArchitectureMapWorkspace<'a> {
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

/// Explicit inputs for one local active-generation map.
#[derive(Clone, Copy)]
pub struct LocalArchitectureMapRequest<'a> {
    database: &'a Path,
    workspace: LocalArchitectureMapWorkspace<'a>,
    limits: ArchitectureMapLimits,
    deadline: Duration,
}

impl<'a> LocalArchitectureMapRequest<'a> {
    /// Constructs a request with the conservative public map bounds.
    #[must_use]
    pub fn new(database: &'a Path, repository_identity: &'a str) -> Self {
        Self {
            database,
            workspace: LocalArchitectureMapWorkspace::SingleRepository {
                repository_identity,
            },
            limits: ArchitectureMapLimits::default(),
            deadline: DEFAULT_LOCAL_ARCHITECTURE_MAP_DEADLINE,
        }
    }

    /// Constructs a map request for one connected-workspace source slot.
    #[must_use]
    pub fn for_connected_workspace(
        database: &'a Path,
        connected_workspace: &'a str,
        source_slot: &'a str,
    ) -> Self {
        Self {
            database,
            workspace: LocalArchitectureMapWorkspace::ConnectedWorkspace {
                connected_workspace,
                source_slot,
            },
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
            .field("workspace", &self.workspace)
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
    /// The connected-workspace identity text was malformed or non-canonical.
    ConnectedWorkspaceIdentity(WorkspaceIdentityTextError),
    /// The source-slot identity text was malformed or non-canonical.
    SourceSlotIdentity(WorkspaceIdentityTextError),
    /// The requested bound was invalid.
    Limits(ArchitectureMapLimitError),
    /// The absolute deadline could not be represented.
    DeadlineNotRepresentable,
    /// The owned reader could not start.
    ReaderStart(SqliteStoreError),
    /// The selected connected workspace or source slot was unavailable.
    WorkspaceUnavailable,
    /// Pinning the selected workspace view failed.
    Workspace(SqliteStoreError),
    /// The selected source slot changed after workspace-view selection.
    WorkspaceGenerationChanged,
    /// The shared application map failed.
    Map(ArchitectureMapError<SqliteStoreError>),
    /// The owned reader could not shut down cleanly.
    Shutdown(SqliteStoreError),
}

impl fmt::Display for LocalArchitectureMapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity(_) => "repository identity is invalid",
            Self::ConnectedWorkspaceIdentity(_) => "connected workspace identity is invalid",
            Self::SourceSlotIdentity(_) => "source slot identity is invalid",
            Self::Limits(_) => "architecture-map limits are invalid",
            Self::DeadlineNotRepresentable => "architecture-map deadline cannot be represented",
            Self::ReaderStart(_) => "local index reader could not start",
            Self::WorkspaceUnavailable => "architecture-map workspace view is unavailable",
            Self::Workspace(_) => "architecture-map workspace view read failed",
            Self::WorkspaceGenerationChanged => {
                "architecture-map source changed after workspace-view selection"
            }
            Self::Map(_) => "local architecture map failed",
            Self::Shutdown(_) => "local index reader could not shut down",
        })
    }
}

impl Error for LocalArchitectureMapError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity(error) => Some(error),
            Self::ConnectedWorkspaceIdentity(error) | Self::SourceSlotIdentity(error) => {
                Some(error)
            }
            Self::Limits(error) => Some(error),
            Self::ReaderStart(error) | Self::Shutdown(error) => Some(error),
            Self::Workspace(error) => Some(error),
            Self::Map(error) => Some(error),
            Self::DeadlineNotRepresentable
            | Self::WorkspaceUnavailable
            | Self::WorkspaceGenerationChanged => None,
        }
    }
}

/// Opens one owned reader, maps the active index, then shuts the reader down.
pub fn map_local_architecture(
    request: LocalArchitectureMapRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalArchitectureMapResult, LocalArchitectureMapError> {
    validate_workspace_identity(request.workspace)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalArchitectureMapError::DeadlineNotRepresentable)?;
    check_facade_control(&cancelled, deadline)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalArchitectureMapError::ReaderStart)?;
    let workspace =
        match selected_workspace(&reader, request.workspace, Arc::clone(&cancelled), deadline) {
            Ok(workspace) => workspace,
            Err(error) => {
                let shutdown = reader.shutdown(deadline);
                return match shutdown {
                    Ok(()) => Err(error),
                    Err(error) => Err(LocalArchitectureMapError::Shutdown(error)),
                };
            }
        };
    let result = match workspace.view.as_ref() {
        None => architecture_map(
            &reader,
            ArchitectureMapRequest::new(
                workspace.repository,
                request.limits,
                Arc::clone(&cancelled),
                deadline,
            ),
        ),
        Some(view) => architecture_map(
            &ConnectedWorkspaceArchitectureMapPort {
                reader: &reader,
                view,
                source_slot: workspace.source_slot,
            },
            ArchitectureMapRequest::new(
                workspace.repository,
                request.limits,
                Arc::clone(&cancelled),
                deadline,
            ),
        ),
    };
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) if *result.generation() == workspace.generation => Ok(result),
        (Ok(_), Ok(())) => Err(LocalArchitectureMapError::WorkspaceGenerationChanged),
        (Err(error), _) => Err(LocalArchitectureMapError::Map(error)),
        (Ok(_), Err(error)) => Err(LocalArchitectureMapError::Shutdown(error)),
    }
}

impl fmt::Debug for LocalArchitectureMapWorkspace<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::SingleRepository { .. } => "single_repository",
            Self::ConnectedWorkspace { .. } => "connected_workspace",
        };
        formatter
            .debug_struct("LocalArchitectureMapWorkspace")
            .field("kind", &kind)
            .finish_non_exhaustive()
    }
}

struct SelectedWorkspace {
    repository: RepositoryIdentityDigest,
    source_slot: SourceSlotId,
    generation: GenerationId,
    view: Option<PinnedWorkspaceView>,
}

fn validate_workspace_identity(
    workspace: LocalArchitectureMapWorkspace<'_>,
) -> Result<(), LocalArchitectureMapError> {
    match workspace {
        LocalArchitectureMapWorkspace::SingleRepository {
            repository_identity,
        } => {
            RepositoryIdentityTextV1::decode(repository_identity)
                .map_err(LocalArchitectureMapError::RepositoryIdentity)?;
        }
        LocalArchitectureMapWorkspace::ConnectedWorkspace {
            connected_workspace,
            source_slot,
        } => {
            ConnectedWorkspaceIdTextV1::decode(connected_workspace)
                .map_err(LocalArchitectureMapError::ConnectedWorkspaceIdentity)?;
            SourceSlotIdTextV1::decode(source_slot)
                .map_err(LocalArchitectureMapError::SourceSlotIdentity)?;
        }
    }
    Ok(())
}

fn selected_workspace(
    reader: &OwnedSqliteReader,
    workspace: LocalArchitectureMapWorkspace<'_>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<SelectedWorkspace, LocalArchitectureMapError> {
    let (connected_workspace, requested_slot) = match workspace {
        LocalArchitectureMapWorkspace::SingleRepository {
            repository_identity,
        } => {
            let repository = RepositoryIdentityTextV1::decode(repository_identity)
                .map_err(LocalArchitectureMapError::RepositoryIdentity)?;
            (
                ConnectedWorkspaceId::for_single_repository(repository),
                None,
            )
        }
        LocalArchitectureMapWorkspace::ConnectedWorkspace {
            connected_workspace,
            source_slot,
        } => (
            ConnectedWorkspaceIdTextV1::decode(connected_workspace)
                .map_err(LocalArchitectureMapError::ConnectedWorkspaceIdentity)?,
            Some(
                SourceSlotIdTextV1::decode(source_slot)
                    .map_err(LocalArchitectureMapError::SourceSlotIdentity)?,
            ),
        ),
    };
    let view = reader
        .pin_workspace_view(connected_workspace, None, cancelled, deadline)
        .map_err(LocalArchitectureMapError::Workspace)?
        .ok_or(LocalArchitectureMapError::WorkspaceUnavailable)?;
    let is_connected_workspace = requested_slot.is_some();
    let member = match requested_slot {
        Some(source_slot) => view
            .members()
            .iter()
            .find(|member| member.source_slot() == source_slot)
            .ok_or(LocalArchitectureMapError::WorkspaceUnavailable)?,
        None => {
            let [member] = view.members() else {
                return Err(LocalArchitectureMapError::WorkspaceUnavailable);
            };
            member
        }
    };
    Ok(SelectedWorkspace {
        repository: member.repository(),
        source_slot: member.source_slot(),
        generation: member.generation(),
        view: is_connected_workspace.then_some(view),
    })
}

struct ConnectedWorkspaceArchitectureMapPort<'a> {
    reader: &'a OwnedSqliteReader,
    view: &'a PinnedWorkspaceView,
    source_slot: SourceSlotId,
}

impl ArchitectureMapPort for ConnectedWorkspaceArchitectureMapPort<'_> {
    type Generation = GenerationId;
    type Error = SqliteStoreError;

    fn architecture_map(
        &self,
        _repository: RepositoryIdentityDigest,
        limits: ArchitectureMapLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<repowitness_application::ArchitectureMapPortResult<Self::Generation>, Self::Error>
    {
        self.reader.architecture_map_workspace_member(
            self.view,
            self.source_slot,
            limits,
            cancelled,
            deadline,
        )
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
        DEFAULT_LOCAL_ARCHITECTURE_MAP_DEADLINE, LocalArchitectureMapError,
        LocalArchitectureMapRequest, map_local_architecture,
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
        assert!(matches!(
            map_local_architecture(
                LocalArchitectureMapRequest::for_connected_workspace(
                    Path::new("/not/opened.sqlite3"),
                    "invalid",
                    "invalid",
                ),
                Arc::new(AtomicBool::new(false)),
            ),
            Err(LocalArchitectureMapError::ConnectedWorkspaceIdentity(_))
        ));
    }
}
