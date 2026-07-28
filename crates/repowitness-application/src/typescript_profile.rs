use repowitness_analysis::{
    TREE_SITTER_RUNTIME_VERSION, TREE_SITTER_TYPESCRIPT_GRAMMAR_VERSION,
    TYPESCRIPT_ANALYSIS_PROFILE_VERSION, TypeScriptDialect,
    typescript_analyzer_implementation_fingerprint_input, typescript_grammar_fingerprint_input,
};
use repowitness_domain::{AnalysisSchemaDigest, ConfigurationDigest, ProducerManifestDigest};
use sha2::{Digest, Sha256};

use crate::{
    RustArtifactIdentity, rust_index::source_preparation_implementation_fingerprint_inputs,
};

const PRODUCER_MANIFEST_DOMAIN: &[u8] = b"RepoWitness\0phase0-typescript-producer-manifest\0";
const CONFIGURATION_DOMAIN: &[u8] = b"RepoWitness\0phase0-typescript-configuration\0";
const ANALYSIS_SCHEMA_DOMAIN: &[u8] = b"RepoWitness\0phase0-typescript-analysis-schema\0";
const PHASE0_TYPESCRIPT_CONFIGURATION: &[u8] = b"typescript-paths=case-sensitive-dot-ts\0\
git-scope=tracked-and-nonignored-untracked\0\
sparse-worktree=reject\0gitlinks=reject\0syntax-errors=retain\0\
tsconfig=not-evaluated\0module-resolution=not-evaluated\0type-checking=not-performed";
const PHASE0_TSX_CONFIGURATION: &[u8] = b"tsx-paths=case-sensitive-dot-tsx\0\
git-scope=tracked-and-nonignored-untracked\0\
sparse-worktree=reject\0gitlinks=reject\0syntax-errors=retain\0\
jsx=syntax-only\0tsconfig=not-evaluated\0module-resolution=not-evaluated\0\
type-checking=not-performed";
const PHASE0_TYPESCRIPT_ANALYSIS_SCHEMA: &[u8] = b"language=typescript\0path-bytes\0\
content-digest-sha256\0artifact-digest-sha256\0symbol-kind\0name-utf8\0\
qualified-name-utf8\0name-span-u64\0declaration-span-u64\0visited-nodes-u32\0\
syntax-error-nodes-u32";
const PHASE0_TSX_ANALYSIS_SCHEMA: &[u8] = b"language=tsx\0path-bytes\0\
content-digest-sha256\0artifact-digest-sha256\0symbol-kind\0name-utf8\0\
qualified-name-utf8\0name-span-u64\0declaration-span-u64\0visited-nodes-u32\0\
syntax-error-nodes-u32";

/// Version of the canonical Phase 0 TypeScript producer-manifest encoding.
pub const PHASE0_TYPESCRIPT_PRODUCER_MANIFEST_VERSION: u32 = 2;
/// Version of the resolved, non-configurable Phase 0 TypeScript policy.
pub const PHASE0_TYPESCRIPT_CONFIGURATION_VERSION: u32 = 1;
/// Version of the persisted Phase 0 TypeScript extraction schema.
pub const PHASE0_TYPESCRIPT_ANALYSIS_SCHEMA_VERSION: u32 = 1;
/// Version of canonical persisted TypeScript fact encodings.
pub const PHASE0_TYPESCRIPT_CANONICALIZATION_VERSION: u32 = 1;

/// Constructs the complete production identity for the plain TypeScript analyzer.
#[must_use]
pub fn phase0_typescript_artifact_identity() -> RustArtifactIdentity {
    artifact_identity(TypeScriptDialect::TypeScript)
}

/// Constructs the complete production identity for the JSX-aware TSX analyzer.
#[must_use]
pub fn phase0_tsx_artifact_identity() -> RustArtifactIdentity {
    artifact_identity(TypeScriptDialect::Tsx)
}

fn artifact_identity(dialect: TypeScriptDialect) -> RustArtifactIdentity {
    RustArtifactIdentity::new(
        producer_manifest_digest(dialect),
        configuration_digest(dialect),
        analysis_schema_digest(dialect),
        PHASE0_TYPESCRIPT_CANONICALIZATION_VERSION,
    )
}

fn producer_manifest_digest(dialect: TypeScriptDialect) -> ProducerManifestDigest {
    let mut hasher = Sha256::new();
    hasher.update(PRODUCER_MANIFEST_DOMAIN);
    hasher.update(PHASE0_TYPESCRIPT_PRODUCER_MANIFEST_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, dialect_tag(dialect));
    update_length_prefixed(&mut hasher, env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(TYPESCRIPT_ANALYSIS_PROFILE_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, TREE_SITTER_RUNTIME_VERSION.as_bytes());
    update_length_prefixed(
        &mut hasher,
        TREE_SITTER_TYPESCRIPT_GRAMMAR_VERSION.as_bytes(),
    );
    update_length_prefixed(
        &mut hasher,
        typescript_analyzer_implementation_fingerprint_input(),
    );
    update_length_prefixed(&mut hasher, typescript_grammar_fingerprint_input(dialect));
    update_length_prefixed(&mut hasher, include_bytes!("rust_index.rs"));
    for input in source_preparation_implementation_fingerprint_inputs() {
        update_length_prefixed(&mut hasher, input);
    }
    update_length_prefixed(&mut hasher, include_bytes!("canonical_digest.rs"));
    update_length_prefixed(&mut hasher, include_bytes!("typescript_profile.rs"));
    ProducerManifestDigest::new(hasher.finalize().into())
}

fn configuration_digest(dialect: TypeScriptDialect) -> ConfigurationDigest {
    let mut hasher = Sha256::new();
    hasher.update(CONFIGURATION_DOMAIN);
    hasher.update(PHASE0_TYPESCRIPT_CONFIGURATION_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, dialect_tag(dialect));
    update_length_prefixed(
        &mut hasher,
        match dialect {
            TypeScriptDialect::TypeScript => PHASE0_TYPESCRIPT_CONFIGURATION,
            TypeScriptDialect::Tsx => PHASE0_TSX_CONFIGURATION,
        },
    );
    ConfigurationDigest::new(hasher.finalize().into())
}

fn analysis_schema_digest(dialect: TypeScriptDialect) -> AnalysisSchemaDigest {
    let mut hasher = Sha256::new();
    hasher.update(ANALYSIS_SCHEMA_DOMAIN);
    hasher.update(PHASE0_TYPESCRIPT_ANALYSIS_SCHEMA_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, dialect_tag(dialect));
    update_length_prefixed(
        &mut hasher,
        match dialect {
            TypeScriptDialect::TypeScript => PHASE0_TYPESCRIPT_ANALYSIS_SCHEMA,
            TypeScriptDialect::Tsx => PHASE0_TSX_ANALYSIS_SCHEMA,
        },
    );
    AnalysisSchemaDigest::new(hasher.finalize().into())
}

const fn dialect_tag(dialect: TypeScriptDialect) -> &'static [u8] {
    match dialect {
        TypeScriptDialect::TypeScript => b"typescript",
        TypeScriptDialect::Tsx => b"tsx",
    }
}

fn update_length_prefixed(hasher: &mut Sha256, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("static producer inputs fit in u64");
    hasher.update(length.to_be_bytes());
    hasher.update(value);
}

#[cfg(test)]
mod tests {
    use super::{
        PHASE0_TYPESCRIPT_ANALYSIS_SCHEMA_VERSION, PHASE0_TYPESCRIPT_CANONICALIZATION_VERSION,
        PHASE0_TYPESCRIPT_CONFIGURATION_VERSION, PHASE0_TYPESCRIPT_PRODUCER_MANIFEST_VERSION,
        phase0_tsx_artifact_identity, phase0_typescript_artifact_identity,
    };

    #[test]
    fn dialect_identities_are_stable_complete_and_distinct() {
        let typescript = phase0_typescript_artifact_identity();
        let tsx = phase0_tsx_artifact_identity();

        assert_eq!(typescript, phase0_typescript_artifact_identity());
        assert_eq!(tsx, phase0_tsx_artifact_identity());
        assert_ne!(typescript, tsx);
        for identity in [typescript, tsx] {
            assert_ne!(identity.producer_manifest().as_bytes(), &[0; 32]);
            assert_ne!(identity.configuration().as_bytes(), &[0; 32]);
            assert_ne!(identity.schema().as_bytes(), &[0; 32]);
            assert_eq!(
                identity.canonicalization_version(),
                PHASE0_TYPESCRIPT_CANONICALIZATION_VERSION
            );
        }
        assert_eq!(PHASE0_TYPESCRIPT_PRODUCER_MANIFEST_VERSION, 2);
        assert_eq!(PHASE0_TYPESCRIPT_CONFIGURATION_VERSION, 1);
        assert_eq!(PHASE0_TYPESCRIPT_ANALYSIS_SCHEMA_VERSION, 1);
    }
}
