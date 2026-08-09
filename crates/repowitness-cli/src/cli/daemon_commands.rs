#[cfg(unix)]
/// Parses and runs the foreground local catalog daemon command.
pub fn run_daemon(
    args: impl IntoIterator<Item = OsString>,
    mut stdout: impl Write,
    mut stderr: impl Write,
) -> u8 {
    let mut args = args.into_iter();
    let _program = args.next();
    if args.next().as_deref() != Some(OsStr::new("daemon")) {
        return emit_error(&mut stderr, EXIT_USAGE, "error: expected daemon command\n");
    }
    let arguments = args.take(MAX_DAEMON_ARGUMENTS + 1).collect::<Vec<_>>();
    if arguments.len() > MAX_DAEMON_ARGUMENTS {
        return emit_error(&mut stderr, EXIT_USAGE, "error: too many daemon arguments\n");
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(&mut stdout, DAEMON_HELP);
    }
    let (arguments, configuration_invocation) = match extract_configuration_arguments(
        &arguments,
        &["--catalog"],
    ) {
        Ok(parsed) => parsed,
        Err(message) => return emit_error(&mut stderr, EXIT_USAGE, message),
    };
    let state_directory = match parse_daemon_arguments(&arguments) {
        Ok(directory) => directory,
        Err(message) => return emit_error(&mut stderr, EXIT_USAGE, message),
    };
    let configuration = match LocalConfigurationLoader.load(&configuration_invocation) {
        Ok(configuration) => configuration,
        Err(_) => return emit_error(&mut stderr, EXIT_SOFTWARE, "error: configuration resolution failed\n"),
    };
    match launch_catalog_daemon(state_directory.as_deref(), configuration) {
        Ok(()) => EXIT_SUCCESS,
        Err(DaemonLaunchError::Unavailable) => emit_error(&mut stderr, EXIT_SOFTWARE, "error: local catalog daemon is unavailable\n"),
        Err(DaemonLaunchError::Runtime) => emit_error(&mut stderr, EXIT_SOFTWARE, "error: local catalog daemon runtime failed\n"),
    }
}

#[cfg(not(unix))]
/// Reports that the local catalog daemon is unavailable on this host.
pub fn run_daemon(
    _args: impl IntoIterator<Item = OsString>,
    _stdout: impl Write,
    mut stderr: impl Write,
) -> u8 {
    emit_error(&mut stderr, EXIT_SOFTWARE, "error: local catalog daemon is unavailable on this host\n")
}

#[cfg(unix)]
const DAEMON_HELP: &str = concat!(
    "Run one foreground Linux RepoWitness catalog daemon.\n\n",
    "Usage:\n",
    "  repowitness daemon --catalog [--catalog-state-dir <path>]\n",
    "      [--user-config <path>] [--workspace-config <path>]\n\n",
    "The daemon admits and indexes only its process-current Git worktree, then\n",
    "serves read-only MCP on a private Unix socket. Native filesystem events are\n",
    "hints only: every publication still uses complete reconciliation and a final\n",
    "source fence. Run it under a user service manager; it never backgrounds\n",
    "itself. Use mcp-serve --catalog --daemon from the same worktree to proxy\n",
    "Codex stdio MCP to this process.\n",
);

#[cfg(unix)]
fn parse_daemon_arguments(arguments: &[OsString]) -> Result<Option<PathBuf>, &'static str> {
    let mut catalog = false;
    let mut state_directory = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        if option == OsStr::new("--catalog") {
            if catalog { return Err("error: daemon accepts --catalog only once\n"); }
            catalog = true;
            index += 1;
            continue;
        }
        if option != OsStr::new("--catalog-state-dir") {
            return Err("error: unknown daemon option; use daemon --help\n");
        }
        let value = arguments.get(index + 1).ok_or("error: daemon option requires a value\n")?;
        if state_directory.replace(PathBuf::from(value)).is_some() {
            return Err("error: daemon accepts --catalog-state-dir only once\n");
        }
        index += 2;
    }
    if !catalog { return Err("error: daemon requires --catalog\n"); }
    if state_directory.as_ref().is_some_and(|path| path.as_os_str().is_empty()) {
        return Err("error: daemon option values must not be empty\n");
    }
    Ok(state_directory)
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DaemonLaunchError { Unavailable, Runtime }

#[cfg(unix)]
fn launch_catalog_daemon(
    requested_state_root: Option<&Path>,
    configuration: ResolvedConfiguration,
) -> Result<(), DaemonLaunchError> {
    if !cfg!(target_os = "linux") {
        return Err(DaemonLaunchError::Unavailable);
    }
    let catalog = prepare_current_worktree_mcp_catalog(requested_state_root, &configuration)
        .map_err(|_| DaemonLaunchError::Unavailable)?;
    let default_repository = catalog.repositories.iter()
        .find(|repository| repository.repository_identity == catalog.default_repository_identity)
        .ok_or(DaemonLaunchError::Unavailable)?;
    if !matches!(default_repository.graph_workspace, GraphWorkspaceContext::SingleRepository(_)) {
        return Err(DaemonLaunchError::Unavailable);
    }
    let root = default_repository.root.clone();
    let database = default_repository.database.clone();
    let repository_identity = default_repository.repository_identity.clone();
    let socket = current_worktree_catalog_daemon_socket(requested_state_root)
        .map_err(|_| DaemonLaunchError::Unavailable)?;
    let listener = bind_catalog_daemon_socket(&socket)?;
    let services = build_mcp_repository_services(catalog.repositories, &configuration)
        .map_err(|_| DaemonLaunchError::Unavailable)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(MCP_RUNTIME_WORKER_THREADS)
        .max_blocking_threads(MCP_RUNTIME_BLOCKING_THREADS)
        .enable_all().build().map_err(|_| DaemonLaunchError::Runtime)?;
    let default_repository_identity = catalog.default_repository_identity;
    let result = runtime.block_on(async move {
        let listener = tokio::net::UnixListener::from_std(listener)
            .map_err(|_| DaemonLaunchError::Runtime)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let watch_cancelled = Arc::clone(&cancelled);
        let watch_configuration = configuration.clone();
        let watcher = tokio::task::spawn_blocking(move || {
            let applied_at_unix_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
                .ok_or(())?;
            let index = LocalIndexRequest::new(
                &root,
                &database,
                &repository_identity,
                applied_at_unix_ms,
            )
            .with_configuration(&watch_configuration);
            watch_local_repository(
                LocalWatchRequest::new(index).with_native_event_hints(),
                watch_cancelled,
            )
            .map(|_| ())
            .map_err(|_| ())
        });
        run_catalog_daemon_listener(
            listener,
            services,
            default_repository_identity,
            cancelled,
            watcher,
        )
        .await
    });
    runtime.shutdown_timeout(Duration::from_millis(250));
    remove_catalog_daemon_socket(&socket);
    result
}

#[cfg(unix)]
fn bind_catalog_daemon_socket(
    socket: &Path,
) -> Result<std::os::unix::net::UnixListener, DaemonLaunchError> {
    use std::os::unix::{fs::FileTypeExt, net::UnixStream};
    let parent = socket.parent().ok_or(DaemonLaunchError::Unavailable)?;
    std::fs::create_dir_all(parent).map_err(|_| DaemonLaunchError::Unavailable)?;
    if let Ok(metadata) = std::fs::symlink_metadata(socket) {
        if !metadata.file_type().is_socket() { return Err(DaemonLaunchError::Unavailable); }
        match UnixStream::connect(socket) {
            Ok(_) => return Err(DaemonLaunchError::Unavailable),
            Err(error) if matches!(error.kind(), std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound) => {
                std::fs::remove_file(socket).map_err(|_| DaemonLaunchError::Unavailable)?;
            }
            Err(_) => return Err(DaemonLaunchError::Unavailable),
        }
    }
    let listener = std::os::unix::net::UnixListener::bind(socket).map_err(|_| DaemonLaunchError::Unavailable)?;
    listener.set_nonblocking(true).map_err(|_| DaemonLaunchError::Unavailable)?;
    Ok(listener)
}

#[cfg(unix)]
fn remove_catalog_daemon_socket(socket: &Path) {
    use std::os::unix::fs::FileTypeExt;
    if std::fs::symlink_metadata(socket).ok().is_some_and(|metadata| metadata.file_type().is_socket()) {
        let _ = std::fs::remove_file(socket);
    }
}

#[cfg(unix)]
async fn run_catalog_daemon_listener(
    listener: tokio::net::UnixListener,
    services: std::collections::BTreeMap<String, Arc<dyn RepositoryService>>,
    default_repository_identity: String,
    cancelled: Arc<AtomicBool>,
    mut watcher: tokio::task::JoinHandle<Result<(), ()>>,
) -> Result<(), DaemonLaunchError> {
    let permits = Arc::new(tokio::sync::Semaphore::new(4));
    let signal = first_shutdown_signal();
    tokio::pin!(signal);
    loop {
        tokio::select! {
            signal = &mut signal => {
                if signal.is_err() { return Err(DaemonLaunchError::Runtime); }
                cancelled.store(true, std::sync::atomic::Ordering::Release);
                return tokio::time::timeout(Duration::from_secs(5), &mut watcher).await
                    .map_err(|_| DaemonLaunchError::Runtime)?
                    .map_err(|_| DaemonLaunchError::Runtime)?
                    .map_err(|_| DaemonLaunchError::Runtime);
            }
            watcher_result = &mut watcher => {
                return watcher_result.map_err(|_| DaemonLaunchError::Runtime)?
                    .map_err(|_| DaemonLaunchError::Runtime);
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted.map_err(|_| DaemonLaunchError::Runtime)?;
                let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else { continue; };
                let services = services.clone();
                let default_repository_identity = default_repository_identity.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let (input, output) = stream.into_split();
                    let _ = repowitness_mcp::serve_transport_with_repository_catalog(
                        services, default_repository_identity, input, output,
                    ).await;
                });
            }
        }
    }
}
