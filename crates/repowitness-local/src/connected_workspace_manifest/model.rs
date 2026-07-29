use core::fmt;
use std::path::{Path, PathBuf};

use repowitness_application::{PackageScope, ResolvedConfiguration};
use repowitness_domain::{ConnectedWorkspaceId, RepositoryIdentityDigest, SourceSlotId};

use crate::source_selector::SourceSelectorV1;

/// One validated version-1 source tuple.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct ConnectedWorkspaceManifestSourceV1 {
    source_slot: SourceSlotId,
    repository: RepositoryIdentityDigest,
    worktree_root: PathBuf,
    selector: SourceSelectorV1,
    package_scope: PackageScope,
}

impl ConnectedWorkspaceManifestSourceV1 {
    pub(super) fn new(
        source_slot: SourceSlotId,
        repository: RepositoryIdentityDigest,
        worktree_root: PathBuf,
        selector: SourceSelectorV1,
        package_scope: PackageScope,
    ) -> Self {
        Self {
            source_slot,
            repository,
            worktree_root,
            selector,
            package_scope,
        }
    }

    /// Returns the opaque source-slot identity.
    #[must_use]
    pub(crate) const fn source_slot(&self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the logical repository identity.
    #[must_use]
    pub(crate) const fn repository(&self) -> RepositoryIdentityDigest {
        self.repository
    }

    /// Returns the explicitly authorized, lexically resolved worktree root.
    #[must_use]
    pub(crate) fn worktree_root(&self) -> &Path {
        &self.worktree_root
    }

    /// Returns the admitted source selector.
    #[must_use]
    pub(crate) const fn selector(&self) -> &SourceSelectorV1 {
        &self.selector
    }

    /// Returns the validated package scope.
    #[must_use]
    pub(crate) const fn package_scope(&self) -> &PackageScope {
        &self.package_scope
    }
}

impl fmt::Debug for ConnectedWorkspaceManifestSourceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectedWorkspaceManifestSourceV1")
            .field("source_slot", &"<redacted-identity>")
            .field("repository", &"<redacted-identity>")
            .field("worktree_root", &"<redacted-path>")
            .field("selector", &self.selector)
            .field("package_scope", &self.package_scope)
            .finish_non_exhaustive()
    }
}

/// Canonical validated version-1 manifest without ordinary configuration.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct ConnectedWorkspaceManifestV1 {
    connected_workspace: ConnectedWorkspaceId,
    sources: Box<[ConnectedWorkspaceManifestSourceV1]>,
}

impl ConnectedWorkspaceManifestV1 {
    pub(super) fn new(
        connected_workspace: ConnectedWorkspaceId,
        sources: Box<[ConnectedWorkspaceManifestSourceV1]>,
    ) -> Self {
        Self {
            connected_workspace,
            sources,
        }
    }

    /// Returns the explicit connected-workspace identity.
    #[must_use]
    pub(crate) const fn connected_workspace(&self) -> ConnectedWorkspaceId {
        self.connected_workspace
    }

    /// Returns sources in canonical exact source-slot order.
    #[must_use]
    pub(crate) fn sources(&self) -> &[ConnectedWorkspaceManifestSourceV1] {
        &self.sources
    }

    /// Attaches exactly one shared resolved configuration without per-slot cloning.
    #[must_use]
    pub(crate) fn with_configuration(
        self,
        configuration: ResolvedConfiguration,
    ) -> ConfiguredConnectedWorkspaceManifestV1 {
        ConfiguredConnectedWorkspaceManifestV1 {
            manifest: self,
            configuration,
        }
    }
}

impl fmt::Debug for ConnectedWorkspaceManifestV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectedWorkspaceManifestV1")
            .field(
                "schema_version",
                &super::CONNECTED_WORKSPACE_MANIFEST_SCHEMA_VERSION,
            )
            .field("connected_workspace", &"<redacted-identity>")
            .field("source_count", &self.sources.len())
            .finish_non_exhaustive()
    }
}

/// Coordinator composition with one shared resolved configuration.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct ConfiguredConnectedWorkspaceManifestV1 {
    manifest: ConnectedWorkspaceManifestV1,
    configuration: ResolvedConfiguration,
}

impl ConfiguredConnectedWorkspaceManifestV1 {
    /// Returns the validated manifest.
    #[must_use]
    #[cfg(test)]
    pub(crate) const fn manifest(&self) -> &ConnectedWorkspaceManifestV1 {
        &self.manifest
    }

    /// Returns the one shared configuration for every source slot.
    #[must_use]
    #[cfg(test)]
    pub(crate) const fn configuration(&self) -> &ResolvedConfiguration {
        &self.configuration
    }

    /// Consumes the composition without cloning configuration per source.
    #[must_use]
    pub(crate) fn into_parts(self) -> (ConnectedWorkspaceManifestV1, ResolvedConfiguration) {
        (self.manifest, self.configuration)
    }
}

impl fmt::Debug for ConfiguredConnectedWorkspaceManifestV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConfiguredConnectedWorkspaceManifestV1")
            .field("manifest", &self.manifest)
            .field("configuration_digest", &self.configuration.digest())
            .finish_non_exhaustive()
    }
}
