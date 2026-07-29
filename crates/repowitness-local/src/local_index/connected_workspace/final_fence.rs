use std::{
    error::Error,
    fmt,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

use repowitness_application::{PackageScope, RustSourceSnapshotIdentity, SourceSlotFinalFence};
use repowitness_domain::SourceSnapshotDigest;

use crate::{
    LocalRustIndexLimits,
    contained_source::FileIdentity,
    rust_index::{
        LocalSourceSnapshotFenceError, LocalSourceSnapshotFenceRequest, SourceLanguageSelection,
        confirm_local_source_snapshot,
    },
    source_selector::{
        ResolvedSourceSelector, SourceSelectorFinalFenceError, SourceSelectorLimits,
    },
};

use super::super::database_alias_identity;

pub(super) struct ConnectedSourceSlotFinalFence<'a> {
    worktree: &'a Path,
    database: &'a Path,
    database_identity: Option<&'a FileIdentity>,
    resolved_selector: &'a ResolvedSourceSelector,
    selector_limits: SourceSelectorLimits,
    package_scope: &'a PackageScope,
    identity: RustSourceSnapshotIdentity,
    languages: SourceLanguageSelection,
    limits: LocalRustIndexLimits,
}

impl<'a> ConnectedSourceSlotFinalFence<'a> {
    #[allow(
        clippy::too_many_arguments,
        reason = "selector, scope, source identity, and database authority remain explicit"
    )]
    pub(super) const fn new(
        worktree: &'a Path,
        database: &'a Path,
        database_identity: Option<&'a FileIdentity>,
        resolved_selector: &'a ResolvedSourceSelector,
        selector_limits: SourceSelectorLimits,
        package_scope: &'a PackageScope,
        identity: RustSourceSnapshotIdentity,
        languages: SourceLanguageSelection,
        limits: LocalRustIndexLimits,
    ) -> Self {
        Self {
            worktree,
            database,
            database_identity,
            resolved_selector,
            selector_limits,
            package_scope,
            identity,
            languages,
            limits,
        }
    }
}

impl SourceSlotFinalFence for ConnectedSourceSlotFinalFence<'_> {
    type Error = ConnectedSourceSlotFinalFenceError;

    fn confirm_source_snapshot(
        &self,
        expected: SourceSnapshotDigest,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<(), Self::Error> {
        self.resolved_selector
            .confirm(self.selector_limits, cancelled.as_ref(), deadline)
            .map_err(ConnectedSourceSlotFinalFenceError::Selector)?;
        confirm_local_source_snapshot(LocalSourceSnapshotFenceRequest::new_scoped(
            self.worktree,
            self.identity,
            expected,
            self.languages,
            self.package_scope,
            self.limits,
            cancelled.as_ref(),
            deadline,
            self.database_identity,
        ))
        .map_err(ConnectedSourceSlotFinalFenceError::Source)?;
        let confirmed_database_identity = database_alias_identity(self.database)
            .map_err(|_| ConnectedSourceSlotFinalFenceError::DatabaseChanged)?;
        if confirmed_database_identity.as_ref() != self.database_identity {
            return Err(ConnectedSourceSlotFinalFenceError::DatabaseChanged);
        }
        Ok(())
    }
}

/// Stable selector-, source-, and path-redacted connected final-fence failure.
#[derive(Debug)]
pub(crate) enum ConnectedSourceSlotFinalFenceError {
    Selector(SourceSelectorFinalFenceError),
    Source(LocalSourceSnapshotFenceError),
    DatabaseChanged,
}

impl fmt::Display for ConnectedSourceSlotFinalFenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Selector(_) => "source selector changed before completion",
            Self::Source(_) => "scoped source snapshot changed before completion",
            Self::DatabaseChanged => "database identity changed before completion",
        })
    }
}

impl Error for ConnectedSourceSlotFinalFenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Selector(source) => Some(source),
            Self::Source(source) => Some(source),
            Self::DatabaseChanged => None,
        }
    }
}
