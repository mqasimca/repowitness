use crate::{GitMemoryQueryError, GitPathDiscoveryError};

use super::super::LocalMemoryManageError;

pub(super) fn map_git_error(error: GitPathDiscoveryError) -> LocalMemoryManageError {
    match error {
        GitPathDiscoveryError::Cancelled => LocalMemoryManageError::Cancelled,
        GitPathDiscoveryError::DeadlineExceeded { .. }
        | GitPathDiscoveryError::DeadlineNotRepresentable => {
            LocalMemoryManageError::DeadlineExceeded
        }
        GitPathDiscoveryError::OutputByteLimitExceeded { .. }
        | GitPathDiscoveryError::PathLimitExceeded { .. } => {
            LocalMemoryManageError::HistoryLimitExceeded
        }
        _ => LocalMemoryManageError::HistoryUnavailable,
    }
}

pub(super) fn map_memory_query_error(error: GitMemoryQueryError) -> LocalMemoryManageError {
    match error {
        GitMemoryQueryError::Cancelled => LocalMemoryManageError::Cancelled,
        GitMemoryQueryError::DeadlineExceeded | GitMemoryQueryError::DeadlineNotRepresentable => {
            LocalMemoryManageError::DeadlineExceeded
        }
        GitMemoryQueryError::InvalidLimits => LocalMemoryManageError::InvalidLimits,
    }
}
