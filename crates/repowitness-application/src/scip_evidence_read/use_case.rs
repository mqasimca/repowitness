use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_domain::{ConnectedWorkspaceId, ScipSymbol, SourceSlotId};

use crate::PackageScope;

/// Immutable workspace/view/slot selection for one SCIP evidence read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScipEvidenceReadSelection {
    connected_workspace: ConnectedWorkspaceId,
    source_slot: Option<SourceSlotId>,
    workspace_view: Option<i64>,
}

impl ScipEvidenceReadSelection {
    /// Selects the current single-source view of one connected workspace.
    #[must_use]
    pub const fn active(connected_workspace: ConnectedWorkspaceId) -> Self {
        Self {
            connected_workspace,
            source_slot: None,
            workspace_view: None,
        }
    }

    /// Selects the current view for one explicit source slot.
    #[must_use]
    pub const fn active_source_slot(
        connected_workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
    ) -> Self {
        Self {
            connected_workspace,
            source_slot: Some(source_slot),
            workspace_view: None,
        }
    }

    /// Selects one exact immutable view whose source slot must be singular.
    pub const fn exact(
        connected_workspace: ConnectedWorkspaceId,
        workspace_view: i64,
    ) -> Result<Self, ScipEvidenceReadSelectionError> {
        if workspace_view <= 0 {
            return Err(ScipEvidenceReadSelectionError::InvalidIdentity);
        }
        Ok(Self {
            connected_workspace,
            source_slot: None,
            workspace_view: Some(workspace_view),
        })
    }

    /// Selects an exact immutable view and source slot.
    pub const fn exact_source_slot(
        connected_workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        workspace_view: i64,
    ) -> Result<Self, ScipEvidenceReadSelectionError> {
        if workspace_view <= 0 {
            return Err(ScipEvidenceReadSelectionError::InvalidIdentity);
        }
        Ok(Self {
            connected_workspace,
            source_slot: Some(source_slot),
            workspace_view: Some(workspace_view),
        })
    }

    /// Returns the connected workspace identity.
    #[must_use]
    pub const fn connected_workspace(self) -> ConnectedWorkspaceId {
        self.connected_workspace
    }

    /// Returns an explicitly selected source slot when required.
    #[must_use]
    pub const fn source_slot(self) -> Option<SourceSlotId> {
        self.source_slot
    }

    /// Returns an exact immutable view pin when one was requested.
    #[must_use]
    pub const fn workspace_view(self) -> Option<i64> {
        self.workspace_view
    }
}

/// A storage-neutral bounded precision-evidence read boundary.
pub trait ScipEvidenceReadPort {
    /// Complete bounded local adapter result.
    type Output;
    /// Stable local adapter error.
    type Error;

    /// Resolves one selected immutable view/slot and reads exact package evidence.
    fn read(
        &self,
        selection: ScipEvidenceReadSelection,
        package_scope: &PackageScope,
        symbol: &ScipSymbol,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ScipEvidenceReadPortResult<Self::Output>, Self::Error>;
}

/// Exact context returned by a precision-evidence adapter.
pub struct ScipEvidenceReadPortResult<T> {
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: i64,
    source_slot: SourceSlotId,
    output: T,
}

impl<T> ScipEvidenceReadPortResult<T> {
    /// Constructs one adapter result for application-level context validation.
    #[must_use]
    pub const fn new(
        connected_workspace: ConnectedWorkspaceId,
        workspace_view: i64,
        source_slot: SourceSlotId,
        output: T,
    ) -> Self {
        Self {
            connected_workspace,
            workspace_view,
            source_slot,
            output,
        }
    }
}

/// Complete application request for one SCIP evidence read.
pub struct ScipEvidenceReadRequest {
    selection: ScipEvidenceReadSelection,
    package_scope: PackageScope,
    symbol: ScipSymbol,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl ScipEvidenceReadRequest {
    /// Constructs one request from validated caller boundary values.
    #[must_use]
    pub const fn new(
        selection: ScipEvidenceReadSelection,
        package_scope: PackageScope,
        symbol: ScipSymbol,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            selection,
            package_scope,
            symbol,
            cancelled,
            deadline,
        }
    }
}

impl fmt::Debug for ScipEvidenceReadRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScipEvidenceReadRequest")
            .field("selection", &self.selection)
            .field("package_scope", &self.package_scope)
            .field("symbol", &self.symbol)
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Validated result pinned to one immutable workspace view and source slot.
#[derive(Debug, Eq, PartialEq)]
pub struct ScipEvidenceReadResult<T> {
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: i64,
    source_slot: SourceSlotId,
    output: T,
}

impl<T> ScipEvidenceReadResult<T> {
    /// Returns the exact connected workspace read by the adapter.
    #[must_use]
    pub const fn connected_workspace(&self) -> ConnectedWorkspaceId {
        self.connected_workspace
    }

    /// Returns the selected immutable workspace-view identity.
    #[must_use]
    pub const fn workspace_view(&self) -> i64 {
        self.workspace_view
    }

    /// Returns the exact selected source slot.
    #[must_use]
    pub const fn source_slot(&self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the bounded operation output.
    #[must_use]
    pub const fn output(&self) -> &T {
        &self.output
    }

    /// Consumes the envelope and returns the operation output.
    #[must_use]
    pub fn into_output(self) -> T {
        self.output
    }
}

/// An invalid precision-evidence selection or adapter context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScipEvidenceReadSelectionError {
    /// A supplied or returned immutable identity was non-positive.
    InvalidIdentity,
    /// The adapter returned a different immutable workspace/view/slot.
    ContextMismatch,
}

impl fmt::Display for ScipEvidenceReadSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidIdentity => "SCIP evidence adapter returned an invalid context identity",
            Self::ContextMismatch => "SCIP evidence adapter returned a different immutable context",
        })
    }
}

impl Error for ScipEvidenceReadSelectionError {}

/// Stable all-or-nothing precision-evidence use-case failure.
#[derive(Debug)]
pub enum ScipEvidenceReadError<E> {
    /// Cancellation was visible before a complete result.
    Cancelled,
    /// The absolute deadline elapsed before a complete result.
    DeadlineExceeded,
    /// The storage adapter failed.
    Port(E),
    /// The adapter violated the requested immutable context.
    InvalidPortOutput(ScipEvidenceReadSelectionError),
}

impl<E: fmt::Display> fmt::Display for ScipEvidenceReadError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("SCIP evidence read cancelled"),
            Self::DeadlineExceeded => formatter.write_str("SCIP evidence read deadline exceeded"),
            Self::Port(error) => write!(formatter, "SCIP evidence adapter failed: {error}"),
            Self::InvalidPortOutput(error) => error.fmt(formatter),
        }
    }
}

impl<E: Error + 'static> Error for ScipEvidenceReadError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Port(error) => Some(error),
            Self::InvalidPortOutput(error) => Some(error),
            Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Executes one context-validated package-scoped SCIP evidence read.
pub fn scip_evidence_read<Port>(
    port: &Port,
    request: ScipEvidenceReadRequest,
) -> Result<ScipEvidenceReadResult<Port::Output>, ScipEvidenceReadError<Port::Error>>
where
    Port: ScipEvidenceReadPort,
{
    check_control(&request.cancelled, request.deadline)?;
    let result = port
        .read(
            request.selection,
            &request.package_scope,
            &request.symbol,
            Arc::clone(&request.cancelled),
            request.deadline,
        )
        .map_err(ScipEvidenceReadError::Port)?;
    check_control(&request.cancelled, request.deadline)?;
    validate_context(request.selection, &result)?;
    Ok(ScipEvidenceReadResult {
        connected_workspace: result.connected_workspace,
        workspace_view: result.workspace_view,
        source_slot: result.source_slot,
        output: result.output,
    })
}

fn validate_context<T, E>(
    selection: ScipEvidenceReadSelection,
    result: &ScipEvidenceReadPortResult<T>,
) -> Result<(), ScipEvidenceReadError<E>> {
    if result.workspace_view <= 0 {
        return Err(ScipEvidenceReadError::InvalidPortOutput(
            ScipEvidenceReadSelectionError::InvalidIdentity,
        ));
    }
    if result.connected_workspace != selection.connected_workspace()
        || selection
            .source_slot()
            .is_some_and(|slot| slot != result.source_slot)
        || selection
            .workspace_view()
            .is_some_and(|view| view != result.workspace_view)
    {
        return Err(ScipEvidenceReadError::InvalidPortOutput(
            ScipEvidenceReadSelectionError::ContextMismatch,
        ));
    }
    Ok(())
}

fn check_control<E>(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), ScipEvidenceReadError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(ScipEvidenceReadError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(ScipEvidenceReadError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{Arc, atomic::AtomicBool},
        time::{Duration, Instant},
    };

    use repowitness_domain::{ConnectedWorkspaceId, RepositoryIdentityDigest, SourceSlotId};

    use super::{
        PackageScope, ScipEvidenceReadPort, ScipEvidenceReadPortResult, ScipEvidenceReadRequest,
        ScipEvidenceReadSelection, ScipEvidenceReadSelectionError, ScipSymbol, scip_evidence_read,
    };

    struct FakePort {
        workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        view: i64,
    }

    impl ScipEvidenceReadPort for FakePort {
        type Output = u8;
        type Error = Infallible;

        fn read(
            &self,
            _: ScipEvidenceReadSelection,
            _: &PackageScope,
            _: &ScipSymbol,
            _: Arc<AtomicBool>,
            _: Instant,
        ) -> Result<ScipEvidenceReadPortResult<Self::Output>, Self::Error> {
            Ok(ScipEvidenceReadPortResult::new(
                self.workspace,
                self.view,
                self.source_slot,
                7,
            ))
        }
    }

    fn deadline() -> Instant {
        Instant::now()
            .checked_add(Duration::from_secs(1))
            .expect("test deadline should be representable")
    }

    #[test]
    fn exact_context_is_preserved_and_mismatches_fail_closed() {
        let workspace =
            ConnectedWorkspaceId::for_single_repository(RepositoryIdentityDigest::new([1; 32]));
        let source_slot = SourceSlotId::new([2; 32]);
        let selection = ScipEvidenceReadSelection::exact_source_slot(workspace, source_slot, 9)
            .expect("positive view should select");
        let request = || {
            ScipEvidenceReadRequest::new(
                selection,
                PackageScope::whole_repository(),
                ScipSymbol::try_new("scip-rust pkg 1 Item.".to_owned()).expect("symbol"),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
        };
        let result = scip_evidence_read(
            &FakePort {
                workspace,
                source_slot,
                view: 9,
            },
            request(),
        )
        .expect("matching immutable context should pass");
        assert_eq!(result.workspace_view(), 9);
        assert_eq!(result.source_slot(), source_slot);
        assert_eq!(*result.output(), 7);

        let error = scip_evidence_read(
            &FakePort {
                workspace,
                source_slot,
                view: 10,
            },
            request(),
        )
        .expect_err("different immutable view must fail");
        assert!(matches!(
            error,
            super::ScipEvidenceReadError::InvalidPortOutput(
                ScipEvidenceReadSelectionError::ContextMismatch
            )
        ));
    }
}
