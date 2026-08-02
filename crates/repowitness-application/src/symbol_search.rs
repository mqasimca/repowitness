//! Bounded, typed declaration discovery over immutable supported-language facts.
//!
//! This use case intentionally reports declarations and their exact syntax
//! receipts. It does not infer relationships from same-name matches.

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
    BoundedResultItems, CoverageItemCount, CoverageSummary, EvidenceIdentity, EvidenceLocation,
    EvidenceRecord, EvidenceRelation, EvidenceTier, MaterialResult, MaterialResultError,
    ProducerIdentity, ProducerManifestDigest, RepositoryIdentityDigest, RepositoryPath,
    ResolutionStatus, ResultItemLimit, ResultItemsError, ResultNotice, ResultNoticeKind,
    SourceContentDigest, SourceSnapshotDigest,
};
use sha2::{Digest, Sha256};

use crate::{
    CodeSearchCandidate, CodeSearchLimits, CodeSearchPortOutputError, CodeSearchPortResult,
    CodeSearchProducer, RustSymbolOccurrence, SourceLanguage,
};

/// Version of the typed declaration-discovery profile.
pub const SYMBOL_SEARCH_PROFILE_VERSION: u16 = 1;
/// Maximum UTF-8 byte length of an admitted declaration-name selector.
pub const MAX_SYMBOL_SEARCH_NAME_BYTES: usize = 1_024;

const MAX_PATH_PREFIX_BYTES: usize = 4_096;
const QUERY_HASH_DOMAIN: &[u8] = b"repowitness.symbol-search-query.v1\0";

/// Stable failure to admit an untrusted typed declaration selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolSearchQueryError {
    /// The requested declaration name is empty.
    EmptyName,
    /// The requested declaration name is too large.
    NameTooLong,
    /// The requested declaration name has control data or whitespace.
    InvalidName,
    /// The repository-relative path prefix is too large.
    PathPrefixTooLong,
    /// The path prefix is not a safe repository-relative byte prefix.
    InvalidPathPrefix,
}

impl fmt::Display for SymbolSearchQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyName => "symbol-search name must not be empty",
            Self::NameTooLong => "symbol-search name exceeds the byte limit",
            Self::InvalidName => "symbol-search name contains invalid characters",
            Self::PathPrefixTooLong => "symbol-search path prefix exceeds the byte limit",
            Self::InvalidPathPrefix => "symbol-search path prefix is not repository-relative",
        })
    }
}

impl Error for SymbolSearchQueryError {}

/// Exact match or deterministic byte-prefix selection for declaration names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolSearchNameMatch {
    /// Return declarations whose unqualified name is byte-for-byte equal.
    Exact,
    /// Return declarations whose unqualified name begins with the requested bytes.
    Prefix,
}

impl SymbolSearchNameMatch {
    /// Returns the stable profile spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Prefix => "prefix",
        }
    }
}

/// Non-reversible identity of one admitted typed declaration selector.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SymbolSearchQueryDigest([u8; 32]);

impl SymbolSearchQueryDigest {
    /// Returns the exact SHA-256 digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for SymbolSearchQueryDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SymbolSearchQueryDigest")
            .field("algorithm", &"SHA-256")
            .finish_non_exhaustive()
    }
}

/// Validated exact or prefix declaration search plus optional typed filters.
#[derive(Clone, Eq, PartialEq)]
pub struct SymbolSearchQuery {
    name: String,
    name_match: SymbolSearchNameMatch,
    language: Option<SourceLanguage>,
    kind: Option<RustSymbolKind>,
    path_prefix: Option<String>,
    digest: SymbolSearchQueryDigest,
}

impl SymbolSearchQuery {
    /// Validates a selector without a language, kind, or path restriction.
    pub fn try_new(
        name: &str,
        name_match: SymbolSearchNameMatch,
    ) -> Result<Self, SymbolSearchQueryError> {
        Self::try_new_with_filters(name, name_match, None, None, None)
    }

    /// Validates a selector with only evidence-backed persisted filters.
    pub fn try_new_with_filters(
        name: &str,
        name_match: SymbolSearchNameMatch,
        language: Option<SourceLanguage>,
        kind: Option<RustSymbolKind>,
        path_prefix: Option<&str>,
    ) -> Result<Self, SymbolSearchQueryError> {
        if name.is_empty() {
            return Err(SymbolSearchQueryError::EmptyName);
        }
        if name.len() > MAX_SYMBOL_SEARCH_NAME_BYTES {
            return Err(SymbolSearchQueryError::NameTooLong);
        }
        if name
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err(SymbolSearchQueryError::InvalidName);
        }
        let path_prefix = path_prefix.map(str::to_owned);
        if let Some(prefix) = &path_prefix {
            if prefix.len() > MAX_PATH_PREFIX_BYTES {
                return Err(SymbolSearchQueryError::PathPrefixTooLong);
            }
            if !is_safe_path_prefix(prefix) {
                return Err(SymbolSearchQueryError::InvalidPathPrefix);
            }
        }
        let digest = query_digest(name, name_match, language, kind, path_prefix.as_deref());
        Ok(Self {
            name: name.to_owned(),
            name_match,
            language,
            kind,
            path_prefix,
            digest,
        })
    }

    /// Returns the admitted unqualified declaration name selector.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the exact or prefix selector mode.
    #[must_use]
    pub const fn name_match(&self) -> SymbolSearchNameMatch {
        self.name_match
    }

    /// Returns the optional syntax-adapter language filter.
    #[must_use]
    pub const fn language(&self) -> Option<SourceLanguage> {
        self.language
    }

    /// Returns the optional syntax declaration-kind filter.
    #[must_use]
    pub const fn kind(&self) -> Option<RustSymbolKind> {
        self.kind
    }

    /// Returns the optional canonical repository-relative byte prefix.
    #[must_use]
    pub fn path_prefix(&self) -> Option<&str> {
        self.path_prefix.as_deref()
    }

    /// Returns the non-reversible canonical selector identity.
    #[must_use]
    pub const fn digest(&self) -> SymbolSearchQueryDigest {
        self.digest
    }
}

impl fmt::Debug for SymbolSearchQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SymbolSearchQuery")
            .field("name", &"<redacted-symbol>")
            .field("name_match", &self.name_match)
            .field("language", &self.language)
            .field("kind", &self.kind)
            .field(
                "path_prefix",
                &self.path_prefix.as_ref().map(|_| "<redacted-path>"),
            )
            .field("digest", &self.digest)
            .finish()
    }
}

/// Complete response from one declaration-search persistence adapter.
pub type SymbolSearchPortResult<G> = CodeSearchPortResult<G>;

/// Narrow, bounded read boundary for typed active-generation discovery.
pub trait SymbolSearchPort {
    /// Opaque immutable generation identity owned by the adapter.
    type Generation: Copy + Eq;
    /// Stable adapter failure mapped at its boundary.
    type Error;

    /// Finds only direct declarations that satisfy the admitted selector.
    fn search_symbols(
        &self,
        repository: RepositoryIdentityDigest,
        query: &SymbolSearchQuery,
        limits: CodeSearchLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<SymbolSearchPortResult<Self::Generation>, Self::Error>;
}

/// Application request shared by the local CLI and MCP adapters.
pub struct SymbolSearchRequest {
    repository: RepositoryIdentityDigest,
    query: SymbolSearchQuery,
    limits: CodeSearchLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl SymbolSearchRequest {
    /// Constructs a request from validated boundary values.
    #[must_use]
    pub const fn new(
        repository: RepositoryIdentityDigest,
        query: SymbolSearchQuery,
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

impl fmt::Debug for SymbolSearchRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SymbolSearchRequest")
            .field("repository", &self.repository)
            .field("query", &self.query)
            .field("limits", &self.limits)
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Claim established by one bounded typed declaration search.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SymbolSearchClaim {
    query: SymbolSearchQueryDigest,
    name_match: SymbolSearchNameMatch,
    returned_matches: u64,
    total_matches: u64,
}

impl SymbolSearchClaim {
    /// Returns the non-reversible selector identity.
    #[must_use]
    pub const fn query(self) -> SymbolSearchQueryDigest {
        self.query
    }
    /// Returns the exact or prefix semantics used by the admitted selector.
    #[must_use]
    pub const fn name_match(self) -> SymbolSearchNameMatch {
        self.name_match
    }
    /// Returns the number of retained declaration receipts.
    #[must_use]
    pub const fn returned_matches(self) -> u64 {
        self.returned_matches
    }
    /// Returns the count before result and byte truncation.
    #[must_use]
    pub const fn total_matches(self) -> u64 {
        self.total_matches
    }
    /// Returns the profile version.
    #[must_use]
    pub const fn profile_version(self) -> u16 {
        SYMBOL_SEARCH_PROFILE_VERSION
    }
}

/// Evidence identity carried by one typed declaration receipt.
pub type SymbolSearchEvidenceIdentity = EvidenceIdentity<
    RepositoryIdentityDigest,
    SourceSnapshotDigest,
    RepositoryPath,
    SourceContentDigest,
    RustSymbolOccurrence,
>;

/// Producer identity carried by one typed declaration receipt.
pub type SymbolSearchProducerIdentity =
    ProducerIdentity<CodeSearchProducer, ProducerManifestDigest>;

/// Limitations that must accompany this non-relational discovery result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolSearchNotice {
    /// Results are parser declaration facts from the five built-in source languages.
    DirectSyntaxDeclarationsOnly,
    /// Same-name results neither assert identity nor create relationship edges.
    NoNameBasedRelationshipResolution,
}

/// Proof-carrying typed declaration result.
pub type SymbolSearchResult<G> = MaterialResult<
    SymbolSearchClaim,
    SymbolSearchEvidenceIdentity,
    SymbolSearchProducerIdentity,
    SourceSnapshotDigest,
    G,
    SymbolSearchNotice,
>;

/// Stable application failure for one typed declaration search.
#[derive(Debug)]
pub enum SymbolSearchError<E> {
    /// Cancellation was visible before a complete result existed.
    Cancelled,
    /// The deadline elapsed before a complete result existed.
    DeadlineExceeded,
    /// The retrieval adapter failed.
    Port(E),
    /// The retrieval adapter violated the common direct-declaration contract.
    InvalidPortOutput(CodeSearchPortOutputError),
    /// A bounded evidence or notice collection could not be represented.
    ResultItems(ResultItemsError),
    /// The composed material result violated a domain invariant.
    MaterialResult(MaterialResultError),
}

impl<E: fmt::Display> fmt::Display for SymbolSearchError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("symbol search was cancelled"),
            Self::DeadlineExceeded => formatter.write_str("symbol-search deadline exceeded"),
            Self::Port(error) => write!(formatter, "symbol-search adapter failed: {error}"),
            Self::InvalidPortOutput(error) => error.fmt(formatter),
            Self::ResultItems(error) => error.fmt(formatter),
            Self::MaterialResult(error) => error.fmt(formatter),
        }
    }
}

impl<E> Error for SymbolSearchError<E>
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

/// Runs a bounded typed declaration search and maps direct syntax facts to evidence.
pub fn symbol_search<Port>(
    port: &Port,
    request: SymbolSearchRequest,
) -> Result<SymbolSearchResult<Port::Generation>, SymbolSearchError<Port::Error>>
where
    Port: SymbolSearchPort,
{
    check_control(&request.cancelled, request.deadline)?;
    let query_digest = request.query.digest();
    let limits = request.limits;
    let repository = request.repository;
    let (snapshot, generation, index_coverage, candidates, total_matches, output_bytes) = port
        .search_symbols(
            repository,
            &request.query,
            limits,
            Arc::clone(&request.cancelled),
            request.deadline,
        )
        .map_err(SymbolSearchError::Port)?
        .into_parts();
    check_control(&request.cancelled, request.deadline)?;
    let returned_matches = u64::try_from(candidates.len()).map_err(|_| {
        SymbolSearchError::InvalidPortOutput(CodeSearchPortOutputError::CountNotRepresentable)
    })?;
    if returned_matches > u64::from(limits.max_results()) {
        return Err(SymbolSearchError::InvalidPortOutput(
            CodeSearchPortOutputError::CandidateLimitExceeded,
        ));
    }
    let omitted_matches =
        total_matches
            .checked_sub(returned_matches)
            .ok_or(SymbolSearchError::InvalidPortOutput(
                CodeSearchPortOutputError::InvalidTotalMatches,
            ))?;
    if output_bytes > limits.max_output_bytes() {
        return Err(SymbolSearchError::InvalidPortOutput(
            CodeSearchPortOutputError::OutputByteLimitExceeded,
        ));
    }
    if candidates
        .iter()
        .any(|candidate| !candidate_matches_query(candidate, &request.query))
    {
        return Err(SymbolSearchError::InvalidPortOutput(
            CodeSearchPortOutputError::InvalidCandidate,
        ));
    }
    let evidence = evidence(repository, snapshot, candidates, limits)?;
    let notices = BoundedResultItems::try_from_vec(
        vec![
            ResultNotice::new(
                ResultNoticeKind::Limitation,
                SymbolSearchNotice::DirectSyntaxDeclarationsOnly,
            ),
            ResultNotice::new(
                ResultNoticeKind::Limitation,
                SymbolSearchNotice::NoNameBasedRelationshipResolution,
            ),
        ],
        ResultItemLimit::new(2),
    )
    .map_err(SymbolSearchError::ResultItems)?;
    let coverage = CoverageSummary::new(
        CoverageItemCount::new(index_coverage.searched()),
        CoverageItemCount::new(index_coverage.skipped()),
        CoverageItemCount::new(
            index_coverage
                .unresolved()
                .checked_add(u64::from(returned_matches == 0))
                .ok_or(SymbolSearchError::InvalidPortOutput(
                    CodeSearchPortOutputError::CountNotRepresentable,
                ))?,
        ),
        CoverageItemCount::new(
            index_coverage
                .truncated()
                .checked_add(omitted_matches)
                .ok_or(SymbolSearchError::InvalidPortOutput(
                    CodeSearchPortOutputError::CountNotRepresentable,
                ))?,
        ),
    );
    MaterialResult::try_new(
        SymbolSearchClaim {
            query: query_digest,
            name_match: request.query.name_match(),
            returned_matches,
            total_matches,
        },
        evidence,
        if returned_matches == 0 {
            ResolutionStatus::Unresolved
        } else {
            ResolutionStatus::Confirmed
        },
        snapshot,
        generation,
        notices,
        coverage,
    )
    .map_err(SymbolSearchError::MaterialResult)
}

fn candidate_matches_query(candidate: &CodeSearchCandidate, query: &SymbolSearchQuery) -> bool {
    let (path, _, occurrence) = candidate.clone().into_parts();
    let name_matches = match query.name_match() {
        SymbolSearchNameMatch::Exact => occurrence.name() == query.name(),
        SymbolSearchNameMatch::Prefix => occurrence.name().starts_with(query.name()),
    };
    name_matches
        && query
            .language()
            .is_none_or(|language| occurrence.language() == language)
        && query.kind().is_none_or(|kind| occurrence.kind() == kind)
        && query
            .path_prefix()
            .is_none_or(|prefix| path.as_bytes().starts_with(prefix.as_bytes()))
}

fn evidence<E>(
    repository: RepositoryIdentityDigest,
    snapshot: SourceSnapshotDigest,
    candidates: Vec<CodeSearchCandidate>,
    limits: CodeSearchLimits,
) -> Result<
    BoundedResultItems<EvidenceRecord<SymbolSearchEvidenceIdentity, SymbolSearchProducerIdentity>>,
    SymbolSearchError<E>,
> {
    let evidence = candidates
        .into_iter()
        .map(|candidate| {
            let (path, content_digest, occurrence) = candidate.into_parts();
            let producer = ProducerIdentity::new(
                match occurrence.language() {
                    SourceLanguage::Rust => CodeSearchProducer::RustSyntax,
                    SourceLanguage::Go => CodeSearchProducer::GoSyntax,
                    SourceLanguage::TypeScript => CodeSearchProducer::TypeScriptSyntax,
                    SourceLanguage::Tsx => CodeSearchProducer::TsxSyntax,
                    SourceLanguage::Python => CodeSearchProducer::PythonSyntax,
                },
                occurrence.producer_manifest(),
            );
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
    .map_err(SymbolSearchError::ResultItems)
}

fn is_safe_path_prefix(prefix: &str) -> bool {
    let normalized = prefix.strip_suffix('/').unwrap_or(prefix);

    !normalized.is_empty()
        && !normalized.starts_with('/')
        && !normalized.starts_with('\\')
        && !normalized.contains('\\')
        && !normalized.chars().any(char::is_control)
        && normalized
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

fn query_digest(
    name: &str,
    name_match: SymbolSearchNameMatch,
    language: Option<SourceLanguage>,
    kind: Option<RustSymbolKind>,
    path_prefix: Option<&str>,
) -> SymbolSearchQueryDigest {
    let mut hasher = Sha256::new();
    hasher.update(QUERY_HASH_DOMAIN);
    for value in [
        name_match.as_str(),
        language.map_or("", SourceLanguage::as_str),
        kind.map_or("", RustSymbolKind::as_str),
        path_prefix.unwrap_or(""),
        name,
    ] {
        let length = u16::try_from(value.len()).expect("validated selector field fits in u16");
        hasher.update(length.to_be_bytes());
        hasher.update(value.as_bytes());
    }
    SymbolSearchQueryDigest(hasher.finalize().into())
}

fn check_control<E>(cancelled: &AtomicBool, deadline: Instant) -> Result<(), SymbolSearchError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(SymbolSearchError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(SymbolSearchError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_SYMBOL_SEARCH_NAME_BYTES, SymbolSearchNameMatch, SymbolSearchQuery,
        SymbolSearchQueryError,
    };

    #[test]
    fn selector_validation_rejects_unsafe_inputs_without_retaining_them() {
        assert_eq!(
            SymbolSearchQuery::try_new("", SymbolSearchNameMatch::Exact),
            Err(SymbolSearchQueryError::EmptyName)
        );
        assert_eq!(
            SymbolSearchQuery::try_new_with_filters(
                "name",
                SymbolSearchNameMatch::Prefix,
                None,
                None,
                Some("../private")
            ),
            Err(SymbolSearchQueryError::InvalidPathPrefix)
        );
        let query = SymbolSearchQuery::try_new_with_filters(
            "private_symbol",
            SymbolSearchNameMatch::Prefix,
            None,
            None,
            Some("src/"),
        )
        .expect("safe trailing-slash selector");
        assert_eq!(query.path_prefix(), Some("src/"));
        let debug = format!("{query:?}");
        assert!(!debug.contains("private_symbol"));
        assert!(!debug.contains("src"));

        let maximum_name = "a".repeat(MAX_SYMBOL_SEARCH_NAME_BYTES);
        assert!(SymbolSearchQuery::try_new(&maximum_name, SymbolSearchNameMatch::Exact).is_ok());
        let too_long_name = "a".repeat(MAX_SYMBOL_SEARCH_NAME_BYTES + 1);
        assert_eq!(
            SymbolSearchQuery::try_new(&too_long_name, SymbolSearchNameMatch::Exact),
            Err(SymbolSearchQueryError::NameTooLong)
        );
    }
}
