//! Pure domain identities, snapshots, artifact keys, evidence, coverage, memory
//! lifecycle, temporal state, and invariants.
//!
//! This package remains independent of async runtimes, persistence, protocols,
//! parsers, Git, and filesystem I/O.

mod artifact;
mod coverage;
mod digest;
mod evidence;
mod memory;
mod memory_revalidation;
mod path;
mod phase2_evidence;
mod resolution;
mod result;
mod scip_overlay;
mod snapshot;
mod workspace;

pub use artifact::{AnalysisArtifactKey, AnalysisArtifactKeyVersion};
pub use coverage::{CoverageCompleteness, CoverageItemCount, CoverageSummary};
pub use digest::{
    AnalysisArtifactDigest, AnalysisArtifactPayloadDigest, AnalysisSchemaDigest,
    CanonicalMemoryDigest, ConfigurationDigest, CorrespondenceFingerprintDigest,
    CorrespondenceProfileDigest, DeclarationDigest, GitStateDigest, MemoryPresentationDigest,
    MigrationDigest, ProducerManifestDigest, RepositoryIdentityDigest, SHA256_DIGEST_BYTES,
    ScipImporterDigest, ScipInputDigest, ScipOverlayDigest, ScipSchemaDigest,
    Sha256DigestLengthError, SourceContentDigest, SourceManifestDigest, SourceSnapshotDigest,
    WorktreeStateDigest,
};
pub use evidence::{
    ByteLength, ByteOffset, ByteSpan, ByteSpanError, EvidenceIdentity, EvidenceLocation,
    EvidenceRecord, EvidenceRelation, EvidenceTier, ProducerIdentity,
};
pub use memory::{
    MAX_MEMORY_COMMITS, MAX_MEMORY_EVIDENCE, MAX_MEMORY_INTEROPERABLE_INTEGER, MAX_MEMORY_PARENTS,
    MAX_MEMORY_RELATIONSHIPS, MAX_MEMORY_SOURCE_BYTES, MEMORY_RECORD_SCHEMA_VERSION, MemoryActorId,
    MemoryActorKind, MemoryAssurance, MemoryAuditActorId, MemoryBody, MemoryClaim,
    MemoryCollectionField, MemoryCommitId, MemoryCorrespondenceReviewOperation,
    MemoryDisplayRevision, MemoryEvidence, MemoryEvidenceIndex, MemoryFactOrdinal,
    MemoryIntegerField, MemoryKind, MemoryLifecycle, MemoryObjectFormat, MemoryObservationSource,
    MemoryProducerId, MemoryProducerVersion, MemoryProvenance, MemoryProvenanceOrigin,
    MemoryQualifiedName, MemoryRecord, MemoryRecordError, MemoryRecordHeader, MemoryRecordId,
    MemoryRecordedAtUnixMillis, MemoryRelationship, MemoryRelationshipKind, MemoryScope,
    MemorySymbolName, MemoryTextField, MemoryTitle, MemoryValidity, RustMemorySymbolKind,
    RustSymbolMemoryEvidence,
};
pub use memory_revalidation::{
    MAX_MEMORY_ANCESTRY_CHECKS, MemoryAncestryCheck, MemoryAncestryOutcome, MemoryProjectValidity,
    MemoryRevalidationTarget, MemoryValidityEvaluationError, evaluate_memory_project_validity,
};
pub use path::{
    RepositoryPath, RepositoryPathByteCount, RepositoryPathComponentCount, RepositoryPathError,
    RepositoryPathLimits, RepositoryPathVersion,
};
pub use phase2_evidence::{
    PHASE2_EVIDENCE_BALANCED_PROFILE_ID, PHASE2_EVIDENCE_BALANCED_PROFILE_VERSION,
    Phase2ContextCandidateId, Phase2ContextProfile, Phase2ContextProviderAttribution,
    Phase2ContextProviderAvailability, Phase2ContextProviderCoverage,
    Phase2ContextProviderCoverageError, Phase2ContextProviderId, Phase2ContextScope,
    Phase2ContextScopeError, Phase2ContextTier,
};
pub use resolution::ResolutionStatus;
pub use result::{
    BoundedResultItems, MaterialResult, MaterialResultError, MaterialResultVersion,
    ResultItemCount, ResultItemLimit, ResultItemsError, ResultNotice, ResultNoticeKind,
};
pub use scip_overlay::{
    MAX_SCIP_SYMBOL_BYTES, SCIP_OVERLAY_SCHEMA_VERSION, ScipOccurrence, ScipRelationship,
    ScipRelationshipError, ScipRelationshipKinds, ScipSymbol, ScipSymbolError, ScipSymbolRoles,
};
pub use snapshot::{
    SourceFileCount, SourceFileKind, SourceFileLimit, SourceManifest, SourceManifestEntry,
    SourceManifestError, SourceManifestVersion, SourceSnapshot, SourceSnapshotMetadata,
    SourceSnapshotVersion,
};
pub use workspace::{
    ConnectedWorkspaceId, SourceSlotId, WORKSPACE_ID_BYTES, WorkspaceIdentityLengthError,
};
