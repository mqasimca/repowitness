//! Canonical, adapter-neutral text encoding for repository-path identities.

use core::fmt;

pub use repowitness_domain::RepositoryPathLimits;
use repowitness_domain::{RepositoryPath, RepositoryPathByteCount, RepositoryPathError};

const PREFIX: &str = "rwp1:h:";
const BASE16_UPPER: &[u8; 16] = b"0123456789ABCDEF";

/// The semantic version of the canonical repository-path text encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryPathTextVersion(u16);

impl RepositoryPathTextVersion {
    /// The `rwp1:h:` uppercase-Base16 profile.
    pub const V1: Self = Self(1);

    /// Returns the fixed-width version number.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A fixed-width number of bytes in an encoded repository-path scalar.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepositoryPathTextByteCount(u64);

impl RepositoryPathTextByteCount {
    /// Returns the fixed-width byte count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// An inclusive bound on bytes in an encoded repository-path scalar.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RepositoryPathTextByteLimit(u64);

impl RepositoryPathTextByteLimit {
    /// Creates an inclusive encoded-scalar byte limit.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the fixed-width byte limit.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A canonical `rwp1:h:` repository-path text scalar.
///
/// This is an adapter-neutral scalar, not an MCP, persistence, configuration,
/// or Git-memory DTO. Adapters wrap it in their own versioned DTOs.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepositoryPathTextV1 {
    encoded: Box<str>,
    encoded_byte_count: RepositoryPathTextByteCount,
}

impl RepositoryPathTextV1 {
    /// The semantic version implemented by this scalar.
    pub const VERSION: RepositoryPathTextVersion = RepositoryPathTextVersion::V1;

    /// Encodes one validated repository path under an explicit output bound.
    ///
    /// # Errors
    ///
    /// Returns [`RepositoryPathTextError::EncodedLengthNotRepresentable`] if
    /// the encoded length cannot be represented on this platform, or
    /// [`RepositoryPathTextError::EncodedLimitExceeded`] if the canonical
    /// scalar would exceed `limit`.
    pub fn encode(
        path: &RepositoryPath,
        limit: RepositoryPathTextByteLimit,
    ) -> Result<Self, RepositoryPathTextError> {
        let (encoded_length, encoded_byte_count) = encoded_length(path)?;
        enforce_encoded_limit(encoded_byte_count, limit)?;

        let mut encoded = String::with_capacity(encoded_length);
        encoded.push_str(PREFIX);
        for byte in path.as_bytes() {
            encoded.push(char::from(BASE16_UPPER[usize::from(byte >> 4)]));
            encoded.push(char::from(BASE16_UPPER[usize::from(byte & 0x0f)]));
        }

        Ok(Self {
            encoded: encoded.into_boxed_str(),
            encoded_byte_count,
        })
    }

    /// Decodes and validates one canonical repository-path text scalar.
    ///
    /// The encoded limit is checked before parsing. The decoded path-byte
    /// limit is checked before allocating its byte buffer.
    ///
    /// # Errors
    ///
    /// Returns a [`RepositoryPathTextError`] for an over-limit input, unknown
    /// tag, empty or odd-length payload, non-canonical Base16, or invalid
    /// decoded repository path.
    pub fn decode(
        encoded: &str,
        encoded_limit: RepositoryPathTextByteLimit,
        path_limits: RepositoryPathLimits,
    ) -> Result<RepositoryPath, RepositoryPathTextError> {
        let encoded_byte_count = text_byte_count(encoded)?;
        enforce_encoded_limit(encoded_byte_count, encoded_limit)?;

        let payload = encoded
            .strip_prefix(PREFIX)
            .ok_or(RepositoryPathTextError::InvalidTag)?;
        if payload.is_empty() {
            return Err(RepositoryPathTextError::EmptyPayload);
        }
        if !payload.len().is_multiple_of(2) {
            return Err(RepositoryPathTextError::OddPayloadLength);
        }

        let decoded_length = payload.len() / 2;
        let decoded_byte_count = u64::try_from(decoded_length)
            .map(RepositoryPathByteCount::new)
            .map_err(|_| RepositoryPathTextError::DecodedLengthNotRepresentable)?;
        if decoded_byte_count.get() > path_limits.max_bytes().get() {
            return Err(RepositoryPathError::ByteLimitExceeded {
                actual: decoded_byte_count,
                limit: path_limits.max_bytes(),
            }
            .into());
        }

        let mut decoded = Vec::with_capacity(decoded_length);
        for pair in payload.as_bytes().chunks_exact(2) {
            let high = decode_nibble(pair[0]).ok_or(RepositoryPathTextError::NonCanonicalBase16)?;
            let low = decode_nibble(pair[1]).ok_or(RepositoryPathTextError::NonCanonicalBase16)?;
            decoded.push((high << 4) | low);
        }

        RepositoryPath::try_from_vec(decoded, path_limits).map_err(Into::into)
    }

    /// Returns the semantic version implemented by this scalar.
    #[must_use]
    pub const fn version(&self) -> RepositoryPathTextVersion {
        Self::VERSION
    }

    /// Returns the canonical ASCII scalar.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.encoded
    }

    /// Returns the fixed-width encoded byte count.
    #[must_use]
    pub const fn encoded_byte_count(&self) -> RepositoryPathTextByteCount {
        self.encoded_byte_count
    }

    /// Consumes the scalar and returns its canonical text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.encoded.into()
    }
}

impl AsRef<str> for RepositoryPathTextV1 {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for RepositoryPathTextV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryPathTextV1")
            .field("version", &Self::VERSION)
            .field("encoded_byte_count", &self.encoded_byte_count)
            .finish_non_exhaustive()
    }
}

/// Failure to encode or decode a repository-path text scalar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryPathTextError {
    /// The encoded input length cannot be represented as a `u64`.
    EncodedByteCountNotRepresentable,
    /// The canonical output length cannot be represented on this platform.
    EncodedLengthNotRepresentable,
    /// The encoded scalar exceeds its declared byte bound.
    EncodedLimitExceeded {
        /// The scalar's actual encoded byte count.
        actual: RepositoryPathTextByteCount,
        /// The inclusive encoded byte bound.
        limit: RepositoryPathTextByteLimit,
    },
    /// The scalar does not begin with the exact version and encoding tag.
    InvalidTag,
    /// The Base16 payload is empty.
    EmptyPayload,
    /// The Base16 payload does not contain complete byte pairs.
    OddPayloadLength,
    /// The payload contains lowercase or a non-Base16 character.
    NonCanonicalBase16,
    /// The decoded payload length cannot be represented as a `u64`.
    DecodedLengthNotRepresentable,
    /// The decoded bytes do not form a valid bounded repository path.
    InvalidRepositoryPath(RepositoryPathError),
}

impl fmt::Display for RepositoryPathTextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EncodedByteCountNotRepresentable => formatter
                .write_str("repository path text byte count cannot be represented as a u64"),
            Self::EncodedLengthNotRepresentable => {
                formatter.write_str("repository path encoded length cannot be represented")
            }
            Self::EncodedLimitExceeded { actual, limit } => write!(
                formatter,
                "repository path text byte count {} exceeds limit {}",
                actual.get(),
                limit.get()
            ),
            Self::InvalidTag => {
                formatter.write_str("repository path text has an unknown version or encoding tag")
            }
            Self::EmptyPayload => {
                formatter.write_str("repository path text has an empty Base16 payload")
            }
            Self::OddPayloadLength => {
                formatter.write_str("repository path text Base16 payload has an odd length")
            }
            Self::NonCanonicalBase16 => formatter
                .write_str("repository path text payload is not canonical uppercase Base16"),
            Self::DecodedLengthNotRepresentable => {
                formatter.write_str("decoded repository path length cannot be represented as a u64")
            }
            Self::InvalidRepositoryPath(error) => {
                write!(formatter, "decoded repository path is invalid: {error}")
            }
        }
    }
}

impl std::error::Error for RepositoryPathTextError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidRepositoryPath(error) => Some(error),
            _ => None,
        }
    }
}

impl From<RepositoryPathError> for RepositoryPathTextError {
    fn from(error: RepositoryPathError) -> Self {
        Self::InvalidRepositoryPath(error)
    }
}

fn text_byte_count(encoded: &str) -> Result<RepositoryPathTextByteCount, RepositoryPathTextError> {
    u64::try_from(encoded.len())
        .map(RepositoryPathTextByteCount)
        .map_err(|_| RepositoryPathTextError::EncodedByteCountNotRepresentable)
}

fn encoded_length(
    path: &RepositoryPath,
) -> Result<(usize, RepositoryPathTextByteCount), RepositoryPathTextError> {
    let payload_length = path
        .as_bytes()
        .len()
        .checked_mul(2)
        .ok_or(RepositoryPathTextError::EncodedLengthNotRepresentable)?;
    let encoded_length = PREFIX
        .len()
        .checked_add(payload_length)
        .ok_or(RepositoryPathTextError::EncodedLengthNotRepresentable)?;
    let encoded_byte_count = u64::try_from(encoded_length)
        .map(RepositoryPathTextByteCount)
        .map_err(|_| RepositoryPathTextError::EncodedLengthNotRepresentable)?;

    Ok((encoded_length, encoded_byte_count))
}

fn enforce_encoded_limit(
    actual: RepositoryPathTextByteCount,
    limit: RepositoryPathTextByteLimit,
) -> Result<(), RepositoryPathTextError> {
    if actual.get() > limit.get() {
        return Err(RepositoryPathTextError::EncodedLimitExceeded { actual, limit });
    }
    Ok(())
}

const fn decode_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use repowitness_domain::{RepositoryPathByteCount, RepositoryPathError};

    use super::{
        RepositoryPath, RepositoryPathLimits, RepositoryPathTextByteCount,
        RepositoryPathTextByteLimit, RepositoryPathTextError, RepositoryPathTextV1,
        RepositoryPathTextVersion,
    };

    const GENEROUS_PATH: RepositoryPathLimits = RepositoryPathLimits::new(1024, 64);
    const GENEROUS_TEXT: RepositoryPathTextByteLimit = RepositoryPathTextByteLimit::new(4096);

    #[test]
    fn golden_vectors_round_trip_exact_bytes() {
        let fixtures: &[(&[u8], &str)] = &[
            (b"src/lib.rs", "rwp1:h:7372632F6C69622E7273"),
            (b"line\n\tname", "rwp1:h:6C696E650A096E616D65"),
            (&[0xff, b'/', 0x80], "rwp1:h:FF2F80"),
            ("é.rs".as_bytes(), "rwp1:h:C3A92E7273"),
            (b"C:\\temp", "rwp1:h:433A5C74656D70"),
        ];

        for (bytes, expected) in fixtures {
            let path = RepositoryPath::try_from_bytes(bytes, GENEROUS_PATH)
                .expect("fixture path must be valid");
            let encoded =
                RepositoryPathTextV1::encode(&path, GENEROUS_TEXT).expect("encoding must succeed");

            assert_eq!(encoded.as_str(), *expected);
            assert_eq!(encoded.as_ref(), *expected);
            assert_eq!(
                RepositoryPathTextV1::decode(encoded.as_str(), GENEROUS_TEXT, GENEROUS_PATH)
                    .expect("decoding must succeed"),
                path
            );
        }
    }

    #[test]
    fn every_valid_component_byte_round_trips() {
        for byte in 1_u8..=u8::MAX {
            if byte == b'/' {
                continue;
            }
            let bytes = [b'a', byte, b'b'];
            let path = RepositoryPath::try_from_bytes(&bytes, GENEROUS_PATH)
                .expect("surrounded non-NUL non-separator byte must be valid");
            let encoded =
                RepositoryPathTextV1::encode(&path, GENEROUS_TEXT).expect("encoding must succeed");
            let decoded =
                RepositoryPathTextV1::decode(encoded.as_str(), GENEROUS_TEXT, GENEROUS_PATH)
                    .expect("decoding must succeed");

            assert_eq!(decoded.as_bytes(), bytes);
        }
    }

    #[test]
    fn encoded_and_domain_ordering_are_identical() {
        let mut paths = [
            b"a".as_slice(),
            b"a/b".as_slice(),
            b"a0".as_slice(),
            b"\x7f".as_slice(),
            b"\x80".as_slice(),
        ]
        .map(|bytes| RepositoryPath::try_from_bytes(bytes, GENEROUS_PATH).expect("valid path"));
        paths.sort();

        let mut encoded = paths
            .iter()
            .map(|path| RepositoryPathTextV1::encode(path, GENEROUS_TEXT).expect("encodes"))
            .collect::<Vec<_>>();
        encoded.reverse();
        encoded.sort();

        let decoded = encoded
            .iter()
            .map(|value| {
                RepositoryPathTextV1::decode(value.as_str(), GENEROUS_TEXT, GENEROUS_PATH)
                    .expect("decodes")
            })
            .collect::<Vec<_>>();
        assert_eq!(decoded, paths);
    }

    #[test]
    fn encoded_limits_are_inclusive_for_encode_and_decode() {
        let path =
            RepositoryPath::try_from_bytes(b"src/lib.rs", GENEROUS_PATH).expect("valid path");
        let exact = RepositoryPathTextByteLimit::new(27);
        let below = RepositoryPathTextByteLimit::new(26);
        let encoded =
            RepositoryPathTextV1::encode(&path, exact).expect("exact output limit must pass");

        assert_eq!(encoded.encoded_byte_count().get(), 27);
        assert_eq!(
            encoded,
            RepositoryPathTextV1::encode(&path, GENEROUS_TEXT)
                .expect("construction limit must not affect identity")
        );
        assert_eq!(
            RepositoryPathTextV1::encode(&path, below)
                .expect_err("one byte below output size must fail"),
            RepositoryPathTextError::EncodedLimitExceeded {
                actual: RepositoryPathTextByteCount(27),
                limit: below,
            }
        );
        assert_eq!(
            RepositoryPathTextV1::decode(encoded.as_str(), exact, GENEROUS_PATH)
                .expect("exact input limit must pass"),
            path
        );
        assert_eq!(
            RepositoryPathTextV1::decode(encoded.as_str(), below, GENEROUS_PATH)
                .expect_err("one byte below input size must fail"),
            RepositoryPathTextError::EncodedLimitExceeded {
                actual: RepositoryPathTextByteCount(27),
                limit: below,
            }
        );
    }

    #[test]
    fn decoded_byte_limit_is_enforced_before_domain_construction() {
        let error = RepositoryPathTextV1::decode(
            "rwp1:h:7372632F6C69622E7273",
            GENEROUS_TEXT,
            RepositoryPathLimits::new(9, 64),
        )
        .expect_err("ten decoded bytes must exceed a nine-byte path limit");

        assert_eq!(
            error,
            RepositoryPathTextError::InvalidRepositoryPath(
                RepositoryPathError::ByteLimitExceeded {
                    actual: RepositoryPathByteCount::new(10),
                    limit: RepositoryPathByteCount::new(9),
                }
            )
        );
    }

    #[test]
    fn malformed_or_noncanonical_text_is_rejected() {
        assert_eq!(
            RepositoryPathTextV1::decode(
                "wrong:h:41",
                RepositoryPathTextByteLimit::new(1),
                GENEROUS_PATH,
            )
            .expect_err("the encoded limit must be checked before the tag"),
            RepositoryPathTextError::EncodedLimitExceeded {
                actual: RepositoryPathTextByteCount(10),
                limit: RepositoryPathTextByteLimit::new(1),
            }
        );

        let cases = [
            ("wrong:h:41", RepositoryPathTextError::InvalidTag),
            ("rwp2:h:41", RepositoryPathTextError::InvalidTag),
            ("rwp1:b:41", RepositoryPathTextError::InvalidTag),
            ("rwp1:h:", RepositoryPathTextError::EmptyPayload),
            ("rwp1:h:4", RepositoryPathTextError::OddPayloadLength),
            ("rwp1:h:aa", RepositoryPathTextError::NonCanonicalBase16),
            ("rwp1:h:GG", RepositoryPathTextError::NonCanonicalBase16),
            ("rwp1:h:20 0", RepositoryPathTextError::NonCanonicalBase16),
            ("rwp1:h:20=0", RepositoryPathTextError::NonCanonicalBase16),
        ];

        for (encoded, expected) in cases {
            assert_eq!(
                RepositoryPathTextV1::decode(encoded, GENEROUS_TEXT, GENEROUS_PATH)
                    .expect_err("malformed text must fail"),
                expected
            );
        }
    }

    #[test]
    fn decoded_path_rules_are_reapplied() {
        assert_eq!(
            RepositoryPathTextV1::decode("rwp1:h:00", GENEROUS_TEXT, GENEROUS_PATH)
                .expect_err("decoded NUL must fail"),
            RepositoryPathTextError::InvalidRepositoryPath(RepositoryPathError::ContainsNul)
        );
        assert_eq!(
            RepositoryPathTextV1::decode("rwp1:h:2E2E", GENEROUS_TEXT, GENEROUS_PATH)
                .expect_err("decoded parent component must fail"),
            RepositoryPathTextError::InvalidRepositoryPath(
                RepositoryPathError::ParentDirectoryComponent
            )
        );
        assert_eq!(
            RepositoryPathTextV1::decode(
                "rwp1:h:612F62",
                GENEROUS_TEXT,
                RepositoryPathLimits::new(1024, 1),
            )
            .expect_err("decoded component bound must be reapplied"),
            RepositoryPathTextError::InvalidRepositoryPath(
                RepositoryPathError::ComponentLimitExceeded {
                    actual: repowitness_domain::RepositoryPathComponentCount::new(2),
                    limit: repowitness_domain::RepositoryPathComponentCount::new(1),
                }
            )
        );
    }

    #[test]
    fn version_accessors_owned_output_and_redacted_debug_are_stable() {
        let path = RepositoryPath::try_from_bytes(b"private/customer.rs", GENEROUS_PATH)
            .expect("valid path");
        let encoded =
            RepositoryPathTextV1::encode(&path, GENEROUS_TEXT).expect("encoding must succeed");
        let debug = format!("{encoded:?}");

        assert_eq!(RepositoryPathTextV1::VERSION, RepositoryPathTextVersion::V1);
        assert_eq!(encoded.version(), RepositoryPathTextVersion::V1);
        assert_eq!(RepositoryPathTextVersion::V1.get(), 1);
        assert_eq!(RepositoryPathTextByteLimit::new(7).get(), 7);
        assert!(debug.contains("encoded_byte_count"));
        assert!(!debug.contains("private"));
        assert!(!debug.contains("customer"));
        assert!(!debug.contains("70726976617465"));
        assert!(encoded.into_string().starts_with("rwp1:h:"));
    }

    #[test]
    fn errors_have_stable_redacted_diagnostics_and_sources() {
        let cases = [
            (
                RepositoryPathTextError::EncodedByteCountNotRepresentable,
                "repository path text byte count cannot be represented as a u64",
            ),
            (
                RepositoryPathTextError::EncodedLengthNotRepresentable,
                "repository path encoded length cannot be represented",
            ),
            (
                RepositoryPathTextError::InvalidTag,
                "repository path text has an unknown version or encoding tag",
            ),
            (
                RepositoryPathTextError::EmptyPayload,
                "repository path text has an empty Base16 payload",
            ),
            (
                RepositoryPathTextError::OddPayloadLength,
                "repository path text Base16 payload has an odd length",
            ),
            (
                RepositoryPathTextError::NonCanonicalBase16,
                "repository path text payload is not canonical uppercase Base16",
            ),
            (
                RepositoryPathTextError::DecodedLengthNotRepresentable,
                "decoded repository path length cannot be represented as a u64",
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
            assert!(error.source().is_none());
        }

        let limit_error = RepositoryPathTextError::EncodedLimitExceeded {
            actual: RepositoryPathTextByteCount(11),
            limit: RepositoryPathTextByteLimit::new(10),
        };
        assert_eq!(
            limit_error.to_string(),
            "repository path text byte count 11 exceeds limit 10"
        );
        assert!(limit_error.source().is_none());

        let path_error =
            RepositoryPathTextError::from(RepositoryPathError::ParentDirectoryComponent);
        assert_eq!(
            path_error.to_string(),
            "decoded repository path is invalid: repository path cannot contain a '..' component"
        );
        assert!(path_error.source().is_some());
    }
}
