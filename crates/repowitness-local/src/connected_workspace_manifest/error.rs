use core::fmt;

use repowitness_application::{
    PackageScopeError, RepositoryIdentityTextError, RepositoryPathTextError,
    WorkspaceIdentityTextError,
};

use crate::source_selector::SourceSelectorAdmissionError;

/// Stable content-, selector-, and host-path-redacted manifest failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ConnectedWorkspaceManifestError {
    /// Input exceeded the one-mebibyte boundary before parsing.
    InputTooLarge {
        /// Inclusive byte limit.
        limit: u64,
    },
    /// Input was not UTF-8.
    InvalidUtf8,
    /// TOML syntax, structure, types, duplicate keys, or unknown keys failed.
    InvalidToml,
    /// The required version-1 schema was not selected.
    UnsupportedSchemaVersion,
    /// The source count was outside the inclusive version-1 range.
    SourceCountOutOfRange {
        /// Minimum source count.
        minimum: u64,
        /// Maximum source count.
        maximum: u64,
    },
    /// The connected-workspace identity was not canonical.
    InvalidConnectedWorkspaceId {
        /// Redacted identity-codec failure.
        source: WorkspaceIdentityTextError,
    },
    /// One source tuple failed validation.
    InvalidSource {
        /// One-based manifest source ordinal.
        ordinal: u64,
        /// Redacted source validation failure.
        source: ConnectedWorkspaceManifestSourceError,
    },
    /// Two source tuples named the same exact source-slot identity.
    DuplicateSourceSlot,
}

impl fmt::Display for ConnectedWorkspaceManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge { limit } => {
                write!(
                    formatter,
                    "workspace manifest exceeds its {limit} byte limit"
                )
            }
            Self::InvalidUtf8 => formatter.write_str("workspace manifest is not valid UTF-8"),
            Self::InvalidToml => {
                formatter.write_str("workspace manifest syntax or schema is invalid")
            }
            Self::UnsupportedSchemaVersion => {
                formatter.write_str("workspace manifest schema version is unsupported")
            }
            Self::SourceCountOutOfRange { minimum, maximum } => write!(
                formatter,
                "workspace manifest source count must be between {minimum} and {maximum}"
            ),
            Self::InvalidConnectedWorkspaceId { .. } => {
                formatter.write_str("workspace manifest identity is invalid")
            }
            Self::InvalidSource { ordinal, .. } => {
                write!(formatter, "workspace manifest source {ordinal} is invalid")
            }
            Self::DuplicateSourceSlot => {
                formatter.write_str("workspace manifest contains a duplicate source slot")
            }
        }
    }
}

impl std::error::Error for ConnectedWorkspaceManifestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidConnectedWorkspaceId { source } => Some(source),
            Self::InvalidSource { source, .. } => Some(source),
            Self::InputTooLarge { .. }
            | Self::InvalidUtf8
            | Self::InvalidToml
            | Self::UnsupportedSchemaVersion
            | Self::SourceCountOutOfRange { .. }
            | Self::DuplicateSourceSlot => None,
        }
    }
}

/// Stable redacted failure for one manifest source tuple.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ConnectedWorkspaceManifestSourceError {
    /// The source-slot identity was not canonical.
    SourceSlotId {
        /// Redacted identity-codec failure.
        source: WorkspaceIdentityTextError,
    },
    /// The logical repository identity was not canonical.
    RepositoryIdentity {
        /// Redacted identity-codec failure.
        source: RepositoryIdentityTextError,
    },
    /// The UTF-8 worktree root was empty, over-limit, or contained NUL.
    WorktreeRoot,
    /// Selector kind and required/forbidden value fields disagreed.
    SelectorShape,
    /// Selector admission rejected the typed selector value.
    Selector {
        /// Redacted selector-admission failure.
        source: SourceSelectorAdmissionError,
    },
    /// Scope kind and required/forbidden roots fields disagreed.
    ScopeShape,
    /// One package-root text scalar failed canonical decoding.
    PackageRoot {
        /// One-based package-root ordinal.
        ordinal: u64,
        /// Redacted repository-path text failure.
        source: RepositoryPathTextError,
    },
    /// Canonical roots violated the bounded package-scope contract.
    PackageScope {
        /// Redacted scope-validation failure.
        source: PackageScopeError,
    },
}

impl fmt::Display for ConnectedWorkspaceManifestSourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SourceSlotId { .. } => "source-slot identity is invalid",
            Self::RepositoryIdentity { .. } => "repository identity is invalid",
            Self::WorktreeRoot => "worktree root is invalid",
            Self::SelectorShape => "source selector structure is invalid",
            Self::Selector { .. } => "source selector value is invalid",
            Self::ScopeShape => "package scope structure is invalid",
            Self::PackageRoot { .. } => "package-root text is invalid",
            Self::PackageScope { .. } => "package scope is invalid",
        })
    }
}

impl std::error::Error for ConnectedWorkspaceManifestSourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SourceSlotId { source } => Some(source),
            Self::RepositoryIdentity { source } => Some(source),
            Self::Selector { source } => Some(source),
            Self::PackageRoot { source, .. } => Some(source),
            Self::PackageScope { source } => Some(source),
            Self::WorktreeRoot | Self::SelectorShape | Self::ScopeShape => None,
        }
    }
}
