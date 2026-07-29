use std::{
    cell::{Cell, RefCell},
    sync::atomic::Ordering,
    time::Duration,
};

use super::*;

struct StubGcConfigurationLoader {
    calls: Cell<u64>,
    invocation: RefCell<Option<ConfigurationInvocation>>,
    outcome: Result<ResolvedConfiguration, ConfigurationLoadError>,
}

impl ConfigurationLoader for StubGcConfigurationLoader {
    fn load(
        &self,
        invocation: &ConfigurationInvocation,
    ) -> Result<ResolvedConfiguration, ConfigurationLoadError> {
        self.calls.set(self.calls.get() + 1);
        self.invocation.replace(Some(invocation.clone()));
        self.outcome.clone()
    }
}

struct RecordingGcLauncher {
    calls: Cell<u64>,
    operation: Cell<Option<GcOperation>>,
    database: RefCell<Option<PathBuf>>,
    timeout: Cell<Option<Duration>>,
    generation_pin_count: Cell<Option<usize>>,
    workspace_view_pin_count: Cell<Option<usize>>,
    configuration: RefCell<Option<ResolvedConfiguration>>,
    outcome: RefCell<Option<Result<CliGcReport, GcLaunchError>>>,
}

impl RecordingGcLauncher {
    fn success(report: CliGcReport) -> Self {
        Self::with_outcome(Ok(report))
    }

    fn failure(error: GcLaunchError) -> Self {
        Self::with_outcome(Err(error))
    }

    fn with_outcome(outcome: Result<CliGcReport, GcLaunchError>) -> Self {
        Self {
            calls: Cell::new(0),
            operation: Cell::new(None),
            database: RefCell::new(None),
            timeout: Cell::new(None),
            generation_pin_count: Cell::new(None),
            workspace_view_pin_count: Cell::new(None),
            configuration: RefCell::new(None),
            outcome: RefCell::new(Some(outcome)),
        }
    }
}

impl GcLauncher for RecordingGcLauncher {
    fn launch(
        &self,
        invocation: GcInvocation,
        configuration: ResolvedConfiguration,
    ) -> Result<CliGcReport, GcLaunchError> {
        self.calls.set(self.calls.get() + 1);
        self.operation.set(Some(invocation.operation));
        self.database.replace(Some(invocation.database));
        self.timeout.set(Some(invocation.timeout));
        self.generation_pin_count
            .set(Some(invocation.pins.generation_pin_count()));
        self.workspace_view_pin_count
            .set(Some(invocation.pins.workspace_view_pin_count()));
        self.configuration.replace(Some(configuration));
        self.outcome
            .borrow_mut()
            .take()
            .expect("one GC launcher outcome")
    }
}

fn gc_policy() -> CliGcPolicy {
    CliGcPolicy {
        configuration_sha256: "11".repeat(32),
        policy_sha256: "22".repeat(32),
        retained_generations_per_source_slot: 2,
        max_generation_candidates: 64,
        max_rows: 1_000_000,
        max_bytes: 536_870_912,
        generation_pin_count: 1,
        workspace_view_pin_count: 1,
    }
}

fn gc_plan_report() -> CliGcReport {
    CliGcReport::Plan(CliGcPlanReport {
        policy: gc_policy(),
        plan_sha256: "33".repeat(32),
        candidate_count: 3,
        estimated_rows: 40,
        estimated_bytes: 4_096,
        root_count: 7,
        unresolved_count: 2,
        unresolved_truncated: true,
        logical_work_rows: 91,
        more_work: true,
    })
}

fn gc_apply_report(shutdown_complete: bool, database_identity_confirmed: bool) -> CliGcReport {
    CliGcReport::Apply(CliGcApplyReport {
        policy: gc_policy(),
        plan_sha256: "33".repeat(32),
        collection_id: 9,
        generation_count: 3,
        workspace_view_count: 2,
        source_slot_receipt_count: 4,
        snapshot_count: 3,
        artifact_count: 8,
        deleted_rows: 20,
        estimated_deleted_bytes: 4_096,
        more_work: false,
        shutdown_complete,
        database_identity_confirmed,
    })
}

fn valid_gc_arguments(subcommand: &str) -> Vec<OsString> {
    let mut arguments = vec![
        OsString::from("repowitness"),
        OsString::from("gc"),
        OsString::from(subcommand),
        OsString::from("--database"),
        OsString::from("../private-index.sqlite3"),
        OsString::from("--timeout-ms"),
        OsString::from("1234"),
        OsString::from("--pin-generation"),
        OsString::from("7"),
        OsString::from("--pin-workspace-view"),
        OsString::from("8"),
    ];
    if subcommand == "apply" {
        arguments.extend([
            OsString::from("--plan-digest"),
            OsString::from("33".repeat(32)),
        ]);
    }
    arguments
}

#[test]
fn parser_accepts_exact_plan_and_apply_grammar_and_redacts_debug() {
    let plan = parse_gc_arguments(&valid_gc_arguments("plan")[2..]).expect("valid plan invocation");
    assert_eq!(plan.operation, GcOperation::Plan);
    assert_eq!(plan.database, Path::new("../private-index.sqlite3"));
    assert_eq!(plan.timeout, Duration::from_millis(1234));
    assert_eq!(plan.pins.generation_pin_count(), 1);
    assert_eq!(plan.pins.workspace_view_pin_count(), 1);

    let apply =
        parse_gc_arguments(&valid_gc_arguments("apply")[2..]).expect("valid apply invocation");
    assert_eq!(
        apply.operation,
        GcOperation::Apply {
            expected_plan_digest: [0x33; 32]
        }
    );
    let debug = format!("{apply:?}");
    assert!(!debug.contains("private"));
    assert!(!debug.contains(&"33".repeat(32)));
}

#[test]
fn help_and_invalid_arguments_never_load_configuration_or_launch_work() {
    let mut too_many_pins = valid_gc_arguments("plan");
    for pin in 0..=MAX_GC_PIN_OPTIONS {
        too_many_pins.extend([
            OsString::from("--pin-generation"),
            OsString::from((pin + 1).to_string()),
        ]);
    }
    let invalid = [
        vec!["repowitness", "gc", "--help"],
        vec!["repowitness", "gc"],
        vec!["repowitness", "gc", "delete"],
        vec!["repowitness", "gc", "plan"],
        vec!["repowitness", "gc", "plan", "--database"],
        vec![
            "repowitness",
            "gc",
            "plan",
            "--database",
            "private",
            "--database",
            "other",
        ],
        vec![
            "repowitness",
            "gc",
            "plan",
            "--database",
            "private",
            "--timeout-ms",
            "0",
        ],
        vec![
            "repowitness",
            "gc",
            "plan",
            "--database",
            "private",
            "--timeout-ms",
            "01",
        ],
        vec![
            "repowitness",
            "gc",
            "plan",
            "--database",
            "private",
            "--pin-generation",
            "0",
        ],
        vec![
            "repowitness",
            "gc",
            "plan",
            "--database",
            "private",
            "--plan-digest",
            "33",
        ],
        vec!["repowitness", "gc", "apply", "--database", "private"],
        vec![
            "repowitness",
            "gc",
            "apply",
            "--database",
            "private",
            "--plan-digest",
            "AA",
        ],
        vec![
            "repowitness",
            "gc",
            "plan",
            "--database",
            "private",
            "--unknown",
            "value",
        ],
    ];
    for arguments in invalid {
        assert_invalid_gc(arguments.into_iter().map(OsString::from).collect());
    }
    assert_invalid_gc(too_many_pins);
}

fn assert_invalid_gc(arguments: Vec<OsString>) {
    let loader = StubGcConfigurationLoader {
        calls: Cell::new(0),
        invocation: RefCell::new(None),
        outcome: Ok(resolve_configuration(&[]).expect("default configuration")),
    };
    let launcher = RecordingGcLauncher::failure(GcLaunchError::Operation);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let code = run_gc_with_adapters(arguments, &mut stdout, &mut stderr, &loader, &launcher);

    if stdout.is_empty() {
        assert_eq!(code, EXIT_USAGE);
        assert!(
            String::from_utf8(stderr)
                .expect("UTF-8 diagnostic")
                .starts_with("error:")
        );
    } else {
        assert_eq!(code, EXIT_SUCCESS);
        assert!(stderr.is_empty());
    }
    assert_eq!(loader.calls.get(), 0);
    assert_eq!(launcher.calls.get(), 0);
}

#[test]
fn gc_resolves_configuration_then_emits_deterministic_path_free_plan_metrics() {
    let configuration = resolve_configuration(&[]).expect("default configuration");
    let loader = StubGcConfigurationLoader {
        calls: Cell::new(0),
        invocation: RefCell::new(None),
        outcome: Ok(configuration.clone()),
    };
    let launcher = RecordingGcLauncher::success(gc_plan_report());
    let mut arguments = valid_gc_arguments("plan");
    arguments.extend([
        OsString::from("--user-config"),
        OsString::from("../private-user.toml"),
    ]);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let code = run_gc_with_adapters(arguments, &mut stdout, &mut stderr, &loader, &launcher);

    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert_eq!(loader.calls.get(), 1);
    assert_eq!(launcher.calls.get(), 1);
    assert_eq!(launcher.operation.get(), Some(GcOperation::Plan));
    assert_eq!(
        launcher.database.borrow().as_deref(),
        Some(Path::new("../private-index.sqlite3"))
    );
    assert_eq!(launcher.timeout.get(), Some(Duration::from_millis(1234)));
    assert_eq!(launcher.generation_pin_count.get(), Some(1));
    assert_eq!(launcher.workspace_view_pin_count.get(), Some(1));
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
    let report = String::from_utf8(stdout).expect("UTF-8 GC report");
    assert!(report.starts_with("status=ok\noperation=gc_plan\nschema_version=1\n"));
    assert!(report.contains("candidate_count=3\n"));
    assert!(report.contains("estimated_rows=40\n"));
    assert!(report.contains("root_count=7\n"));
    assert!(report.contains("unresolved_candidate_count=2\n"));
    assert!(report.contains("unresolved_candidates_truncated=true\n"));
    assert!(report.contains("logical_work_rows=91\n"));
    assert!(report.ends_with("more_work=true\n"));
    assert!(!report.contains("private"));
}

#[test]
fn committed_apply_emits_only_aggregate_counts_and_explicit_warnings() {
    let report = gc_apply_report(false, false);
    let mut output = Vec::new();

    assert_eq!(emit_gc_report(&mut output, &report), EXIT_SUCCESS);

    let output = String::from_utf8(output).expect("UTF-8 GC report");
    assert!(output.starts_with("status=warning\noperation=gc_apply\nschema_version=1\n"));
    assert!(output.contains("collection_id=9\n"));
    assert!(output.contains("deleted_generations=3\n"));
    assert!(output.contains("deleted_workspace_views=2\n"));
    assert!(output.contains("deleted_source_slot_receipts=4\n"));
    assert!(output.contains("deleted_snapshots=3\n"));
    assert!(output.contains("deleted_artifacts=8\n"));
    assert!(output.contains("deleted_rows=20\n"));
    assert!(output.contains("maintenance_shutdown=incomplete\n"));
    assert!(output.contains("database_identity_fence=changed\n"));
    assert!(output.contains("warning_count=2\n"));
    assert!(output.contains("warning_0=committed_apply_shutdown_incomplete\n"));
    assert!(output.contains("warning_1=committed_apply_database_identity_changed\n"));
    assert!(!output.contains("private"));
}

#[test]
fn inconsistent_aggregate_report_is_rejected_before_output() {
    let CliGcReport::Plan(mut report) = gc_plan_report() else {
        unreachable!("fixture is a plan");
    };
    report.logical_work_rows = report.policy.max_rows + 1;
    let mut output = Vec::new();

    assert_eq!(
        emit_gc_report(&mut output, &CliGcReport::Plan(report)),
        EXIT_SOFTWARE
    );
    assert!(output.is_empty());
}

#[test]
fn configuration_and_launcher_failures_are_generic_and_path_free() {
    let loader = StubGcConfigurationLoader {
        calls: Cell::new(0),
        invocation: RefCell::new(None),
        outcome: Err(ConfigurationLoadError::Invalid),
    };
    let launcher = RecordingGcLauncher::failure(GcLaunchError::Operation);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_gc_with_adapters(
        valid_gc_arguments("plan"),
        &mut stdout,
        &mut stderr,
        &loader,
        &launcher,
    );
    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"error: configuration resolution failed\n");
    assert_eq!(launcher.calls.get(), 0);

    let loader = StubGcConfigurationLoader {
        calls: Cell::new(0),
        invocation: RefCell::new(None),
        outcome: Ok(resolve_configuration(&[]).expect("default configuration")),
    };
    let launcher = RecordingGcLauncher::failure(GcLaunchError::Worker);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_gc_with_adapters(
        valid_gc_arguments("apply"),
        &mut stdout,
        &mut stderr,
        &loader,
        &launcher,
    );
    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"error: gc operation failed\n");
    assert!(!String::from_utf8_lossy(&stderr).contains("private"));
}

#[test]
fn unknown_apply_outcome_requires_exact_authoritative_recovery() {
    let loader = StubGcConfigurationLoader {
        calls: Cell::new(0),
        invocation: RefCell::new(None),
        outcome: Ok(resolve_configuration(&[]).expect("default configuration")),
    };
    let launcher = RecordingGcLauncher::failure(GcLaunchError::OutcomeUnknown);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let code = run_gc_with_adapters(
        valid_gc_arguments("apply"),
        &mut stdout,
        &mut stderr,
        &loader,
        &launcher,
    );

    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        GC_APPLY_OUTCOME_UNKNOWN.as_bytes(),
        "the recovery guidance must remain exact and path-free"
    );
    assert!(!String::from_utf8_lossy(&stderr).contains("private"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn non_destructive_signal_shutdown_is_bounded_and_categorical() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let task = tokio::task::spawn_blocking(|| {
        std::thread::sleep(Duration::from_millis(25));
        Ok(gc_plan_report())
    });

    let error = supervise_gc_task(
        task,
        Arc::clone(&cancelled),
        std::future::ready(Ok(())),
        Duration::from_millis(1),
        false,
    )
    .await
    .expect_err("unresponsive read-only plan reaches the bounded timeout");

    assert!(cancelled.load(Ordering::Acquire));
    assert_eq!(error, GcLaunchError::ShutdownTimeout);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn destructive_signal_waits_for_definitive_committed_outcome() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let task = tokio::task::spawn_blocking(|| {
        std::thread::sleep(Duration::from_millis(20));
        Ok(gc_apply_report(true, true))
    });

    let report = supervise_gc_task(
        task,
        Arc::clone(&cancelled),
        std::future::ready(Err(WatchSignalError::Registration)),
        Duration::from_millis(1),
        true,
    )
    .await
    .expect("committed apply receipt wins over signal registration failure");

    assert!(cancelled.load(Ordering::Acquire));
    assert_eq!(report, gc_apply_report(true, true));
}
