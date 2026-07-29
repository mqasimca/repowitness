use std::{error::Error, fmt};

use repowitness_domain::{ConnectedWorkspaceId, SourceSlotId, WORKSPACE_ID_BYTES};

const CONNECTED_WORKSPACE_PREFIX: &str = "cwi1:h:";
const SOURCE_SLOT_PREFIX: &str = "ssi1:h:";
const PREFIX_BYTES: usize = CONNECTED_WORKSPACE_PREFIX.len();
const PAYLOAD_BYTES: usize = WORKSPACE_ID_BYTES * 2;

/// Exact byte length of a canonical version-1 connected-workspace identity.
pub const CONNECTED_WORKSPACE_ID_TEXT_BYTES: usize = PREFIX_BYTES + PAYLOAD_BYTES;
/// Exact byte length of a canonical version-1 source-slot identity.
pub const SOURCE_SLOT_ID_TEXT_BYTES: usize = PREFIX_BYTES + PAYLOAD_BYTES;

/// Canonical tagged text for one explicit connected-workspace identity.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ConnectedWorkspaceIdTextV1(String);

impl ConnectedWorkspaceIdTextV1 {
    /// Encodes one connected-workspace identity as strict uppercase Base16.
    #[must_use]
    pub fn encode(identity: ConnectedWorkspaceId) -> Self {
        Self(encode(CONNECTED_WORKSPACE_PREFIX, identity.as_bytes()))
    }

    /// Decodes one canonical connected-workspace identity.
    pub fn decode(text: &str) -> Result<ConnectedWorkspaceId, WorkspaceIdentityTextError> {
        decode(text, CONNECTED_WORKSPACE_PREFIX).map(ConnectedWorkspaceId::new)
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

impl AsRef<str> for ConnectedWorkspaceIdTextV1 {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for ConnectedWorkspaceIdTextV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug(
            formatter,
            "ConnectedWorkspaceIdTextV1",
            CONNECTED_WORKSPACE_ID_TEXT_BYTES,
        )
    }
}

/// Canonical tagged text for one explicit source-slot identity.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceSlotIdTextV1(String);

impl SourceSlotIdTextV1 {
    /// Encodes one source-slot identity as strict uppercase Base16.
    #[must_use]
    pub fn encode(identity: SourceSlotId) -> Self {
        Self(encode(SOURCE_SLOT_PREFIX, identity.as_bytes()))
    }

    /// Decodes one canonical source-slot identity.
    pub fn decode(text: &str) -> Result<SourceSlotId, WorkspaceIdentityTextError> {
        decode(text, SOURCE_SLOT_PREFIX).map(SourceSlotId::new)
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

impl AsRef<str> for SourceSlotIdTextV1 {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for SourceSlotIdTextV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug(formatter, "SourceSlotIdTextV1", SOURCE_SLOT_ID_TEXT_BYTES)
    }
}

/// Stable failure while decoding a workspace identity boundary scalar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceIdentityTextError {
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

impl WorkspaceIdentityTextError {
    /// Returns the observed encoded byte count when length validation failed.
    #[must_use]
    pub const fn actual_bytes(self) -> Option<u64> {
        match self {
            Self::InvalidLength { actual_bytes } => Some(actual_bytes),
            Self::InvalidPrefix | Self::InvalidBase16 => None,
        }
    }
}

impl fmt::Display for WorkspaceIdentityTextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLength { .. } => "workspace identity text has an invalid byte count",
            Self::InvalidPrefix => "workspace identity text has an invalid format tag",
            Self::InvalidBase16 => "workspace identity text has a non-canonical Base16 payload",
        })
    }
}

impl Error for WorkspaceIdentityTextError {}

fn encode(prefix: &str, bytes: &[u8; WORKSPACE_ID_BYTES]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let mut encoded = String::with_capacity(PREFIX_BYTES + PAYLOAD_BYTES);
    encoded.push_str(prefix);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0F)]));
    }
    encoded
}

fn decode(
    text: &str,
    prefix: &str,
) -> Result<[u8; WORKSPACE_ID_BYTES], WorkspaceIdentityTextError> {
    if text.len() != PREFIX_BYTES + PAYLOAD_BYTES {
        return Err(WorkspaceIdentityTextError::InvalidLength {
            actual_bytes: u64::try_from(text.len()).unwrap_or(u64::MAX),
        });
    }
    let payload = text
        .strip_prefix(prefix)
        .ok_or(WorkspaceIdentityTextError::InvalidPrefix)?
        .as_bytes();
    let mut decoded = [0_u8; WORKSPACE_ID_BYTES];
    for (output, pair) in decoded.iter_mut().zip(payload.chunks_exact(2)) {
        let high = decode_nibble(pair[0])?;
        let low = decode_nibble(pair[1])?;
        *output = (high << 4) | low;
    }
    Ok(decoded)
}

fn decode_nibble(byte: u8) -> Result<u8, WorkspaceIdentityTextError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(WorkspaceIdentityTextError::InvalidBase16),
    }
}

fn redacted_debug(
    formatter: &mut fmt::Formatter<'_>,
    type_name: &str,
    encoded_bytes: usize,
) -> fmt::Result {
    formatter
        .debug_struct(type_name)
        .field("version", &1_u32)
        .field("encoded_bytes", &encoded_bytes)
        .finish_non_exhaustive()
}

#[cfg(test)]
mod tests {
    use repowitness_domain::{ConnectedWorkspaceId, SourceSlotId};

    use super::{
        CONNECTED_WORKSPACE_ID_TEXT_BYTES, ConnectedWorkspaceIdTextV1, SOURCE_SLOT_ID_TEXT_BYTES,
        SourceSlotIdTextV1, WorkspaceIdentityTextError,
    };

    #[test]
    fn golden_vectors_round_trip_exact_bytes() {
        let workspace = ConnectedWorkspaceId::new([0xAB; 32]);
        let slot = SourceSlotId::new([0xCD; 32]);
        let workspace_text = ConnectedWorkspaceIdTextV1::encode(workspace);
        let slot_text = SourceSlotIdTextV1::encode(slot);

        assert_eq!(
            workspace_text.as_str(),
            "cwi1:h:ABABABABABABABABABABABABABABABABABABABABABABABABABABABABABABABAB"
        );
        assert_eq!(
            slot_text.as_str(),
            "ssi1:h:CDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCDCD"
        );
        assert_eq!(
            ConnectedWorkspaceIdTextV1::decode(workspace_text.as_str()),
            Ok(workspace)
        );
        assert_eq!(SourceSlotIdTextV1::decode(slot_text.as_str()), Ok(slot));
        assert_eq!(
            workspace_text.as_str().len(),
            CONNECTED_WORKSPACE_ID_TEXT_BYTES
        );
        assert_eq!(slot_text.as_str().len(), SOURCE_SLOT_ID_TEXT_BYTES);
    }

    #[test]
    fn every_byte_value_round_trips_for_both_kinds() {
        for byte in u8::MIN..=u8::MAX {
            let workspace = ConnectedWorkspaceId::new([byte; 32]);
            let slot = SourceSlotId::new([byte; 32]);
            let workspace_text = ConnectedWorkspaceIdTextV1::encode(workspace);
            let slot_text = SourceSlotIdTextV1::encode(slot);

            assert_eq!(
                ConnectedWorkspaceIdTextV1::decode(workspace_text.as_str()),
                Ok(workspace)
            );
            assert_eq!(SourceSlotIdTextV1::decode(slot_text.as_str()), Ok(slot));
        }
    }

    #[test]
    fn alternate_kinds_and_malformed_forms_are_rejected() {
        let canonical =
            ConnectedWorkspaceIdTextV1::encode(ConnectedWorkspaceId::new([0xAB; 32])).into_string();
        let slot = SourceSlotIdTextV1::encode(SourceSlotId::new([0xAB; 32])).into_string();
        let mut lowercase = canonical.clone();
        lowercase.replace_range(7..8, "a");
        let mut nul = canonical.clone();
        nul.replace_range(7..8, "\0");
        let mut non_ascii = canonical.clone();
        non_ascii.replace_range(7..9, "é");

        assert_eq!(
            ConnectedWorkspaceIdTextV1::decode(&canonical[..canonical.len() - 1]),
            Err(WorkspaceIdentityTextError::InvalidLength { actual_bytes: 70 })
        );
        assert_eq!(
            ConnectedWorkspaceIdTextV1::decode(&(canonical.clone() + "0")),
            Err(WorkspaceIdentityTextError::InvalidLength { actual_bytes: 72 })
        );
        assert_eq!(
            ConnectedWorkspaceIdTextV1::decode(&slot),
            Err(WorkspaceIdentityTextError::InvalidPrefix)
        );
        assert_eq!(
            ConnectedWorkspaceIdTextV1::decode(&lowercase),
            Err(WorkspaceIdentityTextError::InvalidBase16)
        );
        assert_eq!(
            ConnectedWorkspaceIdTextV1::decode(&nul),
            Err(WorkspaceIdentityTextError::InvalidBase16)
        );
        assert_eq!(
            ConnectedWorkspaceIdTextV1::decode(&non_ascii),
            Err(WorkspaceIdentityTextError::InvalidBase16)
        );
    }

    #[test]
    fn errors_and_debug_output_are_redacted() {
        let workspace = ConnectedWorkspaceIdTextV1::encode(ConnectedWorkspaceId::new([0xA5; 32]));
        let slot = SourceSlotIdTextV1::encode(SourceSlotId::new([0xA5; 32]));
        let error = ConnectedWorkspaceIdTextV1::decode("private").expect_err("input should fail");

        assert_eq!(error.actual_bytes(), Some(7));
        assert_eq!(
            error.to_string(),
            "workspace identity text has an invalid byte count"
        );
        for debug in [format!("{workspace:?}"), format!("{slot:?}")] {
            assert!(!debug.contains("A5"));
            assert!(!debug.contains(workspace.as_str()));
            assert!(!debug.contains(slot.as_str()));
        }
    }
}
