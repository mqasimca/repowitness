use std::{
    env, fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::Duration,
};

use repowitness_application::resolve_configuration;

use super::{
    LocalRetentionApplyRequest, LocalRetentionPins, TempDirectory, apply_local_retention,
    initialize_database, plan,
};

const PROCESS_CRASH_CHILD: &str = "REPOWITNESS_RETENTION_CRASH_CHILD";
const PROCESS_CRASH_DATABASE: &str = "REPOWITNESS_RETENTION_CRASH_DATABASE";
const PROCESS_CRASH_SENTINEL: &str = "REPOWITNESS_RETENTION_CRASH_SENTINEL";

#[test]
fn process_termination_after_committed_apply_preserves_authoritative_receipt() {
    if env::var_os(PROCESS_CRASH_CHILD).is_some() {
        run_crash_child();
        return;
    }

    let directory = TempDirectory::new();
    let database = initialize_database(directory.path());
    let sentinel = directory.path().join("crash-child-ready");
    let configuration = resolve_configuration(&[]).expect("configuration should resolve");
    let plan = plan(&database, &configuration, Duration::from_secs(10))
        .expect("retention plan should be available");

    let mut child = Command::new(
        env::current_exe().expect("unit-test executable path should be available"),
    )
    .args([
        "--exact",
        "local_retention::tests::process_recovery::process_termination_after_committed_apply_preserves_authoritative_receipt",
        "--nocapture",
    ])
    .env(PROCESS_CRASH_CHILD, "1")
    .env(PROCESS_CRASH_DATABASE, &database)
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

    let replay = LocalRetentionApplyRequest::try_new(
        &database,
        3,
        &configuration,
        LocalRetentionPins::default(),
        plan.plan_digest(),
        Arc::new(AtomicBool::new(false)),
        Duration::from_secs(10),
    )
    .expect("replay request should validate");
    let receipt = apply_local_retention(replay)
        .expect("the exact committed apply receipt must remain recoverable after a crash");
    assert_eq!(receipt.plan_digest(), plan.plan_digest());
    assert!(receipt.collection_id() > 0);
    assert!(receipt.shutdown_complete());
    assert!(receipt.database_identity_confirmed());
}

fn run_crash_child() {
    let database = environment_path(PROCESS_CRASH_DATABASE);
    let sentinel = environment_path(PROCESS_CRASH_SENTINEL);
    let configuration = resolve_configuration(&[]).expect("configuration should resolve");
    let plan = plan(&database, &configuration, Duration::from_secs(10))
        .expect("child retention plan should be available");
    let request = LocalRetentionApplyRequest::try_new(
        &database,
        2,
        &configuration,
        LocalRetentionPins::default(),
        plan.plan_digest(),
        Arc::new(AtomicBool::new(false)),
        Duration::from_secs(10),
    )
    .expect("child apply request should validate");
    super::super::execution::apply_local_retention_with_hooks(
        request,
        || {
            fs::write(&sentinel, b"ready").expect("crash child sentinel should be written");
            thread::sleep(Duration::from_secs(60));
        },
        |deadline| deadline,
    )
    .expect("crash child should return only if it was not terminated");
}

fn environment_path(name: &str) -> PathBuf {
    env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("crash child environment input is missing"))
}

fn wait_for_sentinel(child: &mut Child, sentinel: &std::path::Path) {
    for _ in 0..100 {
        if sentinel.is_file() {
            return;
        }
        if let Some(status) = child
            .try_wait()
            .expect("crash child status should be observable")
        {
            panic!("crash child exited early with {status}");
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("crash child did not reach the durable retention-commit boundary");
}
