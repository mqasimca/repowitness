//! Bounded immutable traversal over exact SCIP relationship evidence.

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

use crate::{PackageScope, ScipEvidenceReadSelection};

/// Highest permitted number of exact relationship-evidence hops.
pub const MAX_SCIP_RELATIONSHIP_TRACE_DEPTH: u8 = 4;
/// Default relationship-evidence traversal depth.
pub const DEFAULT_SCIP_RELATIONSHIP_TRACE_DEPTH: u8 = 2;
/// Default retained relationship-edge ceiling for one precision trace.
pub const DEFAULT_SCIP_RELATIONSHIP_TRACE_EDGES: u16 = 100;
/// Maximum retained relationship-edge ceiling admitted by the precision profile.
pub const MAX_SCIP_RELATIONSHIP_TRACE_EDGES: u16 = 256;

/// One explicit direction through producer-declared and enclosed-reference SCIP rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScipRelationshipTraceDirection {
    /// Follow relationships whose source is the current frontier symbol.
    Outgoing,
    /// Follow relationships whose target is the current frontier symbol.
    Incoming,
}

/// Validated bounded relationship-trace depth.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScipRelationshipTraceDepth(u8);

impl ScipRelationshipTraceDepth {
    /// Admits one non-zero depth within the fixed precision-trace profile.
    pub const fn try_new(value: u8) -> Result<Self, ScipRelationshipTraceDepthError> {
        if value == 0 || value > MAX_SCIP_RELATIONSHIP_TRACE_DEPTH {
            return Err(ScipRelationshipTraceDepthError);
        }
        Ok(Self(value))
    }

    /// Returns the validated inclusive traversal depth.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// The supplied trace depth is outside the fixed profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScipRelationshipTraceDepthError;

impl fmt::Display for ScipRelationshipTraceDepthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SCIP relationship trace depth is outside the supported profile")
    }
}

impl Error for ScipRelationshipTraceDepthError {}

/// Validated retained relationship-edge ceiling for one trace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScipRelationshipTraceMaxEdges(u16);

impl ScipRelationshipTraceMaxEdges {
    /// Admits one non-zero returned-edge limit within the fixed precision profile.
    pub const fn try_new(value: u16) -> Result<Self, ScipRelationshipTraceMaxEdgesError> {
        if value == 0 || value > MAX_SCIP_RELATIONSHIP_TRACE_EDGES {
            return Err(ScipRelationshipTraceMaxEdgesError);
        }
        Ok(Self(value))
    }

    /// Returns the validated retained relationship-edge ceiling.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// The supplied trace edge ceiling is outside the fixed profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScipRelationshipTraceMaxEdgesError;

impl fmt::Display for ScipRelationshipTraceMaxEdgesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SCIP relationship trace edge limit is outside the supported profile")
    }
}

impl Error for ScipRelationshipTraceMaxEdgesError {}

/// A storage-neutral bounded SCIP relationship-trace boundary.
pub trait ScipRelationshipTracePort {
    /// Complete bounded local adapter result.
    type Output;
    /// Stable local adapter error.
    type Error;

    /// Resolves one selected immutable view/slot and traces exact relationship evidence.
    #[allow(
        clippy::too_many_arguments,
        reason = "selection, scope, root, traversal controls, and deadline are independent trust inputs"
    )]
    fn trace(
        &self,
        selection: ScipEvidenceReadSelection,
        package_scope: &PackageScope,
        root: &ScipSymbol,
        direction: ScipRelationshipTraceDirection,
        max_depth: ScipRelationshipTraceDepth,
        max_edges: ScipRelationshipTraceMaxEdges,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ScipRelationshipTracePortResult<Self::Output>, Self::Error>;
}

/// Exact context returned by a bounded precision-trace adapter.
pub struct ScipRelationshipTracePortResult<T> {
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: i64,
    source_slot: SourceSlotId,
    output: T,
}

impl<T> ScipRelationshipTracePortResult<T> {
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

/// Complete application request for one pinned SCIP relationship traversal.
pub struct ScipRelationshipTraceRequest {
    selection: ScipEvidenceReadSelection,
    package_scope: PackageScope,
    root: ScipSymbol,
    direction: ScipRelationshipTraceDirection,
    max_depth: ScipRelationshipTraceDepth,
    max_edges: ScipRelationshipTraceMaxEdges,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl ScipRelationshipTraceRequest {
    /// Constructs one request from validated caller boundary values.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "selection, scope, root, direction, depth, and controls are independently material"
    )]
    pub const fn new(
        selection: ScipEvidenceReadSelection,
        package_scope: PackageScope,
        root: ScipSymbol,
        direction: ScipRelationshipTraceDirection,
        max_depth: ScipRelationshipTraceDepth,
        max_edges: ScipRelationshipTraceMaxEdges,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            selection,
            package_scope,
            root,
            direction,
            max_depth,
            max_edges,
            cancelled,
            deadline,
        }
    }
}

impl fmt::Debug for ScipRelationshipTraceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScipRelationshipTraceRequest")
            .field("selection", &self.selection)
            .field("package_scope", &self.package_scope)
            .field("root", &"<redacted-symbol>")
            .field("direction", &self.direction)
            .field("max_depth", &self.max_depth)
            .field("max_edges", &self.max_edges)
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Validated result pinned to one immutable workspace view and source slot.
#[derive(Debug, Eq, PartialEq)]
pub struct ScipRelationshipTraceResult<T> {
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: i64,
    source_slot: SourceSlotId,
    output: T,
}

impl<T> ScipRelationshipTraceResult<T> {
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

/// An invalid precision-trace selection or adapter context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScipRelationshipTraceSelectionError {
    /// A returned immutable workspace-view identity was non-positive.
    InvalidIdentity,
    /// The adapter returned a different immutable workspace/view/slot.
    ContextMismatch,
}

impl fmt::Display for ScipRelationshipTraceSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidIdentity => {
                "SCIP relationship-trace adapter returned an invalid context identity"
            }
            Self::ContextMismatch => {
                "SCIP relationship-trace adapter returned a different immutable context"
            }
        })
    }
}

impl Error for ScipRelationshipTraceSelectionError {}

/// Stable all-or-nothing precision-trace use-case failure.
#[derive(Debug)]
pub enum ScipRelationshipTraceError<E> {
    /// Cancellation was visible before a complete result.
    Cancelled,
    /// The absolute deadline elapsed before a complete result.
    DeadlineExceeded,
    /// The storage adapter failed.
    Port(E),
    /// The adapter violated the requested immutable context.
    InvalidPortOutput(ScipRelationshipTraceSelectionError),
}

impl<E: fmt::Display> fmt::Display for ScipRelationshipTraceError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("SCIP relationship trace cancelled"),
            Self::DeadlineExceeded => {
                formatter.write_str("SCIP relationship trace deadline exceeded")
            }
            Self::Port(error) => {
                write!(formatter, "SCIP relationship-trace adapter failed: {error}")
            }
            Self::InvalidPortOutput(error) => error.fmt(formatter),
        }
    }
}

impl<E: Error + 'static> Error for ScipRelationshipTraceError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Port(error) => Some(error),
            Self::InvalidPortOutput(error) => Some(error),
            Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Executes one context-validated package-scoped SCIP relationship traversal.
pub fn scip_relationship_trace<Port>(
    port: &Port,
    request: ScipRelationshipTraceRequest,
) -> Result<ScipRelationshipTraceResult<Port::Output>, ScipRelationshipTraceError<Port::Error>>
where
    Port: ScipRelationshipTracePort,
{
    check_control(&request.cancelled, request.deadline)?;
    let result = port
        .trace(
            request.selection,
            &request.package_scope,
            &request.root,
            request.direction,
            request.max_depth,
            request.max_edges,
            Arc::clone(&request.cancelled),
            request.deadline,
        )
        .map_err(ScipRelationshipTraceError::Port)?;
    check_control(&request.cancelled, request.deadline)?;
    validate_context(request.selection, &result)?;
    Ok(ScipRelationshipTraceResult {
        connected_workspace: result.connected_workspace,
        workspace_view: result.workspace_view,
        source_slot: result.source_slot,
        output: result.output,
    })
}

fn validate_context<T, E>(
    selection: ScipEvidenceReadSelection,
    result: &ScipRelationshipTracePortResult<T>,
) -> Result<(), ScipRelationshipTraceError<E>> {
    if result.workspace_view <= 0 {
        return Err(ScipRelationshipTraceError::InvalidPortOutput(
            ScipRelationshipTraceSelectionError::InvalidIdentity,
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
        return Err(ScipRelationshipTraceError::InvalidPortOutput(
            ScipRelationshipTraceSelectionError::ContextMismatch,
        ));
    }
    Ok(())
}

fn check_control<E>(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), ScipRelationshipTraceError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(ScipRelationshipTraceError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(ScipRelationshipTraceError::DeadlineExceeded)
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
        MAX_SCIP_RELATIONSHIP_TRACE_DEPTH, ScipEvidenceReadSelection, ScipRelationshipTraceDepth,
        ScipRelationshipTraceDirection, ScipRelationshipTraceError, ScipRelationshipTraceMaxEdges,
        ScipRelationshipTracePort, ScipRelationshipTracePortResult, ScipRelationshipTraceRequest,
        ScipRelationshipTraceSelectionError, ScipSymbol, scip_relationship_trace,
    };
    use crate::PackageScope;

    #[derive(Clone, Copy)]
    struct FakePort {
        workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        workspace_view: i64,
    }

    impl ScipRelationshipTracePort for FakePort {
        type Output = u8;
        type Error = Infallible;

        fn trace(
            &self,
            _selection: ScipEvidenceReadSelection,
            _package_scope: &PackageScope,
            _root: &ScipSymbol,
            _direction: ScipRelationshipTraceDirection,
            _max_depth: ScipRelationshipTraceDepth,
            _max_edges: ScipRelationshipTraceMaxEdges,
            _cancelled: Arc<AtomicBool>,
            _deadline: Instant,
        ) -> Result<ScipRelationshipTracePortResult<Self::Output>, Self::Error> {
            Ok(ScipRelationshipTracePortResult::new(
                self.workspace,
                self.workspace_view,
                self.source_slot,
                7,
            ))
        }
    }

    fn context() -> (ConnectedWorkspaceId, SourceSlotId) {
        let repository = RepositoryIdentityDigest::try_from_slice(&[3; 32])
            .expect("fixture repository identity should be valid");
        (
            ConnectedWorkspaceId::for_single_repository(repository),
            SourceSlotId::try_from_slice(&[4; 32]).expect("fixture source slot should be valid"),
        )
    }

    #[test]
    fn trace_validates_depth_and_pins_the_adapter_context() {
        assert!(ScipRelationshipTraceDepth::try_new(0).is_err());
        assert!(
            ScipRelationshipTraceDepth::try_new(MAX_SCIP_RELATIONSHIP_TRACE_DEPTH + 1).is_err()
        );
        let (workspace, source_slot) = context();
        let result = scip_relationship_trace(
            &FakePort {
                workspace,
                source_slot,
                workspace_view: 9,
            },
            ScipRelationshipTraceRequest::new(
                ScipEvidenceReadSelection::exact_source_slot(workspace, source_slot, 9)
                    .expect("fixture selection should be valid"),
                PackageScope::whole_repository(),
                ScipSymbol::try_new("scip-rust pkg 1 Root.".to_owned())
                    .expect("fixture symbol should be valid"),
                ScipRelationshipTraceDirection::Outgoing,
                ScipRelationshipTraceDepth::try_new(2).expect("fixture depth should be valid"),
                ScipRelationshipTraceMaxEdges::try_new(2)
                    .expect("fixture edge cap should be valid"),
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
            ),
        )
        .expect("matching adapter context should be accepted");
        assert_eq!(result.workspace_view(), 9);
        assert_eq!(result.into_output(), 7);
    }

    #[test]
    fn trace_rejects_cancelled_and_expired_requests_before_adapter_work() {
        let (workspace, source_slot) = context();
        let port = FakePort {
            workspace,
            source_slot,
            workspace_view: 9,
        };
        for (cancelled, deadline, expected) in [
            (
                Arc::new(AtomicBool::new(true)),
                Instant::now() + Duration::from_secs(1),
                true,
            ),
            (
                Arc::new(AtomicBool::new(false)),
                Instant::now() - Duration::from_secs(1),
                false,
            ),
        ] {
            let error = scip_relationship_trace(
                &port,
                ScipRelationshipTraceRequest::new(
                    ScipEvidenceReadSelection::exact_source_slot(workspace, source_slot, 9)
                        .expect("fixture selection should be valid"),
                    PackageScope::whole_repository(),
                    ScipSymbol::try_new("scip-rust pkg 1 Root.".to_owned())
                        .expect("fixture symbol should validate"),
                    ScipRelationshipTraceDirection::Outgoing,
                    ScipRelationshipTraceDepth::try_new(1).expect("fixture depth should be valid"),
                    ScipRelationshipTraceMaxEdges::try_new(1)
                        .expect("fixture edge cap should be valid"),
                    cancelled,
                    deadline,
                ),
            )
            .expect_err("cancelled or expired trace must fail before the adapter");
            assert!(matches!(
                (expected, error),
                (true, ScipRelationshipTraceError::Cancelled)
                    | (false, ScipRelationshipTraceError::DeadlineExceeded)
            ));
        }
    }

    #[test]
    fn trace_rejects_mismatched_or_non_positive_adapter_contexts() {
        let (workspace, source_slot) = context();
        for workspace_view in [0, 8] {
            let error = scip_relationship_trace(
                &FakePort {
                    workspace,
                    source_slot,
                    workspace_view,
                },
                ScipRelationshipTraceRequest::new(
                    ScipEvidenceReadSelection::exact_source_slot(workspace, source_slot, 9)
                        .expect("fixture selection should be valid"),
                    PackageScope::whole_repository(),
                    ScipSymbol::try_new("scip-rust pkg 1 Root.".to_owned())
                        .expect("fixture symbol should be valid"),
                    ScipRelationshipTraceDirection::Incoming,
                    ScipRelationshipTraceDepth::try_new(1).expect("fixture depth should be valid"),
                    ScipRelationshipTraceMaxEdges::try_new(1)
                        .expect("fixture edge cap should be valid"),
                    Arc::new(AtomicBool::new(false)),
                    Instant::now() + Duration::from_secs(1),
                ),
            )
            .expect_err("mismatched context must fail closed");
            assert!(matches!(
                error,
                ScipRelationshipTraceError::InvalidPortOutput(
                    ScipRelationshipTraceSelectionError::InvalidIdentity
                        | ScipRelationshipTraceSelectionError::ContextMismatch
                )
            ));
        }
    }
}
