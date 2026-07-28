use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use super::{
    DiscoveredRepositoryPaths, GitPathDiscoveryLimits, GitPathDiscoveryScope,
    capture_git_output_from_command, discover_repository_paths, discovered_worktree_root,
    null_device, parse_git_paths, sanitized_git_command,
};

static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

struct TempRepository {
    base: PathBuf,
    root: PathBuf,
    auxiliary: PathBuf,
}

impl TempRepository {
    fn new(object_format: Option<&str>) -> Self {
        let fixture_id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!(
            "repowitness-gix-spike-{}-{fixture_id}",
            std::process::id()
        ));
        let root = base.join("worktree");
        let auxiliary = base.join("auxiliary");
        fs::create_dir_all(&root).expect("fixture worktree must be created");
        fs::create_dir_all(&auxiliary).expect("fixture auxiliary directory must be created");

        let repository = Self {
            base,
            root,
            auxiliary,
        };
        let mut arguments = vec![
            OsString::from("init"),
            OsString::from("--quiet"),
            OsString::from("--initial-branch=main"),
        ];
        if let Some(object_format) = object_format {
            arguments.push(OsString::from(format!("--object-format={object_format}")));
        }
        repository.git(&arguments);
        repository
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn auxiliary(&self) -> &Path {
        &self.auxiliary
    }

    fn write(&self, relative: impl AsRef<Path>, contents: &[u8]) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent directory must be created");
        }
        fs::write(path, contents).expect("fixture file must be written");
    }

    fn git_text(&self, arguments: &[&str]) {
        let arguments = arguments.iter().map(OsString::from).collect::<Vec<_>>();
        self.git(&arguments);
    }

    fn git(&self, arguments: &[OsString]) {
        let status = self.git_status(arguments);
        assert!(status.success(), "fixture Git command failed with {status}");
    }

    fn git_status(&self, arguments: &[OsString]) -> ExitStatus {
        fixture_git_command(&self.root)
            .args(arguments)
            .status()
            .expect("fixture Git command must start")
    }

    fn git_output_text(&self, arguments: &[&str]) -> String {
        let output = fixture_git_command(&self.root)
            .args(arguments)
            .stdout(Stdio::piped())
            .output()
            .expect("fixture Git command must start");
        assert!(
            output.status.success(),
            "fixture Git command failed with {}",
            output.status
        );
        String::from_utf8(output.stdout)
            .expect("fixture Git output must be UTF-8")
            .trim()
            .to_owned()
    }

    fn commit(&self, message: &str) {
        self.git_text(&["commit", "--quiet", "-m", message]);
    }
}

impl Drop for TempRepository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn fixture_git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .arg("--literal-pathspecs")
        .arg("-c")
        .arg("user.name=RepoWitness Test")
        .arg("-c")
        .arg("user.email=repowitness@example.invalid")
        .arg("-c")
        .arg(format!("core.hooksPath={}", null_device()))
        .arg("-C")
        .arg(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_NAMESPACE")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_EXTERNAL_DIFF")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[derive(Debug, Eq, PartialEq)]
struct GixIndexSnapshot {
    paths: Vec<Vec<u8>>,
    raw_entry_count: u64,
    sparse_entry_count: u64,
    submodule_entry_count: u64,
}

fn gix_index_snapshot(root: &Path) -> GixIndexSnapshot {
    let repository = open_gix_repository(root);
    gix_index_snapshot_from_repository(&repository)
}

fn open_gix_repository(root: &Path) -> gix::Repository {
    gix::open_opts(
        root.to_owned(),
        gix::open::Options::isolated().bail_if_untrusted(true),
    )
    .expect("gix must open the fixture through isolated permissions")
}

fn gix_index_snapshot_from_repository(repository: &gix::Repository) -> GixIndexSnapshot {
    let index = repository
        .index_or_empty()
        .expect("gix must read the fixture index");
    let mut paths = Vec::with_capacity(index.entries().len());
    let mut sparse_entry_count = 0_u64;
    let mut submodule_entry_count = 0_u64;
    for entry in index.entries() {
        paths.push(entry.path(&index).to_vec());
        if entry.mode.is_sparse() {
            sparse_entry_count += 1;
        }
        if entry.mode.is_submodule() {
            submodule_entry_count += 1;
        }
    }
    let raw_entry_count =
        u64::try_from(paths.len()).expect("fixture index entry count must fit u64");
    paths.sort_unstable();
    paths.dedup();
    GixIndexSnapshot {
        paths,
        raw_entry_count,
        sparse_entry_count,
        submodule_entry_count,
    }
}

fn git_cli_cached_paths(root: &Path) -> DiscoveredRepositoryPaths {
    let limits = GitPathDiscoveryLimits::default();
    let deadline = Instant::now()
        .checked_add(limits.deadline())
        .expect("fixture deadline must be representable");
    let mut is_cancelled = || false;
    let worktree_root =
        discovered_worktree_root(root).expect("fixture worktree root must resolve safely");
    let output = capture_git_output_from_command(
        sanitized_git_command(&worktree_root, GitPathDiscoveryScope::Cached),
        limits,
        deadline,
        &mut is_cancelled,
    )
    .expect("sanitized Git cached-path discovery must succeed");
    parse_git_paths(output, limits).expect("sanitized Git cached paths must validate")
}

fn owned_paths(discovered: &DiscoveredRepositoryPaths) -> Vec<Vec<u8>> {
    discovered
        .paths()
        .iter()
        .map(|path| path.as_bytes().to_vec())
        .collect()
}

#[test]
#[ignore = "requires REPOWITNESS_REAL_REPOSITORY, Git, and a non-sparse worktree index"]
fn gix_and_sanitized_git_agree_on_real_repository_cached_paths() {
    let configured_root = std::env::var_os("REPOWITNESS_REAL_REPOSITORY")
        .expect("REPOWITNESS_REAL_REPOSITORY must identify a Git worktree");
    let configured_root = Path::new(&configured_root);
    let root = if configured_root.is_absolute() {
        configured_root.to_owned()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(configured_root)
    };
    let gix = gix_index_snapshot(&root);
    assert_eq!(
        gix.sparse_entry_count, 0,
        "the real-repository comparison requires a non-sparse index"
    );
    let cached = owned_paths(&git_cli_cached_paths(&root));
    assert_eq!(gix.paths, cached);
    println!(
        "gix and sanitized Git agreed on {} cached repository paths",
        cached.len()
    );
}

#[test]
fn gix_and_sanitized_git_agree_on_tracked_paths_and_untracked_scope_is_explicit() {
    let repository = TempRepository::new(None);
    repository.write(".gitignore", b"ignored.rs\n");
    repository.write("tracked.rs", b"fn tracked() {}\n");
    repository.write("untracked.rs", b"fn untracked() {}\n");
    repository.write("ignored.rs", b"fn ignored() {}\n");
    repository.git_text(&["add", "--", ".gitignore", "tracked.rs"]);

    let gix = gix_index_snapshot(repository.root());
    let cached = owned_paths(&git_cli_cached_paths(repository.root()));
    assert_eq!(gix.paths, cached);
    assert_eq!(cached, [b".gitignore".to_vec(), b"tracked.rs".to_vec()]);

    let all = discover_repository_paths(repository.root(), GitPathDiscoveryLimits::default())
        .expect("production-shaped discovery must succeed");
    assert_eq!(
        owned_paths(&all),
        [
            b".gitignore".to_vec(),
            b"tracked.rs".to_vec(),
            b"untracked.rs".to_vec()
        ]
    );
}

#[test]
fn sanitized_git_pins_the_worktree_against_hostile_repository_config() {
    let repository = TempRepository::new(None);
    repository.write("tracked.rs", b"fn tracked() {}\n");
    repository.write("actual-untracked.rs", b"fn actual() {}\n");
    repository.git_text(&["add", "--", "tracked.rs"]);
    fs::write(
        repository.auxiliary().join("outside-private-name.rs"),
        b"fn outside() {}\n",
    )
    .expect("outside fixture file must be written");
    repository.git(&[
        OsString::from("config"),
        OsString::from("core.worktree"),
        repository.auxiliary().as_os_str().to_owned(),
    ]);

    let discovered =
        discover_repository_paths(repository.root(), GitPathDiscoveryLimits::default())
            .expect("sanitized discovery must remain pinned to the requested worktree");

    assert_eq!(
        owned_paths(&discovered),
        [b"actual-untracked.rs".to_vec(), b"tracked.rs".to_vec()]
    );
}

#[test]
fn sanitized_git_ignores_an_external_excludes_file_from_local_config() {
    let repository = TempRepository::new(None);
    repository.write("actual-untracked.rs", b"fn actual() {}\n");
    let external_excludes = repository.auxiliary().join("external-excludes");
    fs::write(&external_excludes, b"actual-untracked.rs\n")
        .expect("external excludes fixture must be written");
    repository.git(&[
        OsString::from("config"),
        OsString::from("core.excludesFile"),
        external_excludes.into_os_string(),
    ]);
    assert_eq!(
        repository.git_output_text(&["ls-files", "--others", "--exclude-standard"]),
        ""
    );

    let discovered =
        discover_repository_paths(repository.root(), GitPathDiscoveryLimits::default())
            .expect("sanitized discovery must ignore external excludes configuration");

    assert_eq!(owned_paths(&discovered), [b"actual-untracked.rs".to_vec()]);
}

#[test]
fn nested_input_discovers_the_complete_worktree_with_root_relative_paths() {
    let repository = TempRepository::new(None);
    repository.write("top-level.rs", b"fn top_level() {}\n");
    repository.write("nested/inside.rs", b"fn inside() {}\n");
    repository.git_text(&["add", "--", "top-level.rs"]);

    let discovered = discover_repository_paths(
        &repository.root().join("nested"),
        GitPathDiscoveryLimits::default(),
    )
    .expect("a nested input must resolve the complete containing worktree");

    assert_eq!(
        owned_paths(&discovered),
        [b"nested/inside.rs".to_vec(), b"top-level.rs".to_vec()]
    );
}

#[test]
fn non_worktree_input_fails_closed_before_git_runs() {
    let repository = TempRepository::new(None);

    let error =
        discover_repository_paths(repository.auxiliary(), GitPathDiscoveryLimits::default())
            .expect_err("a directory outside every worktree must fail closed");

    assert!(matches!(
        error,
        super::GitPathDiscoveryError::WorktreeMarkerNotFound
    ));
}

#[cfg(unix)]
#[test]
fn symlinked_worktree_marker_is_rejected_before_git_runs() {
    use std::os::unix::fs::symlink;

    let repository = TempRepository::new(None);
    repository.write("outside-private-name.rs", b"fn outside() {}\n");
    repository.git_text(&["add", "--", "outside-private-name.rs"]);
    let requested_root = repository.auxiliary().join("requested");
    fs::create_dir_all(&requested_root).expect("requested fixture directory must be created");
    symlink(repository.root().join(".git"), requested_root.join(".git"))
        .expect("redirecting worktree marker symlink must be created");

    let error = discover_repository_paths(&requested_root, GitPathDiscoveryLimits::default())
        .expect_err("a symlinked worktree marker must fail before Git can read its index");

    assert!(matches!(
        error,
        super::GitPathDiscoveryError::WorktreeMarkerUnsupported
    ));
}

#[cfg(unix)]
#[test]
fn special_file_worktree_marker_is_rejected_before_git_runs() {
    use std::os::unix::net::UnixListener;

    let repository = TempRepository::new(None);
    let requested_root = repository.auxiliary();
    let _listener = UnixListener::bind(requested_root.join(".git"))
        .expect("special worktree marker fixture must be created");

    let error = discover_repository_paths(requested_root, GitPathDiscoveryLimits::default())
        .expect_err("a special-file worktree marker must fail before Git runs");

    assert!(matches!(
        error,
        super::GitPathDiscoveryError::WorktreeMarkerUnsupported
    ));
}

#[cfg(unix)]
#[cfg_attr(
    target_vendor = "apple",
    ignore = "Apple filesystems reject the byte-invalid fixture name"
)]
#[test]
fn gix_and_sanitized_git_preserve_non_utf8_index_paths_exactly() {
    use std::os::unix::ffi::OsStringExt;

    let repository = TempRepository::new(None);
    let raw_name = b"non-utf8-\xFF.rs".to_vec();
    let file_name = OsString::from_vec(raw_name.clone());
    repository.write(PathBuf::from(&file_name), b"fn bytes() {}\n");
    repository.git(&[OsString::from("add"), OsString::from("--"), file_name]);

    let gix = gix_index_snapshot(repository.root());
    let cached = owned_paths(&git_cli_cached_paths(repository.root()));
    assert_eq!(gix.paths, vec![raw_name.clone()]);
    assert_eq!(cached, vec![raw_name]);
}

#[test]
fn gix_and_sanitized_git_support_sha256_indexes_when_explicitly_enabled() {
    let repository = TempRepository::new(Some("sha256"));
    repository.write("sha256.rs", b"fn sha256() {}\n");
    repository.git_text(&["add", "--", "sha256.rs"]);

    let gix = gix_index_snapshot(repository.root());
    let cached = owned_paths(&git_cli_cached_paths(repository.root()));
    assert_eq!(gix.paths, vec![b"sha256.rs".to_vec()]);
    assert_eq!(cached, gix.paths);
}

#[test]
fn conflicted_index_stages_are_deduplicated_into_one_repository_identity() {
    let repository = TempRepository::new(None);
    repository.write("conflict.rs", b"base\n");
    repository.git_text(&["add", "--", "conflict.rs"]);
    repository.commit("base");

    repository.git_text(&["checkout", "--quiet", "-b", "other"]);
    repository.write("conflict.rs", b"other\n");
    repository.git_text(&["add", "--", "conflict.rs"]);
    repository.commit("other");

    repository.git_text(&["checkout", "--quiet", "main"]);
    repository.write("conflict.rs", b"main\n");
    repository.git_text(&["add", "--", "conflict.rs"]);
    repository.commit("main");
    let merge_status = repository.git_status(&[
        OsString::from("merge"),
        OsString::from("--no-edit"),
        OsString::from("other"),
    ]);
    assert!(!merge_status.success(), "fixture merge must conflict");

    let gix = gix_index_snapshot(repository.root());
    assert_eq!(gix.raw_entry_count, 3);
    assert_eq!(gix.paths, vec![b"conflict.rs".to_vec()]);
    assert_eq!(
        owned_paths(&git_cli_cached_paths(repository.root())),
        gix.paths
    );
}

#[test]
fn gitlink_paths_agree_but_are_explicitly_not_regular_source_files() {
    let repository = TempRepository::new(None);
    repository.write("base.rs", b"fn base() {}\n");
    repository.git_text(&["add", "--", "base.rs"]);
    repository.commit("base");
    let commit_id = repository.git_output_text(&["rev-parse", "HEAD"]);
    repository.git_text(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{commit_id},vendor/dependency"),
    ]);

    let gix = gix_index_snapshot(repository.root());
    let cached = owned_paths(&git_cli_cached_paths(repository.root()));
    assert_eq!(gix.paths, cached);
    assert_eq!(gix.submodule_entry_count, 1);
    assert_eq!(cached, [b"base.rs".to_vec(), b"vendor/dependency".to_vec()]);
}

#[test]
fn gix_and_sanitized_git_agree_in_a_linked_worktree() {
    let repository = TempRepository::new(None);
    repository.write("linked.rs", b"fn linked() {}\n");
    repository.git_text(&["add", "--", "linked.rs"]);
    repository.commit("base");
    let linked_worktree = repository.auxiliary().join("linked-worktree");
    repository.git(&[
        OsString::from("worktree"),
        OsString::from("add"),
        OsString::from("--quiet"),
        OsString::from("-b"),
        OsString::from("linked"),
        linked_worktree.as_os_str().to_owned(),
    ]);

    let gix = gix_index_snapshot(&linked_worktree);
    let cached = owned_paths(&git_cli_cached_paths(&linked_worktree));
    assert_eq!(gix.paths, cached);
    assert_eq!(cached, [b"linked.rs".to_vec()]);
}

#[test]
fn gix_and_sanitized_git_preserve_case_colliding_identities() {
    let repository = TempRepository::new(None);
    let empty_blob = repository.git_output_text(&["hash-object", "-w", "--stdin"]);
    repository.git_text(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{empty_blob},Case.rs"),
    ]);
    repository.git_text(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{empty_blob},case.rs"),
    ]);

    let gix = gix_index_snapshot(repository.root());
    let cached = owned_paths(&git_cli_cached_paths(repository.root()));
    assert_eq!(gix.paths, cached);
    assert_eq!(cached, [b"Case.rs".to_vec(), b"case.rs".to_vec()]);
}

#[test]
fn sparse_gix_placeholders_are_not_mistaken_for_source_file_paths() {
    let repository = TempRepository::new(None);
    repository.write("kept/a.rs", b"fn kept() {}\n");
    repository.write("hidden/b.rs", b"fn hidden() {}\n");
    repository.git_text(&["add", "--", "kept/a.rs", "hidden/b.rs"]);
    repository.commit("sparse base");
    repository.git_text(&["sparse-checkout", "init", "--cone", "--sparse-index"]);
    repository.git_text(&["sparse-checkout", "set", "kept"]);

    let gix = gix_index_snapshot(repository.root());
    assert_eq!(gix.sparse_entry_count, 1);
    assert_ne!(gix.paths, [b"hidden/b.rs".to_vec(), b"kept/a.rs".to_vec()]);
    assert_eq!(
        owned_paths(&git_cli_cached_paths(repository.root())),
        [b"hidden/b.rs".to_vec(), b"kept/a.rs".to_vec()]
    );
}

#[cfg(unix)]
#[test]
fn isolated_gix_and_sanitized_git_do_not_execute_hostile_repository_config() {
    use std::os::unix::fs::PermissionsExt;

    let repository = TempRepository::new(None);
    repository.write("tracked.rs", b"fn tracked() {}\n");
    repository.git_text(&["add", "--", "tracked.rs"]);

    let marker = repository.auxiliary().join("executed");
    let script = repository.auxiliary().join("fsmonitor");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf executed > '{}'\n",
            marker.to_string_lossy()
        ),
    )
    .expect("hostile fixture script must be written");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700))
        .expect("hostile fixture script must be executable");
    let included_config = repository.auxiliary().join("included.config");
    fs::write(
        &included_config,
        format!("[core]\n\tfsmonitor = {}\n", script.to_string_lossy()),
    )
    .expect("hostile included config must be written");
    repository.git(&[
        OsString::from("config"),
        OsString::from("include.path"),
        included_config.into_os_string(),
    ]);

    let gix = gix_index_snapshot(repository.root());
    let all = discover_repository_paths(repository.root(), GitPathDiscoveryLimits::default())
        .expect("sanitized discovery must ignore executable config");
    assert_eq!(gix.paths, vec![b"tracked.rs".to_vec()]);
    assert_eq!(owned_paths(&all), gix.paths);
    assert!(
        !marker.exists(),
        "repository configuration must never execute the fixture script"
    );
}

mod performance;
