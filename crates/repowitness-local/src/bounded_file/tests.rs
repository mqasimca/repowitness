use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use super::*;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let physical_temporary_directory = std::fs::canonicalize(std::env::temp_dir())
            .expect("canonicalize temporary directory for no-follow fixture");
        let path = physical_temporary_directory.join(format!(
            "repowitness-bounded-file-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create temporary directory");
        Self(path)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn exact_limit_empty_and_option_shaped_names_are_admitted() {
    let directory = TempDirectory::new();
    let empty = directory.0.join("empty");
    fs::write(&empty, []).expect("write empty file");
    let admitted = read_bounded_regular_file(&empty, 0).expect("admit empty file");
    assert!(admitted.bytes().is_empty());

    let option_shaped = directory.0.join("--repository-config");
    fs::write(&option_shaped, b"1234").expect("write option-shaped file");
    let admitted = read_bounded_regular_file(&option_shaped, 4).expect("admit exact limit");
    assert_eq!(admitted.bytes(), b"1234");
    assert_eq!(admitted.sha256().len(), 32);
}

#[test]
fn one_byte_over_limit_and_unbounded_requests_fail_closed() {
    let directory = TempDirectory::new();
    let path = directory.0.join("control");
    fs::write(&path, b"12345").expect("write control file");
    assert_eq!(
        read_bounded_regular_file(&path, 4).expect_err("reject one byte over"),
        BoundedFileReadError::TooLarge
    );
    assert_eq!(
        read_bounded_regular_file(&path, MAX_BOUNDED_CONTROL_FILE_BYTES + 1)
            .expect_err("reject unbounded request"),
        BoundedFileReadError::InvalidRequest
    );
}

#[test]
fn absolute_relative_dot_and_parent_spellings_name_the_same_file() {
    let directory = TempDirectory::new();
    let child = directory.0.join("child");
    fs::create_dir(&child).expect("create child");
    let path = directory.0.join("control");
    fs::write(&path, b"safe").expect("write control file");
    let absolute = read_bounded_regular_file(&path, 4).expect("absolute path");

    let current = std::env::current_dir().expect("current directory");
    let relative_control = relative_path(&current, &path).expect("paths share an absolute root");
    let child_path = relative_path(&current, &child).expect("paths share an absolute root");
    let relative = read_bounded_regular_file(&Path::new(".").join(relative_control), 4);
    let parent = read_bounded_regular_file(&child_path.join("..").join("control"), 4);

    assert_eq!(relative.expect("relative path"), absolute);
    assert_eq!(parent.expect("parent path"), absolute);
}

fn relative_path(base: &Path, target: &Path) -> Option<PathBuf> {
    let base = base.components().collect::<Vec<_>>();
    let target = target.components().collect::<Vec<_>>();
    let common = base
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return None;
    }
    let mut relative = PathBuf::new();
    for _ in &base[common..] {
        relative.push("..");
    }
    for component in &target[common..] {
        relative.push(component.as_os_str());
    }
    Some(relative)
}

#[cfg(unix)]
#[test]
fn symlinked_parent_and_final_component_are_rejected() {
    use std::os::unix::fs::symlink;

    let directory = TempDirectory::new();
    let real_parent = directory.0.join("real");
    fs::create_dir(&real_parent).expect("create real parent");
    fs::write(real_parent.join("control"), b"safe").expect("write target");
    let parent_alias = directory.0.join("parent-alias");
    symlink(&real_parent, &parent_alias).expect("create parent alias");
    assert_eq!(
        read_bounded_regular_file(&parent_alias.join("control"), 4)
            .expect_err("reject symlinked parent"),
        BoundedFileReadError::Unavailable
    );

    let final_alias = directory.0.join("final-alias");
    symlink(real_parent.join("control"), &final_alias).expect("create final alias");
    assert_eq!(
        read_bounded_regular_file(&final_alias, 4).expect_err("reject final symlink"),
        BoundedFileReadError::Unavailable
    );
}

#[cfg(unix)]
#[test]
fn hard_links_fifo_directory_and_device_are_rejected_without_blocking() {
    use std::os::unix::fs::OpenOptionsExt as _;

    let directory = TempDirectory::new();
    let path = directory.0.join("control");
    fs::write(&path, b"safe").expect("write control file");
    let alias = directory.0.join("hard-link");
    fs::hard_link(&path, &alias).expect("create hard link");
    assert_eq!(
        read_bounded_regular_file(&path, 4).expect_err("reject hard link"),
        BoundedFileReadError::Unavailable
    );
    assert_eq!(
        read_bounded_regular_file(&directory.0, 4).expect_err("reject directory"),
        BoundedFileReadError::Unavailable
    );
    assert_eq!(
        read_bounded_regular_file(Path::new("/dev/null"), 4).expect_err("reject device"),
        BoundedFileReadError::Unavailable
    );

    let fifo = directory.0.join("fifo");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("start mkfifo");
    assert!(status.success());
    let _nonblocking_probe = fs::OpenOptions::new()
        .read(true)
        .custom_flags(
            i32::try_from(rustix::fs::OFlags::NONBLOCK.bits()).expect("flags fit platform integer"),
        )
        .open(&fifo)
        .expect("probe FIFO without blocking");
    assert_eq!(
        read_bounded_regular_file(&fifo, 4).expect_err("reject FIFO"),
        BoundedFileReadError::Unavailable
    );
}

#[test]
fn final_replacement_after_open_is_detected() {
    let directory = TempDirectory::new();
    let path = directory.0.join("control");
    let old = directory.0.join("old");
    fs::write(&path, b"same").expect("write original");
    let result = read_bounded_regular_file_with_hook(&path, 4, || {
        fs::rename(&path, &old).expect("move original");
        fs::write(&path, b"same").expect("write replacement");
    });
    assert_eq!(
        result.expect_err("detect final replacement"),
        BoundedFileReadError::Changed
    );
}

#[test]
fn parent_replacement_after_open_is_detected() {
    let directory = TempDirectory::new();
    let parent = directory.0.join("parent");
    let old_parent = directory.0.join("old-parent");
    fs::create_dir(&parent).expect("create parent");
    let path = parent.join("control");
    fs::write(&path, b"same").expect("write original");
    let result = read_bounded_regular_file_with_hook(&path, 4, || {
        fs::rename(&parent, &old_parent).expect("move parent");
        fs::create_dir(&parent).expect("create replacement parent");
        fs::write(parent.join("control"), b"same").expect("write replacement");
    });
    assert_eq!(
        result.expect_err("detect parent replacement"),
        BoundedFileReadError::Changed
    );
}

#[test]
fn admitted_parent_is_redacted_and_revalidates_unchanged_chain() {
    let directory = TempDirectory::new();
    let parent = directory.0.join("manifest-parent");
    fs::create_dir(&parent).expect("create manifest parent");
    let path = parent.join("repowitness-workspace.toml");
    fs::write(&path, b"safe").expect("write manifest");

    let (contents, admitted_parent) =
        read_bounded_regular_file_with_parent(&path, 4).expect("admit manifest and parent");

    assert_eq!(contents.bytes(), b"safe");
    assert!(admitted_parent.matches_contents(b"safe"));
    assert!(!admitted_parent.matches_contents(b"other"));
    assert_eq!(admitted_parent.lexical_path(), parent);
    admitted_parent
        .revalidate()
        .expect("unchanged parent chain");
    let debug = format!("{admitted_parent:?}");
    assert!(debug.contains("<redacted-path>"));
    assert!(!debug.contains("manifest-parent"));
}

#[test]
fn admitted_parent_revalidation_detects_final_file_mutation() {
    let directory = TempDirectory::new();
    let parent = directory.0.join("manifest-parent");
    fs::create_dir(&parent).expect("create manifest parent");
    let path = parent.join("repowitness-workspace.toml");
    fs::write(&path, b"safe").expect("write manifest");
    let (_contents, admitted_parent) =
        read_bounded_regular_file_with_parent(&path, 4).expect("admit manifest and parent");

    fs::write(&path, b"evil").expect("mutate admitted manifest in place");

    assert_eq!(
        admitted_parent
            .revalidate()
            .expect_err("detect final-file mutation"),
        BoundedFileReadError::Changed
    );
}

#[test]
fn admitted_parent_revalidation_detects_ancestor_replacement() {
    let directory = TempDirectory::new();
    let ancestor = directory.0.join("ancestor");
    let parent = ancestor.join("manifest-parent");
    fs::create_dir_all(&parent).expect("create manifest parent");
    let path = parent.join("repowitness-workspace.toml");
    fs::write(&path, b"safe").expect("write manifest");
    let (_contents, admitted_parent) =
        read_bounded_regular_file_with_parent(&path, 4).expect("admit manifest and parent");
    let moved = directory.0.join("moved-ancestor");
    fs::rename(&ancestor, &moved).expect("move admitted ancestor");
    fs::create_dir_all(&parent).expect("create replacement chain");
    fs::write(&path, b"safe").expect("write replacement manifest");

    assert_eq!(
        admitted_parent
            .revalidate()
            .expect_err("detect ancestor replacement"),
        BoundedFileReadError::Changed
    );
}

#[test]
fn in_place_mutation_after_open_is_detected() {
    let directory = TempDirectory::new();
    let path = directory.0.join("control");
    fs::write(&path, b"first").expect("write original");
    let result = read_bounded_regular_file_with_hook(&path, 6, || {
        fs::write(&path, b"second").expect("mutate file");
    });
    assert_eq!(
        result.expect_err("detect in-place mutation"),
        BoundedFileReadError::Changed
    );
}
