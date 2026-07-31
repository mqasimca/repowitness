//! Immutable SCIP overlay persistence inputs.

use std::{collections::BTreeSet, error::Error, fmt};

use repowitness_analysis::ScipOverlayDocument;
use repowitness_application::{
    PackageScopeDigest, RustSourceSnapshotIdentity, ScipOverlayIdentityInput,
    ScipOverlayScopeIdentity, hash_scip_overlay_identity,
};
use repowitness_domain::{
    ByteSpan, ConnectedWorkspaceId, RepositoryPath, ScipOverlayDigest, ScipRelationshipKinds,
    ScipSymbol, ScipSymbolRoles, SourceContentDigest, SourceManifestDigest, SourceSlotId,
    SourceSnapshotDigest,
};

use super::{GenerationId, SourceSlotEpoch, SqliteStoreError, WorkspaceViewId};

/// Inclusive maximum number of source documents in one persisted overlay.
pub const MAX_SCIP_OVERLAY_DOCUMENTS: usize = 100_000;
/// Inclusive maximum retained occurrence facts returned by one overlay read.
pub const MAX_SCIP_EVIDENCE_OCCURRENCES: u16 = 1_000;
/// Inclusive maximum retained relationship facts returned by one overlay read.
pub const MAX_SCIP_EVIDENCE_RELATIONSHIPS: u16 = 1_000;
/// Inclusive encoded-output ceiling for one overlay evidence read.
pub const MAX_SCIP_EVIDENCE_OUTPUT_BYTES: u64 = 1_048_576;

/// Exact completed source member selected for one contained SCIP import.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScipOverlayImportScope {
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: WorkspaceViewId,
    source_slot: SourceSlotId,
    source_epoch: SourceSlotEpoch,
    generation: GenerationId,
    source_snapshot: SourceSnapshotDigest,
    source_manifest: SourceManifestDigest,
    source_identity: RustSourceSnapshotIdentity,
}

impl ScipOverlayImportScope {
    #[allow(
        clippy::too_many_arguments,
        reason = "every exact workspace and source identity is needed at the import boundary"
    )]
    pub(crate) const fn new(
        connected_workspace: ConnectedWorkspaceId,
        workspace_view: WorkspaceViewId,
        source_slot: SourceSlotId,
        source_epoch: SourceSlotEpoch,
        generation: GenerationId,
        source_snapshot: SourceSnapshotDigest,
        source_manifest: SourceManifestDigest,
        source_identity: RustSourceSnapshotIdentity,
    ) -> Self {
        Self {
            connected_workspace,
            workspace_view,
            source_slot,
            source_epoch,
            generation,
            source_snapshot,
            source_manifest,
            source_identity,
        }
    }

    /// Returns the selected immutable connected workspace.
    #[must_use]
    pub const fn connected_workspace(self) -> ConnectedWorkspaceId {
        self.connected_workspace
    }

    /// Returns the selected immutable workspace view.
    #[must_use]
    pub const fn workspace_view(self) -> WorkspaceViewId {
        self.workspace_view
    }

    /// Returns the exact source slot whose path namespace is admitted.
    #[must_use]
    pub const fn source_slot(self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the completed source-slot epoch bound by the view member.
    #[must_use]
    pub const fn source_epoch(self) -> SourceSlotEpoch {
        self.source_epoch
    }

    /// Returns the exact active generation selected by that view member.
    #[must_use]
    pub const fn generation(self) -> GenerationId {
        self.generation
    }

    /// Returns the complete exact source snapshot receipt.
    #[must_use]
    pub const fn source_snapshot(self) -> SourceSnapshotDigest {
        self.source_snapshot
    }

    /// Returns the source manifest that the imported document paths must match.
    #[must_use]
    pub const fn source_manifest(self) -> SourceManifestDigest {
        self.source_manifest
    }

    /// Returns the complete snapshot identity needed for the final local fence.
    #[must_use]
    pub const fn source_identity(self) -> RustSourceSnapshotIdentity {
        self.source_identity
    }

    /// Returns the complete non-local scope identity included in the overlay digest.
    pub fn overlay_scope_identity(self) -> Result<ScipOverlayScopeIdentity, SqliteStoreError> {
        ScipOverlayScopeIdentity::new(
            self.connected_workspace,
            self.workspace_view.get(),
            self.source_slot,
            self.source_epoch,
            self.generation.get(),
        )
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)
    }
}

/// Complete validated overlay payload ready for one atomic SQLite publication.
pub struct PreparedScipOverlay {
    identity: ScipOverlayIdentityInput,
    digest: ScipOverlayDigest,
    documents: Box<[ScipOverlayDocument]>,
    occurrence_count: u64,
    relationship_count: u64,
}

impl PreparedScipOverlay {
    /// Validates one complete decoded overlay before it reaches SQLite.
    pub fn try_new(
        identity: ScipOverlayIdentityInput,
        documents: Vec<ScipOverlayDocument>,
    ) -> Result<Self, ScipOverlayPreparationError> {
        if documents.len() > MAX_SCIP_OVERLAY_DOCUMENTS {
            return Err(ScipOverlayPreparationError::DocumentLimitExceeded);
        }
        let mut paths = BTreeSet::new();
        let mut occurrence_count = 0_u64;
        let mut relationship_count = 0_u64;
        for document in &documents {
            if !paths.insert(document.path()) {
                return Err(ScipOverlayPreparationError::InvalidDocuments);
            }
            for (ordinal, occurrence) in document.occurrences().iter().enumerate() {
                let ordinal = u32::try_from(ordinal)
                    .map_err(|_| ScipOverlayPreparationError::CountOverflow)?;
                if occurrence.path() != document.path()
                    || occurrence.content() != document.content()
                    || occurrence.ordinal() != ordinal
                {
                    return Err(ScipOverlayPreparationError::InvalidDocuments);
                }
                occurrence_count = occurrence_count
                    .checked_add(1)
                    .ok_or(ScipOverlayPreparationError::CountOverflow)?;
            }
            relationship_count = relationship_count
                .checked_add(
                    u64::try_from(document.relationships().len())
                        .map_err(|_| ScipOverlayPreparationError::CountOverflow)?,
                )
                .ok_or(ScipOverlayPreparationError::CountOverflow)?;
        }
        Ok(Self {
            digest: hash_scip_overlay_identity(identity),
            identity,
            documents: documents.into_boxed_slice(),
            occurrence_count,
            relationship_count,
        })
    }

    /// Returns the exact identity inputs bound to this payload.
    #[must_use]
    pub const fn identity(&self) -> ScipOverlayIdentityInput {
        self.identity
    }

    /// Returns the immutable content-addressed receipt identity.
    #[must_use]
    pub const fn digest(&self) -> ScipOverlayDigest {
        self.digest
    }

    /// Returns decoded documents in producer source order.
    #[must_use]
    pub fn documents(&self) -> &[ScipOverlayDocument] {
        &self.documents
    }

    /// Returns the exact retained occurrence count.
    #[must_use]
    pub const fn occurrence_count(&self) -> u64 {
        self.occurrence_count
    }

    /// Returns the exact retained relationship count.
    #[must_use]
    pub const fn relationship_count(&self) -> u64 {
        self.relationship_count
    }
}

impl fmt::Debug for PreparedScipOverlay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedScipOverlay")
            .field("identity", &self.identity)
            .field("digest", &self.digest)
            .field("document_count", &self.documents.len())
            .field("occurrence_count", &self.occurrence_count)
            .field("relationship_count", &self.relationship_count)
            .finish()
    }
}

/// Failure to prepare a complete immutable overlay for persistence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScipOverlayPreparationError {
    /// Document paths, content, or source-order occurrence facts disagreed.
    InvalidDocuments,
    /// The source-document bound was exceeded.
    DocumentLimitExceeded,
    /// A persisted count could not be represented exactly.
    CountOverflow,
}

impl fmt::Display for ScipOverlayPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidDocuments => "SCIP overlay documents are inconsistent",
            Self::DocumentLimitExceeded => "SCIP overlay document limit exceeded",
            Self::CountOverflow => "SCIP overlay count overflowed",
        })
    }
}

impl Error for ScipOverlayPreparationError {}

/// Categorical result of resolving a source-slot overlay in one pinned view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScipOverlayAvailability {
    /// No complete active overlay is scoped to the requested immutable view/slot.
    NotProduced,
    /// One complete immutable overlay is exactly scoped to the requested view/slot.
    Complete(ScipOverlaySummary),
}

/// Count-only receipt metadata for a complete immutable SCIP overlay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScipOverlaySummary {
    digest: ScipOverlayDigest,
    source_slot: SourceSlotId,
    documents: u64,
    occurrences: u64,
    relationships: u64,
}

impl ScipOverlaySummary {
    pub(crate) const fn new(
        digest: ScipOverlayDigest,
        source_slot: SourceSlotId,
        documents: u64,
        occurrences: u64,
        relationships: u64,
    ) -> Self {
        Self {
            digest,
            source_slot,
            documents,
            occurrences,
            relationships,
        }
    }

    /// Returns the immutable overlay receipt identity.
    #[must_use]
    pub const fn digest(self) -> ScipOverlayDigest {
        self.digest
    }

    /// Returns the exact source slot that owns all document paths.
    #[must_use]
    pub const fn source_slot(self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the exact retained document count.
    #[must_use]
    pub const fn documents(self) -> u64 {
        self.documents
    }

    /// Returns the exact retained occurrence count.
    #[must_use]
    pub const fn occurrences(self) -> u64 {
        self.occurrences
    }

    /// Returns the exact retained relationship count.
    #[must_use]
    pub const fn relationships(self) -> u64 {
        self.relationships
    }
}

/// Bounded row and output limits for one package-scoped SCIP evidence read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScipEvidenceReadLimits {
    max_occurrences: u16,
    max_relationships: u16,
    max_output_bytes: u64,
}

impl ScipEvidenceReadLimits {
    /// Validates independent occurrence, relationship, and output ceilings.
    pub const fn try_new(
        max_occurrences: u16,
        max_relationships: u16,
        max_output_bytes: u64,
    ) -> Result<Self, ScipEvidenceReadLimitsError> {
        if max_occurrences == 0
            || max_occurrences > MAX_SCIP_EVIDENCE_OCCURRENCES
            || max_relationships == 0
            || max_relationships > MAX_SCIP_EVIDENCE_RELATIONSHIPS
            || max_output_bytes == 0
            || max_output_bytes > MAX_SCIP_EVIDENCE_OUTPUT_BYTES
        {
            return Err(ScipEvidenceReadLimitsError);
        }
        Ok(Self {
            max_occurrences,
            max_relationships,
            max_output_bytes,
        })
    }

    /// Returns the inclusive occurrence ceiling.
    #[must_use]
    pub const fn max_occurrences(self) -> u16 {
        self.max_occurrences
    }

    /// Returns the inclusive relationship ceiling.
    #[must_use]
    pub const fn max_relationships(self) -> u16 {
        self.max_relationships
    }

    /// Returns the inclusive encoded-output ceiling.
    #[must_use]
    pub const fn max_output_bytes(self) -> u64 {
        self.max_output_bytes
    }
}

impl Default for ScipEvidenceReadLimits {
    fn default() -> Self {
        Self {
            max_occurrences: 100,
            max_relationships: 100,
            max_output_bytes: 256 * 1024,
        }
    }
}

/// The supplied SCIP evidence read bounds were invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScipEvidenceReadLimitsError;

impl fmt::Display for ScipEvidenceReadLimitsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SCIP evidence read limits are invalid")
    }
}

impl Error for ScipEvidenceReadLimitsError {}

/// Exact source occurrence backed by one selected immutable SCIP overlay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScipOccurrenceEvidence {
    path: RepositoryPath,
    content: SourceContentDigest,
    span: ByteSpan,
    roles: ScipSymbolRoles,
}

impl ScipOccurrenceEvidence {
    pub(crate) const fn new(
        path: RepositoryPath,
        content: SourceContentDigest,
        span: ByteSpan,
        roles: ScipSymbolRoles,
    ) -> Self {
        Self {
            path,
            content,
            span,
            roles,
        }
    }

    #[must_use]
    /// Returns the exact repository-relative source path.
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }
    #[must_use]
    /// Returns the exact source-content digest.
    pub const fn content(&self) -> SourceContentDigest {
        self.content
    }
    #[must_use]
    /// Returns the exact half-open source-byte span.
    pub const fn span(&self) -> ByteSpan {
        self.span
    }
    #[must_use]
    /// Returns the preserved producer occurrence-role bits.
    pub const fn roles(&self) -> ScipSymbolRoles {
        self.roles
    }
}

/// Direction of a relationship relative to the requested exact SCIP symbol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScipRelationshipDirection {
    /// The requested symbol is the relation source.
    Outgoing,
    /// The requested symbol is the relation target.
    Incoming,
}

/// One package-scoped validated cross-file SCIP relationship.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScipRelationshipEvidence {
    path: RepositoryPath,
    content: SourceContentDigest,
    direction: ScipRelationshipDirection,
    source: ScipSymbol,
    target: ScipSymbol,
    kinds: ScipRelationshipKinds,
}

impl ScipRelationshipEvidence {
    #[allow(
        clippy::too_many_arguments,
        reason = "all exact evidence identity fields are required"
    )]
    pub(crate) const fn new(
        path: RepositoryPath,
        content: SourceContentDigest,
        direction: ScipRelationshipDirection,
        source: ScipSymbol,
        target: ScipSymbol,
        kinds: ScipRelationshipKinds,
    ) -> Self {
        Self {
            path,
            content,
            direction,
            source,
            target,
            kinds,
        }
    }

    #[must_use]
    /// Returns the exact repository-relative source path of the relationship.
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }
    #[must_use]
    /// Returns the exact source-content digest of the relationship document.
    pub const fn content(&self) -> SourceContentDigest {
        self.content
    }
    #[must_use]
    /// Returns the relationship direction relative to the requested symbol.
    pub const fn direction(&self) -> ScipRelationshipDirection {
        self.direction
    }
    #[must_use]
    /// Returns the opaque source producer symbol.
    pub const fn source(&self) -> &ScipSymbol {
        &self.source
    }
    #[must_use]
    /// Returns the opaque target producer symbol.
    pub const fn target(&self) -> &ScipSymbol {
        &self.target
    }
    #[must_use]
    /// Returns the explicitly declared relationship flags.
    pub const fn kinds(&self) -> ScipRelationshipKinds {
        self.kinds
    }
}

/// Exact package-scoped evidence from a selected immutable overlay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScipSymbolEvidence {
    overlay: ScipOverlaySummary,
    package_scope: PackageScopeDigest,
    occurrences: Box<[ScipOccurrenceEvidence]>,
    relationships: Box<[ScipRelationshipEvidence]>,
    occurrences_truncated: bool,
    relationships_truncated: bool,
    output_bytes: u64,
}

impl ScipSymbolEvidence {
    #[allow(
        clippy::too_many_arguments,
        reason = "result fields are independently material coverage"
    )]
    pub(crate) fn new(
        overlay: ScipOverlaySummary,
        package_scope: PackageScopeDigest,
        occurrences: Vec<ScipOccurrenceEvidence>,
        relationships: Vec<ScipRelationshipEvidence>,
        occurrences_truncated: bool,
        relationships_truncated: bool,
        output_bytes: u64,
    ) -> Self {
        Self {
            overlay,
            package_scope,
            occurrences: occurrences.into_boxed_slice(),
            relationships: relationships.into_boxed_slice(),
            occurrences_truncated,
            relationships_truncated,
            output_bytes,
        }
    }

    #[must_use]
    /// Returns the exact selected immutable overlay summary.
    pub const fn overlay(&self) -> ScipOverlaySummary {
        self.overlay
    }
    #[must_use]
    /// Returns the semantic identity of the explicit package scope.
    pub const fn package_scope(&self) -> PackageScopeDigest {
        self.package_scope
    }
    #[must_use]
    /// Returns exact matching occurrence evidence in deterministic order.
    pub fn occurrences(&self) -> &[ScipOccurrenceEvidence] {
        &self.occurrences
    }
    #[must_use]
    /// Returns exact matching relationship evidence in deterministic order.
    pub fn relationships(&self) -> &[ScipRelationshipEvidence] {
        &self.relationships
    }
    #[must_use]
    /// Reports that further matching occurrences exceeded the row ceiling.
    pub const fn occurrences_truncated(&self) -> bool {
        self.occurrences_truncated
    }
    #[must_use]
    /// Reports that further matching relationships exceeded the row ceiling.
    pub const fn relationships_truncated(&self) -> bool {
        self.relationships_truncated
    }
    #[must_use]
    /// Returns the conservative encoded-output byte count.
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
}

/// Categorical result of exact package-scoped SCIP symbol evidence lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScipSymbolEvidenceResult {
    /// No exact complete overlay is selected for the requested pinned view/slot.
    NotProduced,
    /// An exact overlay exists but it has no matching evidence in the requested scope.
    NoMatch(ScipOverlaySummary),
    /// Matching evidence, including explicit independent truncation signals.
    Found(ScipSymbolEvidence),
}

/// Categorical exact syntax-span to opaque SCIP-symbol resolution in one selected overlay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScipSyntaxSymbolResolution {
    /// No complete overlay is selected for the requested pinned view and source slot.
    NotProduced,
    /// The overlay has no producer symbol at the exact syntax identifier span.
    NoExactMatch,
    /// More than one distinct opaque producer symbol matched the exact syntax span.
    Ambiguous,
    /// Exactly one opaque producer symbol matched the exact syntax span.
    Exact(ScipSymbol),
}
