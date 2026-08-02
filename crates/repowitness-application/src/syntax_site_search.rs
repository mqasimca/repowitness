//! Exact-target discovery over immutable all-language raw syntax observations.
//!
//! A matching target spelling is an attributed parser observation only. It is
//! never treated as a declaration identity, caller, reference, or graph edge.

use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_domain::{RepositoryIdentityDigest, SourceSnapshotDigest};
use sha2::{Digest, Sha256};

use crate::{OutboundSitesAvailability, OutboundSyntaxSite, RustIndexCoverage};

/// Version of the exact raw-target syntax-observation search profile.
pub const SYNTAX_SITE_SEARCH_PROFILE_VERSION: u16 = 1;
/// Maximum exact raw target bytes admitted at the public boundary.
pub const MAX_SYNTAX_SITE_SEARCH_TARGET_BYTES: usize = 16 * 1024;
/// Default number of retained exact observations.
pub const DEFAULT_SYNTAX_SITE_SEARCH_RESULTS: u16 = 100;
/// Hard number of retained exact observations.
pub const MAX_SYNTAX_SITE_SEARCH_RESULTS: u16 = 1_000;
/// Default conservative encoded-output ceiling.
pub const DEFAULT_SYNTAX_SITE_SEARCH_OUTPUT_BYTES: u64 = 512 * 1024;
/// Hard conservative encoded-output ceiling.
pub const MAX_SYNTAX_SITE_SEARCH_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;

const QUERY_HASH_DOMAIN: &[u8] = b"repowitness.syntax-site-search.query.v1\0";
const FIXED_SITE_OUTPUT_BYTES: u64 = 176;

/// Stable failure to admit an exact raw target spelling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxSiteSearchQueryError {
    /// No target bytes were supplied.
    Empty,
    /// The target exceeds the retained raw-site text ceiling.
    TooLong,
}

impl fmt::Display for SyntaxSiteSearchQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "syntax-site target is empty",
            Self::TooLong => "syntax-site target exceeds the configured byte limit",
        })
    }
}

impl Error for SyntaxSiteSearchQueryError {}

/// Non-reversible SHA-256 identity for one exact raw target spelling.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SyntaxSiteSearchQueryDigest([u8; 32]);

impl SyntaxSiteSearchQueryDigest {
    /// Returns the exact digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for SyntaxSiteSearchQueryDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyntaxSiteSearchQueryDigest")
            .field("algorithm", &"SHA-256")
            .finish_non_exhaustive()
    }
}

/// Validated exact raw target spelling for the version-1 profile.
#[derive(Clone, Eq, PartialEq)]
pub struct SyntaxSiteSearchQuery {
    target: String,
    digest: SyntaxSiteSearchQueryDigest,
}

impl SyntaxSiteSearchQuery {
    /// Validates one exact UTF-8 target without normalizing its source bytes.
    pub fn try_new(input: &str) -> Result<Self, SyntaxSiteSearchQueryError> {
        if input.is_empty() {
            return Err(SyntaxSiteSearchQueryError::Empty);
        }
        if input.len() > MAX_SYNTAX_SITE_SEARCH_TARGET_BYTES {
            return Err(SyntaxSiteSearchQueryError::TooLong);
        }
        let mut hasher = Sha256::new();
        hasher.update(QUERY_HASH_DOMAIN);
        hasher.update(
            u16::try_from(input.len())
                .expect("validated target length fits in u16")
                .to_be_bytes(),
        );
        hasher.update(input.as_bytes());
        Ok(Self {
            target: input.to_owned(),
            digest: SyntaxSiteSearchQueryDigest(hasher.finalize().into()),
        })
    }

    /// Returns the exact target spelling used as the SQLite-bound parameter.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.target
    }

    /// Returns the non-reversible query identity.
    #[must_use]
    pub const fn digest(&self) -> SyntaxSiteSearchQueryDigest {
        self.digest
    }
}

impl fmt::Debug for SyntaxSiteSearchQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyntaxSiteSearchQuery")
            .field("target_bytes", &self.target.len())
            .field("digest", &self.digest)
            .field("target", &"<redacted-raw-target>")
            .finish()
    }
}

/// Failure to construct bounded raw-target search limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyntaxSiteSearchLimitError;

impl fmt::Display for SyntaxSiteSearchLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("syntax-site search limits are zero or exceed compiled ceilings")
    }
}

impl Error for SyntaxSiteSearchLimitError {}

/// Independent bounded result limits for exact raw-target discovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyntaxSiteSearchLimits {
    max_results: u16,
    max_output_bytes: u64,
}

impl SyntaxSiteSearchLimits {
    /// Validates the fixed public profile bounds.
    pub const fn try_new(
        max_results: u16,
        max_output_bytes: u64,
    ) -> Result<Self, SyntaxSiteSearchLimitError> {
        if max_results == 0
            || max_results > MAX_SYNTAX_SITE_SEARCH_RESULTS
            || max_output_bytes == 0
            || max_output_bytes > MAX_SYNTAX_SITE_SEARCH_OUTPUT_BYTES
        {
            return Err(SyntaxSiteSearchLimitError);
        }
        Ok(Self {
            max_results,
            max_output_bytes,
        })
    }

    /// Returns the maximum retained raw observations.
    #[must_use]
    pub const fn max_results(self) -> u16 {
        self.max_results
    }

    /// Returns the maximum conservative encoded output bytes.
    #[must_use]
    pub const fn max_output_bytes(self) -> u64 {
        self.max_output_bytes
    }
}

impl Default for SyntaxSiteSearchLimits {
    fn default() -> Self {
        Self {
            max_results: DEFAULT_SYNTAX_SITE_SEARCH_RESULTS,
            max_output_bytes: DEFAULT_SYNTAX_SITE_SEARCH_OUTPUT_BYTES,
        }
    }
}

/// Complete untrusted adapter answer from one active immutable generation.
pub struct SyntaxSiteSearchPortResult<G> {
    snapshot: SourceSnapshotDigest,
    generation: G,
    index_coverage: RustIndexCoverage,
    availability: OutboundSitesAvailability,
    sites: Box<[OutboundSyntaxSite]>,
    total_sites: u64,
    output_bytes: u64,
}

impl<G> SyntaxSiteSearchPortResult<G> {
    /// Constructs the complete adapter response for application validation.
    #[must_use]
    pub const fn new(
        snapshot: SourceSnapshotDigest,
        generation: G,
        index_coverage: RustIndexCoverage,
        availability: OutboundSitesAvailability,
        sites: Box<[OutboundSyntaxSite]>,
        total_sites: u64,
        output_bytes: u64,
    ) -> Self {
        Self {
            snapshot,
            generation,
            index_coverage,
            availability,
            sites,
            total_sites,
            output_bytes,
        }
    }
}

/// Complete validated input passed to one raw-target search adapter call.
pub struct SyntaxSiteSearchPortRequest {
    repository: RepositoryIdentityDigest,
    query: SyntaxSiteSearchQuery,
    limits: SyntaxSiteSearchLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl SyntaxSiteSearchPortRequest {
    /// Returns the exact repository identity.
    #[must_use]
    pub const fn repository(&self) -> RepositoryIdentityDigest {
        self.repository
    }

    /// Returns the exact raw target spelling.
    #[must_use]
    pub const fn query(&self) -> &SyntaxSiteSearchQuery {
        &self.query
    }

    /// Returns explicit bounded read limits.
    #[must_use]
    pub const fn limits(&self) -> SyntaxSiteSearchLimits {
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

/// Narrow active-generation raw-target syntax-site boundary.
pub trait SyntaxSiteSearchPort {
    /// Opaque immutable generation identity owned by the adapter.
    type Generation: Copy + Eq;
    /// Stable adapter failure mapped at its boundary.
    type Error;

    /// Searches exact raw target spellings without resolving them to declarations.
    fn syntax_site_search(
        &self,
        request: SyntaxSiteSearchPortRequest,
    ) -> Result<SyntaxSiteSearchPortResult<Self::Generation>, Self::Error>;
}

/// Application request shared by local CLI and MCP adapters.
pub struct SyntaxSiteSearchRequest {
    repository: RepositoryIdentityDigest,
    query: SyntaxSiteSearchQuery,
    limits: SyntaxSiteSearchLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl SyntaxSiteSearchRequest {
    /// Constructs a request from already-validated boundary values.
    #[must_use]
    pub const fn new(
        repository: RepositoryIdentityDigest,
        query: SyntaxSiteSearchQuery,
        limits: SyntaxSiteSearchLimits,
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

impl fmt::Debug for SyntaxSiteSearchRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyntaxSiteSearchRequest")
            .field("repository", &self.repository)
            .field("query", &self.query)
            .field("limits", &self.limits)
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Claim established by one exact target observation search.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyntaxSiteSearchClaim {
    query: SyntaxSiteSearchQueryDigest,
    returned_sites: u64,
    total_sites: u64,
}

impl SyntaxSiteSearchClaim {
    /// Returns the versioned exact target identity.
    #[must_use]
    pub const fn query(self) -> SyntaxSiteSearchQueryDigest {
        self.query
    }

    /// Returns retained raw observations.
    #[must_use]
    pub const fn returned_sites(self) -> u64 {
        self.returned_sites
    }

    /// Returns exact matching observations before the retained-result bound.
    #[must_use]
    pub const fn total_sites(self) -> u64 {
        self.total_sites
    }
}

/// Fixed v1 scope boundary for raw-target discovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxSiteSearchNotice {
    /// Exact spelling equality does not resolve a target or establish a relationship.
    ExactRawTargetOnlyNoResolutionOrInferredEdges,
}

/// Complete validated active-generation result.
#[derive(Eq, PartialEq)]
pub struct SyntaxSiteSearchResult<G> {
    claim: SyntaxSiteSearchClaim,
    snapshot: SourceSnapshotDigest,
    generation: G,
    index_coverage: RustIndexCoverage,
    availability: OutboundSitesAvailability,
    sites: Box<[OutboundSyntaxSite]>,
    output_bytes: u64,
    notice: SyntaxSiteSearchNotice,
}

impl<G: fmt::Debug> fmt::Debug for SyntaxSiteSearchResult<G> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyntaxSiteSearchResult")
            .field("claim", &self.claim)
            .field("snapshot", &self.snapshot)
            .field("generation", &self.generation)
            .field("index_coverage", &self.index_coverage)
            .field("availability", &self.availability)
            .field(
                "sites",
                &format_args!("<{} raw observations>", self.sites.len()),
            )
            .field("output_bytes", &self.output_bytes)
            .field("notice", &self.notice)
            .finish()
    }
}

impl<G> SyntaxSiteSearchResult<G> {
    /// Returns the exact target-search claim.
    #[must_use]
    pub const fn claim(&self) -> SyntaxSiteSearchClaim {
        self.claim
    }
    /// Returns the concrete active snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> SourceSnapshotDigest {
        self.snapshot
    }
    /// Returns the concrete active immutable generation.
    #[must_use]
    pub const fn generation(&self) -> &G {
        &self.generation
    }
    /// Returns source-index coverage.
    #[must_use]
    pub const fn index_coverage(&self) -> RustIndexCoverage {
        self.index_coverage
    }
    /// Returns categorical raw projection availability.
    #[must_use]
    pub const fn availability(&self) -> OutboundSitesAvailability {
        self.availability
    }
    /// Returns observations in canonical path then source-span order.
    #[must_use]
    pub const fn sites(&self) -> &[OutboundSyntaxSite] {
        &self.sites
    }
    /// Returns conservative retained-output bytes.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
    /// Returns the fixed no-resolution limitation.
    #[must_use]
    pub const fn notice(&self) -> SyntaxSiteSearchNotice {
        self.notice
    }
}

/// Stable invalid-adapter-output classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxSiteSearchPortOutputError {
    /// The adapter violated a declared bound, count, or byte total.
    InvalidCoverage,
    /// The adapter returned a malformed, unmatched, or non-canonical observation.
    InvalidObservation,
}

impl fmt::Display for SyntaxSiteSearchPortOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidCoverage => "syntax-site-search adapter returned invalid bounded coverage",
            Self::InvalidObservation => {
                "syntax-site-search adapter returned invalid raw syntax observation data"
            }
        })
    }
}

impl Error for SyntaxSiteSearchPortOutputError {}

/// Application failure for one exact raw-target search.
#[derive(Debug)]
pub enum SyntaxSiteSearchError<E> {
    /// Cancellation was observed before complete output existed.
    Cancelled,
    /// The absolute deadline elapsed before complete output existed.
    DeadlineExceeded,
    /// The storage-neutral adapter failed.
    Port(E),
    /// The adapter violated the output contract.
    InvalidPortOutput(SyntaxSiteSearchPortOutputError),
}

impl<E: fmt::Display> fmt::Display for SyntaxSiteSearchError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("syntax-site search cancelled"),
            Self::DeadlineExceeded => formatter.write_str("syntax-site search deadline exceeded"),
            Self::Port(error) => write!(formatter, "syntax-site-search adapter failed: {error}"),
            Self::InvalidPortOutput(error) => error.fmt(formatter),
        }
    }
}

impl<E: Error + 'static> Error for SyntaxSiteSearchError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Port(error) => Some(error),
            Self::InvalidPortOutput(error) => Some(error),
            Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Searches exact raw target spellings in one immutable active generation.
pub fn syntax_site_search<Port>(
    port: &Port,
    request: SyntaxSiteSearchRequest,
) -> Result<SyntaxSiteSearchResult<Port::Generation>, SyntaxSiteSearchError<Port::Error>>
where
    Port: SyntaxSiteSearchPort,
{
    check_control(&request.cancelled, request.deadline)?;
    let result = port
        .syntax_site_search(SyntaxSiteSearchPortRequest {
            repository: request.repository,
            query: request.query.clone(),
            limits: request.limits,
            cancelled: Arc::clone(&request.cancelled),
            deadline: request.deadline,
        })
        .map_err(SyntaxSiteSearchError::Port)?;
    check_control(&request.cancelled, request.deadline)?;
    validate_result(&request, &result)?;
    let returned_sites = u64::try_from(result.sites.len()).unwrap_or(u64::MAX);
    Ok(SyntaxSiteSearchResult {
        claim: SyntaxSiteSearchClaim {
            query: request.query.digest(),
            returned_sites,
            total_sites: result.total_sites,
        },
        snapshot: result.snapshot,
        generation: result.generation,
        index_coverage: result.index_coverage,
        availability: result.availability,
        sites: result.sites,
        output_bytes: result.output_bytes,
        notice: SyntaxSiteSearchNotice::ExactRawTargetOnlyNoResolutionOrInferredEdges,
    })
}

fn validate_result<G: Eq, E>(
    request: &SyntaxSiteSearchRequest,
    result: &SyntaxSiteSearchPortResult<G>,
) -> Result<(), SyntaxSiteSearchError<E>> {
    if result.sites.len() > usize::from(request.limits.max_results())
        || result.total_sites < u64::try_from(result.sites.len()).unwrap_or(u64::MAX)
        || result.output_bytes > request.limits.max_output_bytes()
    {
        return Err(SyntaxSiteSearchError::InvalidPortOutput(
            SyntaxSiteSearchPortOutputError::InvalidCoverage,
        ));
    }
    if result.availability == OutboundSitesAvailability::NotProduced {
        if !result.sites.is_empty() || result.total_sites != 0 || result.output_bytes != 0 {
            return Err(SyntaxSiteSearchError::InvalidPortOutput(
                SyntaxSiteSearchPortOutputError::InvalidCoverage,
            ));
        }
        return Ok(());
    }
    let mut expected_output_bytes = 0_u64;
    let mut previous: Option<(crate::RepositoryPath, u64, u64, u64, u64, u32)> = None;
    for site in &result.sites {
        if site.site().raw_target() != request.query.as_str()
            || !site.language().matches_repository_path(site.path())
        {
            return Err(SyntaxSiteSearchError::InvalidPortOutput(
                SyntaxSiteSearchPortOutputError::InvalidObservation,
            ));
        }
        let occurrence = site.site().occurrence_span();
        let target = site.site().target_span();
        let order = (
            site.path().clone(),
            occurrence.start().get(),
            occurrence.end().get(),
            target.start().get(),
            target.end().get(),
            site.site().ordinal().get(),
        );
        if previous.as_ref().is_some_and(|previous| previous >= &order) {
            return Err(SyntaxSiteSearchError::InvalidPortOutput(
                SyntaxSiteSearchPortOutputError::InvalidObservation,
            ));
        }
        previous = Some(order);
        expected_output_bytes = expected_output_bytes
            .checked_add(FIXED_SITE_OUTPUT_BYTES)
            .and_then(|value| value.checked_add(u64::try_from(site.path().as_bytes().len()).ok()?))
            .and_then(|value| {
                value.checked_add(u64::try_from(site.site().raw_target().len()).ok()?)
            })
            .ok_or(SyntaxSiteSearchError::InvalidPortOutput(
                SyntaxSiteSearchPortOutputError::InvalidCoverage,
            ))?;
    }
    if expected_output_bytes != result.output_bytes {
        return Err(SyntaxSiteSearchError::InvalidPortOutput(
            SyntaxSiteSearchPortOutputError::InvalidCoverage,
        ));
    }
    Ok(())
}

fn check_control<E>(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SyntaxSiteSearchError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(SyntaxSiteSearchError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(SyntaxSiteSearchError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        sync::{Arc, atomic::AtomicBool},
        time::{Duration, Instant},
    };

    use repowitness_analysis::{
        RawSyntaxSite, RawSyntaxSiteAnalysisLimits, RawSyntaxSiteEvidence, RawSyntaxSiteKind,
        RawSyntaxSiteOrdinal,
    };
    use repowitness_domain::{
        AnalysisArtifactDigest, ByteOffset, ByteSpan, RepositoryIdentityDigest, RepositoryPath,
        RepositoryPathLimits, SourceContentDigest, SourceSnapshotDigest,
    };

    use super::{
        MAX_SYNTAX_SITE_SEARCH_TARGET_BYTES, OutboundSitesAvailability, OutboundSyntaxSite,
        RustIndexCoverage, SyntaxSiteSearchError, SyntaxSiteSearchLimits, SyntaxSiteSearchPort,
        SyntaxSiteSearchPortOutputError, SyntaxSiteSearchPortRequest, SyntaxSiteSearchPortResult,
        SyntaxSiteSearchQuery, SyntaxSiteSearchQueryError, SyntaxSiteSearchRequest,
        syntax_site_search,
    };
    use crate::SourceLanguage;

    const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(128, 8);

    struct FakePort {
        calls: Cell<u64>,
        result: Cell<Option<Result<SyntaxSiteSearchPortResult<u64>, std::convert::Infallible>>>,
    }

    impl FakePort {
        fn with(result: Result<SyntaxSiteSearchPortResult<u64>, std::convert::Infallible>) -> Self {
            Self {
                calls: Cell::new(0),
                result: Cell::new(Some(result)),
            }
        }
    }

    impl SyntaxSiteSearchPort for FakePort {
        type Generation = u64;
        type Error = std::convert::Infallible;

        fn syntax_site_search(
            &self,
            _request: SyntaxSiteSearchPortRequest,
        ) -> Result<SyntaxSiteSearchPortResult<Self::Generation>, Self::Error> {
            self.calls.set(self.calls.get() + 1);
            self.result
                .take()
                .expect("fake port should be called at most once")
        }
    }

    fn site(target: &str) -> OutboundSyntaxSite {
        let target_end = u64::try_from(target.len()).expect("fixture target fits");
        let raw_site = RawSyntaxSite::try_new(
            RawSyntaxSiteOrdinal::new(0),
            RawSyntaxSiteKind::Call,
            RawSyntaxSiteEvidence::DirectSyntax,
            ByteSpan::try_new(ByteOffset::ZERO, ByteOffset::new(target_end))
                .expect("fixture occurrence is valid"),
            ByteSpan::try_new(ByteOffset::ZERO, ByteOffset::new(target_end))
                .expect("fixture target span is valid"),
            target.to_owned(),
            RawSyntaxSiteAnalysisLimits::default(),
        )
        .expect("fixture raw syntax site is valid");
        OutboundSyntaxSite::new(
            RepositoryPath::try_from_bytes(b"src/main.rs", PATH_LIMITS)
                .expect("fixture path is valid"),
            SourceContentDigest::new([3; 32]),
            AnalysisArtifactDigest::new([4; 32]),
            SourceLanguage::Rust,
            raw_site,
        )
    }

    fn result(
        sites: Vec<OutboundSyntaxSite>,
        total_sites: u64,
        output_bytes: u64,
    ) -> SyntaxSiteSearchPortResult<u64> {
        SyntaxSiteSearchPortResult::new(
            SourceSnapshotDigest::new([2; 32]),
            7,
            RustIndexCoverage::new(8, 2, 1, 0),
            OutboundSitesAvailability::Complete,
            sites.into_boxed_slice(),
            total_sites,
            output_bytes,
        )
    }

    fn request(target: &str) -> SyntaxSiteSearchRequest {
        SyntaxSiteSearchRequest::new(
            RepositoryIdentityDigest::new([1; 32]),
            SyntaxSiteSearchQuery::try_new(target).expect("fixture query is valid"),
            SyntaxSiteSearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        )
    }

    #[test]
    fn query_is_exact_bounded_and_redacted() {
        let first = SyntaxSiteSearchQuery::try_new("value::target").expect("query is valid");
        let second = SyntaxSiteSearchQuery::try_new("value::target").expect("query is valid");
        assert_eq!(first, second);
        assert!(format!("{first:?}").contains("<redacted-raw-target>"));
        assert_eq!(
            SyntaxSiteSearchQuery::try_new(""),
            Err(SyntaxSiteSearchQueryError::Empty)
        );
        assert_eq!(
            SyntaxSiteSearchQuery::try_new(&"x".repeat(MAX_SYNTAX_SITE_SEARCH_TARGET_BYTES + 1)),
            Err(SyntaxSiteSearchQueryError::TooLong)
        );
    }

    #[test]
    fn exact_observations_remain_generation_pinned_and_unresolved() {
        let observation = site("run");
        let port = FakePort::with(Ok(result(
            vec![observation],
            2,
            176 + u64::try_from(b"src/main.rs".len()).expect("fixture path fits") + 3,
        )));

        let searched = syntax_site_search(&port, request("run")).expect("search should succeed");

        assert_eq!(port.calls.get(), 1);
        assert_eq!(searched.generation(), &7);
        assert_eq!(searched.snapshot(), SourceSnapshotDigest::new([2; 32]));
        assert_eq!(searched.claim().returned_sites(), 1);
        assert_eq!(searched.claim().total_sites(), 2);
        assert_eq!(
            searched.index_coverage(),
            RustIndexCoverage::new(8, 2, 1, 0)
        );
        assert_eq!(searched.availability(), OutboundSitesAvailability::Complete);
        assert_eq!(searched.sites().len(), 1);
        assert_eq!(searched.sites()[0].site().raw_target(), "run");
        assert_eq!(
            searched.notice(),
            super::SyntaxSiteSearchNotice::ExactRawTargetOnlyNoResolutionOrInferredEdges
        );
    }

    #[test]
    fn unmatched_or_misaccounted_adapter_observations_are_rejected() {
        let mismatched_target = FakePort::with(Ok(result(vec![site("other")], 1, 194)));
        assert!(matches!(
            syntax_site_search(&mismatched_target, request("run")),
            Err(SyntaxSiteSearchError::InvalidPortOutput(
                SyntaxSiteSearchPortOutputError::InvalidObservation
            ))
        ));

        let misaccounted = FakePort::with(Ok(result(vec![site("run")], 1, 1)));
        assert!(matches!(
            syntax_site_search(&misaccounted, request("run")),
            Err(SyntaxSiteSearchError::InvalidPortOutput(
                SyntaxSiteSearchPortOutputError::InvalidCoverage
            ))
        ));
    }
}
