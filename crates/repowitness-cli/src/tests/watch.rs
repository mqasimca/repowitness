use std::{
    cell::{Cell, RefCell},
    sync::atomic::Ordering,
    time::Duration,
};

use super::*;

struct StubWatchConfigurationLoader {
    calls: Cell<u64>,
    invocation: RefCell<Option<ConfigurationInvocation>>,
    outcome: Result<ResolvedConfiguration, ConfigurationLoadError>,
}

impl ConfigurationLoader for StubWatchConfigurationLoader {
    fn load(
        &self,
        invocation: &ConfigurationInvocation,
    ) -> Result<ResolvedConfiguration, ConfigurationLoadError> {
        self.calls.set(self.calls.get() + 1);
        self.invocation.replace(Some(invocation.clone()));
        self.outcome.clone()
    }
}

struct RecordingWatchLauncher {
    calls: Cell<u64>,
    root: RefCell<Option<PathBuf>>,
    database: RefCell<Option<PathBuf>>,
    repository_identity: RefCell<Option<OsString>>,
    max_runtime: Cell<Option<Duration>>,
    configuration: RefCell<Option<ResolvedConfiguration>>,
    outcome: RefCell<Option<Result<CliWatchReport, WatchLaunchError>>>,
}

impl RecordingWatchLauncher {
    fn success(report: CliWatchReport) -> Self {
        Self {
            calls: Cell::new(0),
            root: RefCell::new(None),
            database: RefCell::new(None),
            repository_identity: RefCell::new(None),
            max_runtime: Cell::new(None),
            configuration: RefCell::new(None),
            outcome: RefCell::new(Some(Ok(report))),
        }
    }

    fn failure(error: WatchLaunchError) -> Self {
        Self {
            calls: Cell::new(0),
            root: RefCell::new(None),
            database: RefCell::new(None),
            repository_identity: RefCell::new(None),
            max_runtime: Cell::new(None),
            configuration: RefCell::new(None),
            outcome: RefCell::new(Some(Err(error))),
        }
    }
}

impl WatchLauncher for RecordingWatchLauncher {
    fn launch(
        &self,
        invocation: WatchInvocation,
        configuration: ResolvedConfiguration,
    ) -> Result<CliWatchReport, WatchLaunchError> {
        self.calls.set(self.calls.get() + 1);
        self.root.replace(Some(invocation.repository_root));
        self.database.replace(Some(invocation.database));
        self.repository_identity
            .replace(Some(invocation.repository_identity));
        self.max_runtime.set(invocation.max_runtime);
        self.configuration.replace(Some(configuration));
        self.outcome
            .borrow_mut()
            .take()
            .expect("one launcher outcome")
    }
}

fn watch_arguments() -> Vec<OsString> {
    vec![
        OsString::from("repowitness"),
        OsString::from("watch"),
        OsString::from("--repository-id"),
        OsString::from(format!("rwi1:h:{}", "AB".repeat(32))),
        OsString::from("--database"),
        OsString::from("../private-index.sqlite3"),
        OsString::from("--max-runtime-ms"),
        OsString::from("1234"),
        OsString::from("--"),
        OsString::from("../private-repository"),
    ]
}

fn watch_report() -> CliWatchReport {
    CliWatchReport {
        configuration_sha256: "11".repeat(32),
        exit: "cancelled",
        reconciliations_started: 1,
        reconciliations_completed: 1,
        retryable_failures: 0,
        coalesced_observations: 0,
        backpressure_observations: 0,
        clock_regressions: 0,
        observed_events: 0,
        duplicate_hints: 0,
        coalesced_hints: 0,
        overflow_events: 0,
        unsupported_events: 0,
        last_reconciliation: "published",
        last_generation: Some(7),
        last_source_epoch: Some(2),
    }
}

#[test]
fn watch_parser_accepts_one_exact_foreground_invocation_and_redacts_debug() {
    let invocation =
        parse_watch_arguments(&watch_arguments()[2..]).expect("complete watch invocation");
    assert_eq!(
        invocation.repository_root,
        Path::new("../private-repository")
    );
    assert_eq!(invocation.database, Path::new("../private-index.sqlite3"));
    assert_eq!(invocation.max_runtime, Some(Duration::from_millis(1234)));
    let debug = format!("{invocation:?}");
    assert!(!debug.contains("private"));
    assert!(!debug.contains("rwi1:"));
}

#[test]
fn watch_parser_accepts_the_documented_optional_separator() {
    let mut arguments = watch_arguments();
    arguments.remove(arguments.len() - 2);

    let invocation =
        parse_watch_arguments(&arguments[2..]).expect("separator-free watch invocation");

    assert_eq!(
        invocation.repository_root,
        Path::new("../private-repository")
    );
    assert_eq!(invocation.database, Path::new("../private-index.sqlite3"));
}

#[test]
fn help_and_invalid_arguments_never_load_configuration_or_launch_work() {
    let configuration = resolve_configuration(&[]).expect("default configuration");
    for arguments in [
        vec!["repowitness", "watch", "--help"],
        vec!["repowitness", "watch"],
        vec!["repowitness", "watch", "../private-repository"],
        vec![
            "repowitness",
            "watch",
            "--repository-id",
            "invalid-private-identity",
            "--database",
            "../private-index.sqlite3",
            "../private-repository",
        ],
        vec![
            "repowitness",
            "watch",
            "--max-runtime-ms",
            "0",
            "--repository-id",
            "invalid",
        ],
        vec!["repowitness", "watch", "--unknown", "private"],
    ] {
        let loader = StubWatchConfigurationLoader {
            calls: Cell::new(0),
            invocation: RefCell::new(None),
            outcome: Ok(configuration.clone()),
        };
        let launcher = RecordingWatchLauncher::failure(WatchLaunchError::Operation);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run_watch_with_adapters(
            arguments.into_iter().map(OsString::from),
            &mut stdout,
            &mut stderr,
            &loader,
            &launcher,
        );
        if stdout.is_empty() {
            assert_eq!(code, EXIT_USAGE);
            let diagnostic = String::from_utf8(stderr).expect("UTF-8 diagnostic");
            assert!(diagnostic.starts_with("error:"));
            assert!(!diagnostic.contains("private"));
        } else {
            assert_eq!(code, EXIT_SUCCESS);
            assert!(stderr.is_empty());
            assert!(
                String::from_utf8(stdout)
                    .expect("UTF-8 help")
                    .contains("never detaches")
            );
        }
        assert_eq!(loader.calls.get(), 0);
        assert_eq!(launcher.calls.get(), 0);
    }
}

#[test]
fn watch_resolves_configuration_then_emits_one_path_free_receipt() {
    let configuration = resolve_configuration(&[]).expect("default configuration");
    let loader = StubWatchConfigurationLoader {
        calls: Cell::new(0),
        invocation: RefCell::new(None),
        outcome: Ok(configuration.clone()),
    };
    let launcher = RecordingWatchLauncher::success(watch_report());
    let mut arguments = watch_arguments();
    arguments.splice(
        2..2,
        [
            OsString::from("--user-config"),
            OsString::from("../private-user.toml"),
        ],
    );
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let code = run_watch_with_adapters(arguments, &mut stdout, &mut stderr, &loader, &launcher);

    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert_eq!(loader.calls.get(), 1);
    assert_eq!(launcher.calls.get(), 1);
    assert_eq!(
        launcher.max_runtime.get(),
        Some(Duration::from_millis(1234))
    );
    assert_eq!(
        launcher
            .configuration
            .borrow()
            .as_ref()
            .map(ResolvedConfiguration::digest),
        Some(configuration.digest())
    );
    assert_eq!(
        loader
            .invocation
            .borrow()
            .as_ref()
            .and_then(|invocation| invocation.user.as_deref()),
        Some(Path::new("../private-user.toml"))
    );
    let report = String::from_utf8(stdout).expect("UTF-8 report");
    assert!(report.starts_with("status=ok\noperation=watch\nschema_version=1\n"));
    assert!(report.contains("exit=cancelled\n"));
    assert!(report.contains("reconciliations_started=1\n"));
    assert!(report.contains("last_reconciliation=published\n"));
    assert!(report.contains("last_generation=7\n"));
    assert!(!report.contains("private"));
    assert!(!report.contains("rwi1:"));
}

#[test]
fn configuration_and_launcher_failures_are_generic_and_path_free() {
    let loader = StubWatchConfigurationLoader {
        calls: Cell::new(0),
        invocation: RefCell::new(None),
        outcome: Err(ConfigurationLoadError::Invalid),
    };
    let launcher = RecordingWatchLauncher::failure(WatchLaunchError::Operation);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_watch_with_adapters(
        watch_arguments(),
        &mut stdout,
        &mut stderr,
        &loader,
        &launcher,
    );
    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"error: configuration resolution failed\n");
    assert_eq!(launcher.calls.get(), 0);

    let loader = StubWatchConfigurationLoader {
        calls: Cell::new(0),
        invocation: RefCell::new(None),
        outcome: Ok(resolve_configuration(&[]).expect("default configuration")),
    };
    let launcher = RecordingWatchLauncher::failure(WatchLaunchError::ShutdownTimeout);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_watch_with_adapters(
        watch_arguments(),
        &mut stdout,
        &mut stderr,
        &loader,
        &launcher,
    );
    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"error: watch failed\n");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_signal_sets_shared_cancellation_and_awaits_cooperative_exit() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&cancelled);
    let task = tokio::task::spawn_blocking(move || {
        while !observed.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        Ok(watch_report())
    });

    let report = supervise_watch_task(
        task,
        cancelled.clone(),
        std::future::ready(Ok(())),
        Duration::from_secs(1),
    )
    .await
    .expect("first signal stops work");

    assert!(cancelled.load(Ordering::Acquire));
    assert_eq!(report, watch_report());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cooperative_shutdown_timeout_is_bounded_and_categorical() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let task = tokio::task::spawn_blocking(move || {
        std::thread::sleep(Duration::from_millis(25));
        Ok(watch_report())
    });

    let error = supervise_watch_task(
        task,
        cancelled.clone(),
        std::future::ready(Ok(())),
        Duration::from_millis(1),
    )
    .await
    .expect_err("unresponsive work reaches the bounded timeout");

    assert!(cancelled.load(Ordering::Acquire));
    assert_eq!(error, WatchLaunchError::ShutdownTimeout);
}
