use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

use repowitness_application::{RustSourceSnapshotIdentity, SourceSlotFinalFence};
use repowitness_domain::SourceSnapshotDigest;

use crate::{
    LocalRustIndexLimits,
    contained_source::FileIdentity,
    rust_index::{
        LocalSourceSnapshotFenceError, LocalSourceSnapshotFenceRequest, SourceLanguageSelection,
        confirm_local_source_snapshot,
    },
};

use super::database_alias_identity;

pub(super) struct LocalSourceSlotFinalFence<'a> {
    worktree: &'a Path,
    database: &'a Path,
    database_identity: Option<&'a FileIdentity>,
    identity: RustSourceSnapshotIdentity,
    languages: SourceLanguageSelection,
    limits: LocalRustIndexLimits,
}

impl<'a> LocalSourceSlotFinalFence<'a> {
    pub(super) const fn new(
        worktree: &'a Path,
        database: &'a Path,
        database_identity: Option<&'a FileIdentity>,
        identity: RustSourceSnapshotIdentity,
        languages: SourceLanguageSelection,
        limits: LocalRustIndexLimits,
    ) -> Self {
        Self {
            worktree,
            database,
            database_identity,
            identity,
            languages,
            limits,
        }
    }
}

impl SourceSlotFinalFence for LocalSourceSlotFinalFence<'_> {
    type Error = LocalSourceSnapshotFenceError;

    fn confirm_source_snapshot(
        &self,
        expected: SourceSnapshotDigest,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<(), Self::Error> {
        confirm_local_source_snapshot(LocalSourceSnapshotFenceRequest::new(
            self.worktree,
            self.identity,
            expected,
            self.languages,
            self.limits,
            cancelled.as_ref(),
            deadline,
            self.database_identity,
        ))?;
        let confirmed_database_identity = database_alias_identity(self.database)
            .map_err(|_| LocalSourceSnapshotFenceError::SourceChanged)?;
        if confirmed_database_identity.as_ref() != self.database_identity {
            return Err(LocalSourceSnapshotFenceError::SourceChanged);
        }
        Ok(())
    }
}
