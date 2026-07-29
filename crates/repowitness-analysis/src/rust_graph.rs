//! Bounded artifact-local Rust graph-site extraction.
//!
//! This module emits inspectable raw syntax sites from one immutable source
//! input. It deliberately performs no repository, filesystem, database, or
//! cross-file resolution.

use std::{
    error::Error,
    fmt,
    ops::ControlFlow,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use repowitness_domain::ByteSpan;
use tree_sitter::{ParseOptions, Parser};

use crate::RustSymbolKind;

/// Version of the artifact-local Rust graph-site extraction behavior.
pub const RUST_GRAPH_SITE_PROFILE_VERSION: u32 = 1;

const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SYNTAX_NODES: u32 = 1_000_000;
const MAX_SYNTAX_DEPTH: u16 = 256;
const MAX_GRAPH_SITES: u32 = 250_000;
const MAX_NAME_BYTES: u16 = 1_024;
const MAX_PATH_BYTES: u16 = 16_384;
const MAX_OWNED_TEXT_BYTES: u64 = 64 * 1024 * 1024;

/// Returns the exact public graph-site implementation source for fingerprinting.
#[must_use]
pub fn rust_graph_site_implementation_fingerprint_input() -> &'static [u8] {
    include_bytes!("rust_graph.rs")
}

/// Returns the exact graph-site traversal source for fingerprinting.
#[must_use]
pub fn rust_graph_site_traversal_fingerprint_input() -> &'static [u8] {
    include_bytes!("rust_graph/analyzer.rs")
}

/// Returns the exact graph-site extraction source for fingerprinting.
#[must_use]
pub fn rust_graph_site_extraction_fingerprint_input() -> &'static [u8] {
    include_bytes!("rust_graph/extraction.rs")
}

/// Explicit limits for one immutable Rust graph-site analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RustGraphAnalysisLimits {
    max_source_bytes: u64,
    max_syntax_nodes: u32,
    max_syntax_depth: u16,
    max_graph_sites: u32,
    max_name_bytes: u16,
    max_path_bytes: u16,
    max_owned_text_bytes: u64,
}

impl RustGraphAnalysisLimits {
    /// Conservative hard-bounded defaults for one Rust source file.
    pub const DEFAULT: Self = Self {
        max_source_bytes: MAX_SOURCE_BYTES,
        max_syntax_nodes: MAX_SYNTAX_NODES,
        max_syntax_depth: MAX_SYNTAX_DEPTH,
        max_graph_sites: MAX_GRAPH_SITES,
        max_name_bytes: MAX_NAME_BYTES,
        max_path_bytes: MAX_PATH_BYTES,
        max_owned_text_bytes: MAX_OWNED_TEXT_BYTES,
    };

    /// Creates positive limits no larger than the compiled hard ceilings.
    pub fn try_new(
        max_source_bytes: u64,
        max_syntax_nodes: u32,
        max_syntax_depth: u16,
        max_graph_sites: u32,
        max_name_bytes: u16,
        max_path_bytes: u16,
        max_owned_text_bytes: u64,
    ) -> Result<Self, RustGraphAnalysisError> {
        let limits = Self {
            max_source_bytes,
            max_syntax_nodes,
            max_syntax_depth,
            max_graph_sites,
            max_name_bytes,
            max_path_bytes,
            max_owned_text_bytes,
        };
        if limits.is_valid() {
            Ok(limits)
        } else {
            Err(RustGraphAnalysisError::InvalidLimits)
        }
    }

    const fn is_valid(self) -> bool {
        self.max_source_bytes != 0
            && self.max_source_bytes <= MAX_SOURCE_BYTES
            && self.max_syntax_nodes != 0
            && self.max_syntax_nodes <= MAX_SYNTAX_NODES
            && self.max_syntax_depth != 0
            && self.max_syntax_depth <= MAX_SYNTAX_DEPTH
            && self.max_graph_sites != 0
            && self.max_graph_sites <= MAX_GRAPH_SITES
            && self.max_name_bytes != 0
            && self.max_name_bytes <= MAX_NAME_BYTES
            && self.max_path_bytes != 0
            && self.max_path_bytes <= MAX_PATH_BYTES
            && self.max_owned_text_bytes != 0
            && self.max_owned_text_bytes <= MAX_OWNED_TEXT_BYTES
    }

    /// Returns the immutable source-byte limit.
    #[must_use]
    pub const fn max_source_bytes(self) -> u64 {
        self.max_source_bytes
    }

    /// Returns the syntax-node traversal limit.
    #[must_use]
    pub const fn max_syntax_nodes(self) -> u32 {
        self.max_syntax_nodes
    }

    /// Returns the syntax-tree depth limit.
    #[must_use]
    pub const fn max_syntax_depth(self) -> u16 {
        self.max_syntax_depth
    }

    /// Returns the emitted raw-site limit.
    #[must_use]
    pub const fn max_graph_sites(self) -> u32 {
        self.max_graph_sites
    }

    /// Returns the enclosing-definition name-component limit.
    #[must_use]
    pub const fn max_name_bytes(self) -> u16 {
        self.max_name_bytes
    }

    /// Returns the raw target and qualified-path limit.
    #[must_use]
    pub const fn max_path_bytes(self) -> u16 {
        self.max_path_bytes
    }

    /// Returns the aggregate owned target/descriptor text limit.
    #[must_use]
    pub const fn max_owned_text_bytes(self) -> u64 {
        self.max_owned_text_bytes
    }
}

impl Default for RustGraphAnalysisLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Cooperative cancellation and monotonic deadline for one analysis.
#[derive(Clone, Copy)]
pub struct RustGraphAnalysisControl<'a> {
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

impl<'a> RustGraphAnalysisControl<'a> {
    /// Creates control state from an owned cancellation flag and deadline.
    #[must_use]
    pub const fn new(cancelled: &'a AtomicBool, deadline: Instant) -> Self {
        Self {
            cancelled,
            deadline,
        }
    }

    fn outcome(self) -> Option<RustGraphAnalysisError> {
        if self.cancelled.load(Ordering::Acquire) {
            Some(RustGraphAnalysisError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Some(RustGraphAnalysisError::DeadlineExceeded)
        } else {
            None
        }
    }
}

impl fmt::Debug for RustGraphAnalysisControl<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphAnalysisControl")
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Stable artifact-local Rust graph-site categories.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RustGraphSiteKind {
    /// One exact `use` argument.
    Import,
    /// A precision-first identifier, type-path, or field reference candidate.
    Reference,
    /// The function expression of one call expression.
    Call,
    /// The macro path of one macro invocation.
    MacroCall,
    /// A direct or conditional test marker.
    TestMarker,
}

impl RustGraphSiteKind {
    /// Returns the stable persistence spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Reference => "reference",
            Self::Call => "call",
            Self::MacroCall => "macro_call",
            Self::TestMarker => "test_marker",
        }
    }

    /// Decodes an exact stable persistence spelling.
    #[must_use]
    pub fn from_stable_str(value: &str) -> Option<Self> {
        match value {
            "import" => Some(Self::Import),
            "reference" => Some(Self::Reference),
            "call" => Some(Self::Call),
            "macro_call" => Some(Self::MacroCall),
            "test_marker" => Some(Self::TestMarker),
            _ => None,
        }
    }
}

/// Evidence supporting one raw site classification.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RustGraphSiteEvidence {
    /// The pinned grammar directly identifies the construct.
    DirectSyntax,
    /// Bounded syntax inspection conservatively classifies a candidate.
    SyntaxHeuristic,
}

impl RustGraphSiteEvidence {
    /// Returns the stable persistence spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DirectSyntax => "direct_syntax",
            Self::SyntaxHeuristic => "syntax_heuristic",
        }
    }

    /// Decodes an exact stable persistence spelling.
    #[must_use]
    pub fn from_stable_str(value: &str) -> Option<Self> {
        match value {
            "direct_syntax" => Some(Self::DirectSyntax),
            "syntax_heuristic" => Some(Self::SyntaxHeuristic),
            _ => None,
        }
    }
}

/// Zero-based source-order occurrence identity within one graph-site artifact.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RustGraphSiteOrdinal(u32);

impl RustGraphSiteOrdinal {
    /// Creates an artifact-local source-order ordinal.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the fixed-width source-order value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Artifact-local descriptor of the declaration enclosing one raw site.
#[derive(Clone, Eq, PartialEq)]
pub struct RustGraphEnclosingDefinition {
    kind: RustSymbolKind,
    name: String,
    qualified_name: String,
    name_span: ByteSpan,
    declaration_span: ByteSpan,
}

impl RustGraphEnclosingDefinition {
    /// Reconstructs one validated descriptor at a persistence boundary.
    pub fn try_new(
        kind: RustSymbolKind,
        name: String,
        qualified_name: String,
        name_span: ByteSpan,
        declaration_span: ByteSpan,
        limits: RustGraphAnalysisLimits,
    ) -> Result<Self, RustGraphAnalysisError> {
        if !limits.is_valid()
            || name.is_empty()
            || name.len() > usize::from(limits.max_name_bytes())
        {
            return Err(RustGraphAnalysisError::NameLimitExceeded);
        }
        if qualified_name.is_empty() || qualified_name.len() > usize::from(limits.max_path_bytes())
        {
            return Err(RustGraphAnalysisError::PathLimitExceeded);
        }
        if name_span.start().get() < declaration_span.start().get()
            || name_span.end().get() > declaration_span.end().get()
            || name_span.end().get() > limits.max_source_bytes()
            || declaration_span.end().get() > limits.max_source_bytes()
            || name_span.end().get() - name_span.start().get()
                != u64::try_from(name.len())
                    .map_err(|_| RustGraphAnalysisError::NameLimitExceeded)?
        {
            return Err(RustGraphAnalysisError::InvalidSourceSpan);
        }
        Ok(Self {
            kind,
            name,
            qualified_name,
            name_span,
            declaration_span,
        })
    }

    /// Returns the existing stable declaration category.
    #[must_use]
    pub const fn kind(&self) -> RustSymbolKind {
        self.kind
    }

    /// Returns the exact declaration name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the deterministic syntax-qualified declaration path.
    #[must_use]
    pub fn qualified_name(&self) -> &str {
        &self.qualified_name
    }

    /// Returns the exact name span.
    #[must_use]
    pub const fn name_span(&self) -> ByteSpan {
        self.name_span
    }

    /// Returns the exact complete declaration span.
    #[must_use]
    pub const fn declaration_span(&self) -> ByteSpan {
        self.declaration_span
    }
}

impl fmt::Debug for RustGraphEnclosingDefinition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphEnclosingDefinition")
            .field("kind", &self.kind)
            .field("name", &"<redacted>")
            .field("qualified_name", &"<redacted>")
            .field("name_span", &self.name_span)
            .field("declaration_span", &self.declaration_span)
            .finish()
    }
}

/// One exact unresolved graph-site occurrence from an immutable source input.
#[derive(Clone, Eq, PartialEq)]
pub struct RustGraphSite {
    ordinal: RustGraphSiteOrdinal,
    kind: RustGraphSiteKind,
    evidence: RustGraphSiteEvidence,
    occurrence_span: ByteSpan,
    target_span: ByteSpan,
    raw_target: String,
    enclosing_definition: Option<RustGraphEnclosingDefinition>,
}

impl RustGraphSite {
    /// Reconstructs one validated raw site at a persistence boundary.
    #[allow(
        clippy::too_many_arguments,
        reason = "all exact site fields participate in the validated artifact"
    )]
    pub fn try_new(
        ordinal: RustGraphSiteOrdinal,
        kind: RustGraphSiteKind,
        evidence: RustGraphSiteEvidence,
        occurrence_span: ByteSpan,
        target_span: ByteSpan,
        raw_target: String,
        enclosing_definition: Option<RustGraphEnclosingDefinition>,
        limits: RustGraphAnalysisLimits,
    ) -> Result<Self, RustGraphAnalysisError> {
        if !limits.is_valid()
            || raw_target.is_empty()
            || raw_target.len() > usize::from(limits.max_path_bytes())
        {
            return Err(RustGraphAnalysisError::PathLimitExceeded);
        }
        if target_span.start().get() < occurrence_span.start().get()
            || target_span.end().get() > occurrence_span.end().get()
            || occurrence_span.end().get() > limits.max_source_bytes()
            || target_span.end().get() - target_span.start().get()
                != u64::try_from(raw_target.len())
                    .map_err(|_| RustGraphAnalysisError::PathLimitExceeded)?
        {
            return Err(RustGraphAnalysisError::InvalidSourceSpan);
        }
        Ok(Self {
            ordinal,
            kind,
            evidence,
            occurrence_span,
            target_span,
            raw_target,
            enclosing_definition,
        })
    }

    /// Returns the source-order artifact-local ordinal.
    #[must_use]
    pub const fn ordinal(&self) -> RustGraphSiteOrdinal {
        self.ordinal
    }

    /// Returns the stable raw-site category.
    #[must_use]
    pub const fn kind(&self) -> RustGraphSiteKind {
        self.kind
    }

    /// Returns the evidence supporting only this raw classification.
    #[must_use]
    pub const fn evidence(&self) -> RustGraphSiteEvidence {
        self.evidence
    }

    /// Returns the exact complete construct span.
    #[must_use]
    pub const fn occurrence_span(&self) -> ByteSpan {
        self.occurrence_span
    }

    /// Returns the exact raw-target span.
    #[must_use]
    pub const fn target_span(&self) -> ByteSpan {
        self.target_span
    }

    /// Returns the exact UTF-8 target spelling at [`Self::target_span`].
    #[must_use]
    pub fn raw_target(&self) -> &str {
        &self.raw_target
    }

    /// Returns an artifact-local enclosing declaration descriptor, when any.
    #[must_use]
    pub const fn enclosing_definition(&self) -> Option<&RustGraphEnclosingDefinition> {
        self.enclosing_definition.as_ref()
    }
}

impl fmt::Debug for RustGraphSite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphSite")
            .field("ordinal", &self.ordinal)
            .field("kind", &self.kind)
            .field("evidence", &self.evidence)
            .field("occurrence_span", &self.occurrence_span)
            .field("target_span", &self.target_span)
            .field("raw_target", &"<redacted>")
            .field("enclosing_definition", &self.enclosing_definition)
            .finish()
    }
}

/// Complete bounded graph-site output for one immutable source input.
#[derive(Clone, Eq, PartialEq)]
pub struct RustGraphSiteAnalysis {
    sites: Vec<RustGraphSite>,
    visited_nodes: u32,
    syntax_error_nodes: u32,
    max_observed_depth: u16,
    owned_text_bytes: u64,
}

impl RustGraphSiteAnalysis {
    /// Returns raw sites in deterministic source order.
    #[must_use]
    pub fn sites(&self) -> &[RustGraphSite] {
        &self.sites
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

    /// Returns the deepest zero-based syntax depth visited.
    #[must_use]
    pub const fn max_observed_depth(&self) -> u16 {
        self.max_observed_depth
    }

    /// Returns aggregate bytes owned by target and enclosing-descriptor text.
    #[must_use]
    pub const fn owned_text_bytes(&self) -> u64 {
        self.owned_text_bytes
    }

    /// Reports whether Tree-sitter returned malformed or incomplete syntax.
    #[must_use]
    pub const fn has_syntax_errors(&self) -> bool {
        self.syntax_error_nodes != 0
    }
}

impl fmt::Debug for RustGraphSiteAnalysis {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphSiteAnalysis")
            .field("sites", &self.sites)
            .field("visited_nodes", &self.visited_nodes)
            .field("syntax_error_nodes", &self.syntax_error_nodes)
            .field("max_observed_depth", &self.max_observed_depth)
            .field("owned_text_bytes", &self.owned_text_bytes)
            .finish()
    }
}

/// Stable redacted failure from bounded Rust graph-site analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustGraphAnalysisError {
    /// A limit is zero or exceeds its compiled hard ceiling.
    InvalidLimits,
    /// The immutable source exceeds its byte limit.
    SourceLimitExceeded,
    /// Rust source is not valid UTF-8.
    InvalidSourceEncoding,
    /// The pinned Rust grammar could not be loaded.
    GrammarUnavailable,
    /// Tree-sitter stopped without a cancellation or deadline reason.
    ParseFailed,
    /// Cancellation was observed before complete output existed.
    Cancelled,
    /// The absolute monotonic deadline elapsed.
    DeadlineExceeded,
    /// Syntax traversal exceeded its node limit.
    NodeLimitExceeded,
    /// Syntax traversal exceeded its depth limit.
    DepthLimitExceeded,
    /// Raw-site extraction exceeded its result limit.
    SiteLimitExceeded,
    /// An enclosing declaration name exceeded its byte limit.
    NameLimitExceeded,
    /// A raw target or qualified declaration path exceeded its byte limit.
    PathLimitExceeded,
    /// Aggregate owned target and descriptor text exceeded its byte limit.
    OwnedTextLimitExceeded,
    /// Tree-sitter returned an invalid or inconsistent source span.
    InvalidSourceSpan,
    /// A required syntax field was absent from a non-error node.
    InvalidSyntaxShape,
    /// Reconstructed artifact metadata and rows are internally inconsistent.
    InvalidAnalysisShape,
}

impl fmt::Display for RustGraphAnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLimits => "Rust graph analysis limits are invalid",
            Self::SourceLimitExceeded => "Rust graph source byte limit exceeded",
            Self::InvalidSourceEncoding => "Rust graph source encoding is invalid",
            Self::GrammarUnavailable => "Rust graph grammar is unavailable",
            Self::ParseFailed => "Rust graph source parsing failed",
            Self::Cancelled => "Rust graph analysis cancelled",
            Self::DeadlineExceeded => "Rust graph analysis deadline exceeded",
            Self::NodeLimitExceeded => "Rust graph syntax node limit exceeded",
            Self::DepthLimitExceeded => "Rust graph syntax depth limit exceeded",
            Self::SiteLimitExceeded => "Rust graph site limit exceeded",
            Self::NameLimitExceeded => "Rust graph name limit exceeded",
            Self::PathLimitExceeded => "Rust graph path limit exceeded",
            Self::OwnedTextLimitExceeded => "Rust graph owned text limit exceeded",
            Self::InvalidSourceSpan => "Rust graph parser returned an invalid source span",
            Self::InvalidSyntaxShape => "Rust graph parser returned an invalid syntax shape",
            Self::InvalidAnalysisShape => "Rust graph analysis shape is invalid",
        })
    }
}

impl Error for RustGraphAnalysisError {}

/// Reusable owner of one pinned Tree-sitter Rust parser.
pub struct RustGraphSiteAnalyzer {
    parser: Parser,
}

mod extraction;
mod reconstruction;
mod resolution;
mod traversal;

include!("rust_graph/analyzer.rs");

pub use resolution::{
    RUST_GRAPH_RESOLVER_PROFILE_VERSION, RustGraphDefinitionIdentity,
    RustGraphDefinitionOccurrence, RustGraphResolution, RustGraphResolutionCandidate,
    RustGraphResolutionControl, RustGraphResolutionCoverage, RustGraphResolutionError,
    RustGraphResolutionEvidence, RustGraphResolutionLimits, RustGraphResolutionOutcome,
    RustGraphSiteIdentity, RustGraphSiteOccurrence, RustGraphSiteResolution,
    RustGraphUnresolvedReason, resolve_rust_graph_sites,
};
pub use traversal::{
    RUST_GRAPH_TRAVERSAL_PROFILE_VERSION, RustGraphEdgeKind, RustGraphEdgeKinds, RustGraphImpact,
    RustGraphImpactClass, RustGraphImpactRequest, RustGraphImpactResult,
    RustGraphRelationshipCardinality, RustGraphTraceControl, RustGraphTraceCoverage,
    RustGraphTraceDirection, RustGraphTraceEdge, RustGraphTraceError, RustGraphTraceLimits,
    RustGraphTraceRequest, RustGraphTraceResult, RustGraphTraceStart, RustGraphTraceTruncation,
    RustGraphTraversalEdge, analyze_rust_graph_impact, trace_rust_graph,
};

#[cfg(test)]
mod tests;
