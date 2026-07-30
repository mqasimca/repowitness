use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use repowitness_application::{PackageScope, ResolvedConfiguration};
use repowitness_domain::{
    ConnectedWorkspaceId, RepositoryIdentityDigest, RepositoryPathLimits, SourceSlotId,
};
#[cfg(unix)]
use rusqlite::{Connection, OpenFlags};

use super::super::{
    index_connected_workspace,
    model::{
        ConnectedSourceSlotRequest, ConnectedWorkspaceIndexReport, ConnectedWorkspaceIndexRequest,
    },
};
use crate::{OwnedSqliteIndex, WorkspaceViewId};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
const FIXTURE_DEADLINE: Duration = Duration::from_secs(180);

pub(super) struct TempDirectory(PathBuf);

impl TempDirectory {
    pub(super) fn new() -> Self {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let physical_temporary_directory = std::fs::canonicalize(std::env::temp_dir())
            .expect("canonicalize temporary directory for no-follow fixture");
        let path = physical_temporary_directory.join(format!(
            "repowitness-connected-workspace-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture directory should be created");
        Self(path)
    }

    pub(super) fn repository(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    pub(super) fn database(&self) -> PathBuf {
        self.0.join("connected.sqlite3")
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn fixture_repository(directory: &TempDirectory, name: &str) -> PathBuf {
    let repository = directory.repository(name);
    fs::create_dir_all(&repository).expect("fixture repository should be created");
    git(&repository, &["init", "--quiet"]);
    git(&repository, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(
        &repository,
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(&repository, &["config", "user.name", "RepoWitness Fixture"]);
    fs::write(repository.join("README.md"), "base\n").expect("base file should be written");
    git(&repository, &["add", "--", "README.md"]);
    git(&repository, &["commit", "--quiet", "-m", "base"]);

    write_source(
        &repository,
        "pkg_a/src/lib.rs",
        b"pub struct PackageA;\nimpl PackageA { pub fn run() {} }\n",
    );
    write_source(
        &repository,
        "pkg_b/src/lib.rs",
        b"pub struct PackageB;\nimpl PackageB { pub fn run() {} }\n",
    );
    git(&repository, &["commit", "--quiet", "-m", "sources"]);
    git(&repository, &["tag", "selected"]);
    repository
}

pub(super) fn write_source(repository: &Path, path: &str, content: &[u8]) {
    let destination = repository.join(path);
    fs::create_dir_all(destination.parent().expect("source has a parent"))
        .expect("source parent should be created");
    fs::write(destination, content).expect("source should be written");
    git(repository, &["add", "--", path]);
}

pub(super) fn git(repository: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(repository)
        .args(["-c", "core.autocrlf=false", "-c", "core.eol=lf"])
        .args(args)
        .status()
        .expect("Git should start");
    assert!(status.success(), "Git command should succeed");
}

pub(super) fn default_configuration() -> ResolvedConfiguration {
    repowitness_application::resolve_configuration(&[])
        .expect("built-in configuration should resolve")
}

pub(super) const fn connected(value: u8) -> ConnectedWorkspaceId {
    ConnectedWorkspaceId::new([value; 32])
}

pub(super) const fn repository(value: u8) -> RepositoryIdentityDigest {
    RepositoryIdentityDigest::new([value; 32])
}

pub(super) const fn source_slot(value: u8) -> SourceSlotId {
    SourceSlotId::new([value.wrapping_add(0x80); 32])
}

pub(super) fn scope(root: &[u8]) -> PackageScope {
    PackageScope::try_explicit_root_bytes([root], RepositoryPathLimits::new(4_096, 128))
        .expect("fixture package root should be valid")
}

pub(super) fn slot<'a>(
    source_slot: SourceSlotId,
    repository: RepositoryIdentityDigest,
    worktree: &'a Path,
    selector: &str,
    package_scope: PackageScope,
    configuration: &'a ResolvedConfiguration,
) -> ConnectedSourceSlotRequest<'a> {
    ConnectedSourceSlotRequest::try_new(
        source_slot,
        repository,
        worktree,
        selector,
        package_scope,
        configuration,
        crate::LocalRustIndexLimits::default(),
        crate::source_selector::SourceSelectorLimits::default(),
        FIXTURE_DEADLINE,
    )
    .expect("fixture source slot should validate")
}

pub(super) fn request<'a>(
    connected_workspace: ConnectedWorkspaceId,
    database: &'a Path,
    source_slots: Vec<ConnectedSourceSlotRequest<'a>>,
) -> ConnectedWorkspaceIndexRequest<'a> {
    ConnectedWorkspaceIndexRequest::try_new(
        connected_workspace,
        database,
        0,
        FIXTURE_DEADLINE,
        source_slots,
    )
    .expect("fixture workspace request should validate")
}

pub(super) fn index(request: ConnectedWorkspaceIndexRequest<'_>) -> ConnectedWorkspaceIndexReport {
    index_connected_workspace(request, Arc::new(AtomicBool::new(false)))
        .expect("connected workspace should index")
}

pub(super) fn active_view(
    database: &Path,
    connected_workspace: ConnectedWorkspaceId,
) -> WorkspaceViewId {
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(10))
        .expect("fixture deadline should fit");
    let (writer, _) =
        OwnedSqliteIndex::start(database, 0, deadline).expect("fixture writer should start");
    let view = writer
        .active_workspace_view(
            connected_workspace,
            Arc::new(AtomicBool::new(false)),
            deadline,
        )
        .expect("active view should load")
        .expect("active view should exist")
        .view();
    writer.shutdown(deadline).expect("writer should shut down");
    view
}

#[cfg(unix)]
pub(super) fn active_view_database_id_unchecked(
    database: &Path,
    connected_workspace: ConnectedWorkspaceId,
) -> i64 {
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .expect("read-only verification connection should open");
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
