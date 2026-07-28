use std::sync::{Arc, atomic::AtomicBool};

use repowitness_application::{
    MemoryImportApproval, RepositoryIdentityTextV1, import_memory_record,
};
use repowitness_domain::{MemoryAuditActorId, MemoryObservationSource, MemoryRecordedAtUnixMillis};

use super::{
    LocalMemoryApprovalReceipt, LocalMemoryApprovalRequest, LocalMemoryManageError, check_control,
    checked_deadline, map_file_error, map_repository_identity_error, map_store_error, open_store,
    open_worktree, record_id, secret,
};
use crate::MemoryRecordFiles;

pub(super) fn approve(
    request: LocalMemoryApprovalRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalMemoryApprovalReceipt, LocalMemoryManageError> {
    let deadline = checked_deadline(request.deadline)?;
    check_control(cancelled.as_ref(), deadline)?;
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(map_repository_identity_error)?;
    let record_id = record_id(request.record_id)?;
    let actor = MemoryAuditActorId::try_new(request.actor.to_owned())
        .map_err(|_| LocalMemoryManageError::ActorInvalid)?;
    let recorded_at = MemoryRecordedAtUnixMillis::try_new(request.recorded_at_unix_ms)
        .map_err(|_| LocalMemoryManageError::InvalidLimits)?;
    let worktree = open_worktree(request.repository_root)?;
    let files = MemoryRecordFiles::open(&worktree).map_err(map_file_error)?;
    let loaded = files
        .load(record_id, cancelled.as_ref(), deadline)
        .map_err(map_file_error)?;
    if loaded.record().scope().repository() != repository {
        return Err(LocalMemoryManageError::ScopeMismatch);
    }
    secret::check_record(loaded.record())?;

    let store = open_store(
        &worktree,
        request.database,
        request.migration_applied_at_unix_ms,
        Arc::clone(&cancelled),
        deadline,
    )?;
    let operation = approve_loaded(
        &store,
        repository,
        actor,
        recorded_at,
        loaded,
        &cancelled,
        deadline,
    );
    finish(store, operation, deadline)
}

#[allow(
    clippy::too_many_arguments,
    reason = "the exact source, actor, timestamp, and control inputs are independent trust values"
)]
fn approve_loaded(
    store: &crate::OwnedSqliteIndex,
    repository: repowitness_domain::RepositoryIdentityDigest,
    actor: MemoryAuditActorId,
    recorded_at: MemoryRecordedAtUnixMillis,
    loaded: crate::LoadedMemoryRecord,
    cancelled: &Arc<AtomicBool>,
    deadline: std::time::Instant,
) -> Result<LocalMemoryApprovalReceipt, LocalMemoryManageError> {
    let source = store
        .load_memory_source(repository, Arc::clone(cancelled), deadline)
        .map_err(map_store_error)?;
    let (record, _, presentation) = loaded.into_parts();
    import_memory_record(
        store,
        repowitness_application::ImportMemoryRecordRequest::new(
            repository,
            record,
            presentation,
            MemoryObservationSource::Worktree(source.snapshot()),
            actor,
            recorded_at,
            MemoryImportApproval::LocallyApproved,
            Arc::clone(cancelled),
            deadline,
        ),
    )
    .map(LocalMemoryApprovalReceipt::from)
    .map_err(|_| LocalMemoryManageError::PersistenceFailed)
}

fn finish(
    store: crate::OwnedSqliteIndex,
    operation: Result<LocalMemoryApprovalReceipt, LocalMemoryManageError>,
    deadline: std::time::Instant,
) -> Result<LocalMemoryApprovalReceipt, LocalMemoryManageError> {
    if operation.is_ok() {
        store.checkpoint(deadline).map_err(map_store_error)?;
    }
    let shutdown = store.shutdown(deadline).map_err(map_store_error);
    match (operation, shutdown) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(receipt), Ok(())) => Ok(receipt),
    }
}
