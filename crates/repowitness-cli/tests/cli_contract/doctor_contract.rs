fn fixture_entries(path: &Path) -> Vec<Vec<u8>> {
    let mut entries = fs::read_dir(path)
        .expect("fixture directory should be readable")
        .map(|entry| {
            entry
                .expect("fixture entry should be readable")
                .file_name()
                .as_encoded_bytes()
                .to_vec()
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

#[test]
fn doctor_without_targets_is_a_successful_configuration_only_warning() {
    let output = repowitness(&["doctor"]);

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let report = String::from_utf8(output.stdout).expect("doctor report must be UTF-8");
    assert!(report.starts_with("operation=doctor\nstatus=warning\nschema_version=1\n"));
    assert_eq!(report_value(&report, "check_configuration"), "ok");
    assert_eq!(report_value(&report, "check_language_adapters"), "ok");
    assert_eq!(report_value(&report, "check_mcp_tool_profile"), "ok");
    assert_eq!(
        report_value(&report, "check_repository_capability"),
        "not_run"
    );
    assert_eq!(report_value(&report, "database_state"), "not_requested");
    assert_eq!(
        report_value(&report, "warning_0"),
        "target_checks_not_requested"
    );
}

#[test]
fn doctor_validates_an_existing_index_without_mutating_local_state() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let indexed = index(&repository, &database, REPOSITORY_ID);
    assert!(
        indexed.status.success(),
        "index failed: {}",
        String::from_utf8_lossy(&indexed.stderr)
    );
    let before_bytes = fs::read(&database).expect("database should be readable");
    let before_entries = fixture_entries(&directory.0);

    let output = repowitness_os([
        OsStr::new("doctor"),
        OsStr::new("--repository"),
        repository.as_os_str(),
        OsStr::new("--database"),
        database.as_os_str(),
    ]);

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let report = String::from_utf8(output.stdout).expect("doctor report must be UTF-8");
    assert!(report.starts_with("operation=doctor\nstatus=ok\n"));
    assert_eq!(report_value(&report, "check_repository_capability"), "ok");
    assert_eq!(report_value(&report, "check_database_placement"), "ok");
    assert_eq!(report_value(&report, "check_database_capability"), "ok");
    assert_eq!(report_value(&report, "check_sqlite_runtime"), "ok");
    assert_eq!(report_value(&report, "check_sqlite_compile_options"), "ok");
    assert_eq!(report_value(&report, "check_database_schema"), "ok");
    assert_eq!(report_value(&report, "database_state"), "existing");
    assert_eq!(report_value(&report, "error_count"), "0");
    assert_eq!(report_value(&report, "warning_count"), "0");
    assert!(!report.contains(repository.to_string_lossy().as_ref()));
    assert!(!report.contains(database.to_string_lossy().as_ref()));
    assert_eq!(
        fs::read(&database).expect("database should remain readable"),
        before_bytes
    );
    assert_eq!(fixture_entries(&directory.0), before_entries);
}
