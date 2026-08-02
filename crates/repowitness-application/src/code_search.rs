//! Shared Phase 0 lexical code-search request and evidence mapping.

use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_analysis::RustSymbolKind;
use repowitness_domain::{
    AnalysisArtifactDigest, BoundedResultItems, ByteSpan, CoverageItemCount, CoverageSummary,
    EvidenceIdentity, EvidenceLocation, EvidenceRecord, EvidenceRelation, EvidenceTier,
    MaterialResult, MaterialResultError, ProducerIdentity, ProducerManifestDigest,
    RepositoryIdentityDigest, RepositoryPath, ResolutionStatus, ResultItemLimit, ResultItemsError,
    ResultNotice, ResultNoticeKind, SourceContentDigest, SourceSnapshotDigest,
};
use sha2::{Digest, Sha256};

use crate::{RustIndexCoverage, SourceLanguage};

/// Version of the literal supported-language symbol search profile.
pub const CODE_SEARCH_PROFILE_VERSION: u16 = 3;
/// Default maximum number of returned candidates.
pub const DEFAULT_CODE_SEARCH_RESULTS: u16 = 20;
/// Default aggregate byte bound for returned candidate data.
pub const DEFAULT_CODE_SEARCH_OUTPUT_BYTES: u64 = 256 * 1024;
/// Hard Phase 0 candidate-count ceiling.
pub const MAX_CODE_SEARCH_RESULTS: u16 = 100;
/// Hard Phase 0 aggregate byte ceiling for returned candidate data.
pub const MAX_CODE_SEARCH_OUTPUT_BYTES: u64 = 1024 * 1024;

const MAX_QUERY_BYTES: usize = 256;
const MAX_QUERY_TERMS: usize = 8;
const MAX_TERM_BYTES: usize = 64;
const MAX_SYMBOL_NAME_BYTES: usize = 1_024;
const MAX_QUALIFIED_NAME_BYTES: usize = 4_096;
const QUERY_HASH_DOMAIN: &[u8] = b"repowitness.code-search-query.v1\0";

/// Stable failure to admit an untrusted literal search query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeSearchQueryError {
    /// No literal term was supplied.
    Empty,
    /// The complete input exceeds the Phase 0 byte ceiling.
    QueryTooLong,
    /// The input contains more literal terms than the Phase 0 ceiling.
    TooManyTerms,
    /// At least one literal term exceeds its byte ceiling.
    TermTooLong,
    /// At least one literal term contains a control character.
    InvalidTerm,
}

impl fmt::Display for CodeSearchQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "code-search query must contain at least one term",
            Self::QueryTooLong => "code-search query exceeds the byte limit",
            Self::TooManyTerms => "code-search query exceeds the term-count limit",
            Self::TermTooLong => "code-search query term exceeds the byte limit",
            Self::InvalidTerm => "code-search query term contains an invalid character",
        })
    }
}

impl Error for CodeSearchQueryError {}

/// SHA-256 identity for one canonical admitted query without retaining it in a claim.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CodeSearchQueryDigest([u8; 32]);

impl CodeSearchQueryDigest {
    /// Returns the exact digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for CodeSearchQueryDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodeSearchQueryDigest")
            .field("algorithm", &"SHA-256")
            .finish_non_exhaustive()
    }
}

/// Validated canonical terms for the version-1 literal search profile.
#[derive(Clone, Eq, PartialEq)]
pub struct CodeSearchQuery {
    canonical: String,
    digest: CodeSearchQueryDigest,
    term_count: u8,
}

impl CodeSearchQuery {
    /// Validates and canonicalizes an untrusted UTF-8 query.
    ///
    /// Runs in time linear in at most 256 input bytes.
    pub fn try_new(input: &str) -> Result<Self, CodeSearchQueryError> {
        if input.len() > MAX_QUERY_BYTES {
            return Err(CodeSearchQueryError::QueryTooLong);
        }
        let terms: Vec<&str> = input.split_whitespace().collect();
        if terms.is_empty() {
            return Err(CodeSearchQueryError::Empty);
        }
        if terms.len() > MAX_QUERY_TERMS {
            return Err(CodeSearchQueryError::TooManyTerms);
        }
        if terms.iter().any(|term| term.len() > MAX_TERM_BYTES) {
            return Err(CodeSearchQueryError::TermTooLong);
        }
        if terms.iter().any(|term| term.chars().any(char::is_control)) {
            return Err(CodeSearchQueryError::InvalidTerm);
        }

        let canonical = terms.join(" ");
        let mut hasher = Sha256::new();
        hasher.update(QUERY_HASH_DOMAIN);
        for term in &terms {
            let term_bytes = u16::try_from(term.len()).expect("validated query terms fit in a u16");
            hasher.update(term_bytes.to_be_bytes());
            hasher.update(term.as_bytes());
        }
        let digest = CodeSearchQueryDigest(hasher.finalize().into());
        let term_count =
            u8::try_from(terms.len()).expect("validated query term count fits in a u8");
        Ok(Self {
            canonical,
            digest,
            term_count,
        })
    }

    /// Returns the canonical single-space-separated literal terms.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.canonical
    }

    /// Returns the non-reversible canonical query identity.
    #[must_use]
    pub const fn digest(&self) -> CodeSearchQueryDigest {
        self.digest
    }

    /// Returns the admitted term count.
    #[must_use]
    pub const fn term_count(&self) -> u8 {
        self.term_count
    }
}

impl fmt::Debug for CodeSearchQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodeSearchQuery")
            .field("term_count", &self.term_count)
            .field("digest", &self.digest)
            .field("text", &"<redacted-query>")
            .finish()
    }
}

/// Stable failure to construct Phase 0 result and output bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeSearchLimitError;

impl fmt::Display for CodeSearchLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("code-search limits are zero or exceed Phase 0 ceilings")
    }
}

impl Error for CodeSearchLimitError {}

/// Inclusive candidate and aggregate-output bounds for one query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeSearchLimits {
    max_results: u16,
    max_output_bytes: u64,
}

impl CodeSearchLimits {
    /// Validates limits against the Phase 0 hard ceilings.
    pub const fn try_new(
        max_results: u16,
        max_output_bytes: u64,
    ) -> Result<Self, CodeSearchLimitError> {
        if max_results == 0
            || max_results > MAX_CODE_SEARCH_RESULTS
            || max_output_bytes == 0
            || max_output_bytes > MAX_CODE_SEARCH_OUTPUT_BYTES
        {
            return Err(CodeSearchLimitError);
        }
        Ok(Self {
            max_results,
            max_output_bytes,
        })
    }

    /// Returns the inclusive candidate bound.
    #[must_use]
    pub const fn max_results(self) -> u16 {
        self.max_results
    }

    /// Returns the inclusive aggregate candidate byte bound.
    #[must_use]
    pub const fn max_output_bytes(self) -> u64 {
        self.max_output_bytes
    }
}

impl Default for CodeSearchLimits {
    fn default() -> Self {
        Self {
            max_results: DEFAULT_CODE_SEARCH_RESULTS,
            max_output_bytes: DEFAULT_CODE_SEARCH_OUTPUT_BYTES,
        }
    }
}

/// Stable invalid-adapter-output classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeSearchPortOutputError {
    /// The adapter returned more candidates than requested.
    CandidateLimitExceeded,
    /// The adapter's total match count is smaller than its returned count.
    InvalidTotalMatches,
    /// The adapter exceeded the requested encoded-output bound.
    OutputByteLimitExceeded,
    /// A candidate violated the shared syntax-occurrence contract.
    InvalidCandidate,
    /// A fixed-width count overflowed while composing coverage.
    CountNotRepresentable,
}

impl fmt::Display for CodeSearchPortOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CandidateLimitExceeded => {
                "code-search adapter returned more candidates than requested"
            }
            Self::InvalidTotalMatches => {
                "code-search adapter returned an invalid total match count"
            }
            Self::OutputByteLimitExceeded => {
                "code-search adapter exceeded the requested output byte limit"
            }
            Self::InvalidCandidate => "code-search adapter returned an invalid candidate",
            Self::CountNotRepresentable => "code-search result count cannot be represented safely",
        })
    }
}

impl Error for CodeSearchPortOutputError {}

/// Exact persisted artifact attribution carried by one syntax occurrence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceArtifactEvidence {
    artifact_digest: AnalysisArtifactDigest,
    producer_manifest: ProducerManifestDigest,
}

impl SourceArtifactEvidence {
    /// Constructs exact artifact and producer attribution read from persistence.
    #[must_use]
    pub const fn new(
        artifact_digest: AnalysisArtifactDigest,
        producer_manifest: ProducerManifestDigest,
    ) -> Self {
        Self {
            artifact_digest,
            producer_manifest,
        }
    }

    /// Returns the semantics-complete artifact digest.
    #[must_use]
    pub const fn artifact_digest(self) -> AnalysisArtifactDigest {
        self.artifact_digest
    }

    /// Returns the syntax producer that created the artifact.
    #[must_use]
    pub const fn producer_manifest(self) -> ProducerManifestDigest {
        self.producer_manifest
    }
}

/// Validated identity and display data for one syntax symbol occurrence.
#[derive(Clone, Eq, PartialEq)]
pub struct RustSymbolOccurrence {
    language: SourceLanguage,
    fact_ordinal: u64,
    artifact: SourceArtifactEvidence,
    kind: RustSymbolKind,
    name: String,
    qualified_name: String,
    name_span: ByteSpan,
    declaration_span: ByteSpan,
}

impl RustSymbolOccurrence {
    /// Constructs one occurrence after enforcing extractor-compatible bounds.
    pub fn try_new(
        fact_ordinal: u64,
        artifact: SourceArtifactEvidence,
        kind: RustSymbolKind,
        name: String,
        qualified_name: String,
        name_span: ByteSpan,
        declaration_span: ByteSpan,
    ) -> Result<Self, CodeSearchPortOutputError> {
        if name.is_empty()
            || name.len() > MAX_SYMBOL_NAME_BYTES
            || qualified_name.is_empty()
            || qualified_name.len() > MAX_QUALIFIED_NAME_BYTES
            || name_span.is_empty()
            || name.chars().any(char::is_control)
            || qualified_name.chars().any(char::is_control)
            || declaration_span.start() > name_span.start()
            || declaration_span.end() < name_span.end()
        {
            return Err(CodeSearchPortOutputError::InvalidCandidate);
        }
        Ok(Self {
            language: SourceLanguage::Rust,
            fact_ordinal,
            artifact,
            kind,
            name,
            qualified_name,
            name_span,
            declaration_span,
        })
    }

    /// Rebinds this validated occurrence to its explicitly persisted language.
    #[must_use]
    pub const fn with_language(mut self, language: SourceLanguage) -> Self {
        self.language = language;
        self
    }

    /// Returns the syntax adapter language that produced this occurrence.
    #[must_use]
    pub const fn language(&self) -> SourceLanguage {
        self.language
    }

    /// Returns the deterministic source-order ordinal within the file.
    #[must_use]
    pub const fn fact_ordinal(&self) -> u64 {
        self.fact_ordinal
    }

    /// Returns the semantics-complete artifact identity.
    #[must_use]
    pub const fn artifact_digest(&self) -> AnalysisArtifactDigest {
        self.artifact.artifact_digest()
    }

    /// Returns the exact syntax producer that created the persisted artifact.
    #[must_use]
    pub const fn producer_manifest(&self) -> ProducerManifestDigest {
        self.artifact.producer_manifest()
    }

    /// Returns the syntax declaration category.
    #[must_use]
    pub const fn kind(&self) -> RustSymbolKind {
        self.kind
    }

    /// Returns the exact symbol name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the deterministic syntax-qualified name.
    #[must_use]
    pub fn qualified_name(&self) -> &str {
        &self.qualified_name
    }

    /// Returns the identifier byte span.
    #[must_use]
    pub const fn name_span(&self) -> ByteSpan {
        self.name_span
    }

    /// Returns the complete declaration byte span.
    #[must_use]
    pub const fn declaration_span(&self) -> ByteSpan {
        self.declaration_span
    }
}

impl fmt::Debug for RustSymbolOccurrence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustSymbolOccurrence")
            .field("language", &self.language)
            .field("fact_ordinal", &self.fact_ordinal)
            .field("artifact", &self.artifact)
            .field("kind", &self.kind)
            .field("name", &"<redacted-symbol>")
            .field("qualified_name", &"<redacted-symbol>")
            .field("name_span", &self.name_span)
            .field("declaration_span", &self.declaration_span)
            .finish()
    }
}

/// Language-neutral compatibility name for one syntax symbol occurrence.
pub type SourceSymbolOccurrence = RustSymbolOccurrence;

/// One storage-neutral lexical candidate returned by a retrieval adapter.
#[derive(Clone, Eq, PartialEq)]
pub struct CodeSearchCandidate {
    path: RepositoryPath,
    content_digest: SourceContentDigest,
    occurrence: RustSymbolOccurrence,
}

impl CodeSearchCandidate {
    /// Constructs a candidate from already-validated identity components.
    #[must_use]
    pub const fn new(
        path: RepositoryPath,
        content_digest: SourceContentDigest,
        occurrence: RustSymbolOccurrence,
    ) -> Self {
        Self {
            path,
            content_digest,
            occurrence,
        }
    }

    pub(crate) fn into_parts(self) -> (RepositoryPath, SourceContentDigest, RustSymbolOccurrence) {
        (self.path, self.content_digest, self.occurrence)
    }
}

impl fmt::Debug for CodeSearchCandidate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodeSearchCandidate")
            .field("path", &self.path)
            .field("content_digest", &self.content_digest)
            .field("occurrence", &self.occurrence)
            .finish()
    }
}

/// Complete adapter response pinned to one snapshot and generation.
pub struct CodeSearchPortResult<G> {
    snapshot: SourceSnapshotDigest,
    generation: G,
    index_coverage: RustIndexCoverage,
    candidates: Vec<CodeSearchCandidate>,
    total_matches: u64,
    output_bytes: u64,
}

impl<G> CodeSearchPortResult<G> {
    /// Constructs a port response for validation by the application use case.
    #[must_use]
    pub const fn new(
        snapshot: SourceSnapshotDigest,
        generation: G,
        index_coverage: RustIndexCoverage,
        candidates: Vec<CodeSearchCandidate>,
        total_matches: u64,
        output_bytes: u64,
    ) -> Self {
        Self {
            snapshot,
            generation,
            index_coverage,
            candidates,
            total_matches,
            output_bytes,
        }
    }

    /// Decomposes the validated storage-neutral response for a sibling use case.
    #[allow(
        clippy::type_complexity,
        reason = "this internal adapter boundary preserves the complete immutable response"
    )]
    pub(crate) fn into_parts(
        self,
    ) -> (
        SourceSnapshotDigest,
        G,
        RustIndexCoverage,
        Vec<CodeSearchCandidate>,
        u64,
        u64,
    ) {
        (
            self.snapshot,
            self.generation,
            self.index_coverage,
            self.candidates,
            self.total_matches,
            self.output_bytes,
        )
    }
}

/// Narrow retrieval boundary used by CLI and MCP application composition.
pub trait CodeSearchPort {
    /// Opaque immutable generation identity owned by the adapter.
    type Generation: Copy + Eq;
    /// Stable adapter failure mapped at its boundary.
    type Error;

    /// Searches one active generation under explicit controls and bounds.
    fn search(
        &self,
        repository: RepositoryIdentityDigest,
        query: &CodeSearchQuery,
        limits: CodeSearchLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<CodeSearchPortResult<Self::Generation>, Self::Error>;
}

/// Application request shared by local CLI and MCP adapters.
pub struct CodeSearchRequest {
    repository: RepositoryIdentityDigest,
    query: CodeSearchQuery,
    limits: CodeSearchLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl CodeSearchRequest {
    /// Constructs a request from already-validated boundary values.
    #[must_use]
    pub const fn new(
        repository: RepositoryIdentityDigest,
        query: CodeSearchQuery,
        limits: CodeSearchLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            repository,
            query,
            limits,
            cancelled,
            deadline,
        }
    }
}

impl fmt::Debug for CodeSearchRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodeSearchRequest")
            .field("repository", &self.repository)
            .field("query", &self.query)
            .field("limits", &self.limits)
            .field(
                "cancelled",
                &self.cancelled.load(std::sync::atomic::Ordering::Acquire),
            )
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Claim established by one bounded lexical search.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeSearchClaim {
    query: CodeSearchQueryDigest,
    returned_matches: u64,
    total_matches: u64,
}

impl CodeSearchClaim {
    /// Returns the versioned canonical query identity.
    #[must_use]
    pub const fn query(self) -> CodeSearchQueryDigest {
        self.query
    }

    /// Returns the number of candidates carried by the result.
    #[must_use]
    pub const fn returned_matches(self) -> u64 {
        self.returned_matches
    }

    /// Returns the exact number of candidates matched before result truncation.
    #[must_use]
    pub const fn total_matches(self) -> u64 {
        self.total_matches
    }

    /// Returns the literal search profile version.
    #[must_use]
    pub const fn profile_version(self) -> u16 {
        CODE_SEARCH_PROFILE_VERSION
    }
}

/// Fixed producer classes for direct Phase 0 syntax evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeSearchProducer {
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

/// Structured limitations attached to a Phase 0 lexical search.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeSearchNotice {
    /// The query covers indexed supported-language symbol names, not arbitrary source text.
    SupportedLanguageSymbolLexicalOnly,
}

/// Evidence identity returned by the shared Phase 0 search use case.
pub type CodeSearchEvidenceIdentity = EvidenceIdentity<
    RepositoryIdentityDigest,
    SourceSnapshotDigest,
    RepositoryPath,
    SourceContentDigest,
    RustSymbolOccurrence,
>;

/// Producer attribution returned by the shared Phase 0 search use case.
pub type CodeSearchProducerIdentity = ProducerIdentity<CodeSearchProducer, ProducerManifestDigest>;

/// Proof-carrying result returned by the shared Phase 0 search use case.
pub type CodeSearchResult<G> = MaterialResult<
    CodeSearchClaim,
    CodeSearchEvidenceIdentity,
    CodeSearchProducerIdentity,
    SourceSnapshotDigest,
    G,
    CodeSearchNotice,
>;

/// Stable application failure for one lexical search.
#[derive(Debug)]
pub enum CodeSearchError<E> {
    /// Cancellation was visible before a complete application result existed.
    Cancelled,
    /// The request deadline elapsed before a complete application result existed.
    DeadlineExceeded,
    /// The retrieval adapter failed.
    Port(E),
    /// The retrieval adapter violated the shared result contract.
    InvalidPortOutput(CodeSearchPortOutputError),
    /// A bounded evidence or notice collection could not be represented.
    ResultItems(ResultItemsError),
    /// The composed material result violated a domain invariant.
    MaterialResult(MaterialResultError),
}

impl<E: fmt::Display> fmt::Display for CodeSearchError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("code search was cancelled"),
            Self::DeadlineExceeded => formatter.write_str("code search deadline exceeded"),
            Self::Port(error) => write!(formatter, "code-search adapter failed: {error}"),
            Self::InvalidPortOutput(error) => error.fmt(formatter),
            Self::ResultItems(error) => error.fmt(formatter),
            Self::MaterialResult(error) => error.fmt(formatter),
        }
    }
}

impl<E> Error for CodeSearchError<E>
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

include!("code_search/use_case.rs");

#[cfg(test)]
mod tests;
