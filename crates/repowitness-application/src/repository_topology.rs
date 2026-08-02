//! Bounded, generation-pinned, path-only repository topology inventory.
//!
//! This profile deliberately classifies canonical repository paths without
//! reading or returning their contents. A category is not a package,
//! ownership, dependency, build, deployment, or runtime claim.

use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_domain::{RepositoryIdentityDigest, RepositoryPath, SourceSnapshotDigest};

/// Version of the bounded path-only topology profile.
pub const REPOSITORY_TOPOLOGY_PROFILE_VERSION: u16 = 1;
/// Default maximum number of exact path receipts returned by one topology read.
pub const DEFAULT_REPOSITORY_TOPOLOGY_PATHS: u16 = 200;
/// Default conservative encoded-output ceiling for one topology read.
pub const DEFAULT_REPOSITORY_TOPOLOGY_OUTPUT_BYTES: u64 = 512 * 1024;
/// Hard maximum number of returned exact path receipts.
pub const MAX_REPOSITORY_TOPOLOGY_PATHS: u16 = 1_000;
/// Hard conservative encoded-output ceiling for one topology read.
pub const MAX_REPOSITORY_TOPOLOGY_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;

/// A fixed, path-only allow-list classification.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RepositoryTopologyCategory {
    /// Documentation path.
    Documentation,
    /// Agent instruction path.
    AgentInstruction,
    /// Continuous-integration workflow descriptor path.
    WorkflowDescriptor,
    /// Build descriptor path.
    BuildDescriptor,
    /// Package descriptor path.
    PackageDescriptor,
    /// Configuration descriptor path.
    ConfigurationDescriptor,
    /// A tracked path outside the specific allow-list categories.
    OtherTrackedFile,
}

impl RepositoryTopologyCategory {
    /// Returns the stable persisted and wire category label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Documentation => "documentation",
            Self::AgentInstruction => "agent_instruction",
            Self::WorkflowDescriptor => "workflow_descriptor",
            Self::BuildDescriptor => "build_descriptor",
            Self::PackageDescriptor => "package_descriptor",
            Self::ConfigurationDescriptor => "configuration_descriptor",
            Self::OtherTrackedFile => "other_tracked_file",
        }
    }

    /// Decodes one stable persisted category label.
    #[must_use]
    pub fn from_stable_str(value: &str) -> Option<Self> {
        Some(match value {
            "documentation" => Self::Documentation,
            "agent_instruction" => Self::AgentInstruction,
            "workflow_descriptor" => Self::WorkflowDescriptor,
            "build_descriptor" => Self::BuildDescriptor,
            "package_descriptor" => Self::PackageDescriptor,
            "configuration_descriptor" => Self::ConfigurationDescriptor,
            "other_tracked_file" => Self::OtherTrackedFile,
            _ => return None,
        })
    }

    /// Returns categories in stable label order.
    #[must_use]
    pub const fn all() -> [Self; 7] {
        [
            Self::AgentInstruction,
            Self::BuildDescriptor,
            Self::ConfigurationDescriptor,
            Self::Documentation,
            Self::OtherTrackedFile,
            Self::PackageDescriptor,
            Self::WorkflowDescriptor,
        ]
    }
}

/// One exact path-only topology receipt.
#[derive(Clone, Eq, PartialEq)]
pub struct RepositoryTopologyEntry {
    path: RepositoryPath,
    category: RepositoryTopologyCategory,
}

impl fmt::Debug for RepositoryTopologyEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryTopologyEntry")
            .field("path", &"<redacted-path>")
            .field("category", &self.category)
            .finish()
    }
}

impl RepositoryTopologyEntry {
    /// Constructs one untrusted adapter path receipt.
    #[must_use]
    pub const fn new(path: RepositoryPath, category: RepositoryTopologyCategory) -> Self {
        Self { path, category }
    }

    /// Returns the exact canonical repository-relative path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the path-only category.
    #[must_use]
    pub const fn category(&self) -> RepositoryTopologyCategory {
        self.category
    }
}

/// Complete path totals for one topology category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryTopologyCategorySummary {
    category: RepositoryTopologyCategory,
    path_count: u64,
}

impl RepositoryTopologyCategorySummary {
    /// Constructs one untrusted adapter summary.
    #[must_use]
    pub const fn new(category: RepositoryTopologyCategory, path_count: u64) -> Self {
        Self {
            category,
            path_count,
        }
    }

    /// Returns the category.
    #[must_use]
    pub const fn category(&self) -> RepositoryTopologyCategory {
        self.category
    }

    /// Returns the complete count before entry truncation.
    #[must_use]
    pub const fn path_count(&self) -> u64 {
        self.path_count
    }
}

/// Explicit path-discovery coverage for one persisted topology inventory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryTopologyCoverage {
    discovered_paths: u64,
    omitted_paths: u64,
}

impl RepositoryTopologyCoverage {
    /// Constructs coverage from trusted publication totals.
    #[must_use]
    pub const fn new(discovered_paths: u64, omitted_paths: u64) -> Self {
        Self {
            discovered_paths,
            omitted_paths,
        }
    }

    /// Returns complete discovered path count.
    #[must_use]
    pub const fn discovered_paths(self) -> u64 {
        self.discovered_paths
    }

    /// Returns paths explicitly omitted before publication.
    #[must_use]
    pub const fn omitted_paths(self) -> u64 {
        self.omitted_paths
    }
}

/// Stable topology read bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryTopologyLimits {
    max_paths: u16,
    max_output_bytes: u64,
}

impl RepositoryTopologyLimits {
    /// Validates bounds against the fixed public profile.
    pub const fn try_new(
        max_paths: u16,
        max_output_bytes: u64,
    ) -> Result<Self, RepositoryTopologyLimitError> {
        if max_paths == 0
            || max_paths > MAX_REPOSITORY_TOPOLOGY_PATHS
            || max_output_bytes == 0
            || max_output_bytes > MAX_REPOSITORY_TOPOLOGY_OUTPUT_BYTES
        {
            return Err(RepositoryTopologyLimitError);
        }
        Ok(Self {
            max_paths,
            max_output_bytes,
        })
    }

    /// Returns the exact returned-path ceiling.
    #[must_use]
    pub const fn max_paths(self) -> u16 {
        self.max_paths
    }

    /// Returns the conservative encoded-output ceiling.
    #[must_use]
    pub const fn max_output_bytes(self) -> u64 {
        self.max_output_bytes
    }
}

impl Default for RepositoryTopologyLimits {
    fn default() -> Self {
        Self {
            max_paths: DEFAULT_REPOSITORY_TOPOLOGY_PATHS,
            max_output_bytes: DEFAULT_REPOSITORY_TOPOLOGY_OUTPUT_BYTES,
        }
    }
}

/// Stable invalid topology-limit classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryTopologyLimitError;

impl fmt::Display for RepositoryTopologyLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("repository-topology limits are zero or exceed compiled ceilings")
    }
}

impl Error for RepositoryTopologyLimitError {}

/// Immutable receipt identifying one topology publication before its bounded rows.
pub struct RepositoryTopologyReceipt<G> {
    snapshot: SourceSnapshotDigest,
    generation: G,
    topology_profile_version: u16,
    topology_digest: [u8; 32],
}

impl<G> RepositoryTopologyReceipt<G> {
    /// Constructs the immutable identity fields supplied by storage.
    #[must_use]
    pub const fn new(
        snapshot: SourceSnapshotDigest,
        generation: G,
        topology_profile_version: u16,
        topology_digest: [u8; 32],
    ) -> Self {
        Self {
            snapshot,
            generation,
            topology_profile_version,
            topology_digest,
        }
    }
}

/// Untrusted storage response pinned to an immutable active generation.
pub struct RepositoryTopologyPortResult<G> {
    receipt: RepositoryTopologyReceipt<G>,
    coverage: RepositoryTopologyCoverage,
    entries: Vec<RepositoryTopologyEntry>,
    category_summaries: Vec<RepositoryTopologyCategorySummary>,
    total_paths: u64,
    output_bytes: u64,
}

impl<G> RepositoryTopologyPortResult<G> {
    /// Constructs an untrusted storage response for application validation.
    #[must_use]
    pub const fn new(
        receipt: RepositoryTopologyReceipt<G>,
        coverage: RepositoryTopologyCoverage,
        entries: Vec<RepositoryTopologyEntry>,
        category_summaries: Vec<RepositoryTopologyCategorySummary>,
        total_paths: u64,
        output_bytes: u64,
    ) -> Self {
        Self {
            receipt,
            coverage,
            entries,
            category_summaries,
            total_paths,
            output_bytes,
        }
    }
}

/// Narrow path-only immutable topology read port shared by CLI and MCP.
pub trait RepositoryTopologyPort {
    /// Immutable local generation identity.
    type Generation;
    /// Stable storage-adapter error.
    type Error;

    /// Reads a bounded topology inventory from one active generation.
    fn repository_topology(
        &self,
        repository: RepositoryIdentityDigest,
        limits: RepositoryTopologyLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RepositoryTopologyPortResult<Self::Generation>, Self::Error>;
}

/// Application request for one active-generation path-only topology inventory.
pub struct RepositoryTopologyRequest {
    repository: RepositoryIdentityDigest,
    limits: RepositoryTopologyLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl RepositoryTopologyRequest {
    /// Creates a bounded request from trusted composition inputs.
    #[must_use]
    pub const fn new(
        repository: RepositoryIdentityDigest,
        limits: RepositoryTopologyLimits,
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

impl fmt::Debug for RepositoryTopologyRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryTopologyRequest")
            .field("repository", &"<redacted-identity>")
            .field("limits", &self.limits)
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Validated topology result with explicit path-only limitations.
#[derive(Eq, PartialEq)]
pub struct RepositoryTopologyResult<G> {
    snapshot: SourceSnapshotDigest,
    generation: G,
    topology_profile_version: u16,
    topology_digest: [u8; 32],
    coverage: RepositoryTopologyCoverage,
    entries: Box<[RepositoryTopologyEntry]>,
    category_summaries: Box<[RepositoryTopologyCategorySummary]>,
    total_paths: u64,
    output_bytes: u64,
}

impl<G> RepositoryTopologyResult<G> {
    /// Returns the active source snapshot paired with this topology receipt.
    #[must_use]
    pub const fn snapshot(&self) -> SourceSnapshotDigest {
        self.snapshot
    }
    /// Returns the active immutable generation.
    #[must_use]
    pub const fn generation(&self) -> &G {
        &self.generation
    }
    /// Returns the persisted, validated topology profile version.
    #[must_use]
    pub const fn topology_profile_version(&self) -> u16 {
        self.topology_profile_version
    }
    /// Returns the separate path-only topology digest.
    #[must_use]
    pub const fn topology_digest(&self) -> &[u8; 32] {
        &self.topology_digest
    }
    /// Returns explicit discovery coverage.
    #[must_use]
    pub const fn coverage(&self) -> RepositoryTopologyCoverage {
        self.coverage
    }
    /// Returns path receipts in canonical byte-path order.
    #[must_use]
    pub const fn entries(&self) -> &[RepositoryTopologyEntry] {
        &self.entries
    }
    /// Returns complete category totals in stable category order.
    #[must_use]
    pub const fn category_summaries(&self) -> &[RepositoryTopologyCategorySummary] {
        &self.category_summaries
    }
    /// Returns the complete persisted path count before entry truncation.
    #[must_use]
    pub const fn total_paths(&self) -> u64 {
        self.total_paths
    }
    /// Returns conservative encoded-output bytes.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
    /// Returns whether returned entries were bounded before all paths were returned.
    #[must_use]
    pub fn truncated(&self) -> bool {
        u64::try_from(self.entries.len()).ok() < Some(self.total_paths)
    }
}

impl<G> fmt::Debug for RepositoryTopologyResult<G> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryTopologyResult")
            .field("snapshot", &"<redacted-digest>")
            .field("generation", &"<redacted-generation>")
            .field("topology_profile_version", &self.topology_profile_version)
            .field("topology_digest", &"<redacted-digest>")
            .field("coverage", &self.coverage)
            .field(
                "entries",
                &format_args!("<{} redacted entries>", self.entries.len()),
            )
            .field("category_summaries", &self.category_summaries)
            .field("total_paths", &self.total_paths)
            .field("output_bytes", &self.output_bytes)
            .finish()
    }
}

/// Stable invalid topology-port-output classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryTopologyPortOutputError {
    /// The adapter returned an unsupported topology profile.
    InvalidTopologyProfile,
    /// The adapter returned more entries than requested.
    PathLimitExceeded,
    /// Totals or coverage are inconsistent.
    InvalidTotals,
    /// The adapter exceeded the requested output ceiling.
    OutputByteLimitExceeded,
    /// Entries were duplicated or not byte-path ordered.
    InvalidPathOrder,
    /// Category totals were malformed.
    InvalidCategorySummaries,
    /// A fixed-width count could not be represented.
    CountNotRepresentable,
}

impl fmt::Display for RepositoryTopologyPortOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidTopologyProfile => {
                "repository-topology adapter returned an unsupported profile"
            }
            Self::PathLimitExceeded => "repository-topology adapter exceeded the path limit",
            Self::InvalidTotals => "repository-topology adapter returned inconsistent totals",
            Self::OutputByteLimitExceeded => {
                "repository-topology adapter exceeded the output limit"
            }
            Self::InvalidPathOrder => "repository-topology adapter returned invalid path ordering",
            Self::InvalidCategorySummaries => {
                "repository-topology adapter returned invalid category summaries"
            }
            Self::CountNotRepresentable => {
                "repository-topology counts cannot be represented safely"
            }
        })
    }
}

impl Error for RepositoryTopologyPortOutputError {}

/// Application failure for one all-or-nothing topology read.
#[derive(Debug)]
pub enum RepositoryTopologyError<E> {
    /// Cancellation was observed before a complete result existed.
    Cancelled,
    /// The monotonic deadline elapsed before a complete result existed.
    DeadlineExceeded,
    /// The storage-neutral adapter failed.
    Port(E),
    /// The storage-neutral adapter violated the public receipt contract.
    InvalidPortOutput(RepositoryTopologyPortOutputError),
}

impl<E: fmt::Display> fmt::Display for RepositoryTopologyError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("repository-topology read cancelled"),
            Self::DeadlineExceeded => {
                formatter.write_str("repository-topology read deadline exceeded")
            }
            Self::Port(error) => write!(formatter, "repository-topology adapter failed: {error}"),
            Self::InvalidPortOutput(error) => error.fmt(formatter),
        }
    }
}

impl<E: Error + 'static> Error for RepositoryTopologyError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Port(error) => Some(error),
            Self::InvalidPortOutput(error) => Some(error),
            Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Reads a bounded path-only inventory from one active immutable generation.
pub fn repository_topology<Port>(
    port: &Port,
    request: RepositoryTopologyRequest,
) -> Result<RepositoryTopologyResult<Port::Generation>, RepositoryTopologyError<Port::Error>>
where
    Port: RepositoryTopologyPort,
{
    check_control(&request.cancelled, request.deadline)?;
    let limits = request.limits;
    let result = port
        .repository_topology(
            request.repository,
            limits,
            Arc::clone(&request.cancelled),
            request.deadline,
        )
        .map_err(RepositoryTopologyError::Port)?;
    check_control(&request.cancelled, request.deadline)?;
    validate_port_result(&result, limits)?;
    Ok(RepositoryTopologyResult {
        snapshot: result.receipt.snapshot,
        generation: result.receipt.generation,
        topology_profile_version: result.receipt.topology_profile_version,
        topology_digest: result.receipt.topology_digest,
        coverage: result.coverage,
        entries: result.entries.into_boxed_slice(),
        category_summaries: result.category_summaries.into_boxed_slice(),
        total_paths: result.total_paths,
        output_bytes: result.output_bytes,
    })
}

fn validate_port_result<G, E>(
    result: &RepositoryTopologyPortResult<G>,
    limits: RepositoryTopologyLimits,
) -> Result<(), RepositoryTopologyError<E>> {
    if result.receipt.topology_profile_version != REPOSITORY_TOPOLOGY_PROFILE_VERSION {
        return Err(invalid(
            RepositoryTopologyPortOutputError::InvalidTopologyProfile,
        ));
    }
    let returned = u64::try_from(result.entries.len())
        .map_err(|_| invalid(RepositoryTopologyPortOutputError::CountNotRepresentable))?;
    if returned > u64::from(limits.max_paths()) {
        return Err(invalid(
            RepositoryTopologyPortOutputError::PathLimitExceeded,
        ));
    }
    if returned > result.total_paths
        || result.coverage.discovered_paths() < result.total_paths
        || result.coverage.omitted_paths() != 0
    {
        return Err(invalid(RepositoryTopologyPortOutputError::InvalidTotals));
    }
    if result.output_bytes > limits.max_output_bytes() {
        return Err(invalid(
            RepositoryTopologyPortOutputError::OutputByteLimitExceeded,
        ));
    }
    if result
        .entries
        .windows(2)
        .any(|pair| pair[0].path >= pair[1].path)
    {
        return Err(invalid(RepositoryTopologyPortOutputError::InvalidPathOrder));
    }
    if result.category_summaries.len() != RepositoryTopologyCategory::all().len() {
        return Err(invalid(
            RepositoryTopologyPortOutputError::InvalidCategorySummaries,
        ));
    }
    let mut total = 0_u64;
    for (summary, category) in result
        .category_summaries
        .iter()
        .zip(RepositoryTopologyCategory::all())
    {
        if summary.category != category {
            return Err(invalid(
                RepositoryTopologyPortOutputError::InvalidCategorySummaries,
            ));
        }
        total = total
            .checked_add(summary.path_count)
            .ok_or_else(|| invalid(RepositoryTopologyPortOutputError::CountNotRepresentable))?;
    }
    if total != result.total_paths {
        return Err(invalid(
            RepositoryTopologyPortOutputError::InvalidCategorySummaries,
        ));
    }
    Ok(())
}

fn invalid<E>(error: RepositoryTopologyPortOutputError) -> RepositoryTopologyError<E> {
    RepositoryTopologyError::InvalidPortOutput(error)
}

fn check_control<E>(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), RepositoryTopologyError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(RepositoryTopologyError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(RepositoryTopologyError::DeadlineExceeded)
    } else {
        Ok(())
    }
}
