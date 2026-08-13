//! Versioned, deterministic inputs for a revision-pinned change receipt.
//!
//! Local adapters derive a bounded Git manifest, fence the source state, and
//! compose separately attributed indexed context when it remains current.

use std::{error::Error, fmt};

use repowitness_domain::{GitObjectId, GitStateDigest, RepositoryPath};

use crate::ContextBuildResult;

/// Version of the revision-pinned change-manifest contract.
pub const CHANGE_MANIFEST_PROFILE_VERSION: u16 = 1;

/// The categorical change observed for one exact repository path.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ChangeKind {
    /// The path exists in the compared worktree but not at the base revision.
    Added,
    /// The path exists at both source states with content changes.
    Modified,
    /// The path existed at the base revision but not in the compared worktree.
    Deleted,
    /// The path's Git file type changed.
    TypeChanged,
    /// The path is a non-ignored, untracked worktree file.
    Untracked,
}

impl ChangeKind {
    /// Returns the stable receipt label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::TypeChanged => "type_changed",
            Self::Untracked => "untracked",
        }
    }
}

/// One exact path change in canonical repository-path order.
#[derive(Clone, Eq, PartialEq)]
pub struct ChangeManifestEntry {
    path: RepositoryPath,
    kind: ChangeKind,
}

impl ChangeManifestEntry {
    /// Constructs one adapter-derived path change.
    #[must_use]
    pub const fn new(path: RepositoryPath, kind: ChangeKind) -> Self {
        Self { path, kind }
    }

    /// Returns the exact repository-relative path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the categorical change kind.
    #[must_use]
    pub const fn kind(&self) -> ChangeKind {
        self.kind
    }
}

impl fmt::Debug for ChangeManifestEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChangeManifestEntry")
            .field("path", &"<redacted-path>")
            .field("kind", &self.kind)
            .finish()
    }
}

/// Deterministic path-level difference from one exact base commit.
#[derive(Clone, Eq, PartialEq)]
pub struct ChangeManifest {
    base: GitObjectId,
    entries: Box<[ChangeManifestEntry]>,
}

impl ChangeManifest {
    /// Validates canonical path order and creates an immutable manifest.
    ///
    /// # Errors
    ///
    /// Returns [`ChangeManifestError`] when entries are unordered or contain
    /// the same exact repository path more than once.
    pub fn try_new(
        base: GitObjectId,
        entries: Vec<ChangeManifestEntry>,
    ) -> Result<Self, ChangeManifestError> {
        for pair in entries.windows(2) {
            match pair[0].path().cmp(pair[1].path()) {
                std::cmp::Ordering::Less => {}
                std::cmp::Ordering::Equal => return Err(ChangeManifestError::DuplicatePath),
                std::cmp::Ordering::Greater => {
                    return Err(ChangeManifestError::PathsNotInCanonicalOrder);
                }
            }
        }
        Ok(Self {
            base,
            entries: entries.into_boxed_slice(),
        })
    }

    /// Returns the exact base commit for this comparison.
    #[must_use]
    pub const fn base(&self) -> &GitObjectId {
        &self.base
    }

    /// Returns changes in deterministic unsigned-byte repository-path order.
    #[must_use]
    pub fn entries(&self) -> &[ChangeManifestEntry] {
        &self.entries
    }

    /// Returns the number of changed paths.
    #[must_use]
    pub fn path_count(&self) -> u64 {
        u64::try_from(self.entries.len()).unwrap_or(u64::MAX)
    }
}

impl fmt::Debug for ChangeManifest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChangeManifest")
            .field("base", &self.base)
            .field("path_count", &self.path_count())
            .finish()
    }
}

/// An adapter supplied invalid change-manifest ordering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeManifestError {
    /// Entries were not sorted by exact unsigned-byte repository path.
    PathsNotInCanonicalOrder,
    /// More than one entry named the same exact repository path.
    DuplicatePath,
}

impl fmt::Display for ChangeManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::PathsNotInCanonicalOrder => {
                "change-manifest entries are not in canonical repository-path order"
            }
            Self::DuplicatePath => "change-manifest contains a duplicate repository path",
        })
    }
}

impl Error for ChangeManifestError {}

/// The availability of separately attributed indexed context.
///
/// A current worktree can legitimately differ from the immutable source bytes
/// required to expand an indexed declaration. That absence must remain
/// categorical rather than being represented by stale declaration content.
pub enum IndexedContext<G, E> {
    /// The context was built from one immutable indexed snapshot and generation.
    Available(Box<ContextBuildResult<G, E>>),
    /// Exact indexed source expansion was unavailable for the fenced worktree.
    Unavailable {
        /// The stable, non-sensitive reason indexed context was omitted.
        reason: IndexedContextUnavailableReason,
    },
}

impl<G: fmt::Debug, E: fmt::Debug> fmt::Debug for IndexedContext<G, E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Available(context) => formatter.debug_tuple("Available").field(context).finish(),
            Self::Unavailable { reason } => formatter
                .debug_struct("Unavailable")
                .field("reason", reason)
                .finish(),
        }
    }
}

/// Stable categorical reason separately indexed context was not included.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexedContextUnavailableReason {
    /// Current source bytes did not match an indexed declaration selected by the intent.
    StaleSource,
}

/// Categorical relationship between the reviewed worktree and active index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexWorktreeAlignment {
    /// The active indexed source identity matches the reviewed worktree.
    Verified,
    /// The active indexed source identity does not match the reviewed worktree.
    Mismatch,
    /// The comparison could not be completed from the available local evidence.
    Unavailable,
}

impl IndexWorktreeAlignment {
    /// Returns the stable receipt label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Mismatch => "mismatch",
            Self::Unavailable => "unavailable",
        }
    }
}

impl IndexedContextUnavailableReason {
    /// Returns the stable receipt label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StaleSource => "stale_source",
        }
    }
}

/// A read-only change-review receipt with separately attributed indexed context.
///
/// The receipt states an exact fenced worktree Git state and an exact immutable
/// index generation. They are retained together for review, but this type does
/// not claim the index generation was built from that worktree state.
pub struct ChangeReviewReceipt<G, E> {
    worktree_git_state: GitStateDigest,
    manifest: ChangeManifest,
    indexed_context: IndexedContext<G, E>,
    index_worktree_alignment: IndexWorktreeAlignment,
}

impl<G, E> ChangeReviewReceipt<G, E> {
    /// Combines a final-fenced worktree state with independently pinned context.
    #[must_use]
    pub fn with_indexed_context(
        worktree_git_state: GitStateDigest,
        manifest: ChangeManifest,
        indexed_context: ContextBuildResult<G, E>,
        index_worktree_alignment: IndexWorktreeAlignment,
    ) -> Self {
        Self {
            worktree_git_state,
            manifest,
            indexed_context: IndexedContext::Available(Box::new(indexed_context)),
            index_worktree_alignment,
        }
    }

    /// Combines a final-fenced worktree state with an explicit unavailable context receipt.
    #[must_use]
    pub const fn without_indexed_context(
        worktree_git_state: GitStateDigest,
        manifest: ChangeManifest,
        reason: IndexedContextUnavailableReason,
        index_worktree_alignment: IndexWorktreeAlignment,
    ) -> Self {
        Self {
            worktree_git_state,
            manifest,
            indexed_context: IndexedContext::Unavailable { reason },
            index_worktree_alignment,
        }
    }

    /// Returns the opaque Git-state receipt captured before and after review work.
    #[must_use]
    pub const fn worktree_git_state(&self) -> GitStateDigest {
        self.worktree_git_state
    }

    /// Returns the exact revision-pinned worktree change manifest.
    #[must_use]
    pub const fn manifest(&self) -> &ChangeManifest {
        &self.manifest
    }

    /// Returns context pinned to its own immutable index snapshot and generation.
    #[must_use]
    pub const fn indexed_context(&self) -> &IndexedContext<G, E> {
        &self.indexed_context
    }

    /// Returns the categorical index/worktree relationship.
    #[must_use]
    pub const fn index_worktree_alignment(&self) -> IndexWorktreeAlignment {
        self.index_worktree_alignment
    }
}

impl<G: fmt::Debug, E: fmt::Debug> fmt::Debug for ChangeReviewReceipt<G, E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChangeReviewReceipt")
            .field("worktree_git_state", &self.worktree_git_state)
            .field("manifest", &self.manifest)
            .field("indexed_context", &self.indexed_context)
            .field("index_worktree_alignment", &self.index_worktree_alignment)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use repowitness_domain::{GitObjectId, RepositoryPath, RepositoryPathLimits};

    use super::{ChangeKind, ChangeManifest, ChangeManifestEntry, ChangeManifestError};

    fn base() -> GitObjectId {
        GitObjectId::try_from_hex("0123456789abcdef0123456789abcdef01234567")
            .expect("canonical base id")
    }

    fn path(value: &[u8]) -> RepositoryPath {
        RepositoryPath::try_from_bytes(value, RepositoryPathLimits::new(1024, 32))
            .expect("canonical path")
    }

    #[test]
    fn manifest_requires_unique_canonical_path_order() {
        let first = ChangeManifestEntry::new(path(b"a.rs"), ChangeKind::Added);
        let second = ChangeManifestEntry::new(path(b"b.rs"), ChangeKind::Modified);
        let manifest = ChangeManifest::try_new(base(), vec![first.clone(), second])
            .expect("ordered paths should be accepted");
        assert_eq!(manifest.path_count(), 2);
        assert_eq!(manifest.entries()[0].kind().as_str(), "added");
        assert!(matches!(
            ChangeManifest::try_new(base(), vec![first.clone(), first]),
            Err(ChangeManifestError::DuplicatePath)
        ));
        assert!(matches!(
            ChangeManifest::try_new(
                base(),
                vec![
                    ChangeManifestEntry::new(path(b"b.rs"), ChangeKind::Added),
                    ChangeManifestEntry::new(path(b"a.rs"), ChangeKind::Added),
                ],
            ),
            Err(ChangeManifestError::PathsNotInCanonicalOrder)
        ));
    }
}
