//! Pure domain identities, snapshots, artifact keys, evidence, coverage, memory
//! lifecycle, temporal state, and invariants.
//!
//! This package remains independent of async runtimes, persistence, protocols,
//! parsers, Git, and filesystem I/O.

mod artifact;
mod coverage;
mod digest;
mod evidence;
mod path;
mod resolution;
mod result;
mod snapshot;

pub use artifact::{AnalysisArtifactKey, AnalysisArtifactKeyVersion};
pub use coverage::{CoverageCompleteness, CoverageItemCount, CoverageSummary};
pub use digest::{
    AnalysisArtifactDigest, AnalysisArtifactPayloadDigest, AnalysisSchemaDigest,
    CanonicalMemoryDigest, ConfigurationDigest, GitStateDigest, MigrationDigest,
    ProducerManifestDigest, RepositoryIdentityDigest, SHA256_DIGEST_BYTES, Sha256DigestLengthError,
    SourceContentDigest, SourceManifestDigest, SourceSnapshotDigest, WorktreeStateDigest,
};
pub use evidence::{
    ByteLength, ByteOffset, ByteSpan, ByteSpanError, EvidenceIdentity, EvidenceLocation,
    EvidenceRecord, EvidenceRelation, EvidenceTier, ProducerIdentity,
};
pub use path::{
    RepositoryPath, RepositoryPathByteCount, RepositoryPathComponentCount, RepositoryPathError,
    RepositoryPathLimits, RepositoryPathVersion,
};
pub use resolution::ResolutionStatus;
pub use result::{
    BoundedResultItems, MaterialResult, MaterialResultError, MaterialResultVersion,
    ResultItemCount, ResultItemLimit, ResultItemsError, ResultNotice, ResultNoticeKind,
};
pub use snapshot::{
    SourceFileCount, SourceFileKind, SourceFileLimit, SourceManifest, SourceManifestEntry,
    SourceManifestError, SourceManifestVersion, SourceSnapshot, SourceSnapshotMetadata,
    SourceSnapshotVersion,
};
