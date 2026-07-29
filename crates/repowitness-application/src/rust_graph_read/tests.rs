use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use repowitness_domain::{ConnectedWorkspaceId, SourceSlotId};

use super::*;

struct FakePort {
    result_workspace: ConnectedWorkspaceId,
    result_view: i64,
    result_generation: i64,
    calls: RefCell<u32>,
}

impl RustGraphReadPort for FakePort {
    type Output = &'static str;
    type Error = &'static str;

    fn read(
        &self,
        _selection: RustGraphReadSelection,
        _operation: &RustGraphReadOperation,
        _cancelled: Arc<AtomicBool>,
        _deadline: Instant,
    ) -> Result<RustGraphReadPortResult<Self::Output>, Self::Error> {
        *self.calls.borrow_mut() += 1;
        Ok(RustGraphReadPortResult::new(
            self.result_workspace,
            self.result_view,
            self.result_generation,
            "complete",
        ))
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(1)
}

#[test]
fn active_and_exact_contexts_are_validated() {
    let workspace = ConnectedWorkspaceId::new([0x11; 32]);
    let source_slot = SourceSlotId::new([0x12; 32]);
    let port = FakePort {
        result_workspace: workspace,
        result_view: 7,
        result_generation: 9,
        calls: RefCell::new(0),
    };
    let active = rust_graph_read(
        &port,
        RustGraphReadRequest::new(
            RustGraphReadSelection::active(workspace),
            RustGraphReadOperation::Status,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
    )
    .expect("active context should be admitted");
    assert_eq!(active.workspace_view(), 7);
    assert_eq!(active.graph_generation(), 9);
    assert_eq!(active.output(), &"complete");

    let exact = RustGraphReadSelection::exact(workspace, 7, 9).expect("valid exact selection");
    rust_graph_read(
        &port,
        RustGraphReadRequest::new(
            exact,
            RustGraphReadOperation::Status,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
    )
    .expect("matching exact context should be admitted");
    assert_eq!(*port.calls.borrow(), 2);

    let active_source = RustGraphReadSelection::active_source_slot(workspace, source_slot);
    assert_eq!(active_source.source_slot(), Some(source_slot));
    let exact_source = RustGraphReadSelection::exact_source_slot(workspace, source_slot, 7, 9)
        .expect("valid source-slot selection");
    assert_eq!(exact_source.exact_pin(), Some((7, 9)));
    assert_eq!(exact_source.source_slot(), Some(source_slot));
}

#[test]
fn wrong_context_and_invalid_identifiers_fail_closed() {
    let workspace = ConnectedWorkspaceId::new([0x22; 32]);
    assert!(RustGraphReadSelection::exact(workspace, 0, 1).is_err());
    assert!(RustGraphReadSelection::exact(workspace, 1, -1).is_err());

    let port = FakePort {
        result_workspace: workspace,
        result_view: 8,
        result_generation: 9,
        calls: RefCell::new(0),
    };
    let error = rust_graph_read(
        &port,
        RustGraphReadRequest::new(
            RustGraphReadSelection::exact(workspace, 7, 9).expect("valid request"),
            RustGraphReadOperation::Status,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
    )
    .expect_err("a different immutable view must fail");
    assert!(matches!(
        error,
        RustGraphReadError::InvalidPortOutput(RustGraphReadSelectionError::ContextMismatch)
    ));
}

#[test]
fn cancellation_and_deadline_stop_before_port_access() {
    let workspace = ConnectedWorkspaceId::new([0x33; 32]);
    let port = FakePort {
        result_workspace: workspace,
        result_view: 1,
        result_generation: 1,
        calls: RefCell::new(0),
    };
    let cancelled = Arc::new(AtomicBool::new(true));
    let error = rust_graph_read(
        &port,
        RustGraphReadRequest::new(
            RustGraphReadSelection::active(workspace),
            RustGraphReadOperation::Status,
            Arc::clone(&cancelled),
            deadline(),
        ),
    )
    .expect_err("pre-cancellation must fail");
    assert!(matches!(error, RustGraphReadError::Cancelled));

    cancelled.store(false, Ordering::Release);
    let error = rust_graph_read(
        &port,
        RustGraphReadRequest::new(
            RustGraphReadSelection::active(workspace),
            RustGraphReadOperation::Status,
            cancelled,
            Instant::now(),
        ),
    )
    .expect_err("elapsed deadline must fail");
    assert!(matches!(error, RustGraphReadError::DeadlineExceeded));
    assert_eq!(*port.calls.borrow(), 0);
}

#[test]
fn query_and_debug_are_bounded_and_redacted() {
    assert!(matches!(
        RustGraphSymbolQuery::try_new(""),
        Err(RustGraphSymbolQueryError::Empty)
    ));
    assert!(RustGraphSymbolQuery::try_new("private\nsymbol").is_err());
    assert!(RustGraphSymbolQuery::try_new(&"x".repeat(MAX_RUST_GRAPH_QUERY_BYTES + 1)).is_err());

    let query = RustGraphSymbolQuery::try_new("private_customer_symbol").expect("valid query");
    let operation = RustGraphReadOperation::Search {
        query,
        limits: RustGraphTraceLimits::default(),
    };
    let request = RustGraphReadRequest::new(
        RustGraphReadSelection::active(ConnectedWorkspaceId::new([0x44; 32])),
        operation,
        Arc::new(AtomicBool::new(false)),
        deadline(),
    );
    let debug = format!("{request:?}");
    assert!(debug.contains("search"));
    assert!(!debug.contains("private_customer_symbol"));
    assert!(!debug.contains("44"));
}
