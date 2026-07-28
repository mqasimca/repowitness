use repowitness_analysis::{
    RUST_CORRESPONDENCE_PROFILE_ID, RUST_CORRESPONDENCE_PROFILE_VERSION, RustSourceAnalysis,
};
use repowitness_domain::{
    AnalysisArtifactDigest, AnalysisArtifactKey, AnalysisArtifactKeyVersion,
    AnalysisArtifactPayloadDigest, AnalysisSchemaDigest, ConfigurationDigest,
    ProducerManifestDigest, RepositoryPath, SourceContentDigest, SourceFileKind, SourceManifest,
    SourceManifestDigest, SourceManifestVersion,
};
use sha2::{Digest, Sha256};

const ARTIFACT_KEY_DOMAIN: &[u8] = b"RepoWitness\0analysis-artifact-key\0";
const ARTIFACT_PAYLOAD_DOMAIN: &[u8] = b"RepoWitness\0analysis-artifact-payload\0";
const SOURCE_MANIFEST_DOMAIN: &[u8] = b"RepoWitness\0source-manifest\0";

const LEGACY_ANALYSIS_ARTIFACT_PAYLOAD_VERSION: u32 = 1;
/// Current canonical version of the persisted analysis-artifact payload
/// encoding.
pub const ANALYSIS_ARTIFACT_PAYLOAD_VERSION: u32 = 2;

/// Concrete logical key whose components have canonical fixed-width identities.
pub type CanonicalAnalysisArtifactKey = AnalysisArtifactKey<
    SourceContentDigest,
    ProducerManifestDigest,
    ConfigurationDigest,
    AnalysisSchemaDigest,
    u32,
>;

/// Canonical Phase 0 manifest over exact repository paths and SHA-256 content.
pub type CanonicalSourceManifest =
    SourceManifest<RepositoryPath, SourceFileKind, SourceContentDigest>;

/// Computes the raw SHA-256 identity of exact immutable source bytes.
#[must_use]
pub fn hash_source_content(content: &[u8]) -> SourceContentDigest {
    SourceContentDigest::new(hash_bytes(content))
}

/// Hashes every semantic artifact-key component in a fixed domain and order.
#[must_use]
pub fn hash_analysis_artifact_key(key: &CanonicalAnalysisArtifactKey) -> AnalysisArtifactDigest {
    let mut hasher = Sha256::new();
    hasher.update(ARTIFACT_KEY_DOMAIN);
    hasher.update(AnalysisArtifactKeyVersion::V1.get().to_be_bytes());
    hasher.update(key.source_digest().as_bytes());
    hasher.update(key.analyzer_identity().as_bytes());
    hasher.update(key.configuration_identity().as_bytes());
    hasher.update(key.schema_identity().as_bytes());
    hasher.update(key.canonicalization_version().to_be_bytes());
    AnalysisArtifactDigest::new(hasher.finalize().into())
}

/// Hashes complete ordered Rust analysis output for persisted integrity checks.
#[must_use]
pub fn hash_analysis_artifact_payload(
    analysis: &RustSourceAnalysis,
) -> AnalysisArtifactPayloadDigest {
    let mut hasher = Sha256::new();
    let has_correspondence = analysis
        .facts()
        .iter()
        .any(|fact| fact.correspondence().is_some());
    hasher.update(ARTIFACT_PAYLOAD_DOMAIN);
    hasher.update(
        if has_correspondence {
            ANALYSIS_ARTIFACT_PAYLOAD_VERSION
        } else {
            LEGACY_ANALYSIS_ARTIFACT_PAYLOAD_VERSION
        }
        .to_be_bytes(),
    );
    if has_correspondence {
        update_length_prefixed(&mut hasher, RUST_CORRESPONDENCE_PROFILE_ID.as_bytes());
        hasher.update(RUST_CORRESPONDENCE_PROFILE_VERSION.to_be_bytes());
    }
    hasher.update(analysis.visited_nodes().to_be_bytes());
    hasher.update(analysis.syntax_error_nodes().to_be_bytes());
    hasher.update(
        u64::try_from(analysis.facts().len())
            .expect("bounded source fact count fits in u64")
            .to_be_bytes(),
    );
    for (ordinal, fact) in analysis.facts().iter().enumerate() {
        hasher.update(
            u64::try_from(ordinal)
                .expect("bounded source fact ordinal fits in u64")
                .to_be_bytes(),
        );
        update_length_prefixed(&mut hasher, fact.kind().as_str().as_bytes());
        update_length_prefixed(&mut hasher, fact.name().as_bytes());
        update_length_prefixed(&mut hasher, fact.qualified_name().as_bytes());
        hasher.update(fact.name_span().start().get().to_be_bytes());
        hasher.update(fact.name_span().end().get().to_be_bytes());
        hasher.update(fact.declaration_span().start().get().to_be_bytes());
        hasher.update(fact.declaration_span().end().get().to_be_bytes());
        if has_correspondence {
            if let Some(fingerprint) = fact.correspondence() {
                hasher.update([1]);
                hasher.update(fingerprint.declaration().as_bytes());
                hasher.update(fingerprint.name_elided().as_bytes());
            } else {
                hasher.update([0]);
            }
        }
    }
    AnalysisArtifactPayloadDigest::new(hasher.finalize().into())
}

/// Hashes a sorted source manifest with fixed-width framing and file-kind tags.
#[must_use]
pub fn hash_source_manifest(manifest: &CanonicalSourceManifest) -> SourceManifestDigest {
    let mut hasher = Sha256::new();
    hasher.update(SOURCE_MANIFEST_DOMAIN);
    hasher.update(SourceManifestVersion::V1.get().to_be_bytes());
    hasher.update(manifest.count().get().to_be_bytes());
    for entry in manifest.as_slice() {
        hasher.update(entry.path().byte_count().get().to_be_bytes());
        hasher.update(entry.path().as_bytes());
        hasher.update([entry.file_type().canonical_tag()]);
        hasher.update(entry.content_digest().as_bytes());
    }
    SourceManifestDigest::new(hasher.finalize().into())
}

fn hash_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn update_length_prefixed(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(
        u64::try_from(value.len())
            .expect("bounded canonical value length fits in u64")
            .to_be_bytes(),
    );
    hasher.update(value);
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::AtomicBool,
        time::{Duration, Instant},
    };

    use repowitness_analysis::{
        RustAnalysisControl, RustAnalysisLimits, RustSourceAnalysis, RustSourceAnalyzer,
        RustSymbolFact,
    };
    use repowitness_domain::{
        AnalysisArtifactKey, AnalysisSchemaDigest, ConfigurationDigest, ProducerManifestDigest,
        RepositoryPath, RepositoryPathLimits, SourceContentDigest, SourceFileKind, SourceFileLimit,
        SourceManifest, SourceManifestEntry,
    };

    use super::{
        hash_analysis_artifact_key, hash_analysis_artifact_payload, hash_source_content,
        hash_source_manifest,
    };

    const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(1024, 32);

    fn manifest_entry(
        path: &[u8],
        kind: SourceFileKind,
        digest: [u8; 32],
    ) -> SourceManifestEntry<RepositoryPath, SourceFileKind, SourceContentDigest> {
        SourceManifestEntry::new(
            RepositoryPath::try_from_bytes(path, PATH_LIMITS)
                .expect("fixture repository path must be valid"),
            kind,
            SourceContentDigest::new(digest),
        )
    }

    #[test]
    fn source_content_uses_the_standard_sha256_bytes() {
        assert_eq!(
            hash_source_content(b"abc").into_bytes(),
            [
                0xBA, 0x78, 0x16, 0xBF, 0x8F, 0x01, 0xCF, 0xEA, 0x41, 0x41, 0x40, 0xDE, 0x5D, 0xAE,
                0x22, 0x23, 0xB0, 0x03, 0x61, 0xA3, 0x96, 0x17, 0x7A, 0x9C, 0xB4, 0x10, 0xFF, 0x61,
                0xF2, 0x00, 0x15, 0xAD,
            ]
        );
    }

    #[test]
    fn artifact_key_hash_has_a_stable_golden_vector() {
        let key = AnalysisArtifactKey::new(
            SourceContentDigest::new([1; 32]),
            ProducerManifestDigest::new([2; 32]),
            ConfigurationDigest::new([3; 32]),
            AnalysisSchemaDigest::new([4; 32]),
            7_u32,
        );

        assert_eq!(
            hash_analysis_artifact_key(&key).into_bytes(),
            [
                0x19, 0x9F, 0x2C, 0x24, 0x8D, 0x00, 0xF5, 0x4E, 0x14, 0x6C, 0x6E, 0xAC, 0x91, 0xCC,
                0x38, 0x39, 0xD7, 0xAE, 0xB7, 0x6A, 0xD7, 0x0D, 0xE5, 0x37, 0x58, 0x18, 0xB3, 0xDE,
                0xE8, 0xFA, 0xFB, 0xE1,
            ]
        );
    }

    #[test]
    fn artifact_payload_hash_has_a_stable_golden_vector() {
        let cancelled = AtomicBool::new(false);
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(1))
            .expect("fixture deadline should be representable");
        let analysis = RustSourceAnalyzer::new()
            .expect("Rust analyzer should initialize")
            .analyze(
                b"pub fn alpha() {}\n",
                RustAnalysisLimits::default(),
                RustAnalysisControl::new(&cancelled, deadline),
            )
            .expect("fixture source should analyze");

        assert_eq!(
            hash_analysis_artifact_payload(&analysis).into_bytes(),
            [
                0x94, 0x01, 0xB6, 0xA0, 0x5C, 0xF8, 0x8C, 0x10, 0x42, 0x58, 0xA6, 0x79, 0x60, 0x0A,
                0x69, 0x3B, 0xE6, 0x92, 0xA7, 0x8E, 0x9E, 0xB9, 0x31, 0xA0, 0x3A, 0x65, 0x51, 0x26,
                0xDE, 0x1C, 0x2E, 0xD4,
            ]
        );

        let fact = &analysis.facts()[0];
        let legacy_fact = RustSymbolFact::try_new(
            fact.kind(),
            fact.name().to_owned(),
            fact.qualified_name().to_owned(),
            fact.name_span(),
            fact.declaration_span(),
            RustAnalysisLimits::DEFAULT,
        )
        .expect("legacy fact remains structurally valid");
        let legacy = RustSourceAnalysis::try_from_parts(
            vec![legacy_fact],
            analysis.visited_nodes(),
            analysis.syntax_error_nodes(),
            RustAnalysisLimits::DEFAULT,
        )
        .expect("legacy analysis remains structurally valid");
        assert_eq!(
            hash_analysis_artifact_payload(&legacy).into_bytes(),
            [
                0x6B, 0x9A, 0x3D, 0x92, 0xCE, 0xDF, 0x99, 0x7D, 0xB5, 0x71, 0x6E, 0x70, 0xB2, 0xB6,
                0xC3, 0x6E, 0x1E, 0x5C, 0xE8, 0x9F, 0x11, 0xA1, 0x9D, 0x49, 0x5D, 0x98, 0xFB, 0xBA,
                0xB4, 0x85, 0x09, 0x07,
            ]
        );
    }

    #[test]
    fn source_manifest_hash_has_a_stable_golden_vector() {
        let manifest = SourceManifest::try_from_vec(
            vec![
                manifest_entry(b"src/b.rs", SourceFileKind::Regular, [2; 32]),
                manifest_entry(b"a.rs", SourceFileKind::Regular, [1; 32]),
            ],
            SourceFileLimit::new(2),
        )
        .expect("fixture manifest must be canonical");

        assert_eq!(
            hash_source_manifest(&manifest).into_bytes(),
            [
                0x84, 0x3B, 0x17, 0x44, 0x77, 0x3F, 0xEB, 0x94, 0xBC, 0xA1, 0x95, 0x9A, 0xC0, 0x0E,
                0x89, 0xBA, 0x35, 0x57, 0x35, 0x2B, 0x0A, 0xFB, 0x2F, 0x53, 0x9D, 0x07, 0xBB, 0xF0,
                0x84, 0x70, 0xE2, 0x03,
            ]
        );
    }

    #[test]
    fn every_manifest_component_changes_the_digest() {
        let base = SourceManifest::try_from_vec(
            vec![manifest_entry(b"a.rs", SourceFileKind::Regular, [1; 32])],
            SourceFileLimit::new(1),
        )
        .expect("base fixture manifest must be canonical");
        let variants = [
            SourceManifest::try_from_vec(
                vec![manifest_entry(b"b.rs", SourceFileKind::Regular, [1; 32])],
                SourceFileLimit::new(1),
            )
            .expect("path variant must be valid"),
            SourceManifest::try_from_vec(
                vec![manifest_entry(
                    b"a.rs",
                    SourceFileKind::SymbolicLink,
                    [1; 32],
                )],
                SourceFileLimit::new(1),
            )
            .expect("kind variant must be valid"),
            SourceManifest::try_from_vec(
                vec![manifest_entry(b"a.rs", SourceFileKind::Regular, [2; 32])],
                SourceFileLimit::new(1),
            )
            .expect("digest variant must be valid"),
        ];

        let expected = hash_source_manifest(&base);
        assert!(
            variants
                .iter()
                .all(|variant| hash_source_manifest(variant) != expected)
        );
    }

    #[test]
    fn every_artifact_component_changes_the_digest() {
        let base = AnalysisArtifactKey::new(
            SourceContentDigest::new([1; 32]),
            ProducerManifestDigest::new([2; 32]),
            ConfigurationDigest::new([3; 32]),
            AnalysisSchemaDigest::new([4; 32]),
            5_u32,
        );
        let expected = hash_analysis_artifact_key(&base);
        let variants = [
            AnalysisArtifactKey::new(
                SourceContentDigest::new([9; 32]),
                *base.analyzer_identity(),
                *base.configuration_identity(),
                *base.schema_identity(),
                *base.canonicalization_version(),
            ),
            AnalysisArtifactKey::new(
                *base.source_digest(),
                ProducerManifestDigest::new([9; 32]),
                *base.configuration_identity(),
                *base.schema_identity(),
                *base.canonicalization_version(),
            ),
            AnalysisArtifactKey::new(
                *base.source_digest(),
                *base.analyzer_identity(),
                ConfigurationDigest::new([9; 32]),
                *base.schema_identity(),
                *base.canonicalization_version(),
            ),
            AnalysisArtifactKey::new(
                *base.source_digest(),
                *base.analyzer_identity(),
                *base.configuration_identity(),
                AnalysisSchemaDigest::new([9; 32]),
                *base.canonicalization_version(),
            ),
            AnalysisArtifactKey::new(
                *base.source_digest(),
                *base.analyzer_identity(),
                *base.configuration_identity(),
                *base.schema_identity(),
                9_u32,
            ),
        ];

        assert!(
            variants
                .iter()
                .all(|variant| hash_analysis_artifact_key(variant) != expected)
        );
    }
}
