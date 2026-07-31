use std::{error::Error, fmt};

/// Number of bytes in every SHA-256 digest.
pub const SHA256_DIGEST_BYTES: usize = 32;

/// Error returned when a boundary supplies a non-SHA-256-sized digest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sha256DigestLengthError {
    actual_bytes: u64,
}

impl Sha256DigestLengthError {
    /// Returns the observed byte count without exposing digest contents.
    #[must_use]
    pub const fn actual_bytes(self) -> u64 {
        self.actual_bytes
    }
}

impl fmt::Display for Sha256DigestLengthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SHA-256 digest must contain exactly 32 bytes")
    }
}

impl Error for Sha256DigestLengthError {}

macro_rules! define_sha256_digest {
    ($name:ident, $documentation:literal) => {
        #[doc = $documentation]
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; SHA256_DIGEST_BYTES]);

        impl $name {
            /// Creates the typed digest from exactly 32 bytes.
            #[must_use]
            pub const fn new(bytes: [u8; SHA256_DIGEST_BYTES]) -> Self {
                Self(bytes)
            }

            /// Copies an exactly sized boundary representation.
            pub fn try_from_slice(bytes: &[u8]) -> Result<Self, Sha256DigestLengthError> {
                let digest = <[u8; SHA256_DIGEST_BYTES]>::try_from(bytes).map_err(|_| {
                    Sha256DigestLengthError {
                        actual_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                    }
                })?;
                Ok(Self(digest))
            }

            /// Returns the exact digest bytes.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; SHA256_DIGEST_BYTES] {
                &self.0
            }

            /// Consumes the value and returns the exact digest bytes.
            #[must_use]
            pub const fn into_bytes(self) -> [u8; SHA256_DIGEST_BYTES] {
                self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($name))
                    .field("algorithm", &"SHA-256")
                    .finish_non_exhaustive()
            }
        }
    };
}

define_sha256_digest!(
    SourceContentDigest,
    "A SHA-256 identity for exact immutable source bytes."
);
define_sha256_digest!(
    SourceManifestDigest,
    "A SHA-256 identity for a canonical complete source manifest."
);
define_sha256_digest!(
    SourceSnapshotDigest,
    "A SHA-256 identity for a canonical complete source snapshot."
);
define_sha256_digest!(
    RepositoryIdentityDigest,
    "A SHA-256 identity for one repository within a workspace."
);
define_sha256_digest!(
    GitStateDigest,
    "A SHA-256 identity for the complete relevant Git state."
);
define_sha256_digest!(
    WorktreeStateDigest,
    "A SHA-256 identity for relevant worktree and submodule state."
);
define_sha256_digest!(
    AnalysisArtifactDigest,
    "A SHA-256 identity for a canonical analysis-artifact key."
);
define_sha256_digest!(
    AnalysisArtifactPayloadDigest,
    "A SHA-256 integrity identity for canonical analysis-artifact output."
);
define_sha256_digest!(
    ConfigurationDigest,
    "A SHA-256 identity for resolved semantics-affecting configuration."
);
define_sha256_digest!(
    ProducerManifestDigest,
    "A SHA-256 identity for the complete producer and grammar manifest."
);
define_sha256_digest!(
    AnalysisSchemaDigest,
    "A SHA-256 identity for the versioned analysis fact schema."
);
define_sha256_digest!(
    MigrationDigest,
    "A SHA-256 identity for an exact database migration."
);
define_sha256_digest!(
    CanonicalMemoryDigest,
    "A SHA-256 identity for validated canonical memory semantics."
);
define_sha256_digest!(
    MemoryPresentationDigest,
    "A SHA-256 receipt for the exact admitted memory-file presentation bytes."
);
define_sha256_digest!(
    DeclarationDigest,
    "A SHA-256 identity for exact source declaration bytes."
);
define_sha256_digest!(
    CorrespondenceFingerprintDigest,
    "A SHA-256 identity for one versioned occurrence-correspondence fingerprint."
);
define_sha256_digest!(
    CorrespondenceProfileDigest,
    "A SHA-256 identity for one complete versioned correspondence profile."
);
define_sha256_digest!(
    ScipOverlayDigest,
    "A SHA-256 identity for one immutable validated SCIP precision overlay."
);
define_sha256_digest!(
    ScipInputDigest,
    "A SHA-256 identity for one exact hostile SCIP input artifact."
);
define_sha256_digest!(
    ScipSchemaDigest,
    "A SHA-256 identity for one reviewed exact SCIP schema."
);
define_sha256_digest!(
    ScipImporterDigest,
    "A SHA-256 identity for one bounded SCIP importer implementation."
);

#[cfg(test)]
mod tests {
    use super::{
        AnalysisArtifactDigest, SHA256_DIGEST_BYTES, Sha256DigestLengthError, SourceContentDigest,
    };

    #[test]
    fn typed_digests_preserve_exact_bytes_and_kind() {
        let bytes = [0xA5; SHA256_DIGEST_BYTES];
        let source = SourceContentDigest::new(bytes);
        let artifact = AnalysisArtifactDigest::new(bytes);

        assert_eq!(source.as_bytes(), &bytes);
        assert_eq!(source.into_bytes(), bytes);
        assert_eq!(artifact.as_bytes(), &bytes);
    }

    #[test]
    fn boundary_length_errors_are_stable_and_redacted() {
        let error = SourceContentDigest::try_from_slice(&[0xA5; 31]).unwrap_err();

        assert_eq!(error, Sha256DigestLengthError { actual_bytes: 31 });
        assert_eq!(error.actual_bytes(), 31);
        assert_eq!(
            error.to_string(),
            "SHA-256 digest must contain exactly 32 bytes"
        );
        assert!(!format!("{error:?}").contains("A5"));
    }

    #[test]
    fn debug_output_does_not_expose_digest_bytes() {
        let digest = SourceContentDigest::new([0xA5; SHA256_DIGEST_BYTES]);
        let debug = format!("{digest:?}");

        assert!(debug.contains("SHA-256"));
        assert!(!debug.contains("A5"));
        assert!(!debug.contains("165"));
    }
}
