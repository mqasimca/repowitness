use std::{error::Error, fmt, path::Path, time::Duration};

use repowitness_application::{PackageScope, ResolvedConfiguration};
use repowitness_domain::{
    ConfigurationDigest, ConnectedWorkspaceId, RepositoryIdentityDigest, SourceSlotId,
};

#[cfg(test)]
use crate::source_selector::SourceSelectorAdmissionError;
use crate::{
    GenerationId, LocalIndexError, LocalRustIndexLimits, SqliteStoreError, WorkspaceViewId,
    source_selector::{SourceSelectorLimits, SourceSelectorResolutionError, SourceSelectorV1},
};

use super::{ConnectedSourceSlotFinalFenceError, ConnectedWorkspaceViewDigest};
use crate::local_index::post_commit::PostCommitMaintenanceStatus;

/// One validated, caller-authorized connected source slot.
#[derive(Clone)]
pub(crate) struct ConnectedSourceSlotRequest<'a> {
    source_slot: SourceSlotId,
    repository: RepositoryIdentityDigest,
    worktree: &'a Path,
    selector: SourceSelectorV1,
    package_scope: PackageScope,
    configuration: &'a ResolvedConfiguration,
    limits: LocalRustIndexLimits,
    selector_limits: SourceSelectorLimits,
    deadline: Duration,
}

impl<'a> ConnectedSourceSlotRequest<'a> {
    #[cfg(test)]
    #[allow(
        clippy::too_many_arguments,
        reason = "test admission covers slot identity, source authority, policy, and bounds"
    )]
    pub(crate) fn try_new(
        source_slot: SourceSlotId,
        repository: RepositoryIdentityDigest,
        worktree: &'a Path,
        selector_text: &str,
        package_scope: PackageScope,
        configuration: &'a ResolvedConfiguration,
        limits: LocalRustIndexLimits,
        selector_limits: SourceSelectorLimits,
        deadline: Duration,
    ) -> Result<Self, ConnectedWorkspaceRequestError> {
        let selector = SourceSelectorV1::parse(selector_text)
            .map_err(|source| ConnectedWorkspaceRequestError::Selector { source })?;
        Self::try_from_validated(
            source_slot,
            repository,
            worktree,
            selector,
            package_scope,
            configuration,
            limits,
            selector_limits,
            deadline,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "validated slot identity, source authority, policy, and bounds remain explicit"
    )]
    pub(crate) fn try_from_validated(
        source_slot: SourceSlotId,
        repository: RepositoryIdentityDigest,
        worktree: &'a Path,
        selector: SourceSelectorV1,
        package_scope: PackageScope,
        configuration: &'a ResolvedConfiguration,
        limits: LocalRustIndexLimits,
        selector_limits: SourceSelectorLimits,
        deadline: Duration,
    ) -> Result<Self, ConnectedWorkspaceRequestError> {
        if deadline.is_zero() || limits.deadline().is_zero() || selector_limits.deadline().is_zero()
        {
            return Err(ConnectedWorkspaceRequestError::ZeroDeadline);
        }
        Ok(Self {
            source_slot,
            repository,
            worktree,
            selector,
            package_scope,
            configuration,
            limits,
            selector_limits,
            deadline,
        })
    }

    pub(super) const fn source_slot(&self) -> SourceSlotId {
        self.source_slot
    }

    pub(super) const fn repository(&self) -> RepositoryIdentityDigest {
        self.repository
    }

    pub(super) const fn worktree(&self) -> &Path {
        self.worktree
    }

    pub(super) const fn selector(&self) -> &SourceSelectorV1 {
        &self.selector
    }

    pub(super) const fn package_scope(&self) -> &PackageScope {
        &self.package_scope
    }

    pub(super) const fn configuration(&self) -> &ResolvedConfiguration {
        self.configuration
    }

    pub(super) const fn limits(&self) -> LocalRustIndexLimits {
        self.limits
    }

    pub(super) const fn selector_limits(&self) -> SourceSelectorLimits {
        self.selector_limits
    }

    pub(super) const fn deadline(&self) -> Duration {
        self.deadline
    }
}

impl fmt::Debug for ConnectedSourceSlotRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectedSourceSlotRequest")
            .field("source_slot", &"<redacted-identity>")
            .field("repository", &"<redacted-identity>")
            .field("worktree", &"<redacted-path>")
            .field("selector", &self.selector)
            .field("package_scope", &self.package_scope)
            .field("configuration_digest", &self.configuration.digest())
            .field("limits", &self.limits)
            .field("selector_limits", &self.selector_limits)
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// One validated bounded connected-workspace indexing request.
pub(crate) struct ConnectedWorkspaceIndexRequest<'a> {
    connected_workspace: ConnectedWorkspaceId,
    database: &'a Path,
    migration_applied_at_unix_ms: u64,
    deadline: Duration,
    configuration_digest: ConfigurationDigest,
    source_slots: Box<[ConnectedSourceSlotRequest<'a>]>,
}

impl<'a> ConnectedWorkspaceIndexRequest<'a> {
    pub(crate) fn try_new(
        connected_workspace: ConnectedWorkspaceId,
        database: &'a Path,
        migration_applied_at_unix_ms: u64,
        deadline: Duration,
        mut source_slots: Vec<ConnectedSourceSlotRequest<'a>>,
    ) -> Result<Self, ConnectedWorkspaceRequestError> {
        if deadline.is_zero() {
            return Err(ConnectedWorkspaceRequestError::ZeroDeadline);
        }
        if source_slots.is_empty() {
            return Err(ConnectedWorkspaceRequestError::EmptySourceSlots);
        }
        if source_slots.len() > crate::MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS {
            return Err(ConnectedWorkspaceRequestError::SourceSlotLimitExceeded {
                limit: crate::MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS,
            });
        }
        if source_slots
            .iter()
            .try_fold(0_u64, |total, slot| {
                total.checked_add(slot.limits.discovery().paths())
            })
            .is_none()
        {
            return Err(ConnectedWorkspaceRequestError::AggregatePathLimitExceeded);
        }
        let configuration_digest = source_slots[0].configuration().digest();
        if source_slots
            .iter()
            .any(|slot| slot.configuration().digest() != configuration_digest)
        {
            return Err(ConnectedWorkspaceRequestError::MixedConfiguration);
        }
        if source_slots.iter().any(|slot| {
            connected_workspace == ConnectedWorkspaceId::for_single_repository(slot.repository())
        }) {
            return Err(ConnectedWorkspaceRequestError::ReservedCompatibilityWorkspace);
        }
        source_slots.sort_unstable_by_key(ConnectedSourceSlotRequest::source_slot);
        let compatibility_slots = source_slots
            .iter()
            .map(|slot| SourceSlotId::for_repository(slot.repository()))
            .collect::<std::collections::BTreeSet<_>>();
        if source_slots
            .iter()
            .any(|slot| compatibility_slots.contains(&slot.source_slot()))
        {
            return Err(ConnectedWorkspaceRequestError::ReservedCompatibilitySourceSlot);
        }
        if source_slots
            .windows(2)
            .any(|pair| pair[0].source_slot() == pair[1].source_slot())
        {
            return Err(ConnectedWorkspaceRequestError::DuplicateSourceSlot);
        }
        Ok(Self {
            connected_workspace,
            database,
            migration_applied_at_unix_ms,
            deadline,
            configuration_digest,
            source_slots: source_slots.into_boxed_slice(),
        })
    }

    pub(super) const fn connected_workspace(&self) -> ConnectedWorkspaceId {
        self.connected_workspace
    }

    pub(super) const fn database(&self) -> &Path {
        self.database
    }

    pub(super) const fn migration_applied_at_unix_ms(&self) -> u64 {
        self.migration_applied_at_unix_ms
    }

    pub(super) const fn deadline(&self) -> Duration {
        self.deadline
    }

    pub(super) const fn configuration_digest(&self) -> ConfigurationDigest {
        self.configuration_digest
    }

    pub(super) const fn source_slots(&self) -> &[ConnectedSourceSlotRequest<'a>] {
        &self.source_slots
    }
}

impl fmt::Debug for ConnectedWorkspaceIndexRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectedWorkspaceIndexRequest")
            .field("connected_workspace", &"<redacted-identity>")
            .field("database", &"<redacted-path>")
            .field(
                "migration_applied_at_unix_ms",
                &self.migration_applied_at_unix_ms,
            )
            .field("deadline", &self.deadline)
            .field("configuration_digest", &self.configuration_digest)
            .field("source_slots", &self.source_slots)
            .finish()
    }
}

/// Stable validation failure for a connected-workspace request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectedWorkspaceRequestError {
    EmptySourceSlots,
    SourceSlotLimitExceeded {
        limit: usize,
    },
    DuplicateSourceSlot,
    ReservedCompatibilityWorkspace,
    ReservedCompatibilitySourceSlot,
    MixedConfiguration,
    ZeroDeadline,
    AggregatePathLimitExceeded,
    #[cfg(test)]
    Selector {
        source: SourceSelectorAdmissionError,
    },
}

impl fmt::Display for ConnectedWorkspaceRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptySourceSlots => "connected workspace requires at least one source slot",
            Self::SourceSlotLimitExceeded { .. } => {
                "connected workspace exceeds its source-slot limit"
            }
            Self::DuplicateSourceSlot => "connected workspace contains a duplicate source slot",
            Self::ReservedCompatibilityWorkspace => {
                "connected workspace identity collides with a compatibility identity"
            }
            Self::ReservedCompatibilitySourceSlot => {
                "connected workspace source slot collides with a compatibility identity"
            }
            Self::MixedConfiguration => {
                "connected workspace requires one shared resolved configuration"
            }
            Self::ZeroDeadline => "connected workspace deadlines must be positive",
            Self::AggregatePathLimitExceeded => {
                "connected workspace aggregate path limit is not representable"
            }
            #[cfg(test)]
            Self::Selector { .. } => "connected workspace source selector is invalid",
        })
    }
}

impl Error for ConnectedWorkspaceRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            #[cfg(test)]
            Self::Selector { source } => Some(source),
            _ => None,
        }
    }
}

/// Stable failure phase for connected-workspace indexing.
#[derive(Debug)]
pub(crate) enum ConnectedWorkspaceIndexError {
    DeadlineNotRepresentable,
    Cancelled,
    DeadlineExceeded,
    ManifestParentAuthority {
        source: crate::BoundedFileReadError,
    },
    SelectorResolution {
        slot_ordinal: u64,
        source: SourceSelectorResolutionError,
    },
    DatabaseIsolation {
        source: LocalIndexError,
    },
    Preparation {
        slot_ordinal: u64,
        source: LocalIndexError,
    },
    StoreStartup {
        source: SqliteStoreError,
    },
    WorkspaceRegistration {
        source: SqliteStoreError,
    },
    PublicationStaging {
        slot_ordinal: u64,
        source: SqliteStoreError,
    },
    GraphPublicationStaging {
        slot_ordinal: u64,
        source: SqliteStoreError,
    },
    FinalSourceFence {
        slot_ordinal: u64,
        source: ConnectedSourceSlotFinalFenceError,
    },
    Completion {
        slot_ordinal: u64,
        source: SqliteStoreError,
    },
    ViewPublication {
        source: SqliteStoreError,
    },
}

impl fmt::Display for ConnectedWorkspaceIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::DeadlineNotRepresentable => "connected-workspace deadline is not representable",
            Self::Cancelled => "connected-workspace indexing was cancelled",
            Self::DeadlineExceeded => "connected-workspace indexing exceeded its deadline",
            Self::ManifestParentAuthority { .. } => {
                "connected-workspace manifest parent authority changed"
            }
            Self::SelectorResolution { .. } => "source-slot selector resolution failed",
            Self::DatabaseIsolation { .. } => "connected-workspace database isolation failed",
            Self::Preparation { .. } => "connected-workspace source preparation failed",
            Self::StoreStartup { .. } => "connected-workspace store startup failed",
            Self::WorkspaceRegistration { .. } => {
                "connected-workspace source-slot registration failed"
            }
            Self::PublicationStaging { .. } => {
                "connected-workspace source generation staging failed"
            }
            Self::GraphPublicationStaging { .. } => {
                "connected-workspace graph generation staging failed"
            }
            Self::FinalSourceFence { .. } => "connected-workspace final source fence failed",
            Self::Completion { .. } => "connected-workspace source-slot completion failed",
            Self::ViewPublication { .. } => "connected-workspace view publication failed",
        })
    }
}

impl Error for ConnectedWorkspaceIndexError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ManifestParentAuthority { source } => Some(source),
            Self::SelectorResolution { source, .. } => Some(source),
            Self::DatabaseIsolation { source } | Self::Preparation { source, .. } => Some(source),
            Self::StoreStartup { source }
            | Self::WorkspaceRegistration { source }
            | Self::PublicationStaging { source, .. }
            | Self::GraphPublicationStaging { source, .. }
            | Self::Completion { source, .. }
            | Self::ViewPublication { source } => Some(source),
            Self::FinalSourceFence { source, .. } => Some(source),
            Self::DeadlineNotRepresentable | Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Non-sensitive outcome for one published connected source slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConnectedSourceSlotReport {
    source_slot: SourceSlotId,
    generation: GenerationId,
    discovered_paths: u64,
    indexed_files: u64,
    skipped_paths: u64,
    skipped_policy_paths: u64,
    skipped_unsupported_paths: u64,
    reused_files: u64,
    analyzed_files: u64,
}

impl ConnectedSourceSlotReport {
    #[allow(
        clippy::too_many_arguments,
        reason = "fixed-width slot publication totals remain explicit"
    )]
    pub(super) const fn new(
        source_slot: SourceSlotId,
        generation: GenerationId,
        discovered_paths: u64,
        indexed_files: u64,
        skipped_paths: u64,
        skipped_policy_paths: u64,
        skipped_unsupported_paths: u64,
        reused_files: u64,
        analyzed_files: u64,
    ) -> Self {
        Self {
            source_slot,
            generation,
            discovered_paths,
            indexed_files,
            skipped_paths,
            skipped_policy_paths,
            skipped_unsupported_paths,
            reused_files,
            analyzed_files,
        }
    }

    #[cfg(test)]
    pub(crate) const fn source_slot(self) -> SourceSlotId {
        self.source_slot
    }

    pub(crate) const fn generation(self) -> GenerationId {
        self.generation
    }

    pub(crate) const fn discovered_paths(self) -> u64 {
        self.discovered_paths
    }

    pub(crate) const fn indexed_files(self) -> u64 {
        self.indexed_files
    }

    #[cfg(test)]
    pub(crate) const fn skipped_paths(self) -> u64 {
        self.skipped_paths
    }

    pub(crate) const fn skipped_policy_paths(self) -> u64 {
        self.skipped_policy_paths
    }

    pub(crate) const fn skipped_unsupported_paths(self) -> u64 {
        self.skipped_unsupported_paths
    }

    pub(crate) const fn reused_files(self) -> u64 {
        self.reused_files
    }

    pub(crate) const fn analyzed_files(self) -> u64 {
        self.analyzed_files
    }
}

/// Non-sensitive outcome from one atomic connected-workspace view publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConnectedWorkspaceIndexReport {
    view: WorkspaceViewId,
    recovered_generations: u64,
    configuration_digest: ConfigurationDigest,
    view_receipt_digest: ConnectedWorkspaceViewDigest,
    maintenance: PostCommitMaintenanceStatus,
    source_slots: Box<[ConnectedSourceSlotReport]>,
}

impl ConnectedWorkspaceIndexReport {
    pub(super) fn new(
        view: WorkspaceViewId,
        recovered_generations: u64,
        configuration_digest: ConfigurationDigest,
        view_receipt_digest: ConnectedWorkspaceViewDigest,
        maintenance: PostCommitMaintenanceStatus,
        source_slots: Vec<ConnectedSourceSlotReport>,
    ) -> Self {
        Self {
            view,
            recovered_generations,
            configuration_digest,
            view_receipt_digest,
            maintenance,
            source_slots: source_slots.into_boxed_slice(),
        }
    }

    #[cfg(test)]
    pub(crate) const fn view(&self) -> WorkspaceViewId {
        self.view
    }

    pub(crate) const fn recovered_generations(&self) -> u64 {
        self.recovered_generations
    }

    pub(crate) const fn configuration_digest(&self) -> ConfigurationDigest {
        self.configuration_digest
    }

    pub(crate) const fn view_receipt_digest(&self) -> ConnectedWorkspaceViewDigest {
        self.view_receipt_digest
    }

    pub(crate) const fn maintenance(&self) -> PostCommitMaintenanceStatus {
        self.maintenance
    }

    pub(crate) const fn source_slots(&self) -> &[ConnectedSourceSlotReport] {
        &self.source_slots
    }
}
