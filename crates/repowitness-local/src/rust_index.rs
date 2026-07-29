use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use repowitness_analysis::{RustGraphAnalysisError, RustGraphSiteAnalysis, RustSourceAnalysis};
use repowitness_application::{
    ImmutableRustSource, PackageScope, PreparedRustIndex, RustArtifactIdentity, RustIndexLimits,
    RustIndexPreparationError, SourceArtifactIdentities, SourceLanguage,
    hash_analysis_artifact_key, hash_source_content, phase1_rust_graph_artifact_identity,
    prepare_source_index_with_reuse,
};
use repowitness_domain::{
    AnalysisArtifactDigest, AnalysisArtifactKey, GitStateDigest, WorktreeStateDigest,
};

use crate::contained_source::{
    ContainedSourceError, ContainedSourceRoot, FileIdentity, SourceReadLimitError, SourceReadLimits,
};
use crate::git_paths::{
    DiscoveredRepositoryPaths, GitPathDiscoveryError, GitPathDiscoveryLimits,
    discover_repository_paths_with_cancel, discovered_worktree_root,
};
use crate::source_state::{SourceStateError, capture_source_state_with_cancel};
use crate::sqlite::SqliteStoreError;

mod graph_preparation;
pub(crate) use graph_preparation::PreparedLocalRustGraphArtifact;
use graph_preparation::{
    prepare_local_rust_graph_artifacts, requested_local_rust_graph_artifact_digests,
};
mod source_snapshot_fence;
pub use source_snapshot_fence::LocalSourceSnapshotFenceError;
pub(crate) use source_snapshot_fence::{
    LocalSourceSnapshotFenceRequest, confirm_local_source_snapshot,
};

/// Default wall-clock deadline for complete local source index preparation.
pub const DEFAULT_LOCAL_RUST_INDEX_DEADLINE: Duration = Duration::from_secs(30);

/// All stage-specific and end-to-end limits for local source preparation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalRustIndexLimits {
    deadline: Duration,
    discovery: GitPathDiscoveryLimits,
    source_read: SourceReadLimits,
    preparation: RustIndexLimits,
}

impl LocalRustIndexLimits {
    /// Constructs one explicit end-to-end local preparation policy.
    #[must_use]
    pub const fn new(
        deadline: Duration,
        discovery: GitPathDiscoveryLimits,
        source_read: SourceReadLimits,
        preparation: RustIndexLimits,
    ) -> Self {
        Self {
            deadline,
            discovery,
            source_read,
            preparation,
        }
    }

    /// Returns the end-to-end wall-clock deadline.
    #[must_use]
    pub const fn deadline(self) -> Duration {
        self.deadline
    }

    /// Returns bounded Git path-discovery limits.
    #[must_use]
    pub const fn discovery(self) -> GitPathDiscoveryLimits {
        self.discovery
    }

    /// Returns per-file contained-read limits.
    #[must_use]
    pub const fn source_read(self) -> SourceReadLimits {
        self.source_read
    }

    /// Returns aggregate and per-file analysis limits.
    #[must_use]
    pub const fn preparation(self) -> RustIndexLimits {
        self.preparation
    }
}

impl Default for LocalRustIndexLimits {
    fn default() -> Self {
        Self {
            deadline: DEFAULT_LOCAL_RUST_INDEX_DEADLINE,
            discovery: GitPathDiscoveryLimits::default(),
            source_read: SourceReadLimits::default(),
            preparation: RustIndexLimits::default(),
        }
    }
}

/// A fully prepared local source index plus explicit discovery coverage.
pub struct LocalRustIndexPreparation {
    prepared: PreparedRustIndex,
    graph_artifacts: Box<[PreparedLocalRustGraphArtifact]>,
    git_state: GitStateDigest,
    worktree_state: WorktreeStateDigest,
    discovered_paths: u64,
    selected_rust_files: u64,
    selected_go_files: u64,
    selected_typescript_files: u64,
    selected_tsx_files: u64,
    selected_python_files: u64,
    skipped_policy_paths: u64,
    skipped_unsupported_paths: u64,
}

impl LocalRustIndexPreparation {
    /// Returns canonical manifest, artifact identities, and deterministic facts.
    #[must_use]
    pub const fn prepared(&self) -> &PreparedRustIndex {
        &self.prepared
    }

    /// Consumes the wrapper and returns the complete prepared index.
    #[must_use]
    pub fn into_prepared(self) -> PreparedRustIndex {
        self.prepared
    }

    pub(crate) fn into_prepared_parts(
        self,
    ) -> (PreparedRustIndex, Box<[PreparedLocalRustGraphArtifact]>) {
        (self.prepared, self.graph_artifacts)
    }

    /// Returns the stable concrete Git-state receipt captured around preparation.
    #[must_use]
    pub const fn git_state(&self) -> GitStateDigest {
        self.git_state
    }

    /// Returns the stable worktree receipt bound to the prepared source manifest.
    #[must_use]
    pub const fn worktree_state(&self) -> WorktreeStateDigest {
        self.worktree_state
    }

    /// Returns every Git-discovered repository path in scope.
    #[must_use]
    pub const fn discovered_paths(&self) -> u64 {
        self.discovered_paths
    }

    /// Returns the number of selected case-sensitive `.rs` paths.
    #[must_use]
    pub const fn selected_rust_files(&self) -> u64 {
        self.selected_rust_files
    }

    /// Returns the number of selected case-sensitive `.go` paths.
    #[must_use]
    pub const fn selected_go_files(&self) -> u64 {
        self.selected_go_files
    }

    /// Returns the number of selected case-sensitive `.ts` paths.
    #[must_use]
    pub const fn selected_typescript_files(&self) -> u64 {
        self.selected_typescript_files
    }

    /// Returns the number of selected case-sensitive `.tsx` paths.
    #[must_use]
    pub const fn selected_tsx_files(&self) -> u64 {
        self.selected_tsx_files
    }

    /// Returns the number of selected case-sensitive `.py` and `.pyi` paths.
    #[must_use]
    pub const fn selected_python_files(&self) -> u64 {
        self.selected_python_files
    }

    /// Returns supported-language paths excluded by resolved policy.
    #[must_use]
    pub const fn skipped_policy_paths(&self) -> u64 {
        self.skipped_policy_paths
    }

    /// Returns paths explicitly skipped by the supported-language adapters.
    #[must_use]
    pub const fn skipped_unsupported_paths(&self) -> u64 {
        self.skipped_unsupported_paths
    }

    /// Returns paths skipped by the selected compatibility policy.
    ///
    /// For Rust-only entry points this retains its original meaning. Mixed
    /// entry points return unsupported-language paths.
    #[must_use]
    pub const fn skipped_non_rust_paths(&self) -> u64 {
        self.skipped_unsupported_paths
    }
}

impl fmt::Debug for LocalRustIndexPreparation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalRustIndexPreparation")
            .field("prepared", &self.prepared)
            .field("graph_artifact_count", &self.graph_artifacts.len())
            .field("git_state", &self.git_state)
            .field("worktree_state", &self.worktree_state)
            .field("discovered_paths", &self.discovered_paths)
            .field("selected_rust_files", &self.selected_rust_files)
            .field("selected_go_files", &self.selected_go_files)
            .field("selected_typescript_files", &self.selected_typescript_files)
            .field("selected_tsx_files", &self.selected_tsx_files)
            .field("selected_python_files", &self.selected_python_files)
            .field("skipped_policy_paths", &self.skipped_policy_paths)
            .field("skipped_unsupported_paths", &self.skipped_unsupported_paths)
            .finish()
    }
}

/// Failure to discover, read, analyze, or revalidate one local Rust snapshot.
#[derive(Debug)]
pub enum LocalRustIndexError {
    /// The configured duration cannot be represented by the monotonic clock.
    DeadlineNotRepresentable,
    /// The end-to-end operation was cancelled.
    Cancelled,
    /// The end-to-end wall-clock deadline elapsed.
    DeadlineExceeded,
    /// Repository root resolution or Git path discovery failed.
    Discovery {
        /// Stable redacted discovery failure.
        source: GitPathDiscoveryError,
    },
    /// Canonical Git/worktree source-state capture failed.
    SourceState {
        /// Stable redacted source-state failure.
        source: SourceStateError,
    },
    /// The resolved worktree directory capability could not be opened.
    RootOpen {
        /// Stable redacted contained-source failure.
        source: ContainedSourceError,
    },
    /// A selected source could not be opened or read.
    SourceRead {
        /// One-based selected-file ordinal.
        ordinal: u64,
        /// Stable redacted contained-source failure.
        source: ContainedSourceError,
    },
    /// A discovered repository path aliases an excluded external file.
    ExcludedFileAlias,
    /// A selected source byte count overflowed.
    SourceByteCountOverflowed,
    /// Selected source bytes exceeded the aggregate preparation limit.
    SourceByteLimitExceeded {
        /// Configured inclusive limit.
        limit: u64,
    },
    /// A remaining-deadline source-read policy could not be constructed.
    DerivedReadLimits {
        /// Stable limit failure.
        source: SourceReadLimitError,
    },
    /// Pure application preparation failed without publishing partial output.
    Preparation {
        /// Stable redacted preparation failure.
        source: RustIndexPreparationError,
    },
    /// Pure raw Rust graph-site preparation failed without partial output.
    GraphPreparation {
        /// Stable redacted graph-analysis failure.
        source: RustGraphAnalysisError,
    },
    /// Persisted artifact inventory failed validation or bounded loading.
    ArtifactReuse {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// Package-scope filtering failed before source bytes were read.
    PackageScope,
    /// The repository path set changed during preparation.
    StalePathSet,
    /// A selected source's exact bytes changed during preparation.
    StaleSourceContent {
        /// One-based canonical file ordinal.
        ordinal: u64,
    },
    /// Revalidation could not reopen a selected source.
    RevalidationRead {
        /// One-based canonical file ordinal.
        ordinal: u64,
        /// Stable redacted contained-source failure.
        source: ContainedSourceError,
    },
}

impl fmt::Display for LocalRustIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeadlineNotRepresentable => {
                formatter.write_str("local source index deadline is not representable")
            }
            Self::Cancelled => formatter.write_str("local source index preparation was cancelled"),
            Self::DeadlineExceeded => {
                formatter.write_str("local source index preparation exceeded its deadline")
            }
            Self::Discovery { .. } => formatter.write_str("repository path discovery failed"),
            Self::SourceState { .. } => {
                formatter.write_str("repository source-state capture failed")
            }
            Self::RootOpen { .. } => {
                formatter.write_str("repository source capability could not be opened")
            }
            Self::SourceRead { ordinal, .. } => {
                write!(formatter, "source ordinal {ordinal} could not be read")
            }
            Self::ExcludedFileAlias => {
                formatter.write_str("repository path aliases an excluded external file")
            }
            Self::SourceByteCountOverflowed => {
                formatter.write_str("selected source byte count overflowed")
            }
            Self::SourceByteLimitExceeded { limit } => {
                write!(
                    formatter,
                    "selected source bytes exceed the limit of {limit}"
                )
            }
            Self::DerivedReadLimits { .. } => {
                formatter.write_str("remaining source-read limits could not be represented")
            }
            Self::Preparation { .. } => formatter.write_str("source index preparation failed"),
            Self::GraphPreparation { .. } => {
                formatter.write_str("Rust graph-site preparation failed")
            }
            Self::ArtifactReuse { .. } => {
                formatter.write_str("reusable source artifact loading failed")
            }
            Self::PackageScope => formatter.write_str("package-scope filtering failed"),
            Self::StalePathSet => {
                formatter.write_str("repository path set changed during preparation")
            }
            Self::StaleSourceContent { ordinal } => {
                write!(
                    formatter,
                    "source ordinal {ordinal} changed during preparation"
                )
            }
            Self::RevalidationRead { ordinal, .. } => {
                write!(
                    formatter,
                    "source ordinal {ordinal} could not be revalidated"
                )
            }
        }
    }
}

impl Error for LocalRustIndexError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Discovery { source } => Some(source),
            Self::SourceState { source } => Some(source),
            Self::RootOpen { source } => Some(source),
            Self::SourceRead { source, .. } => Some(source),
            Self::DerivedReadLimits { source } => Some(source),
            Self::Preparation { source } => Some(source),
            Self::GraphPreparation { source } => Some(source),
            Self::ArtifactReuse { source } => Some(source),
            Self::RevalidationRead { source, .. } => Some(source),
            Self::DeadlineNotRepresentable
            | Self::Cancelled
            | Self::DeadlineExceeded
            | Self::ExcludedFileAlias
            | Self::SourceByteCountOverflowed
            | Self::SourceByteLimitExceeded { .. }
            | Self::PackageScope
            | Self::StalePathSet
            | Self::StaleSourceContent { .. } => None,
        }
    }
}

include!("rust_index/preparation_entrypoints.rs");

struct LocalPreparationContext<'a> {
    requested_root: &'a Path,
    identities: SourceArtifactIdentities,
    graph_identity: RustArtifactIdentity,
    selection: SelectionPolicy,
    package_scope: Option<&'a PackageScope>,
    limits: LocalRustIndexLimits,
    cancelled: &'a AtomicBool,
    excluded_identity: Option<&'a FileIdentity>,
}

fn map_graph_preparation_error(source: RustGraphAnalysisError) -> LocalRustIndexError {
    match source {
        RustGraphAnalysisError::Cancelled => LocalRustIndexError::Cancelled,
        RustGraphAnalysisError::DeadlineExceeded => LocalRustIndexError::DeadlineExceeded,
        source => LocalRustIndexError::GraphPreparation { source },
    }
}

struct LocalArtifactPreparationContext<'a> {
    sources: Vec<ImmutableRustSource>,
    identities: SourceArtifactIdentities,
    graph_identity: RustArtifactIdentity,
    limits: LocalRustIndexLimits,
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

struct PreparedLocalArtifacts {
    source: PreparedRustIndex,
    graph: Box<[PreparedLocalRustGraphArtifact]>,
}

struct LocalPreparationCompletion<'a> {
    prepared: PreparedRustIndex,
    graph_artifacts: Box<[PreparedLocalRustGraphArtifact]>,
    git_state: GitStateDigest,
    worktree_state: WorktreeStateDigest,
    discovered: &'a ScopedRepositoryPaths,
    counts: SelectedLanguageCounts,
    skipped_policy_paths: u64,
    skipped_unsupported_paths: u64,
}

fn prepare_selected_source_artifacts(
    context: LocalArtifactPreparationContext<'_>,
    load_reusable: &mut impl FnMut(
        SourceLanguage,
        &[AnalysisArtifactDigest],
        Instant,
    ) -> Result<
        BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
        SqliteStoreError,
    >,
    load_reusable_graph: &mut impl FnMut(
        &[AnalysisArtifactDigest],
        Instant,
    ) -> Result<
        BTreeMap<AnalysisArtifactDigest, RustGraphSiteAnalysis>,
        SqliteStoreError,
    >,
) -> Result<PreparedLocalArtifacts, LocalRustIndexError> {
    let requested_artifacts = requested_artifact_digests(
        &context.sources,
        context.identities,
        context.cancelled,
        context.deadline,
    )?;
    let reusable = load_reusable_artifacts(&requested_artifacts, context.deadline, load_reusable)?;
    let requested_graph_artifacts = requested_local_rust_graph_artifact_digests(
        &context.sources,
        context.graph_identity,
        context.cancelled,
        context.deadline,
    )
    .map_err(map_graph_preparation_error)?;
    let reusable_graph = load_reusable_graph(&requested_graph_artifacts, context.deadline)
        .map_err(|source| LocalRustIndexError::ArtifactReuse { source })?;
    let graph = prepare_local_rust_graph_artifacts(
        &context.sources,
        context.graph_identity,
        &reusable_graph,
        context.cancelled,
        context.deadline,
    )
    .map_err(map_graph_preparation_error)?;
    let source = prepare_source_index_with_reuse(
        context.sources,
        context.identities,
        context.limits.preparation(),
        &reusable,
        context.cancelled,
        context.deadline,
    )
    .map_err(|source| match source {
        RustIndexPreparationError::Cancelled => LocalRustIndexError::Cancelled,
        RustIndexPreparationError::DeadlineExceeded => LocalRustIndexError::DeadlineExceeded,
        source => LocalRustIndexError::Preparation { source },
    })?;
    Ok(PreparedLocalArtifacts { source, graph })
}

fn complete_local_preparation(
    completion: LocalPreparationCompletion<'_>,
) -> Result<LocalRustIndexPreparation, LocalRustIndexError> {
    let discovered_paths = completion.discovered.discovered_paths();
    let skipped_policy_paths = completion
        .skipped_policy_paths
        .checked_add(completion.discovered.policy_omitted_paths())
        .ok_or(LocalRustIndexError::SourceByteCountOverflowed)?;
    let classified_paths = completion
        .counts
        .total()?
        .checked_add(skipped_policy_paths)
        .and_then(|count| count.checked_add(completion.skipped_unsupported_paths))
        .ok_or(LocalRustIndexError::SourceByteCountOverflowed)?;
    if classified_paths != discovered_paths {
        return Err(LocalRustIndexError::SourceByteCountOverflowed);
    }
    Ok(LocalRustIndexPreparation {
        prepared: completion.prepared,
        graph_artifacts: completion.graph_artifacts,
        git_state: completion.git_state,
        worktree_state: completion.worktree_state,
        discovered_paths,
        selected_rust_files: completion.counts.rust,
        selected_go_files: completion.counts.go,
        selected_typescript_files: completion.counts.typescript,
        selected_tsx_files: completion.counts.tsx,
        selected_python_files: completion.counts.python,
        skipped_policy_paths,
        skipped_unsupported_paths: completion.skipped_unsupported_paths,
    })
}

fn prepare_local_index_with_exclusion_reuse_and_hook(
    context: LocalPreparationContext<'_>,
    mut load_reusable: impl FnMut(
        SourceLanguage,
        &[AnalysisArtifactDigest],
        Instant,
    ) -> Result<
        BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
        SqliteStoreError,
    >,
    mut load_reusable_graph: impl FnMut(
        &[AnalysisArtifactDigest],
        Instant,
    ) -> Result<
        BTreeMap<AnalysisArtifactDigest, RustGraphSiteAnalysis>,
        SqliteStoreError,
    >,
    mut before_revalidation: impl FnMut(),
) -> Result<LocalRustIndexPreparation, LocalRustIndexError> {
    let LocalPreparationContext {
        requested_root,
        identities,
        graph_identity,
        selection,
        package_scope,
        limits,
        cancelled,
        excluded_identity,
    } = context;
    if limits.deadline().is_zero() {
        return Err(LocalRustIndexError::DeadlineExceeded);
    }
    let deadline = Instant::now()
        .checked_add(limits.deadline())
        .ok_or(LocalRustIndexError::DeadlineNotRepresentable)?;
    check_control(cancelled, deadline)?;
    let worktree_root = discovered_worktree_root(requested_root)
        .map_err(|source| LocalRustIndexError::Discovery { source })?;
    check_control(cancelled, deadline)?;
    let source_state_before =
        capture_source_state_for_index(&worktree_root, limits.discovery(), cancelled, deadline)?;

    let discovered = select_discovered_paths(
        discover_paths(&worktree_root, limits.discovery(), cancelled, deadline)?,
        package_scope,
        cancelled,
        deadline,
    )?;
    check_control(cancelled, deadline)?;
    let root = ContainedSourceRoot::open(&worktree_root)
        .map_err(|source| LocalRustIndexError::RootOpen { source })?;
    reject_excluded_file_aliases(
        &root,
        &discovered,
        excluded_identity,
        limits,
        cancelled,
        deadline,
    )?;
    let selected =
        read_selected_sources(&root, &discovered, selection, limits, cancelled, deadline)?;
    let artifacts = prepare_selected_source_artifacts(
        LocalArtifactPreparationContext {
            sources: selected.sources,
            identities,
            graph_identity,
            limits,
            cancelled,
            deadline,
        },
        &mut load_reusable,
        &mut load_reusable_graph,
    )?;
    let prepared = artifacts.source;
    before_revalidation();
    revalidate_path_set(
        &worktree_root,
        &discovered,
        package_scope,
        limits.discovery(),
        cancelled,
        deadline,
    )?;
    revalidate_content(&root, &prepared, limits.source_read(), cancelled, deadline)?;
    check_control(cancelled, deadline)?;
    let source_state_after =
        recapture_source_state_for_index(&worktree_root, limits.discovery(), cancelled, deadline)?;
    let (git_state, worktree_state) = validated_final_source_identity(
        &source_state_before,
        &source_state_after,
        selection,
        prepared.manifest_digest(),
    )?;

    complete_local_preparation(LocalPreparationCompletion {
        prepared,
        graph_artifacts: artifacts.graph,
        git_state,
        worktree_state,
        discovered: &discovered,
        counts: selected.counts,
        skipped_policy_paths: selected.skipped_policy_paths,
        skipped_unsupported_paths: selected.skipped_unsupported_paths,
    })
}

include!("rust_index/source_io.rs");

#[cfg(test)]
mod tests;
