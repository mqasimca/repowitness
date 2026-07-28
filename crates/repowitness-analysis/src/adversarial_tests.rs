use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

use crate::{
    GoSourceAnalyzer, PythonSourceAnalyzer, RustSourceAnalyzer, SourceAnalysis,
    SourceAnalysisControl, SourceAnalysisError, SourceAnalysisLimits, TypeScriptDialect,
    TypeScriptSourceAnalyzer,
};

fn control(cancelled: &AtomicBool) -> SourceAnalysisControl<'_> {
    SourceAnalysisControl::new(
        cancelled,
        Instant::now()
            .checked_add(Duration::from_secs(5))
            .expect("short test deadline should be representable"),
    )
}

fn assert_invalid_identifier_is_explicit(result: Result<SourceAnalysis, SourceAnalysisError>) {
    match result {
        Ok(analysis) => {
            assert!(
                analysis.has_syntax_errors(),
                "invalid identifier bytes must not produce clean coverage"
            );
            assert!(
                analysis.facts().iter().all(|fact| fact.name() == "Good"),
                "invalid identifier bytes must not produce a symbol fact"
            );
        }
        Err(SourceAnalysisError::InvalidIdentifierEncoding) => {}
        Err(error) => panic!("unexpected invalid-encoding outcome: {error}"),
    }
}

fn assert_name_span(source: &[u8], analysis: &SourceAnalysis, name: &str) {
    let fact = analysis
        .facts()
        .iter()
        .find(|fact| fact.name() == name)
        .expect("expected Unicode symbol should be extracted");
    let start = usize::try_from(fact.name_span().start().get()).expect("span should fit usize");
    let end = usize::try_from(fact.name_span().end().get()).expect("span should fit usize");

    assert_eq!(&source[start..end], name.as_bytes());
    assert!(fact.declaration_span().start() <= fact.name_span().start());
    assert!(fact.declaration_span().end() >= fact.name_span().end());
}

#[test]
fn unicode_identifier_spans_are_exact_for_every_language_and_dialect() {
    let cancelled = AtomicBool::new(false);

    let rust = "mod café { struct Élément; impl Élément { fn méthode() {} } }\n".as_bytes();
    let rust_analysis = RustSourceAnalyzer::new()
        .expect("Rust grammar should load")
        .analyze(rust, SourceAnalysisLimits::default(), control(&cancelled))
        .expect("Unicode Rust should analyze");
    assert_name_span(rust, &rust_analysis, "café");
    assert_name_span(rust, &rust_analysis, "Élément");
    assert_name_span(rust, &rust_analysis, "méthode");

    let go = "package café\ntype Élément struct{}\nfunc (value *Élément) Méthode() {}\n".as_bytes();
    let go_analysis = GoSourceAnalyzer::new()
        .expect("Go grammar should load")
        .analyze(go, SourceAnalysisLimits::default(), control(&cancelled))
        .expect("Unicode Go should analyze");
    assert_name_span(go, &go_analysis, "Élément");
    assert_name_span(go, &go_analysis, "Méthode");

    let typescript = "namespace Café { export class Élément { méthode(): void {} } }\n".as_bytes();
    let typescript_analysis = TypeScriptSourceAnalyzer::new(TypeScriptDialect::TypeScript)
        .expect("TypeScript grammar should load")
        .analyze(
            typescript,
            SourceAnalysisLimits::default(),
            control(&cancelled),
        )
        .expect("Unicode TypeScript should analyze");
    assert_name_span(typescript, &typescript_analysis, "Café");
    assert_name_span(typescript, &typescript_analysis, "Élément");
    assert_name_span(typescript, &typescript_analysis, "méthode");

    let tsx = "export const Élément = () => <section />\n".as_bytes();
    let tsx_analysis = TypeScriptSourceAnalyzer::new(TypeScriptDialect::Tsx)
        .expect("TSX grammar should load")
        .analyze(tsx, SourceAnalysisLimits::default(), control(&cancelled))
        .expect("Unicode TSX should analyze");
    assert_name_span(tsx, &tsx_analysis, "Élément");

    let python = "class Café:\n    def résumé(self):\n        pass\n".as_bytes();
    let python_analysis = PythonSourceAnalyzer::new()
        .expect("Python grammar should load")
        .analyze(python, SourceAnalysisLimits::default(), control(&cancelled))
        .expect("Unicode Python should analyze");
    assert_name_span(python, &python_analysis, "Café");
    assert_name_span(python, &python_analysis, "résumé");
}

#[test]
fn invalid_utf8_identifier_bytes_never_become_clean_symbol_evidence() {
    let cancelled = AtomicBool::new(false);
    let limits = SourceAnalysisLimits::default();

    assert_invalid_identifier_is_explicit(
        RustSourceAnalyzer::new()
            .expect("Rust grammar should load")
            .analyze(b"fn Good() {}\nfn \xFF() {}\n", limits, control(&cancelled)),
    );
    assert_invalid_identifier_is_explicit(
        GoSourceAnalyzer::new()
            .expect("Go grammar should load")
            .analyze(
                b"package sample\nfunc Good() {}\nvar \xFF = 1\n",
                limits,
                control(&cancelled),
            ),
    );
    for dialect in [TypeScriptDialect::TypeScript, TypeScriptDialect::Tsx] {
        assert_invalid_identifier_is_explicit(
            TypeScriptSourceAnalyzer::new(dialect)
                .expect("TypeScript grammar should load")
                .analyze(
                    b"export const Good = 1;\nexport const \xFF = 2;\n",
                    limits,
                    control(&cancelled),
                ),
        );
    }
    assert_invalid_identifier_is_explicit(
        PythonSourceAnalyzer::new()
            .expect("Python grammar should load")
            .analyze(
                b"def Good():\n    pass\n\ndef \xFF():\n    pass\n",
                limits,
                control(&cancelled),
            ),
    );
}
