use std::{
    io,
    ops::Deref,
    path::{Path, PathBuf},
    time::Instant,
};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt as _};
use cap_std::{
    ambient_authority,
    fs::{Dir, Metadata, OpenOptions},
};

use super::database_alias_identity;
use crate::{SqliteStoreError, contained_source::FileIdentity, sqlite::OwnedSqliteIndex};

/// Categorical state of one bounded post-commit SQLite maintenance step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalMemoryMaintenanceStep {
    /// The step completed at its named final fence.
    Complete,
    /// The durable mutation receipt is known, but this step was not confirmed.
    Deferred,
}

/// Database-path evidence observed after a committed memory mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalMemoryDatabaseIdentity {
    /// The canonical path still named the exact unique writer-opened file.
    ConfirmedAtFinalFence,
    /// The path, file type, link policy, or file identity changed after commit.
    ChangedAfterCommit,
    /// The final path identity could not be determined safely.
    Unconfirmed,
}

/// Path-free finalization evidence for one committed SQLite memory mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalMemoryMaintenance {
    checkpoint: LocalMemoryMaintenanceStep,
    shutdown: LocalMemoryMaintenanceStep,
    database_identity: LocalMemoryDatabaseIdentity,
}

impl LocalMemoryMaintenance {
    #[cfg(test)]
    #[allow(
        non_upper_case_globals,
        reason = "legacy test spelling remains local to the uncommitted receipt transition"
    )]
    /// Test-only fully confirmed maintenance evidence.
    pub const Complete: Self = Self::from_evidence(
        true,
        true,
        LocalMemoryDatabaseIdentity::ConfirmedAtFinalFence,
    );

    #[cfg(test)]
    #[allow(
        non_upper_case_globals,
        reason = "legacy test spelling remains local to the uncommitted receipt transition"
    )]
    /// Test-only checkpoint-deferred maintenance evidence.
    pub const CheckpointDeferred: Self = Self::from_evidence(
        false,
        true,
        LocalMemoryDatabaseIdentity::ConfirmedAtFinalFence,
    );

    #[cfg(test)]
    #[allow(
        non_upper_case_globals,
        reason = "legacy test spelling remains local to the uncommitted receipt transition"
    )]
    /// Test-only shutdown-deferred maintenance evidence.
    pub const ShutdownDeferred: Self = Self::from_evidence(
        true,
        false,
        LocalMemoryDatabaseIdentity::ConfirmedAtFinalFence,
    );

    #[cfg(test)]
    #[allow(
        non_upper_case_globals,
        reason = "legacy test spelling remains local to the uncommitted receipt transition"
    )]
    /// Test-only checkpoint-and-shutdown-deferred maintenance evidence.
    pub const CheckpointAndShutdownDeferred: Self = Self::from_evidence(
        false,
        false,
        LocalMemoryDatabaseIdentity::ConfirmedAtFinalFence,
    );

    pub(super) const fn from_evidence(
        checkpoint_complete: bool,
        shutdown_complete: bool,
        database_identity: LocalMemoryDatabaseIdentity,
    ) -> Self {
        Self {
            checkpoint: if checkpoint_complete {
                LocalMemoryMaintenanceStep::Complete
            } else {
                LocalMemoryMaintenanceStep::Deferred
            },
            shutdown: if shutdown_complete {
                LocalMemoryMaintenanceStep::Complete
            } else {
                LocalMemoryMaintenanceStep::Deferred
            },
            database_identity,
        }
    }

    #[cfg(test)]
    pub(super) const fn from_completion(
        checkpoint_complete: bool,
        shutdown_complete: bool,
    ) -> Self {
        Self::from_evidence(
            checkpoint_complete,
            shutdown_complete,
            LocalMemoryDatabaseIdentity::ConfirmedAtFinalFence,
        )
    }

    pub(super) const fn pending() -> Self {
        Self::from_evidence(false, false, LocalMemoryDatabaseIdentity::Unconfirmed)
    }

    /// Reports whether every maintenance step and the final identity fence completed.
    #[must_use]
    pub const fn complete(self) -> bool {
        matches!(self.checkpoint, LocalMemoryMaintenanceStep::Complete)
            && matches!(self.shutdown, LocalMemoryMaintenanceStep::Complete)
            && matches!(
                self.database_identity,
                LocalMemoryDatabaseIdentity::ConfirmedAtFinalFence
            )
    }

    /// Returns the terminal WAL-checkpoint status.
    #[must_use]
    pub const fn checkpoint(self) -> LocalMemoryMaintenanceStep {
        self.checkpoint
    }

    /// Returns the writer-shutdown status.
    #[must_use]
    pub const fn shutdown(self) -> LocalMemoryMaintenanceStep {
        self.shutdown
    }

    /// Returns the database-path evidence observed at the final fence.
    #[must_use]
    pub const fn database_identity(self) -> LocalMemoryDatabaseIdentity {
        self.database_identity
    }

    /// Returns the exact number of unconfirmed finalization facts.
    #[must_use]
    pub fn warning_count(self) -> u8 {
        u8::from(matches!(
            self.checkpoint,
            LocalMemoryMaintenanceStep::Deferred
        )) + u8::from(matches!(
            self.shutdown,
            LocalMemoryMaintenanceStep::Deferred
        )) + u8::from(!matches!(
            self.database_identity,
            LocalMemoryDatabaseIdentity::ConfirmedAtFinalFence
        ))
    }
}

/// Writer plus the canonical path and independently retained opened-file identity.
pub(crate) struct OpenedMemoryStore {
    store: OwnedSqliteIndex,
    database: PathBuf,
    opened_identity: FileIdentity,
}

impl OpenedMemoryStore {
    /// Captures a second stable handle that exactly matches the writer-opened file.
    pub(crate) fn from_started(
        database: PathBuf,
        store: OwnedSqliteIndex,
        deadline: Instant,
    ) -> Result<Self, SqliteStoreError> {
        let opened_identity = match database_alias_identity(&database) {
            Ok(Some(identity)) if &identity == store.opened_database_identity() => identity,
            Ok(Some(_)) | Ok(None) | Err(_) => {
                let _ = store.shutdown(deadline);
                return Err(SqliteStoreError::DatabaseIdentityChanged);
            }
        };
        Ok(Self {
            store,
            database,
            opened_identity,
        })
    }

    /// Requests bounded owner shutdown while preserving the captured path evidence.
    pub(crate) fn shutdown(self, deadline: Instant) -> Result<(), SqliteStoreError> {
        self.store.shutdown(deadline)
    }
}

impl Deref for OpenedMemoryStore {
    type Target = OwnedSqliteIndex;

    fn deref(&self) -> &Self::Target {
        &self.store
    }
}

#[cfg(test)]
pub(crate) fn finish_known_memory_mutation<T>(
    store: OwnedSqliteIndex,
    receipt: T,
    deadline: Instant,
) -> (T, LocalMemoryMaintenance) {
    let checkpoint_complete = store.checkpoint(deadline).is_ok_and(|outcome| {
        outcome.busy() == 0 && outcome.checkpointed_frames() == outcome.log_frames()
    });
    let shutdown_complete = store.shutdown(deadline).is_ok();
    (
        receipt,
        LocalMemoryMaintenance::from_completion(checkpoint_complete, shutdown_complete),
    )
}

/// Finalizes a known commit and runs one test seam before its path-identity fence.
pub(crate) fn finish_known_memory_mutation_with_hook<T>(
    store: OpenedMemoryStore,
    receipt: T,
    deadline: Instant,
    after_commit: impl FnOnce(),
) -> (T, LocalMemoryMaintenance) {
    let OpenedMemoryStore {
        store,
        database,
        opened_identity,
    } = store;
    let checkpoint_complete = store.checkpoint(deadline).is_ok_and(|outcome| {
        outcome.busy() == 0 && outcome.checkpointed_frames() == outcome.log_frames()
    });
    let shutdown_complete = store.shutdown(deadline).is_ok();
    after_commit();
    let database_identity = final_database_identity(&database, &opened_identity);
    (
        receipt,
        LocalMemoryMaintenance::from_evidence(
            checkpoint_complete,
            shutdown_complete,
            database_identity,
        ),
    )
}

fn final_database_identity(
    database: &Path,
    opened_identity: &FileIdentity,
) -> LocalMemoryDatabaseIdentity {
    let Some(parent) = database.parent() else {
        return LocalMemoryDatabaseIdentity::Unconfirmed;
    };
    let Some(file_name) = database.file_name() else {
        return LocalMemoryDatabaseIdentity::Unconfirmed;
    };
    let parent = match Dir::open_ambient_dir(parent, ambient_authority()) {
        Ok(parent) => parent,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return LocalMemoryDatabaseIdentity::ChangedAfterCommit;
        }
        Err(_) => return LocalMemoryDatabaseIdentity::Unconfirmed,
    };
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = match parent.open_with(file_name, &options) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return LocalMemoryDatabaseIdentity::ChangedAfterCommit;
        }
        Err(_) => {
            return match parent.symlink_metadata(file_name) {
                Ok(metadata) if !metadata.is_file() => {
                    LocalMemoryDatabaseIdentity::ChangedAfterCommit
                }
                Err(source) if source.kind() == io::ErrorKind::NotFound => {
                    LocalMemoryDatabaseIdentity::ChangedAfterCommit
                }
                Ok(_) | Err(_) => LocalMemoryDatabaseIdentity::Unconfirmed,
            };
        }
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(_) => return LocalMemoryDatabaseIdentity::Unconfirmed,
    };
    if !metadata.is_file() || !has_one_link(&metadata) {
        return LocalMemoryDatabaseIdentity::ChangedAfterCommit;
    }
    match FileIdentity::from_file(file.into_std()) {
        Ok(current) if &current == opened_identity => {
            LocalMemoryDatabaseIdentity::ConfirmedAtFinalFence
        }
        Ok(_) => LocalMemoryDatabaseIdentity::ChangedAfterCommit,
        Err(_) => LocalMemoryDatabaseIdentity::Unconfirmed,
    }
}

#[cfg(unix)]
fn has_one_link(metadata: &Metadata) -> bool {
    use cap_std::fs::MetadataExt as _;

    metadata.nlink() == 1
}

#[cfg(windows)]
fn has_one_link(metadata: &Metadata) -> bool {
    use cap_fs_ext::MetadataExt as _;

    metadata.nlink() == 1
}

#[cfg(not(any(unix, windows)))]
fn has_one_link(_metadata: &Metadata) -> bool {
    false
}
