use core::fmt;

use crate::{
    BoundedFileReadError, ContainedSourceError, GitPathDiscoveryError, LocalIndexError,
    LocalRustIndexError, SqliteStoreError,
    connected_workspace_manifest::{
        ConnectedWorkspaceManifestError, ConnectedWorkspaceManifestSourceError,
    },
    local_index::connected_workspace::{
        ConnectedSourceSlotFinalFenceError,
        model::{
            ConnectedWorkspaceIndexError as InternalIndexError, ConnectedWorkspaceRequestError,
        },
    },
    rust_index::LocalSourceSnapshotFenceError,
    source_selector::{SourceSelectorFinalFenceError, SourceSelectorResolutionError},
    source_state::SourceStateError,
};

/// Stable category for hostile manifest admission failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalConnectedWorkspaceManifestErrorKind {
    /// The admitted bytes exceeded the version-1 limit.
    InputTooLarge,
    /// The admitted bytes were not valid UTF-8.
    InvalidEncoding,
    /// TOML syntax, shape, types, duplicate keys, or unknown keys failed.
    InvalidSyntaxOrSchema,
    /// The requested manifest schema version is unsupported.
    UnsupportedSchemaVersion,
    /// The source count was outside the version-1 range.
    SourceCount,
    /// The connected-workspace identity was not canonical.
    WorkspaceIdentity,
    /// A source-slot identity was not canonical.
    SourceSlotIdentity,
    /// A logical repository identity was not canonical.
    RepositoryIdentity,
    /// A manifest worktree root was not admissible.
    WorktreeRoot,
    /// A source selector shape or value was invalid.
    Selector,
    /// A package-scope shape, root, or bound was invalid.
    PackageScope,
    /// Two source tuples named the same exact source slot.
    DuplicateSourceSlot,
}

/// Stable invalid connected-workspace request category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalConnectedWorkspaceRequestErrorKind {
    /// A source or whole-operation deadline was zero.
    Deadline,
    /// Selector output or aggregate path bounds were zero or not representable.
    ResourceLimit,
    /// Source cardinality was outside the supported range.
    SourceCount,
    /// Two source tuples named the same source slot.
    DuplicateSourceSlot,
    /// An explicit identity collided with a reserved compatibility identity.
    ReservedIdentity,
    /// Source slots did not share one exact resolved configuration.
    MixedConfiguration,
    /// A typed selector failed request admission.
    Selector,
}

/// Stable category for an admitted manifest-parent authority failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalConnectedWorkspaceParentErrorKind {
    /// The original no-follow ancestor identity chain was replaced.
    Changed,
    /// The admitted parent authority could not be revalidated.
    Unavailable,
}

/// Stable connected-workspace failure phase without paths or raw selectors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalConnectedWorkspacePhase {
    /// Worktree and database isolation could not be proven.
    DatabaseIsolation,
    /// One typed selector could not be resolved.
    SelectorResolution,
    /// One complete scoped source could not be prepared.
    Preparation,
    /// The owned SQLite writer could not start.
    StoreStartup,
    /// Canonical connected-workspace membership could not be registered.
    WorkspaceRegistration,
    /// One immutable source generation could not be staged.
    PublicationStaging,
    /// One immutable graph generation could not be staged.
    GraphPublicationStaging,
    /// One source or selector changed at its final fence.
    FinalSourceFence,
    /// One source-slot completion receipt could not be recorded.
    Completion,
    /// The complete immutable workspace view could not be published.
    ViewPublication,
}

/// Typed, aggregate-only, path- and selector-redacted facade failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalConnectedWorkspaceIndexError {
    /// Hostile manifest admission failed.
    Manifest {
        /// Stable admission category.
        kind: LocalConnectedWorkspaceManifestErrorKind,
        /// One-based original manifest source ordinal, when applicable.
        source_ordinal: Option<u64>,
    },
    /// Explicit facade or coordinator request validation failed.
    InvalidRequest {
        /// Stable request category.
        kind: LocalConnectedWorkspaceRequestErrorKind,
    },
    /// The manifest parent no longer names its admitted ancestor chain.
    ManifestParent {
        /// Stable authority category.
        kind: LocalConnectedWorkspaceParentErrorKind,
    },
    /// A configured duration could not be represented by the monotonic clock.
    DeadlineNotRepresentable,
    /// Shared cancellation was observed before publication.
    Cancelled,
    /// An explicit operation deadline elapsed before publication.
    DeadlineExceeded,
    /// A queued SQLite mutation may have committed without a definitive receipt.
    MutationOutcomeUnknown {
        /// Exact coordinator phase containing the uncertain mutation.
        phase: LocalConnectedWorkspacePhase,
        /// One-based canonical source-slot ordinal, when applicable.
        source_ordinal: Option<u64>,
    },
    /// A redacted coordinator phase failed before publication.
    Phase {
        /// Stable failure phase.
        phase: LocalConnectedWorkspacePhase,
        /// One-based canonical source-slot ordinal, when applicable.
        source_ordinal: Option<u64>,
    },
}

impl fmt::Display for LocalConnectedWorkspaceIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Manifest { .. } => "connected-workspace manifest is invalid",
            Self::InvalidRequest { .. } => "connected-workspace request is invalid",
            Self::ManifestParent { .. } => "connected-workspace manifest parent authority changed",
            Self::DeadlineNotRepresentable => "connected-workspace deadline is not representable",
            Self::Cancelled => "connected-workspace indexing was cancelled",
            Self::DeadlineExceeded => "connected-workspace indexing exceeded its deadline",
            Self::MutationOutcomeUnknown { .. } => {
                "connected-workspace mutation outcome could not be determined"
            }
            Self::Phase { .. } => "connected-workspace indexing failed before publication",
        })
    }
}

impl std::error::Error for LocalConnectedWorkspaceIndexError {}

impl LocalConnectedWorkspaceIndexError {
    /// Returns non-sensitive authoritative guidance for an unknown mutation.
    #[must_use]
    pub const fn reconciliation_guidance(&self) -> Option<&'static str> {
        let Self::MutationOutcomeUnknown { phase, .. } = self else {
            return None;
        };
        Some(match phase {
            LocalConnectedWorkspacePhase::StoreStartup => {
                "reopen the store and run read-only database diagnostics before retrying startup"
            }
            LocalConnectedWorkspacePhase::WorkspaceRegistration => {
                "reopen the store and read durable workspace membership and source-slot epochs before retrying"
            }
            LocalConnectedWorkspacePhase::PublicationStaging => {
                "reopen the store, allow startup recovery, and inspect the source-slot candidate before retrying"
            }
            LocalConnectedWorkspacePhase::GraphPublicationStaging => {
                "reopen the store and inspect the candidate generation graph receipt before retrying"
            }
            LocalConnectedWorkspacePhase::Completion => {
                "reopen the store and read the durable source-slot completion before retrying"
            }
            LocalConnectedWorkspacePhase::ViewPublication => {
                "reopen the store and read the active immutable workspace view before retrying"
            }
            LocalConnectedWorkspacePhase::DatabaseIsolation
            | LocalConnectedWorkspacePhase::SelectorResolution
            | LocalConnectedWorkspacePhase::Preparation
            | LocalConnectedWorkspacePhase::FinalSourceFence => {
                "reopen the store and inspect authoritative workspace state before retrying"
            }
        })
    }

    pub(super) const fn from_manifest(source: ConnectedWorkspaceManifestError) -> Self {
        let (kind, source_ordinal) = match source {
            ConnectedWorkspaceManifestError::InputTooLarge { .. } => (
                LocalConnectedWorkspaceManifestErrorKind::InputTooLarge,
                None,
            ),
            ConnectedWorkspaceManifestError::InvalidUtf8 => (
                LocalConnectedWorkspaceManifestErrorKind::InvalidEncoding,
                None,
            ),
            ConnectedWorkspaceManifestError::InvalidToml => (
                LocalConnectedWorkspaceManifestErrorKind::InvalidSyntaxOrSchema,
                None,
            ),
            ConnectedWorkspaceManifestError::UnsupportedSchemaVersion => (
                LocalConnectedWorkspaceManifestErrorKind::UnsupportedSchemaVersion,
                None,
            ),
            ConnectedWorkspaceManifestError::SourceCountOutOfRange { .. } => {
                (LocalConnectedWorkspaceManifestErrorKind::SourceCount, None)
            }
            ConnectedWorkspaceManifestError::InvalidConnectedWorkspaceId { .. } => (
                LocalConnectedWorkspaceManifestErrorKind::WorkspaceIdentity,
                None,
            ),
            ConnectedWorkspaceManifestError::InvalidSource { ordinal, source } => {
                (manifest_source_kind(source), Some(ordinal))
            }
            ConnectedWorkspaceManifestError::DuplicateSourceSlot => (
                LocalConnectedWorkspaceManifestErrorKind::DuplicateSourceSlot,
                None,
            ),
        };
        Self::Manifest {
            kind,
            source_ordinal,
        }
    }

    pub(super) const fn from_request(source: ConnectedWorkspaceRequestError) -> Self {
        let kind = match source {
            ConnectedWorkspaceRequestError::ZeroDeadline => {
                LocalConnectedWorkspaceRequestErrorKind::Deadline
            }
            ConnectedWorkspaceRequestError::AggregatePathLimitExceeded => {
                LocalConnectedWorkspaceRequestErrorKind::ResourceLimit
            }
            ConnectedWorkspaceRequestError::EmptySourceSlots
            | ConnectedWorkspaceRequestError::SourceSlotLimitExceeded { .. } => {
                LocalConnectedWorkspaceRequestErrorKind::SourceCount
            }
            ConnectedWorkspaceRequestError::DuplicateSourceSlot => {
                LocalConnectedWorkspaceRequestErrorKind::DuplicateSourceSlot
            }
            ConnectedWorkspaceRequestError::ReservedCompatibilityWorkspace
            | ConnectedWorkspaceRequestError::ReservedCompatibilitySourceSlot => {
                LocalConnectedWorkspaceRequestErrorKind::ReservedIdentity
            }
            ConnectedWorkspaceRequestError::MixedConfiguration => {
                LocalConnectedWorkspaceRequestErrorKind::MixedConfiguration
            }
            #[cfg(test)]
            ConnectedWorkspaceRequestError::Selector { .. } => {
                LocalConnectedWorkspaceRequestErrorKind::Selector
            }
        };
        Self::InvalidRequest { kind }
    }

    pub(super) const fn from_parent(source: BoundedFileReadError) -> Self {
        let kind = match source {
            BoundedFileReadError::Changed => LocalConnectedWorkspaceParentErrorKind::Changed,
            BoundedFileReadError::InvalidRequest
            | BoundedFileReadError::Unavailable
            | BoundedFileReadError::TooLarge => LocalConnectedWorkspaceParentErrorKind::Unavailable,
        };
        Self::ManifestParent { kind }
    }

    pub(super) fn from_internal(source: InternalIndexError) -> Self {
        match source {
            InternalIndexError::DeadlineNotRepresentable => Self::DeadlineNotRepresentable,
            InternalIndexError::Cancelled => Self::Cancelled,
            InternalIndexError::DeadlineExceeded => Self::DeadlineExceeded,
            InternalIndexError::ManifestParentAuthority { source } => Self::from_parent(source),
            InternalIndexError::SelectorResolution {
                slot_ordinal,
                source,
            } => Self::controlled_or_phase(
                selector_control(source),
                LocalConnectedWorkspacePhase::SelectorResolution,
                Some(slot_ordinal),
            ),
            InternalIndexError::DatabaseIsolation { source } => Self::controlled_or_phase(
                local_index_control(
                    source,
                    LocalConnectedWorkspacePhase::DatabaseIsolation,
                    None,
                ),
                LocalConnectedWorkspacePhase::DatabaseIsolation,
                None,
            ),
            InternalIndexError::Preparation {
                slot_ordinal,
                source,
            } => Self::controlled_or_phase(
                local_index_control(
                    source,
                    LocalConnectedWorkspacePhase::Preparation,
                    Some(slot_ordinal),
                ),
                LocalConnectedWorkspacePhase::Preparation,
                Some(slot_ordinal),
            ),
            InternalIndexError::StoreStartup { source } => Self::controlled_or_phase(
                store_control(source, LocalConnectedWorkspacePhase::StoreStartup, None),
                LocalConnectedWorkspacePhase::StoreStartup,
                None,
            ),
            InternalIndexError::WorkspaceRegistration { source } => Self::controlled_or_phase(
                store_control(
                    source,
                    LocalConnectedWorkspacePhase::WorkspaceRegistration,
                    None,
                ),
                LocalConnectedWorkspacePhase::WorkspaceRegistration,
                None,
            ),
            InternalIndexError::PublicationStaging {
                slot_ordinal,
                source,
            } => Self::controlled_or_phase(
                store_control(
                    source,
                    LocalConnectedWorkspacePhase::PublicationStaging,
                    Some(slot_ordinal),
                ),
                LocalConnectedWorkspacePhase::PublicationStaging,
                Some(slot_ordinal),
            ),
            InternalIndexError::GraphPublicationStaging {
                slot_ordinal,
                source,
            } => Self::controlled_or_phase(
                store_control(
                    source,
                    LocalConnectedWorkspacePhase::GraphPublicationStaging,
                    Some(slot_ordinal),
                ),
                LocalConnectedWorkspacePhase::GraphPublicationStaging,
                Some(slot_ordinal),
            ),
            InternalIndexError::FinalSourceFence {
                slot_ordinal,
                source,
            } => Self::controlled_or_phase(
                final_fence_control(source),
                LocalConnectedWorkspacePhase::FinalSourceFence,
                Some(slot_ordinal),
            ),
            InternalIndexError::Completion {
                slot_ordinal,
                source,
            } => Self::controlled_or_phase(
                store_control(
                    source,
                    LocalConnectedWorkspacePhase::Completion,
                    Some(slot_ordinal),
                ),
                LocalConnectedWorkspacePhase::Completion,
                Some(slot_ordinal),
            ),
            InternalIndexError::ViewPublication { source } => Self::controlled_or_phase(
                store_control(source, LocalConnectedWorkspacePhase::ViewPublication, None),
                LocalConnectedWorkspacePhase::ViewPublication,
                None,
            ),
        }
    }

    fn controlled_or_phase(
        control: Option<Self>,
        phase: LocalConnectedWorkspacePhase,
        source_ordinal: Option<u64>,
    ) -> Self {
        control.unwrap_or(Self::Phase {
            phase,
            source_ordinal,
        })
    }
}

const fn manifest_source_kind(
    source: ConnectedWorkspaceManifestSourceError,
) -> LocalConnectedWorkspaceManifestErrorKind {
    match source {
        ConnectedWorkspaceManifestSourceError::SourceSlotId { .. } => {
            LocalConnectedWorkspaceManifestErrorKind::SourceSlotIdentity
        }
        ConnectedWorkspaceManifestSourceError::RepositoryIdentity { .. } => {
            LocalConnectedWorkspaceManifestErrorKind::RepositoryIdentity
        }
        ConnectedWorkspaceManifestSourceError::WorktreeRoot => {
            LocalConnectedWorkspaceManifestErrorKind::WorktreeRoot
        }
        ConnectedWorkspaceManifestSourceError::SelectorShape
        | ConnectedWorkspaceManifestSourceError::Selector { .. } => {
            LocalConnectedWorkspaceManifestErrorKind::Selector
        }
        ConnectedWorkspaceManifestSourceError::ScopeShape
        | ConnectedWorkspaceManifestSourceError::PackageRoot { .. }
        | ConnectedWorkspaceManifestSourceError::PackageScope { .. } => {
            LocalConnectedWorkspaceManifestErrorKind::PackageScope
        }
    }
}

fn selector_control(
    source: SourceSelectorResolutionError,
) -> Option<LocalConnectedWorkspaceIndexError> {
    match source {
        SourceSelectorResolutionError::DeadlineNotRepresentable => {
            Some(LocalConnectedWorkspaceIndexError::DeadlineNotRepresentable)
        }
        SourceSelectorResolutionError::Cancelled => {
            Some(LocalConnectedWorkspaceIndexError::Cancelled)
        }
        SourceSelectorResolutionError::DeadlineExceeded { .. } => {
            Some(LocalConnectedWorkspaceIndexError::DeadlineExceeded)
        }
        SourceSelectorResolutionError::Git { source } => git_control(source),
        _ => None,
    }
}

fn local_index_control(
    source: LocalIndexError,
    phase: LocalConnectedWorkspacePhase,
    source_ordinal: Option<u64>,
) -> Option<LocalConnectedWorkspaceIndexError> {
    match source {
        LocalIndexError::DeadlineNotRepresentable => {
            Some(LocalConnectedWorkspaceIndexError::DeadlineNotRepresentable)
        }
        LocalIndexError::Preparation { source } => {
            local_rust_control(source, phase, source_ordinal)
        }
        LocalIndexError::StoreStartup { source }
        | LocalIndexError::ArtifactReuse { source }
        | LocalIndexError::WorkspaceRegistration { source }
        | LocalIndexError::PublicationStaging { source }
        | LocalIndexError::GraphPublicationStaging { source }
        | LocalIndexError::PublicationActivation { source }
        | LocalIndexError::Checkpoint { source }
        | LocalIndexError::Shutdown { source } => store_control(source, phase, source_ordinal),
        LocalIndexError::MutationOutcomeUnknown { .. } => {
            Some(LocalConnectedWorkspaceIndexError::MutationOutcomeUnknown {
                phase,
                source_ordinal,
            })
        }
        _ => None,
    }
}

fn local_rust_control(
    source: LocalRustIndexError,
    phase: LocalConnectedWorkspacePhase,
    source_ordinal: Option<u64>,
) -> Option<LocalConnectedWorkspaceIndexError> {
    match source {
        LocalRustIndexError::DeadlineNotRepresentable => {
            Some(LocalConnectedWorkspaceIndexError::DeadlineNotRepresentable)
        }
        LocalRustIndexError::Cancelled => Some(LocalConnectedWorkspaceIndexError::Cancelled),
        LocalRustIndexError::DeadlineExceeded => {
            Some(LocalConnectedWorkspaceIndexError::DeadlineExceeded)
        }
        LocalRustIndexError::Discovery { source } => git_control(source),
        LocalRustIndexError::SourceState { source } => source_state_control(source),
        LocalRustIndexError::RootOpen { source }
        | LocalRustIndexError::SourceRead { source, .. }
        | LocalRustIndexError::RevalidationRead { source, .. } => contained_control(source),
        LocalRustIndexError::ArtifactReuse { source } => {
            store_control(source, phase, source_ordinal)
        }
        _ => None,
    }
}

fn source_state_control(source: SourceStateError) -> Option<LocalConnectedWorkspaceIndexError> {
    match source {
        SourceStateError::DeadlineNotRepresentable => {
            Some(LocalConnectedWorkspaceIndexError::DeadlineNotRepresentable)
        }
        SourceStateError::Git { source } => git_control(source),
        _ => None,
    }
}

fn git_control(source: GitPathDiscoveryError) -> Option<LocalConnectedWorkspaceIndexError> {
    match source {
        GitPathDiscoveryError::DeadlineNotRepresentable => {
            Some(LocalConnectedWorkspaceIndexError::DeadlineNotRepresentable)
        }
        GitPathDiscoveryError::Cancelled => Some(LocalConnectedWorkspaceIndexError::Cancelled),
        GitPathDiscoveryError::DeadlineExceeded { .. } => {
            Some(LocalConnectedWorkspaceIndexError::DeadlineExceeded)
        }
        _ => None,
    }
}

fn contained_control(source: ContainedSourceError) -> Option<LocalConnectedWorkspaceIndexError> {
    match source {
        ContainedSourceError::DeadlineNotRepresentable => {
            Some(LocalConnectedWorkspaceIndexError::DeadlineNotRepresentable)
        }
        ContainedSourceError::Cancelled => Some(LocalConnectedWorkspaceIndexError::Cancelled),
        ContainedSourceError::DeadlineExceeded { .. } => {
            Some(LocalConnectedWorkspaceIndexError::DeadlineExceeded)
        }
        _ => None,
    }
}

const fn store_control(
    source: SqliteStoreError,
    phase: LocalConnectedWorkspacePhase,
    source_ordinal: Option<u64>,
) -> Option<LocalConnectedWorkspaceIndexError> {
    match source {
        SqliteStoreError::Cancelled => Some(LocalConnectedWorkspaceIndexError::Cancelled),
        SqliteStoreError::DeadlineExceeded | SqliteStoreError::ReplyTimeout => {
            Some(LocalConnectedWorkspaceIndexError::DeadlineExceeded)
        }
        SqliteStoreError::MutationOutcomeUnknown => {
            Some(LocalConnectedWorkspaceIndexError::MutationOutcomeUnknown {
                phase,
                source_ordinal,
            })
        }
        _ => None,
    }
}

fn final_fence_control(
    source: ConnectedSourceSlotFinalFenceError,
) -> Option<LocalConnectedWorkspaceIndexError> {
    match source {
        ConnectedSourceSlotFinalFenceError::Selector(source) => match source {
            SourceSelectorFinalFenceError::Cancelled => {
                Some(LocalConnectedWorkspaceIndexError::Cancelled)
            }
            SourceSelectorFinalFenceError::DeadlineExceeded { .. } => {
                Some(LocalConnectedWorkspaceIndexError::DeadlineExceeded)
            }
            SourceSelectorFinalFenceError::Inspection { source } => selector_control(source),
            SourceSelectorFinalFenceError::SourceChanged => None,
        },
        ConnectedSourceSlotFinalFenceError::Source(source) => match source {
            LocalSourceSnapshotFenceError::Cancelled => {
                Some(LocalConnectedWorkspaceIndexError::Cancelled)
            }
            LocalSourceSnapshotFenceError::DeadlineExceeded => {
                Some(LocalConnectedWorkspaceIndexError::DeadlineExceeded)
            }
            _ => None,
        },
        ConnectedSourceSlotFinalFenceError::DatabaseChanged => None,
    }
}
