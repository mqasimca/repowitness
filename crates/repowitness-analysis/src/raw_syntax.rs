//! Bounded, language-neutral raw syntax-site extraction.
//!
//! This product is deliberately separate from the Rust graph.  It records
//! exact syntax observations from one immutable source blob; it does not read
//! files, resolve names, create edges, or imply that a target is a declaration.

use std::{
    error::Error,
    fmt,
    ops::ControlFlow,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use repowitness_domain::{ByteOffset, ByteSpan};
use tree_sitter::{Node, ParseOptions, Parser};

use crate::{
    TypeScriptDialect, go_grammar_fingerprint_input, python_grammar_fingerprint_input,
    rust_grammar_fingerprint_input, typescript_grammar_fingerprint_input,
};

/// Version of the all-language, artifact-local raw syntax-site behavior.
pub const RAW_SYNTAX_SITE_PROFILE_VERSION: u32 = 1;

const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SYNTAX_NODES: u32 = 1_000_000;
const MAX_SYNTAX_DEPTH: u16 = 256;
const MAX_SITES: u32 = 250_000;
const MAX_TARGET_BYTES: u16 = 16_384;
const MAX_OWNED_TEXT_BYTES: u64 = 64 * 1024 * 1024;

/// Returns exact first-party implementation bytes for producer fingerprinting.
#[must_use]
pub fn raw_syntax_site_implementation_fingerprint_input() -> &'static [u8] {
    include_bytes!("raw_syntax.rs")
}

/// One pinned syntax grammar/dialect supported by this extractor.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RawSyntaxLanguage {
    /// Rust source.
    Rust,
    /// Go source.
    Go,
    /// Plain TypeScript source.
    TypeScript,
    /// JSX-aware TypeScript source.
    Tsx,
    /// Python source.
    Python,
}

impl RawSyntaxLanguage {
    /// Returns the stable persistence spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Go => "go",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::Python => "python",
        }
    }

    /// Decodes an exact persistence spelling.
    #[must_use]
    pub fn from_stable_str(value: &str) -> Option<Self> {
        match value {
            "rust" => Some(Self::Rust),
            "go" => Some(Self::Go),
            "typescript" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "python" => Some(Self::Python),
            _ => None,
        }
    }

    const fn type_script_dialect(self) -> Option<TypeScriptDialect> {
        match self {
            Self::TypeScript => Some(TypeScriptDialect::TypeScript),
            Self::Tsx => Some(TypeScriptDialect::Tsx),
            _ => None,
        }
    }
}

/// Returns the exact pinned grammar fingerprint input for `language`.
#[must_use]
pub fn raw_syntax_grammar_fingerprint_input(language: RawSyntaxLanguage) -> &'static [u8] {
    match language {
        RawSyntaxLanguage::Rust => rust_grammar_fingerprint_input(),
        RawSyntaxLanguage::Go => go_grammar_fingerprint_input(),
        RawSyntaxLanguage::TypeScript | RawSyntaxLanguage::Tsx => {
            typescript_grammar_fingerprint_input(
                language
                    .type_script_dialect()
                    .expect("TypeScript variants have a dialect"),
            )
        }
        RawSyntaxLanguage::Python => python_grammar_fingerprint_input(),
    }
}

/// Stable syntax categories.  They describe occurrence shape, never resolution.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RawSyntaxSiteKind {
    /// A module/package import target.
    Import,
    /// A precision-first syntax reference candidate.
    Reference,
    /// A function/callable expression of one invocation.
    Call,
    /// A language-level test marker.
    TestMarker,
}

impl RawSyntaxSiteKind {
    /// Returns the stable persistence spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Reference => "reference",
            Self::Call => "call",
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
            "test_marker" => Some(Self::TestMarker),
            _ => None,
        }
    }
}

/// Evidence for a raw syntax classification, not for a target association.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RawSyntaxSiteEvidence {
    /// The pinned grammar directly identifies the site shape.
    DirectSyntax,
    /// Bounded syntax inspection classified a conservative candidate.
    SyntaxHeuristic,
}

impl RawSyntaxSiteEvidence {
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

/// Artifact-local zero-based source-order occurrence identity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RawSyntaxSiteOrdinal(u32);

impl RawSyntaxSiteOrdinal {
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

/// One exact, unresolved raw occurrence in an immutable source blob.
#[derive(Clone, Eq, PartialEq)]
pub struct RawSyntaxSite {
    ordinal: RawSyntaxSiteOrdinal,
    kind: RawSyntaxSiteKind,
    evidence: RawSyntaxSiteEvidence,
    occurrence_span: ByteSpan,
    target_span: ByteSpan,
    raw_target: String,
}

impl RawSyntaxSite {
    /// Reconstructs one shape-validated artifact-local site.
    #[allow(
        clippy::too_many_arguments,
        reason = "all persisted fields are validated together at this boundary"
    )]
    pub fn try_new(
        ordinal: RawSyntaxSiteOrdinal,
        kind: RawSyntaxSiteKind,
        evidence: RawSyntaxSiteEvidence,
        occurrence_span: ByteSpan,
        target_span: ByteSpan,
        raw_target: String,
        limits: RawSyntaxSiteAnalysisLimits,
    ) -> Result<Self, RawSyntaxSiteAnalysisError> {
        if !limits.is_valid()
            || raw_target.is_empty()
            || raw_target.len() > usize::from(limits.max_target_bytes())
        {
            return Err(RawSyntaxSiteAnalysisError::TargetLimitExceeded);
        }
        if target_span.start().get() < occurrence_span.start().get()
            || target_span.end().get() > occurrence_span.end().get()
            || occurrence_span.end().get() > limits.max_source_bytes()
            || target_span.end().get() - target_span.start().get()
                != u64::try_from(raw_target.len())
                    .map_err(|_| RawSyntaxSiteAnalysisError::TargetLimitExceeded)?
        {
            return Err(RawSyntaxSiteAnalysisError::InvalidSourceSpan);
        }
        Ok(Self {
            ordinal,
            kind,
            evidence,
            occurrence_span,
            target_span,
            raw_target,
        })
    }

    /// Returns this artifact-local source-order ordinal.
    #[must_use]
    pub const fn ordinal(&self) -> RawSyntaxSiteOrdinal {
        self.ordinal
    }

    /// Returns the raw syntax category.
    #[must_use]
    pub const fn kind(&self) -> RawSyntaxSiteKind {
        self.kind
    }

    /// Returns only the supporting extraction evidence.
    #[must_use]
    pub const fn evidence(&self) -> RawSyntaxSiteEvidence {
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
}

impl fmt::Debug for RawSyntaxSite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawSyntaxSite")
            .field("ordinal", &self.ordinal)
            .field("kind", &self.kind)
            .field("evidence", &self.evidence)
            .field("occurrence_span", &self.occurrence_span)
            .field("target_span", &self.target_span)
            .field("raw_target", &"<redacted>")
            .finish()
    }
}

/// Whether a site class is intentionally supported for one grammar profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawSyntaxSiteSupport {
    /// The extractor can emit this category under the current profile.
    Available,
    /// The category is deliberately not emitted without a precision-safe rule.
    Unsupported,
}

/// Per-kind coverage for one completed parser run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawSyntaxSiteKindCoverage {
    support: RawSyntaxSiteSupport,
    emitted: u32,
}

impl RawSyntaxSiteKindCoverage {
    /// Returns the explicit support state.
    #[must_use]
    pub const fn support(self) -> RawSyntaxSiteSupport {
        self.support
    }

    /// Returns the number of emitted observations, not a semantic absence claim.
    #[must_use]
    pub const fn emitted(self) -> u32 {
        self.emitted
    }
}

/// Categorical parser and kind coverage accompanying one raw-site artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawSyntaxSiteCoverage {
    import: RawSyntaxSiteKindCoverage,
    reference: RawSyntaxSiteKindCoverage,
    call: RawSyntaxSiteKindCoverage,
    test_marker: RawSyntaxSiteKindCoverage,
}

impl RawSyntaxSiteCoverage {
    /// Returns coverage for `kind`.
    #[must_use]
    pub const fn for_kind(self, kind: RawSyntaxSiteKind) -> RawSyntaxSiteKindCoverage {
        match kind {
            RawSyntaxSiteKind::Import => self.import,
            RawSyntaxSiteKind::Reference => self.reference,
            RawSyntaxSiteKind::Call => self.call,
            RawSyntaxSiteKind::TestMarker => self.test_marker,
        }
    }
}

/// Complete bounded output for one immutable source blob.
#[derive(Clone, Eq, PartialEq)]
pub struct RawSyntaxSiteAnalysis {
    language: RawSyntaxLanguage,
    sites: Vec<RawSyntaxSite>,
    visited_nodes: u32,
    syntax_error_nodes: u32,
    max_observed_depth: u16,
    owned_text_bytes: u64,
    coverage: RawSyntaxSiteCoverage,
}

impl RawSyntaxSiteAnalysis {
    /// Reconstructs a complete persisted analysis after validating every bound and count.
    ///
    /// This is intentionally separate from parsing: callers must already have verified the
    /// artifact identity and canonical payload digest before accepting this reconstructed value.
    #[allow(
        clippy::too_many_arguments,
        reason = "the complete persisted analysis shape is validated atomically at this trust boundary"
    )]
    pub fn try_from_parts_with_control(
        language: RawSyntaxLanguage,
        sites: Vec<RawSyntaxSite>,
        visited_nodes: u32,
        syntax_error_nodes: u32,
        max_observed_depth: u16,
        owned_text_bytes: u64,
        limits: RawSyntaxSiteAnalysisLimits,
        control: RawSyntaxSiteAnalysisControl<'_>,
    ) -> Result<Self, RawSyntaxSiteAnalysisError> {
        if !limits.is_valid()
            || visited_nodes > limits.max_syntax_nodes()
            || syntax_error_nodes > visited_nodes
            || max_observed_depth > limits.max_syntax_depth()
            || owned_text_bytes > limits.max_owned_text_bytes()
            || usize::try_from(limits.max_sites()).ok() < Some(sites.len())
        {
            return Err(RawSyntaxSiteAnalysisError::InvalidSourceSpan);
        }
        let mut emitted = [0_u32; 4];
        let mut observed_owned_text_bytes = 0_u64;
        for (index, site) in sites.iter().enumerate() {
            if let Some(outcome) = control.outcome() {
                return Err(outcome);
            }
            if site.ordinal().get()
                != u32::try_from(index)
                    .map_err(|_| RawSyntaxSiteAnalysisError::SiteLimitExceeded)?
            {
                return Err(RawSyntaxSiteAnalysisError::InvalidSourceSpan);
            }
            let validated = RawSyntaxSite::try_new(
                site.ordinal(),
                site.kind(),
                site.evidence(),
                site.occurrence_span(),
                site.target_span(),
                site.raw_target().to_owned(),
                limits,
            )?;
            if &validated != site {
                return Err(RawSyntaxSiteAnalysisError::InvalidSourceSpan);
            }
            observed_owned_text_bytes = observed_owned_text_bytes
                .checked_add(
                    u64::try_from(site.raw_target().len())
                        .map_err(|_| RawSyntaxSiteAnalysisError::OwnedTextLimitExceeded)?,
                )
                .ok_or(RawSyntaxSiteAnalysisError::OwnedTextLimitExceeded)?;
            let counter = match site.kind() {
                RawSyntaxSiteKind::Import => &mut emitted[0],
                RawSyntaxSiteKind::Reference => &mut emitted[1],
                RawSyntaxSiteKind::Call => &mut emitted[2],
                RawSyntaxSiteKind::TestMarker => &mut emitted[3],
            };
            *counter = counter
                .checked_add(1)
                .ok_or(RawSyntaxSiteAnalysisError::SiteLimitExceeded)?;
        }
        if observed_owned_text_bytes != owned_text_bytes {
            return Err(RawSyntaxSiteAnalysisError::InvalidSourceSpan);
        }
        if let Some(outcome) = control.outcome() {
            return Err(outcome);
        }
        Ok(Self {
            language,
            sites,
            visited_nodes,
            syntax_error_nodes,
            max_observed_depth,
            owned_text_bytes,
            coverage: coverage(language, emitted),
        })
    }

    /// Returns the grammar/dialect that produced this immutable artifact.
    #[must_use]
    pub const fn language(&self) -> RawSyntaxLanguage {
        self.language
    }

    /// Returns raw occurrences in deterministic source order.
    #[must_use]
    pub fn sites(&self) -> &[RawSyntaxSite] {
        &self.sites
    }

    /// Returns exact parser coverage and explicitly unsupported categories.
    #[must_use]
    pub const fn coverage(&self) -> RawSyntaxSiteCoverage {
        self.coverage
    }

    /// Returns the number of traversed syntax nodes.
    #[must_use]
    pub const fn visited_nodes(&self) -> u32 {
        self.visited_nodes
    }

    /// Returns explicit Tree-sitter error/missing nodes.
    #[must_use]
    pub const fn syntax_error_nodes(&self) -> u32 {
        self.syntax_error_nodes
    }

    /// Returns deepest zero-based syntax depth visited.
    #[must_use]
    pub const fn max_observed_depth(&self) -> u16 {
        self.max_observed_depth
    }

    /// Returns aggregate bytes owned by raw targets.
    #[must_use]
    pub const fn owned_text_bytes(&self) -> u64 {
        self.owned_text_bytes
    }

    /// Reports malformed or incomplete parser output categorically.
    #[must_use]
    pub const fn has_syntax_errors(&self) -> bool {
        self.syntax_error_nodes != 0
    }
}

impl fmt::Debug for RawSyntaxSiteAnalysis {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawSyntaxSiteAnalysis")
            .field("language", &self.language)
            .field("sites", &self.sites)
            .field("visited_nodes", &self.visited_nodes)
            .field("syntax_error_nodes", &self.syntax_error_nodes)
            .field("max_observed_depth", &self.max_observed_depth)
            .field("owned_text_bytes", &self.owned_text_bytes)
            .field("coverage", &self.coverage)
            .finish()
    }
}

/// Explicit bounds for one raw syntax-site analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawSyntaxSiteAnalysisLimits {
    max_source_bytes: u64,
    max_syntax_nodes: u32,
    max_syntax_depth: u16,
    max_sites: u32,
    max_target_bytes: u16,
    max_owned_text_bytes: u64,
}

impl RawSyntaxSiteAnalysisLimits {
    /// Conservative hard-bounded defaults.
    pub const DEFAULT: Self = Self {
        max_source_bytes: MAX_SOURCE_BYTES,
        max_syntax_nodes: MAX_SYNTAX_NODES,
        max_syntax_depth: MAX_SYNTAX_DEPTH,
        max_sites: MAX_SITES,
        max_target_bytes: MAX_TARGET_BYTES,
        max_owned_text_bytes: MAX_OWNED_TEXT_BYTES,
    };

    /// Creates positive limits no larger than the compiled ceilings.
    pub fn try_new(
        max_source_bytes: u64,
        max_syntax_nodes: u32,
        max_syntax_depth: u16,
        max_sites: u32,
        max_target_bytes: u16,
        max_owned_text_bytes: u64,
    ) -> Result<Self, RawSyntaxSiteAnalysisError> {
        let limits = Self {
            max_source_bytes,
            max_syntax_nodes,
            max_syntax_depth,
            max_sites,
            max_target_bytes,
            max_owned_text_bytes,
        };
        limits
            .is_valid()
            .then_some(limits)
            .ok_or(RawSyntaxSiteAnalysisError::InvalidLimits)
    }

    const fn is_valid(self) -> bool {
        self.max_source_bytes != 0
            && self.max_source_bytes <= MAX_SOURCE_BYTES
            && self.max_syntax_nodes != 0
            && self.max_syntax_nodes <= MAX_SYNTAX_NODES
            && self.max_syntax_depth != 0
            && self.max_syntax_depth <= MAX_SYNTAX_DEPTH
            && self.max_sites != 0
            && self.max_sites <= MAX_SITES
            && self.max_target_bytes != 0
            && self.max_target_bytes <= MAX_TARGET_BYTES
            && self.max_owned_text_bytes != 0
            && self.max_owned_text_bytes <= MAX_OWNED_TEXT_BYTES
    }

    /// Returns the immutable source-byte limit.
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
    /// Returns the emitted-site limit.
    #[must_use]
    pub const fn max_sites(self) -> u32 {
        self.max_sites
    }
    /// Returns the raw-target byte limit.
    #[must_use]
    pub const fn max_target_bytes(self) -> u16 {
        self.max_target_bytes
    }
    /// Returns aggregate owned raw-target byte limit.
    #[must_use]
    pub const fn max_owned_text_bytes(self) -> u64 {
        self.max_owned_text_bytes
    }
}

impl Default for RawSyntaxSiteAnalysisLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Cooperative cancellation and monotonic deadline for one analysis.
#[derive(Clone, Copy)]
pub struct RawSyntaxSiteAnalysisControl<'a> {
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

impl<'a> RawSyntaxSiteAnalysisControl<'a> {
    /// Creates control state from cancellation and a monotonic deadline.
    #[must_use]
    pub const fn new(cancelled: &'a AtomicBool, deadline: Instant) -> Self {
        Self {
            cancelled,
            deadline,
        }
    }

    fn outcome(self) -> Option<RawSyntaxSiteAnalysisError> {
        if self.cancelled.load(Ordering::Acquire) {
            Some(RawSyntaxSiteAnalysisError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Some(RawSyntaxSiteAnalysisError::DeadlineExceeded)
        } else {
            None
        }
    }
}

impl fmt::Debug for RawSyntaxSiteAnalysisControl<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawSyntaxSiteAnalysisControl")
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Stable redacted failure from raw syntax-site analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawSyntaxSiteAnalysisError {
    /// Limits are zero, contradictory, or exceed fixed ceilings.
    InvalidLimits,
    /// The pinned grammar could not be initialized.
    GrammarUnavailable,
    /// The immutable source is larger than the configured bound.
    SourceLimitExceeded,
    /// Raw target text cannot be represented under the configured bound.
    TargetLimitExceeded,
    /// Traversal would exceed its node budget.
    NodeLimitExceeded,
    /// Traversal would exceed its depth budget.
    DepthLimitExceeded,
    /// Emission would exceed its site budget.
    SiteLimitExceeded,
    /// Aggregate owned target text would exceed its budget.
    OwnedTextLimitExceeded,
    /// A selected raw target slice is not valid UTF-8.
    InvalidSourceEncoding,
    /// Tree-sitter returned no complete parse tree.
    ParseFailed,
    /// A grammar node or source span was internally inconsistent.
    InvalidSourceSpan,
    /// Cancellation was observed before complete output existed.
    Cancelled,
    /// The absolute monotonic deadline elapsed.
    DeadlineExceeded,
}

impl fmt::Display for RawSyntaxSiteAnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLimits => "raw syntax-site limits are invalid",
            Self::GrammarUnavailable => "raw syntax-site grammar is unavailable",
            Self::SourceLimitExceeded => "raw syntax-site source limit exceeded",
            Self::TargetLimitExceeded => "raw syntax-site target limit exceeded",
            Self::NodeLimitExceeded => "raw syntax-site node limit exceeded",
            Self::DepthLimitExceeded => "raw syntax-site depth limit exceeded",
            Self::SiteLimitExceeded => "raw syntax-site count limit exceeded",
            Self::OwnedTextLimitExceeded => "raw syntax-site owned text limit exceeded",
            Self::InvalidSourceEncoding => "raw syntax-site source encoding is invalid",
            Self::ParseFailed => "raw syntax-site parsing failed",
            Self::InvalidSourceSpan => "raw syntax-site source span is invalid",
            Self::Cancelled => "raw syntax-site analysis cancelled",
            Self::DeadlineExceeded => "raw syntax-site analysis deadline exceeded",
        })
    }
}

impl Error for RawSyntaxSiteAnalysisError {}

/// Reusable owner of one pinned language parser.
pub struct RawSyntaxSiteAnalyzer {
    language: RawSyntaxLanguage,
    parser: Parser,
}

impl RawSyntaxSiteAnalyzer {
    /// Creates an analyzer using the selected pinned grammar/dialect.
    pub fn new(language: RawSyntaxLanguage) -> Result<Self, RawSyntaxSiteAnalysisError> {
        let mut parser = Parser::new();
        let grammar = match language {
            RawSyntaxLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
            RawSyntaxLanguage::Go => tree_sitter_go::LANGUAGE.into(),
            RawSyntaxLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            RawSyntaxLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            RawSyntaxLanguage::Python => tree_sitter_python::LANGUAGE.into(),
        };
        parser
            .set_language(&grammar)
            .map_err(|_| RawSyntaxSiteAnalysisError::GrammarUnavailable)?;
        Ok(Self { language, parser })
    }

    /// Extracts complete raw syntax sites without filesystem or database I/O.
    pub fn analyze(
        &mut self,
        source: &[u8],
        limits: RawSyntaxSiteAnalysisLimits,
        control: RawSyntaxSiteAnalysisControl<'_>,
    ) -> Result<RawSyntaxSiteAnalysis, RawSyntaxSiteAnalysisError> {
        admit_source(source, limits, control)?;
        let tree = parse_source(&mut self.parser, source, control)?;
        traverse_sites(self.language, &tree, source, limits, control)
    }
}

impl fmt::Debug for RawSyntaxSiteAnalyzer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawSyntaxSiteAnalyzer")
            .field("language", &self.language)
            .finish_non_exhaustive()
    }
}

fn admit_source(
    source: &[u8],
    limits: RawSyntaxSiteAnalysisLimits,
    control: RawSyntaxSiteAnalysisControl<'_>,
) -> Result<(), RawSyntaxSiteAnalysisError> {
    if let Some(outcome) = control.outcome() {
        return Err(outcome);
    }
    let bytes =
        u64::try_from(source.len()).map_err(|_| RawSyntaxSiteAnalysisError::SourceLimitExceeded)?;
    if bytes > limits.max_source_bytes() {
        return Err(RawSyntaxSiteAnalysisError::SourceLimitExceeded);
    }
    Ok(())
}

fn parse_source(
    parser: &mut Parser,
    source: &[u8],
    control: RawSyntaxSiteAnalysisControl<'_>,
) -> Result<tree_sitter::Tree, RawSyntaxSiteAnalysisError> {
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
    let tree = parser.parse_with_options(
        &mut read,
        None,
        Some(ParseOptions::new().progress_callback(&mut progress)),
    );
    if let Some(outcome) = interrupted {
        parser.reset();
        return Err(outcome);
    }
    tree.ok_or_else(|| {
        parser.reset();
        RawSyntaxSiteAnalysisError::ParseFailed
    })
}

fn traverse_sites(
    language: RawSyntaxLanguage,
    tree: &tree_sitter::Tree,
    source: &[u8],
    limits: RawSyntaxSiteAnalysisLimits,
    control: RawSyntaxSiteAnalysisControl<'_>,
) -> Result<RawSyntaxSiteAnalysis, RawSyntaxSiteAnalysisError> {
    let mut state = TraversalState::default();
    let mut cursor = tree.walk();
    loop {
        state.visit(language, cursor.node(), source, limits, control)?;
        if cursor.goto_first_child() {
            state.descend(limits)?;
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return state.finish(language, source, limits, control);
            }
            state.ascend()?;
        }
    }
}

#[derive(Default)]
struct TraversalState {
    sites: Vec<RawSyntaxSite>,
    visited_nodes: u32,
    syntax_error_nodes: u32,
    depth: u16,
    max_observed_depth: u16,
    owned_text_bytes: u64,
}

impl TraversalState {
    fn visit(
        &mut self,
        language: RawSyntaxLanguage,
        node: Node<'_>,
        source: &[u8],
        limits: RawSyntaxSiteAnalysisLimits,
        control: RawSyntaxSiteAnalysisControl<'_>,
    ) -> Result<(), RawSyntaxSiteAnalysisError> {
        if let Some(outcome) = control.outcome() {
            return Err(outcome);
        }
        self.visited_nodes = self
            .visited_nodes
            .checked_add(1)
            .ok_or(RawSyntaxSiteAnalysisError::NodeLimitExceeded)?;
        if self.visited_nodes > limits.max_syntax_nodes() {
            return Err(RawSyntaxSiteAnalysisError::NodeLimitExceeded);
        }
        if node.is_error() || node.is_missing() {
            self.syntax_error_nodes = self.syntax_error_nodes.saturating_add(1);
            return Ok(());
        }
        let sites = extract_sites(language, node, source, limits)?;
        for site in sites {
            let max_sites = usize::try_from(limits.max_sites())
                .map_err(|_| RawSyntaxSiteAnalysisError::SiteLimitExceeded)?;
            if self.sites.len() >= max_sites {
                return Err(RawSyntaxSiteAnalysisError::SiteLimitExceeded);
            }
            self.owned_text_bytes = self
                .owned_text_bytes
                .checked_add(
                    u64::try_from(site.raw_target.len())
                        .map_err(|_| RawSyntaxSiteAnalysisError::OwnedTextLimitExceeded)?,
                )
                .ok_or(RawSyntaxSiteAnalysisError::OwnedTextLimitExceeded)?;
            if self.owned_text_bytes > limits.max_owned_text_bytes() {
                return Err(RawSyntaxSiteAnalysisError::OwnedTextLimitExceeded);
            }
            self.sites.push(site);
        }
        Ok(())
    }

    fn descend(
        &mut self,
        limits: RawSyntaxSiteAnalysisLimits,
    ) -> Result<(), RawSyntaxSiteAnalysisError> {
        self.depth = self
            .depth
            .checked_add(1)
            .ok_or(RawSyntaxSiteAnalysisError::DepthLimitExceeded)?;
        if self.depth > limits.max_syntax_depth() {
            return Err(RawSyntaxSiteAnalysisError::DepthLimitExceeded);
        }
        self.max_observed_depth = self.max_observed_depth.max(self.depth);
        Ok(())
    }

    fn ascend(&mut self) -> Result<(), RawSyntaxSiteAnalysisError> {
        self.depth = self
            .depth
            .checked_sub(1)
            .ok_or(RawSyntaxSiteAnalysisError::InvalidSourceSpan)?;
        Ok(())
    }

    fn finish(
        mut self,
        language: RawSyntaxLanguage,
        source: &[u8],
        limits: RawSyntaxSiteAnalysisLimits,
        control: RawSyntaxSiteAnalysisControl<'_>,
    ) -> Result<RawSyntaxSiteAnalysis, RawSyntaxSiteAnalysisError> {
        self.sites.sort_unstable_by(site_order);
        self.sites
            .dedup_by(|left, right| site_order(left, right).is_eq());
        let mut emitted = [0_u32; 4];
        for (index, site) in self.sites.iter_mut().enumerate() {
            if let Some(outcome) = control.outcome() {
                return Err(outcome);
            }
            site.ordinal = RawSyntaxSiteOrdinal::new(
                u32::try_from(index).map_err(|_| RawSyntaxSiteAnalysisError::SiteLimitExceeded)?,
            );
            validate_site(site, source, limits)?;
            let counter = match site.kind {
                RawSyntaxSiteKind::Import => &mut emitted[0],
                RawSyntaxSiteKind::Reference => &mut emitted[1],
                RawSyntaxSiteKind::Call => &mut emitted[2],
                RawSyntaxSiteKind::TestMarker => &mut emitted[3],
            };
            *counter = counter
                .checked_add(1)
                .ok_or(RawSyntaxSiteAnalysisError::SiteLimitExceeded)?;
        }
        if let Some(outcome) = control.outcome() {
            return Err(outcome);
        }
        Ok(RawSyntaxSiteAnalysis {
            language,
            sites: self.sites,
            visited_nodes: self.visited_nodes,
            syntax_error_nodes: self.syntax_error_nodes,
            max_observed_depth: self.max_observed_depth,
            owned_text_bytes: self.owned_text_bytes,
            coverage: coverage(language, emitted),
        })
    }
}

fn site_order(left: &RawSyntaxSite, right: &RawSyntaxSite) -> std::cmp::Ordering {
    (
        left.occurrence_span.start().get(),
        left.occurrence_span.end().get(),
        left.target_span.start().get(),
        left.target_span.end().get(),
        left.kind,
        left.evidence,
    )
        .cmp(&(
            right.occurrence_span.start().get(),
            right.occurrence_span.end().get(),
            right.target_span.start().get(),
            right.target_span.end().get(),
            right.kind,
            right.evidence,
        ))
}

fn coverage(language: RawSyntaxLanguage, emitted: [u32; 4]) -> RawSyntaxSiteCoverage {
    let available = RawSyntaxSiteSupport::Available;
    let unsupported = RawSyntaxSiteSupport::Unsupported;
    RawSyntaxSiteCoverage {
        import: RawSyntaxSiteKindCoverage {
            support: available,
            emitted: emitted[0],
        },
        reference: RawSyntaxSiteKindCoverage {
            support: if language == RawSyntaxLanguage::Rust {
                available
            } else {
                unsupported
            },
            emitted: emitted[1],
        },
        call: RawSyntaxSiteKindCoverage {
            support: available,
            emitted: emitted[2],
        },
        test_marker: RawSyntaxSiteKindCoverage {
            support: if language == RawSyntaxLanguage::Rust {
                available
            } else {
                unsupported
            },
            emitted: emitted[3],
        },
    }
}

fn extract_sites(
    language: RawSyntaxLanguage,
    node: Node<'_>,
    source: &[u8],
    limits: RawSyntaxSiteAnalysisLimits,
) -> Result<Vec<RawSyntaxSite>, RawSyntaxSiteAnalysisError> {
    let mut sites = Vec::new();
    extract_import_sites(&mut sites, language, node, source, limits)?;
    extract_call_site(&mut sites, language, node, source, limits)?;
    if language == RawSyntaxLanguage::Rust {
        extract_rust_marker_site(&mut sites, node, source, limits)?;
        extract_rust_reference_site(&mut sites, node, source, limits)?;
    }
    Ok(sites)
}

fn extract_import_sites(
    sites: &mut Vec<RawSyntaxSite>,
    language: RawSyntaxLanguage,
    node: Node<'_>,
    source: &[u8],
    limits: RawSyntaxSiteAnalysisLimits,
) -> Result<(), RawSyntaxSiteAnalysisError> {
    let common = (
        RawSyntaxSiteKind::Import,
        RawSyntaxSiteEvidence::DirectSyntax,
        source,
        limits,
    );
    match (language, node.kind()) {
        (RawSyntaxLanguage::Rust, "use_declaration") => {
            push_field_site(
                sites, node, "argument", common.0, common.1, common.2, common.3,
            )?;
        }
        (RawSyntaxLanguage::Go, "import_spec") => {
            push_field_site(sites, node, "path", common.0, common.1, common.2, common.3)?;
        }
        (RawSyntaxLanguage::TypeScript | RawSyntaxLanguage::Tsx, "import_statement") => {
            push_field_site(
                sites, node, "source", common.0, common.1, common.2, common.3,
            )?;
        }
        (RawSyntaxLanguage::Python, "import_statement") => {
            push_field_sites(sites, node, "name", common.0, common.1, common.2, common.3)?;
        }
        (RawSyntaxLanguage::Python, "import_from_statement") => {
            push_field_site(
                sites,
                node,
                "module_name",
                common.0,
                common.1,
                common.2,
                common.3,
            )?;
        }
        _ => {}
    }
    Ok(())
}

fn extract_call_site(
    sites: &mut Vec<RawSyntaxSite>,
    language: RawSyntaxLanguage,
    node: Node<'_>,
    source: &[u8],
    limits: RawSyntaxSiteAnalysisLimits,
) -> Result<(), RawSyntaxSiteAnalysisError> {
    let is_call = matches!(
        (language, node.kind()),
        (
            RawSyntaxLanguage::Rust
                | RawSyntaxLanguage::Go
                | RawSyntaxLanguage::TypeScript
                | RawSyntaxLanguage::Tsx,
            "call_expression"
        ) | (RawSyntaxLanguage::Python, "call")
    );
    if is_call {
        push_field_site(
            sites,
            node,
            "function",
            RawSyntaxSiteKind::Call,
            RawSyntaxSiteEvidence::DirectSyntax,
            source,
            limits,
        )?;
    }
    Ok(())
}

fn extract_rust_marker_site(
    sites: &mut Vec<RawSyntaxSite>,
    node: Node<'_>,
    source: &[u8],
    limits: RawSyntaxSiteAnalysisLimits,
) -> Result<(), RawSyntaxSiteAnalysisError> {
    if node.kind() != "attribute_item" {
        return Ok(());
    }
    let Some(attribute) = node
        .named_child(0)
        .filter(|node| node.kind() == "attribute")
    else {
        return Ok(());
    };
    let Some(path) = attribute.named_child(0) else {
        return Ok(());
    };
    let path_text = source_text(path, source)?;
    if matches!(path_text, "test" | "cfg")
        && (path_text == "test" || source_text(attribute, source)?.contains("test"))
    {
        let evidence = if path_text == "test" {
            RawSyntaxSiteEvidence::DirectSyntax
        } else {
            RawSyntaxSiteEvidence::SyntaxHeuristic
        };
        push_site(
            sites,
            node,
            path,
            RawSyntaxSiteKind::TestMarker,
            evidence,
            source,
            limits,
        )?;
    }
    Ok(())
}

fn extract_rust_reference_site(
    sites: &mut Vec<RawSyntaxSite>,
    node: Node<'_>,
    source: &[u8],
    limits: RawSyntaxSiteAnalysisLimits,
) -> Result<(), RawSyntaxSiteAnalysisError> {
    if is_rust_reference_candidate(node.kind()) && is_rust_reference(node) {
        push_site(
            sites,
            node,
            node,
            RawSyntaxSiteKind::Reference,
            RawSyntaxSiteEvidence::SyntaxHeuristic,
            source,
            limits,
        )?;
    }
    Ok(())
}

fn push_field_site(
    sites: &mut Vec<RawSyntaxSite>,
    occurrence: Node<'_>,
    field: &str,
    kind: RawSyntaxSiteKind,
    evidence: RawSyntaxSiteEvidence,
    source: &[u8],
    limits: RawSyntaxSiteAnalysisLimits,
) -> Result<(), RawSyntaxSiteAnalysisError> {
    if let Some(target) = occurrence.child_by_field_name(field) {
        push_site(sites, occurrence, target, kind, evidence, source, limits)?;
    }
    Ok(())
}

fn push_field_sites(
    sites: &mut Vec<RawSyntaxSite>,
    occurrence: Node<'_>,
    field: &str,
    kind: RawSyntaxSiteKind,
    evidence: RawSyntaxSiteEvidence,
    source: &[u8],
    limits: RawSyntaxSiteAnalysisLimits,
) -> Result<(), RawSyntaxSiteAnalysisError> {
    let mut cursor = occurrence.walk();
    for target in occurrence.children_by_field_name(field, &mut cursor) {
        push_site(sites, occurrence, target, kind, evidence, source, limits)?;
    }
    Ok(())
}

fn push_site(
    sites: &mut Vec<RawSyntaxSite>,
    occurrence: Node<'_>,
    target: Node<'_>,
    kind: RawSyntaxSiteKind,
    evidence: RawSyntaxSiteEvidence,
    source: &[u8],
    limits: RawSyntaxSiteAnalysisLimits,
) -> Result<(), RawSyntaxSiteAnalysisError> {
    let occurrence_span = source_span(occurrence, source)?;
    let target_span = source_span(target, source)?;
    let raw_target = source_text(target, source)?.to_owned();
    sites.push(RawSyntaxSite::try_new(
        RawSyntaxSiteOrdinal::new(0),
        kind,
        evidence,
        occurrence_span,
        target_span,
        raw_target,
        limits,
    )?);
    Ok(())
}

fn is_rust_reference_candidate(kind: &str) -> bool {
    matches!(
        kind,
        "identifier"
            | "shorthand_field_identifier"
            | "type_identifier"
            | "scoped_identifier"
            | "scoped_type_identifier"
            | "field_expression"
            | "generic_function"
    )
}

fn is_rust_reference(node: Node<'_>) -> bool {
    if node
        .parent()
        .is_some_and(|parent| is_rust_reference_candidate(parent.kind()))
    {
        return false;
    }
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        match current.kind() {
            "attribute"
            | "attribute_item"
            | "inner_attribute_item"
            | "visibility_modifier"
            | "token_tree" => return false,
            "use_declaration" if node_in_field(current, "argument", node) => return false,
            "call_expression" if node_in_field(current, "function", node) => return false,
            "macro_invocation" if node_in_field(current, "macro", node) => return false,
            "let_declaration" | "parameter" | "variadic_parameter"
                if node_in_field(current, "pattern", node) =>
            {
                return false;
            }
            "function_item"
            | "function_signature_item"
            | "struct_item"
            | "enum_item"
            | "trait_item"
            | "mod_item"
            | "type_item"
            | "const_item"
            | "static_item"
                if node_in_field(current, "name", node) =>
            {
                return false;
            }
            _ => {}
        }
        ancestor = current.parent();
    }
    true
}

fn node_in_field(ancestor: Node<'_>, field: &str, node: Node<'_>) -> bool {
    ancestor.child_by_field_name(field).is_some_and(|outer| {
        let outer = outer.byte_range();
        let inner = node.byte_range();
        outer.start <= inner.start && inner.end <= outer.end
    })
}

fn validate_site(
    site: &RawSyntaxSite,
    source: &[u8],
    limits: RawSyntaxSiteAnalysisLimits,
) -> Result<(), RawSyntaxSiteAnalysisError> {
    let occurrence = span_range(site.occurrence_span, source)?;
    let target = span_range(site.target_span, source)?;
    if site.raw_target.is_empty()
        || target.start < occurrence.start
        || target.end > occurrence.end
        || source.get(target) != Some(site.raw_target.as_bytes())
        || site.raw_target.len() > usize::from(limits.max_target_bytes())
    {
        return Err(RawSyntaxSiteAnalysisError::InvalidSourceSpan);
    }
    Ok(())
}

fn source_span(node: Node<'_>, source: &[u8]) -> Result<ByteSpan, RawSyntaxSiteAnalysisError> {
    let range = node.byte_range();
    if range.end > source.len() {
        return Err(RawSyntaxSiteAnalysisError::InvalidSourceSpan);
    }
    ByteSpan::try_new(
        ByteOffset::new(
            u64::try_from(range.start)
                .map_err(|_| RawSyntaxSiteAnalysisError::InvalidSourceSpan)?,
        ),
        ByteOffset::new(
            u64::try_from(range.end).map_err(|_| RawSyntaxSiteAnalysisError::InvalidSourceSpan)?,
        ),
    )
    .map_err(|_| RawSyntaxSiteAnalysisError::InvalidSourceSpan)
}

fn span_range(
    span: ByteSpan,
    source: &[u8],
) -> Result<std::ops::Range<usize>, RawSyntaxSiteAnalysisError> {
    let start = usize::try_from(span.start().get())
        .map_err(|_| RawSyntaxSiteAnalysisError::InvalidSourceSpan)?;
    let end = usize::try_from(span.end().get())
        .map_err(|_| RawSyntaxSiteAnalysisError::InvalidSourceSpan)?;
    if start > end || end > source.len() {
        return Err(RawSyntaxSiteAnalysisError::InvalidSourceSpan);
    }
    Ok(start..end)
}

fn source_text<'a>(
    node: Node<'_>,
    source: &'a [u8],
) -> Result<&'a str, RawSyntaxSiteAnalysisError> {
    let range = node.byte_range();
    let bytes = source
        .get(range)
        .ok_or(RawSyntaxSiteAnalysisError::InvalidSourceSpan)?;
    std::str::from_utf8(bytes).map_err(|_| RawSyntaxSiteAnalysisError::InvalidSourceEncoding)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::AtomicBool,
        time::{Duration, Instant},
    };

    fn analyze(language: RawSyntaxLanguage, source: &[u8]) -> RawSyntaxSiteAnalysis {
        RawSyntaxSiteAnalyzer::new(language)
            .expect("grammar should load")
            .analyze(
                source,
                RawSyntaxSiteAnalysisLimits::default(),
                RawSyntaxSiteAnalysisControl::new(
                    &AtomicBool::new(false),
                    Instant::now() + Duration::from_secs(2),
                ),
            )
            .expect("fixture should analyze")
    }

    #[test]
    fn extracts_exact_imports_and_calls_for_every_supported_language() {
        let fixtures = [
            (
                RawSyntaxLanguage::Rust,
                b"use crate::tools; fn run() { execute(); }".as_slice(),
            ),
            (
                RawSyntaxLanguage::Go,
                b"package app\nimport \"example/tool\"\nfunc run() { execute() }".as_slice(),
            ),
            (
                RawSyntaxLanguage::TypeScript,
                b"import tool from './tool'; execute();".as_slice(),
            ),
            (
                RawSyntaxLanguage::Tsx,
                b"import tool from './tool'; execute(<View />);".as_slice(),
            ),
            (
                RawSyntaxLanguage::Python,
                b"import tool\nexecute()".as_slice(),
            ),
        ];
        for (language, source) in fixtures {
            let analysis = analyze(language, source);
            assert!(
                analysis
                    .sites()
                    .iter()
                    .any(|site| site.kind() == RawSyntaxSiteKind::Import)
            );
            assert!(
                analysis
                    .sites()
                    .iter()
                    .any(|site| site.kind() == RawSyntaxSiteKind::Call)
            );
            for (index, site) in analysis.sites().iter().enumerate() {
                assert_eq!(
                    site.ordinal().get(),
                    u32::try_from(index).expect("small fixture")
                );
                let span = span_range(site.target_span(), source).expect("valid target span");
                assert_eq!(source.get(span), Some(site.raw_target().as_bytes()));
            }
        }
    }

    #[test]
    fn optional_import_targets_remain_complete_raw_observations() {
        let fixtures = [
            (
                RawSyntaxLanguage::TypeScript,
                b"import Common = require('common');".as_slice(),
            ),
            (
                RawSyntaxLanguage::Python,
                b"from . import sibling".as_slice(),
            ),
        ];
        for (language, source) in fixtures {
            let analysis = analyze(language, source);
            for site in analysis.sites() {
                let span = span_range(site.target_span(), source).expect("valid target span");
                assert_eq!(source.get(span), Some(site.raw_target().as_bytes()));
            }
        }
    }

    #[test]
    fn rust_marks_test_and_reference_without_resolving_them() {
        let analysis = analyze(
            RawSyntaxLanguage::Rust,
            b"#[test]\nfn checks() { dependency(); let _ = value; }",
        );
        assert!(
            analysis
                .sites()
                .iter()
                .any(|site| site.kind() == RawSyntaxSiteKind::TestMarker)
        );
        assert!(analysis.sites().iter().any(
            |site| site.kind() == RawSyntaxSiteKind::Reference && site.raw_target() == "value"
        ));
        assert_eq!(
            analysis
                .coverage()
                .for_kind(RawSyntaxSiteKind::Reference)
                .support(),
            RawSyntaxSiteSupport::Available
        );
        assert_eq!(
            analysis
                .coverage()
                .for_kind(RawSyntaxSiteKind::TestMarker)
                .support(),
            RawSyntaxSiteSupport::Available
        );
    }

    #[test]
    fn unsupported_categories_are_categorical_not_empty_claims() {
        let analysis = analyze(RawSyntaxLanguage::Python, b"def test_name():\n    run()\n");
        assert_eq!(
            analysis
                .coverage()
                .for_kind(RawSyntaxSiteKind::Reference)
                .support(),
            RawSyntaxSiteSupport::Unsupported
        );
        assert_eq!(
            analysis
                .coverage()
                .for_kind(RawSyntaxSiteKind::TestMarker)
                .support(),
            RawSyntaxSiteSupport::Unsupported
        );
    }

    #[test]
    fn non_utf8_outside_a_selected_target_does_not_fail_the_source_projection() {
        let analysis = analyze(RawSyntaxLanguage::Rust, b"// \xff\nfn run() { execute(); }");
        assert!(
            analysis.sites().iter().any(
                |site| site.kind() == RawSyntaxSiteKind::Call && site.raw_target() == "execute"
            )
        );
    }

    #[test]
    fn cancellation_deadlines_and_bounds_fail_closed() {
        let mut analyzer =
            RawSyntaxSiteAnalyzer::new(RawSyntaxLanguage::Rust).expect("grammar should load");
        let cancelled = AtomicBool::new(true);
        assert_eq!(
            analyzer.analyze(
                b"fn run() {}",
                RawSyntaxSiteAnalysisLimits::default(),
                RawSyntaxSiteAnalysisControl::new(
                    &cancelled,
                    Instant::now() + Duration::from_secs(1)
                )
            ),
            Err(RawSyntaxSiteAnalysisError::Cancelled)
        );
        let active = AtomicBool::new(false);
        let limits =
            RawSyntaxSiteAnalysisLimits::try_new(8, 10, 8, 10, 32, 64).expect("valid small limits");
        assert_eq!(
            analyzer.analyze(
                b"fn too_large() {}",
                limits,
                RawSyntaxSiteAnalysisControl::new(&active, Instant::now() + Duration::from_secs(1))
            ),
            Err(RawSyntaxSiteAnalysisError::SourceLimitExceeded)
        );
    }
}
