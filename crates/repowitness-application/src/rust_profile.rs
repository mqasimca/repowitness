use repowitness_analysis::{
    RUST_ANALYSIS_PROFILE_VERSION, TREE_SITTER_RUNTIME_VERSION, TREE_SITTER_RUST_GRAMMAR_VERSION,
    rust_analyzer_implementation_fingerprint_input, rust_grammar_fingerprint_input,
};
use repowitness_domain::{AnalysisSchemaDigest, ConfigurationDigest, ProducerManifestDigest};
use sha2::{Digest, Sha256};

use crate::RustArtifactIdentity;

const PRODUCER_MANIFEST_DOMAIN: &[u8] = b"RepoWitness\0phase0-rust-producer-manifest\0";
const CONFIGURATION_DOMAIN: &[u8] = b"RepoWitness\0phase0-rust-configuration\0";
const ANALYSIS_SCHEMA_DOMAIN: &[u8] = b"RepoWitness\0phase0-rust-analysis-schema\0";
const PHASE0_RUST_CONFIGURATION: &[u8] = b"rust-paths=case-sensitive-dot-rs\0\
git-scope=tracked-and-nonignored-untracked\0\
sparse-worktree=reject\0gitlinks=reject\0syntax-errors=retain";
const PHASE0_RUST_ANALYSIS_SCHEMA: &[u8] = b"path-bytes\0content-digest-sha256\0\
artifact-digest-sha256\0symbol-kind\0name-utf8\0qualified-name-utf8\0\
name-span-u64\0declaration-span-u64\0visited-nodes-u32\0syntax-error-nodes-u32";

/// Version of the canonical Phase 0 Rust producer-manifest encoding.
pub const PHASE0_RUST_PRODUCER_MANIFEST_VERSION: u32 = 1;
/// Version of the resolved, non-configurable Phase 0 Rust policy.
pub const PHASE0_RUST_CONFIGURATION_VERSION: u32 = 1;
/// Version of the persisted Phase 0 Rust extraction schema.
pub const PHASE0_RUST_ANALYSIS_SCHEMA_VERSION: u32 = 1;
/// Version of canonical persisted Rust fact encodings.
pub const PHASE0_RUST_CANONICALIZATION_VERSION: u32 = 1;

/// Constructs the complete production identity for the Phase 0 Rust analyzer.
///
/// The producer digest includes exact analyzer source and grammar schema bytes,
/// so a changed build cannot silently reuse artifacts from different behavior.
#[must_use]
pub fn phase0_rust_artifact_identity() -> RustArtifactIdentity {
    RustArtifactIdentity::new(
        producer_manifest_digest(),
        configuration_digest(),
        analysis_schema_digest(),
        PHASE0_RUST_CANONICALIZATION_VERSION,
    )
}

fn producer_manifest_digest() -> ProducerManifestDigest {
    let mut hasher = Sha256::new();
    hasher.update(PRODUCER_MANIFEST_DOMAIN);
    hasher.update(PHASE0_RUST_PRODUCER_MANIFEST_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(RUST_ANALYSIS_PROFILE_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, TREE_SITTER_RUNTIME_VERSION.as_bytes());
    update_length_prefixed(&mut hasher, TREE_SITTER_RUST_GRAMMAR_VERSION.as_bytes());
    update_length_prefixed(
        &mut hasher,
        rust_analyzer_implementation_fingerprint_input(),
    );
    update_length_prefixed(&mut hasher, rust_grammar_fingerprint_input());
    update_length_prefixed(&mut hasher, include_bytes!("rust_index.rs"));
    update_length_prefixed(&mut hasher, include_bytes!("canonical_digest.rs"));
    update_length_prefixed(&mut hasher, include_bytes!("rust_profile.rs"));
    ProducerManifestDigest::new(hasher.finalize().into())
}

fn configuration_digest() -> ConfigurationDigest {
    let mut hasher = Sha256::new();
    hasher.update(CONFIGURATION_DOMAIN);
    hasher.update(PHASE0_RUST_CONFIGURATION_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, PHASE0_RUST_CONFIGURATION);
    ConfigurationDigest::new(hasher.finalize().into())
}

fn analysis_schema_digest() -> AnalysisSchemaDigest {
    let mut hasher = Sha256::new();
    hasher.update(ANALYSIS_SCHEMA_DOMAIN);
    hasher.update(PHASE0_RUST_ANALYSIS_SCHEMA_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, PHASE0_RUST_ANALYSIS_SCHEMA);
    AnalysisSchemaDigest::new(hasher.finalize().into())
}

fn update_length_prefixed(hasher: &mut Sha256, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("static producer inputs fit in u64");
    hasher.update(length.to_be_bytes());
    hasher.update(value);
}

#[cfg(test)]
mod tests {
    use super::{
        PHASE0_RUST_ANALYSIS_SCHEMA_VERSION, PHASE0_RUST_CANONICALIZATION_VERSION,
        PHASE0_RUST_CONFIGURATION_VERSION, PHASE0_RUST_PRODUCER_MANIFEST_VERSION,
        phase0_rust_artifact_identity,
    };

    #[test]
    fn production_identity_is_stable_complete_and_non_placeholder() {
        let first = phase0_rust_artifact_identity();
        let second = phase0_rust_artifact_identity();

        assert_eq!(first, second);
        assert_ne!(first.producer_manifest().as_bytes(), &[0; 32]);
        assert_ne!(first.configuration().as_bytes(), &[0; 32]);
        assert_ne!(first.schema().as_bytes(), &[0; 32]);
        assert_ne!(
            first.producer_manifest().as_bytes(),
            first.configuration().as_bytes()
        );
        assert_ne!(first.configuration().as_bytes(), first.schema().as_bytes());
        assert_eq!(
            first.canonicalization_version(),
            PHASE0_RUST_CANONICALIZATION_VERSION
        );
        assert_eq!(PHASE0_RUST_PRODUCER_MANIFEST_VERSION, 1);
        assert_eq!(PHASE0_RUST_CONFIGURATION_VERSION, 1);
        assert_eq!(PHASE0_RUST_ANALYSIS_SCHEMA_VERSION, 1);
    }
}
