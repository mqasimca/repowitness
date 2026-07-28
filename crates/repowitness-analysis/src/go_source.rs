use std::{fmt, ops::ControlFlow};

use repowitness_domain::{ByteOffset, ByteSpan};
use tree_sitter::{Node, ParseOptions, Parser};

use crate::{
    SourceAnalysis, SourceAnalysisControl, SourceAnalysisError, SourceAnalysisLimits, SymbolFact,
    SymbolKind,
};

/// Version of the Phase 0 Go extraction behavior implemented by this module.
pub const GO_ANALYSIS_PROFILE_VERSION: u32 = 1;
/// Pinned Tree-sitter Go grammar package version.
pub const TREE_SITTER_GO_GRAMMAR_VERSION: &str = "0.25.0";

/// Returns exact first-party Go analyzer bytes for producer fingerprinting.
#[must_use]
pub fn go_analyzer_implementation_fingerprint_input() -> &'static [u8] {
    include_bytes!("go_source.rs")
}

/// Returns the pinned Go grammar node schema for producer fingerprinting.
#[must_use]
pub fn go_grammar_fingerprint_input() -> &'static [u8] {
    tree_sitter_go::NODE_TYPES.as_bytes()
}

/// Reusable owner of one Tree-sitter Go parser.
pub struct GoSourceAnalyzer {
    parser: Parser,
}

impl GoSourceAnalyzer {
    /// Creates an analyzer using the pinned Go grammar.
    pub fn new() -> Result<Self, SourceAnalysisError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_go::LANGUAGE.into())
            .map_err(|_| SourceAnalysisError::GrammarUnavailable)?;
        Ok(Self { parser })
    }

    /// Analyzes immutable Go bytes without filesystem or database I/O.
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

impl fmt::Debug for GoSourceAnalyzer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GoSourceAnalyzer")
            .field("language", &"Go")
            .finish_non_exhaustive()
    }
}

fn traverse_tree(
    tree: &tree_sitter::Tree,
    source: &[u8],
    limits: SourceAnalysisLimits,
    control: SourceAnalysisControl<'_>,
) -> Result<SourceAnalysis, SourceAnalysisError> {
    let package = package_name(tree.root_node(), source)?;
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
        extract_symbol_facts(node, package, source, limits, &mut facts)?;

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
                    0,
                    limits,
                );
            }
            depth = depth
                .checked_sub(1)
                .ok_or(SourceAnalysisError::InvalidSourceSpan)?;
        }
    }
}

fn package_name<'a>(
    root: Node<'_>,
    source: &'a [u8],
) -> Result<Option<&'a str>, SourceAnalysisError> {
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        if child.kind() == "package_clause" {
            let Some(name) = child.named_child(0) else {
                return Ok(None);
            };
            return source_text(name, source).map(Some);
        }
    }
    Ok(None)
}

fn extract_symbol_facts(
    node: Node<'_>,
    package: Option<&str>,
    source: &[u8],
    limits: SourceAnalysisLimits,
    facts: &mut Vec<SymbolFact>,
) -> Result<(), SourceAnalysisError> {
    let Some(kind) = symbol_kind(node) else {
        return Ok(());
    };
    let mut cursor = node.walk();
    let names = node.children_by_field_name("name", &mut cursor);
    for name_node in names {
        if !matches!(
            name_node.kind(),
            "identifier" | "field_identifier" | "type_identifier"
        ) {
            continue;
        }
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
        let qualified_name = qualified_name(node, package, name, source, limits)?;
        facts.push(SymbolFact::try_new(
            kind,
            name.to_owned(),
            qualified_name,
            source_span(name_node, source)?,
            source_span(node, source)?,
            limits,
        )?);
    }
    Ok(())
}

fn symbol_kind(node: Node<'_>) -> Option<SymbolKind> {
    match node.kind() {
        "function_declaration" => Some(SymbolKind::Function),
        "method_declaration" => Some(SymbolKind::Method),
        "type_spec" if is_package_level_declaration(node) => {
            match node.child_by_field_name("type").map(|child| child.kind()) {
                Some("struct_type") => Some(SymbolKind::Struct),
                Some("interface_type") => Some(SymbolKind::Interface),
                Some(_) => Some(SymbolKind::DefinedType),
                None => None,
            }
        }
        "type_alias" if is_package_level_declaration(node) => Some(SymbolKind::TypeAlias),
        "const_spec" if is_package_level_declaration(node) => Some(SymbolKind::Constant),
        "var_spec" if is_package_level_declaration(node) => Some(SymbolKind::Variable),
        _ => None,
    }
}

fn is_package_level_declaration(node: Node<'_>) -> bool {
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        match current.kind() {
            "function_declaration" | "method_declaration" | "func_literal" => return false,
            "source_file" => return true,
            _ => ancestor = current.parent(),
        }
    }
    false
}

fn qualified_name(
    node: Node<'_>,
    package: Option<&str>,
    name: &str,
    source: &[u8],
    limits: SourceAnalysisLimits,
) -> Result<String, SourceAnalysisError> {
    let receiver = if node.kind() == "method_declaration" {
        node.child_by_field_name("receiver")
            .and_then(first_receiver_type_identifier)
            .map(|receiver| source_text(receiver, source))
            .transpose()?
    } else {
        None
    };
    let required_bytes = [package, receiver, Some(name)]
        .into_iter()
        .flatten()
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
    for component in [package, receiver, Some(name)].into_iter().flatten() {
        if !qualified.is_empty() {
            qualified.push_str("::");
        }
        qualified.push_str(component);
    }
    Ok(qualified)
}

fn first_receiver_type_identifier<'tree>(node: Node<'tree>) -> Option<Node<'tree>> {
    if node.kind() == "type_identifier" {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(identifier) = first_receiver_type_identifier(child) {
            return Some(identifier);
        }
    }
    None
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

    fn analyze(source: &[u8]) -> SourceAnalysis {
        let cancelled = AtomicBool::new(false);
        GoSourceAnalyzer::new()
            .expect("Go grammar must load")
            .analyze(
                source,
                SourceAnalysisLimits::default(),
                SourceAnalysisControl::new(&cancelled, Instant::now() + Duration::from_secs(5)),
            )
            .expect("fixture should analyze")
    }

    #[test]
    fn extracts_go_declarations_in_source_order() {
        let source = br#"package sample

const (
    First, Second = 1, 2
)
var Visible, Hidden string
type Item struct{}
type Reader interface { Read([]byte) (int, error) }
type Identifier string
type Alias = Identifier
func Build[T any](value T) T { return value }
func (item *Item) Save() {}
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
                (
                    SymbolKind::Constant,
                    "First".to_owned(),
                    "sample::First".to_owned()
                ),
                (
                    SymbolKind::Constant,
                    "Second".to_owned(),
                    "sample::Second".to_owned()
                ),
                (
                    SymbolKind::Variable,
                    "Visible".to_owned(),
                    "sample::Visible".to_owned()
                ),
                (
                    SymbolKind::Variable,
                    "Hidden".to_owned(),
                    "sample::Hidden".to_owned()
                ),
                (
                    SymbolKind::Struct,
                    "Item".to_owned(),
                    "sample::Item".to_owned()
                ),
                (
                    SymbolKind::Interface,
                    "Reader".to_owned(),
                    "sample::Reader".to_owned()
                ),
                (
                    SymbolKind::DefinedType,
                    "Identifier".to_owned(),
                    "sample::Identifier".to_owned()
                ),
                (
                    SymbolKind::TypeAlias,
                    "Alias".to_owned(),
                    "sample::Alias".to_owned()
                ),
                (
                    SymbolKind::Function,
                    "Build".to_owned(),
                    "sample::Build".to_owned()
                ),
                (
                    SymbolKind::Method,
                    "Save".to_owned(),
                    "sample::Item::Save".to_owned()
                ),
            ]
        );
        assert!(!analysis.has_syntax_errors());
        for fact in analysis.facts() {
            let start = usize::try_from(fact.name_span().start().get()).expect("span fits");
            let end = usize::try_from(fact.name_span().end().get()).expect("span fits");
            assert_eq!(&source[start..end], fact.name().as_bytes());
        }
    }

    #[test]
    fn generic_pointer_receivers_use_the_declared_receiver_type() {
        let analysis = analyze(
            b"package generic\n\
              type Pair[A, B any] struct { left A; right B }\n\
              func (pair *Pair[A, B]) Swap() Pair[B, A] { panic(\"fixture\") }\n",
        );
        let method = analysis
            .facts()
            .iter()
            .find(|fact| fact.kind() == SymbolKind::Method)
            .expect("method should be present");

        assert_eq!(method.qualified_name(), "generic::Pair::Swap");
    }

    #[test]
    fn local_declarations_are_not_misattributed_as_package_symbols() {
        let analysis = analyze(
            br#"package scoped

var PackageValue int
type PackageType struct{}

func Outer() {
    const localConstant = 1
    var localVariable int
    type localType struct{}
    type localAlias = int
    _ = func() {
        var closureLocal int
    }
}
"#,
        );
        let names = analysis
            .facts()
            .iter()
            .map(SymbolFact::name)
            .collect::<Vec<_>>();

        assert_eq!(names, ["PackageValue", "PackageType", "Outer"]);
        assert!(!analysis.has_syntax_errors());
    }

    #[test]
    fn malformed_go_retains_explicit_syntax_error_coverage() {
        let analysis = analyze(b"package broken\nfunc Incomplete(\n");

        assert!(analysis.has_syntax_errors());
        assert!(analysis.syntax_error_nodes() > 0);
    }

    #[test]
    fn cancellation_and_deadlines_fail_closed() {
        let mut analyzer = GoSourceAnalyzer::new().expect("Go grammar must load");
        let cancelled = AtomicBool::new(true);
        let result = analyzer.analyze(
            b"package cancelled",
            SourceAnalysisLimits::default(),
            SourceAnalysisControl::new(&cancelled, Instant::now() + Duration::from_secs(5)),
        );
        assert_eq!(result, Err(SourceAnalysisError::Cancelled));

        cancelled.store(false, Ordering::Release);
        let result = analyzer.analyze(
            b"package expired",
            SourceAnalysisLimits::default(),
            SourceAnalysisControl::new(&cancelled, Instant::now()),
        );
        assert_eq!(result, Err(SourceAnalysisError::DeadlineExceeded));
    }

    #[test]
    fn configured_bounds_are_enforced() {
        let limits = SourceAnalysisLimits::try_new(8, 1_000, 64, 10, 32, 128)
            .expect("fixture limits should be valid");
        let cancelled = AtomicBool::new(false);
        let result = GoSourceAnalyzer::new()
            .expect("Go grammar must load")
            .analyze(
                b"package too_large",
                limits,
                SourceAnalysisControl::new(&cancelled, Instant::now() + Duration::from_secs(5)),
            );

        assert_eq!(result, Err(SourceAnalysisError::SourceLimitExceeded));
    }
}
