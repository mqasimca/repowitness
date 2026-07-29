use repowitness_analysis::{
    RUST_GRAPH_SITE_PROFILE_VERSION, TREE_SITTER_RUNTIME_VERSION, TREE_SITTER_RUST_GRAMMAR_VERSION,
    rust_grammar_fingerprint_input, rust_graph_site_extraction_fingerprint_input,
    rust_graph_site_implementation_fingerprint_input, rust_graph_site_traversal_fingerprint_input,
};
use repowitness_domain::{AnalysisSchemaDigest, ConfigurationDigest, ProducerManifestDigest};
use sha2::{Digest, Sha256};

use crate::RustArtifactIdentity;

const PRODUCER_MANIFEST_DOMAIN: &[u8] = b"RepoWitness\0phase1-rust-graph-producer-manifest\0";
const CONFIGURATION_DOMAIN: &[u8] = b"RepoWitness\0phase1-rust-graph-configuration\0";
const ANALYSIS_SCHEMA_DOMAIN: &[u8] = b"RepoWitness\0phase1-rust-graph-analysis-schema\0";
const PHASE1_RUST_GRAPH_CONFIGURATION: &[u8] = b"language=rust\0\
paths=case-sensitive-dot-rs\0conditional-syntax=retain\0macro-expansion=none\0\
sites=import,reference,call,macro-call,test-marker\0resolution=not-in-artifact";
const PHASE1_RUST_GRAPH_ANALYSIS_SCHEMA: &[u8] = b"path-bytes\0\
content-digest-sha256\0artifact-digest-sha256\0source-order-ordinal-u32\0\
site-kind\0extraction-evidence\0target-utf8\0occurrence-span-u64\0\
target-span-u64\0enclosing-kind\0enclosing-name-utf8\0\
enclosing-qualified-name-utf8\0enclosing-name-span-u64\0\
enclosing-declaration-span-u64\0visited-nodes-u32\0syntax-error-nodes-u32";

/// Version of the canonical Phase 1 Rust graph producer-manifest encoding.
pub const PHASE1_RUST_GRAPH_PRODUCER_MANIFEST_VERSION: u32 = 1;
/// Version of the semantics-affecting Rust graph-site policy.
pub const PHASE1_RUST_GRAPH_CONFIGURATION_VERSION: u32 = 1;
/// Version of the persisted raw Rust graph-site schema.
pub const PHASE1_RUST_GRAPH_ANALYSIS_SCHEMA_VERSION: u32 = 1;
/// Version of canonical persisted raw Rust graph-site encodings.
pub const PHASE1_RUST_GRAPH_CANONICALIZATION_VERSION: u32 = 1;

/// Constructs the complete production identity for raw Phase 1 Rust graph sites.
///
/// Generation-scoped resolution has a separate profile and never becomes part
/// of this reusable content-local artifact identity.
#[must_use]
pub fn phase1_rust_graph_artifact_identity() -> RustArtifactIdentity {
    RustArtifactIdentity::new(
        producer_manifest_digest(),
        configuration_digest(),
        analysis_schema_digest(),
        PHASE1_RUST_GRAPH_CANONICALIZATION_VERSION,
    )
}

fn producer_manifest_digest() -> ProducerManifestDigest {
    let mut hasher = Sha256::new();
    hasher.update(PRODUCER_MANIFEST_DOMAIN);
    hasher.update(PHASE1_RUST_GRAPH_PRODUCER_MANIFEST_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(RUST_GRAPH_SITE_PROFILE_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, TREE_SITTER_RUNTIME_VERSION.as_bytes());
    update_length_prefixed(&mut hasher, TREE_SITTER_RUST_GRAMMAR_VERSION.as_bytes());
    update_length_prefixed(&mut hasher, rust_grammar_fingerprint_input());
    update_length_prefixed(
        &mut hasher,
        rust_graph_site_implementation_fingerprint_input(),
    );
    update_length_prefixed(&mut hasher, rust_graph_site_traversal_fingerprint_input());
    update_length_prefixed(&mut hasher, rust_graph_site_extraction_fingerprint_input());
    update_length_prefixed(&mut hasher, include_bytes!("rust_graph_profile.rs"));
    ProducerManifestDigest::new(hasher.finalize().into())
}

fn configuration_digest() -> ConfigurationDigest {
    let mut hasher = Sha256::new();
    hasher.update(CONFIGURATION_DOMAIN);
    hasher.update(PHASE1_RUST_GRAPH_CONFIGURATION_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, PHASE1_RUST_GRAPH_CONFIGURATION);
    ConfigurationDigest::new(hasher.finalize().into())
}

fn analysis_schema_digest() -> AnalysisSchemaDigest {
    let mut hasher = Sha256::new();
    hasher.update(ANALYSIS_SCHEMA_DOMAIN);
    hasher.update(PHASE1_RUST_GRAPH_ANALYSIS_SCHEMA_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, PHASE1_RUST_GRAPH_ANALYSIS_SCHEMA);
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
        PHASE1_RUST_GRAPH_ANALYSIS_SCHEMA_VERSION, PHASE1_RUST_GRAPH_CANONICALIZATION_VERSION,
        PHASE1_RUST_GRAPH_CONFIGURATION_VERSION, PHASE1_RUST_GRAPH_PRODUCER_MANIFEST_VERSION,
        phase1_rust_graph_artifact_identity,
    };

    #[test]
    fn graph_identity_is_stable_complete_and_separate_from_declarations() {
        let first = phase1_rust_graph_artifact_identity();
        let second = phase1_rust_graph_artifact_identity();

        assert_eq!(first, second);
        assert_ne!(first.producer_manifest().as_bytes(), &[0; 32]);
        assert_ne!(first.configuration().as_bytes(), &[0; 32]);
        assert_ne!(first.schema().as_bytes(), &[0; 32]);
        assert_ne!(first, crate::phase0_rust_artifact_identity());
        assert_eq!(
            first.canonicalization_version(),
            PHASE1_RUST_GRAPH_CANONICALIZATION_VERSION
        );
        assert_eq!(PHASE1_RUST_GRAPH_PRODUCER_MANIFEST_VERSION, 1);
        assert_eq!(PHASE1_RUST_GRAPH_CONFIGURATION_VERSION, 1);
        assert_eq!(PHASE1_RUST_GRAPH_ANALYSIS_SCHEMA_VERSION, 1);
        assert_eq!(PHASE1_RUST_GRAPH_CANONICALIZATION_VERSION, 1);
    }
}
