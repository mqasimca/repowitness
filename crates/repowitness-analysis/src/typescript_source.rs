use std::{fmt, ops::ControlFlow};

use repowitness_domain::{ByteOffset, ByteSpan};
use tree_sitter::{Node, ParseOptions, Parser};

use crate::{
    SourceAnalysis, SourceAnalysisControl, SourceAnalysisError, SourceAnalysisLimits, SymbolFact,
    SymbolKind,
};

/// Version of the Phase 0 TypeScript/TSX extraction behavior.
pub const TYPESCRIPT_ANALYSIS_PROFILE_VERSION: u32 = 2;
/// Pinned Tree-sitter TypeScript grammar package version.
pub const TREE_SITTER_TYPESCRIPT_GRAMMAR_VERSION: &str = "0.23.2";

/// Exact TypeScript grammar dialect selected from the repository extension.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TypeScriptDialect {
    /// Plain TypeScript selected by an exact `.ts` extension.
    TypeScript,
    /// JSX-aware TypeScript selected by an exact `.tsx` extension.
    Tsx,
}

/// Returns exact first-party TypeScript analyzer bytes for producer fingerprinting.
#[must_use]
pub fn typescript_analyzer_implementation_fingerprint_input() -> &'static [u8] {
    include_bytes!("typescript_source.rs")
}

/// Returns the pinned dialect grammar node schema for producer fingerprinting.
#[must_use]
pub fn typescript_grammar_fingerprint_input(dialect: TypeScriptDialect) -> &'static [u8] {
    match dialect {
        TypeScriptDialect::TypeScript => tree_sitter_typescript::TYPESCRIPT_NODE_TYPES.as_bytes(),
        TypeScriptDialect::Tsx => tree_sitter_typescript::TSX_NODE_TYPES.as_bytes(),
    }
}

/// Reusable owner of one Tree-sitter TypeScript or TSX parser.
pub struct TypeScriptSourceAnalyzer {
    parser: Parser,
    dialect: TypeScriptDialect,
}

impl TypeScriptSourceAnalyzer {
    /// Creates an analyzer using the pinned grammar for `dialect`.
    pub fn new(dialect: TypeScriptDialect) -> Result<Self, SourceAnalysisError> {
        let mut parser = Parser::new();
        let language = match dialect {
            TypeScriptDialect::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            TypeScriptDialect::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        };
        parser
            .set_language(&language)
            .map_err(|_| SourceAnalysisError::GrammarUnavailable)?;
        Ok(Self { parser, dialect })
    }

    /// Analyzes immutable TypeScript or TSX bytes without filesystem or database I/O.
    pub fn analyze(
        &mut self,
        source: &[u8],
        limits: SourceAnalysisLimits,
        control: SourceAnalysisControl<'_>,
    ) -> Result<SourceAnalysis, SourceAnalysisError> {
        if let Some(outcome) = control.outcome() {
            return Err(outcome);
        }
        let source_bytes =
            u64::try_from(source.len()).map_err(|_| SourceAnalysisError::SourceLimitExceeded)?;
        if source_bytes > limits.max_source_bytes() {
            return Err(SourceAnalysisError::SourceLimitExceeded);
        }

        let mut interrupted = None;
        let mut progress = |_: &tree_sitter::ParseState| {
            if let Some(outcome) = control.outcome() {
                interrupted = Some(outcome);
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        let mut read = |offset: usize, _| source.get(offset..).unwrap_or_default();
        let tree = self.parser.parse_with_options(
            &mut read,
            None,
            Some(ParseOptions::new().progress_callback(&mut progress)),
        );
        if let Some(outcome) = interrupted {
            self.parser.reset();
            return Err(outcome);
        }
        let tree = tree.ok_or(SourceAnalysisError::ParseFailed)?;
        traverse_tree(&tree, source, limits, control)
    }
}

impl fmt::Debug for TypeScriptSourceAnalyzer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TypeScriptSourceAnalyzer")
            .field("dialect", &self.dialect)
            .finish_non_exhaustive()
    }
}

fn traverse_tree(
    tree: &tree_sitter::Tree,
    source: &[u8],
    limits: SourceAnalysisLimits,
    control: SourceAnalysisControl<'_>,
) -> Result<SourceAnalysis, SourceAnalysisError> {
    let mut facts = Vec::new();
    let mut visited_nodes = 0_u32;
    let mut syntax_error_nodes = 0_u32;
    let mut depth = 0_u16;
    let mut cursor = tree.walk();

    loop {
        if let Some(outcome) = control.outcome() {
            return Err(outcome);
        }
        visited_nodes = visited_nodes
            .checked_add(1)
            .ok_or(SourceAnalysisError::NodeLimitExceeded)?;
        if visited_nodes > limits.max_syntax_nodes() {
            return Err(SourceAnalysisError::NodeLimitExceeded);
        }

        let node = cursor.node();
        if node.is_error() || node.is_missing() {
            syntax_error_nodes = syntax_error_nodes.saturating_add(1);
        }
        extract_symbol_fact(node, source, limits, &mut facts)?;

        if cursor.goto_first_child() {
            depth = depth
                .checked_add(1)
                .ok_or(SourceAnalysisError::DepthLimitExceeded)?;
            if depth > limits.max_syntax_depth() {
                return Err(SourceAnalysisError::DepthLimitExceeded);
            }
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return SourceAnalysis::try_from_parts(
                    facts,
                    visited_nodes,
                    syntax_error_nodes,
                    limits,
                );
            }
            depth = depth
                .checked_sub(1)
                .ok_or(SourceAnalysisError::InvalidSourceSpan)?;
        }
    }
}

fn extract_symbol_fact(
    node: Node<'_>,
    source: &[u8],
    limits: SourceAnalysisLimits,
    facts: &mut Vec<SymbolFact>,
) -> Result<(), SourceAnalysisError> {
    let Some(kind) = symbol_kind(node) else {
        return Ok(());
    };
    let Some(name_node) = admitted_name_node(node, kind) else {
        return Ok(());
    };
    if facts.len()
        >= usize::try_from(limits.max_symbol_facts())
            .map_err(|_| SourceAnalysisError::FactLimitExceeded)?
    {
        return Err(SourceAnalysisError::FactLimitExceeded);
    }

    let name = source_text(name_node, source)?;
    if name.len() > usize::from(limits.max_symbol_name_bytes()) {
        return Err(SourceAnalysisError::NameLimitExceeded);
    }
    let qualified_name = qualified_name(node, name, source, limits)?;
    facts.push(SymbolFact::try_new(
        kind,
        name.to_owned(),
        qualified_name,
        source_span(name_node, source)?,
        source_span(node, source)?,
        limits,
    )?);
    Ok(())
}

fn symbol_kind(node: Node<'_>) -> Option<SymbolKind> {
    match node.kind() {
        "function_declaration" | "generator_function_declaration" | "function_signature" => {
            Some(SymbolKind::Function)
        }
        "method_definition" | "method_signature" | "abstract_method_signature" => {
            Some(SymbolKind::Method)
        }
        "class_declaration" | "abstract_class_declaration" => Some(SymbolKind::Class),
        "interface_declaration" => Some(SymbolKind::Interface),
        "enum_declaration" => Some(SymbolKind::Enum),
        "type_alias_declaration" => Some(SymbolKind::TypeAlias),
        "internal_module" => Some(SymbolKind::Module),
        "variable_declarator" if is_module_level_variable(node) => Some(SymbolKind::Variable),
        _ => None,
    }
}

fn admitted_name_node<'tree>(node: Node<'tree>, kind: SymbolKind) -> Option<Node<'tree>> {
    let name = node.child_by_field_name("name")?;
    let admitted = match kind {
        SymbolKind::Method => matches!(
            name.kind(),
            "identifier" | "property_identifier" | "private_property_identifier"
        ),
        SymbolKind::Module => matches!(
            name.kind(),
            "identifier" | "type_identifier" | "nested_identifier"
        ),
        SymbolKind::Variable => name.kind() == "identifier",
        _ => matches!(name.kind(), "identifier" | "type_identifier"),
    };
    admitted.then_some(name)
}

fn is_module_level_variable(node: Node<'_>) -> bool {
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        match current.kind() {
            "program" | "internal_module" => return true,
            "lexical_declaration"
            | "variable_declaration"
            | "export_statement"
            | "ambient_declaration" => ancestor = current.parent(),
            "statement_block"
                if current
                    .parent()
                    .is_some_and(|parent| parent.kind() == "internal_module") =>
            {
                ancestor = current.parent();
            }
            _ => return false,
        }
    }
    false
}

fn qualified_name(
    node: Node<'_>,
    name: &str,
    source: &[u8],
    limits: SourceAnalysisLimits,
) -> Result<String, SourceAnalysisError> {
    let mut scopes = Vec::new();
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        if let Some(scope) = scope_name(current, source)? {
            scopes.push(scope);
        }
        ancestor = current.parent();
    }
    scopes.reverse();
    scopes.push(name);

    let required_bytes = scopes
        .iter()
        .enumerate()
        .try_fold(0_usize, |total, (index, component)| {
            total
                .checked_add(component.len())
                .and_then(|sum| sum.checked_add(usize::from(index != 0) * 2))
        })
        .ok_or(SourceAnalysisError::QualifiedNameLimitExceeded)?;
    if required_bytes > usize::from(limits.max_qualified_name_bytes()) {
        return Err(SourceAnalysisError::QualifiedNameLimitExceeded);
    }

    let mut qualified = String::with_capacity(required_bytes);
    for component in scopes {
        if !qualified.is_empty() {
            qualified.push_str("::");
        }
        qualified.push_str(component);
    }
    Ok(qualified)
}

fn scope_name<'a>(
    node: Node<'_>,
    source: &'a [u8],
) -> Result<Option<&'a str>, SourceAnalysisError> {
    if !matches!(
        node.kind(),
        "class_declaration"
            | "abstract_class_declaration"
            | "interface_declaration"
            | "internal_module"
    ) {
        return Ok(None);
    }
    let Some(name) = node.child_by_field_name("name") else {
        return Ok(None);
    };
    if !matches!(
        name.kind(),
        "identifier" | "type_identifier" | "nested_identifier"
    ) {
        return Ok(None);
    }
    source_text(name, source).map(Some)
}

fn source_text<'a>(node: Node<'_>, source: &'a [u8]) -> Result<&'a str, SourceAnalysisError> {
    let bytes = source
        .get(node.byte_range())
        .ok_or(SourceAnalysisError::InvalidSourceSpan)?;
    std::str::from_utf8(bytes).map_err(|_| SourceAnalysisError::InvalidIdentifierEncoding)
}

fn source_span(node: Node<'_>, source: &[u8]) -> Result<ByteSpan, SourceAnalysisError> {
    let range = node.byte_range();
    if range.end > source.len() {
        return Err(SourceAnalysisError::InvalidSourceSpan);
    }
    let start = u64::try_from(range.start).map_err(|_| SourceAnalysisError::InvalidSourceSpan)?;
    let end = u64::try_from(range.end).map_err(|_| SourceAnalysisError::InvalidSourceSpan)?;
    ByteSpan::try_new(ByteOffset::new(start), ByteOffset::new(end))
        .map_err(|_| SourceAnalysisError::InvalidSourceSpan)
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::{Duration, Instant},
    };

    use super::*;

    fn analyze(source: &[u8], dialect: TypeScriptDialect) -> SourceAnalysis {
        let cancelled = AtomicBool::new(false);
        TypeScriptSourceAnalyzer::new(dialect)
            .expect("TypeScript grammar must load")
            .analyze(
                source,
                SourceAnalysisLimits::default(),
                SourceAnalysisControl::new(&cancelled, Instant::now() + Duration::from_secs(5)),
            )
            .expect("fixture should analyze")
    }

    #[test]
    fn extracts_typescript_declarations_in_source_order() {
        let source = br#"namespace Api {
  export interface Client { fetch(): Promise<void> }
  export abstract class Base { abstract run(): void }
  export class Service extends Base { run(): void {} }
  export enum Mode { Fast, Safe }
  export type Identifier = string
  export function build(): Service { return new Service() }
  export function* stream(): Generator<number> { yield 1 }
  export const Component = () => new Service()
  export const { skipped } = { skipped: true }
  function outer(): void { const local = 1 }
}
declare function declared(value: string): void
"#;
        let analysis = analyze(source, TypeScriptDialect::TypeScript);
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
                (SymbolKind::Module, "Api".to_owned(), "Api".to_owned()),
                (
                    SymbolKind::Interface,
                    "Client".to_owned(),
                    "Api::Client".to_owned()
                ),
                (
                    SymbolKind::Method,
                    "fetch".to_owned(),
                    "Api::Client::fetch".to_owned()
                ),
                (SymbolKind::Class, "Base".to_owned(), "Api::Base".to_owned()),
                (
                    SymbolKind::Method,
                    "run".to_owned(),
                    "Api::Base::run".to_owned()
                ),
                (
                    SymbolKind::Class,
                    "Service".to_owned(),
                    "Api::Service".to_owned()
                ),
                (
                    SymbolKind::Method,
                    "run".to_owned(),
                    "Api::Service::run".to_owned()
                ),
                (SymbolKind::Enum, "Mode".to_owned(), "Api::Mode".to_owned()),
                (
                    SymbolKind::TypeAlias,
                    "Identifier".to_owned(),
                    "Api::Identifier".to_owned()
                ),
                (
                    SymbolKind::Function,
                    "build".to_owned(),
                    "Api::build".to_owned()
                ),
                (
                    SymbolKind::Function,
                    "stream".to_owned(),
                    "Api::stream".to_owned()
                ),
                (
                    SymbolKind::Variable,
                    "Component".to_owned(),
                    "Api::Component".to_owned()
                ),
                (
                    SymbolKind::Function,
                    "outer".to_owned(),
                    "Api::outer".to_owned()
                ),
                (
                    SymbolKind::Function,
                    "declared".to_owned(),
                    "declared".to_owned()
                ),
            ]
        );
        assert!(!analysis.has_syntax_errors());
        assert!(actual.iter().all(|(_, name, _)| name != "local"));
        assert!(actual.iter().all(|(_, name, _)| name != "skipped"));
        for fact in analysis.facts() {
            let start = usize::try_from(fact.name_span().start().get()).expect("span fits");
            let end = usize::try_from(fact.name_span().end().get()).expect("span fits");
            assert_eq!(&source[start..end], fact.name().as_bytes());
        }
    }

    #[test]
    fn tsx_uses_the_jsx_aware_grammar_and_preserves_utf8_names() {
        let source = "export const Élément = () => <section>Hello</section>\n".as_bytes();
        let analysis = analyze(source, TypeScriptDialect::Tsx);

        assert!(!analysis.has_syntax_errors());
        assert_eq!(analysis.facts().len(), 1);
        assert_eq!(analysis.facts()[0].name(), "Élément");
        assert_eq!(analysis.facts()[0].kind(), SymbolKind::Variable);
    }

    #[test]
    fn block_and_loop_bindings_are_not_misattributed_as_module_variables() {
        let analysis = analyze(
            br#"export const Direct = 1
namespace Scope {
  export const Nested = 2
  if (true) {
    const NamespaceBlock = 3
  }
}
if (true) {
  const ProgramBlock = 4
}
for (const LoopBinding of []) {
  const LoopBody = 5
}
"#,
            TypeScriptDialect::TypeScript,
        );
        let variables = analysis
            .facts()
            .iter()
            .filter(|fact| fact.kind() == SymbolKind::Variable)
            .map(|fact| (fact.name(), fact.qualified_name()))
            .collect::<Vec<_>>();

        assert_eq!(
            variables,
            [("Direct", "Direct"), ("Nested", "Scope::Nested")]
        );
        assert!(!analysis.has_syntax_errors());
    }

    #[test]
    fn dialect_schemas_are_distinct_and_malformed_syntax_is_explicit() {
        assert_ne!(
            typescript_grammar_fingerprint_input(TypeScriptDialect::TypeScript),
            typescript_grammar_fingerprint_input(TypeScriptDialect::Tsx)
        );
        let analysis = analyze(
            b"export interface Broken { value:",
            TypeScriptDialect::TypeScript,
        );
        assert!(analysis.has_syntax_errors());
        assert!(analysis.syntax_error_nodes() > 0);
    }

    #[test]
    fn cancellation_deadlines_and_bounds_fail_closed() {
        let mut analyzer =
            TypeScriptSourceAnalyzer::new(TypeScriptDialect::TypeScript).expect("grammar loads");
        let cancelled = AtomicBool::new(true);
        let result = analyzer.analyze(
            b"export const cancelled = true",
            SourceAnalysisLimits::default(),
            SourceAnalysisControl::new(&cancelled, Instant::now() + Duration::from_secs(5)),
        );
        assert_eq!(result, Err(SourceAnalysisError::Cancelled));

        cancelled.store(false, Ordering::Release);
        let result = analyzer.analyze(
            b"export const expired = true",
            SourceAnalysisLimits::default(),
            SourceAnalysisControl::new(&cancelled, Instant::now()),
        );
        assert_eq!(result, Err(SourceAnalysisError::DeadlineExceeded));

        let limits = SourceAnalysisLimits::try_new(8, 1_000, 64, 10, 32, 128)
            .expect("fixture limits should be valid");
        let result = analyzer.analyze(
            b"const tooLarge = true",
            limits,
            SourceAnalysisControl::new(&cancelled, Instant::now() + Duration::from_secs(5)),
        );
        assert_eq!(result, Err(SourceAnalysisError::SourceLimitExceeded));
    }
}
