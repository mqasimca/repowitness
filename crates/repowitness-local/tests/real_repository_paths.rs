//! Opt-in validation of the production Git path-discovery adapter.
//!
//! Run with:
//!
//! ```text
//! REPOWITNESS_REAL_REPOSITORY=/path/to/repository \
//!   cargo test -p repowitness-local --test real_repository_paths \
//!   -- --ignored --exact validates_all_discovered_git_paths
//! ```

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use repowitness_application::{RepositoryPathTextByteLimit, RepositoryPathTextV1};
use repowitness_local::{GitPathDiscoveryLimits, discover_repository_paths};

#[test]
#[ignore = "requires REPOWITNESS_REAL_REPOSITORY and Git"]
fn validates_all_discovered_git_paths() {
    let configured_root = std::env::var_os("REPOWITNESS_REAL_REPOSITORY")
        .expect("REPOWITNESS_REAL_REPOSITORY must identify a Git worktree");
    let root = resolve_repository_root(&configured_root);
    assert!(
        root.is_dir(),
        "REPOWITNESS_REAL_REPOSITORY must identify a directory"
    );

    let discovery_limits = GitPathDiscoveryLimits::default();
    let discovered = discover_repository_paths(&root, discovery_limits)
        .expect("bounded Git repository-path discovery must succeed");
    let stats = discovered.stats();
    assert!(stats.path_count() > 0, "the repository must contain paths");

    let text_limit = RepositoryPathTextByteLimit::new(
        discovery_limits
            .repository_path()
            .max_bytes()
            .get()
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(64))
            .expect("the default text limit must fit u64"),
    );
    let mut total_encoded_path_bytes = 0_u64;
    let mut longest_encoded_path_bytes = 0_u64;
    for path in discovered.paths() {
        let encoded = RepositoryPathTextV1::encode(path, text_limit)
            .expect("each discovered path must encode");
        let decoded = RepositoryPathTextV1::decode(
            encoded.as_str(),
            text_limit,
            discovery_limits.repository_path(),
        )
        .expect("each encoded path must decode");
        assert_eq!(&decoded, path, "text encoding must preserve exact bytes");
        total_encoded_path_bytes = total_encoded_path_bytes
            .checked_add(encoded.encoded_byte_count().get())
            .expect("total encoded path bytes must fit u64");
        longest_encoded_path_bytes =
            longest_encoded_path_bytes.max(encoded.encoded_byte_count().get());
    }

    println!(
        "validated {} repository paths ({} raw bytes; {} encoded bytes; longest path {} raw / {} encoded bytes; most components {})",
        stats.path_count(),
        stats.total_path_bytes(),
        total_encoded_path_bytes,
        stats.longest_path_bytes(),
        longest_encoded_path_bytes,
        stats.most_components()
    );
}

fn resolve_repository_root(configured_root: &OsStr) -> PathBuf {
    let configured_root = Path::new(configured_root);
    if configured_root.is_absolute() {
        return configured_root.to_owned();
    }

    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(configured_root)
}

#[test]
fn resolves_relative_repository_paths_from_the_workspace_root() {
    assert_eq!(
        resolve_repository_root(OsStr::new("../sibling")),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sibling")
    );
    let absolute = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert_eq!(resolve_repository_root(absolute.as_os_str()), absolute);
}
