//! One-shot composition of a fenced worktree change manifest and pinned context.

use std::{
    error::Error,
    fmt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use repowitness_application::{
    ChangeReviewReceipt, IndexWorktreeAlignment, IndexedContextUnavailableReason,
    RepositoryIdentityTextV1, ResolvedConfiguration, SymbolGetError, hash_source_manifest,
    resolve_configuration,
};
use repowitness_domain::GitObjectId;

use crate::{
    CapturedSourceState, ConnectedWorkspaceId, GitPathDiscoveryLimits, LocalChangeManifestError,
    LocalChangeManifestLimits, LocalContextBuildError, LocalContextBuildRequest,
    LocalRustIndexLimits, OwnedSqliteReader, SourceSlotId, SourceStateError, build_local_context,
    capture_local_change_manifest_with_cancel, capture_source_state_with_cancel,
    rust_index::{LocalSourceSnapshotFenceRequest, capture_confirmed_local_source_snapshot},
};

/// Default end-to-end deadline for one local read-only change review.
pub const DEFAULT_LOCAL_CHANGE_REVIEW_DEADLINE: Duration = Duration::from_secs(15);

/// A locally fenced receipt with context pinned to its own immutable generation.
pub type LocalChangeReviewReceipt = ChangeReviewReceipt<crate::GenerationId, i64>;

/// Complete inputs for one local read-only change review.
#[derive(Clone)]
pub struct LocalChangeReviewRequest<'a> {
    root: &'a Path,
    database: &'a Path,
    repository_identity: &'a str,
    intent: &'a str,
    base: GitObjectId,
    configuration: Option<&'a ResolvedConfiguration>,
    deadline: Duration,
}

impl<'a> LocalChangeReviewRequest<'a> {
    /// Creates a bounded request with explicit comparison base and review intent.
    #[must_use]
    pub const fn new(
        root: &'a Path,
        database: &'a Path,
        repository_identity: &'a str,
        intent: &'a str,
        base: GitObjectId,
    ) -> Self {
        Self {
            root,
            database,
            repository_identity,
            intent,
            base,
            configuration: None,
            deadline: DEFAULT_LOCAL_CHANGE_REVIEW_DEADLINE,
        }
    }

    /// Applies resolved configuration ceilings to the indexed-context provider.
    #[must_use]
    pub const fn with_configuration(mut self, configuration: &'a ResolvedConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }

    /// Replaces the total monotonic review deadline.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalChangeReviewRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalChangeReviewRequest")
            .field("root", &"<redacted-path>")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("intent", &"<redacted-intent>")
            .field("base", &self.base)
            .field(
                "configuration",
                &self.configuration.map(ResolvedConfiguration::digest),
            )
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Stable local change-review failure.
#[derive(Debug)]
pub enum LocalChangeReviewError {
    /// A zero or unrepresentable total deadline was supplied.
    DeadlineNotRepresentable,
    /// Cancellation was visible before a complete receipt existed.
    Cancelled,
    /// The total review deadline elapsed before a complete receipt existed.
    DeadlineExceeded,
    /// One initial or final source-state fence failed.
    SourceState {
        /// The redacted source-state failure.
        source: SourceStateError,
    },
    /// The bounded base-to-worktree manifest capture failed.
    Manifest {
        /// The redacted local manifest failure.
        source: LocalChangeManifestError,
    },
    /// The immutable indexed-context build failed.
    Context {
        /// The redacted context-build failure.
        source: LocalContextBuildError,
    },
    /// The worktree changed while the receipt was being assembled.
    ConcurrentSourceChange,
}

impl fmt::Display for LocalChangeReviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::DeadlineNotRepresentable => "change-review deadline cannot be represented",
            Self::Cancelled => "change review was cancelled",
            Self::DeadlineExceeded => "change-review deadline elapsed",
            Self::SourceState { .. } => "change-review source-state fence failed",
            Self::Manifest { .. } => "change-review change manifest failed",
            Self::Context { .. } => "change-review indexed context failed",
            Self::ConcurrentSourceChange => "worktree changed during change review",
        })
    }
}

impl Error for LocalChangeReviewError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SourceState { source } => Some(source),
            Self::Manifest { source } => Some(source),
            Self::Context { source } => Some(source),
            Self::DeadlineNotRepresentable
            | Self::Cancelled
            | Self::DeadlineExceeded
            | Self::ConcurrentSourceChange => None,
        }
    }
}

/// Builds one read-only receipt and rejects a changed worktree before return.
///
/// The receipt contains a final-fenced current worktree state and separately
/// pinned indexed context. It does not assert those two source states match.
///
/// # Errors
///
/// Returns no partial receipt for cancellation, deadline, source change, or
/// any bounded adapter/provider failure.
pub fn build_local_change_review(
    request: LocalChangeReviewRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalChangeReviewReceipt, LocalChangeReviewError> {
    if request.deadline.is_zero() {
        return Err(LocalChangeReviewError::DeadlineExceeded);
    }
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalChangeReviewError::DeadlineNotRepresentable)?;
    check_control(&cancelled, deadline)?;
    let before = capture_fence(request.root, deadline, &cancelled)?;
    let manifest_limits = LocalChangeManifestLimits::new(remaining_git_limits(deadline)?);
    let manifest = capture_local_change_manifest_with_cancel(
        request.root,
        request.base.clone(),
        manifest_limits,
        || cancelled.load(Ordering::Acquire),
    )
    .map_err(|source| LocalChangeReviewError::Manifest { source })?;
    check_control(&cancelled, deadline)?;
    let alignment = compare_index_worktree(
        request.root,
        request.database,
        request.repository_identity,
        request.configuration,
        &cancelled,
        deadline,
    );
    let mut context_request = LocalContextBuildRequest::new(
        request.root,
        request.database,
        request.repository_identity,
        request.intent,
    )
    .with_deadline(remaining(deadline)?);
    if let Some(configuration) = request.configuration {
        context_request = context_request.with_configuration(configuration);
    }
    let context = build_local_context(context_request, Arc::clone(&cancelled));
    check_control(&cancelled, deadline)?;
    let after = capture_fence(request.root, deadline, &cancelled)?;
    let final_manifest = capture_local_change_manifest_with_cancel(
        request.root,
        request.base.clone(),
        LocalChangeManifestLimits::new(remaining_git_limits(deadline)?),
        || cancelled.load(Ordering::Acquire),
    )
    .map_err(|source| LocalChangeReviewError::Manifest { source })?;
    if before != after || !manifest.same_tracked_diff(&final_manifest) {
        return Err(LocalChangeReviewError::ConcurrentSourceChange);
    }
    let manifest = manifest.into_manifest();
    match context {
        Ok(context) => Ok(ChangeReviewReceipt::with_indexed_context(
            before.git_state(),
            manifest,
            context,
            alignment,
        )),
        Err(LocalContextBuildError::Symbol(SymbolGetError::Port(
            crate::LocalSymbolPortError::StaleSource,
        ))) => Ok(ChangeReviewReceipt::without_indexed_context(
            before.git_state(),
            manifest,
            IndexedContextUnavailableReason::StaleSource,
            alignment,
        )),
        Err(source) => Err(LocalChangeReviewError::Context { source }),
    }
}

fn compare_index_worktree(
    root: &Path,
    database: &Path,
    repository_identity: &str,
    configuration: Option<&ResolvedConfiguration>,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> IndexWorktreeAlignment {
    let Ok(repository) = RepositoryIdentityTextV1::decode(repository_identity) else {
        return IndexWorktreeAlignment::Unavailable;
    };
    let resolved_configuration = configuration
        .cloned()
        .or_else(|| resolve_configuration(&[]).ok());
    let Some(resolved_configuration) = resolved_configuration else {
        return IndexWorktreeAlignment::Unavailable;
    };
    let configuration_digest = resolved_configuration.digest();
    let Ok((limits, languages)) = crate::local_index::configured_index_inputs(
        LocalRustIndexLimits::default(),
        &resolved_configuration,
    ) else {
        return IndexWorktreeAlignment::Unavailable;
    };
    let Ok(reader) = OwnedSqliteReader::start(database, deadline) else {
        return IndexWorktreeAlignment::Unavailable;
    };
    let workspace = ConnectedWorkspaceId::for_single_repository(repository);
    let source_slot = SourceSlotId::for_repository(repository);
    let pinned = reader.pin_workspace_view(workspace, None, Arc::clone(cancelled), deadline);
    let result = pinned
        .as_ref()
        .ok()
        .and_then(|view| view.as_ref())
        .and_then(|view| {
            reader
                .scip_import_scope(view, source_slot, Arc::clone(cancelled), deadline)
                .ok()
        })
        .map(|scope| {
            if scope.source_identity().configuration()
                != crate::local_index::local_source_snapshot_configuration(configuration_digest)
            {
                return IndexWorktreeAlignment::Mismatch;
            }
            let captured =
                capture_confirmed_local_source_snapshot(LocalSourceSnapshotFenceRequest::new(
                    root,
                    scope.source_identity(),
                    scope.source_snapshot(),
                    languages,
                    limits,
                    cancelled.as_ref(),
                    deadline,
                    None,
                ));
            match captured {
                Ok(captured)
                    if hash_source_manifest(captured.manifest()) == scope.source_manifest() =>
                {
                    IndexWorktreeAlignment::Verified
                }
                Ok(_) | Err(_) => IndexWorktreeAlignment::Mismatch,
            }
        })
        .unwrap_or(IndexWorktreeAlignment::Unavailable);
    if reader.shutdown(deadline).is_err() {
        IndexWorktreeAlignment::Unavailable
    } else {
        result
    }
}

fn capture_fence(
    root: &Path,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<CapturedSourceState, LocalChangeReviewError> {
    let limits = remaining_git_limits(deadline)?;
    capture_source_state_with_cancel(root, limits, || cancelled.load(Ordering::Acquire))
        .map_err(|source| LocalChangeReviewError::SourceState { source })
}

fn remaining_git_limits(
    deadline: Instant,
) -> Result<GitPathDiscoveryLimits, LocalChangeReviewError> {
    let remaining = remaining(deadline)?;
    Ok(GitPathDiscoveryLimits::new(
        remaining,
        GitPathDiscoveryLimits::default().output_bytes(),
        GitPathDiscoveryLimits::default().paths(),
        GitPathDiscoveryLimits::default().repository_path(),
    ))
}

fn remaining(deadline: Instant) -> Result<Duration, LocalChangeReviewError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(LocalChangeReviewError::DeadlineExceeded)
    } else {
        Ok(remaining)
    }
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), LocalChangeReviewError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalChangeReviewError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(LocalChangeReviewError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_LOCAL_CHANGE_REVIEW_DEADLINE, LocalChangeReviewRequest};
    use repowitness_domain::GitObjectId;

    #[test]
    fn request_debug_is_redacted_and_default_deadline_is_positive() {
        let base = GitObjectId::try_from_hex("0123456789abcdef0123456789abcdef01234567")
            .expect("base should parse");
        let request = LocalChangeReviewRequest::new(
            std::path::Path::new("/private/root"),
            std::path::Path::new("/private/database"),
            "rid1:private",
            "private intent",
            base,
        );
        let rendered = format!("{request:?}");
        assert!(!rendered.contains("private"));
        assert!(!DEFAULT_LOCAL_CHANGE_REVIEW_DEADLINE.is_zero());
    }
}
