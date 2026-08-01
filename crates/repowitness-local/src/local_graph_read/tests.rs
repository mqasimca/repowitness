use super::*;

use repowitness_application::RustGraphSymbolQuery;

const REPOSITORY_ID: &str = concat!(
    "rwi1:h:",
    "0101010101010101010101010101010101010101010101010101010101010101"
);
const CONNECTED_WORKSPACE_ID: &str = concat!(
    "cwi1:h:",
    "0202020202020202020202020202020202020202020202020202020202020202"
);
const SOURCE_SLOT_ID: &str = concat!(
    "ssi1:h:",
    "0303030303030303030303030303030303030303030303030303030303030303"
);

#[test]
fn default_graph_read_deadline_is_explicit_and_sufficient_for_full_graph_reads() {
    let request = LocalRustGraphReadRequest::new(
        Path::new("index"),
        REPOSITORY_ID,
        RustGraphReadOperation::Status,
    );
    assert_eq!(
        DEFAULT_LOCAL_RUST_GRAPH_READ_DEADLINE,
        Duration::from_secs(30)
    );
    assert_eq!(request.deadline, DEFAULT_LOCAL_RUST_GRAPH_READ_DEADLINE);
}

#[test]
fn request_is_bounded_and_redacted() {
    let request = LocalRustGraphReadRequest::new(
        Path::new("/private/graph.sqlite3"),
        REPOSITORY_ID,
        RustGraphReadOperation::Search {
            query: RustGraphSymbolQuery::try_new("private_customer_symbol").expect("valid query"),
            limits: RustGraphTraceLimits::default(),
        },
    )
    .with_exact_pin(3, 5)
    .expect("positive pin")
    .with_deadline(Duration::from_secs(1));
    let debug = format!("{request:?}");
    assert!(debug.contains("search"));
    assert!(!debug.contains("/private"));
    assert!(!debug.contains(REPOSITORY_ID));
    assert!(!debug.contains("private_customer_symbol"));
    assert!(
        LocalRustGraphReadRequest::new(
            Path::new("index"),
            REPOSITORY_ID,
            RustGraphReadOperation::Status
        )
        .with_exact_pin(0, 1)
        .is_err()
    );
}

#[test]
fn invalid_identity_fails_before_database_io() {
    let error = read_local_rust_graph(
        LocalRustGraphReadRequest::new(
            Path::new("/missing/private.sqlite3"),
            "invalid-private-identity",
            RustGraphReadOperation::Status,
        ),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("invalid identity should fail before SQLite");
    assert!(matches!(
        error,
        LocalRustGraphReadError::RepositoryIdentity(_)
    ));
}

#[test]
fn connected_workspace_selection_is_explicit_and_redacted() {
    let request = LocalRustGraphReadRequest::for_connected_workspace(
        Path::new("/private/graph.sqlite3"),
        CONNECTED_WORKSPACE_ID,
        SOURCE_SLOT_ID,
        RustGraphReadOperation::Status,
    )
    .with_exact_pin(3, 5)
    .expect("positive pin");
    let selection = resolve_selection(&request).expect("canonical workspace selection");
    assert_eq!(
        selection.connected_workspace(),
        ConnectedWorkspaceIdTextV1::decode(CONNECTED_WORKSPACE_ID)
            .expect("canonical connected workspace")
    );
    assert_eq!(
        selection.source_slot(),
        Some(SourceSlotIdTextV1::decode(SOURCE_SLOT_ID).expect("canonical source slot"))
    );
    assert_eq!(selection.exact_pin(), Some((3, 5)));

    let debug = format!("{request:?}");
    assert!(!debug.contains(CONNECTED_WORKSPACE_ID));
    assert!(!debug.contains(SOURCE_SLOT_ID));
}

#[test]
fn invalid_connected_workspace_identities_fail_before_database_io() {
    let invalid_workspace = read_local_rust_graph(
        LocalRustGraphReadRequest::for_connected_workspace(
            Path::new("/missing/private.sqlite3"),
            "invalid-private-workspace",
            SOURCE_SLOT_ID,
            RustGraphReadOperation::Status,
        ),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("invalid connected workspace should fail before SQLite");
    assert!(matches!(
        invalid_workspace,
        LocalRustGraphReadError::ConnectedWorkspaceIdentity(_)
    ));

    let invalid_source_slot = read_local_rust_graph(
        LocalRustGraphReadRequest::for_connected_workspace(
            Path::new("/missing/private.sqlite3"),
            CONNECTED_WORKSPACE_ID,
            "invalid-private-source-slot",
            RustGraphReadOperation::Status,
        ),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("invalid source slot should fail before SQLite");
    assert!(matches!(
        invalid_source_slot,
        LocalRustGraphReadError::SourceSlotIdentity(_)
    ));
}
