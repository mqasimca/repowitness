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

use crate::{RustIndexCoverage, RustSymbolOccurrence};

/// Version of the exact Rust-symbol retrieval profile.
pub const SYMBOL_GET_PROFILE_VERSION: u16 = 1;
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
    producer_manifest: ProducerManifestDigest,
    index_coverage: RustIndexCoverage,
    candidate: Option<SymbolGetCandidate>,
}

impl<G> SymbolGetPortResult<G> {
    /// Constructs an adapter response for application validation.
    #[must_use]
    pub const fn new(
        snapshot: SourceSnapshotDigest,
        generation: G,
        producer_manifest: ProducerManifestDigest,
        index_coverage: RustIndexCoverage,
        candidate: Option<SymbolGetCandidate>,
    ) -> Self {
        Self {
            snapshot,
            generation,
            producer_manifest,
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

/// Fixed producer class for direct Phase 0 exact-symbol evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolGetProducer {
    /// Bounded Tree-sitter Rust syntax extraction.
    RustSyntax,
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

/// Retrieves one exact active-generation occurrence with attributed evidence.
pub fn symbol_get<Port>(
    port: &Port,
    request: SymbolGetRequest<Port::Generation>,
) -> Result<SymbolGetResult<Port::Generation>, SymbolGetError<Port::Error>>
where
    Port: SymbolGetPort,
{
    check_control(&request.cancelled, request.deadline)?;
    let result = port
        .get(SymbolGetPortRequest {
            repository: request.repository,
            expected_snapshot: request.expected_snapshot,
            expected_generation: request.expected_generation,
            selector: request.selector.clone(),
            limits: request.limits,
            cancelled: Arc::clone(&request.cancelled),
            deadline: request.deadline,
        })
        .map_err(SymbolGetError::Port)?;
    check_control(&request.cancelled, request.deadline)?;
    validate_context(&request, &result)?;
    let candidate = validate_candidate(&request.selector, result.candidate, request.limits)?;
    let coverage = symbol_coverage(result.index_coverage, candidate.is_none())?;
    let (symbol, evidence) = symbol_and_evidence(
        request.repository,
        result.snapshot,
        result.producer_manifest,
        candidate,
    )?;
    let resolution = if symbol.is_some() {
        ResolutionStatus::Confirmed
    } else {
        ResolutionStatus::Unresolved
    };
    MaterialResult::try_new(
        SymbolGetClaim {
            selector: request.selector,
            symbol,
        },
        evidence,
        resolution,
        result.snapshot,
        result.generation,
        symbol_notices()?,
        coverage,
    )
    .map_err(SymbolGetError::MaterialResult)
}

fn validate_context<G: Eq, E>(
    request: &SymbolGetRequest<G>,
    result: &SymbolGetPortResult<G>,
) -> Result<(), SymbolGetError<E>> {
    if result.snapshot != request.expected_snapshot
        || result.generation != request.expected_generation
    {
        return Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::ContextMismatch,
        ));
    }
    Ok(())
}

fn validate_candidate<E>(
    selector: &SymbolGetSelector,
    candidate: Option<SymbolGetCandidate>,
    limits: SymbolGetLimits,
) -> Result<Option<SymbolGetCandidate>, SymbolGetError<E>> {
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    if candidate.path != selector.path
        || candidate.content_digest != selector.content_digest
        || candidate.occurrence.artifact_digest() != selector.artifact_digest
        || candidate.occurrence.fact_ordinal() != selector.fact_ordinal
    {
        return Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::SelectorMismatch,
        ));
    }
    validate_declaration(&candidate, limits)?;
    Ok(Some(candidate))
}

fn validate_declaration<E>(
    candidate: &SymbolGetCandidate,
    limits: SymbolGetLimits,
) -> Result<(), SymbolGetError<E>> {
    let declaration_bytes = fixed_count(candidate.declaration.len())?;
    if declaration_bytes > limits.max_declaration_bytes() {
        return Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::DeclarationLimitExceeded,
        ));
    }
    let occurrence = &candidate.occurrence;
    if declaration_bytes != occurrence.declaration_span().len().get()
        || !declaration_contains_exact_name(occurrence, &candidate.declaration)
    {
        return Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::InvalidDeclaration,
        ));
    }
    let output_bytes = FIXED_OCCURRENCE_OUTPUT_BYTES
        .checked_add(fixed_count(candidate.path.as_bytes().len())?)
        .and_then(|bytes| bytes.checked_add(declaration_bytes))
        .and_then(|bytes| bytes.checked_add(u64::try_from(occurrence.name().len()).ok()?))
        .and_then(|bytes| bytes.checked_add(u64::try_from(occurrence.qualified_name().len()).ok()?))
        .ok_or(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::CountNotRepresentable,
        ))?;
    if output_bytes > limits.max_output_bytes() {
        return Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::OutputByteLimitExceeded,
        ));
    }
    Ok(())
}

fn declaration_contains_exact_name(occurrence: &RustSymbolOccurrence, declaration: &[u8]) -> bool {
    let declaration_start = occurrence.declaration_span().start().get();
    let Some(relative_start) = occurrence
        .name_span()
        .start()
        .get()
        .checked_sub(declaration_start)
    else {
        return false;
    };
    let Some(relative_end) = occurrence
        .name_span()
        .end()
        .get()
        .checked_sub(declaration_start)
    else {
        return false;
    };
    let (Ok(relative_start), Ok(relative_end)) = (
        usize::try_from(relative_start),
        usize::try_from(relative_end),
    ) else {
        return false;
    };
    declaration.get(relative_start..relative_end) == Some(occurrence.name().as_bytes())
}

fn fixed_count<E>(count: usize) -> Result<u64, SymbolGetError<E>> {
    u64::try_from(count).map_err(|_| {
        SymbolGetError::InvalidPortOutput(SymbolGetPortOutputError::CountNotRepresentable)
    })
}

fn symbol_coverage<E>(
    index: RustIndexCoverage,
    missing: bool,
) -> Result<CoverageSummary, SymbolGetError<E>> {
    let unresolved = index.unresolved().checked_add(u64::from(missing)).ok_or(
        SymbolGetError::InvalidPortOutput(SymbolGetPortOutputError::CountNotRepresentable),
    )?;
    Ok(CoverageSummary::new(
        CoverageItemCount::new(index.searched()),
        CoverageItemCount::new(index.skipped()),
        CoverageItemCount::new(unresolved),
        CoverageItemCount::new(index.truncated()),
    ))
}

type SymbolEvidence =
    BoundedResultItems<EvidenceRecord<SymbolGetEvidenceIdentity, SymbolGetProducerIdentity>>;
type SymbolAndEvidence = (Option<RetrievedSymbol>, SymbolEvidence);

fn symbol_and_evidence<E>(
    repository: RepositoryIdentityDigest,
    snapshot: SourceSnapshotDigest,
    producer_manifest: ProducerManifestDigest,
    candidate: Option<SymbolGetCandidate>,
) -> Result<SymbolAndEvidence, SymbolGetError<E>> {
    let Some(candidate) = candidate else {
        let evidence = BoundedResultItems::try_from_vec(Vec::new(), ResultItemLimit::new(1))
            .map_err(SymbolGetError::ResultItems)?;
        return Ok((None, evidence));
    };
    let evidence_occurrence = candidate.occurrence.clone();
    let evidence = EvidenceRecord::new(
        EvidenceIdentity::new(
            repository,
            snapshot,
            candidate.path,
            candidate.content_digest,
            EvidenceLocation::SymbolOccurrence(evidence_occurrence),
        ),
        ProducerIdentity::new(SymbolGetProducer::RustSyntax, producer_manifest),
        EvidenceTier::Syntax,
        EvidenceRelation::Supports,
    );
    let evidence = BoundedResultItems::try_from_vec(vec![evidence], ResultItemLimit::new(1))
        .map_err(SymbolGetError::ResultItems)?;
    Ok((
        Some(RetrievedSymbol {
            occurrence: candidate.occurrence,
            declaration: candidate.declaration,
        }),
        evidence,
    ))
}

fn symbol_notices<E>()
-> Result<BoundedResultItems<ResultNotice<SymbolGetNotice>>, SymbolGetError<E>> {
    BoundedResultItems::try_from_vec(
        vec![ResultNotice::new(
            ResultNoticeKind::Limitation,
            SymbolGetNotice::DefinitionOnlyNoReferences,
        )],
        ResultItemLimit::new(1),
    )
    .map_err(SymbolGetError::ResultItems)
}

fn check_control<E>(cancelled: &AtomicBool, deadline: Instant) -> Result<(), SymbolGetError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(SymbolGetError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(SymbolGetError::DeadlineExceeded)
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
        AnalysisArtifactDigest, ByteOffset, ByteSpan, EvidenceLocation, ProducerManifestDigest,
        RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits, ResolutionStatus,
        SourceContentDigest, SourceSnapshotDigest,
    };

    use super::{
        MAX_SYMBOL_GET_DECLARATION_BYTES, MAX_SYMBOL_GET_OUTPUT_BYTES, SYMBOL_GET_PROFILE_VERSION,
        SymbolGetCandidate, SymbolGetError, SymbolGetLimits, SymbolGetPort,
        SymbolGetPortOutputError, SymbolGetPortRequest, SymbolGetPortResult, SymbolGetProducer,
        SymbolGetRequest, SymbolGetSelector, symbol_get,
    };
    use crate::{RustIndexCoverage, RustSymbolOccurrence};

    const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(128, 8);
    const DECLARATION: &[u8] = b"fn Widget() {}";

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum FakeError {
        Failed,
    }

    impl std::fmt::Display for FakeError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("fake symbol retrieval failed")
        }
    }

    impl std::error::Error for FakeError {}

    struct FakePort {
        calls: Cell<u64>,
        result: Cell<Option<Result<SymbolGetPortResult<u64>, FakeError>>>,
    }

    impl FakePort {
        fn with(result: Result<SymbolGetPortResult<u64>, FakeError>) -> Self {
            Self {
                calls: Cell::new(0),
                result: Cell::new(Some(result)),
            }
        }
    }

    impl SymbolGetPort for FakePort {
        type Generation = u64;
        type Error = FakeError;

        fn get(
            &self,
            _request: SymbolGetPortRequest<Self::Generation>,
        ) -> Result<SymbolGetPortResult<Self::Generation>, Self::Error> {
            self.calls.set(self.calls.get() + 1);
            self.result
                .take()
                .expect("fake port should be called at most once")
        }
    }

    fn path() -> RepositoryPath {
        RepositoryPath::try_from_bytes(b"src/lib.rs", PATH_LIMITS).expect("fixture path is valid")
    }

    fn selector() -> SymbolGetSelector {
        SymbolGetSelector::new(
            path(),
            SourceContentDigest::new([3; 32]),
            AnalysisArtifactDigest::new([4; 32]),
            5,
        )
    }

    fn occurrence() -> RustSymbolOccurrence {
        RustSymbolOccurrence::try_new(
            5,
            AnalysisArtifactDigest::new([4; 32]),
            RustSymbolKind::Function,
            "Widget".to_owned(),
            "fixture::Widget".to_owned(),
            ByteSpan::try_new(ByteOffset::new(3), ByteOffset::new(9))
                .expect("fixture name span is valid"),
            ByteSpan::try_new(ByteOffset::new(0), ByteOffset::new(14))
                .expect("fixture declaration span is valid"),
        )
        .expect("fixture occurrence is valid")
    }

    fn candidate() -> SymbolGetCandidate {
        SymbolGetCandidate::new(
            path(),
            SourceContentDigest::new([3; 32]),
            occurrence(),
            Box::from(DECLARATION),
        )
    }

    fn result(candidate: Option<SymbolGetCandidate>) -> SymbolGetPortResult<u64> {
        SymbolGetPortResult::new(
            SourceSnapshotDigest::new([2; 32]),
            7,
            ProducerManifestDigest::new([6; 32]),
            RustIndexCoverage::new(8, 2, 1, 3),
            candidate,
        )
    }

    fn request(
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
        limits: SymbolGetLimits,
    ) -> SymbolGetRequest<u64> {
        SymbolGetRequest::new(
            RepositoryIdentityDigest::new([1; 32]),
            SourceSnapshotDigest::new([2; 32]),
            7,
            selector(),
            limits,
            cancelled,
            deadline,
        )
    }

    #[test]
    fn limits_enforce_inclusive_phase0_ceilings() {
        assert!(
            SymbolGetLimits::try_new(
                MAX_SYMBOL_GET_DECLARATION_BYTES,
                MAX_SYMBOL_GET_OUTPUT_BYTES
            )
            .is_ok()
        );
        assert!(SymbolGetLimits::try_new(0, 1).is_err());
        assert!(SymbolGetLimits::try_new(MAX_SYMBOL_GET_DECLARATION_BYTES + 1, 1).is_err());
        assert!(SymbolGetLimits::try_new(1, 0).is_err());
        assert!(SymbolGetLimits::try_new(1, MAX_SYMBOL_GET_OUTPUT_BYTES + 1).is_err());
    }

    #[test]
    fn exact_candidate_becomes_verified_source_and_attributed_evidence() {
        let port = FakePort::with(Ok(result(Some(candidate()))));
        let material = symbol_get(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
                SymbolGetLimits::default(),
            ),
        )
        .expect("exact symbol lookup should succeed");

        assert_eq!(port.calls.get(), 1);
        assert_eq!(material.resolution(), ResolutionStatus::Confirmed);
        assert_eq!(material.generation(), &7);
        assert_eq!(material.snapshot(), &SourceSnapshotDigest::new([2; 32]));
        assert_eq!(
            material.claim().profile_version(),
            SYMBOL_GET_PROFILE_VERSION
        );
        assert_eq!(
            material
                .claim()
                .symbol()
                .expect("symbol should resolve")
                .declaration(),
            DECLARATION
        );
        assert_eq!(material.coverage().searched().get(), 8);
        assert_eq!(material.coverage().skipped().get(), 2);
        assert_eq!(material.coverage().unresolved().get(), 1);
        assert_eq!(material.coverage().truncated().get(), 3);
        let evidence = material.evidence().as_slice();
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].producer().id(), &SymbolGetProducer::RustSyntax);
        assert_eq!(evidence[0].tier(), repowitness_domain::EvidenceTier::Syntax);
        let EvidenceLocation::SymbolOccurrence(occurrence) = evidence[0].identity().location()
        else {
            panic!("symbol evidence should identify one occurrence");
        };
        assert_eq!(occurrence.name(), "Widget");
    }

    #[test]
    fn a_missing_exact_occurrence_abstains_with_unresolved_coverage() {
        let port = FakePort::with(Ok(result(None)));
        let material = symbol_get(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
                SymbolGetLimits::default(),
            ),
        )
        .expect("missing symbol should be an unresolved result");

        assert_eq!(material.resolution(), ResolutionStatus::Unresolved);
        assert!(material.claim().symbol().is_none());
        assert!(material.evidence().is_empty());
        assert_eq!(material.coverage().unresolved().get(), 2);
    }

    #[test]
    fn context_selector_and_declaration_mismatches_fail_closed() {
        let wrong_context = SymbolGetPortResult::new(
            SourceSnapshotDigest::new([9; 32]),
            7,
            ProducerManifestDigest::new([6; 32]),
            RustIndexCoverage::new(1, 0, 0, 0),
            Some(candidate()),
        );
        let port = FakePort::with(Ok(wrong_context));
        assert!(matches!(
            symbol_get(
                &port,
                request(
                    Arc::new(AtomicBool::new(false)),
                    Instant::now() + Duration::from_secs(1),
                    SymbolGetLimits::default(),
                )
            ),
            Err(SymbolGetError::InvalidPortOutput(
                SymbolGetPortOutputError::ContextMismatch
            ))
        ));

        let wrong_selector = SymbolGetCandidate::new(
            path(),
            SourceContentDigest::new([8; 32]),
            occurrence(),
            Box::from(DECLARATION),
        );
        let port = FakePort::with(Ok(result(Some(wrong_selector))));
        assert!(matches!(
            symbol_get(
                &port,
                request(
                    Arc::new(AtomicBool::new(false)),
                    Instant::now() + Duration::from_secs(1),
                    SymbolGetLimits::default(),
                )
            ),
            Err(SymbolGetError::InvalidPortOutput(
                SymbolGetPortOutputError::SelectorMismatch
            ))
        ));

        let wrong_source = SymbolGetCandidate::new(
            path(),
            SourceContentDigest::new([3; 32]),
            occurrence(),
            Box::from(&b"fn Gadget() {}"[..]),
        );
        let port = FakePort::with(Ok(result(Some(wrong_source))));
        assert!(matches!(
            symbol_get(
                &port,
                request(
                    Arc::new(AtomicBool::new(false)),
                    Instant::now() + Duration::from_secs(1),
                    SymbolGetLimits::default(),
                )
            ),
            Err(SymbolGetError::InvalidPortOutput(
                SymbolGetPortOutputError::InvalidDeclaration
            ))
        ));
    }

    #[test]
    fn declaration_and_aggregate_output_bounds_are_rechecked_by_the_use_case() {
        let declaration_limit =
            SymbolGetLimits::try_new(13, MAX_SYMBOL_GET_OUTPUT_BYTES).expect("limits are valid");
        let port = FakePort::with(Ok(result(Some(candidate()))));
        assert!(matches!(
            symbol_get(
                &port,
                request(
                    Arc::new(AtomicBool::new(false)),
                    Instant::now() + Duration::from_secs(1),
                    declaration_limit,
                )
            ),
            Err(SymbolGetError::InvalidPortOutput(
                SymbolGetPortOutputError::DeclarationLimitExceeded
            ))
        ));

        let output_limit =
            SymbolGetLimits::try_new(MAX_SYMBOL_GET_DECLARATION_BYTES, 200).expect("limits valid");
        let port = FakePort::with(Ok(result(Some(candidate()))));
        assert!(matches!(
            symbol_get(
                &port,
                request(
                    Arc::new(AtomicBool::new(false)),
                    Instant::now() + Duration::from_secs(1),
                    output_limit,
                )
            ),
            Err(SymbolGetError::InvalidPortOutput(
                SymbolGetPortOutputError::OutputByteLimitExceeded
            ))
        ));
    }

    #[test]
    fn cancellation_deadline_port_errors_and_debug_output_remain_safe() {
        let cancelled_port = FakePort::with(Err(FakeError::Failed));
        let cancelled = symbol_get(
            &cancelled_port,
            request(
                Arc::new(AtomicBool::new(true)),
                Instant::now() + Duration::from_secs(1),
                SymbolGetLimits::default(),
            ),
        )
        .expect_err("pre-cancelled work should fail");
        assert!(matches!(cancelled, SymbolGetError::Cancelled));
        assert_eq!(cancelled_port.calls.get(), 0);

        let deadline_port = FakePort::with(Err(FakeError::Failed));
        let elapsed = symbol_get(
            &deadline_port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now(),
                SymbolGetLimits::default(),
            ),
        )
        .expect_err("elapsed deadline should fail");
        assert!(matches!(elapsed, SymbolGetError::DeadlineExceeded));
        assert_eq!(deadline_port.calls.get(), 0);

        let failure_port = FakePort::with(Err(FakeError::Failed));
        let failure = symbol_get(
            &failure_port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
                SymbolGetLimits::default(),
            ),
        )
        .expect_err("port failure should remain distinct");
        assert!(matches!(failure, SymbolGetError::Port(FakeError::Failed)));

        struct CancellingPort;
        impl SymbolGetPort for CancellingPort {
            type Generation = u64;
            type Error = FakeError;

            fn get(
                &self,
                request: SymbolGetPortRequest<Self::Generation>,
            ) -> Result<SymbolGetPortResult<Self::Generation>, Self::Error> {
                request.cancelled().store(true, Ordering::Release);
                Ok(result(Some(candidate())))
            }
        }
        let request = request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
            SymbolGetLimits::default(),
        );
        let debug = format!("{request:?}");
        assert!(debug.contains("SymbolGetSelector"));
        assert!(!debug.contains("src/lib.rs"));
        assert!(matches!(
            symbol_get(&CancellingPort, request),
            Err(SymbolGetError::Cancelled)
        ));
    }
}
