//! Canonical, bounded source-manifest contracts.

use core::{cmp::Ordering, fmt};

/// The semantic version of the source-snapshot domain contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSnapshotVersion(u16);

impl SourceSnapshotVersion {
    /// The initial source-snapshot contract.
    pub const V1: Self = Self(1);

    /// Returns the fixed-width version number.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// The semantic version of the source-manifest domain contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceManifestVersion(u16);

impl SourceManifestVersion {
    /// The initial source-manifest contract.
    pub const V1: Self = Self(1);

    /// Returns the fixed-width version number.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Stable source-file category recorded in a canonical manifest.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SourceFileKind {
    /// A regular file whose opened bytes can be analyzed.
    Regular,
    /// A symbolic link, disabled by the default Phase 0 source policy.
    SymbolicLink,
    /// A Gitlink naming another repository.
    Gitlink,
    /// Another filesystem object that is not source-readable.
    Other,
}

impl SourceFileKind {
    /// Returns the stable canonical tag used by manifest hashing and storage.
    #[must_use]
    pub const fn canonical_tag(self) -> u8 {
        match self {
            Self::Regular => 1,
            Self::SymbolicLink => 2,
            Self::Gitlink => 3,
            Self::Other => 4,
        }
    }
}

/// A fixed-width number of files in a source manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceFileCount(u64);

impl SourceFileCount {
    /// No source files.
    pub const ZERO: Self = Self(0);

    /// Returns the fixed-width file count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A fixed-width inclusive file-count bound for a source manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceFileLimit(u64);

impl SourceFileLimit {
    /// Creates an inclusive file-count bound.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the fixed-width bound.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Failure to construct a canonical source manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceManifestError {
    /// The platform collection length cannot be represented as a `u64`.
    CountNotRepresentable,
    /// The input contains more files than its declared bound.
    LimitExceeded {
        /// The input's actual file count.
        actual: SourceFileCount,
        /// The inclusive file-count bound.
        limit: SourceFileLimit,
    },
    /// Two entries have the same validated normalized path.
    DuplicateNormalizedPath,
}

impl fmt::Display for SourceManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CountNotRepresentable => {
                formatter.write_str("source file count cannot be represented as a u64")
            }
            Self::LimitExceeded { actual, limit } => write!(
                formatter,
                "source file count {} exceeds limit {}",
                actual.get(),
                limit.get()
            ),
            Self::DuplicateNormalizedPath => {
                formatter.write_str("source manifest contains a duplicate normalized path")
            }
        }
    }
}

impl std::error::Error for SourceManifestError {}

/// One exact file entry in a source manifest.
///
/// `P` is a validated normalized repository-relative path, `K` a validated
/// file type, and `D` the digest of the file's exact content. The concrete
/// component encodings remain independent of this structure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceManifestEntry<P, K, D> {
    path: P,
    file_type: K,
    content_digest: D,
}

impl<P, K, D> SourceManifestEntry<P, K, D> {
    /// Creates an entry from already-validated components.
    #[must_use]
    pub const fn new(path: P, file_type: K, content_digest: D) -> Self {
        Self {
            path,
            file_type,
            content_digest,
        }
    }

    /// Returns the normalized repository-relative path.
    #[must_use]
    pub const fn path(&self) -> &P {
        &self.path
    }

    /// Returns the validated file type.
    #[must_use]
    pub const fn file_type(&self) -> &K {
        &self.file_type
    }

    /// Returns the digest of the file's exact content.
    #[must_use]
    pub const fn content_digest(&self) -> &D {
        &self.content_digest
    }
}

/// A canonical path-sorted, duplicate-free, bounded source manifest.
///
/// The normalized path type defines canonical ordering through [`Ord`].
/// Component types must enforce their own size limits. Construction validates
/// the count before sorting, sorts the supplied allocation in place, rejects
/// duplicate paths, and stores the result as a boxed slice without unused
/// `Vec` capacity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceManifest<P, K, D> {
    entries: Box<[SourceManifestEntry<P, K, D>]>,
    count: SourceFileCount,
    limit: SourceFileLimit,
}

/// Exact non-file inputs that contribute to one source snapshot.
///
/// `R` is a repository identity, `G` a complete Git identity including object
/// format and optional `HEAD`, `W` the worktree and relevant submodule state,
/// `C` the resolved configuration and policy identity, and `A` the complete
/// analyzer, grammar, producer, and schema manifest. Each component is
/// validated and bounded before construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSnapshotMetadata<R, G, W, C, A> {
    repository: R,
    git_identity: G,
    worktree_identity: W,
    configuration_identity: C,
    producer_manifest: A,
}

impl<R, G, W, C, A> SourceSnapshotMetadata<R, G, W, C, A> {
    /// Creates metadata from already-validated snapshot components.
    #[must_use]
    pub const fn new(
        repository: R,
        git_identity: G,
        worktree_identity: W,
        configuration_identity: C,
        producer_manifest: A,
    ) -> Self {
        Self {
            repository,
            git_identity,
            worktree_identity,
            configuration_identity,
            producer_manifest,
        }
    }

    /// Returns the repository identity.
    #[must_use]
    pub const fn repository(&self) -> &R {
        &self.repository
    }

    /// Returns the complete Git identity.
    #[must_use]
    pub const fn git_identity(&self) -> &G {
        &self.git_identity
    }

    /// Returns the worktree and relevant submodule identity.
    #[must_use]
    pub const fn worktree_identity(&self) -> &W {
        &self.worktree_identity
    }

    /// Returns the resolved configuration and policy identity.
    #[must_use]
    pub const fn configuration_identity(&self) -> &C {
        &self.configuration_identity
    }

    /// Returns the analyzer, grammar, producer, and schema manifest.
    #[must_use]
    pub const fn producer_manifest(&self) -> &A {
        &self.producer_manifest
    }
}

/// One exact source snapshot consumed by analysis.
///
/// The snapshot combines every required non-file input with one canonical,
/// bounded file manifest. It does not claim filesystem-wide atomic capture;
/// adapters describe that limitation in the validated worktree identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSnapshot<R, G, W, C, A, P, K, D> {
    metadata: SourceSnapshotMetadata<R, G, W, C, A>,
    manifest: SourceManifest<P, K, D>,
}

impl<R, G, W, C, A, P, K, D> SourceSnapshot<R, G, W, C, A, P, K, D> {
    /// The semantic version implemented by this snapshot.
    pub const VERSION: SourceSnapshotVersion = SourceSnapshotVersion::V1;

    /// Creates a snapshot from validated metadata and a canonical manifest.
    #[must_use]
    pub const fn new(
        metadata: SourceSnapshotMetadata<R, G, W, C, A>,
        manifest: SourceManifest<P, K, D>,
    ) -> Self {
        Self { metadata, manifest }
    }

    /// Returns the semantic version implemented by this snapshot.
    #[must_use]
    pub const fn version(&self) -> SourceSnapshotVersion {
        Self::VERSION
    }

    /// Returns the validated non-file snapshot metadata.
    #[must_use]
    pub const fn metadata(&self) -> &SourceSnapshotMetadata<R, G, W, C, A> {
        &self.metadata
    }

    /// Returns the canonical bounded file manifest.
    #[must_use]
    pub const fn manifest(&self) -> &SourceManifest<P, K, D> {
        &self.manifest
    }
}

impl<P, K, D> SourceManifest<P, K, D> {
    /// The semantic version implemented by this manifest.
    pub const VERSION: SourceManifestVersion = SourceManifestVersion::V1;

    /// Returns the semantic version implemented by this manifest.
    #[must_use]
    pub const fn version(&self) -> SourceManifestVersion {
        Self::VERSION
    }

    /// Returns the entries in canonical normalized-path order.
    #[must_use]
    pub fn as_slice(&self) -> &[SourceManifestEntry<P, K, D>] {
        &self.entries
    }

    /// Returns the fixed-width file count.
    #[must_use]
    pub const fn count(&self) -> SourceFileCount {
        self.count
    }

    /// Returns the inclusive count bound enforced during construction.
    #[must_use]
    pub const fn limit(&self) -> SourceFileLimit {
        self.limit
    }

    /// Returns whether the manifest contains no files.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count.get() == 0
    }

    /// Consumes the manifest and returns its canonically ordered entries.
    #[must_use]
    pub fn into_vec(self) -> Vec<SourceManifestEntry<P, K, D>> {
        self.entries.into_vec()
    }
}

impl<P: Ord, K, D> SourceManifest<P, K, D> {
    /// Canonicalizes and validates an owned source-manifest collection.
    ///
    /// Work is bounded by `limit`, takes `O(n log n)` comparisons, performs no
    /// I/O, and may shrink excess `Vec` capacity while converting the entries
    /// to a boxed slice.
    ///
    /// # Errors
    ///
    /// Returns [`SourceManifestError::CountNotRepresentable`] when the
    /// collection length cannot fit in the fixed-width count,
    /// [`SourceManifestError::LimitExceeded`] when the input exceeds `limit`,
    /// or [`SourceManifestError::DuplicateNormalizedPath`] when two validated
    /// paths compare equal.
    pub fn try_from_vec(
        mut entries: Vec<SourceManifestEntry<P, K, D>>,
        limit: SourceFileLimit,
    ) -> Result<Self, SourceManifestError> {
        let count = u64::try_from(entries.len())
            .map(SourceFileCount)
            .map_err(|_| SourceManifestError::CountNotRepresentable)?;

        if count.get() > limit.get() {
            return Err(SourceManifestError::LimitExceeded {
                actual: count,
                limit,
            });
        }

        entries.sort_unstable_by(|left, right| left.path.cmp(&right.path));

        if entries
            .windows(2)
            .any(|pair| pair[0].path.cmp(&pair[1].path) == Ordering::Equal)
        {
            return Err(SourceManifestError::DuplicateNormalizedPath);
        }

        Ok(Self {
            entries: entries.into_boxed_slice(),
            count,
            limit,
        })
    }
}

#[cfg(test)]
mod tests {
    use core::cmp::Ordering;

    use super::{
        SourceFileCount, SourceFileKind, SourceFileLimit, SourceManifest, SourceManifestEntry,
        SourceManifestError, SourceManifestVersion, SourceSnapshot, SourceSnapshotMetadata,
        SourceSnapshotVersion,
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct ComparisonMustNotRun;

    impl Ord for ComparisonMustNotRun {
        fn cmp(&self, _other: &Self) -> Ordering {
            panic!("over-limit input must be rejected before path comparison")
        }
    }

    impl PartialOrd for ComparisonMustNotRun {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    fn entry(
        path: &'static str,
        file_type: &'static str,
        digest: &'static str,
    ) -> SourceManifestEntry<&'static str, &'static str, &'static str> {
        SourceManifestEntry::new(path, file_type, digest)
    }

    #[test]
    fn source_file_kind_tags_are_stable_and_distinct() {
        assert_eq!(SourceFileKind::Regular.canonical_tag(), 1);
        assert_eq!(SourceFileKind::SymbolicLink.canonical_tag(), 2);
        assert_eq!(SourceFileKind::Gitlink.canonical_tag(), 3);
        assert_eq!(SourceFileKind::Other.canonical_tag(), 4);
    }

    fn snapshot(
        repository: &'static str,
        git_identity: &'static str,
        worktree_identity: &'static str,
        configuration_identity: &'static str,
        producer_manifest: &'static str,
        content_digest: &'static str,
    ) -> SourceSnapshot<
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
    > {
        let metadata = SourceSnapshotMetadata::new(
            repository,
            git_identity,
            worktree_identity,
            configuration_identity,
            producer_manifest,
        );
        let manifest = SourceManifest::try_from_vec(
            vec![entry("src/lib.rs", "regular", content_digest)],
            SourceFileLimit::new(1),
        )
        .expect("one unique file fits the snapshot manifest");

        SourceSnapshot::new(metadata, manifest)
    }

    #[test]
    fn manifest_sorts_entries_by_validated_normalized_path() {
        let manifest = SourceManifest::try_from_vec(
            vec![
                entry("src/z.rs", "regular", "digest:z"),
                entry("README.md", "regular", "digest:readme"),
                entry("src/a.rs", "regular", "digest:a"),
            ],
            SourceFileLimit::new(3),
        )
        .expect("unique paths within the bound form a valid manifest");
        let same_manifest = SourceManifest::try_from_vec(
            vec![
                entry("src/a.rs", "regular", "digest:a"),
                entry("src/z.rs", "regular", "digest:z"),
                entry("README.md", "regular", "digest:readme"),
            ],
            SourceFileLimit::new(3),
        )
        .expect("another input order must form the same canonical manifest");

        let paths: Vec<_> = manifest
            .as_slice()
            .iter()
            .map(SourceManifestEntry::path)
            .copied()
            .collect();

        assert_eq!(paths, ["README.md", "src/a.rs", "src/z.rs"]);
        assert_eq!(manifest.count().get(), 3);
        assert_eq!(manifest.limit().get(), 3);
        assert!(!manifest.is_empty());
        assert_eq!(
            SourceManifest::<&str, &str, &str>::VERSION,
            SourceManifestVersion::V1
        );
        assert_eq!(manifest.version(), SourceManifestVersion::V1);
        assert_eq!(SourceManifestVersion::V1.get(), 1);
        assert_eq!(manifest, same_manifest);
    }

    #[test]
    fn manifest_preserves_file_type_and_digest_with_each_sorted_path() {
        let manifest = SourceManifest::try_from_vec(
            vec![
                entry("src/z.rs", "generated", "digest:z"),
                entry("src/a.rs", "regular", "digest:a"),
            ],
            SourceFileLimit::new(2),
        )
        .expect("unique entries form a valid manifest");

        let entries = manifest.as_slice();
        assert_eq!(*entries[0].path(), "src/a.rs");
        assert_eq!(*entries[0].file_type(), "regular");
        assert_eq!(*entries[0].content_digest(), "digest:a");
        assert_eq!(*entries[1].path(), "src/z.rs");
        assert_eq!(*entries[1].file_type(), "generated");
        assert_eq!(*entries[1].content_digest(), "digest:z");
    }

    #[test]
    fn manifest_rejects_duplicate_normalized_paths() {
        let error = SourceManifest::try_from_vec(
            vec![
                entry("src/lib.rs", "regular", "digest:first"),
                entry("src/lib.rs", "generated", "digest:second"),
            ],
            SourceFileLimit::new(2),
        )
        .expect_err("one normalized path cannot identify two entries");

        assert_eq!(error, SourceManifestError::DuplicateNormalizedPath);
        assert_eq!(
            error.to_string(),
            "source manifest contains a duplicate normalized path"
        );
    }

    #[test]
    fn unrepresentable_manifest_count_has_a_stable_diagnostic() {
        assert_eq!(
            SourceManifestError::CountNotRepresentable.to_string(),
            "source file count cannot be represented as a u64"
        );
    }

    #[test]
    fn manifest_rejects_input_over_its_file_limit() {
        let error = SourceManifest::try_from_vec(
            vec![
                entry("src/a.rs", "regular", "digest:a"),
                entry("src/b.rs", "regular", "digest:b"),
            ],
            SourceFileLimit::new(1),
        )
        .expect_err("two entries must exceed a one-file limit");

        assert_eq!(
            error,
            SourceManifestError::LimitExceeded {
                actual: SourceFileCount(2),
                limit: SourceFileLimit::new(1),
            }
        );
        assert_eq!(error.to_string(), "source file count 2 exceeds limit 1");
    }

    #[test]
    fn manifest_checks_the_file_limit_before_sorting() {
        let entries = vec![
            SourceManifestEntry::new(ComparisonMustNotRun, (), ()),
            SourceManifestEntry::new(ComparisonMustNotRun, (), ()),
        ];

        let error = SourceManifest::try_from_vec(entries, SourceFileLimit::new(1))
            .expect_err("over-limit input must be rejected without sorting");

        assert!(matches!(error, SourceManifestError::LimitExceeded { .. }));
    }

    #[test]
    fn zero_limit_accepts_only_an_empty_manifest() {
        let manifest =
            SourceManifest::<&str, &str, &str>::try_from_vec(Vec::new(), SourceFileLimit::new(0))
                .expect("an empty manifest fits a zero-file limit");

        assert!(manifest.is_empty());
        assert_eq!(manifest.count(), SourceFileCount::ZERO);
        assert!(manifest.as_slice().is_empty());
        assert!(manifest.into_vec().is_empty());
    }

    #[test]
    fn source_snapshot_preserves_every_exact_input() {
        let snapshot = snapshot(
            "repository:1",
            "git:sha1:head",
            "worktree:dirty:submodules",
            "configuration:digest",
            "producers:digest",
            "content:digest",
        );

        assert_eq!(
            SourceSnapshot::<&str, &str, &str, &str, &str, &str, &str, &str>::VERSION,
            SourceSnapshotVersion::V1
        );
        assert_eq!(snapshot.version(), SourceSnapshotVersion::V1);
        assert_eq!(SourceSnapshotVersion::V1.get(), 1);
        assert_eq!(*snapshot.metadata().repository(), "repository:1");
        assert_eq!(*snapshot.metadata().git_identity(), "git:sha1:head");
        assert_eq!(
            *snapshot.metadata().worktree_identity(),
            "worktree:dirty:submodules"
        );
        assert_eq!(
            *snapshot.metadata().configuration_identity(),
            "configuration:digest"
        );
        assert_eq!(*snapshot.metadata().producer_manifest(), "producers:digest");
        assert_eq!(snapshot.manifest().count().get(), 1);
        assert_eq!(
            *snapshot.manifest().as_slice()[0].content_digest(),
            "content:digest"
        );
    }

    #[test]
    fn every_exact_input_participates_in_snapshot_equality() {
        let baseline = snapshot(
            "repository:1",
            "git:sha1:head",
            "worktree:clean",
            "configuration:digest",
            "producers:digest",
            "content:digest",
        );

        assert_ne!(
            baseline,
            snapshot(
                "repository:changed",
                "git:sha1:head",
                "worktree:clean",
                "configuration:digest",
                "producers:digest",
                "content:digest",
            )
        );
        assert_ne!(
            baseline,
            snapshot(
                "repository:1",
                "git:changed",
                "worktree:clean",
                "configuration:digest",
                "producers:digest",
                "content:digest",
            )
        );
        assert_ne!(
            baseline,
            snapshot(
                "repository:1",
                "git:sha1:head",
                "worktree:changed",
                "configuration:digest",
                "producers:digest",
                "content:digest",
            )
        );
        assert_ne!(
            baseline,
            snapshot(
                "repository:1",
                "git:sha1:head",
                "worktree:clean",
                "configuration:changed",
                "producers:digest",
                "content:digest",
            )
        );
        assert_ne!(
            baseline,
            snapshot(
                "repository:1",
                "git:sha1:head",
                "worktree:clean",
                "configuration:digest",
                "producers:changed",
                "content:digest",
            )
        );
        assert_ne!(
            baseline,
            snapshot(
                "repository:1",
                "git:sha1:head",
                "worktree:clean",
                "configuration:digest",
                "producers:digest",
                "content:changed",
            )
        );
    }
}
