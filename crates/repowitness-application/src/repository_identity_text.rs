use std::{error::Error, fmt};

use repowitness_domain::{RepositoryIdentityDigest, SHA256_DIGEST_BYTES};

const PREFIX: &str = "rwi1:h:";
const PREFIX_BYTES: usize = PREFIX.len();
const PAYLOAD_BYTES: usize = SHA256_DIGEST_BYTES * 2;

/// Exact byte length of a canonical Phase 0 repository identity.
pub const REPOSITORY_IDENTITY_TEXT_BYTES: usize = PREFIX_BYTES + PAYLOAD_BYTES;

/// Canonical tagged text for one explicit 32-byte repository identity.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepositoryIdentityTextV1(String);

impl RepositoryIdentityTextV1 {
    /// Encodes one repository identity as strict uppercase Base16.
    #[must_use]
    pub fn encode(identity: RepositoryIdentityDigest) -> Self {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";

        let mut encoded = String::with_capacity(REPOSITORY_IDENTITY_TEXT_BYTES);
        encoded.push_str(PREFIX);
        for byte in identity.as_bytes() {
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0F)]));
        }
        Self(encoded)
    }

    /// Decodes one canonical repository identity, rejecting every alternate form.
    pub fn decode(text: &str) -> Result<RepositoryIdentityDigest, RepositoryIdentityTextError> {
        if text.len() != REPOSITORY_IDENTITY_TEXT_BYTES {
            return Err(RepositoryIdentityTextError::InvalidLength {
                actual_bytes: u64::try_from(text.len()).unwrap_or(u64::MAX),
            });
        }
        let payload = text
            .strip_prefix(PREFIX)
            .ok_or(RepositoryIdentityTextError::InvalidPrefix)?
            .as_bytes();
        let mut decoded = [0_u8; SHA256_DIGEST_BYTES];
        for (output, pair) in decoded.iter_mut().zip(payload.chunks_exact(2)) {
            let high = decode_nibble(pair[0])?;
            let low = decode_nibble(pair[1])?;
            *output = (high << 4) | low;
        }
        Ok(RepositoryIdentityDigest::new(decoded))
    }

    /// Returns the canonical encoded text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the value and returns its owned canonical text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<str> for RepositoryIdentityTextV1 {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for RepositoryIdentityTextV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryIdentityTextV1")
            .field("version", &1_u32)
            .field("encoded_bytes", &REPOSITORY_IDENTITY_TEXT_BYTES)
            .finish_non_exhaustive()
    }
}

/// Stable failure while decoding a repository identity boundary scalar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryIdentityTextError {
    /// The complete encoded scalar did not have the exact version-1 width.
    InvalidLength {
        /// Observed encoded byte count without exposing the value.
        actual_bytes: u64,
    },
    /// The version, identity kind, or encoding tag was not canonical.
    InvalidPrefix,
    /// The payload contained lowercase or non-Base16 bytes.
    InvalidBase16,
}

impl RepositoryIdentityTextError {
    /// Returns the observed encoded byte count when length validation failed.
    #[must_use]
    pub const fn actual_bytes(self) -> Option<u64> {
        match self {
            Self::InvalidLength { actual_bytes } => Some(actual_bytes),
            Self::InvalidPrefix | Self::InvalidBase16 => None,
        }
    }
}

impl fmt::Display for RepositoryIdentityTextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLength { .. } => "repository identity text has an invalid byte count",
            Self::InvalidPrefix => "repository identity text has an invalid format tag",
            Self::InvalidBase16 => "repository identity text has a non-canonical Base16 payload",
        })
    }
}

impl Error for RepositoryIdentityTextError {}

fn decode_nibble(byte: u8) -> Result<u8, RepositoryIdentityTextError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(RepositoryIdentityTextError::InvalidBase16),
    }
}

#[cfg(test)]
mod tests {
    use repowitness_domain::RepositoryIdentityDigest;

    use super::{
        REPOSITORY_IDENTITY_TEXT_BYTES, RepositoryIdentityTextError, RepositoryIdentityTextV1,
    };

    #[test]
    fn golden_vector_round_trips_exact_bytes() {
        let identity = RepositoryIdentityDigest::new([0xAB; 32]);
        let encoded = RepositoryIdentityTextV1::encode(identity);

        assert_eq!(
            encoded.as_str(),
            "rwi1:h:ABABABABABABABABABABABABABABABABABABABABABABABABABABABABABABABAB"
        );
        assert_eq!(
            RepositoryIdentityTextV1::decode(encoded.as_str()),
            Ok(identity)
        );
        assert_eq!(encoded.as_str().len(), REPOSITORY_IDENTITY_TEXT_BYTES);
    }

    #[test]
    fn every_byte_value_round_trips() {
        for byte in u8::MIN..=u8::MAX {
            let identity = RepositoryIdentityDigest::new([byte; 32]);
            let encoded = RepositoryIdentityTextV1::encode(identity);
            assert_eq!(
                RepositoryIdentityTextV1::decode(encoded.as_str()),
                Ok(identity)
            );
        }
    }

    #[test]
    fn alternate_and_malformed_forms_are_rejected() {
        let canonical = RepositoryIdentityTextV1::encode(RepositoryIdentityDigest::new([0xAB; 32]))
            .into_string();
        let mut lowercase = canonical.clone();
        lowercase.replace_range(7..8, "a");
        let mut wrong_prefix = canonical.clone();
        wrong_prefix.replace_range(0..1, "x");

        assert_eq!(
            RepositoryIdentityTextV1::decode(&canonical[..canonical.len() - 1]),
            Err(RepositoryIdentityTextError::InvalidLength { actual_bytes: 70 })
        );
        assert_eq!(
            RepositoryIdentityTextV1::decode(&(canonical.clone() + "0")),
            Err(RepositoryIdentityTextError::InvalidLength { actual_bytes: 72 })
        );
        assert_eq!(
            RepositoryIdentityTextV1::decode(&wrong_prefix),
            Err(RepositoryIdentityTextError::InvalidPrefix)
        );
        assert_eq!(
            RepositoryIdentityTextV1::decode(&lowercase),
            Err(RepositoryIdentityTextError::InvalidBase16)
        );
    }

    #[test]
    fn errors_and_debug_output_are_redacted() {
        let encoded = RepositoryIdentityTextV1::encode(RepositoryIdentityDigest::new([0xA5; 32]));
        let error = RepositoryIdentityTextV1::decode("private").expect_err("input should fail");
        let debug = format!("{encoded:?}");

        assert_eq!(error.actual_bytes(), Some(7));
        assert_eq!(
            error.to_string(),
            "repository identity text has an invalid byte count"
        );
        assert!(!debug.contains("A5"));
        assert!(!debug.contains(encoded.as_str()));
    }
}
