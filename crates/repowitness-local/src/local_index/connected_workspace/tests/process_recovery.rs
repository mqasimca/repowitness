use std::{
    env, fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::{Duration, Instant},
};

use repowitness_application::PackageScope;

use super::{
    super::{CoordinatorPhase, index_connected_workspace_with_hook},
    fixtures::{
        TempDirectory, active_view, connected, default_configuration, fixture_repository, index,
        repository, request, slot, source_slot,
    },
};
use crate::OwnedSqliteIndex;

const PROCESS_CRASH_CHILD: &str = "REPOWITNESS_CONNECTED_WORKSPACE_CRASH_CHILD";
const PROCESS_CRASH_DATABASE: &str = "REPOWITNESS_CONNECTED_WORKSPACE_CRASH_DATABASE";
const PROCESS_CRASH_REPOSITORY: &str = "REPOWITNESS_CONNECTED_WORKSPACE_CRASH_REPOSITORY";
const PROCESS_CRASH_SENTINEL: &str = "REPOWITNESS_CONNECTED_WORKSPACE_CRASH_SENTINEL";

#[test]
fn process_termination_after_graph_staging_recovers_and_preserves_active_view() {
    if env::var_os(PROCESS_CRASH_CHILD).is_some() {
        run_crash_child();
        return;
    }

    let directory = TempDirectory::new();
    let repository_path = fixture_repository(&directory, "process-recovery");
    let database = directory.database();
    let sentinel = database
        .parent()
        .expect("fixture database has a parent")
        .join("crash-child-ready");
    let configuration = default_configuration();
    let connected_workspace = connected(37);
    let initial = index(workspace_request(
        connected_workspace,
        &database,
        &repository_path,
        &configuration,
    ));

    let mut child = Command::new(
        env::current_exe().expect("unit-test executable path should be available"),
    )
    .args([
        "--exact",
        "local_index::connected_workspace::tests::process_recovery::process_termination_after_graph_staging_recovers_and_preserves_active_view",
        "--nocapture",
    ])
    .env(PROCESS_CRASH_CHILD, "1")
    .env(PROCESS_CRASH_DATABASE, &database)
    .env(PROCESS_CRASH_REPOSITORY, &repository_path)
    .env(PROCESS_CRASH_SENTINEL, &sentinel)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .expect("crash child should start");
    wait_for_sentinel(&mut child, &sentinel);
    child.kill().expect("crash child should terminate");
    assert!(
        !child
            .wait()
            .expect("crash child exit should be observable")
            .success(),
        "crash child must not exit successfully"
    );

    let deadline = Instant::now()
        .checked_add(Duration::from_secs(10))
        .expect("recovery deadline should be representable");
    let (writer, startup) = OwnedSqliteIndex::start(&database, 0, deadline)
        .expect("startup must recover the interrupted candidate");
    assert_eq!(startup.recovered_generations(), 1);
    assert_eq!(
        writer
            .active_workspace_view(
                connected_workspace,
                Arc::new(AtomicBool::new(false)),
                deadline,
            )
            .expect("active view should remain readable")
            .expect("active view should remain present")
            .view(),
        initial.view()
    );
    writer
        .shutdown(deadline)
        .expect("recovery writer should stop");
    assert_eq!(
        active_view(&database, connected_workspace),
        initial.view(),
        "recovered workspace must keep the prior immutable view active"
    );
}

fn run_crash_child() {
    let database = environment_path(PROCESS_CRASH_DATABASE);
    let repository_path = environment_path(PROCESS_CRASH_REPOSITORY);
    let sentinel = environment_path(PROCESS_CRASH_SENTINEL);
    let configuration = default_configuration();
    let request = workspace_request(connected(37), &database, &repository_path, &configuration);
    index_connected_workspace_with_hook(request, Arc::new(AtomicBool::new(false)), |phase, _| {
        if phase == CoordinatorPhase::GraphStaged {
            fs::write(&sentinel, b"ready").expect("crash child sentinel should be written");
            thread::sleep(Duration::from_secs(60));
        }
    })
    .expect("crash child should only return if it was not terminated");
}

fn workspace_request<'a>(
    connected_workspace: repowitness_domain::ConnectedWorkspaceId,
    database: &'a std::path::Path,
    repository_path: &'a std::path::Path,
    configuration: &'a repowitness_application::ResolvedConfiguration,
) -> super::super::model::ConnectedWorkspaceIndexRequest<'a> {
    request(
        connected_workspace,
        database,
        vec![slot(
            source_slot(1),
            repository(1),
            repository_path,
            "worktree-head",
            PackageScope::whole_repository(),
            configuration,
        )],
    )
}

fn environment_path(name: &str) -> PathBuf {
    env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("crash child environment input is missing"))
}

fn wait_for_sentinel(child: &mut Child, sentinel: &std::path::Path) {
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(15))
        .expect("crash-child admission deadline should be representable");
    loop {
        if sentinel.is_file() {
            return;
        }
        if let Some(status) = child
            .try_wait()
            .expect("crash child status should be observable")
        {
            panic!("crash child exited early with {status}");
        }
        if Instant::now() >= deadline {
            panic!("crash child did not reach the durable graph-staging boundary");
        }
        thread::sleep(Duration::from_millis(25));
    }
}
