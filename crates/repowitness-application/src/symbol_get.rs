//! Shared Phase 0 exact-symbol retrieval and evidence mapping.

use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_domain::{
    AnalysisArtifactDigest, BoundedResultItems, CoverageItemCount, CoverageSummary,
    EvidenceIdentity, EvidenceLocation, EvidenceRecord, EvidenceRelation, EvidenceTier,
    MaterialResult, MaterialResultError, ProducerIdentity, ProducerManifestDigest,
    RepositoryIdentityDigest, RepositoryPath, ResolutionStatus, ResultItemLimit, ResultItemsError,
    ResultNotice, ResultNoticeKind, SourceContentDigest, SourceSnapshotDigest,
};

use crate::{RustIndexCoverage, RustSymbolOccurrence, SourceLanguage};

/// Version of the exact supported-language symbol retrieval profile.
pub const SYMBOL_GET_PROFILE_VERSION: u16 = 3;
/// Maximum declaration bytes returned by the Phase 0 profile.
pub const MAX_SYMBOL_GET_DECLARATION_BYTES: u64 = 8 * 1024 * 1024;
/// Maximum aggregate application payload returned by the Phase 0 profile.
pub const MAX_SYMBOL_GET_OUTPUT_BYTES: u64 = 10 * 1024 * 1024;

const FIXED_OCCURRENCE_OUTPUT_BYTES: u64 = 160;

/// Stable failure to construct exact-symbol result limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SymbolGetLimitError;

impl fmt::Display for SymbolGetLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("symbol-get limits are zero or exceed Phase 0 ceilings")
    }
}

impl Error for SymbolGetLimitError {}

/// Inclusive declaration and aggregate-output bounds for one exact lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SymbolGetLimits {
    max_declaration_bytes: u64,
    max_output_bytes: u64,
}

impl SymbolGetLimits {
    /// Validates limits against the Phase 0 hard ceilings.
    pub const fn try_new(
        max_declaration_bytes: u64,
        max_output_bytes: u64,
    ) -> Result<Self, SymbolGetLimitError> {
        if max_declaration_bytes == 0
            || max_declaration_bytes > MAX_SYMBOL_GET_DECLARATION_BYTES
            || max_output_bytes == 0
            || max_output_bytes > MAX_SYMBOL_GET_OUTPUT_BYTES
        {
            return Err(SymbolGetLimitError);
        }
        Ok(Self {
            max_declaration_bytes,
            max_output_bytes,
        })
    }

    /// Returns the inclusive declaration byte bound.
    #[must_use]
    pub const fn max_declaration_bytes(self) -> u64 {
        self.max_declaration_bytes
    }

    /// Returns the inclusive aggregate payload byte bound.
    #[must_use]
    pub const fn max_output_bytes(self) -> u64 {
        self.max_output_bytes
    }
}

impl Default for SymbolGetLimits {
    fn default() -> Self {
        Self {
            max_declaration_bytes: MAX_SYMBOL_GET_DECLARATION_BYTES,
            max_output_bytes: MAX_SYMBOL_GET_OUTPUT_BYTES,
        }
    }
}

/// Exact physical occurrence selected from a prior material result.
#[derive(Clone, Eq, PartialEq)]
pub struct SymbolGetSelector {
    path: RepositoryPath,
    content_digest: SourceContentDigest,
    artifact_digest: AnalysisArtifactDigest,
    fact_ordinal: u64,
}

impl SymbolGetSelector {
    /// Constructs an exact generation-local occurrence selector.
    #[must_use]
    pub const fn new(
        path: RepositoryPath,
        content_digest: SourceContentDigest,
        artifact_digest: AnalysisArtifactDigest,
        fact_ordinal: u64,
    ) -> Self {
        Self {
            path,
            content_digest,
            artifact_digest,
            fact_ordinal,
        }
    }

    /// Returns the exact repository-relative path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the exact indexed source-content identity.
    #[must_use]
    pub const fn content_digest(&self) -> SourceContentDigest {
        self.content_digest
    }

    /// Returns the semantics-complete analysis-artifact identity.
    #[must_use]
    pub const fn artifact_digest(&self) -> AnalysisArtifactDigest {
        self.artifact_digest
    }

    /// Returns the deterministic fact ordinal within the artifact.
    #[must_use]
    pub const fn fact_ordinal(&self) -> u64 {
        self.fact_ordinal
    }
}

impl fmt::Debug for SymbolGetSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SymbolGetSelector")
            .field("path", &self.path)
            .field("content_digest", &self.content_digest)
            .field("artifact_digest", &self.artifact_digest)
            .field("fact_ordinal", &self.fact_ordinal)
            .finish()
    }
}

/// One exact syntax occurrence and its verified declaration bytes.
#[derive(Clone, Eq, PartialEq)]
pub struct SymbolGetCandidate {
    path: RepositoryPath,
    content_digest: SourceContentDigest,
    occurrence: RustSymbolOccurrence,
    declaration: Box<[u8]>,
}

impl SymbolGetCandidate {
    /// Constructs adapter output for validation by the application use case.
    #[must_use]
    pub const fn new(
        path: RepositoryPath,
        content_digest: SourceContentDigest,
        occurrence: RustSymbolOccurrence,
        declaration: Box<[u8]>,
    ) -> Self {
        Self {
            path,
            content_digest,
            occurrence,
            declaration,
        }
    }
}

impl fmt::Debug for SymbolGetCandidate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SymbolGetCandidate")
            .field("path", &self.path)
            .field("content_digest", &self.content_digest)
            .field("occurrence", &self.occurrence)
            .field("declaration_bytes", &self.declaration.len())
            .field("declaration", &"<redacted-source>")
            .finish()
    }
}

/// Complete adapter response pinned to one active snapshot and generation.
pub struct SymbolGetPortResult<G> {
    snapshot: SourceSnapshotDigest,
    generation: G,
    index_coverage: RustIndexCoverage,
    candidate: Option<SymbolGetCandidate>,
}

impl<G> SymbolGetPortResult<G> {
    /// Constructs an adapter response for application validation.
    #[must_use]
    pub const fn new(
        snapshot: SourceSnapshotDigest,
        generation: G,
        index_coverage: RustIndexCoverage,
        candidate: Option<SymbolGetCandidate>,
    ) -> Self {
        Self {
            snapshot,
            generation,
            index_coverage,
            candidate,
        }
    }
}

/// Complete validated input passed to one exact-symbol adapter call.
pub struct SymbolGetPortRequest<G> {
    repository: RepositoryIdentityDigest,
    expected_snapshot: SourceSnapshotDigest,
    expected_generation: G,
    selector: SymbolGetSelector,
    limits: SymbolGetLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl<G> SymbolGetPortRequest<G> {
    /// Returns the exact repository identity.
    #[must_use]
    pub const fn repository(&self) -> RepositoryIdentityDigest {
        self.repository
    }

    /// Returns the snapshot that must still be active.
    #[must_use]
    pub const fn expected_snapshot(&self) -> SourceSnapshotDigest {
        self.expected_snapshot
    }

    /// Returns the generation that must still be active.
    #[must_use]
    pub const fn expected_generation(&self) -> &G {
        &self.expected_generation
    }

    /// Returns the exact requested physical occurrence.
    #[must_use]
    pub const fn selector(&self) -> &SymbolGetSelector {
        &self.selector
    }

    /// Returns the application payload limits.
    #[must_use]
    pub const fn limits(&self) -> SymbolGetLimits {
        self.limits
    }

    /// Returns the cooperative cancellation signal.
    #[must_use]
    pub fn cancelled(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }

    /// Returns the absolute request deadline.
    #[must_use]
    pub const fn deadline(&self) -> Instant {
        self.deadline
    }
}

/// Narrow exact-symbol boundary shared by CLI and MCP composition.
pub trait SymbolGetPort {
    /// Opaque immutable generation identity owned by the adapter.
    type Generation: Copy + Eq;
    /// Stable adapter failure mapped at its boundary.
    type Error;

    /// Resolves one selector only when the expected context is still active.
    fn get(
        &self,
        request: SymbolGetPortRequest<Self::Generation>,
    ) -> Result<SymbolGetPortResult<Self::Generation>, Self::Error>;
}

/// Application request shared by local CLI and MCP adapters.
pub struct SymbolGetRequest<G> {
    repository: RepositoryIdentityDigest,
    expected_snapshot: SourceSnapshotDigest,
    expected_generation: G,
    selector: SymbolGetSelector,
    limits: SymbolGetLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl<G> SymbolGetRequest<G> {
    /// Constructs a request from validated boundary values.
    #[must_use]
    pub const fn new(
        repository: RepositoryIdentityDigest,
        expected_snapshot: SourceSnapshotDigest,
        expected_generation: G,
        selector: SymbolGetSelector,
        limits: SymbolGetLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            repository,
            expected_snapshot,
            expected_generation,
            selector,
            limits,
            cancelled,
            deadline,
        }
    }
}

impl<G> fmt::Debug for SymbolGetRequest<G> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SymbolGetRequest")
            .field("repository", &self.repository)
            .field("expected_snapshot", &self.expected_snapshot)
            .field("expected_generation", &"<opaque-generation>")
            .field("selector", &self.selector)
            .field("limits", &self.limits)
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Exact symbol data established by a successful lookup.
#[derive(Clone, Eq, PartialEq)]
pub struct RetrievedSymbol {
    occurrence: RustSymbolOccurrence,
    declaration: Box<[u8]>,
}

impl RetrievedSymbol {
    /// Returns the validated syntax occurrence.
    #[must_use]
    pub const fn occurrence(&self) -> &RustSymbolOccurrence {
        &self.occurrence
    }

    /// Returns the exact declaration bytes from the indexed content identity.
    #[must_use]
    pub const fn declaration(&self) -> &[u8] {
        &self.declaration
    }
}

impl fmt::Debug for RetrievedSymbol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RetrievedSymbol")
            .field("occurrence", &self.occurrence)
            .field("declaration_bytes", &self.declaration.len())
            .field("declaration", &"<redacted-source>")
            .finish()
    }
}

/// Claim established by one exact symbol lookup.
#[derive(Clone, Eq, PartialEq)]
pub struct SymbolGetClaim {
    selector: SymbolGetSelector,
    symbol: Option<RetrievedSymbol>,
}

impl SymbolGetClaim {
    /// Returns the exact requested physical occurrence.
    #[must_use]
    pub const fn selector(&self) -> &SymbolGetSelector {
        &self.selector
    }

    /// Returns the symbol when the selector resolved in the expected context.
    #[must_use]
    pub const fn symbol(&self) -> Option<&RetrievedSymbol> {
        self.symbol.as_ref()
    }

    /// Returns the exact-symbol retrieval profile version.
    #[must_use]
    pub const fn profile_version(&self) -> u16 {
        SYMBOL_GET_PROFILE_VERSION
    }
}

impl fmt::Debug for SymbolGetClaim {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SymbolGetClaim")
            .field("selector", &self.selector)
            .field("resolved", &self.symbol.is_some())
            .field("symbol", &self.symbol)
            .finish()
    }
}

/// Fixed producer classes for direct Phase 0 exact-symbol evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolGetProducer {
    /// Bounded Tree-sitter Rust syntax extraction.
    RustSyntax,
    /// Bounded Tree-sitter Go syntax extraction.
    GoSyntax,
    /// Bounded Tree-sitter TypeScript syntax extraction.
    TypeScriptSyntax,
    /// Bounded Tree-sitter TSX syntax extraction.
    TsxSyntax,
    /// Bounded Tree-sitter Python syntax extraction.
    PythonSyntax,
}

/// Structured limitations attached to an exact Phase 0 symbol result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolGetNotice {
    /// Phase 0 returns one definition declaration and does not index references.
    DefinitionOnlyNoReferences,
}

/// Evidence identity returned by the shared exact-symbol use case.
pub type SymbolGetEvidenceIdentity = EvidenceIdentity<
    RepositoryIdentityDigest,
    SourceSnapshotDigest,
    RepositoryPath,
    SourceContentDigest,
    RustSymbolOccurrence,
>;

/// Producer attribution returned by the shared exact-symbol use case.
pub type SymbolGetProducerIdentity = ProducerIdentity<SymbolGetProducer, ProducerManifestDigest>;

/// Proof-carrying exact-symbol result returned by the application use case.
pub type SymbolGetResult<G> = MaterialResult<
    SymbolGetClaim,
    SymbolGetEvidenceIdentity,
    SymbolGetProducerIdentity,
    SourceSnapshotDigest,
    G,
    SymbolGetNotice,
>;

/// Stable invalid-adapter-output classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolGetPortOutputError {
    /// The adapter returned a different snapshot or generation.
    ContextMismatch,
    /// The adapter returned a different physical occurrence.
    SelectorMismatch,
    /// Declaration bytes do not agree with the stored source spans and name.
    InvalidDeclaration,
    /// The persisted language does not agree with the repository extension.
    LanguagePathMismatch,
    /// Declaration bytes exceeded the requested bound.
    DeclarationLimitExceeded,
    /// Aggregate returned bytes exceeded the requested bound.
    OutputByteLimitExceeded,
    /// Fixed-width output or coverage accounting overflowed.
    CountNotRepresentable,
}

impl fmt::Display for SymbolGetPortOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ContextMismatch => "symbol-get adapter returned a different source context",
            Self::SelectorMismatch => "symbol-get adapter returned a different occurrence",
            Self::InvalidDeclaration => {
                "symbol-get adapter returned declaration bytes inconsistent with the occurrence"
            }
            Self::LanguagePathMismatch => {
                "symbol-get adapter returned a language inconsistent with the repository path"
            }
            Self::DeclarationLimitExceeded => {
                "symbol-get adapter exceeded the requested declaration byte limit"
            }
            Self::OutputByteLimitExceeded => {
                "symbol-get adapter exceeded the requested output byte limit"
            }
            Self::CountNotRepresentable => {
                "symbol-get output or coverage count cannot be represented safely"
            }
        })
    }
}

impl Error for SymbolGetPortOutputError {}

/// Stable application failure for one exact symbol lookup.
#[derive(Debug)]
pub enum SymbolGetError<E> {
    /// Cancellation was visible before a complete result existed.
    Cancelled,
    /// The request deadline elapsed before a complete result existed.
    DeadlineExceeded,
    /// The exact-symbol adapter failed.
    Port(E),
    /// The adapter violated the shared result contract.
    InvalidPortOutput(SymbolGetPortOutputError),
    /// A bounded evidence or notice collection could not be represented.
    ResultItems(ResultItemsError),
    /// The composed material result violated a domain invariant.
    MaterialResult(MaterialResultError),
}

impl<E: fmt::Display> fmt::Display for SymbolGetError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("symbol get was cancelled"),
            Self::DeadlineExceeded => formatter.write_str("symbol-get deadline exceeded"),
            Self::Port(error) => write!(formatter, "symbol-get adapter failed: {error}"),
            Self::InvalidPortOutput(error) => error.fmt(formatter),
            Self::ResultItems(error) => error.fmt(formatter),
            Self::MaterialResult(error) => error.fmt(formatter),
        }
    }
}

impl<E> Error for SymbolGetError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Port(error) => Some(error),
            Self::InvalidPortOutput(error) => Some(error),
            Self::ResultItems(error) => Some(error),
            Self::MaterialResult(error) => Some(error),
            Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

include!("symbol_get/use_case.rs");

#[cfg(test)]
mod tests;
