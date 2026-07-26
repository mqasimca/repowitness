use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use repowitness_analysis::RustSourceAnalysis;
use repowitness_application::{
    ImmutableRustSource, PreparedRustIndex, RustArtifactIdentity, RustIndexLimits,
    RustIndexPreparationError, hash_analysis_artifact_key, hash_source_content,
    prepare_rust_index_with_reuse,
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

/// Default wall-clock deadline for complete local Rust index preparation.
pub const DEFAULT_LOCAL_RUST_INDEX_DEADLINE: Duration = Duration::from_secs(30);

/// All stage-specific and end-to-end limits for local Rust preparation.
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

/// A fully prepared local Rust index plus explicit discovery coverage.
pub struct LocalRustIndexPreparation {
    prepared: PreparedRustIndex,
    git_state: GitStateDigest,
    worktree_state: WorktreeStateDigest,
    discovered_paths: u64,
    selected_rust_files: u64,
    skipped_non_rust_paths: u64,
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

    /// Returns paths explicitly skipped by the Phase 0 Rust-only adapter.
    #[must_use]
    pub const fn skipped_non_rust_paths(&self) -> u64 {
        self.skipped_non_rust_paths
    }
}

impl fmt::Debug for LocalRustIndexPreparation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalRustIndexPreparation")
            .field("prepared", &self.prepared)
            .field("git_state", &self.git_state)
            .field("worktree_state", &self.worktree_state)
            .field("discovered_paths", &self.discovered_paths)
            .field("selected_rust_files", &self.selected_rust_files)
            .field("skipped_non_rust_paths", &self.skipped_non_rust_paths)
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
    /// A selected Rust source could not be opened or read.
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
    /// Persisted artifact inventory failed validation or bounded loading.
    ArtifactReuse {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
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
                formatter.write_str("local Rust index deadline is not representable")
            }
            Self::Cancelled => formatter.write_str("local Rust index preparation was cancelled"),
            Self::DeadlineExceeded => {
                formatter.write_str("local Rust index preparation exceeded its deadline")
            }
            Self::Discovery { .. } => formatter.write_str("repository path discovery failed"),
            Self::SourceState { .. } => {
                formatter.write_str("repository source-state capture failed")
            }
            Self::RootOpen { .. } => {
                formatter.write_str("repository source capability could not be opened")
            }
            Self::SourceRead { ordinal, .. } => {
                write!(formatter, "Rust source ordinal {ordinal} could not be read")
            }
            Self::ExcludedFileAlias => {
                formatter.write_str("repository path aliases an excluded external file")
            }
            Self::SourceByteCountOverflowed => {
                formatter.write_str("selected Rust source byte count overflowed")
            }
            Self::SourceByteLimitExceeded { limit } => {
                write!(
                    formatter,
                    "selected Rust source bytes exceed the limit of {limit}"
                )
            }
            Self::DerivedReadLimits { .. } => {
                formatter.write_str("remaining source-read limits could not be represented")
            }
            Self::Preparation { .. } => formatter.write_str("Rust index preparation failed"),
            Self::ArtifactReuse { .. } => {
                formatter.write_str("reusable Rust artifact loading failed")
            }
            Self::StalePathSet => {
                formatter.write_str("repository path set changed during preparation")
            }
            Self::StaleSourceContent { ordinal } => {
                write!(
                    formatter,
                    "Rust source ordinal {ordinal} changed during preparation"
                )
            }
            Self::RevalidationRead { ordinal, .. } => {
                write!(
                    formatter,
                    "Rust source ordinal {ordinal} could not be revalidated"
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
            Self::ArtifactReuse { source } => Some(source),
            Self::RevalidationRead { source, .. } => Some(source),
            Self::DeadlineNotRepresentable
            | Self::Cancelled
            | Self::DeadlineExceeded
            | Self::ExcludedFileAlias
            | Self::SourceByteCountOverflowed
            | Self::SourceByteLimitExceeded { .. }
            | Self::StalePathSet
            | Self::StaleSourceContent { .. } => None,
        }
    }
}

/// Runs the Phase 0 local Rust discovery-to-facts vertical slice.
pub fn prepare_local_rust_index(
    requested_root: &Path,
    identity: RustArtifactIdentity,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
) -> Result<LocalRustIndexPreparation, LocalRustIndexError> {
    prepare_local_rust_index_with_hook(requested_root, identity, limits, cancelled, || {})
}

pub(crate) fn prepare_local_rust_index_excluding_identity_with_reuse(
    requested_root: &Path,
    identity: RustArtifactIdentity,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
    excluded_identity: Option<&FileIdentity>,
    load_reusable: impl FnMut(
        &[AnalysisArtifactDigest],
        Instant,
    ) -> Result<
        BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
        SqliteStoreError,
    >,
) -> Result<LocalRustIndexPreparation, LocalRustIndexError> {
    prepare_local_rust_index_with_exclusion_reuse_and_hook(
        requested_root,
        identity,
        limits,
        cancelled,
        excluded_identity,
        load_reusable,
        || {},
    )
}

fn prepare_local_rust_index_with_hook(
    requested_root: &Path,
    identity: RustArtifactIdentity,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
    before_revalidation: impl FnMut(),
) -> Result<LocalRustIndexPreparation, LocalRustIndexError> {
    prepare_local_rust_index_with_exclusion_and_hook(
        requested_root,
        identity,
        limits,
        cancelled,
        None,
        before_revalidation,
    )
}

fn prepare_local_rust_index_with_exclusion_and_hook(
    requested_root: &Path,
    identity: RustArtifactIdentity,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
    excluded_identity: Option<&FileIdentity>,
    before_revalidation: impl FnMut(),
) -> Result<LocalRustIndexPreparation, LocalRustIndexError> {
    prepare_local_rust_index_with_exclusion_reuse_and_hook(
        requested_root,
        identity,
        limits,
        cancelled,
        excluded_identity,
        |_, _| Ok(BTreeMap::new()),
        before_revalidation,
    )
}

fn prepare_local_rust_index_with_exclusion_reuse_and_hook(
    requested_root: &Path,
    identity: RustArtifactIdentity,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
    excluded_identity: Option<&FileIdentity>,
    mut load_reusable: impl FnMut(
        &[AnalysisArtifactDigest],
        Instant,
    ) -> Result<
        BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
        SqliteStoreError,
    >,
    mut before_revalidation: impl FnMut(),
) -> Result<LocalRustIndexPreparation, LocalRustIndexError> {
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

    let discovered = discover_paths(&worktree_root, limits.discovery(), cancelled, deadline)?;
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
    let selected = read_selected_rust_sources(&root, &discovered, limits, cancelled, deadline)?;
    let requested_artifacts =
        requested_artifact_digests(&selected.sources, identity, cancelled, deadline)?;
    let reusable =
        load_reusable(&requested_artifacts, deadline).map_err(|source| match source {
            SqliteStoreError::Cancelled => LocalRustIndexError::Cancelled,
            SqliteStoreError::DeadlineExceeded | SqliteStoreError::ReplyTimeout => {
                LocalRustIndexError::DeadlineExceeded
            }
            source => LocalRustIndexError::ArtifactReuse { source },
        })?;
    let prepared = prepare_rust_index_with_reuse(
        selected.sources,
        identity,
        limits.preparation(),
        &reusable,
        cancelled,
        deadline,
    )
    .map_err(|source| match source {
        RustIndexPreparationError::Cancelled => LocalRustIndexError::Cancelled,
        RustIndexPreparationError::DeadlineExceeded => LocalRustIndexError::DeadlineExceeded,
        source => LocalRustIndexError::Preparation { source },
    })?;
    before_revalidation();
    revalidate_path_set(
        &worktree_root,
        &discovered,
        limits.discovery(),
        cancelled,
        deadline,
    )?;
    revalidate_content(&root, &prepared, limits.source_read(), cancelled, deadline)?;
    check_control(cancelled, deadline)?;
    let source_state_after =
        capture_source_state_for_index(&worktree_root, limits.discovery(), cancelled, deadline)?;
    if source_state_after != source_state_before {
        return Err(LocalRustIndexError::SourceState {
            source: SourceStateError::ConcurrentSourceChange,
        });
    }
    let git_state = source_state_after.git_state();
    let worktree_state = source_state_after.worktree_state(prepared.manifest_digest());

    let discovered_paths = discovered.stats().path_count();
    let skipped_non_rust_paths = discovered_paths
        .checked_sub(selected.count)
        .ok_or(LocalRustIndexError::SourceByteCountOverflowed)?;
    Ok(LocalRustIndexPreparation {
        prepared,
        git_state,
        worktree_state,
        discovered_paths,
        selected_rust_files: selected.count,
        skipped_non_rust_paths,
    })
}

fn requested_artifact_digests(
    sources: &[ImmutableRustSource],
    identity: RustArtifactIdentity,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Box<[AnalysisArtifactDigest]>, LocalRustIndexError> {
    let mut requested = BTreeSet::new();
    for source in sources {
        check_control(cancelled, deadline)?;
        let key = AnalysisArtifactKey::new(
            hash_source_content(source.content()),
            identity.producer_manifest(),
            identity.configuration(),
            identity.schema(),
            identity.canonicalization_version(),
        );
        requested.insert(hash_analysis_artifact_key(&key));
    }
    Ok(requested.into_iter().collect())
}

fn reject_excluded_file_aliases(
    root: &ContainedSourceRoot,
    discovered: &DiscoveredRepositoryPaths,
    excluded_identity: Option<&FileIdentity>,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalRustIndexError> {
    let Some(excluded_identity) = excluded_identity else {
        return Ok(());
    };
    for path in discovered.paths() {
        check_control(cancelled, deadline)?;
        let aliases = root.aliases_identity(
            path,
            excluded_identity,
            limits.deadline(),
            deadline,
            &mut || cancelled.load(Ordering::Relaxed),
        );
        match aliases {
            Ok(true) => return Err(LocalRustIndexError::ExcludedFileAlias),
            Ok(false) => {}
            Err(ContainedSourceError::Cancelled) => {
                return Err(LocalRustIndexError::Cancelled);
            }
            Err(ContainedSourceError::DeadlineExceeded { .. }) => {
                return Err(LocalRustIndexError::DeadlineExceeded);
            }
            Err(_) => {}
        }
    }
    Ok(())
}

fn capture_source_state_for_index(
    worktree_root: &Path,
    limits: GitPathDiscoveryLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<crate::CapturedSourceState, LocalRustIndexError> {
    let limits = capped_discovery_limits(limits, deadline)?;
    capture_source_state_with_cancel(worktree_root, limits, || cancelled.load(Ordering::Relaxed))
        .map_err(|source| match source {
            SourceStateError::Git {
                source: GitPathDiscoveryError::Cancelled,
            } => LocalRustIndexError::Cancelled,
            SourceStateError::Git {
                source: GitPathDiscoveryError::DeadlineExceeded { .. },
            } => LocalRustIndexError::DeadlineExceeded,
            source => LocalRustIndexError::SourceState { source },
        })
}

struct SelectedRustSources {
    sources: Vec<ImmutableRustSource>,
    count: u64,
}

fn read_selected_rust_sources(
    root: &ContainedSourceRoot,
    discovered: &DiscoveredRepositoryPaths,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<SelectedRustSources, LocalRustIndexError> {
    let rust_paths = discovered
        .paths()
        .iter()
        .filter(|path| is_rust_source_path(path.as_bytes()))
        .collect::<Vec<_>>();
    let count = u64::try_from(rust_paths.len())
        .map_err(|_| LocalRustIndexError::SourceByteCountOverflowed)?;
    if count > limits.preparation().max_files() {
        return Err(LocalRustIndexError::Preparation {
            source: RustIndexPreparationError::FileLimitExceeded {
                limit: limits.preparation().max_files(),
            },
        });
    }

    let mut sources = Vec::with_capacity(rust_paths.len());
    let mut total_source_bytes = 0_u64;
    for (index, path) in rust_paths.into_iter().enumerate() {
        check_control(cancelled, deadline)?;
        let ordinal = stable_ordinal(index)?;
        let read_limits = capped_source_read_limits(limits.source_read(), deadline)?;
        let content = read_source(root, path, read_limits, cancelled, ordinal)?;
        total_source_bytes = total_source_bytes
            .checked_add(
                u64::try_from(content.len())
                    .map_err(|_| LocalRustIndexError::SourceByteCountOverflowed)?,
            )
            .ok_or(LocalRustIndexError::SourceByteCountOverflowed)?;
        if total_source_bytes > limits.preparation().max_total_source_bytes() {
            return Err(LocalRustIndexError::SourceByteLimitExceeded {
                limit: limits.preparation().max_total_source_bytes(),
            });
        }
        sources.push(ImmutableRustSource::new(path.clone(), content));
    }
    Ok(SelectedRustSources { sources, count })
}

fn read_source(
    root: &ContainedSourceRoot,
    path: &repowitness_domain::RepositoryPath,
    limits: SourceReadLimits,
    cancelled: &AtomicBool,
    ordinal: u64,
) -> Result<Box<[u8]>, LocalRustIndexError> {
    root.read_with_cancel(path, limits, || cancelled.load(Ordering::Relaxed))
        .map_err(|source| match source {
            ContainedSourceError::Cancelled => LocalRustIndexError::Cancelled,
            ContainedSourceError::DeadlineExceeded { .. } => LocalRustIndexError::DeadlineExceeded,
            source => LocalRustIndexError::SourceRead { ordinal, source },
        })
}

fn discover_paths(
    worktree_root: &Path,
    limits: GitPathDiscoveryLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<DiscoveredRepositoryPaths, LocalRustIndexError> {
    let limits = capped_discovery_limits(limits, deadline)?;
    discover_repository_paths_with_cancel(worktree_root, limits, || {
        cancelled.load(Ordering::Relaxed)
    })
    .map_err(|source| match source {
        GitPathDiscoveryError::Cancelled => LocalRustIndexError::Cancelled,
        GitPathDiscoveryError::DeadlineExceeded { .. } => LocalRustIndexError::DeadlineExceeded,
        source => LocalRustIndexError::Discovery { source },
    })
}

fn revalidate_path_set(
    worktree_root: &Path,
    original: &DiscoveredRepositoryPaths,
    discovery_limits: GitPathDiscoveryLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalRustIndexError> {
    let current = discover_paths(worktree_root, discovery_limits, cancelled, deadline)?;
    if current.paths() != original.paths() {
        return Err(LocalRustIndexError::StalePathSet);
    }
    Ok(())
}

fn revalidate_content(
    root: &ContainedSourceRoot,
    prepared: &PreparedRustIndex,
    source_limits: SourceReadLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalRustIndexError> {
    for (index, file) in prepared.files().iter().enumerate() {
        check_control(cancelled, deadline)?;
        let ordinal = stable_ordinal(index)?;
        let read_limits = capped_source_read_limits(source_limits, deadline)?;
        let content = root
            .read_with_cancel(file.path(), read_limits, || {
                cancelled.load(Ordering::Relaxed)
            })
            .map_err(|source| match source {
                ContainedSourceError::Cancelled => LocalRustIndexError::Cancelled,
                ContainedSourceError::DeadlineExceeded { .. } => {
                    LocalRustIndexError::DeadlineExceeded
                }
                source => LocalRustIndexError::RevalidationRead { ordinal, source },
            })?;
        if hash_source_content(&content) != file.content_digest() {
            return Err(LocalRustIndexError::StaleSourceContent { ordinal });
        }
    }
    Ok(())
}

fn capped_discovery_limits(
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
) -> Result<GitPathDiscoveryLimits, LocalRustIndexError> {
    let remaining = remaining(deadline)?;
    Ok(GitPathDiscoveryLimits::new(
        limits.deadline().min(remaining),
        limits.output_bytes(),
        limits.paths(),
        limits.repository_path(),
    ))
}

fn capped_source_read_limits(
    limits: SourceReadLimits,
    deadline: Instant,
) -> Result<SourceReadLimits, LocalRustIndexError> {
    let remaining = remaining(deadline)?;
    SourceReadLimits::try_new(
        limits.deadline().min(remaining),
        limits.file_bytes(),
        limits.read_chunk_bytes(),
    )
    .map_err(|source| LocalRustIndexError::DerivedReadLimits { source })
}

fn remaining(deadline: Instant) -> Result<Duration, LocalRustIndexError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(LocalRustIndexError::DeadlineExceeded);
    }
    Ok(remaining)
}

fn stable_ordinal(index: usize) -> Result<u64, LocalRustIndexError> {
    u64::try_from(index)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or(LocalRustIndexError::SourceByteCountOverflowed)
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), LocalRustIndexError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(LocalRustIndexError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(LocalRustIndexError::DeadlineExceeded);
    }
    Ok(())
}

fn is_rust_source_path(path: &[u8]) -> bool {
    path.ends_with(b".rs")
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

    use repowitness_analysis::RustAnalysisLimits;
    use repowitness_domain::{AnalysisSchemaDigest, ConfigurationDigest, ProducerManifestDigest};

    use super::*;

    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    struct TempRepository {
        root: PathBuf,
    }

    impl TempRepository {
        fn new() -> Self {
            let fixture_id = NEXT_FIXTURE_ID.fetch_add(1, AtomicOrdering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "repowitness-local-rust-index-{}-{fixture_id}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("fixture directory must be created");
            let repository = Self { root };
            repository.git(&["init", "--quiet", "--initial-branch=main"]);
            repository
        }

        fn root(&self) -> &Path {
            &self.root
        }

        fn write(&self, relative: &str, content: &[u8]) {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("fixture parent must be created");
            }
            fs::write(path, content).expect("fixture source must be written");
        }

        fn git(&self, arguments: &[&str]) {
            let status = Command::new("git")
                .arg("--no-pager")
                .arg("-C")
                .arg(&self.root)
                .args(arguments)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", null_device())
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GCM_INTERACTIVE", "never")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("fixture Git command must start");
            assert!(status.success(), "fixture Git command failed: {status}");
        }

        fn commit_all(&self, message: &str) {
            self.git(&["add", "--all"]);
            self.git(&[
                "-c",
                "user.name=RepoWitness Test",
                "-c",
                "user.email=repowitness@example.invalid",
                "commit",
                "--quiet",
                "-m",
                message,
            ]);
        }

        fn commit_empty(&self, message: &str) {
            self.git(&[
                "-c",
                "user.name=RepoWitness Test",
                "-c",
                "user.email=repowitness@example.invalid",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                message,
            ]);
        }
    }

    impl Drop for TempRepository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn null_device() -> OsString {
        if cfg!(windows) {
            OsString::from("NUL")
        } else {
            OsString::from("/dev/null")
        }
    }

    fn identity() -> RustArtifactIdentity {
        RustArtifactIdentity::new(
            ProducerManifestDigest::new([1; 32]),
            ConfigurationDigest::new([2; 32]),
            AnalysisSchemaDigest::new([3; 32]),
            1,
        )
    }

    #[test]
    fn local_vertical_slice_discovers_reads_analyzes_and_revalidates() {
        let repository = TempRepository::new();
        repository.write("src/lib.rs", b"pub struct Visible;\n");
        repository.write("README.txt", b"not Rust\n");
        repository.write("upper.RS", b"fn upper() {}\n");
        let cancelled = AtomicBool::new(false);

        let prepared = prepare_local_rust_index(
            repository.root(),
            identity(),
            LocalRustIndexLimits::default(),
            &cancelled,
        )
        .expect("stable fixture repository must prepare");

        assert_eq!(prepared.discovered_paths(), 3);
        assert_eq!(prepared.selected_rust_files(), 1);
        assert_eq!(prepared.skipped_non_rust_paths(), 2);
        assert_eq!(prepared.prepared().files().len(), 1);
        assert_eq!(prepared.prepared().total_facts(), 1);
        assert_eq!(
            prepared.prepared().files()[0].path().as_bytes(),
            b"src/lib.rs"
        );

        let repeated = prepare_local_rust_index(
            repository.root(),
            identity(),
            LocalRustIndexLimits::default(),
            &cancelled,
        )
        .expect("unchanged fixture repository must prepare identically");
        assert_eq!(repeated.git_state(), prepared.git_state());
        assert_eq!(repeated.worktree_state(), prepared.worktree_state());

        repository.write("src/lib.rs", b"pub struct Changed;\n");
        let changed = prepare_local_rust_index(
            repository.root(),
            identity(),
            LocalRustIndexLimits::default(),
            &cancelled,
        )
        .expect("new stable source state must prepare");
        assert_eq!(changed.git_state(), prepared.git_state());
        assert_ne!(changed.worktree_state(), prepared.worktree_state());
    }

    #[test]
    fn aggregate_limits_cancellation_and_deadline_fail_closed() {
        let repository = TempRepository::new();
        repository.write("a.rs", b"fn a() {}\n");
        let cancelled = AtomicBool::new(true);
        assert!(matches!(
            prepare_local_rust_index(
                repository.root(),
                identity(),
                LocalRustIndexLimits::default(),
                &cancelled,
            ),
            Err(LocalRustIndexError::Cancelled)
        ));

        let not_cancelled = AtomicBool::new(false);
        let zero_deadline = LocalRustIndexLimits::new(
            Duration::ZERO,
            GitPathDiscoveryLimits::default(),
            SourceReadLimits::default(),
            RustIndexLimits::default(),
        );
        assert!(matches!(
            prepare_local_rust_index(repository.root(), identity(), zero_deadline, &not_cancelled,),
            Err(LocalRustIndexError::DeadlineExceeded)
        ));

        let byte_limited = RustIndexLimits::try_new(10, 1, 100, RustAnalysisLimits::default())
            .expect("fixture aggregate limits must be valid");
        let limits = LocalRustIndexLimits::new(
            Duration::from_secs(5),
            GitPathDiscoveryLimits::default(),
            SourceReadLimits::default(),
            byte_limited,
        );
        assert!(matches!(
            prepare_local_rust_index(repository.root(), identity(), limits, &not_cancelled,),
            Err(LocalRustIndexError::SourceByteLimitExceeded { limit: 1 })
        ));
    }

    #[test]
    fn path_and_content_mutation_are_rejected_by_final_revalidation() {
        let path_repository = TempRepository::new();
        path_repository.write("stable.rs", b"fn stable() {}\n");
        let cancelled = AtomicBool::new(false);
        let path_error = prepare_local_rust_index_with_hook(
            path_repository.root(),
            identity(),
            LocalRustIndexLimits::default(),
            &cancelled,
            || path_repository.write("added.rs", b"fn added() {}\n"),
        )
        .expect_err("a changed path set must fail revalidation");
        assert!(matches!(path_error, LocalRustIndexError::StalePathSet));

        let content_repository = TempRepository::new();
        content_repository.write("stable.rs", b"fn before() {}\n");
        let content_error = prepare_local_rust_index_with_hook(
            content_repository.root(),
            identity(),
            LocalRustIndexLimits::default(),
            &cancelled,
            || content_repository.write("stable.rs", b"fn after() {}\n"),
        )
        .expect_err("changed source bytes must fail revalidation");
        assert!(matches!(
            content_error,
            LocalRustIndexError::StaleSourceContent { ordinal: 1 }
        ));
    }

    #[test]
    fn index_status_and_head_mutations_are_rejected_by_the_source_state_fence() {
        let cancelled = AtomicBool::new(false);

        let index_repository = TempRepository::new();
        index_repository.write("stable.rs", b"fn stable() {}\n");
        let index_error = prepare_local_rust_index_with_hook(
            index_repository.root(),
            identity(),
            LocalRustIndexLimits::default(),
            &cancelled,
            || index_repository.git(&["add", "stable.rs"]),
        )
        .expect_err("an index mutation must fail the source-state fence");
        assert!(matches!(
            index_error,
            LocalRustIndexError::SourceState {
                source: SourceStateError::ConcurrentSourceChange
            }
        ));

        let status_repository = TempRepository::new();
        status_repository.write("stable.rs", b"fn stable() {}\n");
        status_repository.write("README.md", b"before\n");
        status_repository.commit_all("initial");
        let status_error = prepare_local_rust_index_with_hook(
            status_repository.root(),
            identity(),
            LocalRustIndexLimits::default(),
            &cancelled,
            || status_repository.write("README.md", b"after\n"),
        )
        .expect_err("a tracked non-Rust status mutation must fail the source-state fence");
        assert!(matches!(
            status_error,
            LocalRustIndexError::SourceState {
                source: SourceStateError::ConcurrentSourceChange
            }
        ));

        let head_repository = TempRepository::new();
        head_repository.write("stable.rs", b"fn stable() {}\n");
        head_repository.commit_all("initial");
        let head_error = prepare_local_rust_index_with_hook(
            head_repository.root(),
            identity(),
            LocalRustIndexLimits::default(),
            &cancelled,
            || head_repository.commit_empty("move head"),
        )
        .expect_err("a HEAD mutation must fail the source-state fence");
        assert!(matches!(
            head_error,
            LocalRustIndexError::SourceState {
                source: SourceStateError::ConcurrentSourceChange
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn selected_symlink_sources_are_rejected_without_leaking_targets() {
        use std::os::unix::fs::symlink;

        let repository = TempRepository::new();
        let outside = repository
            .root()
            .parent()
            .expect("fixture has a parent")
            .join(format!(
                "repowitness-private-target-{}",
                NEXT_FIXTURE_ID.fetch_add(1, AtomicOrdering::Relaxed)
            ));
        fs::write(&outside, b"fn private_target() {}\n").expect("outside fixture must be written");
        symlink(&outside, repository.root().join("linked.rs"))
            .expect("source symlink must be created");
        let cancelled = AtomicBool::new(false);

        let error = prepare_local_rust_index(
            repository.root(),
            identity(),
            LocalRustIndexLimits::default(),
            &cancelled,
        )
        .expect_err("source symlink must fail closed");
        let _ = fs::remove_file(&outside);

        assert!(matches!(
            error,
            LocalRustIndexError::SourceRead { ordinal: 1, .. }
        ));
        assert!(!error.to_string().contains("private-target"));
        assert!(!format!("{error:?}").contains("private-target"));
    }

    #[test]
    fn rust_path_filter_is_case_sensitive_and_byte_based() {
        assert!(is_rust_source_path(b"src/lib.rs"));
        assert!(!is_rust_source_path(b"src/lib.RS"));
        assert!(!is_rust_source_path(b"src/rs"));
        assert!(is_rust_source_path(b"non-utf8-\xFF.rs"));
    }
}
