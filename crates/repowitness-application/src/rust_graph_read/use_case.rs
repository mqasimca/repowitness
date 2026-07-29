use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_domain::ConnectedWorkspaceId;

use super::{RustGraphReadOperation, RustGraphReadSelection};

/// Complete adapter result with the exact immutable context actually read.
pub struct RustGraphReadPortResult<T> {
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: i64,
    graph_generation: i64,
    output: T,
}

impl<T> RustGraphReadPortResult<T> {
    /// Constructs an adapter result for application validation.
    #[must_use]
    pub const fn new(
        connected_workspace: ConnectedWorkspaceId,
        workspace_view: i64,
        graph_generation: i64,
        output: T,
    ) -> Self {
        Self {
            connected_workspace,
            workspace_view,
            graph_generation,
            output,
        }
    }
}

/// Narrow storage-neutral graph read boundary shared by CLI and MCP.
pub trait RustGraphReadPort {
    /// Complete bounded adapter output for one requested operation.
    type Output;
    /// Stable adapter failure.
    type Error;

    /// Runs one graph read after resolving and pinning the requested context.
    fn read(
        &self,
        selection: RustGraphReadSelection,
        operation: &RustGraphReadOperation,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RustGraphReadPortResult<Self::Output>, Self::Error>;
}

/// Application request for one canonical graph operation.
pub struct RustGraphReadRequest {
    selection: RustGraphReadSelection,
    operation: RustGraphReadOperation,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl RustGraphReadRequest {
    /// Constructs a request from validated boundary values.
    #[must_use]
    pub const fn new(
        selection: RustGraphReadSelection,
        operation: RustGraphReadOperation,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            selection,
            operation,
            cancelled,
            deadline,
        }
    }
}

impl fmt::Debug for RustGraphReadRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphReadRequest")
            .field("selection", &self.selection)
            .field("operation", &operation_label(&self.operation))
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Validated graph result pinned to one immutable workspace view and generation.
#[derive(Debug, Eq, PartialEq)]
pub struct RustGraphReadResult<T> {
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: i64,
    graph_generation: i64,
    output: T,
}

impl<T> RustGraphReadResult<T> {
    /// Returns the connected workspace actually read.
    #[must_use]
    pub const fn connected_workspace(&self) -> ConnectedWorkspaceId {
        self.connected_workspace
    }

    /// Returns the immutable workspace-view identity actually read.
    #[must_use]
    pub const fn workspace_view(&self) -> i64 {
        self.workspace_view
    }

    /// Returns the immutable graph-owning generation actually read.
    #[must_use]
    pub const fn graph_generation(&self) -> i64 {
        self.graph_generation
    }

    /// Returns the operation-specific bounded result.
    #[must_use]
    pub const fn output(&self) -> &T {
        &self.output
    }

    /// Consumes the envelope and returns the operation-specific result.
    #[must_use]
    pub fn into_output(self) -> T {
        self.output
    }
}

/// Stable invalid graph adapter context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustGraphReadSelectionError {
    /// The adapter returned a non-positive view or generation.
    InvalidIdentity,
    /// The adapter returned a different workspace, view, or generation.
    ContextMismatch,
}

impl fmt::Display for RustGraphReadSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidIdentity => "Rust graph adapter returned an invalid context identity",
            Self::ContextMismatch => "Rust graph adapter returned a different immutable context",
        })
    }
}

impl Error for RustGraphReadSelectionError {}

/// Application failure for one all-or-nothing graph read.
#[derive(Debug)]
pub enum RustGraphReadError<E> {
    /// Cancellation was visible before a complete result.
    Cancelled,
    /// The absolute deadline elapsed before a complete result.
    DeadlineExceeded,
    /// The storage-neutral adapter failed.
    Port(E),
    /// The adapter violated the immutable-context contract.
    InvalidPortOutput(RustGraphReadSelectionError),
}

impl<E: fmt::Display> fmt::Display for RustGraphReadError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("Rust graph read cancelled"),
            Self::DeadlineExceeded => formatter.write_str("Rust graph read deadline exceeded"),
            Self::Port(error) => write!(formatter, "Rust graph adapter failed: {error}"),
            Self::InvalidPortOutput(error) => error.fmt(formatter),
        }
    }
}

impl<E: Error + 'static> Error for RustGraphReadError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Port(error) => Some(error),
            Self::InvalidPortOutput(error) => Some(error),
            Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Executes one validated canonical graph read and checks its immutable context.
pub fn rust_graph_read<Port>(
    port: &Port,
    request: RustGraphReadRequest,
) -> Result<RustGraphReadResult<Port::Output>, RustGraphReadError<Port::Error>>
where
    Port: RustGraphReadPort,
{
    check_control(&request.cancelled, request.deadline)?;
    let result = port
        .read(
            request.selection,
            &request.operation,
            Arc::clone(&request.cancelled),
            request.deadline,
        )
        .map_err(RustGraphReadError::Port)?;
    check_control(&request.cancelled, request.deadline)?;
    validate_context(request.selection, &result)?;
    Ok(RustGraphReadResult {
        connected_workspace: result.connected_workspace,
        workspace_view: result.workspace_view,
        graph_generation: result.graph_generation,
        output: result.output,
    })
}

fn validate_context<T, E>(
    selection: RustGraphReadSelection,
    result: &RustGraphReadPortResult<T>,
) -> Result<(), RustGraphReadError<E>> {
    if result.workspace_view <= 0 || result.graph_generation <= 0 {
        return Err(RustGraphReadError::InvalidPortOutput(
            RustGraphReadSelectionError::InvalidIdentity,
        ));
    }
    if result.connected_workspace != selection.connected_workspace()
        || selection
            .exact_pin()
            .is_some_and(|expected| expected != (result.workspace_view, result.graph_generation))
    {
        return Err(RustGraphReadError::InvalidPortOutput(
            RustGraphReadSelectionError::ContextMismatch,
        ));
    }
    Ok(())
}

fn check_control<E>(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), RustGraphReadError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(RustGraphReadError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(RustGraphReadError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

const fn operation_label(operation: &RustGraphReadOperation) -> &'static str {
    match operation {
        RustGraphReadOperation::Status => "status",
        RustGraphReadOperation::Search { .. } => "search",
        RustGraphReadOperation::Evidence { .. } => "evidence",
        RustGraphReadOperation::Architecture { .. } => "architecture",
        RustGraphReadOperation::Trace { .. } => "trace",
        RustGraphReadOperation::Impact { .. } => "impact",
    }
}
