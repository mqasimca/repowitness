use repowitness_analysis::{
    GO_ANALYSIS_PROFILE_VERSION, TREE_SITTER_GO_GRAMMAR_VERSION, TREE_SITTER_RUNTIME_VERSION,
    go_analyzer_implementation_fingerprint_input, go_grammar_fingerprint_input,
};
use repowitness_domain::{AnalysisSchemaDigest, ConfigurationDigest, ProducerManifestDigest};
use sha2::{Digest, Sha256};

use crate::{
    RustArtifactIdentity, rust_index::source_preparation_implementation_fingerprint_inputs,
};

const PRODUCER_MANIFEST_DOMAIN: &[u8] = b"RepoWitness\0phase0-go-producer-manifest\0";
const CONFIGURATION_DOMAIN: &[u8] = b"RepoWitness\0phase0-go-configuration\0";
const ANALYSIS_SCHEMA_DOMAIN: &[u8] = b"RepoWitness\0phase0-go-analysis-schema\0";
const PHASE0_GO_CONFIGURATION: &[u8] = b"go-paths=case-sensitive-dot-go\0\
git-scope=tracked-and-nonignored-untracked\0\
sparse-worktree=reject\0gitlinks=reject\0syntax-errors=retain\0\
build-constraints=not-evaluated";
const PHASE0_GO_ANALYSIS_SCHEMA: &[u8] = b"language=go\0path-bytes\0\
content-digest-sha256\0artifact-digest-sha256\0symbol-kind\0name-utf8\0\
qualified-name-utf8\0name-span-u64\0declaration-span-u64\0visited-nodes-u32\0\
syntax-error-nodes-u32";

/// Version of the canonical Phase 0 Go producer-manifest encoding.
pub const PHASE0_GO_PRODUCER_MANIFEST_VERSION: u32 = 2;
/// Version of the resolved, non-configurable Phase 0 Go policy.
pub const PHASE0_GO_CONFIGURATION_VERSION: u32 = 1;
/// Version of the persisted Phase 0 Go extraction schema.
pub const PHASE0_GO_ANALYSIS_SCHEMA_VERSION: u32 = 1;
/// Version of canonical persisted Go fact encodings.
pub const PHASE0_GO_CANONICALIZATION_VERSION: u32 = 1;

/// Constructs the complete production identity for the Phase 0 Go analyzer.
#[must_use]
pub fn phase0_go_artifact_identity() -> RustArtifactIdentity {
    RustArtifactIdentity::new(
        producer_manifest_digest(),
        configuration_digest(),
        analysis_schema_digest(),
        PHASE0_GO_CANONICALIZATION_VERSION,
    )
}

fn producer_manifest_digest() -> ProducerManifestDigest {
    let mut hasher = Sha256::new();
    hasher.update(PRODUCER_MANIFEST_DOMAIN);
    hasher.update(PHASE0_GO_PRODUCER_MANIFEST_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(GO_ANALYSIS_PROFILE_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, TREE_SITTER_RUNTIME_VERSION.as_bytes());
    update_length_prefixed(&mut hasher, TREE_SITTER_GO_GRAMMAR_VERSION.as_bytes());
    update_length_prefixed(&mut hasher, go_analyzer_implementation_fingerprint_input());
    update_length_prefixed(&mut hasher, go_grammar_fingerprint_input());
    update_length_prefixed(&mut hasher, include_bytes!("rust_index.rs"));
    for input in source_preparation_implementation_fingerprint_inputs() {
        update_length_prefixed(&mut hasher, input);
    }
    update_length_prefixed(&mut hasher, include_bytes!("canonical_digest.rs"));
    update_length_prefixed(&mut hasher, include_bytes!("go_profile.rs"));
    ProducerManifestDigest::new(hasher.finalize().into())
}

fn configuration_digest() -> ConfigurationDigest {
    let mut hasher = Sha256::new();
    hasher.update(CONFIGURATION_DOMAIN);
    hasher.update(PHASE0_GO_CONFIGURATION_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, PHASE0_GO_CONFIGURATION);
    ConfigurationDigest::new(hasher.finalize().into())
}

fn analysis_schema_digest() -> AnalysisSchemaDigest {
    let mut hasher = Sha256::new();
    hasher.update(ANALYSIS_SCHEMA_DOMAIN);
    hasher.update(PHASE0_GO_ANALYSIS_SCHEMA_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, PHASE0_GO_ANALYSIS_SCHEMA);
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
        PHASE0_GO_ANALYSIS_SCHEMA_VERSION, PHASE0_GO_CANONICALIZATION_VERSION,
        PHASE0_GO_CONFIGURATION_VERSION, PHASE0_GO_PRODUCER_MANIFEST_VERSION,
        phase0_go_artifact_identity,
    };

    #[test]
    fn production_identity_is_stable_complete_and_non_placeholder() {
        let first = phase0_go_artifact_identity();
        let second = phase0_go_artifact_identity();

        assert_eq!(first, second);
        assert_ne!(first.producer_manifest().as_bytes(), &[0; 32]);
        assert_ne!(first.configuration().as_bytes(), &[0; 32]);
        assert_ne!(first.schema().as_bytes(), &[0; 32]);
        assert_ne!(
            first.producer_manifest().as_bytes(),
            first.configuration().as_bytes()
        );
        assert_eq!(
            first.canonicalization_version(),
            PHASE0_GO_CANONICALIZATION_VERSION
        );
        assert_eq!(PHASE0_GO_PRODUCER_MANIFEST_VERSION, 2);
        assert_eq!(PHASE0_GO_CONFIGURATION_VERSION, 1);
        assert_eq!(PHASE0_GO_ANALYSIS_SCHEMA_VERSION, 1);
    }
}
