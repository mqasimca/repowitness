/// Identity confirmation for a path involved in canonical memory publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryFileIdentityStatus {
    /// The path still named the authorized file or directory at the final fence.
    ConfirmedAtFinalFence,
    /// Publication committed, but the path identity was no longer confirmed.
    ChangedAfterCommit,
}

/// Categorical state of one post-publication file operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryFilePublicationStepStatus {
    /// The operation does not apply to this publication mode.
    NotRequired,
    /// The operation completed and was confirmed at the final fence.
    Complete,
    /// The canonical target was published, but this operation was not confirmed.
    Deferred,
}

/// Post-commit truth for one canonical memory-file publication.
///
/// A changed identity or deferred step is a warning about the state observed
/// after the atomic publish syscall. It never means that publication rolled
/// back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalMemoryFilePublicationStatus {
    temporary_cleanup: MemoryFilePublicationStepStatus,
    target_identity: MemoryFileIdentityStatus,
    records_directory_identity: MemoryFileIdentityStatus,
    directory_sync: MemoryFilePublicationStepStatus,
}

impl LocalMemoryFilePublicationStatus {
    pub(super) const fn new(
        temporary_cleanup: MemoryFilePublicationStepStatus,
        target_identity: MemoryFileIdentityStatus,
        records_directory_identity: MemoryFileIdentityStatus,
        directory_sync: MemoryFilePublicationStepStatus,
    ) -> Self {
        Self {
            temporary_cleanup,
            target_identity,
            records_directory_identity,
            directory_sync,
        }
    }

    /// Returns the private temporary-file cleanup state.
    #[must_use]
    pub const fn temporary_cleanup(self) -> MemoryFilePublicationStepStatus {
        self.temporary_cleanup
    }

    /// Returns the canonical-target identity observed at its final fence.
    #[must_use]
    pub const fn target_identity(self) -> MemoryFileIdentityStatus {
        self.target_identity
    }

    /// Returns the records-directory identity observed at the final fence.
    #[must_use]
    pub const fn records_directory_identity(self) -> MemoryFileIdentityStatus {
        self.records_directory_identity
    }

    /// Returns the containing-directory synchronization state.
    #[must_use]
    pub const fn directory_sync(self) -> MemoryFilePublicationStepStatus {
        self.directory_sync
    }

    /// Reports whether every applicable post-publication operation completed.
    #[must_use]
    pub const fn is_complete(self) -> bool {
        !matches!(
            self.temporary_cleanup,
            MemoryFilePublicationStepStatus::Deferred
        ) && matches!(
            self.target_identity,
            MemoryFileIdentityStatus::ConfirmedAtFinalFence
        ) && matches!(
            self.records_directory_identity,
            MemoryFileIdentityStatus::ConfirmedAtFinalFence
        ) && !matches!(
            self.directory_sync,
            MemoryFilePublicationStepStatus::Deferred
        )
    }

    /// Returns the number of categorical post-publication warnings.
    #[must_use]
    pub const fn warning_count(self) -> u8 {
        let mut warnings = 0;
        if matches!(
            self.temporary_cleanup,
            MemoryFilePublicationStepStatus::Deferred
        ) {
            warnings += 1;
        }
        if matches!(
            self.target_identity,
            MemoryFileIdentityStatus::ChangedAfterCommit
        ) {
            warnings += 1;
        }
        if matches!(
            self.records_directory_identity,
            MemoryFileIdentityStatus::ChangedAfterCommit
        ) {
            warnings += 1;
        }
        if matches!(
            self.directory_sync,
            MemoryFilePublicationStepStatus::Deferred
        ) {
            warnings += 1;
        }
        warnings
    }
}
