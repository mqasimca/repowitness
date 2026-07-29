use std::{error::Error, fmt};

use crate::RepositoryIdentityDigest;

/// Exact byte width of every connected-workspace and source-slot identity.
pub const WORKSPACE_ID_BYTES: usize = 32;

/// Error returned when a workspace identity has a noncanonical byte width.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceIdentityLengthError {
    actual_bytes: u64,
}

impl WorkspaceIdentityLengthError {
    /// Returns the observed byte count without exposing identity contents.
    #[must_use]
    pub const fn actual_bytes(self) -> u64 {
        self.actual_bytes
    }
}

impl fmt::Display for WorkspaceIdentityLengthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("workspace identity must contain exactly 32 bytes")
    }
}

impl Error for WorkspaceIdentityLengthError {}

macro_rules! define_workspace_identity {
    ($name:ident, $kind:literal, $documentation:literal) => {
        #[doc = $documentation]
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; WORKSPACE_ID_BYTES]);

        impl $name {
            /// Creates an identity from exactly 32 opaque bytes.
            #[must_use]
            pub const fn new(bytes: [u8; WORKSPACE_ID_BYTES]) -> Self {
                Self(bytes)
            }

            /// Copies an exactly sized boundary representation.
            pub fn try_from_slice(bytes: &[u8]) -> Result<Self, WorkspaceIdentityLengthError> {
                let identity = <[u8; WORKSPACE_ID_BYTES]>::try_from(bytes).map_err(|_| {
                    WorkspaceIdentityLengthError {
                        actual_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                    }
                })?;
                Ok(Self(identity))
            }

            /// Returns the exact opaque bytes.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; WORKSPACE_ID_BYTES] {
                &self.0
            }

            /// Consumes the value and returns its exact opaque bytes.
            #[must_use]
            pub const fn into_bytes(self) -> [u8; WORKSPACE_ID_BYTES] {
                self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($name))
                    .field("kind", &$kind)
                    .field("encoded_bytes", &WORKSPACE_ID_BYTES)
                    .finish_non_exhaustive()
            }
        }
    };
}

define_workspace_identity!(
    ConnectedWorkspaceId,
    "connected-workspace",
    "An opaque 32-byte identity for one configured connected workspace."
);
define_workspace_identity!(
    SourceSlotId,
    "source-slot",
    "An opaque 32-byte identity for one source selection in a connected workspace."
);

impl ConnectedWorkspaceId {
    /// Returns the compatibility workspace identity for one repository.
    #[must_use]
    pub const fn for_single_repository(repository: RepositoryIdentityDigest) -> Self {
        Self::new(repository.into_bytes())
    }
}

impl SourceSlotId {
    /// Returns the compatibility source-slot identity for one repository.
    #[must_use]
    pub const fn for_repository(repository: RepositoryIdentityDigest) -> Self {
        Self::new(repository.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use crate::RepositoryIdentityDigest;

    use super::{
        ConnectedWorkspaceId, SourceSlotId, WORKSPACE_ID_BYTES, WorkspaceIdentityLengthError,
    };

    #[test]
    fn identities_preserve_exact_bytes_and_kind() {
        let bytes = [0xA5; WORKSPACE_ID_BYTES];
        let workspace = ConnectedWorkspaceId::new(bytes);
        let slot = SourceSlotId::new(bytes);

        assert_eq!(workspace.as_bytes(), &bytes);
        assert_eq!(workspace.into_bytes(), bytes);
        assert_eq!(slot.as_bytes(), &bytes);
        assert_ne!(format!("{workspace:?}"), format!("{slot:?}"));
    }

    #[test]
    fn repository_compatibility_ids_preserve_bytes() {
        let repository = RepositoryIdentityDigest::new([0x3C; WORKSPACE_ID_BYTES]);

        assert_eq!(
            ConnectedWorkspaceId::for_single_repository(repository).as_bytes(),
            repository.as_bytes()
        );
        assert_eq!(
            SourceSlotId::for_repository(repository).as_bytes(),
            repository.as_bytes()
        );
    }

    #[test]
    fn boundary_length_errors_are_stable_and_redacted() {
        let error = SourceSlotId::try_from_slice(&[0xA5; 31]).unwrap_err();

        assert_eq!(error, WorkspaceIdentityLengthError { actual_bytes: 31 });
        assert_eq!(error.actual_bytes(), 31);
        assert_eq!(
            error.to_string(),
            "workspace identity must contain exactly 32 bytes"
        );
        assert!(!format!("{error:?}").contains("A5"));
    }

    #[test]
    fn debug_output_does_not_expose_identity_bytes() {
        let workspace = ConnectedWorkspaceId::new([0xA5; WORKSPACE_ID_BYTES]);
        let slot = SourceSlotId::new([0xA5; WORKSPACE_ID_BYTES]);

        for debug in [format!("{workspace:?}"), format!("{slot:?}")] {
            assert!(!debug.contains("A5"));
            assert!(!debug.contains("165"));
        }
    }
}
