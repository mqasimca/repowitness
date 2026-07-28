use std::{fmt, ops::ControlFlow};

use repowitness_domain::{ByteOffset, ByteSpan};
use tree_sitter::{Node, ParseOptions, Parser};

use crate::{
    SourceAnalysis, SourceAnalysisControl, SourceAnalysisError, SourceAnalysisLimits, SymbolFact,
    SymbolKind,
};

/// Version of the Phase 0 Python extraction behavior.
pub const PYTHON_ANALYSIS_PROFILE_VERSION: u32 = 1;
/// Pinned Tree-sitter Python grammar package version.
pub const TREE_SITTER_PYTHON_GRAMMAR_VERSION: &str = "0.25.0";

/// Returns exact first-party Python analyzer bytes for producer fingerprinting.
#[must_use]
pub fn python_analyzer_implementation_fingerprint_input() -> &'static [u8] {
    include_bytes!("python_source.rs")
}

/// Returns the pinned Python grammar node schema for producer fingerprinting.
#[must_use]
pub fn python_grammar_fingerprint_input() -> &'static [u8] {
    tree_sitter_python::NODE_TYPES.as_bytes()
}

/// Reusable owner of one Tree-sitter Python parser.
pub struct PythonSourceAnalyzer {
    parser: Parser,
}

impl PythonSourceAnalyzer {
    /// Creates an analyzer using the pinned Python grammar.
    pub fn new() -> Result<Self, SourceAnalysisError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .map_err(|_| SourceAnalysisError::GrammarUnavailable)?;
        Ok(Self { parser })
    }

    /// Analyzes immutable Python bytes without filesystem or database I/O.
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

impl fmt::Debug for PythonSourceAnalyzer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PythonSourceAnalyzer")
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

fn extract_symbol_fact(
    node: Node<'_>,
    source: &[u8],
    limits: SourceAnalysisLimits,
    facts: &mut Vec<SymbolFact>,
) -> Result<(), SourceAnalysisError> {
    let Some(kind) = symbol_kind(node) else {
        return Ok(());
    };
    let Some(name_node) = admitted_name_node(node) else {
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
        source_span(declaration_node(node), source)?,
        limits,
    )?);
    Ok(())
}

fn symbol_kind(node: Node<'_>) -> Option<SymbolKind> {
    match node.kind() {
        "function_definition" => Some(function_kind(node)),
        "class_definition" => Some(SymbolKind::Class),
        "assignment" if is_module_assignment(node) => Some(SymbolKind::Variable),
        "type_alias_statement" => Some(SymbolKind::TypeAlias),
        _ => None,
    }
}

fn function_kind(node: Node<'_>) -> SymbolKind {
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        match current.kind() {
            "function_definition" => return SymbolKind::Function,
            "class_definition" => return SymbolKind::Method,
            "module" => return SymbolKind::Function,
            _ => ancestor = current.parent(),
        }
    }
    SymbolKind::Function
}

fn admitted_name_node(node: Node<'_>) -> Option<Node<'_>> {
    let name = match node.kind() {
        "assignment" => node.child_by_field_name("left")?,
        "type_alias_statement" => return type_alias_name_node(node),
        _ => node.child_by_field_name("name")?,
    };
    (name.kind() == "identifier").then_some(name)
}

fn type_alias_name_node(node: Node<'_>) -> Option<Node<'_>> {
    let left = node.child_by_field_name("left")?;
    let concrete = if left.kind() == "type" {
        left.named_child(0)?
    } else {
        left
    };
    match concrete.kind() {
        "identifier" => Some(concrete),
        "generic_type" => concrete
            .named_child(0)
            .filter(|name| name.kind() == "identifier"),
        _ => None,
    }
}

fn is_module_assignment(node: Node<'_>) -> bool {
    if node
        .child_by_field_name("left")
        .is_none_or(|left| left.kind() != "identifier")
    {
        return false;
    }
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        match current.kind() {
            "function_definition" | "class_definition" => return false,
            "module" => return true,
            _ => ancestor = current.parent(),
        }
    }
    false
}

fn declaration_node(node: Node<'_>) -> Node<'_> {
    let Some(parent) = node.parent() else {
        return node;
    };
    if parent.kind() == "decorated_definition"
        && parent.child_by_field_name("definition") == Some(node)
    {
        parent
    } else {
        node
    }
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
    if !matches!(node.kind(), "class_definition" | "function_definition") {
        return Ok(None);
    }
    let Some(name) = node.child_by_field_name("name") else {
        return Ok(None);
    };
    if name.kind() != "identifier" {
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
mod tests;
