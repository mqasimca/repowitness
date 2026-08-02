//! One-shot local composition for bounded producer-declared SCIP relationship tracing.

use std::{
    error::Error,
    fmt,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use repowitness_application::{
    ScipEvidenceReadSelection, ScipRelationshipTraceDepth, ScipRelationshipTraceDirection,
    ScipRelationshipTraceError as ApplicationScipRelationshipTraceError,
    ScipRelationshipTraceMaxEdges, ScipRelationshipTracePort, ScipRelationshipTracePortResult,
    ScipRelationshipTraceRequest, ScipRelationshipTraceResult, scip_relationship_trace,
};
use repowitness_domain::ScipSymbol;

use crate::local_scip_evidence_read::scip_evidence_selection;
use crate::sqlite::{
    ScipRelationshipTraceReadLimits, ScipRelationshipTraceReadLimitsError,
    ScipRelationshipTraceResult as SqliteTraceResult,
};
use crate::{
    DEFAULT_LOCAL_SCIP_EVIDENCE_READ_DEADLINE, LocalScipEvidenceReadError,
    LocalScipEvidenceWorkspace, OwnedSqliteReader, SqliteStoreError,
};

/// Default end-to-end deadline for one local SCIP relationship trace.
pub const DEFAULT_LOCAL_SCIP_RELATIONSHIP_TRACE_DEADLINE: Duration =
    DEFAULT_LOCAL_SCIP_EVIDENCE_READ_DEADLINE;

/// Validated local result with one concrete immutable workspace/view/slot.
pub type LocalScipRelationshipTraceResult = ScipRelationshipTraceResult<SqliteTraceResult>;

/// Explicit local inputs for one bounded package-scoped SCIP relationship trace.
pub struct LocalScipRelationshipTraceRequest<'a> {
    database: &'a Path,
    workspace: LocalScipEvidenceWorkspace<'a>,
    exact_view: Option<i64>,
    package_scope: repowitness_application::PackageScope,
    root: ScipSymbol,
    direction: ScipRelationshipTraceDirection,
    max_depth: ScipRelationshipTraceDepth,
    max_edges: ScipRelationshipTraceMaxEdges,
    limits: ScipRelationshipTraceReadLimits,
    deadline: Duration,
}

impl<'a> LocalScipRelationshipTraceRequest<'a> {
    /// Constructs an active-view request for one default repository workspace.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "all traversal inputs are explicit at the local I/O boundary"
    )]
    pub fn new(
        database: &'a Path,
        repository_identity: &'a str,
        package_scope: repowitness_application::PackageScope,
        root: ScipSymbol,
        direction: ScipRelationshipTraceDirection,
        max_depth: ScipRelationshipTraceDepth,
        max_edges: ScipRelationshipTraceMaxEdges,
    ) -> Self {
        Self {
            database,
            workspace: LocalScipEvidenceWorkspace::SingleRepository {
                repository_identity,
            },
            exact_view: None,
            package_scope,
            root,
            direction,
            max_depth,
            max_edges,
            limits: ScipRelationshipTraceReadLimits::default(),
            deadline: DEFAULT_LOCAL_SCIP_RELATIONSHIP_TRACE_DEADLINE,
        }
    }

    /// Constructs an active-view request for one explicit connected source slot.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "all traversal inputs are explicit at the local I/O boundary"
    )]
    pub fn for_connected_workspace(
        database: &'a Path,
        connected_workspace: &'a str,
        source_slot: &'a str,
        package_scope: repowitness_application::PackageScope,
        root: ScipSymbol,
        direction: ScipRelationshipTraceDirection,
        max_depth: ScipRelationshipTraceDepth,
        max_edges: ScipRelationshipTraceMaxEdges,
    ) -> Self {
        Self {
            database,
            workspace: LocalScipEvidenceWorkspace::ConnectedWorkspace {
                connected_workspace,
                source_slot,
            },
            exact_view: None,
            package_scope,
            root,
            direction,
            max_depth,
            max_edges,
            limits: ScipRelationshipTraceReadLimits::default(),
            deadline: DEFAULT_LOCAL_SCIP_RELATIONSHIP_TRACE_DEADLINE,
        }
    }

    /// Pins one exact immutable workspace view.
    pub fn with_exact_view(
        mut self,
        workspace_view: i64,
    ) -> Result<Self, LocalScipRelationshipTraceError> {
        if workspace_view <= 0 {
            return Err(LocalScipRelationshipTraceError::InvalidSelection);
        }
        self.exact_view = Some(workspace_view);
        Ok(self)
    }

    /// Replaces independent local traversal/output ceilings.
    #[must_use]
    pub const fn with_limits(mut self, limits: ScipRelationshipTraceReadLimits) -> Self {
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

impl fmt::Debug for LocalScipRelationshipTraceRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalScipRelationshipTraceRequest")
            .field("database", &"<redacted-path>")
            .field("workspace", &self.workspace)
            .field("exact_view", &self.exact_view)
            .field("package_scope", &self.package_scope)
            .field("root", &"<redacted-symbol>")
            .field("direction", &self.direction)
            .field("max_depth", &self.max_depth)
            .field("max_edges", &self.max_edges)
            .field("limits", &self.limits)
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Stable content-redacted local SCIP relationship-trace failure.
#[derive(Debug)]
pub enum LocalScipRelationshipTraceError {
    /// The shared SCIP workspace selection could not be decoded.
    Selection(LocalScipEvidenceReadError),
    /// The exact immutable view selection was invalid.
    InvalidSelection,
    /// Local trace limits are internally inconsistent with the request profile.
    Limits(ScipRelationshipTraceReadLimitsError),
    /// The absolute deadline could not be represented.
    DeadlineNotRepresentable,
    /// The read-only SQLite owner could not start.
    ReaderStart(SqliteStoreError),
    /// The shared application use case failed.
    Trace(ApplicationScipRelationshipTraceError<LocalScipRelationshipTracePortError>),
    /// The read-only SQLite owner did not shut down cleanly.
    Shutdown(SqliteStoreError),
}

impl fmt::Display for LocalScipRelationshipTraceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Selection(_) | Self::InvalidSelection => {
                "SCIP relationship trace immutable context selection is invalid"
            }
            Self::Limits(_) => "SCIP relationship trace limits are invalid",
            Self::DeadlineNotRepresentable => {
                "SCIP relationship trace deadline cannot be represented"
            }
            Self::ReaderStart(_) => "SCIP relationship trace reader startup failed",
            Self::Trace(_) => "local SCIP relationship trace failed",
            Self::Shutdown(_) => "SCIP relationship trace reader shutdown failed",
        })
    }
}

impl Error for LocalScipRelationshipTraceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Selection(error) => Some(error),
            Self::Limits(error) => Some(error),
            Self::ReaderStart(error) | Self::Shutdown(error) => Some(error),
            Self::Trace(error) => Some(error),
            Self::InvalidSelection | Self::DeadlineNotRepresentable => None,
        }
    }
}

/// Stable local adapter failure behind the application relationship-trace port.
#[derive(Debug)]
pub enum LocalScipRelationshipTracePortError {
    /// The selected workspace view or requested source slot is unavailable.
    ViewUnavailable,
    /// Immutable view pinning failed.
    View(SqliteStoreError),
    /// The bounded overlay relationship reader failed.
    Trace(SqliteStoreError),
    /// Local limits cannot safely implement the requested application edge cap.
    Limits(ScipRelationshipTraceReadLimitsError),
}

impl fmt::Display for LocalScipRelationshipTracePortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ViewUnavailable => "SCIP relationship trace workspace view is unavailable",
            Self::View(_) => "SCIP relationship trace workspace view read failed",
            Self::Trace(_) => "SCIP relationship trace read failed",
            Self::Limits(_) => "SCIP relationship trace limits are invalid",
        })
    }
}

impl Error for LocalScipRelationshipTracePortError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::View(error) | Self::Trace(error) => Some(error),
            Self::Limits(error) => Some(error),
            Self::ViewUnavailable => None,
        }
    }
}

struct LocalScipRelationshipTracePort<'a> {
    reader: &'a OwnedSqliteReader,
    limits: ScipRelationshipTraceReadLimits,
}

impl ScipRelationshipTracePort for LocalScipRelationshipTracePort<'_> {
    type Output = SqliteTraceResult;
    type Error = LocalScipRelationshipTracePortError;

    #[allow(
        clippy::too_many_arguments,
        reason = "the application contract keeps all trace trust inputs explicit"
    )]
    fn trace(
        &self,
        selection: ScipEvidenceReadSelection,
        package_scope: &repowitness_application::PackageScope,
        root: &ScipSymbol,
        direction: ScipRelationshipTraceDirection,
        max_depth: ScipRelationshipTraceDepth,
        max_edges: ScipRelationshipTraceMaxEdges,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ScipRelationshipTracePortResult<Self::Output>, Self::Error> {
        let view = self
            .reader
            .pin_workspace_view(
                selection.connected_workspace(),
                selection.workspace_view(),
                Arc::clone(&cancelled),
                deadline,
            )
            .map_err(LocalScipRelationshipTracePortError::View)?
            .ok_or(LocalScipRelationshipTracePortError::ViewUnavailable)?;
        let source_slot = match selection.source_slot() {
            Some(source_slot) => view
                .members()
                .iter()
                .find(|member| member.source_slot() == source_slot)
                .map(|member| member.source_slot())
                .ok_or(LocalScipRelationshipTracePortError::ViewUnavailable)?,
            None => {
                let [member] = view.members() else {
                    return Err(LocalScipRelationshipTracePortError::ViewUnavailable);
                };
                member.source_slot()
            }
        };
        let requested_nodes =
            max_edges
                .get()
                .checked_add(1)
                .ok_or(LocalScipRelationshipTracePortError::Limits(
                    ScipRelationshipTraceReadLimitsError,
                ))?;
        let limits = ScipRelationshipTraceReadLimits::try_new(
            max_edges.get(),
            self.limits.max_nodes().min(requested_nodes),
            self.limits.max_output_bytes(),
        )
        .map_err(LocalScipRelationshipTracePortError::Limits)?;
        let output = self
            .reader
            .scip_relationship_trace(
                &view,
                source_slot,
                package_scope.clone(),
                root.clone(),
                direction,
                max_depth,
                max_edges,
                limits,
                cancelled,
                deadline,
            )
            .map_err(LocalScipRelationshipTracePortError::Trace)?;
        Ok(ScipRelationshipTracePortResult::new(
            view.connected_workspace(),
            view.view().get(),
            source_slot,
            output,
        ))
    }
}

/// Opens one reader, executes the shared relationship-trace use case, and shuts down.
pub fn trace_local_scip_relationships(
    request: LocalScipRelationshipTraceRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalScipRelationshipTraceResult, LocalScipRelationshipTraceError> {
    let selection = scip_evidence_selection(&request.workspace, request.exact_view)
        .map_err(LocalScipRelationshipTraceError::Selection)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalScipRelationshipTraceError::DeadlineNotRepresentable)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalScipRelationshipTraceError::ReaderStart)?;
    let port = LocalScipRelationshipTracePort {
        reader: &reader,
        limits: request.limits,
    };
    let result = scip_relationship_trace(
        &port,
        ScipRelationshipTraceRequest::new(
            selection,
            request.package_scope,
            request.root,
            request.direction,
            request.max_depth,
            request.max_edges,
            cancelled,
            deadline,
        ),
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(LocalScipRelationshipTraceError::Trace(error)),
        (Ok(_), Err(error)) => Err(LocalScipRelationshipTraceError::Shutdown(error)),
    }
}
