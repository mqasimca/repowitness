//! Complete logical keys for reusable immutable analysis artifacts.

/// The semantic version of the analysis-artifact key contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnalysisArtifactKeyVersion(u16);

impl AnalysisArtifactKeyVersion {
    /// The initial analysis-artifact key contract.
    pub const V1: Self = Self(1);

    /// Returns the fixed-width version number.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Every logical input required to identify a reusable analysis artifact.
///
/// `D` is the exact source-content digest, `A` the complete adapter, grammar,
/// and producer identity, `C` the resolved semantics-affecting configuration,
/// `S` the extraction schema identity, and `V` the canonicalization version.
/// Concrete validated components enforce their own bounds.
///
/// This structure is the logical key, not its persisted digest encoding. A
/// boundary may hash a versioned, domain-separated canonical representation,
/// but must not omit or reorder these semantic components. Derived ordering
/// follows the field order above for deterministic in-memory inventories; it
/// is not a persisted encoding.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AnalysisArtifactKey<D, A, C, S, V> {
    source_digest: D,
    analyzer_identity: A,
    configuration_identity: C,
    schema_identity: S,
    canonicalization_version: V,
}

impl<D, A, C, S, V> AnalysisArtifactKey<D, A, C, S, V> {
    /// The semantic version implemented by this logical key.
    pub const VERSION: AnalysisArtifactKeyVersion = AnalysisArtifactKeyVersion::V1;

    /// Creates a key from every already-validated semantic input.
    #[must_use]
    pub const fn new(
        source_digest: D,
        analyzer_identity: A,
        configuration_identity: C,
        schema_identity: S,
        canonicalization_version: V,
    ) -> Self {
        Self {
            source_digest,
            analyzer_identity,
            configuration_identity,
            schema_identity,
            canonicalization_version,
        }
    }

    /// Returns the semantic version implemented by this logical key.
    #[must_use]
    pub const fn version(&self) -> AnalysisArtifactKeyVersion {
        Self::VERSION
    }

    /// Returns the exact source-content digest.
    #[must_use]
    pub const fn source_digest(&self) -> &D {
        &self.source_digest
    }

    /// Returns the complete adapter, grammar, and producer identity.
    #[must_use]
    pub const fn analyzer_identity(&self) -> &A {
        &self.analyzer_identity
    }

    /// Returns the resolved semantics-affecting configuration identity.
    #[must_use]
    pub const fn configuration_identity(&self) -> &C {
        &self.configuration_identity
    }

    /// Returns the extraction schema identity.
    #[must_use]
    pub const fn schema_identity(&self) -> &S {
        &self.schema_identity
    }

    /// Returns the canonicalization version.
    #[must_use]
    pub const fn canonicalization_version(&self) -> &V {
        &self.canonicalization_version
    }
}

#[cfg(test)]
mod tests {
    use super::{AnalysisArtifactKey, AnalysisArtifactKeyVersion};

    fn key() -> AnalysisArtifactKey<&'static str, &'static str, &'static str, &'static str, u16> {
        AnalysisArtifactKey::new(
            "source:digest",
            "rust-adapter:grammar:producer",
            "configuration:digest",
            "schema:1",
            1,
        )
    }

    #[test]
    fn artifact_key_preserves_every_semantic_input() {
        let key = key();

        assert_eq!(
            AnalysisArtifactKey::<&str, &str, &str, &str, u16>::VERSION,
            AnalysisArtifactKeyVersion::V1
        );
        assert_eq!(key.version(), AnalysisArtifactKeyVersion::V1);
        assert_eq!(AnalysisArtifactKeyVersion::V1.get(), 1);
        assert_eq!(*key.source_digest(), "source:digest");
        assert_eq!(*key.analyzer_identity(), "rust-adapter:grammar:producer");
        assert_eq!(*key.configuration_identity(), "configuration:digest");
        assert_eq!(*key.schema_identity(), "schema:1");
        assert_eq!(*key.canonicalization_version(), 1);
    }

    #[test]
    fn every_semantic_input_participates_in_key_equality() {
        let baseline = key();

        assert_ne!(
            baseline,
            AnalysisArtifactKey::new(
                "source:changed",
                "rust-adapter:grammar:producer",
                "configuration:digest",
                "schema:1",
                1,
            )
        );
        assert_ne!(
            baseline,
            AnalysisArtifactKey::new(
                "source:digest",
                "rust-adapter:changed",
                "configuration:digest",
                "schema:1",
                1,
            )
        );
        assert_ne!(
            baseline,
            AnalysisArtifactKey::new(
                "source:digest",
                "rust-adapter:grammar:producer",
                "configuration:changed",
                "schema:1",
                1,
            )
        );
        assert_ne!(
            baseline,
            AnalysisArtifactKey::new(
                "source:digest",
                "rust-adapter:grammar:producer",
                "configuration:digest",
                "schema:changed",
                1,
            )
        );
        assert_ne!(
            baseline,
            AnalysisArtifactKey::new(
                "source:digest",
                "rust-adapter:grammar:producer",
                "configuration:digest",
                "schema:1",
                2,
            )
        );
    }
}
