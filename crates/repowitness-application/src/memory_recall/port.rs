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
    MemoryRevalidationTarget, RepositoryIdentityDigest, SourceSnapshotDigest,
};

use super::{
    evidence::MemoryRecallProducer,
    query::{
        MEMORY_RECALL_PROFILE_VERSION, MemoryRecallLimits, MemoryRecallQuery,
        MemoryRecallQueryDigest,
    },
    record::{MemoryRecallProjectionCoverage, MemoryRecallRecord},
};

/// Complete adapter result pinned to one source and memory projection.
pub struct MemoryRecallPortResult<G, P> {
    snapshot: SourceSnapshotDigest,
    generation: G,
    projection: P,
    source_epoch: u64,
    target: MemoryRevalidationTarget,
    producer: MemoryRecallProducer,
    pub(super) projection_coverage: MemoryRecallProjectionCoverage,
    records: Vec<MemoryRecallRecord>,
    total_matches: u64,
    output_bytes: u64,
    scan_bytes: u64,
}

impl<G, P> MemoryRecallPortResult<G, P> {
    /// Constructs adapter output for application validation.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "source, projection, coverage, and independent bounds remain explicit"
    )]
    pub const fn new(
        snapshot: SourceSnapshotDigest,
        generation: G,
        projection: P,
        source_epoch: u64,
        target: MemoryRevalidationTarget,
        producer: MemoryRecallProducer,
        projection_coverage: MemoryRecallProjectionCoverage,
        records: Vec<MemoryRecallRecord>,
        total_matches: u64,
        output_bytes: u64,
        scan_bytes: u64,
    ) -> Self {
        Self {
            snapshot,
            generation,
            projection,
            source_epoch,
            target,
            producer,
            projection_coverage,
            records,
            total_matches,
            output_bytes,
            scan_bytes,
        }
    }
}

/// Narrow immutable-projection retrieval boundary shared by CLI and MCP.
pub trait MemoryRecallPort {
    /// Opaque active index-generation identity owned by the adapter.
    type Generation: Copy + Eq;
    /// Opaque immutable memory-projection identity owned by the adapter.
    type Projection: Copy + Eq;
    /// Stable adapter failure.
    type Error;

    /// Recalls one active projection using a bounded literal query.
    fn recall(
        &self,
        repository: RepositoryIdentityDigest,
        query: &MemoryRecallQuery,
        limits: MemoryRecallLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<MemoryRecallPortResult<Self::Generation, Self::Projection>, Self::Error>;
}

/// Application request shared by local CLI and MCP adapters.
pub struct MemoryRecallRequest {
    repository: RepositoryIdentityDigest,
    query: MemoryRecallQuery,
    limits: MemoryRecallLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl MemoryRecallRequest {
    /// Constructs a request from validated boundary values.
    #[must_use]
    pub const fn new(
        repository: RepositoryIdentityDigest,
        query: MemoryRecallQuery,
        limits: MemoryRecallLimits,
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

impl fmt::Debug for MemoryRecallRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryRecallRequest")
            .field("repository", &self.repository)
            .field("query", &self.query)
            .field("limits", &self.limits)
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Validated recall result with exact freshness, conflicts, evidence, and coverage.
pub struct MemoryRecallResult<G, P> {
    query: Option<MemoryRecallQueryDigest>,
    snapshot: SourceSnapshotDigest,
    generation: G,
    projection: P,
    source_epoch: u64,
    target: MemoryRevalidationTarget,
    producer: MemoryRecallProducer,
    projection_coverage: MemoryRecallProjectionCoverage,
    records: Box<[MemoryRecallRecord]>,
    total_matches: u64,
    omitted_matches: u64,
}

impl<G, P> MemoryRecallResult<G, P> {
    /// Returns the literal query identity, or `None` for all-records mode.
    #[must_use]
    pub const fn query(&self) -> Option<MemoryRecallQueryDigest> {
        self.query
    }

    /// Returns the exact active source snapshot used by the projection.
    #[must_use]
    pub const fn snapshot(&self) -> SourceSnapshotDigest {
        self.snapshot
    }

    /// Returns the exact active index generation used by the projection.
    #[must_use]
    pub const fn generation(&self) -> &G {
        &self.generation
    }

    /// Returns the immutable projection identity.
    #[must_use]
    pub const fn projection(&self) -> &P {
        &self.projection
    }

    /// Returns the exact workspace source epoch.
    #[must_use]
    pub const fn source_epoch(&self) -> u64 {
        self.source_epoch
    }

    /// Returns the concrete Git/worktree revalidation target.
    #[must_use]
    pub const fn target(&self) -> MemoryRevalidationTarget {
        self.target
    }

    /// Returns correspondence producer attribution.
    #[must_use]
    pub const fn producer(&self) -> &MemoryRecallProducer {
        &self.producer
    }

    /// Returns complete projection-level coverage and state counts.
    #[must_use]
    pub const fn projection_coverage(&self) -> MemoryRecallProjectionCoverage {
        self.projection_coverage
    }

    /// Returns records in deterministic record-ID order.
    #[must_use]
    pub const fn records(&self) -> &[MemoryRecallRecord] {
        &self.records
    }

    /// Returns matching projection rows before the result bound.
    #[must_use]
    pub const fn total_matches(&self) -> u64 {
        self.total_matches
    }

    /// Returns matching rows omitted by the result bound.
    #[must_use]
    pub const fn omitted_matches(&self) -> u64 {
        self.omitted_matches
    }

    /// Returns the recall profile version.
    #[must_use]
    pub const fn profile_version(&self) -> u16 {
        MEMORY_RECALL_PROFILE_VERSION
    }
}

impl<G: fmt::Debug, P: fmt::Debug> fmt::Debug for MemoryRecallResult<G, P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryRecallResult")
            .field("query", &self.query)
            .field("snapshot", &self.snapshot)
            .field("generation", &self.generation)
            .field("projection", &self.projection)
            .field("source_epoch", &self.source_epoch)
            .field("target", &self.target)
            .field("producer", &self.producer)
            .field("projection_coverage", &self.projection_coverage)
            .field("returned_records", &self.records.len())
            .field("total_matches", &self.total_matches)
            .field("omitted_matches", &self.omitted_matches)
            .finish()
    }
}

/// Stable invalid-adapter-output classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRecallPortOutputError {
    /// The adapter returned more records than requested.
    ResultLimitExceeded,
    /// Total matches are smaller than returned matches or exceed projection rows.
    InvalidTotalMatches,
    /// The adapter exceeded the conservative encoded-output limit.
    OutputByteLimitExceeded,
    /// The adapter exceeded the canonical-record scan limit.
    ScanByteLimitExceeded,
    /// Projection coverage or state counts are inconsistent.
    InvalidCoverage,
    /// Correspondence producer attribution is invalid.
    InvalidProducer,
    /// A projected record is inconsistent.
    InvalidRecord,
    /// A projected evidence result is inconsistent.
    InvalidEvidence,
    /// Result ordering contains duplicate or descending record identities.
    InvalidOrdering,
    /// A count cannot be represented safely.
    CountNotRepresentable,
}

impl fmt::Display for MemoryRecallPortOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ResultLimitExceeded => {
                "memory-recall adapter returned more records than requested"
            }
            Self::InvalidTotalMatches => "memory-recall adapter returned invalid match counts",
            Self::OutputByteLimitExceeded => {
                "memory-recall adapter exceeded the requested output byte limit"
            }
            Self::ScanByteLimitExceeded => {
                "memory-recall adapter exceeded the requested canonical scan byte limit"
            }
            Self::InvalidCoverage => "memory-recall adapter returned invalid projection coverage",
            Self::InvalidProducer => "memory-recall adapter returned invalid producer attribution",
            Self::InvalidRecord => "memory-recall adapter returned an invalid projected record",
            Self::InvalidEvidence => "memory-recall adapter returned invalid projected evidence",
            Self::InvalidOrdering => "memory-recall adapter returned invalid record ordering",
            Self::CountNotRepresentable => "memory-recall result count is not representable safely",
        })
    }
}

impl Error for MemoryRecallPortOutputError {}

/// Application failure for one memory recall.
#[derive(Debug)]
pub enum MemoryRecallError<E> {
    /// The operation was cancelled before a complete result.
    Cancelled,
    /// The operation deadline elapsed.
    DeadlineExceeded,
    /// The storage-neutral adapter failed.
    Port(E),
    /// The adapter violated the shared result contract.
    InvalidPortOutput(MemoryRecallPortOutputError),
}

impl<E: fmt::Display> fmt::Display for MemoryRecallError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("memory recall cancelled"),
            Self::DeadlineExceeded => formatter.write_str("memory recall deadline exceeded"),
            Self::Port(error) => write!(formatter, "memory recall adapter failed: {error}"),
            Self::InvalidPortOutput(error) => write!(formatter, "{error}"),
        }
    }
}

impl<E: Error + 'static> Error for MemoryRecallError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Port(error) => Some(error),
            Self::InvalidPortOutput(error) => Some(error),
            Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Result type returned by the shared memory-recall use case.
pub type MemoryRecallUseCaseResult<G, P, E> =
    Result<MemoryRecallResult<G, P>, MemoryRecallError<E>>;

/// Retrieves one exact active projection and validates every exposed boundary.
pub fn memory_recall<Port>(
    port: &Port,
    request: MemoryRecallRequest,
) -> MemoryRecallUseCaseResult<Port::Generation, Port::Projection, Port::Error>
where
    Port: MemoryRecallPort,
{
    check_control(&request.cancelled, request.deadline)?;
    let query_digest = request.query.digest();
    let result = port
        .recall(
            request.repository,
            &request.query,
            request.limits,
            Arc::clone(&request.cancelled),
            request.deadline,
        )
        .map_err(MemoryRecallError::Port)?;
    check_control(&request.cancelled, request.deadline)?;
    validate_port_result(request.repository, &result, request.limits)?;
    let returned = u64::try_from(result.records.len()).map_err(|_| {
        MemoryRecallError::InvalidPortOutput(MemoryRecallPortOutputError::CountNotRepresentable)
    })?;
    let omitted_matches =
        result
            .total_matches
            .checked_sub(returned)
            .ok_or(MemoryRecallError::InvalidPortOutput(
                MemoryRecallPortOutputError::InvalidTotalMatches,
            ))?;
    Ok(MemoryRecallResult {
        query: query_digest,
        snapshot: result.snapshot,
        generation: result.generation,
        projection: result.projection,
        source_epoch: result.source_epoch,
        target: result.target,
        producer: result.producer,
        projection_coverage: result.projection_coverage,
        records: result.records.into_boxed_slice(),
        total_matches: result.total_matches,
        omitted_matches,
    })
}

fn validate_port_result<G, P, E>(
    repository: RepositoryIdentityDigest,
    result: &MemoryRecallPortResult<G, P>,
    limits: MemoryRecallLimits,
) -> Result<(), MemoryRecallError<E>> {
    let returned = u64::try_from(result.records.len()).map_err(|_| {
        MemoryRecallError::InvalidPortOutput(MemoryRecallPortOutputError::CountNotRepresentable)
    })?;
    if returned > u64::from(limits.max_results()) {
        return Err(MemoryRecallError::InvalidPortOutput(
            MemoryRecallPortOutputError::ResultLimitExceeded,
        ));
    }
    if result.total_matches < returned || result.total_matches > result.projection_coverage.total()
    {
        return Err(MemoryRecallError::InvalidPortOutput(
            MemoryRecallPortOutputError::InvalidTotalMatches,
        ));
    }
    if !result.projection_coverage.valid() {
        return Err(MemoryRecallError::InvalidPortOutput(
            MemoryRecallPortOutputError::InvalidCoverage,
        ));
    }
    if result.output_bytes > limits.max_output_bytes() {
        return Err(MemoryRecallError::InvalidPortOutput(
            MemoryRecallPortOutputError::OutputByteLimitExceeded,
        ));
    }
    if result.scan_bytes > limits.max_scan_bytes() {
        return Err(MemoryRecallError::InvalidPortOutput(
            MemoryRecallPortOutputError::ScanByteLimitExceeded,
        ));
    }
    if result
        .records
        .windows(2)
        .any(|pair| pair[0].record_id >= pair[1].record_id)
    {
        return Err(MemoryRecallError::InvalidPortOutput(
            MemoryRecallPortOutputError::InvalidOrdering,
        ));
    }

    let mut computed_output = 0_u64;
    for record in &result.records {
        if record
            .record
            .as_ref()
            .is_some_and(|selected| selected.scope().repository() != repository)
        {
            return Err(MemoryRecallError::InvalidPortOutput(
                MemoryRecallPortOutputError::InvalidRecord,
            ));
        }
        computed_output = computed_output
            .checked_add(
                record
                    .encoded_output_bytes()
                    .map_err(MemoryRecallError::InvalidPortOutput)?,
            )
            .ok_or(MemoryRecallError::InvalidPortOutput(
                MemoryRecallPortOutputError::CountNotRepresentable,
            ))?;
    }
    if computed_output != result.output_bytes || computed_output > limits.max_output_bytes() {
        return Err(MemoryRecallError::InvalidPortOutput(
            MemoryRecallPortOutputError::OutputByteLimitExceeded,
        ));
    }
    Ok(())
}

fn check_control<E>(cancelled: &AtomicBool, deadline: Instant) -> Result<(), MemoryRecallError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(MemoryRecallError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(MemoryRecallError::DeadlineExceeded)
    } else {
        Ok(())
    }
}
