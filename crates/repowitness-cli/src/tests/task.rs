use std::{ffi::OsString, process};

use super::super::{run_task, run_task_status};

#[test]
fn task_status_for_an_absent_database_is_not_found_without_initializing_it() {
    let database = std::env::temp_dir().join(format!(
        "repowitness-task-status-missing-{}.db",
        process::id()
    ));
    assert!(!database.exists(), "fixture path must be absent");
    let repository = format!("rwi1:h:{}", "00".repeat(32));
    let task_id = "00".repeat(16);
    let arguments = vec![
        OsString::from("--repository-id"),
        OsString::from(repository),
        OsString::from("--database"),
        database.as_os_str().to_os_string(),
        OsString::from("--task-id"),
        OsString::from(task_id),
    ];
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit = run_task_status(arguments.into_iter(), &mut stdout, &mut stderr);

    assert_eq!(exit, 0);
    assert_eq!(stdout, b"operation=task-status\nstatus=not_found\n");
    assert!(stderr.is_empty());
    assert!(!database.exists(), "task-status must not create a database");
}

#[test]
fn task_create_and_checkpoint_append_redacted_durable_state() {
    let database = std::env::temp_dir().join(format!(
        "repowitness-task-checkpoint-{}-{}.db",
        process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos(),
    ));
    assert!(!database.exists(), "fixture path must be absent");
    let repository = format!("rwi1:h:{}", "11".repeat(32));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let create = vec![
        OsString::from("create"),
        OsString::from("--repository-id"),
        OsString::from(&repository),
        OsString::from("--database"),
        database.as_os_str().to_os_string(),
        OsString::from("--state"),
        OsString::from("open"),
        OsString::from("--objective"),
        OsString::from("record durable task progress"),
        OsString::from("--next-safe-action"),
        OsString::from("append a checkpoint"),
    ];

    let exit = run_task(create.into_iter(), &mut stdout, &mut stderr);

    assert_eq!(exit, 0);
    assert!(stderr.is_empty());
    let create_output = String::from_utf8(stdout).expect("CLI output is UTF-8");
    let task_id = create_output
        .lines()
        .find_map(|line| line.strip_prefix("task_id="))
        .expect("create emits an opaque task ID")
        .to_owned();
    assert_eq!(task_id.len(), 32);
    assert!(task_id.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(create_output.contains("operation=task-create\n"));
    assert!(create_output.contains("checkpoint_sequence=1\n"));

    let checkpoint = vec![
        OsString::from("checkpoint"),
        OsString::from("--repository-id"),
        OsString::from(&repository),
        OsString::from("--database"),
        database.as_os_str().to_os_string(),
        OsString::from("--task-id"),
        OsString::from(&task_id),
        OsString::from("--state"),
        OsString::from("blocked"),
        OsString::from("--objective"),
        OsString::from("await an explicit decision"),
        OsString::from("--hypothesis"),
        OsString::from("the maintainer will choose a benchmark corpus"),
    ];
    let mut checkpoint_stdout = Vec::new();
    let mut checkpoint_stderr = Vec::new();

    let exit = run_task(
        checkpoint.into_iter(),
        &mut checkpoint_stdout,
        &mut checkpoint_stderr,
    );

    assert_eq!(exit, 0);
    assert!(checkpoint_stderr.is_empty());
    assert_eq!(
        checkpoint_stdout,
        format!("operation=task-checkpoint\ntask_id={task_id}\ncheckpoint_sequence=2\n").as_bytes()
    );

    let status = vec![
        OsString::from("--repository-id"),
        OsString::from(&repository),
        OsString::from("--database"),
        database.as_os_str().to_os_string(),
        OsString::from("--task-id"),
        OsString::from(task_id),
    ];
    let mut status_stdout = Vec::new();
    let mut status_stderr = Vec::new();
    let exit = run_task_status(status.into_iter(), &mut status_stdout, &mut status_stderr);

    assert_eq!(exit, 0);
    assert!(status_stderr.is_empty());
    assert_eq!(
        status_stdout,
        b"operation=task-status\nstatus=found\nstate=blocked\ncheckpoint_sequence=2\nverification_count=0\n"
    );
    let _ = std::fs::remove_file(&database);
    let _ = std::fs::remove_file(database.with_extension("db-shm"));
    let _ = std::fs::remove_file(database.with_extension("db-wal"));
}
