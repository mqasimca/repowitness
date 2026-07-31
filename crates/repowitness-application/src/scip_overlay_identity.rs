//! Canonical immutable identity for one admitted SCIP precision overlay.

use crate::SourceSlotEpoch;
use repowitness_analysis::{
    SCIP_OVERLAY_IMPORTER_VERSION, SCIP_SCHEMA_REVISION, SCIP_SCHEMA_SHA256,
};
use repowitness_domain::{
    ConfigurationDigest, ConnectedWorkspaceId, ProducerManifestDigest, ScipImporterDigest,
    ScipInputDigest, ScipOverlayDigest, ScipSchemaDigest, SourceManifestDigest, SourceSlotId,
    SourceSnapshotDigest,
};
use sha2::{Digest, Sha256};

/// Version of the canonical immutable SCIP overlay identity encoding.
pub const SCIP_OVERLAY_IDENTITY_VERSION: u16 = 1;
const OVERLAY_IDENTITY_DOMAIN: &[u8] = b"repowitness.scip-overlay.identity\0";
const IMPORTER_IDENTITY_DOMAIN: &[u8] = b"repowitness.scip-overlay.importer\0";

/// Exact immutable workspace member scope selected for one overlay import.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScipOverlayScopeIdentity {
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: i64,
    source_slot: SourceSlotId,
    source_epoch: SourceSlotEpoch,
    generation: i64,
}

impl ScipOverlayScopeIdentity {
    /// Creates an exact non-local workspace-view and generation scope.
    pub const fn new(
        connected_workspace: ConnectedWorkspaceId,
        workspace_view: i64,
        source_slot: SourceSlotId,
        source_epoch: SourceSlotEpoch,
        generation: i64,
    ) -> Result<Self, ScipOverlayScopeIdentityError> {
        if workspace_view <= 0 || generation <= 0 {
            return Err(ScipOverlayScopeIdentityError::InvalidDatabaseIdentity);
        }
        Ok(Self {
            connected_workspace,
            workspace_view,
            source_slot,
            source_epoch,
            generation,
        })
    }

    /// Returns the exact connected workspace.
    #[must_use]
    pub const fn connected_workspace(self) -> ConnectedWorkspaceId {
        self.connected_workspace
    }
    /// Returns the positive immutable view identity.
    #[must_use]
    pub const fn workspace_view(self) -> i64 {
        self.workspace_view
    }
    /// Returns the selected source slot.
    #[must_use]
    pub const fn source_slot(self) -> SourceSlotId {
        self.source_slot
    }
    /// Returns the selected completed source-slot epoch.
    #[must_use]
    pub const fn source_epoch(self) -> SourceSlotEpoch {
        self.source_epoch
    }
    /// Returns the selected positive immutable generation identity.
    #[must_use]
    pub const fn generation(self) -> i64 {
        self.generation
    }
}

/// Invalid non-local persistent identifiers at the overlay identity boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScipOverlayScopeIdentityError {
    /// A workspace-view or generation identity was non-positive.
    InvalidDatabaseIdentity,
}

/// Exact immutable inputs whose change prevents an overlay from being reused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScipOverlayIdentityInput {
    scope: ScipOverlayScopeIdentity,
    source_snapshot: SourceSnapshotDigest,
    source_manifest: SourceManifestDigest,
    configuration: ConfigurationDigest,
    producer: ProducerManifestDigest,
    schema: ScipSchemaDigest,
    importer: ScipImporterDigest,
    input: ScipInputDigest,
}

impl ScipOverlayIdentityInput {
    /// Constructs every independently semantics-affecting overlay identity component.
    #[allow(
        clippy::too_many_arguments,
        reason = "every persisted overlay-reuse input remains explicit"
    )]
    #[must_use]
    pub const fn new(
        scope: ScipOverlayScopeIdentity,
        source_snapshot: SourceSnapshotDigest,
        source_manifest: SourceManifestDigest,
        configuration: ConfigurationDigest,
        producer: ProducerManifestDigest,
        schema: ScipSchemaDigest,
        importer: ScipImporterDigest,
        input: ScipInputDigest,
    ) -> Self {
        Self {
            scope,
            source_snapshot,
            source_manifest,
            configuration,
            producer,
            schema,
            importer,
            input,
        }
    }

    /// Returns the exact workspace member scope that owns this overlay.
    #[must_use]
    pub const fn scope(self) -> ScipOverlayScopeIdentity {
        self.scope
    }

    /// Returns the exact pinned source snapshot.
    #[must_use]
    pub const fn source_snapshot(self) -> SourceSnapshotDigest {
        self.source_snapshot
    }

    /// Returns the exact canonical source manifest.
    #[must_use]
    pub const fn source_manifest(self) -> SourceManifestDigest {
        self.source_manifest
    }

    /// Returns the resolved semantics-affecting configuration identity.
    #[must_use]
    pub const fn configuration(self) -> ConfigurationDigest {
        self.configuration
    }

    /// Returns the bounded producer provenance identity.
    #[must_use]
    pub const fn producer(self) -> ProducerManifestDigest {
        self.producer
    }

    /// Returns the reviewed exact protocol/schema identity.
    #[must_use]
    pub const fn schema(self) -> ScipSchemaDigest {
        self.schema
    }

    /// Returns the importer implementation identity.
    #[must_use]
    pub const fn importer(self) -> ScipImporterDigest {
        self.importer
    }

    /// Returns the exact hostile SCIP artifact digest.
    #[must_use]
    pub const fn input(self) -> ScipInputDigest {
        self.input
    }
}

/// Hashes exact hostile SCIP input bytes without retaining them.
#[must_use]
pub fn hash_scip_input(input: &[u8]) -> ScipInputDigest {
    ScipInputDigest::new(Sha256::digest(input).into())
}

/// Returns the schema digest for the exact reviewed SCIP schema revision.
#[must_use]
pub const fn reviewed_scip_schema_digest() -> ScipSchemaDigest {
    ScipSchemaDigest::new(SCIP_SCHEMA_SHA256)
}

/// Returns the identity of this bounded importer implementation.
#[must_use]
pub fn bounded_scip_importer_digest() -> ScipImporterDigest {
    let mut hasher = Sha256::new();
    hasher.update(IMPORTER_IDENTITY_DOMAIN);
    hasher.update(SCIP_OVERLAY_IMPORTER_VERSION.to_be_bytes());
    hasher.update(SCIP_SCHEMA_REVISION.as_bytes());
    hasher.update(SCIP_SCHEMA_SHA256);
    ScipImporterDigest::new(hasher.finalize().into())
}

/// Computes the domain-separated immutable precision-overlay identity.
#[must_use]
pub fn hash_scip_overlay_identity(input: ScipOverlayIdentityInput) -> ScipOverlayDigest {
    let mut hasher = Sha256::new();
    hasher.update(OVERLAY_IDENTITY_DOMAIN);
    hasher.update(SCIP_OVERLAY_IDENTITY_VERSION.to_be_bytes());
    hasher.update(input.scope().connected_workspace().as_bytes());
    hasher.update(input.scope().workspace_view().to_be_bytes());
    hasher.update(input.scope().source_slot().as_bytes());
    hasher.update(input.scope().source_epoch().get().to_be_bytes());
    hasher.update(input.scope().generation().to_be_bytes());
    hasher.update(input.source_snapshot().as_bytes());
    hasher.update(input.source_manifest().as_bytes());
    hasher.update(input.configuration().as_bytes());
    hasher.update(input.producer().as_bytes());
    hasher.update(input.schema().as_bytes());
    hasher.update(input.importer().as_bytes());
    hasher.update(input.input().as_bytes());
    ScipOverlayDigest::new(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> ScipOverlayIdentityInput {
        ScipOverlayIdentityInput::new(
            ScipOverlayScopeIdentity::new(
                ConnectedWorkspaceId::new([7; 32]),
                8,
                SourceSlotId::new([9; 32]),
                SourceSlotEpoch::INITIAL,
                10,
            )
            .expect("scope"),
            SourceSnapshotDigest::new([1; 32]),
            SourceManifestDigest::new([2; 32]),
            ConfigurationDigest::new([3; 32]),
            ProducerManifestDigest::new([4; 32]),
            reviewed_scip_schema_digest(),
            bounded_scip_importer_digest(),
            hash_scip_input(b"scip"),
        )
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the semantic identity vector changes each independently committed Phase 2 field in one auditable fixture"
    )]
    fn overlay_identity_is_stable_and_every_semantic_component_changes_it() {
        let baseline = hash_scip_overlay_identity(input());
        assert_eq!(baseline, hash_scip_overlay_identity(input()));
        assert_ne!(baseline, ScipOverlayDigest::new([0; 32]));
        assert_ne!(
            baseline,
            hash_scip_overlay_identity(ScipOverlayIdentityInput::new(
                ScipOverlayScopeIdentity::new(
                    input().scope().connected_workspace(),
                    input().scope().workspace_view(),
                    input().scope().source_slot(),
                    input().scope().source_epoch(),
                    input().scope().generation() + 1,
                )
                .expect("changed scope"),
                input().source_snapshot(),
                input().source_manifest(),
                input().configuration(),
                input().producer(),
                input().schema(),
                input().importer(),
                input().input(),
            ))
        );
        assert_ne!(
            baseline,
            hash_scip_overlay_identity(ScipOverlayIdentityInput::new(
                input().scope(),
                SourceSnapshotDigest::new([9; 32]),
                input().source_manifest(),
                input().configuration(),
                input().producer(),
                input().schema(),
                input().importer(),
                input().input(),
            ))
        );
        assert_ne!(
            baseline,
            hash_scip_overlay_identity(ScipOverlayIdentityInput::new(
                input().scope(),
                input().source_snapshot(),
                SourceManifestDigest::new([9; 32]),
                input().configuration(),
                input().producer(),
                input().schema(),
                input().importer(),
                input().input(),
            ))
        );
        assert_ne!(
            baseline,
            hash_scip_overlay_identity(ScipOverlayIdentityInput::new(
                input().scope(),
                input().source_snapshot(),
                input().source_manifest(),
                ConfigurationDigest::new([9; 32]),
                input().producer(),
                input().schema(),
                input().importer(),
                input().input(),
            ))
        );
        assert_ne!(
            baseline,
            hash_scip_overlay_identity(ScipOverlayIdentityInput::new(
                input().scope(),
                input().source_snapshot(),
                input().source_manifest(),
                input().configuration(),
                ProducerManifestDigest::new([9; 32]),
                input().schema(),
                input().importer(),
                input().input(),
            ))
        );
        assert_ne!(
            baseline,
            hash_scip_overlay_identity(ScipOverlayIdentityInput::new(
                input().scope(),
                input().source_snapshot(),
                input().source_manifest(),
                input().configuration(),
                input().producer(),
                ScipSchemaDigest::new([9; 32]),
                input().importer(),
                input().input(),
            ))
        );
        assert_ne!(
            baseline,
            hash_scip_overlay_identity(ScipOverlayIdentityInput::new(
                input().scope(),
                input().source_snapshot(),
                input().source_manifest(),
                input().configuration(),
                input().producer(),
                input().schema(),
                ScipImporterDigest::new([9; 32]),
                input().input(),
            ))
        );
        assert_ne!(
            baseline,
            hash_scip_overlay_identity(ScipOverlayIdentityInput::new(
                input().scope(),
                input().source_snapshot(),
                input().source_manifest(),
                input().configuration(),
                input().producer(),
                input().schema(),
                input().importer(),
                hash_scip_input(b"changed"),
            ))
        );
    }

    #[test]
    fn reviewed_schema_and_importer_identities_are_pinned_and_distinct() {
        assert_eq!(
            reviewed_scip_schema_digest().as_bytes(),
            &SCIP_SCHEMA_SHA256
        );
        assert_ne!(
            reviewed_scip_schema_digest().as_bytes(),
            bounded_scip_importer_digest().as_bytes()
        );
    }
}
