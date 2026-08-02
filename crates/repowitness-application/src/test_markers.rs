//! Bounded repository-scoped raw test-marker navigation.
//!
//! A marker is a parser observation.  It is not proof that a test is runnable,
//! associated with a declaration, or related to any other source fact.

use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_analysis::RawSyntaxSiteKind;
use repowitness_domain::{RepositoryIdentityDigest, SourceSnapshotDigest};

use crate::{OutboundSyntaxSite, RustIndexCoverage, SourceLanguage};

/// Version of the repository-scoped raw test-marker profile.
pub const TEST_MARKERS_PROFILE_VERSION: u16 = 1;
/// Default retained marker count.
pub const DEFAULT_TEST_MARKER_RESULTS: u16 = 100;
/// Hard retained marker count.
pub const MAX_TEST_MARKER_RESULTS: u16 = 1_000;
/// Default conservative output ceiling.
pub const DEFAULT_TEST_MARKER_OUTPUT_BYTES: u64 = 512 * 1024;
/// Hard conservative output ceiling.
pub const MAX_TEST_MARKER_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;

/// Conservative fixed response accounting for a complete marker read.
///
/// It covers the versioned envelope, coverage records, field names, JSON
/// punctuation, and response-level receipts. Per-marker accounting below also
/// reserves for display-safe path encoding and worst-case JSON string escaping.
pub const FIXED_TEST_MARKER_OUTPUT_BYTES: u64 = 2_048;
const FIXED_TEST_MARKER_RECORD_OUTPUT_BYTES: u64 = 512;
const PATH_TEXT_EXPANSION: u64 = 2;
const JSON_STRING_ESCAPE_EXPANSION: u64 = 6;
const MAX_PATH_PREFIX_BYTES: usize = 4_096;

/// Returns conservative output accounting for one raw marker record.
#[must_use]
pub fn test_marker_record_output_bytes(path_bytes: u64, raw_target_bytes: u64) -> Option<u64> {
    path_bytes
        .checked_mul(PATH_TEXT_EXPANSION)
        .and_then(|value| value.checked_add(FIXED_TEST_MARKER_RECORD_OUTPUT_BYTES))
        .and_then(|value| {
            raw_target_bytes
                .checked_mul(JSON_STRING_ESCAPE_EXPANSION)
                .and_then(|target| value.checked_add(target))
        })
}

/// Stable invalid test-marker selector classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestMarkersQueryError {
    /// The optional repository-relative path prefix exceeds the fixed ceiling.
    PathPrefixTooLong,
    /// The optional repository-relative path prefix is unsafe or malformed.
    InvalidPathPrefix,
}

impl fmt::Display for TestMarkersQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::PathPrefixTooLong => "test-marker path prefix exceeds the byte limit",
            Self::InvalidPathPrefix => "test-marker path prefix is not repository-relative",
        })
    }
}

impl Error for TestMarkersQueryError {}

/// Optional direct-fact filters for raw test-marker observations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestMarkersQuery {
    language: Option<SourceLanguage>,
    path_prefix: Option<String>,
}

impl TestMarkersQuery {
    /// Admits only an allow-listed language and a safe canonical path prefix.
    pub fn try_new(
        language: Option<SourceLanguage>,
        path_prefix: Option<&str>,
    ) -> Result<Self, TestMarkersQueryError> {
        let path_prefix = path_prefix.map(str::to_owned);
        if let Some(prefix) = &path_prefix {
            if prefix.len() > MAX_PATH_PREFIX_BYTES {
                return Err(TestMarkersQueryError::PathPrefixTooLong);
            }
            if !is_safe_path_prefix(prefix) {
                return Err(TestMarkersQueryError::InvalidPathPrefix);
            }
        }
        Ok(Self {
            language,
            path_prefix,
        })
    }

    /// Returns the optional exact syntax-adapter language restriction.
    #[must_use]
    pub const fn language(&self) -> Option<SourceLanguage> {
        self.language
    }

    /// Returns the optional canonical repository-relative byte prefix.
    #[must_use]
    pub fn path_prefix(&self) -> Option<&str> {
        self.path_prefix.as_deref()
    }
}

/// Stable invalid bounded marker-limit classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TestMarkersLimitError;

impl fmt::Display for TestMarkersLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("test-marker limits are zero or exceed compiled ceilings")
    }
}

impl Error for TestMarkersLimitError {}

/// Independent retained-marker and conservative output bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TestMarkersLimits {
    max_results: u16,
    max_output_bytes: u64,
}

impl TestMarkersLimits {
    /// Validates bounds against the fixed v1 test-marker profile.
    pub const fn try_new(
        max_results: u16,
        max_output_bytes: u64,
    ) -> Result<Self, TestMarkersLimitError> {
        if max_results == 0
            || max_results > MAX_TEST_MARKER_RESULTS
            || max_output_bytes == 0
            || max_output_bytes > MAX_TEST_MARKER_OUTPUT_BYTES
        {
            return Err(TestMarkersLimitError);
        }
        Ok(Self {
            max_results,
            max_output_bytes,
        })
    }

    /// Returns the inclusive retained-marker ceiling.
    #[must_use]
    pub const fn max_results(self) -> u16 {
        self.max_results
    }

    /// Returns the inclusive conservative output-byte ceiling.
    #[must_use]
    pub const fn max_output_bytes(self) -> u64 {
        self.max_output_bytes
    }
}

impl Default for TestMarkersLimits {
    fn default() -> Self {
        Self {
            max_results: DEFAULT_TEST_MARKER_RESULTS,
            max_output_bytes: DEFAULT_TEST_MARKER_OUTPUT_BYTES,
        }
    }
}

/// Whether the required generation-local raw-syntax projection exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestMarkersAvailability {
    /// Every indexed source file produced the immutable raw-syntax projection.
    Complete,
    /// The pinned generation predates the raw-syntax projection.
    NotProduced,
}

/// Exact selected-language support and evidence coverage for test markers.
///
/// `emitted_markers` is an artifact receipt rather than proof that any marker
/// denotes an executable or owned test.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TestMarkerLanguageCoverage {
    language: SourceLanguage,
    indexed_files: u64,
    supported_files: u64,
    unsupported_files: u64,
    emitted_markers: u64,
}

impl TestMarkerLanguageCoverage {
    /// Constructs one storage-adapter coverage receipt for a selected language.
    #[must_use]
    pub const fn new(
        language: SourceLanguage,
        indexed_files: u64,
        supported_files: u64,
        unsupported_files: u64,
        emitted_markers: u64,
    ) -> Self {
        Self {
            language,
            indexed_files,
            supported_files,
            unsupported_files,
            emitted_markers,
        }
    }

    /// Returns the persisted adapter language.
    #[must_use]
    pub const fn language(self) -> SourceLanguage {
        self.language
    }

    /// Returns indexed source files in the selected language/path scope.
    #[must_use]
    pub const fn indexed_files(self) -> u64 {
        self.indexed_files
    }

    /// Returns files whose raw extractor supports test-marker observations.
    #[must_use]
    pub const fn supported_files(self) -> u64 {
        self.supported_files
    }

    /// Returns files for which test-marker observation is explicitly unsupported.
    #[must_use]
    pub const fn unsupported_files(self) -> u64 {
        self.unsupported_files
    }

    /// Returns exact emitted raw marker observations before response truncation.
    #[must_use]
    pub const fn emitted_markers(self) -> u64 {
        self.emitted_markers
    }
}

/// Complete untrusted marker projection returned by a storage adapter.
pub struct TestMarkersPortPayload {
    availability: TestMarkersAvailability,
    language_coverage: Box<[TestMarkerLanguageCoverage]>,
    markers: Box<[OutboundSyntaxSite]>,
    total_markers: u64,
    output_bytes: u64,
}

impl TestMarkersPortPayload {
    /// Constructs an adapter projection that will be fully validated by this use case.
    #[must_use]
    pub const fn new(
        availability: TestMarkersAvailability,
        language_coverage: Box<[TestMarkerLanguageCoverage]>,
        markers: Box<[OutboundSyntaxSite]>,
        total_markers: u64,
        output_bytes: u64,
    ) -> Self {
        Self {
            availability,
            language_coverage,
            markers,
            total_markers,
            output_bytes,
        }
    }
}

/// Complete untrusted adapter answer for a repository-scoped marker read.
pub struct TestMarkersPortResult<G> {
    snapshot: SourceSnapshotDigest,
    generation: G,
    index_coverage: RustIndexCoverage,
    payload: TestMarkersPortPayload,
}

impl<G> TestMarkersPortResult<G> {
    /// Constructs an adapter response that will be fully validated by this use case.
    #[must_use]
    pub const fn new(
        snapshot: SourceSnapshotDigest,
        generation: G,
        index_coverage: RustIndexCoverage,
        payload: TestMarkersPortPayload,
    ) -> Self {
        Self {
            snapshot,
            generation,
            index_coverage,
            payload,
        }
    }
}

/// Validated marker read passed to the storage-neutral port.
pub struct TestMarkersPortRequest {
    repository: RepositoryIdentityDigest,
    query: TestMarkersQuery,
    limits: TestMarkersLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl TestMarkersPortRequest {
    /// Returns the exact repository identity.
    #[must_use]
    pub const fn repository(&self) -> RepositoryIdentityDigest {
        self.repository
    }
    /// Returns direct-fact marker filters.
    #[must_use]
    pub const fn query(&self) -> &TestMarkersQuery {
        &self.query
    }
    /// Returns explicit bounded read limits.
    #[must_use]
    pub const fn limits(&self) -> TestMarkersLimits {
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

/// Narrow, immutable raw test-marker retrieval boundary.
pub trait TestMarkersPort {
    /// Opaque immutable generation identity owned by the adapter.
    type Generation: Copy + Eq;
    /// Stable adapter error mapped at its boundary.
    type Error;

    /// Reads only exact `test_marker` parser observations from one active generation.
    fn test_markers(
        &self,
        request: TestMarkersPortRequest,
    ) -> Result<TestMarkersPortResult<Self::Generation>, Self::Error>;
}

/// Application request shared by local CLI and MCP adapters.
pub struct TestMarkersRequest {
    repository: RepositoryIdentityDigest,
    query: TestMarkersQuery,
    limits: TestMarkersLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl TestMarkersRequest {
    /// Constructs a bounded raw marker request from validated boundary values.
    #[must_use]
    pub const fn new(
        repository: RepositoryIdentityDigest,
        query: TestMarkersQuery,
        limits: TestMarkersLimits,
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

/// Complete validated marker answer pinned to one immutable generation.
#[derive(Eq, PartialEq)]
pub struct TestMarkersResult<G> {
    snapshot: SourceSnapshotDigest,
    generation: G,
    index_coverage: RustIndexCoverage,
    availability: TestMarkersAvailability,
    language_coverage: Box<[TestMarkerLanguageCoverage]>,
    markers: Box<[OutboundSyntaxSite]>,
    total_markers: u64,
    output_bytes: u64,
}

impl<G> TestMarkersResult<G> {
    /// Returns the concrete immutable source snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> SourceSnapshotDigest {
        self.snapshot
    }
    /// Returns the concrete immutable generation.
    #[must_use]
    pub const fn generation(&self) -> &G {
        &self.generation
    }
    /// Returns source-index coverage established before activation.
    #[must_use]
    pub const fn index_coverage(&self) -> RustIndexCoverage {
        self.index_coverage
    }
    /// Returns categorical raw-projection availability.
    #[must_use]
    pub const fn availability(&self) -> TestMarkersAvailability {
        self.availability
    }
    /// Returns exact language-specific marker support and emission receipts.
    #[must_use]
    pub const fn language_coverage(&self) -> &[TestMarkerLanguageCoverage] {
        &self.language_coverage
    }
    /// Returns deterministic raw parser observations only.
    #[must_use]
    pub const fn markers(&self) -> &[OutboundSyntaxSite] {
        &self.markers
    }
    /// Returns the exact marker count before explicit result truncation.
    #[must_use]
    pub const fn total_markers(&self) -> u64 {
        self.total_markers
    }
    /// Returns whether the explicit marker bound omitted observations.
    #[must_use]
    pub fn truncated(&self) -> bool {
        self.total_markers > u64::try_from(self.markers.len()).unwrap_or(u64::MAX)
    }
    /// Returns conservative application output-byte accounting.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
}

impl<G: fmt::Debug> fmt::Debug for TestMarkersResult<G> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TestMarkersResult")
            .field("snapshot", &self.snapshot)
            .field("generation", &self.generation)
            .field("availability", &self.availability)
            .field("returned_markers", &self.markers.len())
            .field("total_markers", &self.total_markers)
            .finish()
    }
}

/// Stable invalid-port-output classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestMarkersPortOutputError {
    /// A returned record was not an exact marker satisfying the filter.
    InvalidMarker,
    /// Records were duplicated or not in canonical source order.
    InvalidOrder,
    /// Counts or bounded output accounting were inconsistent.
    InvalidCoverage,
}

impl fmt::Display for TestMarkersPortOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidMarker => "test-marker adapter returned an invalid marker observation",
            Self::InvalidOrder => {
                "test-marker adapter returned observations out of canonical order"
            }
            Self::InvalidCoverage => "test-marker adapter returned invalid bounded coverage",
        })
    }
}

impl Error for TestMarkersPortOutputError {}

/// Stable application failure for one repository-scoped marker read.
#[derive(Debug)]
pub enum TestMarkersError<E> {
    /// Cancellation was observed before complete output existed.
    Cancelled,
    /// The absolute deadline elapsed before complete output existed.
    DeadlineExceeded,
    /// The storage-neutral adapter failed.
    Port(E),
    /// The adapter violated the immutable raw-marker contract.
    InvalidPortOutput(TestMarkersPortOutputError),
}

impl<E: fmt::Display> fmt::Display for TestMarkersError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("test-marker read cancelled"),
            Self::DeadlineExceeded => formatter.write_str("test-marker read deadline exceeded"),
            Self::Port(error) => write!(formatter, "test-marker adapter failed: {error}"),
            Self::InvalidPortOutput(error) => error.fmt(formatter),
        }
    }
}

impl<E: Error + 'static> Error for TestMarkersError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Port(error) => Some(error),
            Self::InvalidPortOutput(error) => Some(error),
            Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Reads bounded repository-scoped parser-attributed test markers.
pub fn test_markers<Port>(
    port: &Port,
    request: TestMarkersRequest,
) -> Result<TestMarkersResult<Port::Generation>, TestMarkersError<Port::Error>>
where
    Port: TestMarkersPort,
{
    check_control(&request.cancelled, request.deadline)?;
    let result = port
        .test_markers(TestMarkersPortRequest {
            repository: request.repository,
            query: request.query.clone(),
            limits: request.limits,
            cancelled: Arc::clone(&request.cancelled),
            deadline: request.deadline,
        })
        .map_err(TestMarkersError::Port)?;
    check_control(&request.cancelled, request.deadline)?;
    validate_result(&request, &result)?;
    Ok(TestMarkersResult {
        snapshot: result.snapshot,
        generation: result.generation,
        index_coverage: result.index_coverage,
        availability: result.payload.availability,
        language_coverage: result.payload.language_coverage,
        markers: result.payload.markers,
        total_markers: result.payload.total_markers,
        output_bytes: result.payload.output_bytes,
    })
}

fn validate_result<E>(
    request: &TestMarkersRequest,
    result: &TestMarkersPortResult<impl Eq>,
) -> Result<(), TestMarkersError<E>> {
    validate_response_shape(request, &result.payload)?;
    let emitted_markers = validate_language_coverage(request, &result.payload)?;
    if result.payload.availability == TestMarkersAvailability::Complete
        && emitted_markers != result.payload.total_markers
    {
        return Err(invalid_coverage());
    }
    let output_bytes = validate_markers(request, &result.payload)?;
    if output_bytes != result.payload.output_bytes {
        return Err(invalid_coverage());
    }
    Ok(())
}

fn validate_response_shape<E>(
    request: &TestMarkersRequest,
    payload: &TestMarkersPortPayload,
) -> Result<(), TestMarkersError<E>> {
    if payload.markers.len() > usize::from(request.limits.max_results())
        || payload.total_markers < u64::try_from(payload.markers.len()).unwrap_or(u64::MAX)
        || payload.output_bytes > request.limits.max_output_bytes()
        || (payload.availability == TestMarkersAvailability::NotProduced
            && (!payload.language_coverage.is_empty()
                || !payload.markers.is_empty()
                || payload.total_markers != 0
                || payload.output_bytes != 0))
    {
        return Err(invalid_coverage());
    }
    Ok(())
}

fn validate_language_coverage<E>(
    request: &TestMarkersRequest,
    payload: &TestMarkersPortPayload,
) -> Result<u64, TestMarkersError<E>> {
    let mut previous_language = None;
    let mut emitted_markers = 0_u64;
    for coverage in &payload.language_coverage {
        if previous_language.is_some_and(|previous| previous >= coverage.language())
            || request
                .query
                .language()
                .is_some_and(|language| language != coverage.language())
            || coverage.indexed_files()
                != coverage
                    .supported_files()
                    .saturating_add(coverage.unsupported_files())
        {
            return Err(invalid_coverage());
        }
        emitted_markers = emitted_markers
            .checked_add(coverage.emitted_markers())
            .ok_or_else(invalid_coverage)?;
        previous_language = Some(coverage.language());
    }
    Ok(emitted_markers)
}

fn validate_markers<E>(
    request: &TestMarkersRequest,
    payload: &TestMarkersPortPayload,
) -> Result<u64, TestMarkersError<E>> {
    let mut output_bytes = if payload.availability == TestMarkersAvailability::Complete {
        FIXED_TEST_MARKER_OUTPUT_BYTES
    } else {
        0
    };
    let mut previous = None;
    for marker in &payload.markers {
        if marker.site().kind() != RawSyntaxSiteKind::TestMarker
            || !marker.language().matches_repository_path(marker.path())
            || request
                .query
                .language()
                .is_some_and(|language| marker.language() != language)
            || request
                .query
                .path_prefix()
                .is_some_and(|prefix| !marker.path().as_bytes().starts_with(prefix.as_bytes()))
        {
            return Err(TestMarkersError::InvalidPortOutput(
                TestMarkersPortOutputError::InvalidMarker,
            ));
        }
        let site = marker.site();
        let order = (
            marker.path().as_bytes(),
            site.occurrence_span().start().get(),
            site.occurrence_span().end().get(),
            site.target_span().start().get(),
            site.target_span().end().get(),
            site.ordinal().get(),
        );
        if previous.is_some_and(|previous| previous >= order) {
            return Err(TestMarkersError::InvalidPortOutput(
                TestMarkersPortOutputError::InvalidOrder,
            ));
        }
        previous = Some(order);
        output_bytes = output_bytes
            .checked_add(
                test_marker_record_output_bytes(
                    marker.path().byte_count().get(),
                    u64::try_from(site.raw_target().len()).map_err(|_| {
                        TestMarkersError::InvalidPortOutput(
                            TestMarkersPortOutputError::InvalidCoverage,
                        )
                    })?,
                )
                .ok_or_else(invalid_coverage)?,
            )
            .ok_or_else(invalid_coverage)?;
    }
    Ok(output_bytes)
}

fn invalid_coverage<E>() -> TestMarkersError<E> {
    TestMarkersError::InvalidPortOutput(TestMarkersPortOutputError::InvalidCoverage)
}

fn check_control<E>(cancelled: &AtomicBool, deadline: Instant) -> Result<(), TestMarkersError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(TestMarkersError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(TestMarkersError::DeadlineExceeded)
    } else {
        Ok(())
    }
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
