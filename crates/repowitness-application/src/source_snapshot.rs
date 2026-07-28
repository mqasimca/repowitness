use repowitness_domain::{
    AnalysisSchemaDigest, ConfigurationDigest, GitStateDigest, ProducerManifestDigest,
    RepositoryIdentityDigest, SourceManifestDigest, SourceSnapshotDigest, WorktreeStateDigest,
};
use sha2::{Digest, Sha256};

const RUST_SOURCE_SNAPSHOT_DOMAIN: &[u8] = b"RepoWitness\0rust-source-snapshot\0";
const GO_AND_RUST_SOURCE_SNAPSHOT_DOMAIN: &[u8] = b"RepoWitness\0go-and-rust-source-snapshot\0";
const SUPPORTED_LANGUAGES_SOURCE_SNAPSHOT_DOMAIN: &[u8] =
    b"RepoWitness\0supported-languages-source-snapshot\0";

/// Version of the concrete Phase 0 Rust source-snapshot encoding.
pub const RUST_SOURCE_SNAPSHOT_VERSION: u32 = 1;
/// Version of the mixed Go-and-Rust source-snapshot encoding.
pub const GO_AND_RUST_SOURCE_SNAPSHOT_VERSION: u32 = 1;
/// Version of the five-language source-snapshot encoding.
pub const SUPPORTED_LANGUAGES_SOURCE_SNAPSHOT_VERSION: u32 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SnapshotHashProfile {
    RustV1,
    GoAndRustV1,
    SupportedLanguagesV3,
}

/// Every non-file identity that affects one Phase 0 Rust source snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RustSourceSnapshotIdentity {
    repository: RepositoryIdentityDigest,
    git_state: GitStateDigest,
    worktree_state: WorktreeStateDigest,
    configuration: ConfigurationDigest,
    producer_manifest: ProducerManifestDigest,
    analysis_schema: AnalysisSchemaDigest,
    canonicalization_version: u32,
    hash_profile: SnapshotHashProfile,
}

impl RustSourceSnapshotIdentity {
    /// Constructs a semantics-complete snapshot identity.
    #[must_use]
    pub const fn new(
        repository: RepositoryIdentityDigest,
        git_state: GitStateDigest,
        worktree_state: WorktreeStateDigest,
        configuration: ConfigurationDigest,
        producer_manifest: ProducerManifestDigest,
        analysis_schema: AnalysisSchemaDigest,
        canonicalization_version: u32,
    ) -> Self {
        Self {
            repository,
            git_state,
            worktree_state,
            configuration,
            producer_manifest,
            analysis_schema,
            canonicalization_version,
            hash_profile: SnapshotHashProfile::RustV1,
        }
    }

    /// Constructs a semantics-complete mixed Go-and-Rust snapshot identity.
    #[must_use]
    pub const fn new_go_and_rust(
        repository: RepositoryIdentityDigest,
        git_state: GitStateDigest,
        worktree_state: WorktreeStateDigest,
        configuration: ConfigurationDigest,
        producer_manifest: ProducerManifestDigest,
        analysis_schema: AnalysisSchemaDigest,
        canonicalization_version: u32,
    ) -> Self {
        Self {
            repository,
            git_state,
            worktree_state,
            configuration,
            producer_manifest,
            analysis_schema,
            canonicalization_version,
            hash_profile: SnapshotHashProfile::GoAndRustV1,
        }
    }

    /// Constructs a semantics-complete snapshot identity for all supported languages.
    #[must_use]
    pub const fn new_supported_languages(
        repository: RepositoryIdentityDigest,
        git_state: GitStateDigest,
        worktree_state: WorktreeStateDigest,
        configuration: ConfigurationDigest,
        producer_manifest: ProducerManifestDigest,
        analysis_schema: AnalysisSchemaDigest,
        canonicalization_version: u32,
    ) -> Self {
        Self {
            repository,
            git_state,
            worktree_state,
            configuration,
            producer_manifest,
            analysis_schema,
            canonicalization_version,
            hash_profile: SnapshotHashProfile::SupportedLanguagesV3,
        }
    }

    /// Returns the repository identity.
    #[must_use]
    pub const fn repository(self) -> RepositoryIdentityDigest {
        self.repository
    }

    /// Returns the complete relevant Git-state identity.
    #[must_use]
    pub const fn git_state(self) -> GitStateDigest {
        self.git_state
    }

    /// Returns the worktree and submodule-state identity.
    #[must_use]
    pub const fn worktree_state(self) -> WorktreeStateDigest {
        self.worktree_state
    }

    /// Returns the resolved semantics-affecting configuration identity.
    #[must_use]
    pub const fn configuration(self) -> ConfigurationDigest {
        self.configuration
    }

    /// Returns the analyzer and grammar manifest identity.
    #[must_use]
    pub const fn producer_manifest(self) -> ProducerManifestDigest {
        self.producer_manifest
    }

    /// Returns the analysis schema identity.
    #[must_use]
    pub const fn analysis_schema(self) -> AnalysisSchemaDigest {
        self.analysis_schema
    }

    /// Returns the canonical fact-format version.
    #[must_use]
    pub const fn canonicalization_version(self) -> u32 {
        self.canonicalization_version
    }
}

/// Language-neutral compatibility name for a source-snapshot identity.
pub type SourceSnapshotIdentity = RustSourceSnapshotIdentity;

/// Hashes every Phase 0 Rust snapshot component in a fixed domain and order.
#[must_use]
pub fn hash_rust_source_snapshot(
    identity: RustSourceSnapshotIdentity,
    manifest: SourceManifestDigest,
) -> SourceSnapshotDigest {
    let mut hasher = Sha256::new();
    hasher.update(RUST_SOURCE_SNAPSHOT_DOMAIN);
    hasher.update(RUST_SOURCE_SNAPSHOT_VERSION.to_be_bytes());
    hasher.update(identity.repository().as_bytes());
    hasher.update(identity.git_state().as_bytes());
    hasher.update(identity.worktree_state().as_bytes());
    hasher.update(identity.configuration().as_bytes());
    hasher.update(identity.producer_manifest().as_bytes());
    hasher.update(identity.analysis_schema().as_bytes());
    hasher.update(identity.canonicalization_version().to_be_bytes());
    hasher.update(manifest.as_bytes());
    SourceSnapshotDigest::new(hasher.finalize().into())
}

/// Hashes a snapshot using the profile selected by its validated constructor.
#[must_use]
pub fn hash_source_snapshot(
    identity: RustSourceSnapshotIdentity,
    manifest: SourceManifestDigest,
) -> SourceSnapshotDigest {
    match identity.hash_profile {
        SnapshotHashProfile::RustV1 => hash_rust_source_snapshot(identity, manifest),
        SnapshotHashProfile::GoAndRustV1 => hash_snapshot(
            GO_AND_RUST_SOURCE_SNAPSHOT_DOMAIN,
            GO_AND_RUST_SOURCE_SNAPSHOT_VERSION,
            identity,
            manifest,
        ),
        SnapshotHashProfile::SupportedLanguagesV3 => hash_snapshot(
            SUPPORTED_LANGUAGES_SOURCE_SNAPSHOT_DOMAIN,
            SUPPORTED_LANGUAGES_SOURCE_SNAPSHOT_VERSION,
            identity,
            manifest,
        ),
    }
}

fn hash_snapshot(
    domain: &[u8],
    version: u32,
    identity: RustSourceSnapshotIdentity,
    manifest: SourceManifestDigest,
) -> SourceSnapshotDigest {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(version.to_be_bytes());
    hasher.update(identity.repository().as_bytes());
    hasher.update(identity.git_state().as_bytes());
    hasher.update(identity.worktree_state().as_bytes());
    hasher.update(identity.configuration().as_bytes());
    hasher.update(identity.producer_manifest().as_bytes());
    hasher.update(identity.analysis_schema().as_bytes());
    hasher.update(identity.canonicalization_version().to_be_bytes());
    hasher.update(manifest.as_bytes());
    SourceSnapshotDigest::new(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use repowitness_domain::{
        AnalysisSchemaDigest, ConfigurationDigest, GitStateDigest, ProducerManifestDigest,
        RepositoryIdentityDigest, SourceManifestDigest, WorktreeStateDigest,
    };

    use super::{RustSourceSnapshotIdentity, hash_rust_source_snapshot};

    fn identity() -> RustSourceSnapshotIdentity {
        RustSourceSnapshotIdentity::new(
            RepositoryIdentityDigest::new([1; 32]),
            GitStateDigest::new([2; 32]),
            WorktreeStateDigest::new([3; 32]),
            ConfigurationDigest::new([4; 32]),
            ProducerManifestDigest::new([5; 32]),
            AnalysisSchemaDigest::new([6; 32]),
            7,
        )
    }

    #[test]
    fn snapshot_hash_has_a_stable_golden_vector() {
        assert_eq!(
            hash_rust_source_snapshot(identity(), SourceManifestDigest::new([8; 32])).into_bytes(),
            [
                0x8E, 0xEE, 0x4F, 0x50, 0x4F, 0x38, 0xCF, 0x2A, 0x52, 0x67, 0xF1, 0x65, 0xD3, 0x7E,
                0x4F, 0xC9, 0xC3, 0xEE, 0x9B, 0xAF, 0xDB, 0x57, 0xB3, 0x49, 0xEB, 0x4F, 0xED, 0xF9,
                0xDA, 0xEB, 0x8E, 0x52,
            ]
        );
    }

    #[test]
    fn every_snapshot_component_changes_the_digest() {
        let baseline = identity();
        let manifest = SourceManifestDigest::new([8; 32]);
        let expected = hash_rust_source_snapshot(baseline, manifest);
        let variants = [
            RustSourceSnapshotIdentity::new(
                RepositoryIdentityDigest::new([9; 32]),
                baseline.git_state(),
                baseline.worktree_state(),
                baseline.configuration(),
                baseline.producer_manifest(),
                baseline.analysis_schema(),
                baseline.canonicalization_version(),
            ),
            RustSourceSnapshotIdentity::new(
                baseline.repository(),
                GitStateDigest::new([9; 32]),
                baseline.worktree_state(),
                baseline.configuration(),
                baseline.producer_manifest(),
                baseline.analysis_schema(),
                baseline.canonicalization_version(),
            ),
            RustSourceSnapshotIdentity::new(
                baseline.repository(),
                baseline.git_state(),
                WorktreeStateDigest::new([9; 32]),
                baseline.configuration(),
                baseline.producer_manifest(),
                baseline.analysis_schema(),
                baseline.canonicalization_version(),
            ),
            RustSourceSnapshotIdentity::new(
                baseline.repository(),
                baseline.git_state(),
                baseline.worktree_state(),
                ConfigurationDigest::new([9; 32]),
                baseline.producer_manifest(),
                baseline.analysis_schema(),
                baseline.canonicalization_version(),
            ),
            RustSourceSnapshotIdentity::new(
                baseline.repository(),
                baseline.git_state(),
                baseline.worktree_state(),
                baseline.configuration(),
                ProducerManifestDigest::new([9; 32]),
                baseline.analysis_schema(),
                baseline.canonicalization_version(),
            ),
            RustSourceSnapshotIdentity::new(
                baseline.repository(),
                baseline.git_state(),
                baseline.worktree_state(),
                baseline.configuration(),
                baseline.producer_manifest(),
                AnalysisSchemaDigest::new([9; 32]),
                baseline.canonicalization_version(),
            ),
            RustSourceSnapshotIdentity::new(
                baseline.repository(),
                baseline.git_state(),
                baseline.worktree_state(),
                baseline.configuration(),
                baseline.producer_manifest(),
                baseline.analysis_schema(),
                9,
            ),
        ];

        assert!(
            variants
                .into_iter()
                .all(|variant| hash_rust_source_snapshot(variant, manifest) != expected)
        );
        assert_ne!(
            hash_rust_source_snapshot(baseline, SourceManifestDigest::new([9; 32])),
            expected
        );
    }
}
