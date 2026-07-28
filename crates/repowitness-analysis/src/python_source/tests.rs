use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use super::*;

fn analyze(source: &[u8]) -> SourceAnalysis {
    let cancelled = AtomicBool::new(false);
    PythonSourceAnalyzer::new()
        .expect("Python grammar must load")
        .analyze(
            source,
            SourceAnalysisLimits::default(),
            SourceAnalysisControl::new(&cancelled, Instant::now() + Duration::from_secs(5)),
        )
        .expect("fixture should analyze")
}

#[test]
fn extracts_python_declarations_in_source_order() {
    let source = br#"@register("client")
class Client:
    class_attribute = "skipped"

    @property
    def name(self):
        return "client"

    async def fetch(self):
        return None

    def outer(self):
        def inner():
            return None
        return inner()

module_value: int = 1
first = second = 2
type Payload = dict[str, int]
"#;
    let analysis = analyze(source);
    let actual = analysis
        .facts()
        .iter()
        .map(|fact| {
            (
                fact.kind(),
                fact.name().to_owned(),
                fact.qualified_name().to_owned(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        actual,
        vec![
            (SymbolKind::Class, "Client".to_owned(), "Client".to_owned()),
            (
                SymbolKind::Method,
                "name".to_owned(),
                "Client::name".to_owned()
            ),
            (
                SymbolKind::Method,
                "fetch".to_owned(),
                "Client::fetch".to_owned()
            ),
            (
                SymbolKind::Method,
                "outer".to_owned(),
                "Client::outer".to_owned()
            ),
            (
                SymbolKind::Function,
                "inner".to_owned(),
                "Client::outer::inner".to_owned()
            ),
            (
                SymbolKind::Variable,
                "module_value".to_owned(),
                "module_value".to_owned()
            ),
            (SymbolKind::Variable, "first".to_owned(), "first".to_owned()),
            (
                SymbolKind::Variable,
                "second".to_owned(),
                "second".to_owned()
            ),
            (
                SymbolKind::TypeAlias,
                "Payload".to_owned(),
                "Payload".to_owned()
            ),
        ]
    );
    assert!(!analysis.has_syntax_errors());
    assert!(actual.iter().all(|(_, name, _)| name != "class_attribute"));
    for fact in analysis.facts() {
        let start = usize::try_from(fact.name_span().start().get()).expect("span fits");
        let end = usize::try_from(fact.name_span().end().get()).expect("span fits");
        assert_eq!(&source[start..end], fact.name().as_bytes());
    }
}

#[test]
fn decorators_are_part_of_declaration_spans_and_utf8_names_are_exact() {
    let source =
        "@dataclass\nclass Café:\n    @property\n    def résumé(self):\n        return 1\n";
    let analysis = analyze(source.as_bytes());

    assert_eq!(analysis.facts().len(), 2);
    assert_eq!(analysis.facts()[0].name(), "Café");
    assert_eq!(analysis.facts()[1].name(), "résumé");
    assert_eq!(
        declaration_bytes(source.as_bytes(), &analysis.facts()[0]),
        &source.as_bytes()[..source.len() - 1]
    );
    assert!(declaration_bytes(source.as_bytes(), &analysis.facts()[1]).starts_with(b"@property\n"));
}

#[test]
fn malformed_python_retains_valid_facts_and_explicit_error_coverage() {
    let analysis = analyze(b"class Valid:\n    pass\n\ndef broken(:\n");

    assert_eq!(analysis.facts()[0].name(), "Valid");
    assert!(analysis.has_syntax_errors());
    assert!(analysis.syntax_error_nodes() > 0);
}

#[test]
fn cancellation_deadlines_and_bounds_fail_closed() {
    let mut analyzer = PythonSourceAnalyzer::new().expect("grammar loads");
    let cancelled = AtomicBool::new(true);
    let result = analyzer.analyze(
        b"def cancelled():\n    pass\n",
        SourceAnalysisLimits::default(),
        SourceAnalysisControl::new(&cancelled, Instant::now() + Duration::from_secs(5)),
    );
    assert_eq!(result, Err(SourceAnalysisError::Cancelled));

    cancelled.store(false, Ordering::Release);
    let result = analyzer.analyze(
        b"def expired():\n    pass\n",
        SourceAnalysisLimits::default(),
        SourceAnalysisControl::new(&cancelled, Instant::now()),
    );
    assert_eq!(result, Err(SourceAnalysisError::DeadlineExceeded));

    let source_limits = SourceAnalysisLimits::try_new(8, 1_000, 64, 10, 32, 128)
        .expect("fixture limits should be valid");
    let result = analyzer.analyze(
        b"def too_large():\n    pass\n",
        source_limits,
        SourceAnalysisControl::new(&cancelled, Instant::now() + Duration::from_secs(5)),
    );
    assert_eq!(result, Err(SourceAnalysisError::SourceLimitExceeded));

    let node_limits = SourceAnalysisLimits::try_new(1_000, 1, 64, 10, 32, 128)
        .expect("fixture limits should be valid");
    let result = analyzer.analyze(
        b"def too_many_nodes():\n    pass\n",
        node_limits,
        SourceAnalysisControl::new(&cancelled, Instant::now() + Duration::from_secs(5)),
    );
    assert_eq!(result, Err(SourceAnalysisError::NodeLimitExceeded));
}

fn declaration_bytes<'a>(source: &'a [u8], fact: &SymbolFact) -> &'a [u8] {
    let start = usize::try_from(fact.declaration_span().start().get()).expect("span fits");
    let end = usize::try_from(fact.declaration_span().end().get()).expect("span fits");
    &source[start..end]
}
