//! Exact unresolved raw syntax observations contained in one selected declaration.
//!
//! The profile deliberately exposes no resolved target, correspondence, graph
//! edge, or same-name association. A site is only an attributed parser
//! observation with exact source spans.

use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_analysis::RawSyntaxSite;
use repowitness_domain::{
    AnalysisArtifactDigest, ByteSpan, RepositoryIdentityDigest, RepositoryPath,
    SourceContentDigest, SourceSnapshotDigest,
};

use crate::{RustIndexCoverage, SourceLanguage, SymbolGetSelector};

/// Version of the exact declaration-contained raw-site profile.
pub const OUTBOUND_SITES_PROFILE_VERSION: u16 = 1;
/// Default number of retained raw sites from one declaration.
pub const DEFAULT_OUTBOUND_SITES_RESULTS: u16 = 100;
/// Hard number of retained raw sites from one declaration.
pub const MAX_OUTBOUND_SITES_RESULTS: u16 = 1_000;
/// Default conservative encoded-output ceiling.
pub const DEFAULT_OUTBOUND_SITES_OUTPUT_BYTES: u64 = 512 * 1024;
/// Hard conservative encoded-output ceiling.
pub const MAX_OUTBOUND_SITES_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;

const FIXED_OUTBOUND_SITE_OUTPUT_BYTES: u64 = 176;

/// Failure to construct bounded outbound-site read limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutboundSitesLimitError;

impl fmt::Display for OutboundSitesLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("outbound-site limits are zero or exceed compiled ceilings")
    }
}

impl Error for OutboundSitesLimitError {}

/// Independent bounded result limits for one selected declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutboundSitesLimits {
    max_results: u16,
    max_output_bytes: u64,
}

impl OutboundSitesLimits {
    /// Validates the fixed public profile bounds.
    pub const fn try_new(
        max_results: u16,
        max_output_bytes: u64,
    ) -> Result<Self, OutboundSitesLimitError> {
        if max_results == 0
            || max_results > MAX_OUTBOUND_SITES_RESULTS
            || max_output_bytes == 0
            || max_output_bytes > MAX_OUTBOUND_SITES_OUTPUT_BYTES
        {
            return Err(OutboundSitesLimitError);
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

impl Default for OutboundSitesLimits {
    fn default() -> Self {
        Self {
            max_results: DEFAULT_OUTBOUND_SITES_RESULTS,
            max_output_bytes: DEFAULT_OUTBOUND_SITES_OUTPUT_BYTES,
        }
    }
}

/// Categorical projection availability; it never claims a semantic absence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboundSitesAvailability {
    /// A complete raw-site projection exists for the selected generation.
    Complete,
    /// The selected generation did not produce this projection.
    NotProduced,
}

/// Exact direct declaration context returned by an adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutboundSitesDeclaration {
    language: SourceLanguage,
    declaration_span: ByteSpan,
}

impl OutboundSitesDeclaration {
    /// Constructs an untrusted declaration context for application validation.
    #[must_use]
    pub const fn new(language: SourceLanguage, declaration_span: ByteSpan) -> Self {
        Self {
            language,
            declaration_span,
        }
    }

    /// Returns the exact selected language/dialect.
    #[must_use]
    pub const fn language(self) -> SourceLanguage {
        self.language
    }

    /// Returns the complete selected declaration span.
    #[must_use]
    pub const fn declaration_span(self) -> ByteSpan {
        self.declaration_span
    }
}

/// One unresolved, exact raw syntax observation emitted by the parser.
#[derive(Clone, Eq, PartialEq)]
pub struct OutboundSyntaxSite {
    path: RepositoryPath,
    content_digest: SourceContentDigest,
    artifact_digest: AnalysisArtifactDigest,
    language: SourceLanguage,
    site: RawSyntaxSite,
}

impl OutboundSyntaxSite {
    /// Constructs one untrusted storage-adapter observation.
    #[must_use]
    pub const fn new(
        path: RepositoryPath,
        content_digest: SourceContentDigest,
        artifact_digest: AnalysisArtifactDigest,
        language: SourceLanguage,
        site: RawSyntaxSite,
    ) -> Self {
        Self {
            path,
            content_digest,
            artifact_digest,
            language,
            site,
        }
    }

    /// Returns the exact physical path containing the observation.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the exact source-content identity.
    #[must_use]
    pub const fn content_digest(&self) -> SourceContentDigest {
        self.content_digest
    }

    /// Returns the raw-site artifact identity, never a resolved target.
    #[must_use]
    pub const fn artifact_digest(&self) -> AnalysisArtifactDigest {
        self.artifact_digest
    }

    /// Returns the syntax language/dialect.
    #[must_use]
    pub const fn language(&self) -> SourceLanguage {
        self.language
    }

    /// Returns the exact unresolved raw observation and parser evidence tier.
    #[must_use]
    pub const fn site(&self) -> &RawSyntaxSite {
        &self.site
    }
}

impl fmt::Debug for OutboundSyntaxSite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OutboundSyntaxSite")
            .field("path", &self.path)
            .field("content_digest", &self.content_digest)
            .field("artifact_digest", &self.artifact_digest)
            .field("language", &self.language)
            .field("site", &self.site)
            .finish()
    }
}

/// Complete untrusted adapter answer pinned to one expected active generation.
pub struct OutboundSitesPortResult<G> {
    snapshot: SourceSnapshotDigest,
    generation: G,
    index_coverage: RustIndexCoverage,
    declaration: Option<OutboundSitesDeclaration>,
    availability: OutboundSitesAvailability,
    sites: Box<[OutboundSyntaxSite]>,
    total_sites: u64,
    output_bytes: u64,
}

impl<G> OutboundSitesPortResult<G> {
    /// Constructs the complete adapter response for application validation.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "the adapter output's immutable context, categorical coverage, and every bounded field are explicit"
    )]
    pub const fn new(
        snapshot: SourceSnapshotDigest,
        generation: G,
        index_coverage: RustIndexCoverage,
        declaration: Option<OutboundSitesDeclaration>,
        availability: OutboundSitesAvailability,
        sites: Box<[OutboundSyntaxSite]>,
        total_sites: u64,
        output_bytes: u64,
    ) -> Self {
        Self {
            snapshot,
            generation,
            index_coverage,
            declaration,
            availability,
            sites,
            total_sites,
            output_bytes,
        }
    }
}

/// Complete validated input passed to one exact raw-site adapter call.
pub struct OutboundSitesPortRequest<G> {
    repository: RepositoryIdentityDigest,
    expected_snapshot: SourceSnapshotDigest,
    expected_generation: G,
    selector: SymbolGetSelector,
    limits: OutboundSitesLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl<G> OutboundSitesPortRequest<G> {
    /// Returns the exact repository identity.
    #[must_use]
    pub const fn repository(&self) -> RepositoryIdentityDigest {
        self.repository
    }

    /// Returns the required active snapshot.
    #[must_use]
    pub const fn expected_snapshot(&self) -> SourceSnapshotDigest {
        self.expected_snapshot
    }

    /// Returns the required active generation.
    #[must_use]
    pub const fn expected_generation(&self) -> &G {
        &self.expected_generation
    }

    /// Returns the exact selected declaration occurrence.
    #[must_use]
    pub const fn selector(&self) -> &SymbolGetSelector {
        &self.selector
    }

    /// Returns explicit bounded read limits.
    #[must_use]
    pub const fn limits(&self) -> OutboundSitesLimits {
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

/// Narrow exact declaration-contained syntax-site boundary.
pub trait OutboundSitesPort {
    /// Opaque immutable generation identity owned by the adapter.
    type Generation: Copy + Eq;
    /// Stable adapter failure mapped at its boundary.
    type Error;

    /// Reads only raw sites physically contained in the selected declaration.
    fn outbound_sites(
        &self,
        request: OutboundSitesPortRequest<Self::Generation>,
    ) -> Result<OutboundSitesPortResult<Self::Generation>, Self::Error>;
}

/// Application request shared by local CLI and MCP adapters.
pub struct OutboundSitesRequest<G> {
    repository: RepositoryIdentityDigest,
    expected_snapshot: SourceSnapshotDigest,
    expected_generation: G,
    selector: SymbolGetSelector,
    limits: OutboundSitesLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl<G> OutboundSitesRequest<G> {
    /// Constructs a request from already validated boundary values.
    #[must_use]
    pub const fn new(
        repository: RepositoryIdentityDigest,
        expected_snapshot: SourceSnapshotDigest,
        expected_generation: G,
        selector: SymbolGetSelector,
        limits: OutboundSitesLimits,
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

impl<G> fmt::Debug for OutboundSitesRequest<G> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OutboundSitesRequest")
            .field("repository", &self.repository)
            .field("expected_snapshot", &self.expected_snapshot)
            .field("expected_generation", &"<opaque-generation>")
            .field("selector", &self.selector)
            .field("limits", &self.limits)
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Fixed v1 limitation retained with every exact raw-site answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboundSitesNotice {
    /// Raw target spellings remain unresolved; no graph edges or correspondences exist.
    NoTargetResolutionOrInferredEdges,
}

/// Complete validated result for one exact declaration selector.
#[derive(Eq, PartialEq)]
pub struct OutboundSitesResult<G> {
    selector: SymbolGetSelector,
    snapshot: SourceSnapshotDigest,
    generation: G,
    index_coverage: RustIndexCoverage,
    declaration: Option<OutboundSitesDeclaration>,
    availability: OutboundSitesAvailability,
    sites: Box<[OutboundSyntaxSite]>,
    total_sites: u64,
    output_bytes: u64,
    notice: OutboundSitesNotice,
}

impl<G> OutboundSitesResult<G> {
    /// Returns the exact declaration selector that scopes every site.
    #[must_use]
    pub const fn selector(&self) -> &SymbolGetSelector {
        &self.selector
    }

    /// Returns the concrete source snapshot used by the complete read.
    #[must_use]
    pub const fn snapshot(&self) -> SourceSnapshotDigest {
        self.snapshot
    }

    /// Returns the concrete immutable generation used by the complete read.
    #[must_use]
    pub const fn generation(&self) -> &G {
        &self.generation
    }

    /// Returns recorded source-index coverage.
    #[must_use]
    pub const fn index_coverage(&self) -> RustIndexCoverage {
        self.index_coverage
    }

    /// Returns the declaration context when the exact selector still exists.
    #[must_use]
    pub const fn declaration(&self) -> Option<OutboundSitesDeclaration> {
        self.declaration
    }

    /// Returns categorical raw-projection availability.
    #[must_use]
    pub const fn availability(&self) -> OutboundSitesAvailability {
        self.availability
    }

    /// Returns exact raw observations in deterministic source order.
    #[must_use]
    pub const fn sites(&self) -> &[OutboundSyntaxSite] {
        &self.sites
    }

    /// Returns the exact count before the explicit result bound.
    #[must_use]
    pub const fn total_sites(&self) -> u64 {
        self.total_sites
    }

    /// Returns conservative encoded retained-output bytes.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }

    /// Returns the fixed v1 no-resolution limitation.
    #[must_use]
    pub const fn notice(&self) -> OutboundSitesNotice {
        self.notice
    }
}

impl<G: fmt::Debug> fmt::Debug for OutboundSitesResult<G> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OutboundSitesResult")
            .field("selector", &self.selector)
            .field("snapshot", &self.snapshot)
            .field("generation", &self.generation)
            .field("declaration_present", &self.declaration.is_some())
            .field("availability", &self.availability)
            .field("site_count", &self.sites.len())
            .field("total_sites", &self.total_sites)
            .finish()
    }
}

/// Stable invalid-adapter-output classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboundSitesPortOutputError {
    /// The adapter returned a different immutable context.
    ContextMismatch,
    /// The adapter returned malformed declaration or raw-site metadata.
    InvalidObservation,
    /// The adapter returned a site outside the selected declaration.
    SiteOutsideDeclaration,
    /// The adapter violated a declared bound or count.
    InvalidCoverage,
}

impl fmt::Display for OutboundSitesPortOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ContextMismatch => "outbound-sites adapter returned a different active context",
            Self::InvalidObservation => {
                "outbound-sites adapter returned invalid raw observation data"
            }
            Self::SiteOutsideDeclaration => {
                "outbound-sites adapter returned a site outside the selected declaration"
            }
            Self::InvalidCoverage => "outbound-sites adapter returned invalid bounded coverage",
        })
    }
}

impl Error for OutboundSitesPortOutputError {}

/// Application failure for one exact raw-site read.
#[derive(Debug)]
pub enum OutboundSitesError<E> {
    /// Cancellation was observed before complete output existed.
    Cancelled,
    /// The absolute deadline elapsed before complete output existed.
    DeadlineExceeded,
    /// The storage-neutral adapter failed.
    Port(E),
    /// The adapter violated the exact immutable context or output contract.
    InvalidPortOutput(OutboundSitesPortOutputError),
}

impl<E: fmt::Display> fmt::Display for OutboundSitesError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("outbound-sites read cancelled"),
            Self::DeadlineExceeded => formatter.write_str("outbound-sites read deadline exceeded"),
            Self::Port(error) => write!(formatter, "outbound-sites adapter failed: {error}"),
            Self::InvalidPortOutput(error) => error.fmt(formatter),
        }
    }
}

impl<E: Error + 'static> Error for OutboundSitesError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Port(error) => Some(error),
            Self::InvalidPortOutput(error) => Some(error),
            Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Reads bounded, exact raw syntax observations from one selected declaration.
pub fn outbound_sites<Port>(
    port: &Port,
    request: OutboundSitesRequest<Port::Generation>,
) -> Result<OutboundSitesResult<Port::Generation>, OutboundSitesError<Port::Error>>
where
    Port: OutboundSitesPort,
{
    check_control(&request.cancelled, request.deadline)?;
    let result = port
        .outbound_sites(OutboundSitesPortRequest {
            repository: request.repository,
            expected_snapshot: request.expected_snapshot,
            expected_generation: request.expected_generation,
            selector: request.selector.clone(),
            limits: request.limits,
            cancelled: Arc::clone(&request.cancelled),
            deadline: request.deadline,
        })
        .map_err(OutboundSitesError::Port)?;
    check_control(&request.cancelled, request.deadline)?;
    validate_result(&request, &result)?;
    Ok(OutboundSitesResult {
        selector: request.selector,
        snapshot: result.snapshot,
        generation: result.generation,
        index_coverage: result.index_coverage,
        declaration: result.declaration,
        availability: result.availability,
        sites: result.sites,
        total_sites: result.total_sites,
        output_bytes: result.output_bytes,
        notice: OutboundSitesNotice::NoTargetResolutionOrInferredEdges,
    })
}

fn validate_result<G: Eq, E>(
    request: &OutboundSitesRequest<G>,
    result: &OutboundSitesPortResult<G>,
) -> Result<(), OutboundSitesError<E>> {
    if result.snapshot != request.expected_snapshot
        || result.generation != request.expected_generation
    {
        return Err(OutboundSitesError::InvalidPortOutput(
            OutboundSitesPortOutputError::ContextMismatch,
        ));
    }
    if result.sites.len() > usize::from(request.limits.max_results())
        || result.total_sites < u64::try_from(result.sites.len()).unwrap_or(u64::MAX)
        || result.output_bytes > request.limits.max_output_bytes()
    {
        return Err(OutboundSitesError::InvalidPortOutput(
            OutboundSitesPortOutputError::InvalidCoverage,
        ));
    }
    let Some(declaration) = result.declaration else {
        if !result.sites.is_empty() || result.total_sites != 0 || result.output_bytes != 0 {
            return Err(OutboundSitesError::InvalidPortOutput(
                OutboundSitesPortOutputError::InvalidObservation,
            ));
        }
        return Ok(());
    };
    if !declaration
        .language
        .matches_repository_path(request.selector.path())
    {
        return Err(OutboundSitesError::InvalidPortOutput(
            OutboundSitesPortOutputError::InvalidObservation,
        ));
    }
    let mut expected_output_bytes = 0_u64;
    let mut previous = None;
    for site in &result.sites {
        if site.path != *request.selector.path()
            || site.content_digest != request.selector.content_digest()
            || site.language != declaration.language
            || !site.language.matches_repository_path(&site.path)
        {
            return Err(OutboundSitesError::InvalidPortOutput(
                OutboundSitesPortOutputError::InvalidObservation,
            ));
        }
        let occurrence = site.site.occurrence_span();
        if occurrence.start() < declaration.declaration_span.start()
            || occurrence.end() > declaration.declaration_span.end()
        {
            return Err(OutboundSitesError::InvalidPortOutput(
                OutboundSitesPortOutputError::SiteOutsideDeclaration,
            ));
        }
        let order = (
            occurrence.start().get(),
            occurrence.end().get(),
            site.site.target_span().start().get(),
            site.site.target_span().end().get(),
            site.site.ordinal().get(),
        );
        if previous.is_some_and(|previous| previous > order) {
            return Err(OutboundSitesError::InvalidPortOutput(
                OutboundSitesPortOutputError::InvalidObservation,
            ));
        }
        previous = Some(order);
        expected_output_bytes = expected_output_bytes
            .checked_add(FIXED_OUTBOUND_SITE_OUTPUT_BYTES)
            .and_then(|value| value.checked_add(u64::try_from(site.path.as_bytes().len()).ok()?))
            .and_then(|value| value.checked_add(u64::try_from(site.site.raw_target().len()).ok()?))
            .ok_or(OutboundSitesError::InvalidPortOutput(
                OutboundSitesPortOutputError::InvalidCoverage,
            ))?;
    }
    if expected_output_bytes != result.output_bytes {
        return Err(OutboundSitesError::InvalidPortOutput(
            OutboundSitesPortOutputError::InvalidCoverage,
        ));
    }
    Ok(())
}

fn check_control<E>(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), OutboundSitesError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(OutboundSitesError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(OutboundSitesError::DeadlineExceeded)
    } else {
        Ok(())
    }
}
