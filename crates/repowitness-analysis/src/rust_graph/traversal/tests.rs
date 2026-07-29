use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

use repowitness_domain::{
    AnalysisArtifactDigest, ByteOffset, ByteSpan, RepositoryPath, RepositoryPathLimits,
    SourceSlotId,
};

use super::{
    RUST_GRAPH_TRAVERSAL_PROFILE_VERSION, RustGraphEdgeKinds, RustGraphImpactClass,
    RustGraphImpactRequest, RustGraphRelationshipCardinality, RustGraphTraceControl,
    RustGraphTraceCoverage, RustGraphTraceDirection, RustGraphTraceError, RustGraphTraceLimits,
    RustGraphTraceRequest, RustGraphTraceStart, RustGraphTraversalEdge, analyze_rust_graph_impact,
    trace_rust_graph,
};
use crate::{
    RustGraphDefinitionIdentity, RustGraphResolutionEvidence, RustGraphSiteEvidence,
    RustGraphSiteIdentity, RustGraphSiteKind, RustGraphSiteOrdinal, RustSymbolKind,
};

mod adversarial;

fn deadline() -> Instant {
    Instant::now()
        .checked_add(Duration::from_secs(10))
        .expect("short test deadline must fit")
}

fn span(start: u64, end: u64) -> ByteSpan {
    ByteSpan::try_new(ByteOffset::new(start), ByteOffset::new(end))
        .expect("test span must be ordered")
}

fn definition(id: u16) -> RustGraphDefinitionIdentity {
    let path = format!("node-{id}.rs");
    RustGraphDefinitionIdentity::try_new(
        SourceSlotId::new([u8::try_from(id % 251).expect("modulo fits"); 32]),
        RepositoryPath::try_from_bytes(path.as_bytes(), RepositoryPathLimits::new(4_096, 64))
            .expect("test path must be valid"),
        AnalysisArtifactDigest::new([u8::try_from(id % 253).expect("modulo fits"); 32]),
        u64::from(id),
        RustSymbolKind::Function,
        span(1, 2),
        span(0, 10_000),
    )
    .expect("test definition must be valid")
}

fn site(
    source: &RustGraphDefinitionIdentity,
    ordinal: u32,
    kind: RustGraphSiteKind,
) -> RustGraphSiteIdentity {
    let start = 100 + u64::from(ordinal) * 2;
    RustGraphSiteIdentity::try_new(
        source.source_slot(),
        source.path().clone(),
        AnalysisArtifactDigest::new([u8::try_from(ordinal % 251).expect("modulo fits"); 32]),
        RustGraphSiteOrdinal::new(ordinal),
        kind,
        span(start, start + 1),
        span(start, start + 1),
    )
    .expect("test site must be valid")
}

fn edge(
    source: &RustGraphDefinitionIdentity,
    target: &RustGraphDefinitionIdentity,
    ordinal: u32,
    kind: RustGraphSiteKind,
) -> RustGraphTraversalEdge {
    edge_with(
        source,
        target,
        site(source, ordinal, kind),
        RustGraphSiteEvidence::DirectSyntax,
        RustGraphResolutionEvidence::QualifiedSyntax,
        RustGraphRelationshipCardinality::Unique,
    )
}

fn edge_with(
    source: &RustGraphDefinitionIdentity,
    target: &RustGraphDefinitionIdentity,
    site: RustGraphSiteIdentity,
    extraction: RustGraphSiteEvidence,
    resolution: RustGraphResolutionEvidence,
    cardinality: RustGraphRelationshipCardinality,
) -> RustGraphTraversalEdge {
    RustGraphTraversalEdge::try_new(
        source.clone(),
        site,
        target.clone(),
        extraction,
        resolution,
        cardinality,
    )
    .expect("test edge must be valid")
}

#[allow(
    clippy::too_many_arguments,
    reason = "the helper keeps each independently tested resource bound visible"
)]
fn limits(
    input_edges: u64,
    input_bytes: u64,
    depth: u32,
    results: u32,
    nodes: u64,
    visited_edges: u64,
    frontier: u64,
    output_bytes: u64,
) -> RustGraphTraceLimits {
    RustGraphTraceLimits::try_new(
        input_edges,
        input_bytes,
        depth,
        results,
        nodes,
        visited_edges,
        frontier,
        output_bytes,
    )
    .expect("test limits must be valid")
}

fn trace(
    edges: &[RustGraphTraversalEdge],
    start: RustGraphTraceStart,
    direction: RustGraphTraceDirection,
    limits: RustGraphTraceLimits,
) -> Result<super::RustGraphTraceResult, RustGraphTraceError> {
    let cancelled = AtomicBool::new(false);
    trace_rust_graph(RustGraphTraceRequest::new(
        edges,
        start,
        direction,
        RustGraphEdgeKinds::ALL,
        limits,
        RustGraphTraceCoverage::default(),
        RustGraphTraceControl::new(&cancelled, deadline()),
    ))
}

#[test]
fn traversal_profile_and_stable_edge_kinds_are_explicit() {
    assert_eq!(RUST_GRAPH_TRAVERSAL_PROFILE_VERSION, 1);
    assert_eq!(super::RustGraphEdgeKind::Import.as_str(), "import");
    assert_eq!(super::RustGraphEdgeKind::Reference.as_str(), "reference");
    assert_eq!(super::RustGraphEdgeKind::Call.as_str(), "call");
    assert_eq!(
        RustGraphEdgeKinds::try_new(false, false, false),
        Err(RustGraphTraceError::InvalidEdgeKinds)
    );
}

#[test]
fn cycles_terminate_with_stable_shortest_depths_in_both_directions() {
    let a = definition(1);
    let b = definition(2);
    let c = definition(3);
    let edges = vec![
        edge(&a, &b, 1, RustGraphSiteKind::Call),
        edge(&b, &c, 2, RustGraphSiteKind::Reference),
        edge(&c, &a, 3, RustGraphSiteKind::Import),
    ];

    let outbound = trace(
        &edges,
        RustGraphTraceStart::Definition(a.clone()),
        RustGraphTraceDirection::Outbound,
        RustGraphTraceLimits::DEFAULT,
    )
    .expect("cycle trace must succeed");
    assert_eq!(
        outbound
            .edges()
            .iter()
            .map(|edge| edge.depth())
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(outbound.visited_nodes(), 3);
    assert_eq!(outbound.visited_edges(), 3);
    assert_eq!(outbound.maximum_completed_depth(), 3);
    assert!(!outbound.truncation().any());

    let inbound = trace(
        &edges,
        RustGraphTraceStart::Definition(a),
        RustGraphTraceDirection::Inbound,
        RustGraphTraceLimits::DEFAULT,
    )
    .expect("reverse cycle trace must succeed");
    assert_eq!(
        inbound
            .edges()
            .iter()
            .map(|edge| edge.depth())
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(inbound.visited_nodes(), 3);
}

#[test]
fn exact_site_start_does_not_admit_sibling_relationships() {
    let a = definition(1);
    let b = definition(2);
    let c = definition(3);
    let first = edge(&a, &b, 1, RustGraphSiteKind::Call);
    let second = edge(&a, &c, 2, RustGraphSiteKind::Call);
    let edges = vec![second, first.clone()];

    let result = trace(
        &edges,
        RustGraphTraceStart::Site(first.site().clone()),
        RustGraphTraceDirection::Outbound,
        RustGraphTraceLimits::DEFAULT,
    )
    .expect("exact site must trace");
    assert_eq!(result.edges().len(), 1);
    assert_eq!(result.edges()[0].relationship(), &first);

    let absent = site(&a, 99, RustGraphSiteKind::Call);
    assert_eq!(
        trace(
            &edges,
            RustGraphTraceStart::Site(absent),
            RustGraphTraceDirection::Outbound,
            RustGraphTraceLimits::DEFAULT,
        ),
        Err(RustGraphTraceError::StartUnavailable)
    );
}

#[test]
fn more_than_one_sort_chunk_is_permutation_invariant() {
    let root = definition(0);
    let mut edges = (1..=600)
        .map(|id| {
            edge(
                &root,
                &definition(id),
                u32::from(id),
                RustGraphSiteKind::Call,
            )
        })
        .collect::<Vec<_>>();
    let expected = trace(
        &edges,
        RustGraphTraceStart::Definition(root.clone()),
        RustGraphTraceDirection::Outbound,
        RustGraphTraceLimits::DEFAULT,
    )
    .expect("large deterministic trace must succeed");

    edges.reverse();
    edges.rotate_left(137);
    let actual = trace(
        &edges,
        RustGraphTraceStart::Definition(root),
        RustGraphTraceDirection::Outbound,
        RustGraphTraceLimits::DEFAULT,
    )
    .expect("permuted deterministic trace must succeed");
    assert_eq!(actual, expected);
    assert_eq!(actual.edges().len(), 600);
}

#[test]
fn ambiguous_candidates_remain_distinct_and_require_complete_groups() {
    let source = definition(1);
    let first = definition(2);
    let second = definition(3);
    let shared_site = site(&source, 1, RustGraphSiteKind::Call);
    let ambiguous = RustGraphRelationshipCardinality::try_ambiguous(2, 2, false)
        .expect("accounting must be valid");
    let edges = vec![
        edge_with(
            &source,
            &first,
            shared_site.clone(),
            RustGraphSiteEvidence::DirectSyntax,
            RustGraphResolutionEvidence::ExactNameHeuristic,
            ambiguous,
        ),
        edge_with(
            &source,
            &second,
            shared_site,
            RustGraphSiteEvidence::DirectSyntax,
            RustGraphResolutionEvidence::ExactNameHeuristic,
            ambiguous,
        ),
    ];
    let result = trace(
        &edges,
        RustGraphTraceStart::Definition(source.clone()),
        RustGraphTraceDirection::Outbound,
        RustGraphTraceLimits::DEFAULT,
    )
    .expect("complete ambiguity must trace");
    assert_eq!(result.edges().len(), 2);
    assert!(
        result
            .edges()
            .iter()
            .all(|edge| { edge.relationship().cardinality().is_ambiguous() })
    );

    assert_eq!(
        trace(
            &edges[..1],
            RustGraphTraceStart::Definition(source),
            RustGraphTraceDirection::Outbound,
            RustGraphTraceLimits::DEFAULT,
        ),
        Err(RustGraphTraceError::InvalidEdge)
    );
}

#[test]
fn duplicate_targets_and_invalid_source_site_joins_fail_closed() {
    let source = definition(1);
    let target = definition(2);
    let relationship = edge(&source, &target, 1, RustGraphSiteKind::Call);
    assert_eq!(
        trace(
            &[relationship.clone(), relationship],
            RustGraphTraceStart::Definition(source.clone()),
            RustGraphTraceDirection::Outbound,
            RustGraphTraceLimits::DEFAULT,
        ),
        Err(RustGraphTraceError::DuplicateEdge)
    );

    let other = definition(3);
    assert_eq!(
        RustGraphTraversalEdge::try_new(
            source,
            site(&other, 1, RustGraphSiteKind::Call),
            target,
            RustGraphSiteEvidence::DirectSyntax,
            RustGraphResolutionEvidence::QualifiedSyntax,
            RustGraphRelationshipCardinality::Unique,
        ),
        Err(RustGraphTraceError::InvalidEdge)
    );
}

#[test]
fn every_traversal_bound_has_an_independent_outcome() {
    let a = definition(1);
    let b = definition(2);
    let c = definition(3);
    let chain = vec![
        edge(&a, &b, 1, RustGraphSiteKind::Call),
        edge(&b, &c, 2, RustGraphSiteKind::Call),
    ];
    let depth = trace(
        &chain,
        RustGraphTraceStart::Definition(a.clone()),
        RustGraphTraceDirection::Outbound,
        limits(10, 1_000_000, 1, 10, 10, 10, 10, 1_000_000),
    )
    .expect("depth-bounded trace must succeed");
    assert!(depth.truncation().depth());
    let leaf = trace(
        &chain[..1],
        RustGraphTraceStart::Definition(a.clone()),
        RustGraphTraceDirection::Outbound,
        limits(10, 1_000_000, 1, 10, 10, 10, 10, 1_000_000),
    )
    .expect("leaf frontier must succeed");
    assert!(!leaf.truncation().depth());

    let nodes = trace(
        &chain,
        RustGraphTraceStart::Definition(a.clone()),
        RustGraphTraceDirection::Outbound,
        limits(10, 1_000_000, 10, 10, 1, 10, 10, 1_000_000),
    )
    .expect("node-bounded trace must succeed");
    assert!(nodes.truncation().visited_nodes());
    assert!(nodes.edges().is_empty());

    let fanout = vec![
        edge(&a, &b, 1, RustGraphSiteKind::Call),
        edge(&a, &c, 2, RustGraphSiteKind::Call),
    ];
    let visited_edges = trace(
        &fanout,
        RustGraphTraceStart::Definition(a.clone()),
        RustGraphTraceDirection::Outbound,
        limits(10, 1_000_000, 10, 10, 10, 1, 10, 1_000_000),
    )
    .expect("edge-bounded trace must succeed");
    assert!(visited_edges.truncation().visited_edges());
    assert_eq!(visited_edges.edges().len(), 1);

    let frontier = trace(
        &fanout,
        RustGraphTraceStart::Definition(a.clone()),
        RustGraphTraceDirection::Outbound,
        limits(10, 1_000_000, 10, 10, 10, 10, 1, 1_000_000),
    )
    .expect("frontier-bounded trace must succeed");
    assert!(frontier.truncation().frontier());
    assert_eq!(frontier.edges().len(), 2);

    let results = trace(
        &fanout,
        RustGraphTraceStart::Definition(a),
        RustGraphTraceDirection::Outbound,
        limits(10, 1_000_000, 10, 1, 10, 10, 10, 1_000_000),
    )
    .expect("result-bounded trace must succeed");
    assert!(results.truncation().results());
    assert_eq!(results.edges().len(), 1);
}

#[test]
fn input_and_output_byte_limits_are_inclusive_and_fail_without_output() {
    let source = definition(1);
    let target = definition(2);
    let edges = [edge(&source, &target, 1, RustGraphSiteKind::Call)];
    let baseline = trace(
        &edges,
        RustGraphTraceStart::Definition(source.clone()),
        RustGraphTraceDirection::Outbound,
        RustGraphTraceLimits::DEFAULT,
    )
    .expect("baseline trace must succeed");

    let exact = limits(
        10,
        baseline.input_bytes(),
        10,
        10,
        10,
        10,
        10,
        baseline.output_bytes(),
    );
    assert!(
        trace(
            &edges,
            RustGraphTraceStart::Definition(source.clone()),
            RustGraphTraceDirection::Outbound,
            exact,
        )
        .is_ok()
    );

    let input_short = limits(
        10,
        baseline.input_bytes() - 1,
        10,
        10,
        10,
        10,
        10,
        1_000_000,
    );
    assert_eq!(
        trace(
            &edges,
            RustGraphTraceStart::Definition(source.clone()),
            RustGraphTraceDirection::Outbound,
            input_short,
        ),
        Err(RustGraphTraceError::InputByteLimitExceeded)
    );
    assert_eq!(
        trace(
            &edges,
            RustGraphTraceStart::Definition(source.clone()),
            RustGraphTraceDirection::Outbound,
            limits(1, 1_000_000, 10, 10, 10, 10, 10, 1_000_000),
        ),
        Ok(baseline.clone())
    );
    let too_many = [
        edges[0].clone(),
        edge(&source, &definition(3), 2, RustGraphSiteKind::Call),
    ];
    assert_eq!(
        trace(
            &too_many,
            RustGraphTraceStart::Definition(source.clone()),
            RustGraphTraceDirection::Outbound,
            limits(1, 1_000_000, 10, 10, 10, 10, 10, 1_000_000),
        ),
        Err(RustGraphTraceError::InputEdgeLimitExceeded)
    );

    let output_short = limits(
        10,
        1_000_000,
        10,
        10,
        10,
        10,
        10,
        baseline.output_bytes() - 1,
    );
    assert_eq!(
        trace(
            &edges,
            RustGraphTraceStart::Definition(source),
            RustGraphTraceDirection::Outbound,
            output_short,
        ),
        Err(RustGraphTraceError::OutputLimitExceeded)
    );
}

#[test]
fn impact_propagates_direct_and_possible_paths_without_hiding_unknown_coverage() {
    let target = definition(1);
    let direct = definition(2);
    let heuristic = definition(3);
    let ambiguous = definition(4);
    let alternate = definition(5);
    let ambiguous_site = site(&ambiguous, 3, RustGraphSiteKind::Reference);
    let ambiguous_cardinality = RustGraphRelationshipCardinality::try_ambiguous(2, 2, false)
        .expect("accounting must be valid");
    let edges = vec![
        edge(&direct, &target, 1, RustGraphSiteKind::Call),
        edge_with(
            &heuristic,
            &direct,
            site(&heuristic, 2, RustGraphSiteKind::Call),
            RustGraphSiteEvidence::DirectSyntax,
            RustGraphResolutionEvidence::ExactNameHeuristic,
            RustGraphRelationshipCardinality::Unique,
        ),
        edge_with(
            &ambiguous,
            &direct,
            ambiguous_site.clone(),
            RustGraphSiteEvidence::DirectSyntax,
            RustGraphResolutionEvidence::LexicalSyntax,
            ambiguous_cardinality,
        ),
        edge_with(
            &ambiguous,
            &alternate,
            ambiguous_site,
            RustGraphSiteEvidence::DirectSyntax,
            RustGraphResolutionEvidence::LexicalSyntax,
            ambiguous_cardinality,
        ),
    ];
    let cancelled = AtomicBool::new(false);
    let result = analyze_rust_graph_impact(RustGraphImpactRequest::new(
        &edges,
        target,
        RustGraphEdgeKinds::ALL,
        RustGraphTraceLimits::DEFAULT,
        RustGraphTraceCoverage::new(1, 0, 1, 0, 0, 0, 0, 0),
        RustGraphTraceControl::new(&cancelled, deadline()),
    ))
    .expect("impact must succeed");
    assert!(result.unknown_coverage());
    let classes = result
        .impacted()
        .iter()
        .map(|impact| (impact.definition().clone(), impact.class()))
        .collect::<Vec<_>>();
    assert!(classes.contains(&(direct, RustGraphImpactClass::DirectlyConnected)));
    assert!(classes.contains(&(heuristic, RustGraphImpactClass::Possible)));
    assert!(classes.contains(&(ambiguous, RustGraphImpactClass::Possible)));
}
