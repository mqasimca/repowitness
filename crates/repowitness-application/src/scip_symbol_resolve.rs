//! Exact syntax-span to SCIP-symbol navigation over one immutable workspace view.
//!
//! This use case deliberately resolves no source-language syntax itself. The
//! local adapter may return an opaque provider symbol only when an already
//! imported SCIP overlay has exactly one symbol at the caller's exact source
//! path, content digest, and identifier span.

use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_domain::{
    ByteSpan, ConnectedWorkspaceId, RepositoryPath, SourceContentDigest, SourceSlotId,
};

use crate::ScipEvidenceReadSelection;

/// Narrow storage-neutral boundary for one exact SCIP syntax-span resolution.
pub trait ScipSymbolResolvePort {
    /// Categorical adapter output; it must not infer a symbol from a name.
    type Output;
    /// Stable adapter failure.
    type Error;

    /// Resolves one exact source identifier span in one selected immutable view.
    fn resolve(
        &self,
        selection: ScipEvidenceReadSelection,
        path: &RepositoryPath,
        content: SourceContentDigest,
        name_span: ByteSpan,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ScipSymbolResolvePortResult<Self::Output>, Self::Error>;
}

/// Exact context returned by a SCIP syntax-resolution adapter.
pub struct ScipSymbolResolvePortResult<T> {
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: i64,
    source_slot: SourceSlotId,
    output: T,
}

impl<T> ScipSymbolResolvePortResult<T> {
    /// Constructs one adapter result for immutable-context validation.
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

/// Complete request for one exact identifier-span resolution.
pub struct ScipSymbolResolveRequest {
    selection: ScipEvidenceReadSelection,
    path: RepositoryPath,
    content: SourceContentDigest,
    name_span: ByteSpan,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl ScipSymbolResolveRequest {
    /// Constructs one request from already validated boundary values.
    #[must_use]
    pub const fn new(
        selection: ScipEvidenceReadSelection,
        path: RepositoryPath,
        content: SourceContentDigest,
        name_span: ByteSpan,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            selection,
            path,
            content,
            name_span,
            cancelled,
            deadline,
        }
    }
}

impl fmt::Debug for ScipSymbolResolveRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScipSymbolResolveRequest")
            .field("selection", &self.selection)
            .field("path", &"<redacted-path>")
            .field("content", &"<redacted-digest>")
            .field("name_span", &self.name_span)
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Validated result pinned to one concrete workspace view and source slot.
#[derive(Debug, Eq, PartialEq)]
pub struct ScipSymbolResolveResult<T> {
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: i64,
    source_slot: SourceSlotId,
    output: T,
}

impl<T> ScipSymbolResolveResult<T> {
    /// Returns the exact connected workspace read by the adapter.
    #[must_use]
    pub const fn connected_workspace(&self) -> ConnectedWorkspaceId {
        self.connected_workspace
    }

    /// Returns the exact immutable view used by the adapter.
    #[must_use]
    pub const fn workspace_view(&self) -> i64 {
        self.workspace_view
    }

    /// Returns the exact source slot used by the adapter.
    #[must_use]
    pub const fn source_slot(&self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the categorical resolution output.
    #[must_use]
    pub const fn output(&self) -> &T {
        &self.output
    }

    /// Consumes the result and returns its categorical resolution output.
    #[must_use]
    pub fn into_output(self) -> T {
        self.output
    }
}

/// Invalid selection or adapter context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScipSymbolResolveSelectionError {
    /// A requested or returned immutable view identity was non-positive.
    InvalidIdentity,
    /// The adapter returned a workspace, view, or source slot inconsistent with the request.
    ContextMismatch,
}

impl fmt::Display for ScipSymbolResolveSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidIdentity => {
                "SCIP symbol resolution returned an invalid immutable identity"
            }
            Self::ContextMismatch => {
                "SCIP symbol resolution returned a different immutable context"
            }
        })
    }
}

impl Error for ScipSymbolResolveSelectionError {}

/// Stable all-or-nothing exact SCIP syntax-resolution failure.
#[derive(Debug)]
pub enum ScipSymbolResolveError<E> {
    /// Cancellation was visible before a complete result existed.
    Cancelled,
    /// The deadline elapsed before a complete result existed.
    DeadlineExceeded,
    /// The local adapter failed.
    Port(E),
    /// The adapter violated the selected immutable context.
    InvalidPortOutput(ScipSymbolResolveSelectionError),
}

impl<E: fmt::Display> fmt::Display for ScipSymbolResolveError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("SCIP symbol resolution was cancelled"),
            Self::DeadlineExceeded => {
                formatter.write_str("SCIP symbol resolution deadline exceeded")
            }
            Self::Port(error) => {
                write!(formatter, "SCIP symbol resolution adapter failed: {error}")
            }
            Self::InvalidPortOutput(error) => error.fmt(formatter),
        }
    }
}

impl<E> Error for ScipSymbolResolveError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Port(error) => Some(error),
            Self::InvalidPortOutput(error) => Some(error),
            Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Executes one context-validated exact syntax-span to opaque SCIP-symbol read.
pub fn scip_symbol_resolve<Port>(
    port: &Port,
    request: ScipSymbolResolveRequest,
) -> Result<ScipSymbolResolveResult<Port::Output>, ScipSymbolResolveError<Port::Error>>
where
    Port: ScipSymbolResolvePort,
{
    check_control(&request.cancelled, request.deadline)?;
    let result = port
        .resolve(
            request.selection,
            &request.path,
            request.content,
            request.name_span,
            Arc::clone(&request.cancelled),
            request.deadline,
        )
        .map_err(ScipSymbolResolveError::Port)?;
    check_control(&request.cancelled, request.deadline)?;
    validate_context(request.selection, &result)?;
    Ok(ScipSymbolResolveResult {
        connected_workspace: result.connected_workspace,
        workspace_view: result.workspace_view,
        source_slot: result.source_slot,
        output: result.output,
    })
}

fn validate_context<T, E>(
    selection: ScipEvidenceReadSelection,
    result: &ScipSymbolResolvePortResult<T>,
) -> Result<(), ScipSymbolResolveError<E>> {
    if result.workspace_view <= 0 {
        return Err(ScipSymbolResolveError::InvalidPortOutput(
            ScipSymbolResolveSelectionError::InvalidIdentity,
        ));
    }
    if result.connected_workspace != selection.connected_workspace()
        || selection
            .source_slot()
            .is_some_and(|source_slot| source_slot != result.source_slot)
        || selection
            .workspace_view()
            .is_some_and(|view| view != result.workspace_view)
    {
        return Err(ScipSymbolResolveError::InvalidPortOutput(
            ScipSymbolResolveSelectionError::ContextMismatch,
        ));
    }
    Ok(())
}

fn check_control<E>(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), ScipSymbolResolveError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(ScipSymbolResolveError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(ScipSymbolResolveError::DeadlineExceeded)
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

    use repowitness_domain::{
        ByteOffset, ConnectedWorkspaceId, RepositoryIdentityDigest, RepositoryPath,
        RepositoryPathLimits, SourceContentDigest, SourceSlotId,
    };

    use super::{
        ScipEvidenceReadSelection, ScipSymbolResolvePort, ScipSymbolResolvePortResult,
        ScipSymbolResolveRequest, ScipSymbolResolveSelectionError, scip_symbol_resolve,
    };

    struct FakePort {
        workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        view: i64,
    }

    impl ScipSymbolResolvePort for FakePort {
        type Output = u8;
        type Error = Infallible;

        fn resolve(
            &self,
            _: ScipEvidenceReadSelection,
            _: &RepositoryPath,
            _: SourceContentDigest,
            _: super::ByteSpan,
            _: Arc<AtomicBool>,
            _: Instant,
        ) -> Result<ScipSymbolResolvePortResult<Self::Output>, Self::Error> {
            Ok(ScipSymbolResolvePortResult::new(
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

    fn request(selection: ScipEvidenceReadSelection) -> ScipSymbolResolveRequest {
        ScipSymbolResolveRequest::new(
            selection,
            RepositoryPath::try_from_bytes(b"src/lib.rs", RepositoryPathLimits::new(4096, 256))
                .expect("fixture path should be valid"),
            SourceContentDigest::new([3; 32]),
            super::ByteSpan::try_new(ByteOffset::new(4), ByteOffset::new(8))
                .expect("fixture span should be valid"),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
    }

    #[test]
    fn exact_context_is_preserved_and_view_mismatches_fail_closed() {
        let workspace =
            ConnectedWorkspaceId::for_single_repository(RepositoryIdentityDigest::new([1; 32]));
        let source_slot = SourceSlotId::new([2; 32]);
        let selection = ScipEvidenceReadSelection::exact_source_slot(workspace, source_slot, 9)
            .expect("positive view should select");

        let result = scip_symbol_resolve(
            &FakePort {
                workspace,
                source_slot,
                view: 9,
            },
            request(selection),
        )
        .expect("matching immutable context should pass");
        assert_eq!(result.workspace_view(), 9);
        assert_eq!(result.source_slot(), source_slot);
        assert_eq!(*result.output(), 7);

        let error = scip_symbol_resolve(
            &FakePort {
                workspace,
                source_slot,
                view: 10,
            },
            request(selection),
        )
        .expect_err("different immutable view must fail");
        assert!(matches!(
            error,
            super::ScipSymbolResolveError::InvalidPortOutput(
                ScipSymbolResolveSelectionError::ContextMismatch
            )
        ));
    }
}
