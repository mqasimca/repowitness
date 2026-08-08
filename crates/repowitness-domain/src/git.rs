//! Canonical Git object identifiers used as externally supplied revision pins.

use std::fmt;

/// The Git object format encoded by a full object identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GitObjectIdFormat {
    /// The SHA-1 Git object format.
    Sha1,
    /// The SHA-256 Git object format.
    Sha256,
}

impl GitObjectIdFormat {
    /// Returns the exact number of bytes in an identifier of this format.
    #[must_use]
    pub const fn byte_len(self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
        }
    }

    const fn hex_len(self) -> usize {
        self.byte_len() * 2
    }
}

/// A complete, canonical lower-hex Git object identifier.
///
/// Revision expressions, abbreviated identifiers, refs, and upper-case hex
/// are intentionally not accepted. Adapters must resolve any user-facing
/// expression before constructing this domain identity.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct GitObjectId {
    format: GitObjectIdFormat,
    bytes: [u8; 32],
}

impl GitObjectId {
    /// Parses a full, canonical lower-hex SHA-1 or SHA-256 object identifier.
    ///
    /// # Errors
    ///
    /// Returns [`GitObjectIdError`] when the text is not exactly one supported
    /// full lower-hex object identifier.
    pub fn try_from_hex(value: &str) -> Result<Self, GitObjectIdError> {
        let format = match value.len() {
            40 => GitObjectIdFormat::Sha1,
            64 => GitObjectIdFormat::Sha256,
            actual => return Err(GitObjectIdError::InvalidLength { actual }),
        };
        let mut bytes = [0_u8; 32];
        for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
            bytes[index] = decode_lower_hex_pair(chunk).ok_or(GitObjectIdError::InvalidHex)?;
        }
        Ok(Self { format, bytes })
    }

    /// Returns the identifier's object format.
    #[must_use]
    pub const fn format(&self) -> GitObjectIdFormat {
        self.format
    }

    /// Returns the exact binary identifier bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.format.byte_len()]
    }

    /// Returns the canonical lower-hex representation.
    #[must_use]
    pub fn to_hex(&self) -> String {
        let mut value = String::with_capacity(self.format.hex_len());
        for byte in self.as_bytes() {
            use fmt::Write as _;
            let _ = write!(value, "{byte:02x}");
        }
        value
    }
}

impl fmt::Debug for GitObjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GitObjectId")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for GitObjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.as_bytes() {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// A Git object identifier was not in its canonical accepted representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitObjectIdError {
    /// The text was not the full length of a supported Git object format.
    InvalidLength {
        /// The supplied UTF-8 byte length.
        actual: usize,
    },
    /// The text contained a non-lower-hex byte.
    InvalidHex,
}

impl fmt::Display for GitObjectIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength { actual } => write!(
                formatter,
                "Git object identifier length {actual} is not a full SHA-1 or SHA-256 identifier"
            ),
            Self::InvalidHex => {
                formatter.write_str("Git object identifier is not lower hexadecimal")
            }
        }
    }
}

impl std::error::Error for GitObjectIdError {}

fn decode_lower_hex_pair(pair: &[u8]) -> Option<u8> {
    let high = decode_lower_hex_digit(pair.first().copied()?)?;
    let low = decode_lower_hex_digit(pair.get(1).copied()?)?;
    Some((high << 4) | low)
}

const fn decode_lower_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{GitObjectId, GitObjectIdError, GitObjectIdFormat};

    #[test]
    fn accepts_only_full_canonical_object_ids() {
        let sha1 = GitObjectId::try_from_hex("0123456789abcdef0123456789abcdef01234567")
            .expect("full SHA-1 id should parse");
        assert_eq!(sha1.format(), GitObjectIdFormat::Sha1);
        assert_eq!(sha1.as_bytes().len(), 20);
        assert_eq!(sha1.to_hex(), "0123456789abcdef0123456789abcdef01234567");

        let sha256 =
            GitObjectId::try_from_hex(&"a1".repeat(32)).expect("full SHA-256 id should parse");
        assert_eq!(sha256.format(), GitObjectIdFormat::Sha256);
        assert_eq!(sha256.as_bytes().len(), 32);

        assert!(matches!(
            GitObjectId::try_from_hex("0123"),
            Err(GitObjectIdError::InvalidLength { actual: 4 })
        ));
        assert!(matches!(
            GitObjectId::try_from_hex("0123456789abcdef0123456789abcdef0123456G"),
            Err(GitObjectIdError::InvalidHex)
        ));
        assert!(matches!(
            GitObjectId::try_from_hex("0123456789ABCDEF0123456789abcdef01234567"),
            Err(GitObjectIdError::InvalidHex)
        ));
    }
}
