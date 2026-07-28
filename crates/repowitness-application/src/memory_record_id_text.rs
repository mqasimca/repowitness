use std::{error::Error, fmt};

use repowitness_domain::MemoryRecordId;

const PREFIX: &str = "mem_";
const PAYLOAD_BYTES: usize = 26;
const CROCKFORD_BASE32: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Exact byte length of a canonical Phase 0 memory-record identity.
pub const MEMORY_RECORD_ID_TEXT_BYTES: usize = PREFIX.len() + PAYLOAD_BYTES;

/// Canonical Crockford Base32 text for one exact 128-bit memory-record identity.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MemoryRecordIdTextV1(String);

impl MemoryRecordIdTextV1 {
    /// Encodes one record identity without aliases, padding, or case variation.
    #[must_use]
    pub fn encode(record_id: MemoryRecordId) -> Self {
        let mut value = u128::from_be_bytes(record_id.into_bytes());
        let mut payload = [b'0'; PAYLOAD_BYTES];
        for output in payload.iter_mut().rev() {
            *output = CROCKFORD_BASE32[(value & 31) as usize];
            value >>= 5;
        }

        let mut encoded = String::with_capacity(MEMORY_RECORD_ID_TEXT_BYTES);
        encoded.push_str(PREFIX);
        for byte in payload {
            encoded.push(char::from(byte));
        }
        Self(encoded)
    }

    /// Decodes only the canonical version-1 representation.
    pub fn decode(text: &str) -> Result<MemoryRecordId, MemoryRecordIdTextError> {
        if text.len() != MEMORY_RECORD_ID_TEXT_BYTES {
            return Err(MemoryRecordIdTextError::InvalidLength {
                actual_bytes: u64::try_from(text.len()).unwrap_or(u64::MAX),
            });
        }
        let payload = text
            .strip_prefix(PREFIX)
            .ok_or(MemoryRecordIdTextError::InvalidPrefix)?
            .as_bytes();

        let mut decoded = 0_u128;
        for (index, byte) in payload.iter().copied().enumerate() {
            let digit = decode_digit(byte)?;
            if index == 0 && digit > 7 {
                return Err(MemoryRecordIdTextError::OutOfRange);
            }
            decoded = decoded
                .checked_mul(32)
                .and_then(|value| value.checked_add(u128::from(digit)))
                .ok_or(MemoryRecordIdTextError::OutOfRange)?;
        }
        Ok(MemoryRecordId::new(decoded.to_be_bytes()))
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

impl AsRef<str> for MemoryRecordIdTextV1 {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for MemoryRecordIdTextV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryRecordIdTextV1")
            .field("version", &1_u32)
            .field("encoded_bytes", &MEMORY_RECORD_ID_TEXT_BYTES)
            .finish_non_exhaustive()
    }
}

/// Stable, value-redacted failure while decoding a memory-record identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRecordIdTextError {
    /// The complete encoded scalar did not have the exact version-1 width.
    InvalidLength {
        /// Observed encoded byte count without exposing the value.
        actual_bytes: u64,
    },
    /// The version or identity prefix was not canonical.
    InvalidPrefix,
    /// The payload contained lowercase, aliases, or non-Base32 bytes.
    InvalidBase32,
    /// The 26-character payload did not fit in 128 bits.
    OutOfRange,
}

impl MemoryRecordIdTextError {
    /// Returns the observed byte count when length validation failed.
    #[must_use]
    pub const fn actual_bytes(self) -> Option<u64> {
        match self {
            Self::InvalidLength { actual_bytes } => Some(actual_bytes),
            Self::InvalidPrefix | Self::InvalidBase32 | Self::OutOfRange => None,
        }
    }
}

impl fmt::Display for MemoryRecordIdTextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLength { .. } => "memory record identity has an invalid byte count",
            Self::InvalidPrefix => "memory record identity has an invalid format tag",
            Self::InvalidBase32 => {
                "memory record identity has a non-canonical Crockford Base32 payload"
            }
            Self::OutOfRange => "memory record identity exceeds 128 bits",
        })
    }
}

impl Error for MemoryRecordIdTextError {}

fn decode_digit(byte: u8) -> Result<u8, MemoryRecordIdTextError> {
    CROCKFORD_BASE32
        .iter()
        .position(|candidate| *candidate == byte)
        .and_then(|position| u8::try_from(position).ok())
        .ok_or(MemoryRecordIdTextError::InvalidBase32)
}

#[cfg(test)]
mod tests {
    use repowitness_domain::MemoryRecordId;

    use super::{MEMORY_RECORD_ID_TEXT_BYTES, MemoryRecordIdTextError, MemoryRecordIdTextV1};

    #[test]
    fn exact_golden_vectors_round_trip() {
        let vectors = [
            ([0_u8; 16], "mem_00000000000000000000000000"),
            ([0xff_u8; 16], "mem_7ZZZZZZZZZZZZZZZZZZZZZZZZZ"),
            (
                [
                    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
                    0x0d, 0x0e, 0x0f,
                ],
                "mem_00041061050R3GG28A1C60T3GF",
            ),
        ];

        for (bytes, text) in vectors {
            let id = MemoryRecordId::new(bytes);
            assert_eq!(MemoryRecordIdTextV1::encode(id).as_str(), text);
            assert_eq!(MemoryRecordIdTextV1::decode(text), Ok(id));
            assert_eq!(text.len(), MEMORY_RECORD_ID_TEXT_BYTES);
        }
    }

    #[test]
    fn repeated_byte_patterns_round_trip() {
        for byte in u8::MIN..=u8::MAX {
            let id = MemoryRecordId::new([byte; 16]);
            let encoded = MemoryRecordIdTextV1::encode(id);
            assert_eq!(MemoryRecordIdTextV1::decode(encoded.as_str()), Ok(id));
        }
    }

    #[test]
    fn alternate_and_malformed_forms_are_rejected() {
        assert_eq!(
            MemoryRecordIdTextV1::decode("mem_0000000000000000000000000"),
            Err(MemoryRecordIdTextError::InvalidLength { actual_bytes: 29 })
        );
        assert_eq!(
            MemoryRecordIdTextV1::decode("MEM_00000000000000000000000000"),
            Err(MemoryRecordIdTextError::InvalidPrefix)
        );
        assert_eq!(
            MemoryRecordIdTextV1::decode("mem_0000000000000000000000000O"),
            Err(MemoryRecordIdTextError::InvalidBase32)
        );
        assert_eq!(
            MemoryRecordIdTextV1::decode("mem_80000000000000000000000000"),
            Err(MemoryRecordIdTextError::OutOfRange)
        );
    }

    #[test]
    fn errors_and_debug_output_are_redacted() {
        let encoded = MemoryRecordIdTextV1::encode(MemoryRecordId::new([0xA5; 16]));
        let error = MemoryRecordIdTextV1::decode("private").expect_err("input should fail");
        let debug = format!("{encoded:?}");

        assert_eq!(error.actual_bytes(), Some(7));
        assert!(!debug.contains("A5"));
        assert!(!debug.contains(encoded.as_str()));
    }
}
