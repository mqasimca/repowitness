use std::{
    collections::BTreeSet,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

use repowitness_application::{
    CompleteStagedSourceSlotIndexError, PublishSourceSlotIndexRequest, SourceSlotFinalFence,
    StagedSourceSlotIndex, complete_staged_source_slot_index, hash_source_snapshot,
    stage_source_slot_index,
};

use crate::{
    AdmittedFileParent, IndexStoreStartup, LocalIndexError, LocalRustIndexError, OwnedSqliteIndex,
    SqliteStoreError, WorkspaceSourceSlot,
    contained_source::FileIdentity,
    sqlite::{CompletedWorkspaceSource, SqliteMutationLease},
};

use self::{
    final_fence::ConnectedSourceSlotFinalFence,
    model::{
        ConnectedSourceSlotReport, ConnectedWorkspaceIndexError, ConnectedWorkspaceIndexReport,
        ConnectedWorkspaceIndexRequest,
    },
    preparation::{
        PreparedConnectedSource, authorize_worktrees_and_database, check_control,
        prepare_connected_sources, resolve_connected_sources,
    },
    receipt::{CanonicalViewMemberReceipt, canonical_view_receipt_digest},
};
pub(crate) use super::post_commit::PostCommitMaintenanceStatus;
use super::post_commit::{PostCommitMaintenancePhase, finish_index_writer};

mod final_fence;
pub(crate) use final_fence::ConnectedSourceSlotFinalFenceError;
pub(crate) mod model;
mod preparation;
mod receipt;
pub(crate) use receipt::{CONNECTED_WORKSPACE_VIEW_RECEIPT_VERSION, ConnectedWorkspaceViewDigest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CoordinatorPhase {
    MutationLeaseAcquired,
    WorkspaceRegistered,
    SelectorResolved,
    SourcePrepared,
    SourceStaged,
    GraphStaged,
    SourceCompleted,
    BeforeFinalFence,
    BeforeViewPublication,
    ViewPublished,
    CheckpointAttempted,
}

struct CompletedConnectedSource {
    slot_ordinal: u64,
    source_slot: repowitness_domain::SourceSlotId,
    resolved_selector: crate::source_selector::ResolvedSourceSelector,
    package_scope: repowitness_application::PackageScope,
    selector_limits: crate::source_selector::SourceSelectorLimits,
    languages: crate::rust_index::SourceLanguageSelection,
    limits: crate::LocalRustIndexLimits,
    deadline: Instant,
    identity: repowitness_application::RustSourceSnapshotIdentity,
    expected_snapshot: repowitness_domain::SourceSnapshotDigest,
    completed: repowitness_application::CompletedSourceSlotIndex<crate::GenerationId>,
    report: SlotReportTotals,
}

struct StagedConnectedSource {
    slot_ordinal: u64,
    source_slot: repowitness_domain::SourceSlotId,
    resolved_selector: crate::source_selector::ResolvedSourceSelector,
    package_scope: repowitness_application::PackageScope,
    selector_limits: crate::source_selector::SourceSelectorLimits,
    languages: crate::rust_index::SourceLanguageSelection,
    limits: crate::LocalRustIndexLimits,
    deadline: Instant,
    identity: repowitness_application::RustSourceSnapshotIdentity,
    expected_snapshot: repowitness_domain::SourceSnapshotDigest,
    staged: StagedSourceSlotIndex<crate::GenerationId>,
    report: SlotReportTotals,
}

struct StartedConnectedWorkspace {
    writer: OwnedSqliteIndex,
    startup: IndexStoreStartup,
}

#[derive(Clone, Copy)]
struct SlotReportTotals {
    discovered_paths: u64,
    indexed_files: u64,
    skipped_paths: u64,
    skipped_policy_paths: u64,
    skipped_unsupported_paths: u64,
    reused_files: u64,
    analyzed_files: u64,
}

/// Indexes every validated source slot and publishes one all-or-nothing view.
#[cfg(test)]
pub(crate) fn index_connected_workspace(
    request: ConnectedWorkspaceIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<ConnectedWorkspaceIndexReport, ConnectedWorkspaceIndexError> {
    index_connected_workspace_with_control_hooks(
        request,
        cancelled,
        |_, _| {},
        |_, deadline| deadline,
    )
}

pub(crate) fn index_connected_workspace_with_manifest_parent(
    request: ConnectedWorkspaceIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
    manifest_parent: &AdmittedFileParent,
) -> Result<ConnectedWorkspaceIndexReport, ConnectedWorkspaceIndexError> {
    index_connected_workspace_with_parent_control_hooks(
        request,
        cancelled,
        Some(manifest_parent),
        |_, _| {},
        |_, deadline| deadline,
    )
}

#[cfg(test)]
fn index_connected_workspace_with_hook(
    request: ConnectedWorkspaceIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
    after_phase: impl FnMut(CoordinatorPhase, u64),
) -> Result<ConnectedWorkspaceIndexReport, ConnectedWorkspaceIndexError> {
    index_connected_workspace_with_control_hooks(request, cancelled, after_phase, |_, deadline| {
        deadline
    })
}

#[cfg(test)]
fn index_connected_workspace_with_control_hooks(
    request: ConnectedWorkspaceIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
    after_phase: impl FnMut(CoordinatorPhase, u64),
    maintenance_deadline: impl FnMut(PostCommitMaintenancePhase, Instant) -> Instant,
) -> Result<ConnectedWorkspaceIndexReport, ConnectedWorkspaceIndexError> {
    index_connected_workspace_with_parent_control_hooks(
        request,
        cancelled,
        None,
        after_phase,
        maintenance_deadline,
    )
}

fn index_connected_workspace_with_parent_control_hooks(
    request: ConnectedWorkspaceIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
    manifest_parent: Option<&AdmittedFileParent>,
    mut after_phase: impl FnMut(CoordinatorPhase, u64),
    mut maintenance_deadline: impl FnMut(PostCommitMaintenancePhase, Instant) -> Instant,
) -> Result<ConnectedWorkspaceIndexReport, ConnectedWorkspaceIndexError> {
    let operation_started = Instant::now();
    let whole_deadline = operation_started
        .checked_add(request.deadline())
        .ok_or(ConnectedWorkspaceIndexError::DeadlineNotRepresentable)?;
    check_control(cancelled.as_ref(), whole_deadline)?;
    revalidate_manifest_parent(manifest_parent)?;
    let (worktrees, database) = authorize_worktrees_and_database(
        &request,
        operation_started,
        whole_deadline,
        cancelled.as_ref(),
    )?;
    let started = start_connected_workspace(
        &request,
        &database,
        &cancelled,
        whole_deadline,
        &mut after_phase,
    )?;
    let StartedConnectedWorkspace { writer, startup } = started;
    let resolved = resolve_connected_sources(&request, worktrees, &cancelled, &mut after_phase)?;
    let prepared = prepare_connected_sources(
        &request,
        &database,
        Some(writer.opened_database_identity()),
        resolved,
        &cancelled,
        &mut after_phase,
    )?;
    let completed = stage_and_complete_sources(
        &writer,
        request.connected_workspace(),
        &database,
        Some(writer.opened_database_identity()),
        prepared,
        &cancelled,
        &mut after_phase,
    )?;
    after_phase(CoordinatorPhase::BeforeFinalFence, 0);
    check_control(cancelled.as_ref(), whole_deadline)?;
    revalidate_manifest_parent(manifest_parent)?;
    confirm_all_sources(
        &completed,
        &database,
        Some(writer.opened_database_identity()),
        &cancelled,
    )?;
    after_phase(CoordinatorPhase::BeforeViewPublication, 0);
    check_control(cancelled.as_ref(), whole_deadline)?;
    revalidate_manifest_parent(manifest_parent)?;
    confirm_writer_database_identity(&writer, &database)?;
    let source_reports = completed
        .iter()
        .map(connected_source_report)
        .collect::<Vec<_>>();
    let view_receipt_digest = canonical_view_receipt_digest(
        request.connected_workspace(),
        &completed
            .iter()
            .map(|source| {
                CanonicalViewMemberReceipt::new(
                    source.source_slot,
                    source.completed.source_epoch(),
                    source.identity.repository(),
                    source.expected_snapshot,
                    source.identity.configuration(),
                )
            })
            .collect::<Vec<_>>(),
    );
    let view = writer
        .publish_completed_workspace_view(
            request.connected_workspace(),
            completed
                .iter()
                .map(|source| CompletedWorkspaceSource::new(source.source_slot, source.completed))
                .collect(),
            Arc::clone(&cancelled),
            whole_deadline,
        )
        .map_err(|source| ConnectedWorkspaceIndexError::ViewPublication { source })?;
    let maintenance = finish_index_writer(writer, true, whole_deadline, |phase, deadline| {
        after_phase(
            match phase {
                PostCommitMaintenancePhase::Checkpoint => CoordinatorPhase::ViewPublished,
                PostCommitMaintenancePhase::Shutdown => CoordinatorPhase::CheckpointAttempted,
            },
            0,
        );
        maintenance_deadline(phase, deadline)
    });
    Ok(ConnectedWorkspaceIndexReport::new(
        view,
        startup.recovered_generations(),
        request.configuration_digest(),
        view_receipt_digest,
        maintenance,
        source_reports,
    ))
}

fn revalidate_manifest_parent(
    manifest_parent: Option<&AdmittedFileParent>,
) -> Result<(), ConnectedWorkspaceIndexError> {
    manifest_parent.map_or(Ok(()), |parent| {
        parent
            .revalidate()
            .map_err(|source| ConnectedWorkspaceIndexError::ManifestParentAuthority { source })
    })
}

fn start_connected_workspace(
    request: &ConnectedWorkspaceIndexRequest<'_>,
    database: &std::path::Path,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
    after_phase: &mut impl FnMut(CoordinatorPhase, u64),
) -> Result<StartedConnectedWorkspace, ConnectedWorkspaceIndexError> {
    let mutation_lease = SqliteMutationLease::acquire(database, deadline)
        .map_err(|source| ConnectedWorkspaceIndexError::StoreStartup { source })?;
    after_phase(CoordinatorPhase::MutationLeaseAcquired, 0);
    check_control(cancelled.as_ref(), deadline)?;
    let database_identity_before = super::database_alias_identity(database)
        .map_err(|source| ConnectedWorkspaceIndexError::DatabaseIsolation { source })?;
    let (writer, startup) = OwnedSqliteIndex::start_with_lease(
        mutation_lease,
        database_identity_before,
        request.migration_applied_at_unix_ms(),
        Arc::clone(cancelled),
        deadline,
    )
    .map_err(map_store_startup_error)?;
    register_workspace_sources(&writer, request, cancelled, deadline)?;
    after_phase(CoordinatorPhase::WorkspaceRegistered, 0);
    check_control(cancelled.as_ref(), deadline)?;
    confirm_writer_database_identity(&writer, database)?;
    Ok(StartedConnectedWorkspace { writer, startup })
}

fn confirm_writer_database_identity(
    writer: &OwnedSqliteIndex,
    database: &std::path::Path,
) -> Result<(), ConnectedWorkspaceIndexError> {
    let current = super::database_alias_identity(database)
        .map_err(|source| ConnectedWorkspaceIndexError::DatabaseIsolation { source })?;
    if current.as_ref() != Some(writer.opened_database_identity()) {
        return Err(ConnectedWorkspaceIndexError::DatabaseIsolation {
            source: LocalIndexError::DatabaseChangedDuringIndexing,
        });
    }
    Ok(())
}

fn register_workspace_sources(
    writer: &OwnedSqliteIndex,
    request: &ConnectedWorkspaceIndexRequest<'_>,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> Result<(), ConnectedWorkspaceIndexError> {
    let repositories = request
        .source_slots()
        .iter()
        .map(|slot| slot.repository())
        .collect::<BTreeSet<_>>();
    for repository in repositories {
        check_control(cancelled.as_ref(), deadline)?;
        writer
            .ensure_workspace(repository, super::INITIAL_SOURCE_EPOCH, deadline)
            .map_err(|source| ConnectedWorkspaceIndexError::WorkspaceRegistration { source })?;
    }
    let mappings = request
        .source_slots()
        .iter()
        .map(|slot| WorkspaceSourceSlot::new(slot.source_slot(), slot.repository()))
        .collect();
    writer
        .connect_workspace(
            request.connected_workspace(),
            mappings,
            Arc::clone(cancelled),
            deadline,
        )
        .map_err(|source| ConnectedWorkspaceIndexError::WorkspaceRegistration { source })
}

#[allow(
    clippy::too_many_arguments,
    reason = "writer, workspace, database authority, sources, control, and hooks remain explicit"
)]
fn stage_and_complete_sources(
    writer: &OwnedSqliteIndex,
    connected_workspace: repowitness_domain::ConnectedWorkspaceId,
    database: &std::path::Path,
    database_identity: Option<&FileIdentity>,
    prepared: Vec<PreparedConnectedSource>,
    cancelled: &Arc<AtomicBool>,
    mut after_phase: impl FnMut(CoordinatorPhase, u64),
) -> Result<Vec<CompletedConnectedSource>, ConnectedWorkspaceIndexError> {
    let mut completed_sources = Vec::with_capacity(prepared.len());
    for source in prepared {
        let staged = stage_connected_source(
            writer,
            connected_workspace,
            source,
            cancelled,
            &mut after_phase,
        )?;
        completed_sources.push(complete_connected_source(
            writer,
            database,
            database_identity,
            staged,
            cancelled,
            &mut after_phase,
        )?);
    }
    Ok(completed_sources)
}

fn stage_connected_source(
    writer: &OwnedSqliteIndex,
    connected_workspace: repowitness_domain::ConnectedWorkspaceId,
    source: PreparedConnectedSource,
    cancelled: &Arc<AtomicBool>,
    after_phase: &mut impl FnMut(CoordinatorPhase, u64),
) -> Result<StagedConnectedSource, ConnectedWorkspaceIndexError> {
    check_control(cancelled.as_ref(), source.deadline)?;
    let report = slot_report_totals(source.slot_ordinal, &source.report)?;
    let state = writer
        .source_slot_state(
            connected_workspace,
            source.source_slot,
            Arc::clone(cancelled),
            source.deadline,
        )
        .map_err(|error| ConnectedWorkspaceIndexError::WorkspaceRegistration { source: error })?;
    let reserved_epoch = writer
        .reserve_source_slot_epoch(
            connected_workspace,
            source.source_slot,
            state.current_epoch(),
            Arc::clone(cancelled),
            source.deadline,
        )
        .map_err(|error| ConnectedWorkspaceIndexError::WorkspaceRegistration { source: error })?;
    let identity = source.publication.identity;
    let expected_snapshot =
        hash_source_snapshot(identity, source.publication.prepared.manifest_digest());
    let staged = stage_source_slot_index(
        writer,
        PublishSourceSlotIndexRequest::new(
            connected_workspace,
            source.source_slot,
            reserved_epoch,
            identity,
            source.publication.prepared,
            source.publication.coverage,
            Arc::clone(cancelled),
            source.deadline,
        ),
    )
    .map_err(|error| ConnectedWorkspaceIndexError::PublicationStaging {
        slot_ordinal: source.slot_ordinal,
        source: error,
    })?;
    after_phase(CoordinatorPhase::SourceStaged, source.slot_ordinal);
    check_control(cancelled.as_ref(), source.deadline)?;
    if let Some(graph) = source.publication.graph {
        let graph = graph
            .into_generation(staged.generation(), cancelled.as_ref(), source.deadline)
            .map_err(|graph_error| ConnectedWorkspaceIndexError::Preparation {
                slot_ordinal: source.slot_ordinal,
                source: LocalIndexError::GraphPreparation {
                    source: graph_error,
                },
            })?;
        writer
            .stage_rust_graph(
                staged.generation(),
                graph,
                Arc::clone(cancelled),
                source.deadline,
            )
            .map_err(
                |error| ConnectedWorkspaceIndexError::GraphPublicationStaging {
                    slot_ordinal: source.slot_ordinal,
                    source: error,
                },
            )?;
        after_phase(CoordinatorPhase::GraphStaged, source.slot_ordinal);
    }
    check_control(cancelled.as_ref(), source.deadline)?;
    Ok(StagedConnectedSource {
        slot_ordinal: source.slot_ordinal,
        source_slot: source.source_slot,
        resolved_selector: source.resolved_selector,
        package_scope: source.package_scope,
        selector_limits: source.selector_limits,
        languages: source.languages,
        limits: source.limits,
        deadline: source.deadline,
        identity,
        expected_snapshot,
        staged,
        report,
    })
}

fn complete_connected_source(
    writer: &OwnedSqliteIndex,
    database: &std::path::Path,
    database_identity: Option<&FileIdentity>,
    source: StagedConnectedSource,
    cancelled: &Arc<AtomicBool>,
    after_phase: &mut impl FnMut(CoordinatorPhase, u64),
) -> Result<CompletedConnectedSource, ConnectedWorkspaceIndexError> {
    let fence = ConnectedSourceSlotFinalFence::new(
        source.resolved_selector.worktree_root(),
        database,
        database_identity,
        &source.resolved_selector,
        source.selector_limits,
        &source.package_scope,
        source.identity,
        source.languages,
        source.limits,
    );
    let completed =
        complete_staged_source_slot_index(writer, &fence, source.staged).map_err(|error| {
            match error {
                CompleteStagedSourceSlotIndexError::FinalFence(source_error) => {
                    ConnectedWorkspaceIndexError::FinalSourceFence {
                        slot_ordinal: source.slot_ordinal,
                        source: source_error,
                    }
                }
                CompleteStagedSourceSlotIndexError::Complete(source_error) => {
                    ConnectedWorkspaceIndexError::Completion {
                        slot_ordinal: source.slot_ordinal,
                        source: source_error,
                    }
                }
            }
        })?;
    after_phase(CoordinatorPhase::SourceCompleted, source.slot_ordinal);
    check_control(cancelled.as_ref(), source.deadline)?;
    Ok(CompletedConnectedSource {
        slot_ordinal: source.slot_ordinal,
        source_slot: source.source_slot,
        resolved_selector: source.resolved_selector,
        package_scope: source.package_scope,
        selector_limits: source.selector_limits,
        languages: source.languages,
        limits: source.limits,
        deadline: source.deadline,
        identity: source.identity,
        expected_snapshot: source.expected_snapshot,
        completed,
        report: source.report,
    })
}

fn confirm_all_sources(
    completed: &[CompletedConnectedSource],
    database: &std::path::Path,
    database_identity: Option<&FileIdentity>,
    cancelled: &Arc<AtomicBool>,
) -> Result<(), ConnectedWorkspaceIndexError> {
    for source in completed {
        check_control(cancelled.as_ref(), source.deadline)?;
        ConnectedSourceSlotFinalFence::new(
            source.resolved_selector.worktree_root(),
            database,
            database_identity,
            &source.resolved_selector,
            source.selector_limits,
            &source.package_scope,
            source.identity,
            source.languages,
            source.limits,
        )
        .confirm_source_snapshot(
            source.expected_snapshot,
            Arc::clone(cancelled),
            source.deadline,
        )
        .map_err(
            |source_error| ConnectedWorkspaceIndexError::FinalSourceFence {
                slot_ordinal: source.slot_ordinal,
                source: source_error,
            },
        )?;
    }
    Ok(())
}

fn slot_report_totals(
    slot_ordinal: u64,
    report: &super::ReportInput,
) -> Result<SlotReportTotals, ConnectedWorkspaceIndexError> {
    let reused_files = checked_sum(
        [
            report.reused_rust_files,
            report.reused_go_files,
            report.reused_typescript_files,
            report.reused_tsx_files,
            report.reused_python_files,
        ],
        slot_ordinal,
    )?;
    let analyzed_files = checked_sum(
        [
            report.analyzed_rust_files,
            report.analyzed_go_files,
            report.analyzed_typescript_files,
            report.analyzed_tsx_files,
            report.analyzed_python_files,
        ],
        slot_ordinal,
    )?;
    let skipped_paths =
        report
            .skipped_paths()
            .map_err(|source| ConnectedWorkspaceIndexError::Preparation {
                slot_ordinal,
                source,
            })?;
    Ok(SlotReportTotals {
        discovered_paths: report.discovered_paths,
        indexed_files: report.indexed_files,
        skipped_paths,
        skipped_policy_paths: report.skipped_policy_paths,
        skipped_unsupported_paths: report.skipped_unsupported_paths,
        reused_files,
        analyzed_files,
    })
}

fn checked_sum(values: [u64; 5], slot_ordinal: u64) -> Result<u64, ConnectedWorkspaceIndexError> {
    values.into_iter().try_fold(0_u64, |total, value| {
        total
            .checked_add(value)
            .ok_or(ConnectedWorkspaceIndexError::Preparation {
                slot_ordinal,
                source: LocalIndexError::Preparation {
                    source: LocalRustIndexError::SourceByteCountOverflowed,
                },
            })
    })
}

fn connected_source_report(source: &CompletedConnectedSource) -> ConnectedSourceSlotReport {
    ConnectedSourceSlotReport::new(
        source.source_slot,
        source.completed.generation(),
        source.report.discovered_paths,
        source.report.indexed_files,
        source.report.skipped_paths,
        source.report.skipped_policy_paths,
        source.report.skipped_unsupported_paths,
        source.report.reused_files,
        source.report.analyzed_files,
    )
}

fn map_store_startup_error(source: SqliteStoreError) -> ConnectedWorkspaceIndexError {
    match source {
        SqliteStoreError::DatabaseIdentityChanged => {
            ConnectedWorkspaceIndexError::DatabaseIsolation {
                source: LocalIndexError::DatabaseChangedDuringIndexing,
            }
        }
        source => ConnectedWorkspaceIndexError::StoreStartup { source },
    }
}

#[cfg(test)]
mod tests;
