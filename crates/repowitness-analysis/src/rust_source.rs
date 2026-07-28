use std::{
    error::Error,
    fmt,
    ops::ControlFlow,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use repowitness_domain::{ByteOffset, ByteSpan};
use tree_sitter::{Node, ParseOptions, Parser};

use crate::rust_correspondence::{RustOccurrenceFingerprint, fingerprint_rust_occurrence};

/// Version of the Phase 0 Rust extraction behavior implemented by this module.
///
/// This version is an explicit compatibility input in addition to the exact
/// implementation and grammar fingerprints exposed below.
pub const RUST_ANALYSIS_PROFILE_VERSION: u32 = 3;
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

/// Returns exact split analyzer implementation bytes for producer fingerprinting.
#[must_use]
pub fn rust_analyzer_traversal_fingerprint_input() -> &'static [u8] {
    include_bytes!("rust_source/analyzer.rs")
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

    pub(crate) fn outcome(self) -> Option<RustAnalysisError> {
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

/// Stable declaration categories emitted by built-in syntax adapters.
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
    /// A Go interface declaration.
    Interface,
    /// A Go defined type whose underlying type is not a struct or interface.
    DefinedType,
    /// A Go package variable declaration.
    Variable,
    /// A class declaration.
    Class,
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
            Self::Interface => "interface",
            Self::DefinedType => "defined_type",
            Self::Variable => "variable",
            Self::Class => "class",
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
            "interface" => Some(Self::Interface),
            "defined_type" => Some(Self::DefinedType),
            "variable" => Some(Self::Variable),
            "class" => Some(Self::Class),
            _ => None,
        }
    }
}

/// Language-neutral name for the declaration categories shared by adapters.
pub type SymbolKind = RustSymbolKind;

/// One deterministic direct-syntax declaration fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustSymbolFact {
    kind: RustSymbolKind,
    name: String,
    qualified_name: String,
    name_span: ByteSpan,
    declaration_span: ByteSpan,
    correspondence: Option<RustOccurrenceFingerprint>,
}

/// Language-neutral name for one deterministic syntax declaration fact.
pub type SymbolFact = RustSymbolFact;

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
        Self::try_new_inner(
            kind,
            name,
            qualified_name,
            name_span,
            declaration_span,
            None,
            limits,
        )
    }

    /// Reconstructs one structurally validated fact with its exact persisted
    /// Rust correspondence fingerprint.
    pub fn try_new_with_correspondence(
        kind: RustSymbolKind,
        name: String,
        qualified_name: String,
        name_span: ByteSpan,
        declaration_span: ByteSpan,
        correspondence: RustOccurrenceFingerprint,
        limits: RustAnalysisLimits,
    ) -> Result<Self, RustAnalysisError> {
        Self::try_new_inner(
            kind,
            name,
            qualified_name,
            name_span,
            declaration_span,
            Some(correspondence),
            limits,
        )
    }

    fn try_new_inner(
        kind: RustSymbolKind,
        name: String,
        qualified_name: String,
        name_span: ByteSpan,
        declaration_span: ByteSpan,
        correspondence: Option<RustOccurrenceFingerprint>,
        limits: RustAnalysisLimits,
    ) -> Result<Self, RustAnalysisError> {
        let fact = Self {
            kind,
            name,
            qualified_name,
            name_span,
            declaration_span,
            correspondence,
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

    /// Returns the derived Rust occurrence identity when this fact was emitted
    /// by the Rust correspondence-aware analysis profile.
    #[must_use]
    pub const fn correspondence(&self) -> Option<RustOccurrenceFingerprint> {
        self.correspondence
    }
}

/// Complete bounded output for one immutable source input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustSourceAnalysis {
    facts: Vec<RustSymbolFact>,
    visited_nodes: u32,
    syntax_error_nodes: u32,
    known_parser_limitation_nodes: u32,
}

/// Language-neutral name for complete bounded source analysis.
pub type SourceAnalysis = RustSourceAnalysis;

impl RustSourceAnalysis {
    /// Reconstructs structurally validated analysis output at a trust boundary.
    pub fn try_from_parts(
        facts: Vec<RustSymbolFact>,
        visited_nodes: u32,
        syntax_error_nodes: u32,
        known_parser_limitation_nodes: u32,
        limits: RustAnalysisLimits,
    ) -> Result<Self, RustAnalysisError> {
        let fact_count =
            u32::try_from(facts.len()).map_err(|_| RustAnalysisError::InvalidAnalysisArtifact)?;
        if visited_nodes == 0
            || visited_nodes > limits.max_syntax_nodes()
            || syntax_error_nodes > visited_nodes
            || known_parser_limitation_nodes > syntax_error_nodes
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
            known_parser_limitation_nodes,
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
            || self.known_parser_limitation_nodes > self.syntax_error_nodes
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

    /// Returns the conservative subset attributed to recognized parser limitations.
    #[must_use]
    pub const fn known_parser_limitation_nodes(&self) -> u32 {
        self.known_parser_limitation_nodes
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

/// Language-neutral name for a bounded source-analysis failure.
pub type SourceAnalysisError = RustAnalysisError;

impl fmt::Display for RustAnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLimits => "source analysis limits are invalid",
            Self::SourceLimitExceeded => "source byte limit exceeded",
            Self::GrammarUnavailable => "source grammar is unavailable",
            Self::ParseFailed => "source parsing failed",
            Self::Cancelled => "source analysis cancelled",
            Self::DeadlineExceeded => "source analysis deadline exceeded",
            Self::NodeLimitExceeded => "syntax node limit exceeded",
            Self::DepthLimitExceeded => "syntax depth limit exceeded",
            Self::FactLimitExceeded => "symbol fact limit exceeded",
            Self::NameLimitExceeded => "symbol name limit exceeded",
            Self::QualifiedNameLimitExceeded => "qualified name limit exceeded",
            Self::InvalidIdentifierEncoding => "symbol name encoding is invalid",
            Self::InvalidSourceSpan => "parser returned an invalid source span",
            Self::InvalidAnalysisArtifact => "analysis artifact is invalid",
        })
    }
}

/// Language-neutral name for per-file source-analysis limits.
pub type SourceAnalysisLimits = RustAnalysisLimits;

/// Language-neutral name for cooperative analysis control.
pub type SourceAnalysisControl<'a> = RustAnalysisControl<'a>;

impl Error for RustAnalysisError {}

/// Reusable owner of one Tree-sitter Rust parser.
pub struct RustSourceAnalyzer {
    parser: Parser,
}

include!("rust_source/analyzer.rs");

#[cfg(test)]
mod tests;
