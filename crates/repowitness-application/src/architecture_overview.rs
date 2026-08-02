//! Bounded source-only orientation over one immutable active generation.
//!
//! This capability reports only persisted source facts and exact receipts. It
//! deliberately does not infer packages, ownership, runtime entry points,
//! imports, calls, tests, hotspots, or cross-language relationships.

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
    ProducerManifestDigest, RepositoryIdentityDigest, RepositoryPath, SourceContentDigest,
    SourceSnapshotDigest,
};

use crate::{
    ArchitectureMapFile, ArchitectureMapLanguageSummary, RustIndexCoverage, RustSymbolOccurrence,
    SourceLanguage,
};

/// Version of the bounded source-only architecture-overview profile.
pub const ARCHITECTURE_OVERVIEW_PROFILE_VERSION: u16 = 1;
/// Default maximum exact source-root summaries retained in one result.
pub const DEFAULT_ARCHITECTURE_OVERVIEW_ROOTS: u16 = 100;
/// Hard maximum exact source-root summaries retained in one result.
pub const MAX_ARCHITECTURE_OVERVIEW_ROOTS: u16 = 500;
/// Default maximum exact function-named-`main` candidates retained in one result.
pub const DEFAULT_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES: u16 = 100;
/// Hard maximum exact function-named-`main` candidates retained in one result.
pub const MAX_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES: u16 = 500;
/// Default maximum exact per-file declaration receipts retained in one result.
pub const DEFAULT_ARCHITECTURE_OVERVIEW_FILES: u16 = 200;
/// Hard maximum exact per-file declaration receipts retained in one result.
pub const MAX_ARCHITECTURE_OVERVIEW_FILES: u16 = 1_000;
/// Default conservative encoded-output ceiling for one overview.
pub const DEFAULT_ARCHITECTURE_OVERVIEW_OUTPUT_BYTES: u64 = 512 * 1024;
/// Hard conservative encoded-output ceiling for one overview.
pub const MAX_ARCHITECTURE_OVERVIEW_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;

/// Stable failure to construct architecture-overview limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchitectureOverviewLimitError;

impl fmt::Display for ArchitectureOverviewLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("architecture-overview limits are zero or exceed compiled ceilings")
    }
}

impl Error for ArchitectureOverviewLimitError {}

/// Independent inclusive limits for every retained overview receipt family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchitectureOverviewLimits {
    max_roots: u16,
    max_entry_point_candidates: u16,
    max_files: u16,
    max_output_bytes: u64,
}

impl ArchitectureOverviewLimits {
    /// Validates overview bounds against the fixed public profile.
    pub const fn try_new(
        max_roots: u16,
        max_entry_point_candidates: u16,
        max_files: u16,
        max_output_bytes: u64,
    ) -> Result<Self, ArchitectureOverviewLimitError> {
        if max_roots == 0
            || max_roots > MAX_ARCHITECTURE_OVERVIEW_ROOTS
            || max_entry_point_candidates == 0
            || max_entry_point_candidates > MAX_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES
            || max_files == 0
            || max_files > MAX_ARCHITECTURE_OVERVIEW_FILES
            || max_output_bytes == 0
            || max_output_bytes > MAX_ARCHITECTURE_OVERVIEW_OUTPUT_BYTES
        {
            return Err(ArchitectureOverviewLimitError);
        }
        Ok(Self {
            max_roots,
            max_entry_point_candidates,
            max_files,
            max_output_bytes,
        })
    }

    /// Returns the inclusive source-root receipt ceiling.
    #[must_use]
    pub const fn max_roots(self) -> u16 {
        self.max_roots
    }

    /// Returns the inclusive function-named-`main` candidate ceiling.
    #[must_use]
    pub const fn max_entry_point_candidates(self) -> u16 {
        self.max_entry_point_candidates
    }

    /// Returns the inclusive per-file receipt ceiling.
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

impl Default for ArchitectureOverviewLimits {
    fn default() -> Self {
        Self {
            max_roots: DEFAULT_ARCHITECTURE_OVERVIEW_ROOTS,
            max_entry_point_candidates: DEFAULT_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES,
            max_files: DEFAULT_ARCHITECTURE_OVERVIEW_FILES,
            max_output_bytes: DEFAULT_ARCHITECTURE_OVERVIEW_OUTPUT_BYTES,
        }
    }
}

/// Exact structural bucket for paths indexed in one source generation.
///
/// A top-level directory is merely the first canonical repository-path
/// component. It is not a package, module, ownership, or dependency boundary.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ArchitectureOverviewSourceRoot {
    /// A source file directly below the repository root.
    RepositoryRoot,
    /// Every source file whose first canonical path component is this directory.
    TopLevelDirectory(RepositoryPath),
}

impl ArchitectureOverviewSourceRoot {
    /// Returns a structural bucket for direct repository-root files.
    #[must_use]
    pub const fn repository_root() -> Self {
        Self::RepositoryRoot
    }

    /// Returns a structural bucket for one exact first path component.
    #[must_use]
    pub const fn top_level_directory(component: RepositoryPath) -> Self {
        Self::TopLevelDirectory(component)
    }
}

/// Complete declaration and file totals for one exact structural source root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchitectureOverviewSourceRootSummary {
    root: ArchitectureOverviewSourceRoot,
    file_count: u64,
    declaration_count: u64,
}

impl ArchitectureOverviewSourceRootSummary {
    /// Constructs one untrusted storage-adapter structural aggregate.
    #[must_use]
    pub const fn new(
        root: ArchitectureOverviewSourceRoot,
        file_count: u64,
        declaration_count: u64,
    ) -> Self {
        Self {
            root,
            file_count,
            declaration_count,
        }
    }

    /// Returns the exact source-root bucket.
    #[must_use]
    pub const fn root(&self) -> &ArchitectureOverviewSourceRoot {
        &self.root
    }

    /// Returns the complete indexed-file count under this bucket.
    #[must_use]
    pub const fn file_count(&self) -> u64 {
        self.file_count
    }

    /// Returns the complete direct-declaration count under this bucket.
    #[must_use]
    pub const fn declaration_count(&self) -> u64 {
        self.declaration_count
    }
}

/// Complete direct-syntax declaration total for one language and kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchitectureOverviewKindSummary {
    language: SourceLanguage,
    kind: RustSymbolKind,
    declaration_count: u64,
}

impl ArchitectureOverviewKindSummary {
    /// Constructs one untrusted storage-adapter kind aggregate for validation.
    #[must_use]
    pub const fn new(
        language: SourceLanguage,
        kind: RustSymbolKind,
        declaration_count: u64,
    ) -> Self {
        Self {
            language,
            kind,
            declaration_count,
        }
    }

    /// Returns the persisted syntax-adapter language.
    #[must_use]
    pub const fn language(self) -> SourceLanguage {
        self.language
    }

    /// Returns the persisted direct-syntax declaration kind.
    #[must_use]
    pub const fn kind(self) -> RustSymbolKind {
        self.kind
    }

    /// Returns the complete persisted declaration total for this language/kind pair.
    #[must_use]
    pub const fn declaration_count(self) -> u64 {
        self.declaration_count
    }
}

/// Exact direct-syntax declaration matching the v1 entry-point candidate rule.
///
/// The sole v1 rule is `kind=function` and unqualified `name=main`. This is a
/// navigation candidate, not proof that a runtime invokes it.
#[derive(Clone, Eq, PartialEq)]
pub struct ArchitectureOverviewEntryPointCandidate {
    path: RepositoryPath,
    content_digest: SourceContentDigest,
    occurrence: RustSymbolOccurrence,
}

impl ArchitectureOverviewEntryPointCandidate {
    /// Constructs one untrusted adapter candidate for application validation.
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

    /// Returns the exact canonical source path containing this declaration.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the exact source-content identity.
    #[must_use]
    pub const fn content_digest(&self) -> SourceContentDigest {
        self.content_digest
    }

    /// Returns the complete direct-syntax declaration receipt.
    #[must_use]
    pub const fn occurrence(&self) -> &RustSymbolOccurrence {
        &self.occurrence
    }
}

impl fmt::Debug for ArchitectureOverviewEntryPointCandidate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ArchitectureOverviewEntryPointCandidate")
            .field("path", &self.path)
            .field("content_digest", &self.content_digest)
            .field("occurrence", &self.occurrence)
            .finish()
    }
}

/// Complete storage-adapter response pinned to one active generation.
pub struct ArchitectureOverviewPortResult<G> {
    snapshot: SourceSnapshotDigest,
    generation: G,
    source_producer_manifest: ProducerManifestDigest,
    index_coverage: RustIndexCoverage,
    language_summaries: Vec<ArchitectureMapLanguageSummary>,
    kind_summaries: Vec<ArchitectureOverviewKindSummary>,
    source_roots: Vec<ArchitectureOverviewSourceRootSummary>,
    entry_point_candidates: Vec<ArchitectureOverviewEntryPointCandidate>,
    files: Vec<ArchitectureMapFile>,
    total_files: u64,
    total_declarations: u64,
    total_source_roots: u64,
    total_entry_point_candidates: u64,
    output_bytes: u64,
}

impl<G> ArchitectureOverviewPortResult<G> {
    /// Constructs an untrusted adapter result for application validation.
    #[allow(
        clippy::too_many_arguments,
        reason = "every aggregate, receipt family, and pin is independently validated"
    )]
    #[must_use]
    pub const fn new(
        snapshot: SourceSnapshotDigest,
        generation: G,
        source_producer_manifest: ProducerManifestDigest,
        index_coverage: RustIndexCoverage,
        language_summaries: Vec<ArchitectureMapLanguageSummary>,
        kind_summaries: Vec<ArchitectureOverviewKindSummary>,
        source_roots: Vec<ArchitectureOverviewSourceRootSummary>,
        entry_point_candidates: Vec<ArchitectureOverviewEntryPointCandidate>,
        files: Vec<ArchitectureMapFile>,
        total_files: u64,
        total_declarations: u64,
        total_source_roots: u64,
        total_entry_point_candidates: u64,
        output_bytes: u64,
    ) -> Self {
        Self {
            snapshot,
            generation,
            source_producer_manifest,
            index_coverage,
            language_summaries,
            kind_summaries,
            source_roots,
            entry_point_candidates,
            files,
            total_files,
            total_declarations,
            total_source_roots,
            total_entry_point_candidates,
            output_bytes,
        }
    }
}

/// Narrow source-only architecture-overview retrieval boundary.
pub trait ArchitectureOverviewPort {
    /// Immutable local generation identity.
    type Generation;
    /// Stable adapter error.
    type Error;

    /// Reads one bounded aggregate from the active generation.
    fn architecture_overview(
        &self,
        repository: RepositoryIdentityDigest,
        limits: ArchitectureOverviewLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ArchitectureOverviewPortResult<Self::Generation>, Self::Error>;
}

/// Application request for one active-generation architecture overview.
pub struct ArchitectureOverviewRequest {
    repository: RepositoryIdentityDigest,
    limits: ArchitectureOverviewLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl ArchitectureOverviewRequest {
    /// Creates a bounded request from trusted adapter inputs.
    #[must_use]
    pub const fn new(
        repository: RepositoryIdentityDigest,
        limits: ArchitectureOverviewLimits,
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

impl fmt::Debug for ArchitectureOverviewRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ArchitectureOverviewRequest")
            .field("repository", &"<redacted-identity>")
            .field("limits", &self.limits)
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Validated architecture aggregate with explicit source-only limitations.
#[derive(Debug, Eq, PartialEq)]
pub struct ArchitectureOverviewResult<G> {
    snapshot: SourceSnapshotDigest,
    generation: G,
    source_producer_manifest: ProducerManifestDigest,
    index_coverage: RustIndexCoverage,
    language_summaries: Box<[ArchitectureMapLanguageSummary]>,
    kind_summaries: Box<[ArchitectureOverviewKindSummary]>,
    source_roots: Box<[ArchitectureOverviewSourceRootSummary]>,
    entry_point_candidates: Box<[ArchitectureOverviewEntryPointCandidate]>,
    files: Box<[ArchitectureMapFile]>,
    total_files: u64,
    total_declarations: u64,
    total_source_roots: u64,
    total_entry_point_candidates: u64,
    output_bytes: u64,
}

impl<G> ArchitectureOverviewResult<G> {
    /// Returns the exact active source snapshot that was summarized.
    #[must_use]
    pub const fn snapshot(&self) -> SourceSnapshotDigest {
        self.snapshot
    }

    /// Returns the exact immutable active generation that was summarized.
    #[must_use]
    pub const fn generation(&self) -> &G {
        &self.generation
    }

    /// Returns the complete source-profile producer receipt for this snapshot.
    #[must_use]
    pub const fn source_producer_manifest(&self) -> ProducerManifestDigest {
        self.source_producer_manifest
    }

    /// Returns index coverage established before activation.
    #[must_use]
    pub const fn index_coverage(&self) -> RustIndexCoverage {
        self.index_coverage
    }

    /// Returns complete language totals in stable language order.
    #[must_use]
    pub const fn language_summaries(&self) -> &[ArchitectureMapLanguageSummary] {
        &self.language_summaries
    }

    /// Returns complete language/kind direct-declaration totals in stable order.
    #[must_use]
    pub const fn kind_summaries(&self) -> &[ArchitectureOverviewKindSummary] {
        &self.kind_summaries
    }

    /// Returns bounded exact structural source-root summaries.
    #[must_use]
    pub const fn source_roots(&self) -> &[ArchitectureOverviewSourceRootSummary] {
        &self.source_roots
    }

    /// Returns bounded exact direct-syntax `function main` candidates.
    #[must_use]
    pub const fn entry_point_candidates(&self) -> &[ArchitectureOverviewEntryPointCandidate] {
        &self.entry_point_candidates
    }

    /// Returns bounded exact per-file source declaration receipts.
    #[must_use]
    pub const fn files(&self) -> &[ArchitectureMapFile] {
        &self.files
    }

    /// Returns the complete indexed-file count before per-file receipt truncation.
    #[must_use]
    pub const fn total_files(&self) -> u64 {
        self.total_files
    }

    /// Returns the complete direct-declaration count before any receipt truncation.
    #[must_use]
    pub const fn total_declarations(&self) -> u64 {
        self.total_declarations
    }

    /// Returns the complete structural source-root count before root truncation.
    #[must_use]
    pub const fn total_source_roots(&self) -> u64 {
        self.total_source_roots
    }

    /// Returns the complete function-named-`main` count before candidate truncation.
    #[must_use]
    pub const fn total_entry_point_candidates(&self) -> u64 {
        self.total_entry_point_candidates
    }

    /// Returns whether exact source-root summaries were bounded.
    #[must_use]
    pub fn source_roots_truncated(&self) -> bool {
        is_truncated(self.source_roots.len(), self.total_source_roots)
    }

    /// Returns whether exact function-named-`main` candidates were bounded.
    #[must_use]
    pub fn entry_point_candidates_truncated(&self) -> bool {
        is_truncated(
            self.entry_point_candidates.len(),
            self.total_entry_point_candidates,
        )
    }

    /// Returns whether exact per-file declaration receipts were bounded.
    #[must_use]
    pub fn files_truncated(&self) -> bool {
        is_truncated(self.files.len(), self.total_files)
    }

    /// Returns conservative encoded application-output bytes.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
}

/// Stable invalid-storage-output classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchitectureOverviewPortOutputError {
    /// The adapter returned more source-root summaries than requested.
    SourceRootLimitExceeded,
    /// The adapter returned more entry-point candidates than requested.
    EntryPointCandidateLimitExceeded,
    /// The adapter returned more file receipts than requested.
    FileLimitExceeded,
    /// Complete totals are inconsistent with their grouped or exact receipts.
    InvalidTotals,
    /// The adapter exceeded the requested output ceiling.
    OutputByteLimitExceeded,
    /// A source-root summary was invalid, duplicated, or not deterministically ordered.
    InvalidSourceRoots,
    /// Language totals were invalid, duplicated, or not deterministically ordered.
    InvalidLanguageSummaries,
    /// Language/kind declaration totals were invalid or not deterministically ordered.
    InvalidKindSummaries,
    /// A returned file receipt was invalid, duplicated, or not in canonical path order.
    InvalidFiles,
    /// A candidate did not satisfy the exact v1 `function main` criterion.
    InvalidEntryPointCandidate,
    /// Candidates were duplicated or not ordered by canonical path and fact ordinal.
    InvalidEntryPointCandidateOrder,
    /// A fixed-width count could not be represented safely.
    CountNotRepresentable,
}

impl fmt::Display for ArchitectureOverviewPortOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SourceRootLimitExceeded => {
                "architecture-overview adapter exceeded the source-root limit"
            }
            Self::EntryPointCandidateLimitExceeded => {
                "architecture-overview adapter exceeded the entry-point candidate limit"
            }
            Self::FileLimitExceeded => "architecture-overview adapter exceeded the file limit",
            Self::InvalidTotals => "architecture-overview adapter returned inconsistent totals",
            Self::OutputByteLimitExceeded => {
                "architecture-overview adapter exceeded the output limit"
            }
            Self::InvalidSourceRoots => {
                "architecture-overview adapter returned invalid source-root summaries"
            }
            Self::InvalidLanguageSummaries => {
                "architecture-overview adapter returned invalid language summaries"
            }
            Self::InvalidKindSummaries => {
                "architecture-overview adapter returned invalid declaration-kind summaries"
            }
            Self::InvalidFiles => "architecture-overview adapter returned invalid file receipts",
            Self::InvalidEntryPointCandidate => {
                "architecture-overview adapter returned an invalid entry-point candidate"
            }
            Self::InvalidEntryPointCandidateOrder => {
                "architecture-overview adapter returned invalid entry-point candidate ordering"
            }
            Self::CountNotRepresentable => {
                "architecture-overview counts cannot be represented safely"
            }
        })
    }
}

impl Error for ArchitectureOverviewPortOutputError {}

/// Application failure for one all-or-nothing architecture-overview read.
#[derive(Debug)]
pub enum ArchitectureOverviewError<E> {
    /// Cancellation was observed before a complete result existed.
    Cancelled,
    /// The monotonic deadline elapsed before a complete result existed.
    DeadlineExceeded,
    /// The storage-neutral adapter failed.
    Port(E),
    /// The storage-neutral adapter violated the public receipt contract.
    InvalidPortOutput(ArchitectureOverviewPortOutputError),
}

impl<E: fmt::Display> fmt::Display for ArchitectureOverviewError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("architecture-overview read cancelled"),
            Self::DeadlineExceeded => {
                formatter.write_str("architecture-overview read deadline exceeded")
            }
            Self::Port(error) => write!(formatter, "architecture-overview adapter failed: {error}"),
            Self::InvalidPortOutput(error) => error.fmt(formatter),
        }
    }
}

impl<E: Error + 'static> Error for ArchitectureOverviewError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Port(error) => Some(error),
            Self::InvalidPortOutput(error) => Some(error),
            Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Reads a bounded source-only aggregate from one active generation.
pub fn architecture_overview<Port>(
    port: &Port,
    request: ArchitectureOverviewRequest,
) -> Result<ArchitectureOverviewResult<Port::Generation>, ArchitectureOverviewError<Port::Error>>
where
    Port: ArchitectureOverviewPort,
{
    check_control(&request.cancelled, request.deadline)?;
    let limits = request.limits;
    let result = port
        .architecture_overview(
            request.repository,
            limits,
            Arc::clone(&request.cancelled),
            request.deadline,
        )
        .map_err(ArchitectureOverviewError::Port)?;
    check_control(&request.cancelled, request.deadline)?;
    validate_port_result(&result, limits)?;
    Ok(ArchitectureOverviewResult {
        snapshot: result.snapshot,
        generation: result.generation,
        source_producer_manifest: result.source_producer_manifest,
        index_coverage: result.index_coverage,
        language_summaries: result.language_summaries.into_boxed_slice(),
        kind_summaries: result.kind_summaries.into_boxed_slice(),
        source_roots: result.source_roots.into_boxed_slice(),
        entry_point_candidates: result.entry_point_candidates.into_boxed_slice(),
        files: result.files.into_boxed_slice(),
        total_files: result.total_files,
        total_declarations: result.total_declarations,
        total_source_roots: result.total_source_roots,
        total_entry_point_candidates: result.total_entry_point_candidates,
        output_bytes: result.output_bytes,
    })
}

fn validate_port_result<G, E>(
    result: &ArchitectureOverviewPortResult<G>,
    limits: ArchitectureOverviewLimits,
) -> Result<(), ArchitectureOverviewError<E>> {
    validate_returned_limit(
        result.source_roots.len(),
        result.total_source_roots,
        limits.max_roots(),
        ArchitectureOverviewPortOutputError::SourceRootLimitExceeded,
    )?;
    validate_returned_limit(
        result.entry_point_candidates.len(),
        result.total_entry_point_candidates,
        limits.max_entry_point_candidates(),
        ArchitectureOverviewPortOutputError::EntryPointCandidateLimitExceeded,
    )?;
    validate_returned_limit(
        result.files.len(),
        result.total_files,
        limits.max_files(),
        ArchitectureOverviewPortOutputError::FileLimitExceeded,
    )?;
    if result.output_bytes > limits.max_output_bytes() {
        return Err(invalid(
            ArchitectureOverviewPortOutputError::OutputByteLimitExceeded,
        ));
    }

    let (language_files, language_declarations) = validate_languages(&result.language_summaries)?;
    if language_files != result.total_files || language_declarations != result.total_declarations {
        return Err(invalid(ArchitectureOverviewPortOutputError::InvalidTotals));
    }
    if validate_kinds(&result.kind_summaries)? != result.total_declarations {
        return Err(invalid(
            ArchitectureOverviewPortOutputError::InvalidKindSummaries,
        ));
    }
    if result.total_source_roots > result.total_files
        || result.total_entry_point_candidates > result.total_declarations
    {
        return Err(invalid(ArchitectureOverviewPortOutputError::InvalidTotals));
    }

    let (root_files, root_declarations) = validate_source_roots(&result.source_roots)?;
    if root_files > result.total_files || root_declarations > result.total_declarations {
        return Err(invalid(ArchitectureOverviewPortOutputError::InvalidTotals));
    }
    if !is_truncated(result.source_roots.len(), result.total_source_roots)
        && (root_files != result.total_files || root_declarations != result.total_declarations)
    {
        return Err(invalid(ArchitectureOverviewPortOutputError::InvalidTotals));
    }

    validate_entry_point_candidates(&result.entry_point_candidates)?;
    let file_declarations = validate_files(&result.files)?;
    if file_declarations > result.total_declarations
        || (!is_truncated(result.files.len(), result.total_files)
            && file_declarations != result.total_declarations)
    {
        return Err(invalid(ArchitectureOverviewPortOutputError::InvalidTotals));
    }
    Ok(())
}

fn validate_returned_limit<E>(
    returned: usize,
    total: u64,
    limit: u16,
    limit_error: ArchitectureOverviewPortOutputError,
) -> Result<(), ArchitectureOverviewError<E>> {
    let returned = u64::try_from(returned)
        .map_err(|_| invalid::<E>(ArchitectureOverviewPortOutputError::CountNotRepresentable))?;
    if returned > u64::from(limit) {
        return Err(invalid(limit_error));
    }
    if returned > total {
        return Err(invalid(ArchitectureOverviewPortOutputError::InvalidTotals));
    }
    Ok(())
}

fn validate_languages<E>(
    summaries: &[ArchitectureMapLanguageSummary],
) -> Result<(u64, u64), ArchitectureOverviewError<E>> {
    let mut previous = None;
    summaries
        .iter()
        .try_fold((0_u64, 0_u64), |(files, declarations), summary| {
            if summary.file_count() == 0
                || previous.is_some_and(|language: SourceLanguage| {
                    language.as_str() >= summary.language().as_str()
                })
            {
                return Err(invalid(
                    ArchitectureOverviewPortOutputError::InvalidLanguageSummaries,
                ));
            }
            previous = Some(summary.language());
            Ok((
                files.checked_add(summary.file_count()).ok_or(invalid(
                    ArchitectureOverviewPortOutputError::CountNotRepresentable,
                ))?,
                declarations
                    .checked_add(summary.declaration_count())
                    .ok_or(invalid(
                        ArchitectureOverviewPortOutputError::CountNotRepresentable,
                    ))?,
            ))
        })
}

fn validate_kinds<E>(
    summaries: &[ArchitectureOverviewKindSummary],
) -> Result<u64, ArchitectureOverviewError<E>> {
    let mut previous = None;
    summaries.iter().try_fold(0_u64, |total, summary| {
        let current = (summary.language(), summary.kind());
        if summary.declaration_count() == 0
            || previous.is_some_and(|previous: (SourceLanguage, RustSymbolKind)| {
                (previous.0.as_str(), previous.1.as_str())
                    >= (current.0.as_str(), current.1.as_str())
            })
        {
            return Err(invalid(
                ArchitectureOverviewPortOutputError::InvalidKindSummaries,
            ));
        }
        previous = Some(current);
        total
            .checked_add(summary.declaration_count())
            .ok_or(invalid(
                ArchitectureOverviewPortOutputError::CountNotRepresentable,
            ))
    })
}

fn validate_source_roots<E>(
    roots: &[ArchitectureOverviewSourceRootSummary],
) -> Result<(u64, u64), ArchitectureOverviewError<E>> {
    let mut previous = None;
    roots
        .iter()
        .try_fold((0_u64, 0_u64), |(files, declarations), summary| {
            if summary.file_count() == 0
                || matches!(summary.root(), ArchitectureOverviewSourceRoot::TopLevelDirectory(path) if path.as_bytes().contains(&b'/'))
                || previous
                    .is_some_and(|root: &ArchitectureOverviewSourceRoot| root >= summary.root())
            {
                return Err(invalid(
                    ArchitectureOverviewPortOutputError::InvalidSourceRoots,
                ));
            }
            previous = Some(summary.root());
            Ok((
                files.checked_add(summary.file_count()).ok_or(invalid(
                    ArchitectureOverviewPortOutputError::CountNotRepresentable,
                ))?,
                declarations
                    .checked_add(summary.declaration_count())
                    .ok_or(invalid(
                        ArchitectureOverviewPortOutputError::CountNotRepresentable,
                    ))?,
            ))
        })
}

fn validate_entry_point_candidates<E>(
    candidates: &[ArchitectureOverviewEntryPointCandidate],
) -> Result<(), ArchitectureOverviewError<E>> {
    let mut previous: Option<(&RepositoryPath, u64)> = None;
    for candidate in candidates {
        let occurrence = candidate.occurrence();
        if occurrence.kind() != RustSymbolKind::Function
            || occurrence.name() != "main"
            || !occurrence
                .language()
                .matches_repository_path(candidate.path())
        {
            return Err(invalid(
                ArchitectureOverviewPortOutputError::InvalidEntryPointCandidate,
            ));
        }
        let current = (candidate.path(), occurrence.fact_ordinal());
        if previous.is_some_and(|previous| previous >= current) {
            return Err(invalid(
                ArchitectureOverviewPortOutputError::InvalidEntryPointCandidateOrder,
            ));
        }
        previous = Some(current);
    }
    Ok(())
}

fn validate_files<E>(files: &[ArchitectureMapFile]) -> Result<u64, ArchitectureOverviewError<E>> {
    let mut previous = None;
    files.iter().try_fold(0_u64, |declarations, file| {
        if !file.language().matches_repository_path(file.path())
            || previous.is_some_and(|path: &RepositoryPath| path >= file.path())
        {
            return Err(invalid(ArchitectureOverviewPortOutputError::InvalidFiles));
        }
        previous = Some(file.path());
        declarations
            .checked_add(file.declaration_count())
            .ok_or(invalid(
                ArchitectureOverviewPortOutputError::CountNotRepresentable,
            ))
    })
}

fn is_truncated(returned: usize, total: u64) -> bool {
    u64::try_from(returned).ok() < Some(total)
}

fn invalid<E>(error: ArchitectureOverviewPortOutputError) -> ArchitectureOverviewError<E> {
    ArchitectureOverviewError::InvalidPortOutput(error)
}

fn check_control<E>(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), ArchitectureOverviewError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(ArchitectureOverviewError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(ArchitectureOverviewError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
