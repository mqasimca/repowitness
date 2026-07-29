use std::{collections::BTreeSet, sync::atomic::AtomicBool, time::Instant};

use super::{analyze, control, span_text};
use crate::{
    RustGraphAnalysisControl, RustGraphAnalysisError, RustGraphAnalysisLimits,
    RustGraphSiteAnalyzer, RustGraphSiteKind,
};

fn limits(
    source: u64,
    nodes: u32,
    depth: u16,
    sites: u32,
    name: u16,
    path: u16,
) -> RustGraphAnalysisLimits {
    limits_with_owned(source, nodes, depth, sites, name, path, 64 * 1024 * 1024)
}

fn limits_with_owned(
    source: u64,
    nodes: u32,
    depth: u16,
    sites: u32,
    name: u16,
    path: u16,
    owned_text: u64,
) -> RustGraphAnalysisLimits {
    RustGraphAnalysisLimits::try_new(source, nodes, depth, sites, name, path, owned_text)
        .expect("fixture limits must be valid")
}

#[test]
fn bindings_definitions_attributes_and_macro_tokens_are_not_references() {
    let source = br#"
#[allow(dead_code)]
fn target(parameter: TypeName) {
    let binding = parameter;
    let TypeName { field: destructured } = binding;
    log!(binding, fake_call(), TypeName);
    target(binding);
}
"#;
    let analysis = analyze(source);
    let raw = analysis
        .sites()
        .iter()
        .map(|site| (site.kind(), site.raw_target()))
        .collect::<Vec<_>>();

    assert!(!raw.contains(&(RustGraphSiteKind::Reference, "target")));
    assert_eq!(
        raw.iter()
            .filter(|site| **site == (RustGraphSiteKind::Reference, "parameter"))
            .count(),
        1
    );
    assert_eq!(
        raw.iter()
            .filter(|site| **site == (RustGraphSiteKind::Reference, "binding"))
            .count(),
        2
    );
    assert!(raw.contains(&(RustGraphSiteKind::Reference, "TypeName")));
    assert!(raw.contains(&(RustGraphSiteKind::MacroCall, "log")));
    assert!(!raw.iter().any(|(_, target)| *target == "fake_call"));
    assert_eq!(
        raw.iter()
            .filter(|(kind, target)| *kind == RustGraphSiteKind::Call && *target == "target")
            .count(),
        1
    );
}

#[test]
fn generic_bounds_and_defaults_are_references_but_declarations_are_not() {
    let source = br#"
extern crate alloc as heap;

enum State {
    Ready,
    Payload(ExternalType),
    Tagged = TAG,
}

fn constrained<T: Display + Clone = DefaultType, const N: usize = DEFAULT>() {}
"#;
    let analysis = analyze(source);
    let references = analysis
        .sites()
        .iter()
        .filter(|site| site.kind() == RustGraphSiteKind::Reference)
        .map(|site| site.raw_target())
        .collect::<Vec<_>>();

    assert_eq!(
        references,
        [
            "ExternalType",
            "TAG",
            "Display",
            "Clone",
            "DefaultType",
            "DEFAULT"
        ]
    );
}

#[test]
fn pattern_constructors_ranges_and_guards_remain_references_without_bindings() {
    let source = br#"
fn classify(value: Input, context: Context) {
    match value {
        model::State::Payload(inner) if context.ready => consume(inner),
        model::State::Point { x } => consume(x),
        MIN..=MAX => {}
        _ => {}
    }
}
"#;
    let analysis = analyze(source);
    let references = analysis
        .sites()
        .iter()
        .filter(|site| site.kind() == RustGraphSiteKind::Reference)
        .map(|site| site.raw_target())
        .collect::<Vec<_>>();

    for expected in [
        "model::State::Payload",
        "context.ready",
        "inner",
        "model::State::Point",
        "x",
        "MIN",
        "MAX",
    ] {
        assert!(
            references.contains(&expected),
            "missing reference {expected:?}: {references:?}"
        );
    }
    assert_eq!(
        references
            .iter()
            .filter(|target| **target == "inner")
            .count(),
        1,
        "the tuple-pattern binding itself must not be emitted"
    );
    assert_eq!(
        references.iter().filter(|target| **target == "x").count(),
        1,
        "the struct-pattern binding itself must not be emitted"
    );
}

#[test]
fn bare_let_conditions_and_closure_parameters_are_not_references() {
    let source = br#"
fn inspect(value: Input) {
    if let binding = value {
        consume(binding);
    }
    let closure = |argument| consume(argument);
    closure(value);
}
"#;
    let analysis = analyze(source);
    let references = analysis
        .sites()
        .iter()
        .filter(|site| site.kind() == RustGraphSiteKind::Reference)
        .map(|site| site.raw_target())
        .collect::<Vec<_>>();

    assert_eq!(
        references
            .iter()
            .filter(|target| **target == "binding")
            .count(),
        1,
        "the let-condition binding itself must not be emitted"
    );
    assert_eq!(
        references
            .iter()
            .filter(|target| **target == "argument")
            .count(),
        1,
        "the closure parameter itself must not be emitted"
    );
}

#[test]
fn shorthand_field_values_are_references_but_pattern_bindings_are_not() {
    let source = br#"
fn copy(input: Item) -> Item {
    let Item { value } = input;
    Item { value }
}
"#;
    let analysis = analyze(source);
    let references = analysis
        .sites()
        .iter()
        .filter(|site| site.kind() == RustGraphSiteKind::Reference)
        .map(|site| site.raw_target())
        .collect::<Vec<_>>();

    assert_eq!(
        references
            .iter()
            .filter(|target| **target == "value")
            .count(),
        1,
        "only the shorthand initializer value is a reference"
    );
}

#[test]
fn shadowing_and_duplicate_names_remain_raw_unresolved_occurrences() {
    let source = br#"
fn same() {}
mod nested {
    fn same() {}
    fn caller() {
        let same = || {};
        same();
        super::same();
    }
}
"#;
    let analysis = analyze(source);
    let calls = analysis
        .sites()
        .iter()
        .filter(|site| site.kind() == RustGraphSiteKind::Call)
        .map(|site| site.raw_target())
        .collect::<Vec<_>>();

    assert_eq!(calls, ["same", "super::same"]);
}

#[test]
fn trait_ufcs_method_and_function_value_calls_stay_syntactic() {
    let source = br#"
fn run<T: Trait>(value: &T, callback: fn()) {
    value.execute();
    <T as Trait>::execute(value);
    callback();
}
"#;
    let analysis = analyze(source);
    let calls = analysis
        .sites()
        .iter()
        .filter(|site| site.kind() == RustGraphSiteKind::Call)
        .map(|site| site.raw_target())
        .collect::<Vec<_>>();

    assert_eq!(
        calls,
        ["value.execute", "<T as Trait>::execute", "callback"]
    );
}

#[test]
fn malformed_syntax_is_visible_but_output_remains_deterministic() {
    let source = b"fn valid() { helper(); } fn broken( { other(";
    let first = analyze(source);
    let second = analyze(source);

    assert_eq!(first, second);
    assert!(first.has_syntax_errors());
    assert!(first.syntax_error_nodes() > 0);
    assert!(
        first.sites().iter().any(|site| {
            site.kind() == RustGraphSiteKind::Call && site.raw_target() == "helper"
        })
    );
}

#[test]
fn cfg_test_markers_use_identifier_tokens_not_string_contents() {
    let source = br#"
#[cfg(feature = "test")]
fn feature_named_test() {}

#[cfg(test)]
fn actual_test_configuration() {}

#[cfg(test_feature)]
fn similarly_named_configuration() {}
"#;
    let analysis = analyze(source);
    let markers = analysis
        .sites()
        .iter()
        .filter(|site| site.kind() == RustGraphSiteKind::TestMarker)
        .collect::<Vec<_>>();

    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].raw_target(), "test");
    assert_eq!(
        markers[0]
            .enclosing_definition()
            .expect("marker attaches to its function")
            .name(),
        "actual_test_configuration"
    );
}

#[test]
fn invalid_utf8_fails_closed_with_a_redacted_error() {
    let source = b"fn valid() {}\n// \xff";
    let cancelled = AtomicBool::new(false);
    let error = RustGraphSiteAnalyzer::new()
        .expect("Rust grammar must load")
        .analyze(
            source,
            RustGraphAnalysisLimits::DEFAULT,
            control(&cancelled),
        )
        .expect_err("invalid UTF-8 must fail");

    assert_eq!(error, RustGraphAnalysisError::InvalidSourceEncoding);
    assert_eq!(error.to_string(), "Rust graph source encoding is invalid");
    assert_eq!(format!("{error:?}"), "InvalidSourceEncoding");
}

#[test]
fn cancellation_and_deadline_return_no_analysis_and_analyzer_is_reusable() {
    let source = b"fn f() { target(); }";
    let mut analyzer = RustGraphSiteAnalyzer::new().expect("Rust grammar must load");
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        analyzer.analyze(
            source,
            RustGraphAnalysisLimits::DEFAULT,
            control(&cancelled)
        ),
        Err(RustGraphAnalysisError::Cancelled)
    );

    let active = AtomicBool::new(false);
    let elapsed = RustGraphAnalysisControl::new(&active, Instant::now());
    assert_eq!(
        analyzer.analyze(source, RustGraphAnalysisLimits::DEFAULT, elapsed),
        Err(RustGraphAnalysisError::DeadlineExceeded)
    );

    let recovered = analyzer
        .analyze(source, RustGraphAnalysisLimits::DEFAULT, control(&active))
        .expect("reused analyzer must recover");
    assert_eq!(recovered.sites().len(), 1);
}

#[test]
fn source_node_depth_and_site_limits_are_inclusive() {
    let source = b"fn f() { alpha(); beta(); }";
    let baseline = analyze(source);
    let cancelled = AtomicBool::new(false);
    let mut analyzer = RustGraphSiteAnalyzer::new().expect("Rust grammar must load");
    let source_len = u64::try_from(source.len()).expect("fixture length fits u64");
    let site_count = u32::try_from(baseline.sites().len()).expect("fixture count fits u32");

    let exact = limits(
        source_len,
        baseline.visited_nodes(),
        baseline.max_observed_depth(),
        site_count,
        64,
        64,
    );
    assert!(analyzer.analyze(source, exact, control(&cancelled)).is_ok());

    let source_over = limits(source_len - 1, 1_000, 100, 100, 64, 64);
    assert_eq!(
        analyzer.analyze(source, source_over, control(&cancelled)),
        Err(RustGraphAnalysisError::SourceLimitExceeded)
    );
    let nodes_over = limits(source_len, baseline.visited_nodes() - 1, 100, 100, 64, 64);
    assert_eq!(
        analyzer.analyze(source, nodes_over, control(&cancelled)),
        Err(RustGraphAnalysisError::NodeLimitExceeded)
    );
    let depth_over = limits(
        source_len,
        1_000,
        baseline.max_observed_depth() - 1,
        100,
        64,
        64,
    );
    assert_eq!(
        analyzer.analyze(source, depth_over, control(&cancelled)),
        Err(RustGraphAnalysisError::DepthLimitExceeded)
    );
    let sites_over = limits(source_len, 1_000, 100, site_count - 1, 64, 64);
    assert_eq!(
        analyzer.analyze(source, sites_over, control(&cancelled)),
        Err(RustGraphAnalysisError::SiteLimitExceeded)
    );
}

#[test]
fn name_and_path_limits_are_inclusive() {
    let name_source = b"fn name() { call(); }";
    let container_source = b"mod outer { fn f() { call(); } }";
    let path_source = b"fn f() { abc::def(); }";
    let cancelled = AtomicBool::new(false);
    let mut analyzer = RustGraphSiteAnalyzer::new().expect("Rust grammar must load");

    assert!(
        analyzer
            .analyze(
                name_source,
                limits(1_024, 1_000, 100, 100, 4, 64),
                control(&cancelled)
            )
            .is_ok()
    );
    assert_eq!(
        analyzer.analyze(
            name_source,
            limits(1_024, 1_000, 100, 100, 3, 64),
            control(&cancelled)
        ),
        Err(RustGraphAnalysisError::NameLimitExceeded)
    );
    assert!(
        analyzer
            .analyze(
                container_source,
                limits(1_024, 1_000, 100, 100, 5, 64),
                control(&cancelled)
            )
            .is_ok()
    );
    assert_eq!(
        analyzer.analyze(
            container_source,
            limits(1_024, 1_000, 100, 100, 4, 64),
            control(&cancelled)
        ),
        Err(RustGraphAnalysisError::NameLimitExceeded)
    );

    assert!(
        analyzer
            .analyze(
                path_source,
                limits(1_024, 1_000, 100, 100, 64, 8),
                control(&cancelled)
            )
            .is_ok()
    );
    assert_eq!(
        analyzer.analyze(
            path_source,
            limits(1_024, 1_000, 100, 100, 64, 7),
            control(&cancelled)
        ),
        Err(RustGraphAnalysisError::PathLimitExceeded)
    );
}

#[test]
fn aggregate_owned_text_limit_counts_repeated_enclosing_descriptors() {
    let source = br#"
mod lengthy_container {
    fn repeated_definition_name() {
        alpha();
        beta();
        gamma();
        delta();
    }
}
"#;
    let baseline = analyze(source);
    let owned = baseline.owned_text_bytes();
    let independently_unique_target_bytes = baseline
        .sites()
        .iter()
        .map(|site| u64::try_from(site.raw_target().len()).expect("fixture text fits u64"))
        .sum::<u64>();
    assert!(
        owned > independently_unique_target_bytes,
        "repeated enclosing names and paths must consume the aggregate budget"
    );

    let cancelled = AtomicBool::new(false);
    let mut analyzer = RustGraphSiteAnalyzer::new().expect("Rust grammar must load");
    let source_len = u64::try_from(source.len()).expect("fixture source fits u64");
    let exact = limits_with_owned(source_len, 1_000, 100, 100, 100, 100, owned);
    let exact_analysis = analyzer
        .analyze(source, exact, control(&cancelled))
        .expect("the aggregate limit is inclusive");
    assert_eq!(exact_analysis.owned_text_bytes(), owned);

    let one_over = limits_with_owned(source_len, 1_000, 100, 100, 100, 100, owned - 1);
    assert_eq!(
        analyzer.analyze(source, one_over, control(&cancelled)),
        Err(RustGraphAnalysisError::OwnedTextLimitExceeded)
    );
}

#[test]
fn invalid_and_over_ceiling_limits_fail_closed() {
    let invalid = [
        RustGraphAnalysisLimits::try_new(0, 1, 1, 1, 1, 1, 1),
        RustGraphAnalysisLimits::try_new(1, 0, 1, 1, 1, 1, 1),
        RustGraphAnalysisLimits::try_new(1, 1, 0, 1, 1, 1, 1),
        RustGraphAnalysisLimits::try_new(1, 1, 1, 0, 1, 1, 1),
        RustGraphAnalysisLimits::try_new(1, 1, 1, 1, 0, 1, 1),
        RustGraphAnalysisLimits::try_new(1, 1, 1, 1, 1, 0, 1),
        RustGraphAnalysisLimits::try_new(1, 1, 1, 1, 1, 1, 0),
        RustGraphAnalysisLimits::try_new(8 * 1024 * 1024 + 1, 1, 1, 1, 1, 1, 1),
        RustGraphAnalysisLimits::try_new(1, 1_000_001, 1, 1, 1, 1, 1),
        RustGraphAnalysisLimits::try_new(1, 1, 257, 1, 1, 1, 1),
        RustGraphAnalysisLimits::try_new(1, 1, 1, 250_001, 1, 1, 1),
        RustGraphAnalysisLimits::try_new(1, 1, 1, 1, 1_025, 1, 1),
        RustGraphAnalysisLimits::try_new(1, 1, 1, 1, 1, 16_385, 1),
        RustGraphAnalysisLimits::try_new(1, 1, 1, 1, 1, 1, 64 * 1024 * 1024 + 1),
    ];
    assert!(
        invalid
            .iter()
            .all(|result| *result == Err(RustGraphAnalysisError::InvalidLimits))
    );
}

#[test]
fn every_site_has_unique_exact_identity_and_source_order() {
    let source = b"use crate::{a, b}; fn f(x: A) { a(x); b(x); m!(x); }";
    let analysis = analyze(source);
    let mut identities = BTreeSet::new();
    let mut prior_start = 0;

    for site in analysis.sites() {
        assert!(identities.insert((
            site.ordinal().get(),
            site.kind(),
            site.occurrence_span().start().get(),
            site.occurrence_span().end().get(),
            site.target_span().start().get(),
            site.target_span().end().get(),
        )));
        assert!(site.occurrence_span().start().get() >= prior_start);
        prior_start = site.occurrence_span().start().get();
        assert_eq!(
            span_text(source, site.target_span()),
            site.raw_target().as_bytes()
        );
    }
}
