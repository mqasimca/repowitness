//! Public aggregate-only connected-workspace manifest indexing facade.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use crate::{
    connected_workspace_manifest::{
        CONNECTED_WORKSPACE_MANIFEST_SCHEMA_VERSION, MAX_CONNECTED_WORKSPACE_MANIFEST_BYTES,
        parse_connected_workspace_manifest,
    },
    local_index::connected_workspace::{
        index_connected_workspace_with_manifest_parent,
        model::{ConnectedSourceSlotRequest, ConnectedWorkspaceIndexRequest},
    },
};

mod error;
mod model;
mod report;

pub use error::{
    LocalConnectedWorkspaceIndexError, LocalConnectedWorkspaceManifestErrorKind,
    LocalConnectedWorkspaceParentErrorKind, LocalConnectedWorkspacePhase,
    LocalConnectedWorkspaceRequestErrorKind,
};
pub use model::{
    DEFAULT_LOCAL_CONNECTED_WORKSPACE_DEADLINE, DEFAULT_LOCAL_CONNECTED_WORKSPACE_SOURCE_DEADLINE,
    LocalConnectedWorkspaceIndexRequest, LocalConnectedWorkspaceSourceLimits,
};
pub use report::{
    LOCAL_CONNECTED_WORKSPACE_REPORT_VERSION, LocalConnectedWorkspaceCoverage,
    LocalConnectedWorkspaceIndexReport, LocalConnectedWorkspaceMaintenance,
    LocalConnectedWorkspaceOutcome, LocalConnectedWorkspaceViewDigest,
};

/// Inclusive admitted-byte limit for one connected-workspace manifest.
pub const MAX_LOCAL_CONNECTED_WORKSPACE_MANIFEST_BYTES: usize =
    MAX_CONNECTED_WORKSPACE_MANIFEST_BYTES;

/// Parses, prepares, and atomically publishes one explicit connected workspace.
///
/// The admitted manifest parent is revalidated before source access and at the
/// coordinator's final publication fence. The result exposes only aggregate
/// counts and semantic digests; manifest DTOs, roots, selectors, source slots,
/// and database-local identities never cross this facade.
///
/// Do not retry
/// [`LocalConnectedWorkspaceIndexError::MutationOutcomeUnknown`] until
/// authoritative workspace state has been read using the error's
/// [`LocalConnectedWorkspaceIndexError::reconciliation_guidance`].
pub fn index_local_connected_workspace(
    request: LocalConnectedWorkspaceIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalConnectedWorkspaceIndexReport, LocalConnectedWorkspaceIndexError> {
    index_local_connected_workspace_with_hook(request, cancelled, || {})
}

fn index_local_connected_workspace_with_hook(
    request: LocalConnectedWorkspaceIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
    after_manifest_parse: impl FnOnce(),
) -> Result<LocalConnectedWorkspaceIndexReport, LocalConnectedWorkspaceIndexError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(LocalConnectedWorkspaceIndexError::Cancelled);
    }
    if Instant::now().checked_add(request.deadline).is_none() {
        return Err(LocalConnectedWorkspaceIndexError::DeadlineNotRepresentable);
    }
    if !request
        .manifest_parent
        .matches_contents(request.manifest_bytes)
    {
        return Err(LocalConnectedWorkspaceIndexError::ManifestParent {
            kind: LocalConnectedWorkspaceParentErrorKind::Changed,
        });
    }
    request
        .manifest_parent
        .revalidate()
        .map_err(LocalConnectedWorkspaceIndexError::from_parent)?;
    let parsed = parse_connected_workspace_manifest(
        request.manifest_bytes,
        request.manifest_parent.lexical_path(),
    )
    .map_err(LocalConnectedWorkspaceIndexError::from_manifest)?;
    after_manifest_parse();

    let configured = parsed.with_configuration(request.configuration.clone());
    let (manifest, configuration) = configured.into_parts();
    let source_slots = manifest
        .sources()
        .iter()
        .map(|source| {
            ConnectedSourceSlotRequest::try_from_validated(
                source.source_slot(),
                source.repository(),
                source.worktree_root(),
                source.selector().clone(),
                source.package_scope().clone(),
                &configuration,
                request.source_limits.index(),
                request.source_limits.selector_limits(),
                request.source_limits.deadline(),
            )
            .map_err(LocalConnectedWorkspaceIndexError::from_request)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let coordinator = ConnectedWorkspaceIndexRequest::try_new(
        manifest.connected_workspace(),
        request.database,
        request.migration_applied_at_unix_ms,
        request.deadline,
        source_slots,
    )
    .map_err(LocalConnectedWorkspaceIndexError::from_request)?;
    let report = index_connected_workspace_with_manifest_parent(
        coordinator,
        cancelled,
        request.manifest_parent,
    )
    .map_err(LocalConnectedWorkspaceIndexError::from_internal)?;
    Ok(LocalConnectedWorkspaceIndexReport::from_internal(
        CONNECTED_WORKSPACE_MANIFEST_SCHEMA_VERSION,
        report,
    ))
}

#[cfg(test)]
mod tests;
