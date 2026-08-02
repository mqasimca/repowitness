#[cfg(unix)]
#[test]
fn explicit_onboarding_keeps_private_state_outside_the_worktree_and_hands_off_to_read_only_queries() {
    let directory = TempDirectory::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700))
            .expect("fixture parent should be private");
    }
    let repository = fixture_repository(&directory);
    let state_root = directory.0.join("private-state");
    let before = Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(&repository)
        .output()
        .expect("Git status should start");
    assert!(before.status.success());

    let onboarded = repowitness_os([
        OsStr::new("onboard"),
        OsStr::new("--root"),
        repository.as_os_str(),
        OsStr::new("--state-dir"),
        state_root.as_os_str(),
        OsStr::new("--repository-id"),
        OsStr::new(REPOSITORY_ID),
    ]);
    assert!(
        onboarded.status.success(),
        "onboarding failed: {}",
        String::from_utf8_lossy(&onboarded.stderr)
    );
    assert!(onboarded.stderr.is_empty());
    let report = String::from_utf8(onboarded.stdout).expect("onboarding report must be UTF-8");
    assert!(report.contains("status=ok\noperation=onboard\n"));
    assert_eq!(report_value(&report, "repository_id"), REPOSITORY_ID);
    assert_eq!(
        report_value(&report, "state_directory_convention"),
        "repowitness/repositories/<repository-id>/index.sqlite3"
    );
    assert_eq!(report_value(&report, "generation"), "1");
    assert!(!report.contains(repository.to_string_lossy().as_ref()));
    assert!(!report.contains(state_root.to_string_lossy().as_ref()));

    let database = state_root
        .join("repowitness")
        .join("repositories")
        .join(REPOSITORY_ID)
        .join("index.sqlite3");
    assert!(database.is_file());
    assert!(!database.starts_with(&repository));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            fs::metadata(database.parent().expect("database parent"))
                .expect("private state metadata")
                .permissions()
                .mode()
                & 0o077,
            0
        );
    }

    let after = Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(&repository)
        .output()
        .expect("Git status should start");
    assert!(after.status.success());
    assert_eq!(after.stdout, before.stdout);

    let queried = search(&database, REPOSITORY_ID, "Widget", "1");
    assert!(queried.status.success());
    assert!(queried.stderr.is_empty());

    let reindexed = repowitness_os([
        OsStr::new("onboard"),
        OsStr::new("--root"),
        repository.as_os_str(),
        OsStr::new("--state-dir"),
        state_root.as_os_str(),
        OsStr::new("--repository-id"),
        OsStr::new(REPOSITORY_ID),
    ]);
    assert!(
        reindexed.status.success(),
        "repeat onboarding failed: {}",
        String::from_utf8_lossy(&reindexed.stderr)
    );
    let reindexed_report =
        String::from_utf8(reindexed.stdout).expect("repeat onboarding report must be UTF-8");
    assert_eq!(report_value(&reindexed_report, "generation"), "2");
    let reopened = search(&database, REPOSITORY_ID, "Widget", "2");
    assert!(reopened.status.success());
    assert!(reopened.stderr.is_empty());
}

#[test]
fn onboarding_rejects_a_state_directory_inside_the_explicit_repository_before_it_writes() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let before = Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(&repository)
        .output()
        .expect("Git status should start");
    assert!(before.status.success());
    let inside_repository_state = repository.join("private-state");

    let output = repowitness_os([
        OsStr::new("onboard"),
        OsStr::new("--root"),
        repository.as_os_str(),
        OsStr::new("--state-dir"),
        inside_repository_state.as_os_str(),
        OsStr::new("--repository-id"),
        OsStr::new(REPOSITORY_ID),
    ]);
    assert_eq!(output.status.code(), Some(70));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("diagnostic must be UTF-8");
    assert_eq!(diagnostic, "error: private onboarding state is unavailable\n");
    assert!(!diagnostic.contains(repository.to_string_lossy().as_ref()));
    assert!(!inside_repository_state.exists());

    let after = Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(&repository)
        .output()
        .expect("Git status should start");
    assert!(after.status.success());
    assert_eq!(after.stdout, before.stdout);
}

#[test]
fn onboarding_rejects_a_non_repository_root_before_creating_private_state() {
    let directory = TempDirectory::new();
    let non_repository = directory.0.join("not-a-repository");
    fs::create_dir(&non_repository).expect("non-repository fixture directory should exist");
    let state_root = directory.0.join("private-state");

    let output = repowitness_os([
        OsStr::new("onboard"),
        OsStr::new("--root"),
        non_repository.as_os_str(),
        OsStr::new("--state-dir"),
        state_root.as_os_str(),
        OsStr::new("--repository-id"),
        OsStr::new(REPOSITORY_ID),
    ]);

    assert_eq!(output.status.code(), Some(70));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).expect("diagnostic must be UTF-8"),
        "error: onboarding root is unavailable\n"
    );
    assert!(!state_root.exists());
}
