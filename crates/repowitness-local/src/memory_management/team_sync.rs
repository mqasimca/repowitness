use std::sync::{Arc, atomic::AtomicBool};

use repowitness_application::RepositoryIdentityTextV1;
use repowitness_domain::{MemoryAuditActorId, MemoryObservationSource, MemoryRecordedAtUnixMillis};

use super::{
    LocalMemoryMaintenance, LocalMemoryManageError, LocalMemoryMutation,
    LocalTeamMemorySyncRequest, check_control, checked_deadline,
    finish_known_memory_mutation_with_hook, map_file_error, map_repository_identity_error,
    map_store_error, open_store, open_worktree, record_id, secret,
};
use crate::MemoryRecordFiles;

/// Redacted durable outcome from synchronizing one canonical team-memory record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalTeamMemorySyncReceipt {
    revision: repowitness_domain::CanonicalMemoryDigest,
    version_inserted: bool,
    observation_inserted: bool,
    maintenance: LocalMemoryMaintenance,
}

impl LocalTeamMemorySyncReceipt {
    /// Returns the canonical semantic revision that was observed.
    #[must_use]
    pub const fn revision(self) -> repowitness_domain::CanonicalMemoryDigest {
        self.revision
    }
    /// Reports whether the immutable semantic version was newly inserted.
    #[must_use]
    pub const fn version_inserted(self) -> bool {
        self.version_inserted
    }
    /// Reports whether the exact repository observation was newly appended.
    #[must_use]
    pub const fn observation_inserted(self) -> bool {
        self.observation_inserted
    }
    /// Returns truthful post-commit maintenance status.
    #[must_use]
    pub const fn maintenance(self) -> LocalMemoryMaintenance {
        self.maintenance
    }

    fn from_import(receipt: repowitness_application::MemoryImportReceipt) -> Self {
        Self {
            revision: receipt.revision(),
            version_inserted: receipt.version_inserted(),
            observation_inserted: receipt.observation_inserted(),
            maintenance: LocalMemoryMaintenance::pending(),
        }
    }

    const fn with_maintenance(mut self, maintenance: LocalMemoryMaintenance) -> Self {
        self.maintenance = maintenance;
        self
    }
}

pub(super) fn sync(
    request: LocalTeamMemorySyncRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalTeamMemorySyncReceipt, LocalMemoryManageError> {
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
    let loaded = MemoryRecordFiles::open(&worktree)
        .map_err(map_file_error)?
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
    let result = (|| {
        let source = store
            .load_memory_source(repository, Arc::clone(&cancelled), deadline)
            .map_err(|error| map_store_error(error, LocalMemoryMutation::TeamSync))?;
        let (record, _revision, presentation) = loaded.into_parts();
        store
            .sync_team_memory(
                repository,
                record,
                presentation,
                MemoryObservationSource::Worktree(source.snapshot()),
                actor,
                recorded_at,
                Arc::clone(&cancelled),
                deadline,
            )
            .map(LocalTeamMemorySyncReceipt::from_import)
            .map_err(|error| map_store_error(error, LocalMemoryMutation::TeamSync))
    })();
    match result {
        Err(error) => {
            let _ = store.shutdown(deadline);
            Err(error)
        }
        Ok(receipt) => {
            let (receipt, maintenance) =
                finish_known_memory_mutation_with_hook(store, receipt, deadline, || {});
            Ok(receipt.with_maintenance(maintenance))
        }
    }
}
