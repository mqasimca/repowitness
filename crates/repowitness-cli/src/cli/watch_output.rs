#[derive(Debug, Eq, PartialEq)]
struct CliWatchReport {
    configuration_sha256: String,
    exit: &'static str,
    reconciliations_started: u64,
    reconciliations_completed: u64,
    retryable_failures: u64,
    coalesced_observations: u64,
    backpressure_observations: u64,
    clock_regressions: u64,
    observed_events: u64,
    duplicate_hints: u64,
    coalesced_hints: u64,
    overflow_events: u64,
    unsupported_events: u64,
    last_reconciliation: &'static str,
    last_generation: Option<i64>,
    last_source_epoch: Option<u64>,
}

impl CliWatchReport {
    fn from_local(report: LocalWatchReport, configuration: &ResolvedConfiguration) -> Self {
        let state = report.state_counters();
        let hints = report.hint_counters();
        let last = report.last_index();
        Self {
            configuration_sha256: hex(configuration.digest().as_bytes()),
            exit: match report.exit() {
                LocalWatchExit::Cancelled => "cancelled",
                LocalWatchExit::DeadlineExceeded => "deadline_exceeded",
            },
            reconciliations_started: state.reconciliations_started(),
            reconciliations_completed: state.reconciliations_completed(),
            retryable_failures: state.retryable_failures(),
            coalesced_observations: state.coalesced_observations(),
            backpressure_observations: state.backpressure_observations(),
            clock_regressions: state.clock_regressions(),
            observed_events: hints.observed_events(),
            duplicate_hints: hints.duplicate_hints(),
            coalesced_hints: hints.coalesced_hints(),
            overflow_events: hints.overflow_events(),
            unsupported_events: hints.unsupported_events(),
            last_reconciliation: match report.last_reconciliation() {
                Some(LocalWatchReconciliation::Published) => "published",
                Some(LocalWatchReconciliation::Resumed) => "resumed",
                Some(LocalWatchReconciliation::Unchanged) => "unchanged",
                None => "none",
            },
            last_generation: last.map(|report| report.generation().get()),
            last_source_epoch: last.map(LocalIndexReport::source_epoch),
        }
    }
}

fn emit_watch_report(writer: &mut impl Write, report: &CliWatchReport) -> u8 {
    if !watch_report_is_consistent(report) {
        return EXIT_SOFTWARE;
    }
    let result = writeln!(writer, "status=ok")
        .and_then(|()| writeln!(writer, "operation=watch"))
        .and_then(|()| writeln!(writer, "schema_version=1"))
        .and_then(|()| {
            writeln!(
                writer,
                "watch_profile={}",
                repowitness_local::LOCAL_WATCH_PROFILE_VERSION
            )
        })
        .and_then(|()| {
            writeln!(
                writer,
                "configuration_sha256={}",
                report.configuration_sha256
            )
        })
        .and_then(|()| writeln!(writer, "exit={}", report.exit))
        .and_then(|()| {
            writeln!(
                writer,
                "reconciliations_started={}",
                report.reconciliations_started
            )
        })
        .and_then(|()| {
            writeln!(
                writer,
                "reconciliations_completed={}",
                report.reconciliations_completed
            )
        })
        .and_then(|()| writeln!(writer, "retryable_failures={}", report.retryable_failures))
        .and_then(|()| {
            writeln!(
                writer,
                "coalesced_observations={}",
                report.coalesced_observations
            )
        })
        .and_then(|()| {
            writeln!(
                writer,
                "backpressure_observations={}",
                report.backpressure_observations
            )
        })
        .and_then(|()| writeln!(writer, "clock_regressions={}", report.clock_regressions))
        .and_then(|()| writeln!(writer, "observed_events={}", report.observed_events))
        .and_then(|()| writeln!(writer, "duplicate_hints={}", report.duplicate_hints))
        .and_then(|()| writeln!(writer, "coalesced_hints={}", report.coalesced_hints))
        .and_then(|()| writeln!(writer, "overflow_events={}", report.overflow_events))
        .and_then(|()| writeln!(writer, "unsupported_events={}", report.unsupported_events))
        .and_then(|()| {
            writeln!(
                writer,
                "last_reconciliation={}",
                report.last_reconciliation
            )
        })
        .and_then(|()| write_optional_i64(writer, "last_generation", report.last_generation))
        .and_then(|()| {
            write_optional_u64(writer, "last_source_epoch", report.last_source_epoch)
        });
    if result.is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn watch_report_is_consistent(report: &CliWatchReport) -> bool {
    let has_last = report.last_generation.is_some() && report.last_source_epoch.is_some();
    report.reconciliations_completed <= report.reconciliations_started
        && (report.last_reconciliation == "none") == !has_last
        && (!has_last || report.reconciliations_completed > 0)
}

fn write_optional_i64(
    writer: &mut impl Write,
    key: &str,
    value: Option<i64>,
) -> std::io::Result<()> {
    match value {
        Some(value) => writeln!(writer, "{key}={value}"),
        None => writeln!(writer, "{key}=none"),
    }
}

fn write_optional_u64(
    writer: &mut impl Write,
    key: &str,
    value: Option<u64>,
) -> std::io::Result<()> {
    match value {
        Some(value) => writeln!(writer, "{key}={value}"),
        None => writeln!(writer, "{key}=none"),
    }
}
