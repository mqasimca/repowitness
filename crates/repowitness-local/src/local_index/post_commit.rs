use std::time::Instant;

use crate::OwnedSqliteIndex;

/// Redacted maintenance observation after an indexing outcome is committed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PostCommitMaintenanceStatus {
    Complete,
    CheckpointDeferred,
    ShutdownDeferred,
    CheckpointAndShutdownDeferred,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PostCommitMaintenancePhase {
    Checkpoint,
    Shutdown,
}

/// Attempts bounded maintenance without changing an already committed outcome.
pub(crate) fn finish_index_writer(
    writer: OwnedSqliteIndex,
    checkpoint_required: bool,
    deadline: Instant,
    mut phase_deadline: impl FnMut(PostCommitMaintenancePhase, Instant) -> Instant,
) -> PostCommitMaintenanceStatus {
    let checkpoint_complete = !checkpoint_required
        || writer
            .checkpoint(phase_deadline(
                PostCommitMaintenancePhase::Checkpoint,
                deadline,
            ))
            .is_ok();
    let shutdown_complete = writer
        .shutdown(phase_deadline(
            PostCommitMaintenancePhase::Shutdown,
            deadline,
        ))
        .is_ok();

    match (checkpoint_complete, shutdown_complete) {
        (true, true) => PostCommitMaintenanceStatus::Complete,
        (false, true) => PostCommitMaintenanceStatus::CheckpointDeferred,
        (true, false) => PostCommitMaintenanceStatus::ShutdownDeferred,
        (false, false) => PostCommitMaintenanceStatus::CheckpointAndShutdownDeferred,
    }
}
