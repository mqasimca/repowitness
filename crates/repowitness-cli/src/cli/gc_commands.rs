use std::time::Duration as GcDuration;

const GC_RUNTIME_WORKER_THREADS: usize = 1;
const GC_RUNTIME_BLOCKING_THREADS: usize = 2;
const GC_COOPERATIVE_SHUTDOWN: GcDuration = GcDuration::from_secs(5);
const GC_RUNTIME_SHUTDOWN: GcDuration = GcDuration::from_millis(250);
const MAX_GC_PIN_OPTIONS: usize = 128;
const MAX_GC_ARGUMENTS: usize =
    1 + 2 + 2 + 2 + (MAX_GC_PIN_OPTIONS * 2) + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_GC_TIMEOUT_MS: u64 = 300_000;
const GC_APPLY_OUTCOME_UNKNOWN: &str = concat!(
    "error: gc apply outcome is unknown; the apply may have committed. Do not create a new plan ",
    "or change pins/configuration. Re-run the identical gc apply command with the same plan ",
    "digest to recover the authoritative receipt.\n",
);

const GC_HELP: &str = concat!(
    "Plan or explicitly apply bounded generation retention.\n\n",
    "Usage:\n",
    "  repowitness gc plan --database <path> [retention options]\n",
    "  repowitness gc apply --database <path> --plan-digest <64-lowercase-hex>\n",
    "      [retention options]\n\n",
    "Retention options:\n",
    "  --timeout-ms <1-300000>\n",
    "  --pin-generation <positive-database-id> ...\n",
    "  --pin-workspace-view <positive-database-id> ...\n",
    "  --user-config <path> --workspace-config <path> --repository-config <path>\n\n",
    "Plan is deterministic and emits only aggregate counts, estimates, and digests.\n",
    "Apply recomputes current roots and candidates, requires the exact prior plan\n",
    "digest plus the same effective retention policy and pins, and rejects stale\n",
    "plans without deletion. If an apply outcome is unknown, rerun the identical\n",
    "apply with the same digest, policy, and pins before any other maintenance.\n",
    "Index and watch never invoke retention automatically.\n",
);

/// Parses and runs one explicit local generation-retention command.
pub fn run_gc(
    args: impl IntoIterator<Item = OsString>,
    mut stdout: impl Write,
    mut stderr: impl Write,
) -> u8 {
    run_gc_with_adapters(
        args,
        &mut stdout,
        &mut stderr,
        &LocalConfigurationLoader,
        &TokioGcLauncher,
    )
}

fn run_gc_with_adapters(
    args: impl IntoIterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    configuration_loader: &impl ConfigurationLoader,
    launcher: &impl GcLauncher,
) -> u8 {
    let mut args = args.into_iter();
    let _program = args.next();
    if args.next().as_deref() != Some(OsStr::new("gc")) {
        return emit_error(stderr, EXIT_USAGE, "error: expected gc command\n");
    }
    let arguments = args.take(MAX_GC_ARGUMENTS + 1).collect::<Vec<_>>();
    if arguments.len() > MAX_GC_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: gc received too many arguments; use gc --help\n",
        );
    }
    if gc_help_requested(&arguments) {
        return emit_output(stdout, GC_HELP);
    }
    let (arguments, configuration_invocation) =
        match extract_configuration_arguments(&arguments, &[]) {
            Ok(parsed) => parsed,
            Err(message) => return emit_error(stderr, EXIT_USAGE, message),
        };
    let invocation = match parse_gc_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let configuration = match configuration_loader.load(&configuration_invocation) {
        Ok(configuration) => configuration,
        Err(_) => {
            return emit_error(
                stderr,
                EXIT_SOFTWARE,
                "error: configuration resolution failed\n",
            );
        }
    };
    match launcher.launch(invocation, configuration) {
        Ok(report) => emit_gc_report(stdout, &report),
        Err(GcLaunchError::OutcomeUnknown) => {
            emit_error(stderr, EXIT_SOFTWARE, GC_APPLY_OUTCOME_UNKNOWN)
        }
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: gc operation failed\n"),
    }
}

fn gc_help_requested(arguments: &[OsString]) -> bool {
    matches!(arguments, [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
        || matches!(
            arguments,
            [subcommand, help]
                if (subcommand == OsStr::new("plan") || subcommand == OsStr::new("apply"))
                    && (help == OsStr::new("--help") || help == OsStr::new("-h"))
        )
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum GcOperation {
    Plan,
    Apply { expected_plan_digest: [u8; 32] },
}

impl std::fmt::Debug for GcOperation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Plan => formatter.write_str("Plan"),
            Self::Apply { .. } => formatter
                .debug_struct("Apply")
                .field("expected_plan_digest_bytes", &32)
                .finish(),
        }
    }
}

struct GcInvocation {
    operation: GcOperation,
    database: PathBuf,
    timeout: GcDuration,
    pins: LocalRetentionPins,
}

impl std::fmt::Debug for GcInvocation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GcInvocation")
            .field("operation", &self.operation)
            .field("database", &"<redacted-path>")
            .field("timeout", &self.timeout)
            .field("pins", &self.pins)
            .finish()
    }
}

fn parse_gc_arguments(arguments: &[OsString]) -> Result<GcInvocation, &'static str> {
    let (subcommand, remaining) = arguments
        .split_first()
        .ok_or("error: gc requires plan or apply; use gc --help\n")?;
    let apply = if subcommand == OsStr::new("plan") {
        false
    } else if subcommand == OsStr::new("apply") {
        true
    } else {
        return Err("error: gc requires plan or apply; use gc --help\n");
    };
    let mut database = None;
    let mut timeout = None;
    let mut expected_plan_digest = None;
    let mut generation_pins = Vec::new();
    let mut workspace_view_pins = Vec::new();
    let mut index = 0_usize;
    while index < remaining.len() {
        let option = &remaining[index];
        let value = remaining
            .get(index + 1)
            .ok_or("error: gc option requires a value; use gc --help\n")?;
        if option == OsStr::new("--database") {
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: gc accepts --database only once\n");
            }
        } else if option == OsStr::new("--timeout-ms") {
            if timeout.is_some() {
                return Err("error: gc accepts --timeout-ms only once\n");
            }
            timeout = Some(parse_gc_timeout(value)?);
        } else if option == OsStr::new("--plan-digest") {
            if expected_plan_digest
                .replace(parse_plan_digest(value)?)
                .is_some()
            {
                return Err("error: gc accepts --plan-digest only once\n");
            }
        } else if option == OsStr::new("--pin-generation") {
            generation_pins.push(parse_positive_database_id(value)?);
        } else if option == OsStr::new("--pin-workspace-view") {
            workspace_view_pins.push(parse_positive_database_id(value)?);
        } else {
            return Err("error: unknown gc option; use gc --help\n");
        }
        if generation_pins.len() + workspace_view_pins.len() > MAX_GC_PIN_OPTIONS {
            return Err("error: gc pin count exceeds the command limit\n");
        }
        index += 2;
    }
    let database = database.ok_or("error: gc requires --database; use gc --help\n")?;
    if database.as_os_str().is_empty() {
        return Err("error: gc database path must not be empty\n");
    }
    let operation = match (apply, expected_plan_digest) {
        (false, None) => GcOperation::Plan,
        (false, Some(_)) => return Err("error: gc plan does not accept --plan-digest\n"),
        (true, Some(expected_plan_digest)) => GcOperation::Apply {
            expected_plan_digest,
        },
        (true, None) => {
            return Err("error: gc apply requires --plan-digest; use gc --help\n");
        }
    };
    let pins = LocalRetentionPins::try_new(generation_pins, Vec::new(), workspace_view_pins)
        .map_err(|_| "error: gc pins are invalid\n")?;
    Ok(GcInvocation {
        operation,
        database,
        timeout: timeout.unwrap_or(repowitness_local::DEFAULT_LOCAL_RETENTION_TIMEOUT),
        pins,
    })
}

fn parse_gc_timeout(value: &OsStr) -> Result<GcDuration, &'static str> {
    let text = canonical_positive_decimal(value)
        .ok_or("error: gc --timeout-ms must be an integer from 1 through 300000\n")?;
    let milliseconds = text
        .parse::<u64>()
        .ok()
        .filter(|value| *value <= MAX_GC_TIMEOUT_MS)
        .ok_or("error: gc --timeout-ms must be an integer from 1 through 300000\n")?;
    Ok(GcDuration::from_millis(milliseconds))
}

fn parse_positive_database_id(value: &OsStr) -> Result<i64, &'static str> {
    canonical_positive_decimal(value)
        .and_then(|text| text.parse::<i64>().ok())
        .ok_or("error: gc pin must be one positive database-local integer\n")
}

fn canonical_positive_decimal(value: &OsStr) -> Option<&str> {
    let text = value.to_str()?;
    let bytes = text.as_bytes();
    (!bytes.is_empty() && bytes[0] != b'0' && bytes.iter().all(u8::is_ascii_digit)).then_some(text)
}

fn parse_plan_digest(value: &OsStr) -> Result<[u8; 32], &'static str> {
    let text = value
        .to_str()
        .filter(|text| text.len() == 64)
        .filter(|text| {
            text.bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or("error: gc plan digest must be exactly 64 lowercase hexadecimal characters\n")?;
    let mut digest = [0_u8; 32];
    for (index, output) in digest.iter_mut().enumerate() {
        let offset = index * 2;
        *output =
            (hex_nibble(text.as_bytes()[offset]) << 4) | hex_nibble(text.as_bytes()[offset + 1]);
    }
    Ok(digest)
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => 0,
    }
}

trait GcLauncher {
    fn launch(
        &self,
        invocation: GcInvocation,
        configuration: ResolvedConfiguration,
    ) -> Result<CliGcReport, GcLaunchError>;
}

struct TokioGcLauncher;

impl GcLauncher for TokioGcLauncher {
    fn launch(
        &self,
        invocation: GcInvocation,
        configuration: ResolvedConfiguration,
    ) -> Result<CliGcReport, GcLaunchError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(GC_RUNTIME_WORKER_THREADS)
            .max_blocking_threads(GC_RUNTIME_BLOCKING_THREADS)
            .enable_all()
            .build()
            .map_err(|_| GcLaunchError::RuntimeInitialization)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let destructive = matches!(invocation.operation, GcOperation::Apply { .. });
        let result = runtime.block_on(async move {
            let task = tokio::task::spawn_blocking(move || {
                run_local_gc(invocation, configuration, worker_cancelled)
            });
            supervise_gc_task(
                task,
                cancelled,
                first_shutdown_signal(),
                GC_COOPERATIVE_SHUTDOWN,
                destructive,
            )
            .await
        });
        runtime.shutdown_timeout(GC_RUNTIME_SHUTDOWN);
        result
    }
}

fn run_local_gc(
    invocation: GcInvocation,
    configuration: ResolvedConfiguration,
    cancelled: Arc<AtomicBool>,
) -> Result<CliGcReport, GcLaunchError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| GcLaunchError::Operation)?;
    let applied_at_unix_ms = u64::try_from(elapsed.as_millis())
        .map_err(|_| GcLaunchError::Operation)?;
    match invocation.operation {
        GcOperation::Plan => {
            let request = LocalRetentionPlanRequest::try_new(
                &invocation.database,
                applied_at_unix_ms,
                &configuration,
                invocation.pins,
                cancelled,
                invocation.timeout,
            )
            .map_err(|_| GcLaunchError::Operation)?;
            plan_local_retention(request)
                .map(CliGcReport::from_local_plan)
                .map_err(|_| GcLaunchError::Operation)
        }
        GcOperation::Apply {
            expected_plan_digest,
        } => {
            let request = LocalRetentionApplyRequest::try_new(
                &invocation.database,
                applied_at_unix_ms,
                &configuration,
                invocation.pins,
                expected_plan_digest,
                cancelled,
                invocation.timeout,
            )
            .map_err(|_| GcLaunchError::Operation)?;
            apply_local_retention(request)
                .map(CliGcReport::from_local_apply)
                .map_err(|error| {
                    if error.kind() == repowitness_local::LocalRetentionErrorKind::OutcomeUnknown {
                        GcLaunchError::OutcomeUnknown
                    } else {
                        GcLaunchError::Operation
                    }
                })
        }
    }
}

async fn supervise_gc_task<F>(
    mut task: tokio::task::JoinHandle<Result<CliGcReport, GcLaunchError>>,
    cancelled: Arc<AtomicBool>,
    signal: F,
    shutdown_timeout: GcDuration,
    destructive: bool,
) -> Result<CliGcReport, GcLaunchError>
where
    F: std::future::Future<Output = Result<(), WatchSignalError>>,
{
    tokio::pin!(signal);
    tokio::select! {
        biased;
        signal = &mut signal => {
            cancelled.store(true, std::sync::atomic::Ordering::Release);
            let signal = signal.map_err(|_| GcLaunchError::Signal);
            if destructive {
                task.await
                    .map_err(|_| GcLaunchError::Worker)?
            } else {
                let stopped = tokio::time::timeout(shutdown_timeout, &mut task)
                    .await
                    .map_err(|_| GcLaunchError::ShutdownTimeout)?
                    .map_err(|_| GcLaunchError::Worker)?;
                signal.and(stopped)
            }
        }
        result = &mut task => result
            .map_err(|_| GcLaunchError::Worker)?,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GcLaunchError {
    RuntimeInitialization,
    Signal,
    Worker,
    Operation,
    OutcomeUnknown,
    ShutdownTimeout,
}
