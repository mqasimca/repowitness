use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

use repowitness_domain::{ByteOffset, ByteSpan, DeclarationDigest};

use super::{
    RustAnalysisControl, RustAnalysisError, RustAnalysisLimits, RustSourceAnalysis,
    RustSourceAnalyzer, RustSymbolFact, RustSymbolKind, TREE_SITTER_RUNTIME_VERSION,
    TREE_SITTER_RUST_GRAMMAR_VERSION,
};
use crate::RustOccurrenceFingerprint;

#[test]
fn producer_version_labels_match_the_pinned_workspace_dependencies() {
    let manifest = include_str!("../../../../Cargo.toml");
    assert!(manifest.contains(&format!(
        "tree-sitter = {{ version = \"={TREE_SITTER_RUNTIME_VERSION}\""
    )));
    assert!(manifest.contains(&format!(
        "tree-sitter-rust = {{ version = \"={TREE_SITTER_RUST_GRAMMAR_VERSION}\""
    )));
}

fn control(cancelled: &AtomicBool) -> RustAnalysisControl<'_> {
    RustAnalysisControl::new(
        cancelled,
        Instant::now()
            .checked_add(Duration::from_secs(5))
            .expect("short test deadline must be representable"),
    )
}

#[test]
fn extracts_deterministic_qualified_rust_symbols_and_spans() {
    let source = br#"
mod protocol {
    pub struct Frame;

    impl Frame {
        pub fn check(&self) {}
    }

    pub trait Decode {
        fn decode(&self);
    }
}

fn main() {}
"#;
    let cancelled = AtomicBool::new(false);
    let mut analyzer = RustSourceAnalyzer::new().expect("Rust grammar must load");
    let first = analyzer
        .analyze(source, RustAnalysisLimits::DEFAULT, control(&cancelled))
        .expect("valid Rust must analyze");
    let second = analyzer
        .analyze(source, RustAnalysisLimits::DEFAULT, control(&cancelled))
        .expect("reused parser must remain deterministic");

    assert_eq!(first, second);
    assert_eq!(
        first
            .facts()
            .iter()
            .map(|fact| (fact.kind(), fact.qualified_name()))
            .collect::<Vec<_>>(),
        [
            (RustSymbolKind::Module, "protocol"),
            (RustSymbolKind::Struct, "protocol::Frame"),
            (RustSymbolKind::Method, "protocol::Frame::check"),
            (RustSymbolKind::Trait, "protocol::Decode"),
            (RustSymbolKind::Method, "protocol::Decode::decode"),
            (RustSymbolKind::Function, "main"),
        ]
    );
    assert!(!first.has_syntax_errors());
    assert!(first.visited_nodes() > first.facts().len() as u32);
    for fact in first.facts() {
        let span = fact.name_span();
        let start = usize::try_from(span.start().get()).expect("test span fits usize");
        let end = usize::try_from(span.end().get()).expect("test span fits usize");
        assert_eq!(&source[start..end], fact.name().as_bytes());
        assert!(fact.declaration_span().len().get() >= span.len().get());
        assert!(fact.correspondence().is_some());
    }
}

#[test]
fn syntax_errors_remain_explicit_without_hiding_valid_facts() {
    let source = b"struct Good; fn broken( {";
    let cancelled = AtomicBool::new(false);
    let mut analyzer = RustSourceAnalyzer::new().expect("Rust grammar must load");
    let analysis = analyzer
        .analyze(source, RustAnalysisLimits::DEFAULT, control(&cancelled))
        .expect("Tree-sitter must return bounded partial syntax");

    assert!(analysis.has_syntax_errors());
    assert!(analysis.syntax_error_nodes() > 0);
    assert_eq!(analysis.facts()[0].qualified_name(), "Good");
}

#[test]
fn nested_functions_are_not_misattributed_as_methods() {
    let source = br#"
mod scoped {
    struct Item;

    impl Item {
        fn outer() {
            fn helper() {}
        }
    }

    fn top() {
        fn nested() {}
    }
}
"#;
    let cancelled = AtomicBool::new(false);
    let analysis = RustSourceAnalyzer::new()
        .expect("Rust grammar must load")
        .analyze(source, RustAnalysisLimits::DEFAULT, control(&cancelled))
        .expect("valid nested functions must analyze");
    let functions = analysis
        .facts()
        .iter()
        .filter(|fact| matches!(fact.name(), "outer" | "helper" | "top" | "nested"))
        .map(|fact| (fact.kind(), fact.qualified_name()))
        .collect::<Vec<_>>();

    assert_eq!(
        functions,
        [
            (RustSymbolKind::Method, "scoped::Item::outer"),
            (RustSymbolKind::Function, "scoped::Item::outer::helper"),
            (RustSymbolKind::Function, "scoped::top"),
            (RustSymbolKind::Function, "scoped::top::nested"),
        ]
    );
}

#[test]
fn cancellation_deadline_and_resource_limits_return_no_partial_output() {
    let source = b"struct One; struct Two;";
    let mut analyzer = RustSourceAnalyzer::new().expect("Rust grammar must load");

    let cancelled = AtomicBool::new(true);
    assert_eq!(
        analyzer.analyze(source, RustAnalysisLimits::DEFAULT, control(&cancelled)),
        Err(RustAnalysisError::Cancelled)
    );

    let not_cancelled = AtomicBool::new(false);
    let elapsed = RustAnalysisControl::new(&not_cancelled, Instant::now());
    assert_eq!(
        analyzer.analyze(source, RustAnalysisLimits::DEFAULT, elapsed),
        Err(RustAnalysisError::DeadlineExceeded)
    );

    let source_limited =
        RustAnalysisLimits::try_new(1, 100, 20, 10, 100, 200).expect("test limits are valid");
    assert_eq!(
        analyzer.analyze(source, source_limited, control(&not_cancelled)),
        Err(RustAnalysisError::SourceLimitExceeded)
    );

    let fact_limited =
        RustAnalysisLimits::try_new(1_024, 100, 20, 1, 100, 200).expect("test limits are valid");
    assert_eq!(
        analyzer.analyze(source, fact_limited, control(&not_cancelled)),
        Err(RustAnalysisError::FactLimitExceeded)
    );

    let node_limited =
        RustAnalysisLimits::try_new(1_024, 1, 20, 10, 100, 200).expect("test limits are valid");
    assert_eq!(
        analyzer.analyze(source, node_limited, control(&not_cancelled)),
        Err(RustAnalysisError::NodeLimitExceeded)
    );

    let depth_limited =
        RustAnalysisLimits::try_new(1_024, 100, 1, 10, 100, 200).expect("test limits are valid");
    assert_eq!(
        analyzer.analyze(
            b"mod outer { struct Inner; }",
            depth_limited,
            control(&not_cancelled)
        ),
        Err(RustAnalysisError::DepthLimitExceeded)
    );

    let qualified_limited =
        RustAnalysisLimits::try_new(1_024, 100, 20, 10, 100, 3).expect("test limits are valid");
    assert_eq!(
        analyzer.analyze(
            b"mod a { struct B; }",
            qualified_limited,
            control(&not_cancelled)
        ),
        Err(RustAnalysisError::QualifiedNameLimitExceeded)
    );
}

#[test]
fn invalid_limits_and_names_have_stable_redacted_diagnostics() {
    assert_eq!(
        RustAnalysisLimits::try_new(0, 1, 1, 1, 1, 1),
        Err(RustAnalysisError::InvalidLimits)
    );

    let cancelled = AtomicBool::new(false);
    let mut analyzer = RustSourceAnalyzer::new().expect("Rust grammar must load");
    let limits =
        RustAnalysisLimits::try_new(1_024, 100, 20, 10, 3, 10).expect("test limits are valid");
    let error = analyzer
        .analyze(b"struct Longer;", limits, control(&cancelled))
        .expect_err("the name must exceed its bound");
    assert_eq!(error, RustAnalysisError::NameLimitExceeded);
    assert_eq!(error.to_string(), "symbol name limit exceeded");
    assert!(!format!("{error:?}").contains("Longer"));
}

#[test]
fn persisted_analysis_construction_and_source_validation_fail_closed() {
    let kinds = [
        RustSymbolKind::Function,
        RustSymbolKind::Method,
        RustSymbolKind::Struct,
        RustSymbolKind::Enum,
        RustSymbolKind::Union,
        RustSymbolKind::Trait,
        RustSymbolKind::Module,
        RustSymbolKind::TypeAlias,
        RustSymbolKind::Constant,
        RustSymbolKind::Static,
        RustSymbolKind::Macro,
    ];
    for kind in kinds {
        assert_eq!(RustSymbolKind::from_stable_str(kind.as_str()), Some(kind));
    }
    assert_eq!(RustSymbolKind::from_stable_str("Function"), None);

    let span = ByteSpan::try_new(ByteOffset::new(3), ByteOffset::new(8))
        .expect("fixture span must be valid");
    let declaration = ByteSpan::try_new(ByteOffset::new(0), ByteOffset::new(13))
        .expect("fixture declaration must be valid");
    let fact = RustSymbolFact::try_new(
        RustSymbolKind::Function,
        "alpha".to_owned(),
        "alpha".to_owned(),
        span,
        declaration,
        RustAnalysisLimits::DEFAULT,
    )
    .expect("fixture fact must be structurally valid");
    let analysis =
        RustSourceAnalysis::try_from_parts(vec![fact], 5, 0, RustAnalysisLimits::DEFAULT)
            .expect("fixture output must be structurally valid");
    assert!(
        analysis
            .validate_for_reuse(b"fn alpha() {}", RustAnalysisLimits::DEFAULT)
            .is_ok()
    );
    assert_eq!(
        analysis.validate_for_reuse(b"fn other() {}", RustAnalysisLimits::DEFAULT),
        Err(RustAnalysisError::InvalidAnalysisArtifact)
    );
    assert_eq!(
        RustSourceAnalysis::try_from_parts(Vec::new(), 0, 0, RustAnalysisLimits::DEFAULT),
        Err(RustAnalysisError::InvalidAnalysisArtifact)
    );
    let narrower = RustAnalysisLimits::try_new(1024, 100, 20, 10, 1, 1)
        .expect("narrow fixture limits must be valid");
    assert_eq!(
        RustSourceAnalysis::try_from_parts(vec![analysis.facts()[0].clone()], 5, 0, narrower,),
        Err(RustAnalysisError::InvalidAnalysisArtifact)
    );

    let cancelled = AtomicBool::new(false);
    let analyzed = RustSourceAnalyzer::new()
        .expect("Rust grammar must load")
        .analyze(
            b"fn alpha() {}",
            RustAnalysisLimits::DEFAULT,
            control(&cancelled),
        )
        .expect("fixture source must analyze");
    let analyzed_fact = &analyzed.facts()[0];
    let fingerprint = analyzed_fact
        .correspondence()
        .expect("Rust extraction must include correspondence");
    let corrupted = RustOccurrenceFingerprint::new(
        DeclarationDigest::new([0xFF; 32]),
        fingerprint.name_elided(),
    );
    let corrupted_fact = RustSymbolFact::try_new_with_correspondence(
        analyzed_fact.kind(),
        analyzed_fact.name().to_owned(),
        analyzed_fact.qualified_name().to_owned(),
        analyzed_fact.name_span(),
        analyzed_fact.declaration_span(),
        corrupted,
        RustAnalysisLimits::DEFAULT,
    )
    .expect("digest values are structurally valid");
    let corrupted_analysis = RustSourceAnalysis::try_from_parts(
        vec![corrupted_fact],
        analyzed.visited_nodes(),
        analyzed.syntax_error_nodes(),
        RustAnalysisLimits::DEFAULT,
    )
    .expect("persisted structure remains valid");
    assert_eq!(
        corrupted_analysis.validate_for_reuse(b"fn alpha() {}", RustAnalysisLimits::DEFAULT),
        Err(RustAnalysisError::InvalidAnalysisArtifact)
    );
}
