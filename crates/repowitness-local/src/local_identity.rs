//! OS-random canonical identity generation for explicit local boundaries.

use core::fmt;

use repowitness_application::{
    ConnectedWorkspaceIdTextV1, RepositoryIdentityTextV1, SourceSlotIdTextV1,
};
use repowitness_domain::{ConnectedWorkspaceId, RepositoryIdentityDigest, SourceSlotId};

const GENERATED_IDENTITY_BYTES: usize = 32;

/// Allow-listed canonical identity kinds that may be generated locally.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LocalIdentityKind {
    /// A logical repository identity encoded as `rwi1:h:`.
    Repository,
    /// A connected-workspace identity encoded as `cwi1:h:`.
    ConnectedWorkspace,
    /// A source-slot identity encoded as `ssi1:h:`.
    SourceSlot,
}

/// One generated canonical identity.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct GeneratedLocalIdentity {
    kind: LocalIdentityKind,
    text: String,
}

impl GeneratedLocalIdentity {
    /// Returns the allow-listed identity kind.
    #[must_use]
    pub const fn kind(&self) -> LocalIdentityKind {
        self.kind
    }

    /// Returns the canonical versioned identity text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Consumes the generated identity and returns its canonical text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.text
    }
}

impl AsRef<str> for GeneratedLocalIdentity {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for GeneratedLocalIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GeneratedLocalIdentity")
            .field("kind", &self.kind)
            .field("encoded_bytes", &self.text.len())
            .finish_non_exhaustive()
    }
}

/// Stable path- and entropy-detail-free identity generation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalIdentityGenerationError {
    /// The operating system could not provide cryptographically secure bytes.
    EntropyUnavailable,
}

impl fmt::Display for LocalIdentityGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("operating-system identity entropy is unavailable")
    }
}

impl std::error::Error for LocalIdentityGenerationError {}

/// Generates one canonical identity from operating-system secure randomness.
///
/// No fallback PRNG is used. Failure to obtain all identity bytes fails closed.
pub fn generate_local_identity(
    kind: LocalIdentityKind,
) -> Result<GeneratedLocalIdentity, LocalIdentityGenerationError> {
    generate_local_identity_with(kind, |bytes| {
        getrandom::fill(bytes).map_err(|_| LocalIdentityGenerationError::EntropyUnavailable)
    })
}

fn generate_local_identity_with(
    kind: LocalIdentityKind,
    fill: impl FnOnce(&mut [u8]) -> Result<(), LocalIdentityGenerationError>,
) -> Result<GeneratedLocalIdentity, LocalIdentityGenerationError> {
    let mut bytes = [0_u8; GENERATED_IDENTITY_BYTES];
    fill(&mut bytes)?;
    let text = match kind {
        LocalIdentityKind::Repository => {
            RepositoryIdentityTextV1::encode(RepositoryIdentityDigest::new(bytes)).into_string()
        }
        LocalIdentityKind::ConnectedWorkspace => {
            ConnectedWorkspaceIdTextV1::encode(ConnectedWorkspaceId::new(bytes)).into_string()
        }
        LocalIdentityKind::SourceSlot => {
            SourceSlotIdTextV1::encode(SourceSlotId::new(bytes)).into_string()
        }
    };
    Ok(GeneratedLocalIdentity { kind, text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_entropy_is_encoded_for_every_allowlisted_kind() {
        let cases = [
            (LocalIdentityKind::Repository, "rwi1:h:"),
            (LocalIdentityKind::ConnectedWorkspace, "cwi1:h:"),
            (LocalIdentityKind::SourceSlot, "ssi1:h:"),
        ];
        for (kind, prefix) in cases {
            let generated = generate_local_identity_with(kind, |bytes| {
                bytes.fill(0xAB);
                Ok(())
            })
            .expect("deterministic entropy should generate an identity");
            assert_eq!(generated.kind(), kind);
            assert_eq!(
                generated.as_str(),
                format!("{prefix}{}", "AB".repeat(GENERATED_IDENTITY_BYTES))
            );
        }
    }

    #[test]
    fn entropy_failure_fails_closed_without_exposing_partial_bytes() {
        let error = generate_local_identity_with(LocalIdentityKind::Repository, |bytes| {
            bytes.fill(0xA5);
            Err(LocalIdentityGenerationError::EntropyUnavailable)
        })
        .expect_err("entropy failure must not return a partial identity");

        assert_eq!(error, LocalIdentityGenerationError::EntropyUnavailable);
        let rendered = format!("{error} {error:?}");
        assert!(!rendered.contains("A5"));
        assert!(!rendered.contains("165"));
    }

    #[test]
    fn generated_identity_debug_is_redacted() {
        let generated = generate_local_identity_with(LocalIdentityKind::SourceSlot, |bytes| {
            bytes.fill(0xB6);
            Ok(())
        })
        .expect("deterministic entropy should generate an identity");
        let debug = format!("{generated:?}");

        assert!(debug.contains("SourceSlot"));
        assert!(!debug.contains("B6"));
        assert!(!debug.contains(generated.as_str()));
    }
}
