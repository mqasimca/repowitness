use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

use repowitness_application::PackageScope;
use repowitness_domain::SourceSlotId;

use crate::{
    LocalIndexError, LocalRustIndexError, LocalRustIndexLimits,
    contained_source::FileIdentity,
    git_paths::discovered_worktree_root,
    source_selector::{ResolvedSourceSelector, resolve_source_selector_until},
};

use super::super::{
    PreparedLocalIndexPublication, ReportInput, ScopedLocalIndexPublicationPreparationContext,
    configured_index_inputs, prepare_scoped_local_index_publication,
    validated_database_outside_worktree,
};
use super::{
    CoordinatorPhase,
    model::{ConnectedWorkspaceIndexError, ConnectedWorkspaceIndexRequest},
};
use crate::rust_index::SourceLanguageSelection;

pub(super) struct AuthorizedWorktree {
    slot_index: usize,
    slot_ordinal: u64,
    root: PathBuf,
    deadline: Instant,
}

#[derive(Clone)]
pub(super) struct ResolvedConnectedSource {
    slot_index: usize,
    slot_ordinal: u64,
    resolved_selector: ResolvedSourceSelector,
    deadline: Instant,
}

pub(super) struct PreparedConnectedSource {
    pub(super) slot_ordinal: u64,
    pub(super) source_slot: SourceSlotId,
    pub(super) resolved_selector: ResolvedSourceSelector,
    pub(super) package_scope: PackageScope,
    pub(super) selector_limits: crate::source_selector::SourceSelectorLimits,
    pub(super) languages: SourceLanguageSelection,
    pub(super) limits: LocalRustIndexLimits,
    pub(super) deadline: Instant,
    pub(super) publication: PreparedLocalIndexPublication,
    pub(super) report: ReportInput,
}

pub(super) fn authorize_worktrees_and_database(
    request: &ConnectedWorkspaceIndexRequest<'_>,
    operation_started: Instant,
    whole_deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<(Vec<AuthorizedWorktree>, PathBuf), ConnectedWorkspaceIndexError> {
    let mut worktrees = Vec::with_capacity(request.source_slots().len());
    let mut database = None;
    for (slot_index, slot) in request.source_slots().iter().enumerate() {
        check_control(cancelled, whole_deadline)?;
        let slot_ordinal = stable_ordinal(slot_index)?;
        let deadline = operation_started
            .checked_add(slot.deadline())
            .ok_or(ConnectedWorkspaceIndexError::DeadlineNotRepresentable)?
            .min(whole_deadline);
        check_control(cancelled, deadline)?;
        let root = discovered_worktree_root(slot.worktree()).map_err(|source| {
            ConnectedWorkspaceIndexError::Preparation {
                slot_ordinal,
                source: LocalIndexError::Preparation {
                    source: LocalRustIndexError::Discovery { source },
                },
            }
        })?;
        let validated_database = validated_database_outside_worktree(&root, request.database())
            .map_err(|source| ConnectedWorkspaceIndexError::DatabaseIsolation { source })?;
        match &database {
            Some(expected) if expected != &validated_database => {
                return Err(ConnectedWorkspaceIndexError::DatabaseIsolation {
                    source: LocalIndexError::DatabasePathUnavailable,
                });
            }
            Some(_) => {}
            None => database = Some(validated_database),
        }
        worktrees.push(AuthorizedWorktree {
            slot_index,
            slot_ordinal,
            root,
            deadline,
        });
    }
    let database = database.ok_or(ConnectedWorkspaceIndexError::DatabaseIsolation {
        source: LocalIndexError::DatabasePathUnavailable,
    })?;
    Ok((worktrees, database))
}

pub(super) fn resolve_connected_sources(
    request: &ConnectedWorkspaceIndexRequest<'_>,
    worktrees: Vec<AuthorizedWorktree>,
    cancelled: &Arc<AtomicBool>,
    mut after_phase: impl FnMut(CoordinatorPhase, u64),
) -> Result<Vec<ResolvedConnectedSource>, ConnectedWorkspaceIndexError> {
    let mut resolved = Vec::with_capacity(worktrees.len());
    for worktree in worktrees {
        check_control(cancelled.as_ref(), worktree.deadline)?;
        let slot = &request.source_slots()[worktree.slot_index];
        let selector = resolve_source_selector_until(
            &worktree.root,
            slot.selector().clone(),
            slot.selector_limits(),
            cancelled.as_ref(),
            worktree.deadline,
        )
        .map_err(|source| ConnectedWorkspaceIndexError::SelectorResolution {
            slot_ordinal: worktree.slot_ordinal,
            source,
        })?;
        after_phase(CoordinatorPhase::SelectorResolved, worktree.slot_ordinal);
        check_control(cancelled.as_ref(), worktree.deadline)?;
        resolved.push(ResolvedConnectedSource {
            slot_index: worktree.slot_index,
            slot_ordinal: worktree.slot_ordinal,
            resolved_selector: selector,
            deadline: worktree.deadline,
        });
    }
    Ok(resolved)
}

pub(super) fn prepare_connected_sources(
    request: &ConnectedWorkspaceIndexRequest<'_>,
    database: &Path,
    database_identity: Option<&FileIdentity>,
    resolved: Vec<ResolvedConnectedSource>,
    build_graph: bool,
    cancelled: &Arc<AtomicBool>,
    mut after_phase: impl FnMut(CoordinatorPhase, u64),
) -> Result<Vec<PreparedConnectedSource>, ConnectedWorkspaceIndexError> {
    let mut prepared = Vec::with_capacity(resolved.len());
    for resolved_source in resolved {
        check_control(cancelled.as_ref(), resolved_source.deadline)?;
        let slot = &request.source_slots()[resolved_source.slot_index];
        let (limits, languages) = configured_index_inputs(slot.limits(), slot.configuration())
            .map_err(|source| ConnectedWorkspaceIndexError::Preparation {
                slot_ordinal: resolved_source.slot_ordinal,
                source,
            })?;
        let (publication, report) =
            prepare_scoped_local_index_publication(ScopedLocalIndexPublicationPreparationContext {
                worktree: resolved_source.resolved_selector.worktree_root(),
                database,
                database_identity,
                connected_workspace: request.connected_workspace(),
                source_slot: slot.source_slot(),
                repository: slot.repository(),
                configuration_digest: slot.configuration().digest(),
                languages,
                package_scope: slot.package_scope(),
                limits,
                build_graph,
                cancelled,
                deadline: resolved_source.deadline,
            })
            .map_err(|source| ConnectedWorkspaceIndexError::Preparation {
                slot_ordinal: resolved_source.slot_ordinal,
                source,
            })?;
        after_phase(
            CoordinatorPhase::SourcePrepared,
            resolved_source.slot_ordinal,
        );
        check_control(cancelled.as_ref(), resolved_source.deadline)?;
        prepared.push(PreparedConnectedSource {
            slot_ordinal: resolved_source.slot_ordinal,
            source_slot: slot.source_slot(),
            resolved_selector: resolved_source.resolved_selector,
            package_scope: slot.package_scope().clone(),
            selector_limits: slot.selector_limits(),
            languages,
            limits,
            deadline: resolved_source.deadline,
            publication,
            report,
        });
    }
    Ok(prepared)
}

pub(super) fn check_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), ConnectedWorkspaceIndexError> {
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        Err(ConnectedWorkspaceIndexError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(ConnectedWorkspaceIndexError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

fn stable_ordinal(index: usize) -> Result<u64, ConnectedWorkspaceIndexError> {
    u64::try_from(index)
        .ok()
        .and_then(|ordinal| ordinal.checked_add(1))
        .ok_or(ConnectedWorkspaceIndexError::DeadlineNotRepresentable)
}
