use std::{path::Path, sync::atomic::AtomicBool, time::Instant};

use super::{LocalMemoryHistoryImportLimits, capture};
use crate::{git_paths::sanitized_git_base_command, memory_management::LocalMemoryManageError};

pub(super) fn repository_is_shallow(
    worktree: &Path,
    limits: LocalMemoryHistoryImportLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<bool, LocalMemoryManageError> {
    let mut command = sanitized_git_base_command(worktree);
    command.arg("rev-parse").arg("--is-shallow-repository");
    match capture(command, limits, cancelled, deadline)?.as_slice() {
        b"true\n" => Ok(true),
        b"false\n" => Ok(false),
        _ => Err(LocalMemoryManageError::HistoryUnavailable),
    }
}
