use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

use super::{
    RUST_GRAPH_SITE_PROFILE_VERSION, RustGraphAnalysisControl, RustGraphAnalysisError,
    RustGraphAnalysisLimits, RustGraphEnclosingDefinition, RustGraphSite, RustGraphSiteAnalysis,
    RustGraphSiteAnalyzer, RustGraphSiteEvidence, RustGraphSiteKind, RustGraphSiteOrdinal,
    TraversalState,
};
use crate::RustSymbolKind;
use repowitness_domain::{ByteOffset, ByteSpan};

mod adversarial;

fn control(cancelled: &AtomicBool) -> RustGraphAnalysisControl<'_> {
    RustGraphAnalysisControl::new(
        cancelled,
        Instant::now()
            .checked_add(Duration::from_secs(5))
            .expect("short test deadline must be representable"),
    )
}

fn analyze(source: &[u8]) -> RustGraphSiteAnalysis {
    let cancelled = AtomicBool::new(false);
    RustGraphSiteAnalyzer::new()
        .expect("Rust grammar must load")
        .analyze(
            source,
            RustGraphAnalysisLimits::DEFAULT,
            control(&cancelled),
        )
        .expect("fixture must analyze")
}

fn span_text(source: &[u8], span: repowitness_domain::ByteSpan) -> &[u8] {
    let start = usize::try_from(span.start().get()).expect("test span must fit usize");
    let end = usize::try_from(span.end().get()).expect("test span must fit usize");
    &source[start..end]
}

const GOLDEN_SITE_SEQUENCE: &[(RustGraphSiteKind, &str, RustGraphSiteEvidence)] = &[
    (
        RustGraphSiteKind::Import,
        "crate::support::{self, Item as Alias, *}",
        RustGraphSiteEvidence::DirectSyntax,
    ),
    (
        RustGraphSiteKind::Import,
        "super::Alias",
        RustGraphSiteEvidence::DirectSyntax,
    ),
    (
        RustGraphSiteKind::TestMarker,
        "test",
        RustGraphSiteEvidence::SyntaxHeuristic,
    ),
    (
        RustGraphSiteKind::TestMarker,
        "tokio::test",
        RustGraphSiteEvidence::DirectSyntax,
    ),
    (
        RustGraphSiteKind::Reference,
        "Alias",
        RustGraphSiteEvidence::SyntaxHeuristic,
    ),
    (
        RustGraphSiteKind::Reference,
        "input",
        RustGraphSiteEvidence::SyntaxHeuristic,
    ),
    (
        RustGraphSiteKind::Call,
        "helper",
        RustGraphSiteEvidence::DirectSyntax,
    ),
    (
        RustGraphSiteKind::Reference,
        "local",
        RustGraphSiteEvidence::SyntaxHeuristic,
    ),
    (
        RustGraphSiteKind::Call,
        "crate::service::run::<Alias>",
        RustGraphSiteEvidence::DirectSyntax,
    ),
    (
        RustGraphSiteKind::Call,
        "worker.run",
        RustGraphSiteEvidence::DirectSyntax,
    ),
    (
        RustGraphSiteKind::Call,
        "<Worker as Trait>::execute",
        RustGraphSiteEvidence::DirectSyntax,
    ),
    (
        RustGraphSiteKind::Reference,
        "worker",
        RustGraphSiteEvidence::SyntaxHeuristic,
    ),
    (
        RustGraphSiteKind::MacroCall,
        "trace",
        RustGraphSiteEvidence::DirectSyntax,
    ),
    (
        RustGraphSiteKind::Reference,
        "Alias",
        RustGraphSiteEvidence::SyntaxHeuristic,
    ),
    (
        RustGraphSiteKind::Reference,
        "local",
        RustGraphSiteEvidence::SyntaxHeuristic,
    ),
];

#[test]
fn profile_and_stable_spellings_are_explicit() {
    assert_eq!(RUST_GRAPH_SITE_PROFILE_VERSION, 1);
    for kind in [
        RustGraphSiteKind::Import,
        RustGraphSiteKind::Reference,
        RustGraphSiteKind::Call,
        RustGraphSiteKind::MacroCall,
        RustGraphSiteKind::TestMarker,
    ] {
        assert_eq!(
            RustGraphSiteKind::from_stable_str(kind.as_str()),
            Some(kind)
        );
    }
    assert_eq!(RustGraphSiteKind::from_stable_str("Call"), None);

    for evidence in [
        RustGraphSiteEvidence::DirectSyntax,
        RustGraphSiteEvidence::SyntaxHeuristic,
    ] {
        assert_eq!(
            RustGraphSiteEvidence::from_stable_str(evidence.as_str()),
            Some(evidence)
        );
    }
    assert_eq!(RustGraphSiteEvidence::from_stable_str("syntax"), None);
}

#[test]
fn extracts_source_ordered_exact_sites_without_resolution_claims() {
    let source = br#"
use crate::support::{self, Item as Alias, *};

mod nested {
    use super::Alias;

    #[cfg(test)]
    #[tokio::test]
    fn verifies(input: Alias) {
        let local = input;
        helper(local);
        crate::service::run::<Alias>();
        worker.run();
        <Worker as Trait>::execute(&worker);
        trace!(local);
        let _copy: Alias = local;
    }
}
"#;
    let first = analyze(source);
    let second = analyze(source);

    assert_eq!(first, second);
    assert!(!first.has_syntax_errors());
    assert!(first.visited_nodes() > first.sites().len() as u32);
    assert_eq!(
        first
            .sites()
            .iter()
            .map(|site| (site.kind(), site.raw_target(), site.evidence()))
            .collect::<Vec<_>>(),
        GOLDEN_SITE_SEQUENCE
    );

    for (expected_ordinal, site) in first.sites().iter().enumerate() {
        assert_eq!(
            site.ordinal().get(),
            u32::try_from(expected_ordinal).expect("fixture site count fits u32")
        );
        assert_eq!(
            span_text(source, site.target_span()),
            site.raw_target().as_bytes()
        );
        assert!(
            site.occurrence_span().start() <= site.target_span().start()
                && site.target_span().end() <= site.occurrence_span().end()
        );
    }
}

#[test]
fn enclosing_descriptors_use_existing_definition_kinds_and_exact_spans() {
    let source = br#"
mod outer {
    struct Worker;

    impl Worker {
        fn run(&self) {
            helper();
        }
    }
}
"#;
    let analysis = analyze(source);
    let helper_call = analysis
        .sites()
        .iter()
        .find(|site| site.kind() == RustGraphSiteKind::Call)
        .expect("fixture has one call");
    let enclosing = helper_call
        .enclosing_definition()
        .expect("call is enclosed by a method");

    assert_eq!(enclosing.kind(), RustSymbolKind::Method);
    assert_eq!(enclosing.name(), "run");
    assert_eq!(enclosing.qualified_name(), "outer::Worker::run");
    assert_eq!(span_text(source, enclosing.name_span()), b"run");
    assert!(
        enclosing.declaration_span().start() <= helper_call.occurrence_span().start()
            && helper_call.occurrence_span().end() <= enclosing.declaration_span().end()
    );
}

#[test]
fn test_attributes_attach_to_the_following_definition() {
    let source = br#"
#[cfg_attr(test, allow(dead_code))]
#[test]
fn verifies() {}
"#;
    let analysis = analyze(source);
    assert_eq!(analysis.sites().len(), 2);
    for marker in analysis.sites() {
        assert_eq!(marker.kind(), RustGraphSiteKind::TestMarker);
        let enclosing = marker
            .enclosing_definition()
            .expect("outer attributes attach to the following function");
        assert_eq!(enclosing.kind(), RustSymbolKind::Function);
        assert_eq!(enclosing.name(), "verifies");
    }
}

#[test]
fn debug_output_redacts_source_spellings_and_deadlines() {
    let source = b"fn sensitive_name() { secret_target(); }";
    let analysis = analyze(source);
    let rendered = format!("{analysis:?}");

    assert!(!rendered.contains("sensitive_name"));
    assert!(!rendered.contains("secret_target"));
    assert!(rendered.contains("<redacted>"));

    let cancelled = AtomicBool::new(false);
    let control = control(&cancelled);
    let rendered_control = format!("{control:?}");
    assert!(rendered_control.contains("<monotonic>"));
}

#[test]
fn empty_traversal_finish_still_honors_the_absolute_deadline() {
    let active = AtomicBool::new(false);
    let elapsed = RustGraphAnalysisControl::new(&active, Instant::now());

    assert_eq!(
        TraversalState::default().finish(b"", elapsed),
        Err(super::RustGraphAnalysisError::DeadlineExceeded)
    );
}

#[test]
fn persistence_boundary_reconstruction_round_trips_complete_analysis() {
    let original = analyze(b"fn run() { helper(); }");
    let reconstructed = RustGraphSiteAnalysis::try_from_parts(
        original.sites().to_vec(),
        original.visited_nodes(),
        original.syntax_error_nodes(),
        original.max_observed_depth(),
        original.owned_text_bytes(),
        RustGraphAnalysisLimits::DEFAULT,
    )
    .expect("valid persisted parts reconstruct");

    assert_eq!(reconstructed, original);
}

#[test]
fn persistence_boundary_reconstruction_honors_cancellation_and_deadline() {
    let original = analyze(b"fn run() { helper(); }");
    let reconstruct = |control| {
        RustGraphSiteAnalysis::try_from_parts_with_control(
            original.sites().to_vec(),
            original.visited_nodes(),
            original.syntax_error_nodes(),
            original.max_observed_depth(),
            original.owned_text_bytes(),
            RustGraphAnalysisLimits::DEFAULT,
            control,
        )
    };
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        reconstruct(control(&cancelled)),
        Err(RustGraphAnalysisError::Cancelled)
    );
    let active = AtomicBool::new(false);
    assert_eq!(
        reconstruct(RustGraphAnalysisControl::new(&active, Instant::now())),
        Err(RustGraphAnalysisError::DeadlineExceeded)
    );
}

#[test]
fn persistence_boundary_rejects_inconsistent_spans_ordinals_and_metadata() {
    let limits = RustGraphAnalysisLimits::DEFAULT;
    let declaration_span =
        ByteSpan::try_new(ByteOffset::new(0), ByteOffset::new(12)).expect("valid span");
    let name_span = ByteSpan::try_new(ByteOffset::new(3), ByteOffset::new(6)).expect("valid span");
    let enclosing = RustGraphEnclosingDefinition::try_new(
        RustSymbolKind::Function,
        "run".to_owned(),
        "run".to_owned(),
        name_span,
        declaration_span,
        limits,
    )
    .expect("valid descriptor");
    let occurrence_span =
        ByteSpan::try_new(ByteOffset::new(7), ByteOffset::new(11)).expect("valid span");
    let target_span =
        ByteSpan::try_new(ByteOffset::new(7), ByteOffset::new(10)).expect("valid span");
    let site = RustGraphSite::try_new(
        RustGraphSiteOrdinal::new(1),
        RustGraphSiteKind::Call,
        RustGraphSiteEvidence::DirectSyntax,
        occurrence_span,
        target_span,
        "run".to_owned(),
        Some(enclosing),
        limits,
    )
    .expect("valid site");

    assert_eq!(
        RustGraphSiteAnalysis::try_from_parts(vec![site], 2, 0, 1, 9, limits),
        Err(RustGraphAnalysisError::InvalidAnalysisShape)
    );
    assert_eq!(
        RustGraphSite::try_new(
            RustGraphSiteOrdinal::new(0),
            RustGraphSiteKind::Call,
            RustGraphSiteEvidence::DirectSyntax,
            occurrence_span,
            target_span,
            "longer".to_owned(),
            None,
            limits,
        ),
        Err(RustGraphAnalysisError::InvalidSourceSpan)
    );
}
