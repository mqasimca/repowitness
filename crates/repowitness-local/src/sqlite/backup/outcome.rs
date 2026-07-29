/// Identity confirmation for a path involved in a published backup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupIdentityStatus {
    /// The path still named the authorized file at the final identity fence.
    ConfirmedAtFinalFence,
    /// Publication committed, but the path identity was no longer confirmed.
    ChangedAfterCommit,
}

/// Categorical state of backup maintenance attempted after publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupMaintenanceStatus {
    /// The maintenance operation completed at the final fence.
    Complete,
    /// Publication committed, but the maintenance operation was not confirmed.
    Deferred,
}

/// Post-commit truth for one published online backup.
///
/// A changed identity or deferred maintenance step is a warning. It never
/// means that the no-clobber destination publication was rolled back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupPublicationStatus {
    source_identity: BackupIdentityStatus,
    destination_identity: BackupIdentityStatus,
    temporary_cleanup: BackupMaintenanceStatus,
    directory_sync: BackupMaintenanceStatus,
}

impl BackupPublicationStatus {
    pub(super) const fn new(
        source_identity: BackupIdentityStatus,
        destination_identity: BackupIdentityStatus,
        temporary_cleanup: BackupMaintenanceStatus,
        directory_sync: BackupMaintenanceStatus,
    ) -> Self {
        Self {
            source_identity,
            destination_identity,
            temporary_cleanup,
            directory_sync,
        }
    }

    /// Returns the source database identity observed at the final fence.
    #[must_use]
    pub const fn source_identity(self) -> BackupIdentityStatus {
        self.source_identity
    }

    /// Returns the destination identity observed at the final fence.
    #[must_use]
    pub const fn destination_identity(self) -> BackupIdentityStatus {
        self.destination_identity
    }

    /// Returns the private temporary-link cleanup state.
    #[must_use]
    pub const fn temporary_cleanup(self) -> BackupMaintenanceStatus {
        self.temporary_cleanup
    }

    /// Returns the destination-directory synchronization state.
    #[must_use]
    pub const fn directory_sync(self) -> BackupMaintenanceStatus {
        self.directory_sync
    }

    /// Reports whether every post-publication identity and maintenance fence passed.
    #[must_use]
    pub const fn is_complete(self) -> bool {
        matches!(
            self.source_identity,
            BackupIdentityStatus::ConfirmedAtFinalFence
        ) && matches!(
            self.destination_identity,
            BackupIdentityStatus::ConfirmedAtFinalFence
        ) && matches!(self.temporary_cleanup, BackupMaintenanceStatus::Complete)
            && matches!(self.directory_sync, BackupMaintenanceStatus::Complete)
    }

    /// Returns the number of categorical post-publication warnings.
    #[must_use]
    pub const fn warning_count(self) -> u8 {
        let mut warnings = 0;
        if matches!(
            self.source_identity,
            BackupIdentityStatus::ChangedAfterCommit
        ) {
            warnings += 1;
        }
        if matches!(
            self.destination_identity,
            BackupIdentityStatus::ChangedAfterCommit
        ) {
            warnings += 1;
        }
        if matches!(self.temporary_cleanup, BackupMaintenanceStatus::Deferred) {
            warnings += 1;
        }
        if matches!(self.directory_sync, BackupMaintenanceStatus::Deferred) {
            warnings += 1;
        }
        warnings
    }
}
