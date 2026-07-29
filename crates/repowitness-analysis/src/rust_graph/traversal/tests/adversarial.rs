use repowitness_domain::MAX_MEMORY_INTEROPERABLE_INTEGER;

use super::*;

#[test]
fn invalid_limits_and_candidate_accounting_are_rejected() {
    for values in [
        (0, 1, 1, 1, 1, 1, 1, 1),
        (1, 0, 1, 1, 1, 1, 1, 1),
        (1, 1, 0, 1, 1, 1, 1, 1),
        (1, 1, 1, 0, 1, 1, 1, 1),
        (1, 1, 1, 1, 0, 1, 1, 1),
        (1, 1, 1, 1, 1, 0, 1, 1),
        (1, 1, 1, 1, 1, 1, 0, 1),
        (1, 1, 1, 1, 1, 1, 1, 0),
        (4_000_001, 1, 1, 1, 1, 1, 1, 1),
        (1, 512 * 1024 * 1024 + 1, 1, 1, 1, 1, 1, 1),
        (1, 1, 257, 1, 1, 1, 1, 1),
        (1, 1, 1, 100_001, 1, 1, 1, 1),
        (1, 1, 1, 1, 1_000_001, 1, 1, 1),
        (1, 1, 1, 1, 1, 4_000_001, 1, 1),
        (1, 1, 1, 1, 1, 1, 1_000_001, 1),
        (1, 1, 1, 1, 1, 1, 1, 256 * 1024 * 1024 + 1),
    ] {
        assert_eq!(
            RustGraphTraceLimits::try_new(
                values.0, values.1, values.2, values.3, values.4, values.5, values.6, values.7,
            ),
            Err(RustGraphTraceError::InvalidLimits)
        );
    }
    assert_eq!(
        RustGraphRelationshipCardinality::try_ambiguous(1, 1, false),
        Err(RustGraphTraceError::InvalidEdge)
    );
    assert_eq!(
        RustGraphRelationshipCardinality::try_ambiguous(3, 2, false),
        Err(RustGraphTraceError::InvalidEdge)
    );
}

#[test]
fn public_identity_constructors_reject_invalid_persisted_fields() {
    let valid = definition(1);
    let non_rust =
        RepositoryPath::try_from_bytes(b"node.txt", RepositoryPathLimits::new(4_096, 64))
            .expect("test path must be syntactically valid");
    let invalid = crate::RustGraphResolutionError::InvalidOccurrence;

    assert_eq!(
        RustGraphDefinitionIdentity::try_new(
            valid.source_slot(),
            non_rust.clone(),
            valid.artifact(),
            valid.fact_ordinal(),
            valid.kind(),
            valid.name_span(),
            valid.declaration_span(),
        ),
        Err(invalid)
    );
    assert_eq!(
        RustGraphDefinitionIdentity::try_new(
            valid.source_slot(),
            valid.path().clone(),
            valid.artifact(),
            MAX_MEMORY_INTEROPERABLE_INTEGER + 1,
            valid.kind(),
            valid.name_span(),
            valid.declaration_span(),
        ),
        Err(invalid)
    );
    assert_eq!(
        RustGraphDefinitionIdentity::try_new(
            valid.source_slot(),
            valid.path().clone(),
            valid.artifact(),
            valid.fact_ordinal(),
            valid.kind(),
            span(4, 4),
            span(0, 10),
        ),
        Err(invalid)
    );
    assert_eq!(
        RustGraphDefinitionIdentity::try_new(
            valid.source_slot(),
            valid.path().clone(),
            valid.artifact(),
            valid.fact_ordinal(),
            valid.kind(),
            span(11, 12),
            span(0, 10),
        ),
        Err(invalid)
    );

    assert_eq!(
        RustGraphSiteIdentity::try_new(
            valid.source_slot(),
            non_rust,
            valid.artifact(),
            RustGraphSiteOrdinal::new(1),
            RustGraphSiteKind::Call,
            span(10, 12),
            span(10, 11),
        ),
        Err(invalid)
    );
    assert_eq!(
        RustGraphSiteIdentity::try_new(
            valid.source_slot(),
            valid.path().clone(),
            valid.artifact(),
            RustGraphSiteOrdinal::new(1),
            RustGraphSiteKind::Call,
            span(10, 12),
            span(10, 10),
        ),
        Err(invalid)
    );
    assert_eq!(
        RustGraphSiteIdentity::try_new(
            valid.source_slot(),
            valid.path().clone(),
            valid.artifact(),
            RustGraphSiteOrdinal::new(1),
            RustGraphSiteKind::Call,
            span(10, 12),
            span(12, 13),
        ),
        Err(invalid)
    );
}

#[test]
fn edge_kind_allow_list_filters_before_visit_accounting() {
    let source = definition(1);
    let call_target = definition(2);
    let reference_target = definition(3);
    let edges = vec![
        edge(&source, &call_target, 1, RustGraphSiteKind::Call),
        edge(&source, &reference_target, 2, RustGraphSiteKind::Reference),
    ];
    let cancelled = AtomicBool::new(false);
    let result = trace_rust_graph(RustGraphTraceRequest::new(
        &edges,
        RustGraphTraceStart::Definition(source),
        RustGraphTraceDirection::Outbound,
        RustGraphEdgeKinds::try_new(false, true, false).expect("allow-list must be valid"),
        RustGraphTraceLimits::DEFAULT,
        RustGraphTraceCoverage::default(),
        RustGraphTraceControl::new(&cancelled, deadline()),
    ))
    .expect("filtered trace must succeed");
    assert_eq!(result.visited_edges(), 1);
    assert_eq!(result.edges().len(), 1);
    assert_eq!(result.edges()[0].relationship().target(), &reference_target);
}

#[test]
fn cancellation_and_elapsed_deadline_return_no_partial_result() {
    let source = definition(1);
    let target = definition(2);
    let edges = [edge(&source, &target, 1, RustGraphSiteKind::Call)];
    let cancelled = AtomicBool::new(true);
    let request = RustGraphTraceRequest::new(
        &edges,
        RustGraphTraceStart::Definition(source.clone()),
        RustGraphTraceDirection::Outbound,
        RustGraphEdgeKinds::ALL,
        RustGraphTraceLimits::DEFAULT,
        RustGraphTraceCoverage::default(),
        RustGraphTraceControl::new(&cancelled, deadline()),
    );
    assert_eq!(
        trace_rust_graph(request),
        Err(RustGraphTraceError::Cancelled)
    );

    let active = AtomicBool::new(false);
    let elapsed = RustGraphTraceRequest::new(
        &edges,
        RustGraphTraceStart::Definition(source),
        RustGraphTraceDirection::Outbound,
        RustGraphEdgeKinds::ALL,
        RustGraphTraceLimits::DEFAULT,
        RustGraphTraceCoverage::default(),
        RustGraphTraceControl::new(&active, Instant::now()),
    );
    assert_eq!(
        trace_rust_graph(elapsed),
        Err(RustGraphTraceError::DeadlineExceeded)
    );
}

#[test]
fn debug_output_redacts_paths_digests_and_deadlines() {
    let source = definition(42);
    let target = definition(43);
    let relationship = edge(&source, &target, 1, RustGraphSiteKind::Call);
    let debug = format!("{relationship:?}");
    assert!(!debug.contains("node-42.rs"));
    assert!(!debug.contains("node-43.rs"));
    assert!(!debug.contains(&"2A".repeat(32)));

    let cancelled = AtomicBool::new(false);
    let control = RustGraphTraceControl::new(&cancelled, deadline());
    let control_debug = format!("{control:?}");
    assert!(control_debug.contains("<monotonic>"));
}
