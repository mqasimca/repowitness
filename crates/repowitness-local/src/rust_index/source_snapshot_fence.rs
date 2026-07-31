use std::{
    error::Error,
    fmt,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use repowitness_application::{
    ImmutableRustSource, PackageScope, RustSourceSnapshotIdentity, hash_source_content,
    hash_source_manifest, hash_source_snapshot,
};
use repowitness_domain::{
    RepositoryPath, SourceContentDigest, SourceFileKind, SourceFileLimit, SourceManifest,
    SourceManifestEntry, SourceSnapshotDigest,
};

use crate::{
    contained_source::{
        ContainedSourceError, ContainedSourceRoot, ExactReadSessionError, FileIdentity,
    },
    git_paths::discovered_worktree_root,
    source_state::SourceStateError,
};

use super::{
    LocalRustIndexError, LocalRustIndexLimits, SelectionPolicy, SourceLanguageSelection,
    capped_source_read_limits, capture_source_state_for_index, check_control, discover_paths,
    read_selected_sources, recapture_source_state_for_index, reject_excluded_file_aliases,
    revalidate_path_set, select_discovered_paths,
};

/// Inputs for one authoritative, parser-free local source-snapshot fence.
///
/// The absolute deadline and cancellation signal are retained from the
/// publication attempt. The optional excluded identity is the already-opened
/// database identity that no discovered repository path may alias.
#[must_use = "a source-snapshot fence request must be confirmed or deliberately discarded"]
pub(crate) struct LocalSourceSnapshotFenceRequest<'a> {
    requested_root: &'a Path,
    identity: RustSourceSnapshotIdentity,
    expected_snapshot: SourceSnapshotDigest,
    languages: SourceLanguageSelection,
    package_scope: Option<&'a PackageScope>,
    limits: LocalRustIndexLimits,
    cancelled: &'a AtomicBool,
    deadline: Instant,
    excluded_identity: Option<&'a FileIdentity>,
}

/// Complete immutable source bytes captured by one confirmed local fence.
///
/// The values are safe to pass to analysis adapters because every byte has
/// already been admitted below the contained source root and revalidated
/// against the exact source-snapshot identity.
pub(crate) struct ConfirmedLocalSourceSnapshot {
    sources: Box<[ImmutableRustSource]>,
    manifest: SourceManifest<RepositoryPath, SourceFileKind, SourceContentDigest>,
}

impl ConfirmedLocalSourceSnapshot {
    /// Returns the exact captured source bytes in deterministic path order.
    pub(crate) fn sources(&self) -> &[ImmutableRustSource] {
        &self.sources
    }

    /// Returns the source manifest proven by the same final fence.
    pub(crate) const fn manifest(
        &self,
    ) -> &SourceManifest<RepositoryPath, SourceFileKind, SourceContentDigest> {
        &self.manifest
    }
}

impl<'a> LocalSourceSnapshotFenceRequest<'a> {
    #[allow(
        clippy::too_many_arguments,
        reason = "root, snapshot, policy, control, and alias authority remain explicit"
    )]
    pub(crate) const fn new(
        requested_root: &'a Path,
        identity: RustSourceSnapshotIdentity,
        expected_snapshot: SourceSnapshotDigest,
        languages: SourceLanguageSelection,
        limits: LocalRustIndexLimits,
        cancelled: &'a AtomicBool,
        deadline: Instant,
        excluded_identity: Option<&'a FileIdentity>,
    ) -> Self {
        Self {
            requested_root,
            identity,
            expected_snapshot,
            languages,
            package_scope: None,
            limits,
            cancelled,
            deadline,
            excluded_identity,
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "scoped root, snapshot, policy, control, and alias authority remain explicit"
    )]
    pub(crate) const fn new_scoped(
        requested_root: &'a Path,
        identity: RustSourceSnapshotIdentity,
        expected_snapshot: SourceSnapshotDigest,
        languages: SourceLanguageSelection,
        package_scope: &'a PackageScope,
        limits: LocalRustIndexLimits,
        cancelled: &'a AtomicBool,
        deadline: Instant,
        excluded_identity: Option<&'a FileIdentity>,
    ) -> Self {
        Self {
            requested_root,
            identity,
            expected_snapshot,
            languages,
            package_scope: Some(package_scope),
            limits,
            cancelled,
            deadline,
            excluded_identity,
        }
    }
}

impl fmt::Debug for LocalSourceSnapshotFenceRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalSourceSnapshotFenceRequest")
            .field("requested_root", &"<redacted-path>")
            .field("identity", &"<redacted-identity>")
            .field("expected_snapshot", &"<redacted-digest>")
            .field("languages", &"<selected-languages>")
            .field("package_scope", &self.package_scope)
            .field("limits", &self.limits)
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .field("excluded_identity", &self.excluded_identity.is_some())
            .finish()
    }
}

/// Stable redacted failure from an authoritative local source-snapshot fence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalSourceSnapshotFenceError {
    /// The shared publication operation was cancelled.
    Cancelled,
    /// The shared absolute publication deadline elapsed.
    DeadlineExceeded,
    /// The current source state is outside the supported indexing contract.
    UnsupportedSourceState,
    /// A discovered path aliases the excluded database identity.
    ExcludedFileAlias,
    /// Bounded source recapture could not complete safely.
    CaptureFailed,
    /// Source paths, bytes, Git state, or worktree state no longer match.
    SourceChanged,
}

impl fmt::Display for LocalSourceSnapshotFenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Cancelled => "local source-snapshot fence was cancelled",
            Self::DeadlineExceeded => "local source-snapshot fence exceeded its deadline",
            Self::UnsupportedSourceState => "local source state is unsupported",
            Self::ExcludedFileAlias => "repository source aliases an excluded file",
            Self::CaptureFailed => "local source snapshot could not be recaptured",
            Self::SourceChanged => "local source snapshot changed before completion",
        })
    }
}

impl Error for LocalSourceSnapshotFenceError {}

/// Recaptures and confirms the complete selected local source snapshot.
///
/// This deliberately performs no parsing or graph analysis. It repeats
/// contained path discovery, exact content reads, canonical manifest hashing,
/// excluded-file alias checks, and the complete Git/worktree receipt under the
/// caller's original absolute deadline. Every mismatch fails closed.
pub(crate) fn confirm_local_source_snapshot(
    request: LocalSourceSnapshotFenceRequest<'_>,
) -> Result<(), LocalSourceSnapshotFenceError> {
    capture_confirmed_local_source_snapshot(request).map(|_| ())
}

/// Captures source bytes and confirms they still form the requested snapshot.
///
/// This is the source-fence variant for consumers, such as the Phase 2 SCIP
/// importer, which must validate hostile claims against exactly the bytes that
/// were revalidated at the final fence. It performs no parsing or analysis.
pub(crate) fn capture_confirmed_local_source_snapshot(
    request: LocalSourceSnapshotFenceRequest<'_>,
) -> Result<ConfirmedLocalSourceSnapshot, LocalSourceSnapshotFenceError> {
    check_control(request.cancelled, request.deadline).map_err(map_local_error)?;
    let worktree_root =
        discovered_worktree_root(request.requested_root).map_err(|_| fence_capture_failed())?;
    check_control(request.cancelled, request.deadline).map_err(map_local_error)?;

    let source_state_before = capture_source_state_for_index(
        &worktree_root,
        request.limits.discovery(),
        request.cancelled,
        request.deadline,
    )
    .map_err(map_local_error)?;
    if source_state_before.git_state() != request.identity.git_state() {
        return Err(LocalSourceSnapshotFenceError::SourceChanged);
    }

    let discovered = select_discovered_paths(
        discover_paths(
            &worktree_root,
            request.limits.discovery(),
            request.cancelled,
            request.deadline,
        )
        .map_err(map_local_error)?,
        request.package_scope,
        request.cancelled,
        request.deadline,
    )
    .map_err(map_local_error)?;
    let root = ContainedSourceRoot::open(&worktree_root).map_err(|_| fence_capture_failed())?;
    reject_excluded_file_aliases(
        &root,
        &discovered,
        request.excluded_identity,
        request.limits,
        request.cancelled,
        request.deadline,
    )
    .map_err(map_local_error)?;
    let selected = read_selected_sources(
        &root,
        &discovered,
        SelectionPolicy::SupportedLanguages(request.languages),
        request.limits,
        request.cancelled,
        request.deadline,
    )
    .map_err(map_local_error)?;
    let manifest = source_manifest(
        &selected.sources,
        request.limits,
        request.cancelled,
        request.deadline,
    )?;
    let manifest_digest = hash_source_manifest(&manifest);

    revalidate_path_set(
        &worktree_root,
        &discovered,
        request.package_scope,
        request.limits.discovery(),
        request.cancelled,
        request.deadline,
    )
    .map_err(map_local_error)?;
    revalidate_selected_content(
        &root,
        &selected.sources,
        request.limits,
        request.cancelled,
        request.deadline,
    )?;
    check_control(request.cancelled, request.deadline).map_err(map_local_error)?;

    let source_state_after = recapture_source_state_for_index(
        &worktree_root,
        request.limits.discovery(),
        request.cancelled,
        request.deadline,
    )
    .map_err(map_local_error)?;
    if source_state_after != source_state_before
        || source_state_after.git_state() != request.identity.git_state()
        || source_state_after.source_worktree_state(manifest_digest)
            != request.identity.worktree_state()
        || hash_source_snapshot(request.identity, manifest_digest) != request.expected_snapshot
    {
        return Err(LocalSourceSnapshotFenceError::SourceChanged);
    }
    Ok(ConfirmedLocalSourceSnapshot {
        sources: selected.sources.into_boxed_slice(),
        manifest,
    })
}

fn source_manifest(
    sources: &[ImmutableRustSource],
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<
    SourceManifest<RepositoryPath, SourceFileKind, SourceContentDigest>,
    LocalSourceSnapshotFenceError,
> {
    let mut entries = Vec::with_capacity(sources.len());
    for source in sources {
        check_control(cancelled, deadline).map_err(map_local_error)?;
        entries.push(SourceManifestEntry::new(
            source.path().clone(),
            SourceFileKind::Regular,
            hash_source_content(source.content()),
        ));
    }
    SourceManifest::try_from_vec(
        entries,
        SourceFileLimit::new(limits.preparation().max_files()),
    )
    .map_err(|_| fence_capture_failed())
}

fn revalidate_selected_content(
    root: &ContainedSourceRoot,
    sources: &[ImmutableRustSource],
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalSourceSnapshotFenceError> {
    let mut session = root
        .exact_read_session(
            sources.iter().map(ImmutableRustSource::path),
            deadline,
            || cancelled.load(Ordering::Relaxed),
        )
        .map_err(map_session_error)?;
    for source in sources {
        check_control(cancelled, deadline).map_err(map_local_error)?;
        let read_limits =
            capped_source_read_limits(limits.source_read(), deadline).map_err(map_local_error)?;
        let content = session
            .read_with_cancel(source.path(), read_limits, || {
                cancelled.load(Ordering::Relaxed)
            })
            .map_err(map_contained_error)?;
        if hash_source_content(&content) != hash_source_content(source.content()) {
            return Err(LocalSourceSnapshotFenceError::SourceChanged);
        }
    }
    check_control(cancelled, deadline).map_err(map_local_error)
}

fn map_local_error(error: LocalRustIndexError) -> LocalSourceSnapshotFenceError {
    match error {
        LocalRustIndexError::Cancelled => LocalSourceSnapshotFenceError::Cancelled,
        LocalRustIndexError::DeadlineExceeded => LocalSourceSnapshotFenceError::DeadlineExceeded,
        LocalRustIndexError::ExcludedFileAlias => LocalSourceSnapshotFenceError::ExcludedFileAlias,
        LocalRustIndexError::SourceState {
            source:
                SourceStateError::SparseWorktreeUnsupported | SourceStateError::SubmoduleUnsupported,
        } => LocalSourceSnapshotFenceError::UnsupportedSourceState,
        LocalRustIndexError::SourceState {
            source: SourceStateError::ConcurrentSourceChange,
        }
        | LocalRustIndexError::StalePathSet
        | LocalRustIndexError::StaleSourceContent { .. } => {
            LocalSourceSnapshotFenceError::SourceChanged
        }
        LocalRustIndexError::SourceRead { .. } | LocalRustIndexError::RevalidationRead { .. } => {
            LocalSourceSnapshotFenceError::SourceChanged
        }
        _ => LocalSourceSnapshotFenceError::CaptureFailed,
    }
}

fn map_session_error(error: ExactReadSessionError) -> LocalSourceSnapshotFenceError {
    match error {
        ExactReadSessionError::Cancelled => LocalSourceSnapshotFenceError::Cancelled,
        ExactReadSessionError::DeadlineExceeded => LocalSourceSnapshotFenceError::DeadlineExceeded,
    }
}

fn map_contained_error(error: ContainedSourceError) -> LocalSourceSnapshotFenceError {
    match error {
        ContainedSourceError::Cancelled => LocalSourceSnapshotFenceError::Cancelled,
        ContainedSourceError::DeadlineExceeded { .. } => {
            LocalSourceSnapshotFenceError::DeadlineExceeded
        }
        _ => LocalSourceSnapshotFenceError::CaptureFailed,
    }
}

const fn fence_capture_failed() -> LocalSourceSnapshotFenceError {
    LocalSourceSnapshotFenceError::CaptureFailed
}

#[cfg(test)]
#[path = "source_snapshot_fence/tests.rs"]
mod tests;
