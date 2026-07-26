use std::{
    error::Error,
    fmt,
    ops::ControlFlow,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use repowitness_domain::{ByteOffset, ByteSpan};
use tree_sitter::{Node, ParseOptions, Parser};

/// Version of the Phase 0 Rust extraction behavior implemented by this module.
///
/// This version is an explicit compatibility input in addition to the exact
/// implementation and grammar fingerprints exposed below.
pub const RUST_ANALYSIS_PROFILE_VERSION: u32 = 1;
/// Pinned Tree-sitter runtime version used by the Phase 0 Rust analyzer.
pub const TREE_SITTER_RUNTIME_VERSION: &str = "0.26.11";
/// Pinned Tree-sitter Rust grammar package version.
pub const TREE_SITTER_RUST_GRAMMAR_VERSION: &str = "0.24.2";

const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SYNTAX_NODES: u32 = 1_000_000;
const MAX_SYNTAX_DEPTH: u16 = 256;
const MAX_SYMBOL_FACTS: u32 = 100_000;
const MAX_SYMBOL_NAME_BYTES: u16 = 1_024;
const MAX_QUALIFIED_NAME_BYTES: u16 = 4_096;

/// Returns exact first-party analyzer source bytes for producer fingerprinting.
///
/// Conservatively invalidating reuse after any implementation change is safer
/// than relying on a maintainer to remember to increment a version constant.
#[must_use]
pub fn rust_analyzer_implementation_fingerprint_input() -> &'static [u8] {
    include_bytes!("rust_source.rs")
}

/// Returns the pinned grammar node schema for producer fingerprinting.
#[must_use]
pub fn rust_grammar_fingerprint_input() -> &'static [u8] {
    tree_sitter_rust::NODE_TYPES.as_bytes()
}

/// Explicit resource limits for one immutable Rust source analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RustAnalysisLimits {
    max_source_bytes: u64,
    max_syntax_nodes: u32,
    max_syntax_depth: u16,
    max_symbol_facts: u32,
    max_symbol_name_bytes: u16,
    max_qualified_name_bytes: u16,
}

impl RustAnalysisLimits {
    /// Conservative Phase 0 limits for one Rust source file.
    pub const DEFAULT: Self = Self {
        max_source_bytes: MAX_SOURCE_BYTES,
        max_syntax_nodes: MAX_SYNTAX_NODES,
        max_syntax_depth: MAX_SYNTAX_DEPTH,
        max_symbol_facts: MAX_SYMBOL_FACTS,
        max_symbol_name_bytes: MAX_SYMBOL_NAME_BYTES,
        max_qualified_name_bytes: MAX_QUALIFIED_NAME_BYTES,
    };

    /// Creates limits no larger than the Phase 0 hard ceilings.
    pub fn try_new(
        max_source_bytes: u64,
        max_syntax_nodes: u32,
        max_syntax_depth: u16,
        max_symbol_facts: u32,
        max_symbol_name_bytes: u16,
        max_qualified_name_bytes: u16,
    ) -> Result<Self, RustAnalysisError> {
        let limits = Self {
            max_source_bytes,
            max_syntax_nodes,
            max_syntax_depth,
            max_symbol_facts,
            max_symbol_name_bytes,
            max_qualified_name_bytes,
        };
        if max_source_bytes == 0
            || max_source_bytes > MAX_SOURCE_BYTES
            || max_syntax_nodes == 0
            || max_syntax_nodes > MAX_SYNTAX_NODES
            || max_syntax_depth == 0
            || max_syntax_depth > MAX_SYNTAX_DEPTH
            || max_symbol_facts == 0
            || max_symbol_facts > MAX_SYMBOL_FACTS
            || max_symbol_name_bytes == 0
            || max_symbol_name_bytes > MAX_SYMBOL_NAME_BYTES
            || max_qualified_name_bytes == 0
            || max_qualified_name_bytes > MAX_QUALIFIED_NAME_BYTES
        {
            return Err(RustAnalysisError::InvalidLimits);
        }
        Ok(limits)
    }

    /// Returns the source-byte limit.
    #[must_use]
    pub const fn max_source_bytes(self) -> u64 {
        self.max_source_bytes
    }

    /// Returns the syntax-node limit.
    #[must_use]
    pub const fn max_syntax_nodes(self) -> u32 {
        self.max_syntax_nodes
    }

    /// Returns the syntax-depth limit.
    #[must_use]
    pub const fn max_syntax_depth(self) -> u16 {
        self.max_syntax_depth
    }

    /// Returns the symbol-fact limit.
    #[must_use]
    pub const fn max_symbol_facts(self) -> u32 {
        self.max_symbol_facts
    }

    /// Returns the simple symbol-name byte limit.
    #[must_use]
    pub const fn max_symbol_name_bytes(self) -> u16 {
        self.max_symbol_name_bytes
    }

    /// Returns the qualified symbol-name byte limit.
    #[must_use]
    pub const fn max_qualified_name_bytes(self) -> u16 {
        self.max_qualified_name_bytes
    }
}

impl Default for RustAnalysisLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Cooperative cancellation and deadline state for one analysis.
#[derive(Clone, Copy)]
pub struct RustAnalysisControl<'a> {
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

impl<'a> RustAnalysisControl<'a> {
    /// Creates control state from an owned cancellation flag and absolute deadline.
    #[must_use]
    pub const fn new(cancelled: &'a AtomicBool, deadline: Instant) -> Self {
        Self {
            cancelled,
            deadline,
        }
    }

    fn outcome(self) -> Option<RustAnalysisError> {
        if self.cancelled.load(Ordering::Acquire) {
            Some(RustAnalysisError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Some(RustAnalysisError::DeadlineExceeded)
        } else {
            None
        }
    }
}

impl fmt::Debug for RustAnalysisControl<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustAnalysisControl")
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Stable Rust declaration categories emitted by the syntax adapter.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RustSymbolKind {
    /// A free function.
    Function,
    /// A function declared inside an implementation or trait.
    Method,
    /// A structure declaration.
    Struct,
    /// An enumeration declaration.
    Enum,
    /// A union declaration.
    Union,
    /// A trait declaration.
    Trait,
    /// A module declaration.
    Module,
    /// A type alias.
    TypeAlias,
    /// A constant item.
    Constant,
    /// A static item.
    Static,
    /// A declarative macro definition.
    Macro,
}

impl RustSymbolKind {
    /// Returns the stable persistence/wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Union => "union",
            Self::Trait => "trait",
            Self::Module => "module",
            Self::TypeAlias => "type_alias",
            Self::Constant => "constant",
            Self::Static => "static",
            Self::Macro => "macro",
        }
    }

    /// Decodes the exact stable persistence/wire spelling.
    #[must_use]
    pub fn from_stable_str(value: &str) -> Option<Self> {
        match value {
            "function" => Some(Self::Function),
            "method" => Some(Self::Method),
            "struct" => Some(Self::Struct),
            "enum" => Some(Self::Enum),
            "union" => Some(Self::Union),
            "trait" => Some(Self::Trait),
            "module" => Some(Self::Module),
            "type_alias" => Some(Self::TypeAlias),
            "constant" => Some(Self::Constant),
            "static" => Some(Self::Static),
            "macro" => Some(Self::Macro),
            _ => None,
        }
    }
}

/// One deterministic direct-syntax declaration fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustSymbolFact {
    kind: RustSymbolKind,
    name: String,
    qualified_name: String,
    name_span: ByteSpan,
    declaration_span: ByteSpan,
}

impl RustSymbolFact {
    /// Constructs one structurally validated fact at a trust boundary.
    pub fn try_new(
        kind: RustSymbolKind,
        name: String,
        qualified_name: String,
        name_span: ByteSpan,
        declaration_span: ByteSpan,
        limits: RustAnalysisLimits,
    ) -> Result<Self, RustAnalysisError> {
        let fact = Self {
            kind,
            name,
            qualified_name,
            name_span,
            declaration_span,
        };
        validate_fact_structure(&fact, limits)?;
        Ok(fact)
    }

    /// Returns the stable declaration category.
    #[must_use]
    pub const fn kind(&self) -> RustSymbolKind {
        self.kind
    }

    /// Returns the exact UTF-8 identifier text.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the deterministic syntax-qualified name.
    #[must_use]
    pub fn qualified_name(&self) -> &str {
        &self.qualified_name
    }

    /// Returns the identifier's half-open source span.
    #[must_use]
    pub const fn name_span(&self) -> ByteSpan {
        self.name_span
    }

    /// Returns the complete declaration's half-open source span.
    #[must_use]
    pub const fn declaration_span(&self) -> ByteSpan {
        self.declaration_span
    }
}

/// Complete bounded output for one immutable source input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustSourceAnalysis {
    facts: Vec<RustSymbolFact>,
    visited_nodes: u32,
    syntax_error_nodes: u32,
}

impl RustSourceAnalysis {
    /// Reconstructs structurally validated analysis output at a trust boundary.
    pub fn try_from_parts(
        facts: Vec<RustSymbolFact>,
        visited_nodes: u32,
        syntax_error_nodes: u32,
        limits: RustAnalysisLimits,
    ) -> Result<Self, RustAnalysisError> {
        let fact_count =
            u32::try_from(facts.len()).map_err(|_| RustAnalysisError::InvalidAnalysisArtifact)?;
        if visited_nodes == 0
            || visited_nodes > limits.max_syntax_nodes()
            || syntax_error_nodes > visited_nodes
            || fact_count > limits.max_symbol_facts()
        {
            return Err(RustAnalysisError::InvalidAnalysisArtifact);
        }
        for fact in &facts {
            validate_fact_structure(fact, limits)?;
        }
        Ok(Self {
            facts,
            visited_nodes,
            syntax_error_nodes,
        })
    }

    /// Validates persisted output against the exact immutable source bytes.
    pub fn validate_for_reuse(
        &self,
        source: &[u8],
        limits: RustAnalysisLimits,
    ) -> Result<(), RustAnalysisError> {
        let source_bytes =
            u64::try_from(source.len()).map_err(|_| RustAnalysisError::InvalidAnalysisArtifact)?;
        if source_bytes > limits.max_source_bytes()
            || self.visited_nodes == 0
            || self.visited_nodes > limits.max_syntax_nodes()
            || self.syntax_error_nodes > self.visited_nodes
            || self.facts.len()
                > usize::try_from(limits.max_symbol_facts())
                    .map_err(|_| RustAnalysisError::InvalidAnalysisArtifact)?
        {
            return Err(RustAnalysisError::InvalidAnalysisArtifact);
        }
        for fact in &self.facts {
            validate_reusable_fact(fact, source, limits)?;
        }
        Ok(())
    }

    /// Returns declaration facts in deterministic source preorder.
    #[must_use]
    pub fn facts(&self) -> &[RustSymbolFact] {
        &self.facts
    }

    /// Returns the exact number of traversed syntax nodes.
    #[must_use]
    pub const fn visited_nodes(&self) -> u32 {
        self.visited_nodes
    }

    /// Returns the number of explicit error or missing syntax nodes.
    #[must_use]
    pub const fn syntax_error_nodes(&self) -> u32 {
        self.syntax_error_nodes
    }

    /// Reports whether Tree-sitter produced incomplete or erroneous syntax.
    #[must_use]
    pub const fn has_syntax_errors(&self) -> bool {
        self.syntax_error_nodes != 0
    }
}

/// Stable redacted failure from bounded Rust source analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustAnalysisError {
    /// Configured limits are zero or exceed the Phase 0 hard ceilings.
    InvalidLimits,
    /// The immutable source input exceeds its byte limit.
    SourceLimitExceeded,
    /// Tree-sitter could not load the pinned Rust grammar.
    GrammarUnavailable,
    /// Tree-sitter stopped without a cancellation or deadline reason.
    ParseFailed,
    /// Cancellation was observed before producing a complete result.
    Cancelled,
    /// The absolute analysis deadline elapsed.
    DeadlineExceeded,
    /// Syntax traversal exceeded its node limit.
    NodeLimitExceeded,
    /// Syntax traversal exceeded its depth limit.
    DepthLimitExceeded,
    /// Declaration extraction exceeded its fact limit.
    FactLimitExceeded,
    /// An identifier exceeded its byte limit.
    NameLimitExceeded,
    /// A qualified identifier exceeded its byte limit.
    QualifiedNameLimitExceeded,
    /// A declaration identifier was not valid UTF-8.
    InvalidIdentifierEncoding,
    /// Tree-sitter returned a source span outside the immutable input.
    InvalidSourceSpan,
    /// Persisted analysis output failed structural or source-bound validation.
    InvalidAnalysisArtifact,
}

impl fmt::Display for RustAnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLimits => "Rust analysis limits are invalid",
            Self::SourceLimitExceeded => "Rust source byte limit exceeded",
            Self::GrammarUnavailable => "Rust grammar is unavailable",
            Self::ParseFailed => "Rust parsing failed",
            Self::Cancelled => "Rust analysis cancelled",
            Self::DeadlineExceeded => "Rust analysis deadline exceeded",
            Self::NodeLimitExceeded => "Rust syntax node limit exceeded",
            Self::DepthLimitExceeded => "Rust syntax depth limit exceeded",
            Self::FactLimitExceeded => "Rust symbol fact limit exceeded",
            Self::NameLimitExceeded => "Rust symbol name limit exceeded",
            Self::QualifiedNameLimitExceeded => "Rust qualified name limit exceeded",
            Self::InvalidIdentifierEncoding => "Rust symbol name encoding is invalid",
            Self::InvalidSourceSpan => "Rust parser returned an invalid source span",
            Self::InvalidAnalysisArtifact => "Rust analysis artifact is invalid",
        })
    }
}

impl Error for RustAnalysisError {}

/// Reusable owner of one Tree-sitter Rust parser.
pub struct RustSourceAnalyzer {
    parser: Parser,
}

impl RustSourceAnalyzer {
    /// Creates an analyzer using the pinned Rust grammar.
    pub fn new() -> Result<Self, RustAnalysisError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .map_err(|_| RustAnalysisError::GrammarUnavailable)?;
        Ok(Self { parser })
    }

    /// Analyzes immutable bytes without performing filesystem or database I/O.
    pub fn analyze(
        &mut self,
        source: &[u8],
        limits: RustAnalysisLimits,
        control: RustAnalysisControl<'_>,
    ) -> Result<RustSourceAnalysis, RustAnalysisError> {
        if let Some(outcome) = control.outcome() {
            return Err(outcome);
        }
        let source_bytes =
            u64::try_from(source.len()).map_err(|_| RustAnalysisError::SourceLimitExceeded)?;
        if source_bytes > limits.max_source_bytes {
            return Err(RustAnalysisError::SourceLimitExceeded);
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
        let tree = tree.ok_or(RustAnalysisError::ParseFailed)?;
        traverse_tree(&tree, source, limits, control)
    }
}

impl fmt::Debug for RustSourceAnalyzer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustSourceAnalyzer")
            .field("language", &"Rust")
            .finish_non_exhaustive()
    }
}

fn traverse_tree(
    tree: &tree_sitter::Tree,
    source: &[u8],
    limits: RustAnalysisLimits,
    control: RustAnalysisControl<'_>,
) -> Result<RustSourceAnalysis, RustAnalysisError> {
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
            .ok_or(RustAnalysisError::NodeLimitExceeded)?;
        if visited_nodes > limits.max_syntax_nodes {
            return Err(RustAnalysisError::NodeLimitExceeded);
        }

        let node = cursor.node();
        if node.is_error() || node.is_missing() {
            syntax_error_nodes = syntax_error_nodes.saturating_add(1);
        }
        if let Some(kind) = symbol_kind(node) {
            if facts.len()
                >= usize::try_from(limits.max_symbol_facts)
                    .map_err(|_| RustAnalysisError::FactLimitExceeded)?
            {
                return Err(RustAnalysisError::FactLimitExceeded);
            }
            facts.push(extract_symbol_fact(node, kind, source, limits)?);
        }

        if cursor.goto_first_child() {
            depth = depth
                .checked_add(1)
                .ok_or(RustAnalysisError::DepthLimitExceeded)?;
            if depth > limits.max_syntax_depth {
                return Err(RustAnalysisError::DepthLimitExceeded);
            }
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return RustSourceAnalysis::try_from_parts(
                    facts,
                    visited_nodes,
                    syntax_error_nodes,
                    limits,
                );
            }
            depth = depth
                .checked_sub(1)
                .ok_or(RustAnalysisError::InvalidSourceSpan)?;
        }
    }
}

fn symbol_kind(node: Node<'_>) -> Option<RustSymbolKind> {
    match node.kind() {
        "function_item" if inside_method_container(node) => Some(RustSymbolKind::Method),
        "function_signature_item" if inside_method_container(node) => Some(RustSymbolKind::Method),
        "function_item" => Some(RustSymbolKind::Function),
        "function_signature_item" => Some(RustSymbolKind::Function),
        "struct_item" => Some(RustSymbolKind::Struct),
        "enum_item" => Some(RustSymbolKind::Enum),
        "union_item" => Some(RustSymbolKind::Union),
        "trait_item" => Some(RustSymbolKind::Trait),
        "mod_item" => Some(RustSymbolKind::Module),
        "type_item" => Some(RustSymbolKind::TypeAlias),
        "const_item" => Some(RustSymbolKind::Constant),
        "static_item" => Some(RustSymbolKind::Static),
        "macro_definition" => Some(RustSymbolKind::Macro),
        _ => None,
    }
}

fn inside_method_container(node: Node<'_>) -> bool {
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        if matches!(current.kind(), "impl_item" | "trait_item") {
            return true;
        }
        ancestor = current.parent();
    }
    false
}

fn extract_symbol_fact(
    node: Node<'_>,
    kind: RustSymbolKind,
    source: &[u8],
    limits: RustAnalysisLimits,
) -> Result<RustSymbolFact, RustAnalysisError> {
    let name_node = node
        .child_by_field_name("name")
        .ok_or(RustAnalysisError::InvalidSourceSpan)?;
    let name = source_text(name_node, source)?;
    if name.len() > usize::from(limits.max_symbol_name_bytes) {
        return Err(RustAnalysisError::NameLimitExceeded);
    }
    let qualified_name = qualified_name(node, name, source, limits)?;
    RustSymbolFact::try_new(
        kind,
        name.to_owned(),
        qualified_name,
        source_span(name_node, source)?,
        source_span(node, source)?,
        limits,
    )
}

fn validate_reusable_fact(
    fact: &RustSymbolFact,
    source: &[u8],
    limits: RustAnalysisLimits,
) -> Result<(), RustAnalysisError> {
    validate_fact_structure(fact, limits)?;
    let name_start = usize::try_from(fact.name_span.start().get())
        .map_err(|_| RustAnalysisError::InvalidAnalysisArtifact)?;
    let name_end = usize::try_from(fact.name_span.end().get())
        .map_err(|_| RustAnalysisError::InvalidAnalysisArtifact)?;
    let declaration_end = usize::try_from(fact.declaration_span.end().get())
        .map_err(|_| RustAnalysisError::InvalidAnalysisArtifact)?;
    if declaration_end > source.len()
        || source.get(name_start..name_end) != Some(fact.name.as_bytes())
    {
        return Err(RustAnalysisError::InvalidAnalysisArtifact);
    }
    Ok(())
}

fn validate_fact_structure(
    fact: &RustSymbolFact,
    limits: RustAnalysisLimits,
) -> Result<(), RustAnalysisError> {
    if fact.name.is_empty()
        || fact.qualified_name.is_empty()
        || fact.name.len() > usize::from(limits.max_symbol_name_bytes())
        || fact.qualified_name.len() > usize::from(limits.max_qualified_name_bytes())
        || fact.name_span.start() < fact.declaration_span.start()
        || fact.name_span.end() > fact.declaration_span.end()
    {
        return Err(RustAnalysisError::InvalidAnalysisArtifact);
    }
    Ok(())
}

fn qualified_name(
    node: Node<'_>,
    name: &str,
    source: &[u8],
    limits: RustAnalysisLimits,
) -> Result<String, RustAnalysisError> {
    let mut containers = Vec::new();
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        let container = match current.kind() {
            "impl_item" => current.child_by_field_name("type"),
            "trait_item" | "mod_item" => current.child_by_field_name("name"),
            _ => None,
        };
        if let Some(container) = container {
            let text = source_text(container, source)?;
            if text.len() > usize::from(limits.max_symbol_name_bytes) {
                return Err(RustAnalysisError::NameLimitExceeded);
            }
            containers.push(text);
        }
        ancestor = current.parent();
    }

    let required_bytes = containers
        .iter()
        .try_fold(name.len(), |total, component| {
            total
                .checked_add(component.len())
                .and_then(|sum| sum.checked_add(2))
        })
        .ok_or(RustAnalysisError::QualifiedNameLimitExceeded)?;
    if required_bytes > usize::from(limits.max_qualified_name_bytes) {
        return Err(RustAnalysisError::QualifiedNameLimitExceeded);
    }
    let mut qualified = String::with_capacity(required_bytes);
    for container in containers.iter().rev() {
        qualified.push_str(container);
        qualified.push_str("::");
    }
    qualified.push_str(name);
    Ok(qualified)
}

fn source_text<'a>(node: Node<'_>, source: &'a [u8]) -> Result<&'a str, RustAnalysisError> {
    let range = node.byte_range();
    let bytes = source
        .get(range)
        .ok_or(RustAnalysisError::InvalidSourceSpan)?;
    std::str::from_utf8(bytes).map_err(|_| RustAnalysisError::InvalidIdentifierEncoding)
}

fn source_span(node: Node<'_>, source: &[u8]) -> Result<ByteSpan, RustAnalysisError> {
    let range = node.byte_range();
    if range.end > source.len() {
        return Err(RustAnalysisError::InvalidSourceSpan);
    }
    let start = u64::try_from(range.start).map_err(|_| RustAnalysisError::InvalidSourceSpan)?;
    let end = u64::try_from(range.end).map_err(|_| RustAnalysisError::InvalidSourceSpan)?;
    ByteSpan::try_new(ByteOffset::new(start), ByteOffset::new(end))
        .map_err(|_| RustAnalysisError::InvalidSourceSpan)
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::AtomicBool,
        time::{Duration, Instant},
    };

    use repowitness_domain::{ByteOffset, ByteSpan};

    use super::{
        RustAnalysisControl, RustAnalysisError, RustAnalysisLimits, RustSourceAnalysis,
        RustSourceAnalyzer, RustSymbolFact, RustSymbolKind, TREE_SITTER_RUNTIME_VERSION,
        TREE_SITTER_RUST_GRAMMAR_VERSION,
    };

    #[test]
    fn producer_version_labels_match_the_pinned_workspace_dependencies() {
        let manifest = include_str!("../../../Cargo.toml");
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

        let fact_limited = RustAnalysisLimits::try_new(1_024, 100, 20, 1, 100, 200)
            .expect("test limits are valid");
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

        let depth_limited = RustAnalysisLimits::try_new(1_024, 100, 1, 10, 100, 200)
            .expect("test limits are valid");
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
        assert_eq!(error.to_string(), "Rust symbol name limit exceeded");
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
    }
}
