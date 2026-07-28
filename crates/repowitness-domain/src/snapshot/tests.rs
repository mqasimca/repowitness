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
