//! Independent producer identity for all-language raw syntax-site artifacts.

use repowitness_analysis::{
    RAW_SYNTAX_SITE_PROFILE_VERSION, RawSyntaxLanguage, TREE_SITTER_RUNTIME_VERSION,
    raw_syntax_grammar_fingerprint_input, raw_syntax_site_implementation_fingerprint_input,
};
use repowitness_domain::{AnalysisSchemaDigest, ConfigurationDigest, ProducerManifestDigest};
use sha2::{Digest, Sha256};

use crate::{RustArtifactIdentity, SourceArtifactIdentities, SourceLanguage};

const PRODUCER_MANIFEST_DOMAIN: &[u8] = b"RepoWitness\0phase2-raw-syntax-producer-manifest\0";
const CONFIGURATION_DOMAIN: &[u8] = b"RepoWitness\0phase2-raw-syntax-configuration\0";
const ANALYSIS_SCHEMA_DOMAIN: &[u8] = b"RepoWitness\0phase2-raw-syntax-analysis-schema\0";
const CONFIGURATION: &[u8] = b"target-encoding=utf8\0\
sites=import,reference,call,test-marker\0\
reference-rust=bounded-syntax-heuristic\0\
reference-go=unsupported\0reference-typescript=unsupported\0\
reference-tsx=unsupported\0reference-python=unsupported\0\
test-marker-rust=attribute-only\0test-marker-other=unsupported\0\
resolution=not-produced";
const ANALYSIS_SCHEMA: &[u8] = b"language\0path-bytes\0content-digest-sha256\0\
artifact-digest-sha256\0source-order-ordinal-u32\0site-kind\0\
extraction-evidence\0target-utf8\0occurrence-span-u64\0target-span-u64\0\
visited-nodes-u32\0syntax-error-nodes-u32\0max-depth-u16\0owned-text-bytes-u64\0\
per-kind-support\0per-kind-emitted";

/// Version of the raw-site producer-manifest encoding.
pub const RAW_SYNTAX_PRODUCER_MANIFEST_VERSION: u32 = 1;
/// Version of the semantics-affecting raw-site policy.
pub const RAW_SYNTAX_CONFIGURATION_VERSION: u32 = 1;
/// Version of the raw-site persistence schema.
pub const RAW_SYNTAX_ANALYSIS_SCHEMA_VERSION: u32 = 1;
/// Version of canonical raw-site payload encodings.
pub const RAW_SYNTAX_CANONICALIZATION_VERSION: u32 = 1;

/// Returns independent raw-site identities for every supported language/dialect.
#[must_use]
pub fn raw_syntax_artifact_identities() -> SourceArtifactIdentities {
    SourceArtifactIdentities::new(
        raw_syntax_artifact_identity(SourceLanguage::Rust),
        raw_syntax_artifact_identity(SourceLanguage::Go),
        raw_syntax_artifact_identity(SourceLanguage::TypeScript),
        raw_syntax_artifact_identity(SourceLanguage::Tsx),
        raw_syntax_artifact_identity(SourceLanguage::Python),
    )
}

/// Returns the complete identity for one raw-site language/dialect artifact.
#[must_use]
pub fn raw_syntax_artifact_identity(language: SourceLanguage) -> RustArtifactIdentity {
    let raw_language = raw_language(language);
    RustArtifactIdentity::new(
        producer_manifest_digest(raw_language),
        configuration_digest(raw_language),
        analysis_schema_digest(),
        RAW_SYNTAX_CANONICALIZATION_VERSION,
    )
}

fn raw_language(language: SourceLanguage) -> RawSyntaxLanguage {
    match language {
        SourceLanguage::Rust => RawSyntaxLanguage::Rust,
        SourceLanguage::Go => RawSyntaxLanguage::Go,
        SourceLanguage::TypeScript => RawSyntaxLanguage::TypeScript,
        SourceLanguage::Tsx => RawSyntaxLanguage::Tsx,
        SourceLanguage::Python => RawSyntaxLanguage::Python,
    }
}

fn producer_manifest_digest(language: RawSyntaxLanguage) -> ProducerManifestDigest {
    let mut hasher = Sha256::new();
    hasher.update(PRODUCER_MANIFEST_DOMAIN);
    hasher.update(RAW_SYNTAX_PRODUCER_MANIFEST_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, env!("CARGO_PKG_VERSION").as_bytes());
    update_length_prefixed(&mut hasher, language.as_str().as_bytes());
    hasher.update(RAW_SYNTAX_SITE_PROFILE_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, TREE_SITTER_RUNTIME_VERSION.as_bytes());
    update_length_prefixed(&mut hasher, raw_syntax_grammar_fingerprint_input(language));
    update_length_prefixed(
        &mut hasher,
        raw_syntax_site_implementation_fingerprint_input(),
    );
    update_length_prefixed(&mut hasher, include_bytes!("raw_syntax_profile.rs"));
    ProducerManifestDigest::new(hasher.finalize().into())
}

fn configuration_digest(language: RawSyntaxLanguage) -> ConfigurationDigest {
    let mut hasher = Sha256::new();
    hasher.update(CONFIGURATION_DOMAIN);
    hasher.update(RAW_SYNTAX_CONFIGURATION_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, language.as_str().as_bytes());
    update_length_prefixed(&mut hasher, CONFIGURATION);
    ConfigurationDigest::new(hasher.finalize().into())
}

fn analysis_schema_digest() -> AnalysisSchemaDigest {
    let mut hasher = Sha256::new();
    hasher.update(ANALYSIS_SCHEMA_DOMAIN);
    hasher.update(RAW_SYNTAX_ANALYSIS_SCHEMA_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, ANALYSIS_SCHEMA);
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
        RAW_SYNTAX_ANALYSIS_SCHEMA_VERSION, RAW_SYNTAX_CANONICALIZATION_VERSION,
        RAW_SYNTAX_CONFIGURATION_VERSION, RAW_SYNTAX_PRODUCER_MANIFEST_VERSION,
        raw_syntax_artifact_identities,
    };
    use crate::SourceLanguage;

    #[test]
    fn identities_are_stable_and_distinct_from_source_declarations() {
        let first = raw_syntax_artifact_identities();
        let second = raw_syntax_artifact_identities();
        assert_eq!(first, second);
        assert_ne!(
            first.for_language(SourceLanguage::Rust),
            crate::phase0_source_artifact_identities().for_language(SourceLanguage::Rust)
        );
        assert_ne!(
            first.for_language(SourceLanguage::TypeScript),
            first.for_language(SourceLanguage::Tsx)
        );
        assert_eq!(RAW_SYNTAX_PRODUCER_MANIFEST_VERSION, 1);
        assert_eq!(RAW_SYNTAX_CONFIGURATION_VERSION, 1);
        assert_eq!(RAW_SYNTAX_ANALYSIS_SCHEMA_VERSION, 1);
        assert_eq!(RAW_SYNTAX_CANONICALIZATION_VERSION, 1);
    }
}
