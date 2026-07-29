use std::{
    future::Future,
    sync::atomic::Ordering,
    time::Duration,
};

const WATCH_RUNTIME_WORKER_THREADS: usize = 1;
const WATCH_RUNTIME_BLOCKING_THREADS: usize = 2;
const WATCH_COOPERATIVE_SHUTDOWN: Duration = Duration::from_secs(5);
const WATCH_RUNTIME_SHUTDOWN: Duration = Duration::from_millis(250);
const MAX_WATCH_RUNTIME_MS: u64 = 86_400_000;

/// Parses and runs the foreground polling watch command.
pub fn run_watch(
    args: impl IntoIterator<Item = OsString>,
    mut stdout: impl Write,
    mut stderr: impl Write,
) -> u8 {
    run_watch_with_adapters(
        args,
        &mut stdout,
        &mut stderr,
        &LocalConfigurationLoader,
        &TokioWatchLauncher,
    )
}

fn run_watch_with_adapters(
    args: impl IntoIterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    configuration_loader: &impl ConfigurationLoader,
    launcher: &impl WatchLauncher,
) -> u8 {
    let mut args = args.into_iter();
    let _program = args.next();
    if args.next().as_deref() != Some(OsStr::new("watch")) {
        return emit_error(stderr, EXIT_USAGE, "error: expected watch command\n");
    }
    let arguments = args.take(MAX_WATCH_ARGUMENTS + 1).collect::<Vec<_>>();
    if arguments.len() > MAX_WATCH_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: watch received too many arguments; use watch --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, WATCH_HELP);
    }
    let (arguments, configuration_invocation) =
        match extract_configuration_arguments(&arguments, &[]) {
            Ok(parsed) => parsed,
            Err(message) => return emit_error(stderr, EXIT_USAGE, message),
        };
    let invocation = match parse_watch_arguments(&arguments) {
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
        Ok(report) => emit_watch_report(stdout, &report),
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: watch failed\n"),
    }
}

struct WatchInvocation {
    repository_root: PathBuf,
    database: PathBuf,
    repository_identity: OsString,
    max_runtime: Option<Duration>,
}

impl std::fmt::Debug for WatchInvocation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WatchInvocation")
            .field("repository_root", &"<redacted-path>")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("max_runtime", &self.max_runtime)
            .finish()
    }
}

fn parse_watch_arguments(arguments: &[OsString]) -> Result<WatchInvocation, &'static str> {
    let mut repository_identity = None;
    let mut database = None;
    let mut repository_root = None;
    let mut max_runtime = None;
    let mut positional_only = false;
    let mut index = 0_usize;
    while index < arguments.len() {
        let argument = &arguments[index];
        if positional_only {
            set_watch_repository_root(&mut repository_root, argument)?;
            index += 1;
            continue;
        }
        if argument == OsStr::new("--") {
            positional_only = true;
            index += 1;
            continue;
        }
        let takes_value = argument == OsStr::new("--repository-id")
            || argument == OsStr::new("--database")
            || argument == OsStr::new("--max-runtime-ms");
        if !takes_value {
            if os_string_starts_with_hyphen(argument) {
                return Err("error: unknown watch option; use watch --help\n");
            }
            set_watch_repository_root(&mut repository_root, argument)?;
            index += 1;
            continue;
        }
        let value = arguments
            .get(index + 1)
            .ok_or("error: watch option requires a value; use watch --help\n")?;
        if argument == OsStr::new("--repository-id") {
            if repository_identity.replace(value.clone()).is_some() {
                return Err("error: watch accepts --repository-id only once\n");
            }
        } else if argument == OsStr::new("--database") {
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: watch accepts --database only once\n");
            }
        } else {
            if max_runtime.is_some() {
                return Err("error: watch accepts --max-runtime-ms only once\n");
            }
            let millis = value
                .to_str()
                .and_then(|text| text.parse::<u64>().ok())
                .filter(|millis| (1..=MAX_WATCH_RUNTIME_MS).contains(millis))
                .ok_or(
                    "error: watch --max-runtime-ms must be an integer from 1 through 86400000\n",
                )?;
            max_runtime = Some(Duration::from_millis(millis));
        }
        index += 2;
    }
    let repository_identity = repository_identity
        .ok_or("error: watch requires --repository-id; use watch --help\n")?;
    let repository_identity_text = repository_identity
        .to_str()
        .ok_or("error: watch repository identity must be UTF-8\n")?;
    RepositoryIdentityTextV1::decode(repository_identity_text)
        .map_err(|_| "error: watch repository identity is invalid\n")?;
    let database = database.ok_or("error: watch requires --database; use watch --help\n")?;
    if database.as_os_str().is_empty() {
        return Err("error: watch database path must not be empty\n");
    }
    let repository_root =
        repository_root.ok_or("error: watch requires one repository path; use watch --help\n")?;
    Ok(WatchInvocation {
        repository_root,
        database,
        repository_identity,
        max_runtime,
    })
}

fn set_watch_repository_root(
    repository_root: &mut Option<PathBuf>,
    value: &OsStr,
) -> Result<(), &'static str> {
    if value.is_empty() {
        return Err("error: watch repository path must not be empty\n");
    }
    if repository_root.replace(PathBuf::from(value)).is_some() {
        return Err("error: watch accepts exactly one repository path\n");
    }
    Ok(())
}

trait WatchLauncher {
    fn launch(
        &self,
        invocation: WatchInvocation,
        configuration: ResolvedConfiguration,
    ) -> Result<CliWatchReport, WatchLaunchError>;
}

struct TokioWatchLauncher;

impl WatchLauncher for TokioWatchLauncher {
    fn launch(
        &self,
        invocation: WatchInvocation,
        configuration: ResolvedConfiguration,
    ) -> Result<CliWatchReport, WatchLaunchError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(WATCH_RUNTIME_WORKER_THREADS)
            .max_blocking_threads(WATCH_RUNTIME_BLOCKING_THREADS)
            .enable_all()
            .build()
            .map_err(|_| WatchLaunchError::RuntimeInitialization)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let result = runtime.block_on(async move {
            let task = tokio::task::spawn_blocking(move || {
                run_local_watch(invocation, configuration, worker_cancelled)
            });
            supervise_watch_task(
                task,
                cancelled,
                first_shutdown_signal(),
                WATCH_COOPERATIVE_SHUTDOWN,
            )
            .await
        });
        runtime.shutdown_timeout(WATCH_RUNTIME_SHUTDOWN);
        result
    }
}

fn run_local_watch(
    invocation: WatchInvocation,
    configuration: ResolvedConfiguration,
    cancelled: Arc<AtomicBool>,
) -> Result<CliWatchReport, String> {
    let repository_identity = invocation
        .repository_identity
        .to_str()
        .ok_or_else(|| "repository identity is not valid UTF-8".to_owned())?;
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock is before the Unix epoch".to_owned())?;
    let applied_at_unix_ms = u64::try_from(elapsed.as_millis())
        .map_err(|_| "system clock is outside the supported range".to_owned())?;
    let index = LocalIndexRequest::new(
        &invocation.repository_root,
        &invocation.database,
        repository_identity,
        applied_at_unix_ms,
    )
    .with_configuration(&configuration);
    let mut request = LocalWatchRequest::new(index);
    if let Some(max_runtime) = invocation.max_runtime {
        request = request
            .with_max_runtime(max_runtime)
            .map_err(|error| error.to_string())?;
    }
    watch_local_repository(request, cancelled)
        .map(|report| CliWatchReport::from_local(report, &configuration))
        .map_err(|error| error.to_string())
}

async fn supervise_watch_task<F>(
    mut task: tokio::task::JoinHandle<Result<CliWatchReport, String>>,
    cancelled: Arc<AtomicBool>,
    signal: F,
    shutdown_timeout: Duration,
) -> Result<CliWatchReport, WatchLaunchError>
where
    F: Future<Output = Result<(), WatchSignalError>>,
{
    tokio::pin!(signal);
    tokio::select! {
        biased;
        signal = &mut signal => {
            cancelled.store(true, Ordering::Release);
            let signal = signal.map_err(|_| WatchLaunchError::Signal);
            let stopped = tokio::time::timeout(shutdown_timeout, &mut task)
                .await
                .map_err(|_| WatchLaunchError::ShutdownTimeout)?
                .map_err(|_| WatchLaunchError::Worker)?
                .map_err(|_| WatchLaunchError::Operation);
            signal.and(stopped)
        }
        result = &mut task => result
            .map_err(|_| WatchLaunchError::Worker)?
            .map_err(|_| WatchLaunchError::Operation),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WatchLaunchError {
    RuntimeInitialization,
    Signal,
    Worker,
    Operation,
    ShutdownTimeout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WatchSignalError {
    Registration,
    Closed,
}

#[cfg(unix)]
async fn first_shutdown_signal() -> Result<(), WatchSignalError> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut interrupt =
        signal(SignalKind::interrupt()).map_err(|_| WatchSignalError::Registration)?;
    let mut terminate =
        signal(SignalKind::terminate()).map_err(|_| WatchSignalError::Registration)?;
    tokio::select! {
        signal = interrupt.recv() => signal.ok_or(WatchSignalError::Closed),
        signal = terminate.recv() => signal.ok_or(WatchSignalError::Closed),
    }
}

#[cfg(windows)]
async fn first_shutdown_signal() -> Result<(), WatchSignalError> {
    use tokio::signal::windows::{ctrl_break, ctrl_c};

    let mut control_c = ctrl_c().map_err(|_| WatchSignalError::Registration)?;
    let mut control_break = ctrl_break().map_err(|_| WatchSignalError::Registration)?;
    tokio::select! {
        signal = control_c.recv() => signal.ok_or(WatchSignalError::Closed),
        signal = control_break.recv() => signal.ok_or(WatchSignalError::Closed),
    }
}

#[cfg(not(any(unix, windows)))]
async fn first_shutdown_signal() -> Result<(), WatchSignalError> {
    tokio::signal::ctrl_c()
        .await
        .map_err(|_| WatchSignalError::Registration)
}
