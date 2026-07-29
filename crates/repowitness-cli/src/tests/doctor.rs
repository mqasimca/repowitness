use std::{
    cell::Cell,
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use super::*;

struct FakeConfigurationLoader {
    calls: Cell<u64>,
    outcome: Result<ResolvedConfiguration, ConfigurationLoadError>,
}

impl ConfigurationLoader for FakeConfigurationLoader {
    fn load(
        &self,
        _invocation: &ConfigurationInvocation,
    ) -> Result<ResolvedConfiguration, ConfigurationLoadError> {
        self.calls.set(self.calls.get() + 1);
        self.outcome.clone()
    }
}

struct FakeDoctorInspector {
    calls: Cell<u64>,
    saw_targets: Cell<bool>,
    outcome: LocalDoctorReport,
}

impl DoctorInspector for FakeDoctorInspector {
    fn inspect(
        &self,
        _configuration: &ResolvedConfiguration,
        targets: Option<&DoctorTargetsInvocation>,
    ) -> LocalDoctorReport {
        self.calls.set(self.calls.get() + 1);
        self.saw_targets.set(targets.is_some());
        self.outcome
    }
}

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "repowitness-cli-doctor-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("temporary directory should be created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct FailingWriter;

impl io::Write for FailingWriter {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("expected test failure"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn default_configuration() -> ResolvedConfiguration {
    resolve_configuration(&[]).expect("built-in configuration should resolve")
}

fn config_only_report() -> LocalDoctorReport {
    inspect_local_doctor(&default_configuration(), None)
}

#[test]
fn no_argument_doctor_is_successful_and_explicitly_warns_about_targets() {
    let configuration = default_configuration();
    let loader = FakeConfigurationLoader {
        calls: Cell::new(0),
        outcome: Ok(configuration.clone()),
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let code = run_doctor(std::iter::empty(), &mut stdout, &mut stderr, &loader);

    assert_eq!(code, EXIT_SUCCESS);
    assert_eq!(loader.calls.get(), 1);
    assert!(stderr.is_empty());
    let output = String::from_utf8(stdout).expect("report should be UTF-8");
    assert_eq!(
        output,
        format!(
            concat!(
                "operation=doctor\n",
                "status=warning\n",
                "schema_version=1\n",
                "resolver_version=1\n",
                "profile=local\n",
                "profile_supplied_by=built_in_defaults\n",
                "configuration_digest_sha256={}\n",
                "requested_mcp_tool_profile=canonical\n",
                "authorized_mcp_tool_profile=canonical\n",
                "enabled_language_adapter_count=5\n",
                "compiled_language_adapter_count=5\n",
                "check_configuration=ok\n",
                "check_language_adapters=ok\n",
                "check_mcp_tool_profile=ok\n",
                "check_incompatible_settings=ok\n",
                "check_repository_capability=not_run\n",
                "check_database_placement=not_run\n",
                "check_database_capability=not_run\n",
                "check_sqlite_runtime=not_run\n",
                "check_sqlite_compile_options=not_run\n",
                "check_database_schema=not_run\n",
                "database_state=not_requested\n",
                "sqlite_runtime_version_number=not_run\n",
                "error_count=0\n",
                "warning_count=1\n",
                "warning_0=target_checks_not_requested\n",
            ),
            hex(configuration.digest().as_bytes())
        )
    );
}

#[test]
fn paired_targets_run_read_only_checks_and_never_appear_in_output() {
    let directory = TempDirectory::new();
    let repository = directory.path().join("sensitive-repository");
    let state = directory.path().join("sensitive-state");
    std::fs::create_dir(&repository).expect("repository should be created");
    std::fs::create_dir(&state).expect("state should be created");
    let database = state.join("sensitive-index.sqlite3");
    let loader = FakeConfigurationLoader {
        calls: Cell::new(0),
        outcome: Ok(default_configuration()),
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let code = run_doctor(
        [
            OsString::from("--repository"),
            repository.clone().into_os_string(),
            OsString::from("--database"),
            database.clone().into_os_string(),
        ]
        .into_iter(),
        &mut stdout,
        &mut stderr,
        &loader,
    );

    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert!(!database.exists());
    let output = String::from_utf8(stdout).expect("report should be UTF-8");
    assert!(output.starts_with("operation=doctor\nstatus=warning\n"));
    assert!(output.contains("check_repository_capability=ok\n"));
    assert!(output.contains("check_database_placement=ok\n"));
    assert!(output.contains("check_database_capability=ok\n"));
    assert!(output.contains("check_sqlite_runtime=ok\n"));
    assert!(output.contains("check_sqlite_compile_options=ok\n"));
    assert!(output.contains("check_database_schema=not_run\n"));
    assert!(output.contains("database_state=missing\n"));
    assert!(output.ends_with("warning_0=database_missing\n"));
    assert!(!output.contains("sensitive"));
}

#[test]
fn parser_accepts_all_five_option_pairs_in_any_order() {
    let invocation = parse_doctor_arguments(
        &[
            "--database",
            "hidden-db-value",
            "--workspace-config",
            "workspace",
            "--repository",
            "hidden-root-value",
            "--user-config",
            "user",
            "--repository-config",
            "repository-config",
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>(),
    )
    .expect("five option pairs should parse");

    assert_eq!(invocation.configuration.user, Some(PathBuf::from("user")));
    assert_eq!(
        invocation.configuration.workspace,
        Some(PathBuf::from("workspace"))
    );
    assert_eq!(
        invocation.configuration.repository,
        Some(PathBuf::from("repository-config"))
    );
    let targets = invocation.targets.expect("targets should be present");
    assert_eq!(targets.repository, PathBuf::from("hidden-root-value"));
    assert_eq!(targets.database, PathBuf::from("hidden-db-value"));
    let debug = format!("{targets:?}");
    assert_eq!(debug.matches("<redacted-path>").count(), 2);
    assert!(!debug.contains("hidden"));
}

#[test]
fn parser_rejects_partial_duplicate_unknown_odd_empty_and_excess_targets() {
    for arguments in [
        vec!["--repository", "repo"],
        vec!["--database", "db"],
        vec![
            "--repository",
            "one",
            "--repository",
            "two",
            "--database",
            "db",
        ],
        vec![
            "--database",
            "one",
            "--database",
            "two",
            "--repository",
            "repo",
        ],
        vec!["--unknown", "value"],
        vec!["--repository"],
        vec!["--repository", "", "--database", "db"],
        vec![
            "--user-config",
            "one",
            "--workspace-config",
            "two",
            "--repository-config",
            "three",
            "--repository",
            "repo",
            "--database",
            "db",
            "--unknown",
        ],
    ] {
        let loader = FakeConfigurationLoader {
            calls: Cell::new(0),
            outcome: Ok(default_configuration()),
        };
        let inspector = FakeDoctorInspector {
            calls: Cell::new(0),
            saw_targets: Cell::new(false),
            outcome: config_only_report(),
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run_doctor_with_inspector(
            arguments.into_iter().map(OsString::from),
            &mut stdout,
            &mut stderr,
            &loader,
            &inspector,
        );

        assert_eq!(code, EXIT_USAGE);
        assert_eq!(loader.calls.get(), 0);
        assert_eq!(inspector.calls.get(), 0);
        assert!(stdout.is_empty());
        assert!(!stderr.is_empty());
    }
}

#[test]
fn parsed_targets_are_passed_only_after_configuration_load_succeeds() {
    let configuration = default_configuration();
    let loader = FakeConfigurationLoader {
        calls: Cell::new(0),
        outcome: Ok(configuration),
    };
    let inspector = FakeDoctorInspector {
        calls: Cell::new(0),
        saw_targets: Cell::new(false),
        outcome: config_only_report(),
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let code = run_doctor_with_inspector(
        [
            OsString::from("--repository"),
            OsString::from("../secret-repository"),
            OsString::from("--database"),
            OsString::from("../secret-database"),
        ]
        .into_iter(),
        &mut stdout,
        &mut stderr,
        &loader,
        &inspector,
    );

    assert_eq!(code, EXIT_SUCCESS);
    assert_eq!(loader.calls.get(), 1);
    assert_eq!(inspector.calls.get(), 1);
    assert!(inspector.saw_targets.get());
    assert!(stderr.is_empty());
    let output = String::from_utf8(stdout).expect("report should be UTF-8");
    assert!(!output.contains("secret"));
}

#[test]
fn configuration_failure_skips_target_inspection_and_is_path_free() {
    let loader = FakeConfigurationLoader {
        calls: Cell::new(0),
        outcome: Err(ConfigurationLoadError::Invalid),
    };
    let inspector = FakeDoctorInspector {
        calls: Cell::new(0),
        saw_targets: Cell::new(false),
        outcome: config_only_report(),
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let code = run_doctor_with_inspector(
        [
            OsString::from("--repository"),
            OsString::from("../secret-repository"),
            OsString::from("--database"),
            OsString::from("../secret-database"),
        ]
        .into_iter(),
        &mut stdout,
        &mut stderr,
        &loader,
        &inspector,
    );

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(loader.calls.get(), 1);
    assert_eq!(inspector.calls.get(), 0);
    assert!(stderr.is_empty());
    let output = String::from_utf8(stdout).expect("report should be UTF-8");
    assert!(output.contains("check_configuration=error\n"));
    assert!(output.contains("check_repository_capability=not_run\n"));
    assert!(output.contains("database_state=unavailable\n"));
    assert!(!output.contains("secret"));
}

#[test]
fn doctor_help_documents_paired_targets_and_output_failure_is_reported() {
    let loader = FakeConfigurationLoader {
        calls: Cell::new(0),
        outcome: Ok(default_configuration()),
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let help_code = run_doctor(
        [OsString::from("--help")].into_iter(),
        &mut stdout,
        &mut stderr,
        &loader,
    );

    assert_eq!(help_code, EXIT_SUCCESS);
    assert_eq!(loader.calls.get(), 0);
    let help = String::from_utf8(stdout).expect("help should be UTF-8");
    assert!(help.contains("[--repository <path> --database <path>]"));

    let mut failing = FailingWriter;
    let output_code = run_doctor(std::iter::empty(), &mut failing, &mut Vec::new(), &loader);
    assert_eq!(output_code, EXIT_IO);
}
