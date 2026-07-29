fn watch_process(repository: &Path, database: &Path) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .args([
            OsStr::new("watch"),
            OsStr::new("--repository-id"),
            OsStr::new(REPOSITORY_ID),
            OsStr::new("--database"),
            database.as_os_str(),
            OsStr::new("--"),
            repository.as_os_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("foreground watch process must start")
}

fn wait_for_initial_watch_generation(
    child: &mut std::process::Child,
    database: &Path,
    repository: &Path,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        assert!(
            child.try_wait().expect("watch status").is_none(),
            "watch exited before its initial generation"
        );
        if database.is_file() {
            let probe = search(database, REPOSITORY_ID, "Widget", "1");
            if probe.status.success() {
                return;
            }
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "watch did not publish an initial generation; repository path remains private: {}",
                repository.exists()
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn wait_for_changed_watch_generation(
    child: &mut std::process::Child,
    repository: &Path,
    database: &Path,
) {
    fs::write(
        repository.join("src/lib.rs"),
        "pub struct Widget;\nimpl Widget { pub fn run() {} }\npub fn watched_change() {}\n",
    )
    .expect("changed fixture source");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        assert!(
            child.try_wait().expect("watch status").is_none(),
            "watch exited before reconciling a source change"
        );
        let probe = search(database, REPOSITORY_ID, "watched_change", "1");
        if probe.status.success()
            && String::from_utf8_lossy(&probe.stdout).contains("match_0_name=watched_change\n")
        {
            return;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("watch did not reconcile a complete changed source state");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn wait_for_watch_exit(mut child: std::process::Child) -> Output {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().expect("watch status") {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("watch did not stop within the bounded shutdown window");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    std::io::Read::read_to_end(
        child.stdout.as_mut().expect("watch stdout pipe"),
        &mut stdout,
    )
    .expect("watch stdout");
    std::io::Read::read_to_end(
        child.stderr.as_mut().expect("watch stderr pipe"),
        &mut stderr,
    )
    .expect("watch stderr");
    Output {
        status,
        stdout,
        stderr,
    }
}

#[cfg(unix)]
#[test]
fn foreground_watch_handles_first_sigint_and_sigterm_and_preserves_the_active_generation() {
    for signal in ["-INT", "-TERM"] {
        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);
        let database = directory.database();
        let mut child = watch_process(&repository, &database);
        wait_for_initial_watch_generation(&mut child, &database, &repository);
        let expected_generation = if signal == "-TERM" {
            wait_for_changed_watch_generation(&mut child, &repository, &database);
            "2"
        } else {
            "1"
        };

        let signal_status = Command::new("kill")
            .arg(signal)
            .arg(child.id().to_string())
            .status()
            .expect("POSIX kill utility must start");
        assert!(signal_status.success());
        let output = wait_for_watch_exit(child);

        assert!(output.status.success(), "{signal}");
        assert!(output.stderr.is_empty(), "{signal}");
        let report = String::from_utf8(output.stdout).expect("watch receipt must be UTF-8");
        assert!(report.starts_with("status=ok\noperation=watch\nschema_version=1\n"));
        assert!(report.contains("watch_profile=1\n"));
        assert!(report.contains("exit=cancelled\n"));
        assert!(
            report_value(&report, "reconciliations_started")
                .parse::<u64>()
                .expect("started count")
                >= 1
        );
        assert!(
            report_value(&report, "reconciliations_completed")
                .parse::<u64>()
                .expect("completed count")
                >= 1
        );
        assert!(matches!(
            report_value(&report, "last_reconciliation"),
            "published" | "unchanged"
        ));
        assert_eq!(
            report_value(&report, "last_generation"),
            expected_generation
        );
        assert!(!report.contains(REPOSITORY_ID));
        assert!(!report.contains(repository.to_string_lossy().as_ref()));
        assert!(!report.contains(database.to_string_lossy().as_ref()));

        let still_readable = search(&database, REPOSITORY_ID, "Widget", "1");
        assert!(still_readable.status.success());
        assert!(still_readable.stderr.is_empty());
        let search_report =
            String::from_utf8(still_readable.stdout).expect("search report must be UTF-8");
        assert_eq!(
            report_value(&search_report, "generation"),
            expected_generation
        );
        assert!(search_report.contains("match_0_name=Widget\n"));
    }
}

#[test]
fn watch_help_is_foreground_only_and_does_not_create_state() {
    let output = repowitness(&["watch", "--help"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).expect("watch help must be UTF-8");
    assert!(help.contains("never detaches or starts a daemon"));
    assert!(help.contains("SIGTERM"));
}
