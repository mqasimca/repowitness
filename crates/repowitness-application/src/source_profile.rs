use repowitness_domain::{AnalysisSchemaDigest, ConfigurationDigest, ProducerManifestDigest};
use sha2::{Digest, Sha256};

use crate::{
    RustArtifactIdentity, SourceArtifactIdentities, SourceLanguage, phase0_go_artifact_identity,
    phase0_python_artifact_identity, phase0_rust_artifact_identity, phase0_tsx_artifact_identity,
    phase0_typescript_artifact_identity,
};

const PRODUCER_DOMAIN: &[u8] = b"RepoWitness\0phase0-supported-languages-snapshot-producer\0";
const CONFIGURATION_DOMAIN: &[u8] =
    b"RepoWitness\0phase0-supported-languages-snapshot-configuration\0";
const SCHEMA_DOMAIN: &[u8] = b"RepoWitness\0phase0-supported-languages-snapshot-schema\0";
const SELECTION_POLICY: &[u8] =
    b"regular-case-sensitive-dot-rs-dot-go-dot-ts-dot-tsx-dot-py-or-dot-pyi\0\
one-canonical-manifest\0one-generation";

/// Version of the combined supported-language snapshot profile.
pub const PHASE0_SOURCE_SNAPSHOT_PROFILE_VERSION: u32 = 3;
/// Version of the combined snapshot canonicalization.
pub const PHASE0_SOURCE_CANONICALIZATION_VERSION: u32 = 3;

/// Combined non-repository inputs for one mixed-language snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSnapshotProfile {
    configuration: ConfigurationDigest,
    producer_manifest: ProducerManifestDigest,
    analysis_schema: AnalysisSchemaDigest,
    canonicalization_version: u32,
}

impl SourceSnapshotProfile {
    /// Returns the combined source-selection and analyzer configuration.
    #[must_use]
    pub const fn configuration(self) -> ConfigurationDigest {
        self.configuration
    }

    /// Returns the combined producer identity.
    #[must_use]
    pub const fn producer_manifest(self) -> ProducerManifestDigest {
        self.producer_manifest
    }

    /// Returns the combined persisted analysis schema identity.
    #[must_use]
    pub const fn analysis_schema(self) -> AnalysisSchemaDigest {
        self.analysis_schema
    }

    /// Returns the combined canonicalization version.
    #[must_use]
    pub const fn canonicalization_version(self) -> u32 {
        self.canonicalization_version
    }
}

/// Returns the independent exact artifact identities for every supported language.
#[must_use]
pub fn phase0_source_artifact_identities() -> SourceArtifactIdentities {
    SourceArtifactIdentities::new(
        phase0_rust_artifact_identity(),
        phase0_go_artifact_identity(),
        phase0_typescript_artifact_identity(),
        phase0_tsx_artifact_identity(),
        phase0_python_artifact_identity(),
    )
}

/// Returns the stable profile committed by a mixed-language source snapshot.
#[must_use]
pub fn phase0_source_snapshot_profile() -> SourceSnapshotProfile {
    let identities = phase0_source_artifact_identities();
    SourceSnapshotProfile {
        configuration: combined_configuration(identities),
        producer_manifest: combined_producer(identities),
        analysis_schema: combined_schema(identities),
        canonicalization_version: PHASE0_SOURCE_CANONICALIZATION_VERSION,
    }
}

fn combined_producer(identities: SourceArtifactIdentities) -> ProducerManifestDigest {
    let mut hasher = Sha256::new();
    hasher.update(PRODUCER_DOMAIN);
    hasher.update(PHASE0_SOURCE_SNAPSHOT_PROFILE_VERSION.to_be_bytes());
    update_artifact_identity(&mut hasher, identities.for_language(SourceLanguage::Rust));
    update_artifact_identity(&mut hasher, identities.for_language(SourceLanguage::Go));
    update_artifact_identity(
        &mut hasher,
        identities.for_language(SourceLanguage::TypeScript),
    );
    update_artifact_identity(&mut hasher, identities.for_language(SourceLanguage::Tsx));
    update_artifact_identity(&mut hasher, identities.for_language(SourceLanguage::Python));
    update_length_prefixed(&mut hasher, include_bytes!("source_profile.rs"));
    ProducerManifestDigest::new(hasher.finalize().into())
}

fn combined_configuration(identities: SourceArtifactIdentities) -> ConfigurationDigest {
    let mut hasher = Sha256::new();
    hasher.update(CONFIGURATION_DOMAIN);
    hasher.update(PHASE0_SOURCE_SNAPSHOT_PROFILE_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, SELECTION_POLICY);
    hasher.update(
        identities
            .for_language(SourceLanguage::Rust)
            .configuration()
            .as_bytes(),
    );
    hasher.update(
        identities
            .for_language(SourceLanguage::Go)
            .configuration()
            .as_bytes(),
    );
    hasher.update(
        identities
            .for_language(SourceLanguage::TypeScript)
            .configuration()
            .as_bytes(),
    );
    hasher.update(
        identities
            .for_language(SourceLanguage::Tsx)
            .configuration()
            .as_bytes(),
    );
    hasher.update(
        identities
            .for_language(SourceLanguage::Python)
            .configuration()
            .as_bytes(),
    );
    ConfigurationDigest::new(hasher.finalize().into())
}

fn combined_schema(identities: SourceArtifactIdentities) -> AnalysisSchemaDigest {
    let mut hasher = Sha256::new();
    hasher.update(SCHEMA_DOMAIN);
    hasher.update(PHASE0_SOURCE_SNAPSHOT_PROFILE_VERSION.to_be_bytes());
    hasher.update(
        identities
            .for_language(SourceLanguage::Rust)
            .schema()
            .as_bytes(),
    );
    hasher.update(
        identities
            .for_language(SourceLanguage::Go)
            .schema()
            .as_bytes(),
    );
    hasher.update(
        identities
            .for_language(SourceLanguage::TypeScript)
            .schema()
            .as_bytes(),
    );
    hasher.update(
        identities
            .for_language(SourceLanguage::Tsx)
            .schema()
            .as_bytes(),
    );
    hasher.update(
        identities
            .for_language(SourceLanguage::Python)
            .schema()
            .as_bytes(),
    );
    update_length_prefixed(
        &mut hasher,
        b"artifact-language=rust-go-typescript-tsx-or-python\0\
go-kinds=interface-defined_type-variable\0\
typescript-kinds=class-interface-enum-type_alias-module-function-method-variable\0\
python-kinds=class-function-method-variable-type_alias",
    );
    AnalysisSchemaDigest::new(hasher.finalize().into())
}

fn update_artifact_identity(hasher: &mut Sha256, identity: RustArtifactIdentity) {
    hasher.update(identity.producer_manifest().as_bytes());
    hasher.update(identity.configuration().as_bytes());
    hasher.update(identity.schema().as_bytes());
    hasher.update(identity.canonicalization_version().to_be_bytes());
}

fn update_length_prefixed(hasher: &mut Sha256, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("static producer inputs fit in u64");
    hasher.update(length.to_be_bytes());
    hasher.update(value);
}

#[cfg(test)]
mod tests {
    use crate::{SourceLanguage, phase0_source_artifact_identities};

    use super::{
        PHASE0_SOURCE_CANONICALIZATION_VERSION, PHASE0_SOURCE_SNAPSHOT_PROFILE_VERSION,
        phase0_source_snapshot_profile,
    };

    #[test]
    fn mixed_profile_is_stable_and_distinct_from_each_language() {
        let first = phase0_source_snapshot_profile();
        let second = phase0_source_snapshot_profile();
        let identities = phase0_source_artifact_identities();

        assert_eq!(first, second);
        assert_ne!(
            first.producer_manifest(),
            identities
                .for_language(SourceLanguage::Rust)
                .producer_manifest()
        );
        assert_ne!(
            first.producer_manifest(),
            identities
                .for_language(SourceLanguage::Go)
                .producer_manifest()
        );
        assert_ne!(
            first.producer_manifest(),
            identities
                .for_language(SourceLanguage::TypeScript)
                .producer_manifest()
        );
        assert_ne!(
            first.producer_manifest(),
            identities
                .for_language(SourceLanguage::Tsx)
                .producer_manifest()
        );
        assert_ne!(
            first.producer_manifest(),
            identities
                .for_language(SourceLanguage::Python)
                .producer_manifest()
        );
        assert_eq!(
            first.canonicalization_version(),
            PHASE0_SOURCE_CANONICALIZATION_VERSION
        );
        assert_eq!(PHASE0_SOURCE_SNAPSHOT_PROFILE_VERSION, 3);
    }
}
