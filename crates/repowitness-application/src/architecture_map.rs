//! Bounded, generation-pinned source architecture inventory.
//!
//! This capability deliberately inventories exact indexed source files across
//! every Phase 0 language. It does not infer imports, calls, ownership, or
//! cross-language relationships; the Rust graph remains the only relationship
//! capability.

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
    AnalysisArtifactDigest, ProducerManifestDigest, RepositoryIdentityDigest, RepositoryPath,
    SourceContentDigest, SourceSnapshotDigest,
};

use crate::{RustIndexCoverage, SourceLanguage};

/// Version of the bounded multi-language source-inventory profile.
pub const ARCHITECTURE_MAP_PROFILE_VERSION: u16 = 1;
/// Default maximum number of exact source-file entries retained in one map.
pub const DEFAULT_ARCHITECTURE_MAP_FILES: u16 = 200;
/// Default conservative encoded-output ceiling for one map.
pub const DEFAULT_ARCHITECTURE_MAP_OUTPUT_BYTES: u64 = 512 * 1024;
/// Hard maximum number of exact source-file entries retained in one map.
pub const MAX_ARCHITECTURE_MAP_FILES: u16 = 1_000;
/// Hard conservative encoded-output ceiling for one map.
pub const MAX_ARCHITECTURE_MAP_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;

/// Stable failure to construct bounded architecture-map limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchitectureMapLimitError;

impl fmt::Display for ArchitectureMapLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("architecture-map limits are zero or exceed compiled ceilings")
    }
}

impl Error for ArchitectureMapLimitError {}

/// Inclusive file-count and output-size bounds for one source map.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchitectureMapLimits {
    max_files: u16,
    max_output_bytes: u64,
}

impl ArchitectureMapLimits {
    /// Validates architecture-map bounds against the fixed public profile.
    pub const fn try_new(
        max_files: u16,
        max_output_bytes: u64,
    ) -> Result<Self, ArchitectureMapLimitError> {
        if max_files == 0
            || max_files > MAX_ARCHITECTURE_MAP_FILES
            || max_output_bytes == 0
            || max_output_bytes > MAX_ARCHITECTURE_MAP_OUTPUT_BYTES
        {
            return Err(ArchitectureMapLimitError);
        }
        Ok(Self {
            max_files,
            max_output_bytes,
        })
    }

    /// Returns the inclusive retained-file ceiling.
    #[must_use]
    pub const fn max_files(self) -> u16 {
        self.max_files
    }

    /// Returns the inclusive conservative encoded-output ceiling.
    #[must_use]
    pub const fn max_output_bytes(self) -> u64 {
        self.max_output_bytes
    }
}

impl Default for ArchitectureMapLimits {
    fn default() -> Self {
        Self {
            max_files: DEFAULT_ARCHITECTURE_MAP_FILES,
            max_output_bytes: DEFAULT_ARCHITECTURE_MAP_OUTPUT_BYTES,
        }
    }
}

/// Exact persisted receipt for one indexed source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchitectureMapFile {
    path: RepositoryPath,
    language: SourceLanguage,
    content_digest: SourceContentDigest,
    artifact_digest: AnalysisArtifactDigest,
    producer_manifest: ProducerManifestDigest,
    declaration_count: u64,
}

impl ArchitectureMapFile {
    /// Constructs one untrusted storage-adapter file receipt for validation.
    #[must_use]
    pub const fn new(
        path: RepositoryPath,
        language: SourceLanguage,
        content_digest: SourceContentDigest,
        artifact_digest: AnalysisArtifactDigest,
        producer_manifest: ProducerManifestDigest,
        declaration_count: u64,
    ) -> Self {
        Self {
            path,
            language,
            content_digest,
            artifact_digest,
            producer_manifest,
            declaration_count,
        }
    }

    /// Returns the exact canonical repository-relative path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the source adapter language associated with the exact path.
    #[must_use]
    pub const fn language(&self) -> SourceLanguage {
        self.language
    }

    /// Returns the exact source-content identity.
    #[must_use]
    pub const fn content_digest(&self) -> SourceContentDigest {
        self.content_digest
    }

    /// Returns the exact analysis-artifact identity.
    #[must_use]
    pub const fn artifact_digest(&self) -> AnalysisArtifactDigest {
        self.artifact_digest
    }

    /// Returns the exact parser/adapter producer manifest identity.
    #[must_use]
    pub const fn producer_manifest(&self) -> ProducerManifestDigest {
        self.producer_manifest
    }

    /// Returns the exact persisted declaration count for this artifact.
    #[must_use]
    pub const fn declaration_count(&self) -> u64 {
        self.declaration_count
    }
}

/// Complete source and declaration totals for one persisted language.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchitectureMapLanguageSummary {
    language: SourceLanguage,
    file_count: u64,
    declaration_count: u64,
}

impl ArchitectureMapLanguageSummary {
    /// Constructs one untrusted adapter summary for validation.
    #[must_use]
    pub const fn new(language: SourceLanguage, file_count: u64, declaration_count: u64) -> Self {
        Self {
            language,
            file_count,
            declaration_count,
        }
    }

    /// Returns the persisted source language.
    #[must_use]
    pub const fn language(&self) -> SourceLanguage {
        self.language
    }

    /// Returns the complete indexed-file count before entry truncation.
    #[must_use]
    pub const fn file_count(&self) -> u64 {
        self.file_count
    }

    /// Returns the complete declaration count before entry truncation.
    #[must_use]
    pub const fn declaration_count(&self) -> u64 {
        self.declaration_count
    }
}

/// Complete storage-adapter response pinned to one immutable active generation.
pub struct ArchitectureMapPortResult<G> {
    snapshot: SourceSnapshotDigest,
    generation: G,
    index_coverage: RustIndexCoverage,
    files: Vec<ArchitectureMapFile>,
    language_summaries: Vec<ArchitectureMapLanguageSummary>,
    total_files: u64,
    total_declarations: u64,
    output_bytes: u64,
}

impl<G> ArchitectureMapPortResult<G> {
    /// Constructs an untrusted adapter response for application validation.
    #[allow(
        clippy::too_many_arguments,
        reason = "every returned receipt is independently validated"
    )]
    #[must_use]
    pub const fn new(
        snapshot: SourceSnapshotDigest,
        generation: G,
        index_coverage: RustIndexCoverage,
        files: Vec<ArchitectureMapFile>,
        language_summaries: Vec<ArchitectureMapLanguageSummary>,
        total_files: u64,
        total_declarations: u64,
        output_bytes: u64,
    ) -> Self {
        Self {
            snapshot,
            generation,
            index_coverage,
            files,
            language_summaries,
            total_files,
            total_declarations,
            output_bytes,
        }
    }
}

/// Narrow read-only retrieval boundary shared by CLI and MCP composition.
pub trait ArchitectureMapPort {
    /// Immutable local generation identity.
    type Generation;
    /// Stable adapter error.
    type Error;

    /// Reads the active generation's exact bounded source inventory.
    fn architecture_map(
        &self,
        repository: RepositoryIdentityDigest,
        limits: ArchitectureMapLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ArchitectureMapPortResult<Self::Generation>, Self::Error>;
}

/// Application request for one active-generation architecture map.
pub struct ArchitectureMapRequest {
    repository: RepositoryIdentityDigest,
    limits: ArchitectureMapLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl ArchitectureMapRequest {
    /// Creates a bounded request from trusted adapter inputs.
    #[must_use]
    pub const fn new(
        repository: RepositoryIdentityDigest,
        limits: ArchitectureMapLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            repository,
            limits,
            cancelled,
            deadline,
        }
    }
}

impl fmt::Debug for ArchitectureMapRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ArchitectureMapRequest")
            .field("repository", &"<redacted-identity>")
            .field("limits", &self.limits)
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Validated architecture map with explicit source receipts and limitations.
#[derive(Debug, Eq, PartialEq)]
pub struct ArchitectureMapResult<G> {
    snapshot: SourceSnapshotDigest,
    generation: G,
    index_coverage: RustIndexCoverage,
    files: Box<[ArchitectureMapFile]>,
    language_summaries: Box<[ArchitectureMapLanguageSummary]>,
    total_files: u64,
    total_declarations: u64,
    output_bytes: u64,
}

impl<G> ArchitectureMapResult<G> {
    /// Returns the exact active source snapshot that was inventoried.
    #[must_use]
    pub const fn snapshot(&self) -> SourceSnapshotDigest {
        self.snapshot
    }

    /// Returns the exact immutable active generation that was inventoried.
    #[must_use]
    pub const fn generation(&self) -> &G {
        &self.generation
    }

    /// Returns index coverage established before generation activation.
    #[must_use]
    pub const fn index_coverage(&self) -> RustIndexCoverage {
        self.index_coverage
    }

    /// Returns exact file receipts in canonical byte-path order.
    #[must_use]
    pub const fn files(&self) -> &[ArchitectureMapFile] {
        &self.files
    }

    /// Returns complete per-language totals in stable language order.
    #[must_use]
    pub const fn language_summaries(&self) -> &[ArchitectureMapLanguageSummary] {
        &self.language_summaries
    }

    /// Returns the complete indexed-file count before entry truncation.
    #[must_use]
    pub const fn total_files(&self) -> u64 {
        self.total_files
    }

    /// Returns the complete persisted declaration count before entry truncation.
    #[must_use]
    pub const fn total_declarations(&self) -> u64 {
        self.total_declarations
    }

    /// Returns the bounded conservative encoded-output byte count.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }

    /// Returns whether exact file entries were bounded before every indexed file was returned.
    #[must_use]
    pub fn truncated(&self) -> bool {
        u64::try_from(self.files.len()).ok() < Some(self.total_files)
    }
}

/// Stable invalid-storage-output classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchitectureMapPortOutputError {
    /// The adapter returned more file receipts than requested.
    FileLimitExceeded,
    /// Complete total counts are smaller than the returned data or inconsistent.
    InvalidTotals,
    /// The adapter exceeded the requested output ceiling.
    OutputByteLimitExceeded,
    /// A file receipt's language does not match its exact repository path.
    InvalidFile,
    /// File receipts were duplicated or not in canonical byte-path order.
    InvalidFileOrder,
    /// Language summaries were duplicated, unsorted, or did not add up.
    InvalidLanguageSummaries,
    /// A fixed-width count could not be represented safely.
    CountNotRepresentable,
}

impl fmt::Display for ArchitectureMapPortOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::FileLimitExceeded => "architecture-map adapter exceeded the file limit",
            Self::InvalidTotals => "architecture-map adapter returned inconsistent totals",
            Self::OutputByteLimitExceeded => "architecture-map adapter exceeded the output limit",
            Self::InvalidFile => "architecture-map adapter returned an invalid file receipt",
            Self::InvalidFileOrder => "architecture-map adapter returned invalid file ordering",
            Self::InvalidLanguageSummaries => {
                "architecture-map adapter returned invalid language summaries"
            }
            Self::CountNotRepresentable => "architecture-map counts cannot be represented safely",
        })
    }
}

impl Error for ArchitectureMapPortOutputError {}

/// Application failure for one all-or-nothing architecture-map read.
#[derive(Debug)]
pub enum ArchitectureMapError<E> {
    /// Cancellation was observed before a complete result existed.
    Cancelled,
    /// The monotonic deadline elapsed before a complete result existed.
    DeadlineExceeded,
    /// The storage-neutral adapter failed.
    Port(E),
    /// The storage-neutral adapter violated the public receipt contract.
    InvalidPortOutput(ArchitectureMapPortOutputError),
}

impl<E: fmt::Display> fmt::Display for ArchitectureMapError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("architecture-map read cancelled"),
            Self::DeadlineExceeded => {
                formatter.write_str("architecture-map read deadline exceeded")
            }
            Self::Port(error) => write!(formatter, "architecture-map adapter failed: {error}"),
            Self::InvalidPortOutput(error) => error.fmt(formatter),
        }
    }
}

impl<E: Error + 'static> Error for ArchitectureMapError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Port(error) => Some(error),
            Self::InvalidPortOutput(error) => Some(error),
            Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Reads a bounded, exact multi-language source inventory from one active generation.
pub fn architecture_map<Port>(
    port: &Port,
    request: ArchitectureMapRequest,
) -> Result<ArchitectureMapResult<Port::Generation>, ArchitectureMapError<Port::Error>>
where
    Port: ArchitectureMapPort,
{
    check_control(&request.cancelled, request.deadline)?;
    let limits = request.limits;
    let result = port
        .architecture_map(
            request.repository,
            limits,
            Arc::clone(&request.cancelled),
            request.deadline,
        )
        .map_err(ArchitectureMapError::Port)?;
    check_control(&request.cancelled, request.deadline)?;
    validate_port_result(&result, limits)?;
    Ok(ArchitectureMapResult {
        snapshot: result.snapshot,
        generation: result.generation,
        index_coverage: result.index_coverage,
        files: result.files.into_boxed_slice(),
        language_summaries: result.language_summaries.into_boxed_slice(),
        total_files: result.total_files,
        total_declarations: result.total_declarations,
        output_bytes: result.output_bytes,
    })
}

fn validate_port_result<G, E>(
    result: &ArchitectureMapPortResult<G>,
    limits: ArchitectureMapLimits,
) -> Result<(), ArchitectureMapError<E>> {
    let returned = u64::try_from(result.files.len()).map_err(|_| {
        ArchitectureMapError::InvalidPortOutput(
            ArchitectureMapPortOutputError::CountNotRepresentable,
        )
    })?;
    if returned > u64::from(limits.max_files()) {
        return Err(ArchitectureMapError::InvalidPortOutput(
            ArchitectureMapPortOutputError::FileLimitExceeded,
        ));
    }
    if returned > result.total_files {
        return Err(ArchitectureMapError::InvalidPortOutput(
            ArchitectureMapPortOutputError::InvalidTotals,
        ));
    }
    if result.output_bytes > limits.max_output_bytes() {
        return Err(ArchitectureMapError::InvalidPortOutput(
            ArchitectureMapPortOutputError::OutputByteLimitExceeded,
        ));
    }
    if result
        .files
        .iter()
        .any(|file| !file.language.matches_repository_path(&file.path))
    {
        return Err(ArchitectureMapError::InvalidPortOutput(
            ArchitectureMapPortOutputError::InvalidFile,
        ));
    }
    if result
        .files
        .windows(2)
        .any(|pair| pair[0].path >= pair[1].path)
    {
        return Err(ArchitectureMapError::InvalidPortOutput(
            ArchitectureMapPortOutputError::InvalidFileOrder,
        ));
    }

    let mut files = 0_u64;
    let mut declarations = 0_u64;
    let mut previous_language = None;
    for summary in &result.language_summaries {
        if previous_language
            .is_some_and(|previous: SourceLanguage| previous.as_str() >= summary.language.as_str())
        {
            return Err(ArchitectureMapError::InvalidPortOutput(
                ArchitectureMapPortOutputError::InvalidLanguageSummaries,
            ));
        }
        previous_language = Some(summary.language);
        files = files.checked_add(summary.file_count).ok_or(
            ArchitectureMapError::InvalidPortOutput(
                ArchitectureMapPortOutputError::CountNotRepresentable,
            ),
        )?;
        declarations = declarations.checked_add(summary.declaration_count).ok_or(
            ArchitectureMapError::InvalidPortOutput(
                ArchitectureMapPortOutputError::CountNotRepresentable,
            ),
        )?;
    }
    if files != result.total_files || declarations != result.total_declarations {
        return Err(ArchitectureMapError::InvalidPortOutput(
            ArchitectureMapPortOutputError::InvalidLanguageSummaries,
        ));
    }
    Ok(())
}

fn check_control<E>(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), ArchitectureMapError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(ArchitectureMapError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(ArchitectureMapError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
