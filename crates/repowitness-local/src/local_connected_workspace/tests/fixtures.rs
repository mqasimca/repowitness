use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use repowitness_application::{
    ConnectedWorkspaceIdTextV1, RepositoryIdentityTextV1, ResolvedConfiguration, SourceSlotIdTextV1,
};
use repowitness_domain::{ConnectedWorkspaceId, RepositoryIdentityDigest, SourceSlotId};
use rusqlite::{Connection, OpenFlags};

use super::super::*;
use crate::{AdmittedFileParent, BoundedFileContents, read_bounded_regular_file_with_parent};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

pub(super) struct TempDirectory(PathBuf);

impl TempDirectory {
    pub(super) fn new() -> Self {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let physical_temporary_directory = std::fs::canonicalize(std::env::temp_dir())
            .expect("canonicalize temporary directory for no-follow fixture");
        let path = physical_temporary_directory.join(format!(
            "repowitness-connected-facade-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture directory should be created");
        Self(path)
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }

    pub(super) fn join(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn fixture_repository(directory: &Path, name: &str) -> PathBuf {
    let repository = directory.join(name);
    fs::create_dir_all(&repository).expect("fixture repository should be created");
    git(&repository, &["init", "--quiet"]);
    git(&repository, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(
        &repository,
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(&repository, &["config", "user.name", "RepoWitness Fixture"]);
    write_source(&repository, "README.md", b"fixture\n");
    write_source(
        &repository,
        "src/lib.rs",
        b"pub struct Fixture;\nimpl Fixture { pub fn run() {} }\n",
    );
    git(&repository, &["commit", "--quiet", "-m", "fixture"]);
    repository
}

pub(super) fn write_source(repository: &Path, path: &str, content: &[u8]) {
    let destination = repository.join(path);
    fs::create_dir_all(destination.parent().expect("source has a parent"))
        .expect("source parent should be created");
    fs::write(destination, content).expect("source should be written");
    git(repository, &["add", "--", path]);
}

fn git(repository: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .current_dir(repository)
        .args(arguments)
        .status()
        .expect("Git should start");
    assert!(status.success(), "Git command should succeed");
}

pub(super) fn default_configuration() -> ResolvedConfiguration {
    repowitness_application::resolve_configuration(&[])
        .expect("built-in configuration should resolve")
}

pub(super) fn workspace_text(value: u8) -> String {
    ConnectedWorkspaceIdTextV1::encode(ConnectedWorkspaceId::new([value; 32])).into_string()
}

pub(super) fn slot_text(value: u8) -> String {
    SourceSlotIdTextV1::encode(SourceSlotId::new([value.wrapping_add(0x80); 32])).into_string()
}

pub(super) fn repository_text(value: u8) -> String {
    RepositoryIdentityTextV1::encode(RepositoryIdentityDigest::new([value; 32])).into_string()
}

pub(super) fn source_table(slot: u8, repository: u8, worktree_root: &str) -> String {
    format!(
        "\n[[source]]\nsource_slot_id = {:?}\nrepository_identity = {:?}\nworktree_root = {:?}\nselector = {{ kind = \"worktree-head\" }}\nscope = {{ kind = \"whole-repository\" }}\n",
        slot_text(slot),
        repository_text(repository),
        worktree_root,
    )
}

pub(super) fn manifest(workspace: u8, sources: &[String]) -> String {
    let mut text = format!(
        "schema_version = 1\nconnected_workspace_id = {:?}\n",
        workspace_text(workspace)
    );
    for source in sources {
        text.push_str(source);
    }
    text
}

pub(super) fn admit_manifest(
    directory: &Path,
    name: &str,
    text: &str,
) -> (BoundedFileContents, AdmittedFileParent) {
    let path = directory.join(name);
    fs::write(&path, text).expect("manifest should be written");
    read_bounded_regular_file_with_parent(&path, MAX_LOCAL_CONNECTED_WORKSPACE_MANIFEST_BYTES)
        .expect("manifest file should be admitted")
}

pub(super) fn request<'a>(
    contents: &'a BoundedFileContents,
    parent: &'a AdmittedFileParent,
    database: &'a Path,
    configuration: &'a ResolvedConfiguration,
) -> LocalConnectedWorkspaceIndexRequest<'a> {
    LocalConnectedWorkspaceIndexRequest::new(contents.bytes(), parent, database, configuration, 0)
}

pub(super) fn active_view_database_id(database: &Path, workspace: u8) -> i64 {
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .expect("verification connection should open");
    let connected_workspace = ConnectedWorkspaceId::new([workspace; 32]);
    connection
        .query_row(
            "SELECT active.workspace_view_id
             FROM active_workspace_views AS active
             JOIN workspace_views AS view
               ON view.connected_workspace_id = active.connected_workspace_id
              AND view.workspace_view_id = active.workspace_view_id
             WHERE active.connected_workspace_id = ?1
               AND view.lifecycle_state = 'published'",
            [connected_workspace.as_bytes().as_slice()],
            |row| row.get(0),
        )
        .expect("published active view should remain readable")
}

pub(super) fn short_source_limits() -> LocalConnectedWorkspaceSourceLimits {
    LocalConnectedWorkspaceSourceLimits::try_new(
        crate::LocalRustIndexLimits::default(),
        Duration::from_secs(10),
        256,
        Duration::from_secs(30),
    )
    .expect("fixture limits should validate")
}
