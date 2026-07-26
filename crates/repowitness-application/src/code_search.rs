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

use crate::RustIndexCoverage;

/// Version of the literal Rust-symbol search profile.
pub const CODE_SEARCH_PROFILE_VERSION: u16 = 1;
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

/// Validated identity and display data for one syntax symbol occurrence.
#[derive(Clone, Eq, PartialEq)]
pub struct RustSymbolOccurrence {
    fact_ordinal: u64,
    artifact_digest: AnalysisArtifactDigest,
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
        artifact_digest: AnalysisArtifactDigest,
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
            fact_ordinal,
            artifact_digest,
            kind,
            name,
            qualified_name,
            name_span,
            declaration_span,
        })
    }

    /// Returns the deterministic source-order ordinal within the file.
    #[must_use]
    pub const fn fact_ordinal(&self) -> u64 {
        self.fact_ordinal
    }

    /// Returns the semantics-complete artifact identity.
    #[must_use]
    pub const fn artifact_digest(&self) -> AnalysisArtifactDigest {
        self.artifact_digest
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
            .field("fact_ordinal", &self.fact_ordinal)
            .field("artifact_digest", &self.artifact_digest)
            .field("kind", &self.kind)
            .field("name", &"<redacted-symbol>")
            .field("qualified_name", &"<redacted-symbol>")
            .field("name_span", &self.name_span)
            .field("declaration_span", &self.declaration_span)
            .finish()
    }
}

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

    fn into_parts(self) -> (RepositoryPath, SourceContentDigest, RustSymbolOccurrence) {
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
    producer_manifest: ProducerManifestDigest,
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
        producer_manifest: ProducerManifestDigest,
        index_coverage: RustIndexCoverage,
        candidates: Vec<CodeSearchCandidate>,
        total_matches: u64,
        output_bytes: u64,
    ) -> Self {
        Self {
            snapshot,
            generation,
            producer_manifest,
            index_coverage,
            candidates,
            total_matches,
            output_bytes,
        }
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

/// Fixed producer class for direct Phase 0 Rust syntax evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeSearchProducer {
    /// Bounded Tree-sitter Rust syntax extraction.
    RustSyntax,
}

/// Structured limitations attached to a Phase 0 lexical search.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeSearchNotice {
    /// The query covers indexed Rust symbol names, not arbitrary source text.
    RustSymbolLexicalOnly,
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

/// Runs one bounded search and maps storage-neutral candidates to attributed evidence.
pub fn code_search<Port>(
    port: &Port,
    request: CodeSearchRequest,
) -> Result<CodeSearchResult<Port::Generation>, CodeSearchError<Port::Error>>
where
    Port: CodeSearchPort,
{
    check_control(&request.cancelled, request.deadline)?;
    let query_digest = request.query.digest();
    let limits = request.limits;
    let repository = request.repository;
    let result = port
        .search(
            repository,
            &request.query,
            limits,
            Arc::clone(&request.cancelled),
            request.deadline,
        )
        .map_err(CodeSearchError::Port)?;
    check_control(&request.cancelled, request.deadline)?;

    let (returned_matches, omitted_matches) = validate_port_result(&result, limits)?;
    let coverage = search_coverage(result.index_coverage, returned_matches, omitted_matches)?;
    let evidence = search_evidence(
        repository,
        result.snapshot,
        result.producer_manifest,
        result.candidates,
        limits,
    )?;
    let notices = search_notices()?;
    let resolution = if returned_matches == 0 {
        ResolutionStatus::Unresolved
    } else {
        ResolutionStatus::Confirmed
    };
    MaterialResult::try_new(
        CodeSearchClaim {
            query: query_digest,
            returned_matches,
            total_matches: result.total_matches,
        },
        evidence,
        resolution,
        result.snapshot,
        result.generation,
        notices,
        coverage,
    )
    .map_err(CodeSearchError::MaterialResult)
}

fn validate_port_result<G, E>(
    result: &CodeSearchPortResult<G>,
    limits: CodeSearchLimits,
) -> Result<(u64, u64), CodeSearchError<E>> {
    let returned_matches = u64::try_from(result.candidates.len()).map_err(|_| {
        CodeSearchError::InvalidPortOutput(CodeSearchPortOutputError::CountNotRepresentable)
    })?;
    if returned_matches > u64::from(limits.max_results()) {
        return Err(CodeSearchError::InvalidPortOutput(
            CodeSearchPortOutputError::CandidateLimitExceeded,
        ));
    }
    let omitted_matches = result.total_matches.checked_sub(returned_matches).ok_or(
        CodeSearchError::InvalidPortOutput(CodeSearchPortOutputError::InvalidTotalMatches),
    )?;
    if result.output_bytes > limits.max_output_bytes() {
        return Err(CodeSearchError::InvalidPortOutput(
            CodeSearchPortOutputError::OutputByteLimitExceeded,
        ));
    }
    Ok((returned_matches, omitted_matches))
}

fn search_coverage<E>(
    index: RustIndexCoverage,
    returned_matches: u64,
    omitted_matches: u64,
) -> Result<CoverageSummary, CodeSearchError<E>> {
    let unresolved = index
        .unresolved()
        .checked_add(u64::from(returned_matches == 0))
        .ok_or(CodeSearchError::InvalidPortOutput(
            CodeSearchPortOutputError::CountNotRepresentable,
        ))?;
    let truncated = index.truncated().checked_add(omitted_matches).ok_or(
        CodeSearchError::InvalidPortOutput(CodeSearchPortOutputError::CountNotRepresentable),
    )?;
    Ok(CoverageSummary::new(
        CoverageItemCount::new(index.searched()),
        CoverageItemCount::new(index.skipped()),
        CoverageItemCount::new(unresolved),
        CoverageItemCount::new(truncated),
    ))
}

fn search_evidence<E>(
    repository: RepositoryIdentityDigest,
    snapshot: SourceSnapshotDigest,
    producer_manifest: ProducerManifestDigest,
    candidates: Vec<CodeSearchCandidate>,
    limits: CodeSearchLimits,
) -> Result<
    BoundedResultItems<EvidenceRecord<CodeSearchEvidenceIdentity, CodeSearchProducerIdentity>>,
    CodeSearchError<E>,
> {
    let producer = ProducerIdentity::new(CodeSearchProducer::RustSyntax, producer_manifest);
    let evidence = candidates
        .into_iter()
        .map(|candidate| {
            let (path, content_digest, occurrence) = candidate.into_parts();
            EvidenceRecord::new(
                EvidenceIdentity::new(
                    repository,
                    snapshot,
                    path,
                    content_digest,
                    EvidenceLocation::SymbolOccurrence(occurrence),
                ),
                producer,
                EvidenceTier::Syntax,
                EvidenceRelation::Supports,
            )
        })
        .collect();
    BoundedResultItems::try_from_vec(
        evidence,
        ResultItemLimit::new(u64::from(limits.max_results())),
    )
    .map_err(CodeSearchError::ResultItems)
}

fn search_notices<E>()
-> Result<BoundedResultItems<ResultNotice<CodeSearchNotice>>, CodeSearchError<E>> {
    BoundedResultItems::try_from_vec(
        vec![ResultNotice::new(
            ResultNoticeKind::Limitation,
            CodeSearchNotice::RustSymbolLexicalOnly,
        )],
        ResultItemLimit::new(1),
    )
    .map_err(CodeSearchError::ResultItems)
}

fn check_control<E>(cancelled: &AtomicBool, deadline: Instant) -> Result<(), CodeSearchError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(CodeSearchError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(CodeSearchError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    use repowitness_analysis::RustSymbolKind;
    use repowitness_domain::{
        AnalysisArtifactDigest, ByteOffset, ByteSpan, EvidenceLocation, RepositoryPath,
        RepositoryPathLimits, ResolutionStatus, SourceContentDigest, SourceSnapshotDigest,
    };

    use super::{
        CodeSearchCandidate, CodeSearchError, CodeSearchLimits, CodeSearchPort,
        CodeSearchPortOutputError, CodeSearchPortResult, CodeSearchProducer, CodeSearchQuery,
        CodeSearchQueryError, CodeSearchRequest, MAX_CODE_SEARCH_OUTPUT_BYTES,
        MAX_CODE_SEARCH_RESULTS, RustIndexCoverage, RustSymbolOccurrence, code_search,
    };

    const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(128, 8);

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum FakeError {
        Failed,
    }

    impl std::fmt::Display for FakeError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("fake retrieval failed")
        }
    }

    impl std::error::Error for FakeError {}

    struct FakePort {
        calls: Cell<u64>,
        result: Cell<Option<Result<CodeSearchPortResult<u64>, FakeError>>>,
    }

    impl FakePort {
        fn with(result: Result<CodeSearchPortResult<u64>, FakeError>) -> Self {
            Self {
                calls: Cell::new(0),
                result: Cell::new(Some(result)),
            }
        }
    }

    impl CodeSearchPort for FakePort {
        type Generation = u64;
        type Error = FakeError;

        fn search(
            &self,
            _repository: repowitness_domain::RepositoryIdentityDigest,
            _query: &CodeSearchQuery,
            _limits: CodeSearchLimits,
            _cancelled: Arc<AtomicBool>,
            _deadline: Instant,
        ) -> Result<CodeSearchPortResult<Self::Generation>, Self::Error> {
            self.calls.set(self.calls.get() + 1);
            self.result
                .take()
                .expect("fake port should be called at most once")
        }
    }

    fn candidate(ordinal: u64, name: &str) -> CodeSearchCandidate {
        let name_start = 10 + ordinal;
        let name_end = name_start + u64::try_from(name.len()).expect("fixture length fits");
        let occurrence = RustSymbolOccurrence::try_new(
            ordinal,
            AnalysisArtifactDigest::new([4; 32]),
            RustSymbolKind::Function,
            name.to_owned(),
            format!("fixture::{name}"),
            ByteSpan::try_new(ByteOffset::new(name_start), ByteOffset::new(name_end))
                .expect("fixture span is valid"),
            ByteSpan::try_new(ByteOffset::new(0), ByteOffset::new(name_end + 2))
                .expect("fixture declaration is valid"),
        )
        .expect("fixture occurrence is valid");
        CodeSearchCandidate::new(
            RepositoryPath::try_from_bytes(format!("src/{name}.rs").as_bytes(), PATH_LIMITS)
                .expect("fixture path is valid"),
            SourceContentDigest::new([3; 32]),
            occurrence,
        )
    }

    fn result(
        candidates: Vec<CodeSearchCandidate>,
        total_matches: u64,
    ) -> CodeSearchPortResult<u64> {
        CodeSearchPortResult::new(
            SourceSnapshotDigest::new([2; 32]),
            7,
            repowitness_domain::ProducerManifestDigest::new([5; 32]),
            RustIndexCoverage::new(8, 2, 1, 0),
            candidates,
            total_matches,
            512,
        )
    }

    fn request(cancelled: Arc<AtomicBool>, deadline: Instant) -> CodeSearchRequest {
        CodeSearchRequest::new(
            repowitness_domain::RepositoryIdentityDigest::new([1; 32]),
            CodeSearchQuery::try_new("  Widget\t run ").expect("query is valid"),
            CodeSearchLimits::default(),
            cancelled,
            deadline,
        )
    }

    #[test]
    fn query_admission_is_canonical_bounded_and_redacted() {
        let first = CodeSearchQuery::try_new("  Widget\t run ").expect("query is valid");
        let second = CodeSearchQuery::try_new("Widget run").expect("query is valid");
        assert_eq!(first, second);
        assert_eq!(first.as_str(), "Widget run");
        assert_eq!(first.term_count(), 2);
        assert_eq!(first.digest(), second.digest());
        let debug = format!("{first:?}");
        assert!(debug.contains("<redacted-query>"));
        assert!(!debug.contains("Widget"));

        assert_eq!(
            CodeSearchQuery::try_new(""),
            Err(CodeSearchQueryError::Empty)
        );
        assert_eq!(
            CodeSearchQuery::try_new(&"x".repeat(257)),
            Err(CodeSearchQueryError::QueryTooLong)
        );
        assert_eq!(
            CodeSearchQuery::try_new("1 2 3 4 5 6 7 8 9"),
            Err(CodeSearchQueryError::TooManyTerms)
        );
        assert_eq!(
            CodeSearchQuery::try_new(&"x".repeat(65)),
            Err(CodeSearchQueryError::TermTooLong)
        );
        assert_eq!(
            CodeSearchQuery::try_new("private\0term"),
            Err(CodeSearchQueryError::InvalidTerm)
        );
    }

    #[test]
    fn limits_enforce_inclusive_phase0_ceilings() {
        assert!(
            CodeSearchLimits::try_new(MAX_CODE_SEARCH_RESULTS, MAX_CODE_SEARCH_OUTPUT_BYTES)
                .is_ok()
        );
        assert!(CodeSearchLimits::try_new(0, 1).is_err());
        assert!(CodeSearchLimits::try_new(MAX_CODE_SEARCH_RESULTS + 1, 1).is_err());
        assert!(CodeSearchLimits::try_new(1, 0).is_err());
        assert!(CodeSearchLimits::try_new(1, MAX_CODE_SEARCH_OUTPUT_BYTES + 1).is_err());
    }

    #[test]
    fn candidates_become_ordered_attributed_evidence_with_exact_coverage() {
        let port = FakePort::with(Ok(result(
            vec![candidate(0, "Widget"), candidate(1, "run")],
            5,
        )));
        let material = code_search(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
            ),
        )
        .expect("search should succeed");

        assert_eq!(port.calls.get(), 1);
        assert_eq!(material.resolution(), ResolutionStatus::Confirmed);
        assert_eq!(material.claim().returned_matches(), 2);
        assert_eq!(material.claim().total_matches(), 5);
        assert_eq!(material.generation(), &7);
        assert_eq!(material.snapshot(), &SourceSnapshotDigest::new([2; 32]));
        assert_eq!(material.coverage().searched().get(), 8);
        assert_eq!(material.coverage().skipped().get(), 2);
        assert_eq!(material.coverage().unresolved().get(), 1);
        assert_eq!(material.coverage().truncated().get(), 3);
        let evidence = material.evidence().as_slice();
        assert_eq!(evidence.len(), 2);
        assert_eq!(evidence[0].producer().id(), &CodeSearchProducer::RustSyntax);
        assert_eq!(evidence[0].tier(), repowitness_domain::EvidenceTier::Syntax);
        assert_eq!(
            evidence[0].relation(),
            repowitness_domain::EvidenceRelation::Supports
        );
        let EvidenceLocation::SymbolOccurrence(first) = evidence[0].identity().location() else {
            panic!("candidate evidence should identify a symbol occurrence");
        };
        assert_eq!(first.name(), "Widget");
        let EvidenceLocation::SymbolOccurrence(second) = evidence[1].identity().location() else {
            panic!("candidate evidence should identify a symbol occurrence");
        };
        assert_eq!(second.name(), "run");
    }

    #[test]
    fn an_empty_candidate_set_abstains_and_reports_unresolved_scope() {
        let port = FakePort::with(Ok(result(Vec::new(), 0)));
        let material = code_search(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
            ),
        )
        .expect("empty search should be a valid unresolved result");

        assert_eq!(material.resolution(), ResolutionStatus::Unresolved);
        assert!(material.evidence().is_empty());
        assert_eq!(material.coverage().unresolved().get(), 2);
    }

    #[test]
    fn cancellation_deadline_and_port_failures_remain_distinct() {
        let cancelled = Arc::new(AtomicBool::new(true));
        let cancelled_port = FakePort::with(Err(FakeError::Failed));
        let cancelled_error = code_search(
            &cancelled_port,
            request(cancelled, Instant::now() + Duration::from_secs(1)),
        )
        .expect_err("pre-cancelled work should fail");
        assert!(matches!(cancelled_error, CodeSearchError::Cancelled));
        assert_eq!(cancelled_port.calls.get(), 0);

        let deadline_port = FakePort::with(Err(FakeError::Failed));
        let deadline_error = code_search(
            &deadline_port,
            request(Arc::new(AtomicBool::new(false)), Instant::now()),
        )
        .expect_err("elapsed deadline should fail");
        assert!(matches!(deadline_error, CodeSearchError::DeadlineExceeded));
        assert_eq!(deadline_port.calls.get(), 0);

        let failure_port = FakePort::with(Err(FakeError::Failed));
        let failure = code_search(
            &failure_port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
            ),
        )
        .expect_err("adapter failure should remain distinct");
        assert!(matches!(failure, CodeSearchError::Port(FakeError::Failed)));
    }

    #[test]
    fn invalid_adapter_counts_and_bytes_fail_closed() {
        let too_many = (0..21)
            .map(|ordinal| candidate(ordinal, &format!("item{ordinal}")))
            .collect();
        let port = FakePort::with(Ok(result(too_many, 21)));
        assert!(matches!(
            code_search(
                &port,
                request(
                    Arc::new(AtomicBool::new(false)),
                    Instant::now() + Duration::from_secs(1)
                )
            ),
            Err(CodeSearchError::InvalidPortOutput(
                CodeSearchPortOutputError::CandidateLimitExceeded
            ))
        ));

        let port = FakePort::with(Ok(result(vec![candidate(0, "item")], 0)));
        assert!(matches!(
            code_search(
                &port,
                request(
                    Arc::new(AtomicBool::new(false)),
                    Instant::now() + Duration::from_secs(1)
                )
            ),
            Err(CodeSearchError::InvalidPortOutput(
                CodeSearchPortOutputError::InvalidTotalMatches
            ))
        ));

        let oversized = CodeSearchPortResult::new(
            SourceSnapshotDigest::new([2; 32]),
            7,
            repowitness_domain::ProducerManifestDigest::new([5; 32]),
            RustIndexCoverage::new(1, 0, 0, 0),
            vec![candidate(0, "item")],
            1,
            256 * 1024 + 1,
        );
        let port = FakePort::with(Ok(oversized));
        assert!(matches!(
            code_search(
                &port,
                request(
                    Arc::new(AtomicBool::new(false)),
                    Instant::now() + Duration::from_secs(1)
                )
            ),
            Err(CodeSearchError::InvalidPortOutput(
                CodeSearchPortOutputError::OutputByteLimitExceeded
            ))
        ));
    }

    #[test]
    fn request_debug_and_post_port_cancellation_do_not_expose_query_text() {
        struct CancellingPort;

        impl CodeSearchPort for CancellingPort {
            type Generation = u64;
            type Error = FakeError;

            fn search(
                &self,
                _repository: repowitness_domain::RepositoryIdentityDigest,
                _query: &CodeSearchQuery,
                _limits: CodeSearchLimits,
                cancelled: Arc<AtomicBool>,
                _deadline: Instant,
            ) -> Result<CodeSearchPortResult<Self::Generation>, Self::Error> {
                cancelled.store(true, Ordering::Release);
                Ok(result(vec![candidate(0, "private_symbol")], 1))
            }
        }

        let request = request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        );
        let debug = format!("{request:?}");
        assert!(!debug.contains("Widget"));
        assert!(!debug.contains("run"));
        assert!(matches!(
            code_search(&CancellingPort, request),
            Err(CodeSearchError::Cancelled)
        ));
    }
}
