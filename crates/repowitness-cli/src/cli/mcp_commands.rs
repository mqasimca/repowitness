/// Parses and runs the process-level local stdio MCP command.
///
/// Stdout is owned exclusively by the MCP transport. Usage and lifecycle
/// diagnostics are written only to the supplied stderr destination.
pub fn run_mcp_server(args: impl IntoIterator<Item = OsString>, mut stderr: impl Write) -> u8 {
    run_mcp_server_with_adapters(
        args,
        &mut stderr,
        &LocalConfigurationLoader,
        &TokioMcpServerLauncher,
    )
}

fn run_mcp_server_with_adapters(
    args: impl IntoIterator<Item = OsString>,
    stderr: &mut impl Write,
    configuration_loader: &impl ConfigurationLoader,
    launcher: &impl McpServerLauncher,
) -> u8 {
    let mut args = args.into_iter();
    let _program = args.next();
    if args.next().as_deref() != Some(OsStr::new("mcp-serve")) {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: expected mcp-serve command\n",
        );
    }
    let arguments: Vec<OsString> = args.take(MAX_MCP_SERVE_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_MCP_SERVE_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: mcp-serve received too many arguments\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return if stderr.write_all(MCP_SERVE_HELP.as_bytes()).is_ok() {
            EXIT_SUCCESS
        } else {
            EXIT_IO
        };
    }
    let (arguments, configuration_invocation) = match extract_configuration_arguments(
        &arguments,
        &[
            "--catalog",
            "--daemon",
            "--enable-memory-writes",
            "--enable-native-tasks",
            "--enable-personal-memory",
        ],
    ) {
            Ok(parsed) => parsed,
            Err(message) => return emit_error(stderr, EXIT_USAGE, message),
        };
    let invocation = match parse_mcp_serve_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    if invocation.is_multi_repository() && configuration_invocation.repository.is_some() {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: --repository-config is unavailable with --registry or --catalog\n",
        );
    }
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
    let surface = match validate_mcp_startup_configuration(&invocation, &configuration) {
        Ok(surface) => surface,
        Err(message) => return emit_error(stderr, EXIT_SOFTWARE, message),
    };
    match launcher.launch(invocation, configuration, surface) {
        Ok(()) => EXIT_SUCCESS,
        Err(McpLaunchError::RuntimeInitialization) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: MCP runtime initialization failed\n",
        ),
        Err(McpLaunchError::Registry) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: MCP repository registry admission failed\n",
        ),
        Err(McpLaunchError::Catalog) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: MCP catalog admission failed\n",
        ),
        Err(McpLaunchError::DaemonUnavailable) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: MCP catalog daemon is unavailable\n",
        ),
        Err(McpLaunchError::Serve(error)) => {
            if writeln!(stderr, "error: {error}").is_ok() {
                EXIT_SOFTWARE
            } else {
                EXIT_IO
            }
        }
    }
}

fn validate_mcp_startup_configuration(
    invocation: &McpServeInvocation,
    configuration: &ResolvedConfiguration,
) -> Result<McpToolSurface, &'static str> {
    let tool_profile = configuration.preferences().mcp_tool_profile();
    let surface = match (tool_profile.requested(), tool_profile.authorized()) {
        (McpToolProfile::Canonical, Some(McpToolProfile::Canonical)) => McpToolSurface::NativeV1,
        (
            McpToolProfile::IncumbentCompatible,
            Some(McpToolProfile::IncumbentCompatible),
        ) => McpToolSurface::NativeV1PlusIncumbentSubsetV1,
        _ => return Err("error: configured MCP tool profile is unavailable\n"),
    };
    if invocation.is_multi_repository() && surface != McpToolSurface::NativeV1 {
        return Err("error: --registry and --catalog require the canonical MCP tool profile\n");
    }
    if invocation.memory_writes_enabled
        && *configuration.policy().deny_memory_writes().effective()
    {
        return Err("error: MCP memory writes are denied by configuration\n");
    }
    Ok(surface)
}

trait McpServerLauncher {
    fn launch(
        &self,
        invocation: McpServeInvocation,
        configuration: ResolvedConfiguration,
        surface: McpToolSurface,
    ) -> Result<(), McpLaunchError>;
}

struct TokioMcpServerLauncher;

impl McpServerLauncher for TokioMcpServerLauncher {
    #[allow(
        clippy::too_many_lines,
        reason = "the process startup modes share one auditable capability and runtime boundary"
    )]
    fn launch(
        &self,
        invocation: McpServeInvocation,
        configuration: ResolvedConfiguration,
        surface: McpToolSurface,
    ) -> Result<(), McpLaunchError> {
        let registry = match &invocation.target {
        McpServeTarget::Single { .. } | McpServeTarget::Catalog { .. } => None,
            McpServeTarget::Registry { path } => Some(
                build_mcp_registry_services(path, &configuration)
                    .map_err(|_| McpLaunchError::Registry)?,
            ),
        };
        let catalog = match &invocation.target {
            McpServeTarget::Catalog { state_dir, daemon_proxy: false } => Some(
                prepare_current_worktree_mcp_catalog(state_dir.as_deref(), &configuration)
                    .and_then(|catalog| {
                        let default_repository_identity = catalog.default_repository_identity;
                        build_mcp_repository_services(catalog.repositories, &configuration)
                            .map(|services| (services, default_repository_identity))
                    })
                    .map_err(|_| McpLaunchError::Catalog)?,
            ),
            McpServeTarget::Catalog { daemon_proxy: true, .. }
            | McpServeTarget::Single { .. }
            | McpServeTarget::Registry { .. } => None,
        };
        let daemon_socket = match &invocation.target {
            McpServeTarget::Catalog { state_dir, daemon_proxy: true } => {
                Some(
                    current_worktree_catalog_daemon_socket(state_dir.as_deref())
                        .map_err(|_| McpLaunchError::DaemonUnavailable)?,
                )
            }
            McpServeTarget::Catalog { daemon_proxy: false, .. }
            | McpServeTarget::Single { .. }
            | McpServeTarget::Registry { .. } => None,
        };
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(MCP_RUNTIME_WORKER_THREADS)
            .max_blocking_threads(MCP_RUNTIME_BLOCKING_THREADS)
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(_) => return Err(McpLaunchError::RuntimeInitialization),
        };
        match (invocation.target, registry, catalog) {
            (
                McpServeTarget::Catalog { daemon_proxy: false, .. },
                None,
                Some((catalog, default_repository_identity)),
            ) => runtime.block_on(serve_stdio_with_repository_catalog(
                catalog,
                default_repository_identity,
            ))
            .map_err(McpLaunchError::Serve),
            (McpServeTarget::Catalog { daemon_proxy: true, .. }, None, None) => {
                runtime.block_on(proxy_catalog_daemon(
                    daemon_socket.ok_or(McpLaunchError::DaemonUnavailable)?,
                ))
            }
            (McpServeTarget::Registry { .. }, Some(registry), None) => {
                runtime.block_on(serve_stdio_with_repository_registry(registry))
                    .map_err(McpLaunchError::Serve)
            }
            (
                McpServeTarget::Single {
                    root,
                    database,
                    repository_identity,
                    graph_workspace,
                },
                None,
                None,
            ) => {
                let service: Arc<dyn RepositoryService> = Arc::new(LocalMcpRepositoryService {
                    root,
                    database,
                    repository_identity,
                    graph_workspace,
                    memory_actor: invocation.memory_actor,
                    personal_memory_profile: invocation.personal_memory_profile,
                    configuration,
                });
                runtime.block_on(async {
                    if invocation.personal_memory_profile.is_some() {
                        serve_stdio_with_surface_tasks_and_personal_memory(
                            service,
                            surface,
                            invocation.memory_writes_enabled,
                            invocation.native_tasks_enabled,
                        )
                        .await
                    } else {
                        serve_stdio_with_surface_and_native_tasks(
                            service,
                            surface,
                            invocation.memory_writes_enabled,
                            invocation.native_tasks_enabled,
                        )
                        .await
                    }
                })
                .map_err(McpLaunchError::Serve)
            }
            _ => Err(McpLaunchError::Registry),
        }
    }
}

enum McpLaunchError {
    RuntimeInitialization,
    Registry,
    Catalog,
    DaemonUnavailable,
    Serve(repowitness_mcp::McpServeError),
}

struct McpServeInvocation {
    target: McpServeTarget,
    memory_writes_enabled: bool,
    native_tasks_enabled: bool,
    memory_actor: Option<String>,
    personal_memory_profile: Option<PersonalMemoryProfileId>,
}

enum McpServeTarget {
    Single {
        root: PathBuf,
        database: PathBuf,
        repository_identity: String,
        graph_workspace: GraphWorkspaceContext,
    },
    Registry {
        path: PathBuf,
    },
    Catalog {
        state_dir: Option<PathBuf>,
        daemon_proxy: bool,
    },
}

impl McpServeInvocation {
    const fn is_multi_repository(&self) -> bool {
        matches!(
            self.target,
            McpServeTarget::Registry { .. } | McpServeTarget::Catalog { .. }
        )
    }
}

fn build_mcp_registry_services(
    registry_path: &Path,
    configuration: &ResolvedConfiguration,
) -> Result<std::collections::BTreeMap<String, Arc<dyn RepositoryService>>, ()> {
    build_mcp_repository_services(read_mcp_repository_registry(registry_path)?, configuration)
}

fn build_mcp_repository_services(
    repositories: Vec<RegisteredMcpRepository>,
    configuration: &ResolvedConfiguration,
) -> Result<std::collections::BTreeMap<String, Arc<dyn RepositoryService>>, ()> {
    let mut services = std::collections::BTreeMap::new();
    for repository in repositories {
        let repository_identity = repository.repository_identity;
        let service: Arc<dyn RepositoryService> = Arc::new(LocalMcpRepositoryService {
            root: repository.root,
            database: repository.database,
            graph_workspace: repository.graph_workspace,
            repository_identity: repository_identity.clone(),
            memory_actor: None,
            personal_memory_profile: None,
            configuration: configuration.clone(),
        });
        if services.insert(repository_identity, service).is_some() {
            return Err(());
        }
    }
    Ok(services)
}

#[allow(
    clippy::too_many_lines,
    reason = "the startup capability grammar is deliberately parsed as one fail-closed option set before any runtime initialization"
)]
fn parse_mcp_serve_arguments(arguments: &[OsString]) -> Result<McpServeInvocation, &'static str> {
    let mut root = None;
    let mut database = None;
    let mut repository_identity = None;
    let mut registry = None;
    let mut catalog = false;
    let mut daemon_proxy = false;
    let mut catalog_state_dir = None;
    let mut connected_workspace = None;
    let mut source_slot = None;
    let mut memory_writes_enabled = false;
    let mut native_tasks_enabled = false;
    let mut memory_actor = None;
    let mut personal_memory_enabled = false;
    let mut personal_memory_profile = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        if option == OsStr::new("--catalog") {
            if catalog {
                return Err("error: mcp-serve accepts --catalog only once\n");
            }
            catalog = true;
            index += 1;
            continue;
        }
        if option == OsStr::new("--daemon") {
            if daemon_proxy {
                return Err("error: mcp-serve accepts --daemon only once\n");
            }
            daemon_proxy = true;
            index += 1;
            continue;
        }
        if option == OsStr::new("--enable-memory-writes") {
            if memory_writes_enabled {
                return Err(
                    "error: mcp-serve accepts --enable-memory-writes only once\n",
                );
            }
            memory_writes_enabled = true;
            index += 1;
            continue;
        }
        if option == OsStr::new("--enable-native-tasks") {
            if native_tasks_enabled {
                return Err("error: mcp-serve accepts --enable-native-tasks only once\n");
            }
            native_tasks_enabled = true;
            index += 1;
            continue;
        }
        if option == OsStr::new("--enable-personal-memory") {
            if personal_memory_enabled {
                return Err("error: mcp-serve accepts --enable-personal-memory only once\n");
            }
            personal_memory_enabled = true;
            index += 1;
            continue;
        }
        let value = arguments
            .get(index + 1)
            .ok_or("error: mcp-serve option requires a value\n")?;
        if option == OsStr::new("--root") {
            if root.replace(PathBuf::from(value)).is_some() {
                return Err("error: mcp-serve accepts --root only once\n");
            }
        } else if option == OsStr::new("--database") {
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: mcp-serve accepts --database only once\n");
            }
        } else if option == OsStr::new("--repository-id") {
            if repository_identity.replace(value.clone()).is_some() {
                return Err("error: mcp-serve accepts --repository-id only once\n");
            }
        } else if option == OsStr::new("--registry") {
            if registry.replace(PathBuf::from(value)).is_some() {
                return Err("error: mcp-serve accepts --registry only once\n");
            }
        } else if option == OsStr::new("--catalog-state-dir") {
            if catalog_state_dir.replace(PathBuf::from(value)).is_some() {
                return Err("error: mcp-serve accepts --catalog-state-dir only once\n");
            }
        } else if option == OsStr::new("--connected-workspace-id") {
            if connected_workspace.replace(value.clone()).is_some() {
                return Err("error: mcp-serve accepts --connected-workspace-id only once\n");
            }
        } else if option == OsStr::new("--source-slot-id") {
            if source_slot.replace(value.clone()).is_some() {
                return Err("error: mcp-serve accepts --source-slot-id only once\n");
            }
        } else if option == OsStr::new("--memory-actor") {
            if memory_actor.replace(value.clone()).is_some() {
                return Err("error: mcp-serve accepts --memory-actor only once\n");
            }
        } else if option == OsStr::new("--personal-memory-profile") {
            if personal_memory_profile.replace(value.clone()).is_some() {
                return Err("error: mcp-serve accepts --personal-memory-profile only once\n");
            }
        } else {
            return Err("error: unknown mcp-serve option; use mcp-serve --help\n");
        }
        index += 2;
    }

    let target = if catalog {
        if registry.is_some()
            || root.is_some()
            || database.is_some()
            || repository_identity.is_some()
            || connected_workspace.is_some()
            || source_slot.is_some()
        {
            return Err("error: --catalog cannot be combined with registry or single-repository options\n");
        }
        if catalog_state_dir
            .as_ref()
            .is_some_and(|path: &PathBuf| path.as_os_str().is_empty())
        {
            return Err("error: mcp-serve option values must not be empty\n");
        }
        if memory_writes_enabled || native_tasks_enabled || personal_memory_enabled {
            return Err("error: --catalog supports read-only tools only\n");
        }
        McpServeTarget::Catalog {
            state_dir: catalog_state_dir,
            daemon_proxy,
        }
    } else if let Some(path) = registry {
        if daemon_proxy {
            return Err("error: --daemon requires --catalog\n");
        }
        if catalog_state_dir.is_some() {
            return Err("error: --catalog-state-dir requires --catalog\n");
        }
        if path.as_os_str().is_empty() {
            return Err("error: mcp-serve option values must not be empty\n");
        }
        if root.is_some()
            || database.is_some()
            || repository_identity.is_some()
            || connected_workspace.is_some()
            || source_slot.is_some()
        {
            return Err("error: --registry cannot be combined with single-repository options\n");
        }
        if memory_writes_enabled || native_tasks_enabled || personal_memory_enabled {
            return Err("error: --registry supports read-only tools only\n");
        }
        McpServeTarget::Registry { path }
    } else {
        if daemon_proxy {
            return Err("error: --daemon requires --catalog\n");
        }
        if catalog_state_dir.is_some() {
            return Err("error: --catalog-state-dir requires --catalog\n");
        }
        let root = root.ok_or("error: mcp-serve requires --root\n")?;
        let database = database.ok_or("error: mcp-serve requires --database\n")?;
        let repository_identity =
            repository_identity.ok_or("error: mcp-serve requires --repository-id\n")?;
        if root.as_os_str().is_empty()
            || database.as_os_str().is_empty()
            || repository_identity.is_empty()
        {
            return Err("error: mcp-serve option values must not be empty\n");
        }
        let repository_identity = repository_identity
            .to_str()
            .ok_or("error: mcp-serve repository identity must be UTF-8\n")?;
        RepositoryIdentityTextV1::decode(repository_identity)
            .map_err(|_| "error: mcp-serve repository identity is invalid\n")?;
        let graph_workspace = resolve_mcp_graph_workspace(
            repository_identity,
            connected_workspace,
            source_slot,
        )?;
        McpServeTarget::Single {
            root,
            database,
            repository_identity: repository_identity.to_owned(),
            graph_workspace,
        }
    };
    let memory_actor = resolve_mcp_memory_actor(memory_writes_enabled, memory_actor)?;
    let personal_memory_profile = resolve_mcp_personal_memory_profile(
        personal_memory_enabled,
        personal_memory_profile,
    )?;
    Ok(McpServeInvocation {
        target,
        memory_writes_enabled,
        native_tasks_enabled,
        memory_actor,
        personal_memory_profile,
    })
}

#[cfg(unix)]
async fn proxy_catalog_daemon(socket: PathBuf) -> Result<(), McpLaunchError> {
    let stream = tokio::net::UnixStream::connect(socket)
        .await
        .map_err(|_| McpLaunchError::DaemonUnavailable)?;
    let (mut daemon_read, mut daemon_write) = stream.into_split();
    let mut client_read = tokio::io::stdin();
    let mut client_write = tokio::io::stdout();
    let client_to_daemon = async {
        tokio::io::copy(&mut client_read, &mut daemon_write)
            .await
            .map_err(|_| McpLaunchError::DaemonUnavailable)?;
        tokio::io::AsyncWriteExt::shutdown(&mut daemon_write)
            .await
            .map_err(|_| McpLaunchError::DaemonUnavailable)
    };
    let daemon_to_client = tokio::io::copy(&mut daemon_read, &mut client_write);
    tokio::pin!(client_to_daemon);
    tokio::pin!(daemon_to_client);
    tokio::select! {
        client_result = &mut client_to_daemon => {
            client_result?;
            daemon_to_client.await.map_err(|_| McpLaunchError::DaemonUnavailable)?;
        }
        daemon_result = &mut daemon_to_client => {
            daemon_result.map_err(|_| McpLaunchError::DaemonUnavailable)?;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
async fn proxy_catalog_daemon(_socket: PathBuf) -> Result<(), McpLaunchError> {
    Err(McpLaunchError::DaemonUnavailable)
}

fn resolve_mcp_personal_memory_profile(
    personal_memory_enabled: bool,
    profile: Option<OsString>,
) -> Result<Option<PersonalMemoryProfileId>, &'static str> {
    match (personal_memory_enabled, profile) {
        (false, None) => Ok(None),
        (true, Some(profile)) => profile
            .to_str()
            .and_then(parse_personal_memory_profile)
            .map(Some)
            .ok_or("error: --enable-personal-memory requires --personal-memory-profile with 32 lowercase hex characters\n"),
        (true, None) => Err("error: --enable-personal-memory requires --personal-memory-profile\n"),
        (false, Some(_)) => Err("error: --personal-memory-profile requires --enable-personal-memory\n"),
    }
}

fn resolve_mcp_graph_workspace(
    repository_identity: &str,
    connected_workspace: Option<OsString>,
    source_slot: Option<OsString>,
) -> Result<GraphWorkspaceContext, &'static str> {
    match (connected_workspace, source_slot) {
        (None, None) => Ok(GraphWorkspaceContext::SingleRepository(
            repository_identity.to_owned(),
        )),
        (Some(connected_workspace), Some(source_slot)) => {
            let connected_workspace = connected_workspace
                .to_str()
                .ok_or("error: mcp-serve connected workspace identity must be UTF-8\n")?;
            ConnectedWorkspaceIdTextV1::decode(connected_workspace)
                .map_err(|_| "error: mcp-serve connected workspace identity is invalid\n")?;
            let source_slot = source_slot
                .to_str()
                .ok_or("error: mcp-serve source slot identity must be UTF-8\n")?;
            SourceSlotIdTextV1::decode(source_slot)
                .map_err(|_| "error: mcp-serve source slot identity is invalid\n")?;
            Ok(GraphWorkspaceContext::ConnectedWorkspace {
                connected_workspace: connected_workspace.to_owned(),
                source_slot: source_slot.to_owned(),
            })
        }
        (None, Some(_)) => {
            Err("error: mcp-serve --source-slot-id requires --connected-workspace-id\n")
        }
        (Some(_), None) => {
            Err("error: mcp-serve --connected-workspace-id requires --source-slot-id\n")
        }
    }
}

fn resolve_mcp_memory_actor(
    memory_writes_enabled: bool,
    memory_actor: Option<OsString>,
) -> Result<Option<String>, &'static str> {
    match (memory_writes_enabled, memory_actor) {
        (false, None) => Ok(None),
        (true, Some(actor)) => {
            let actor = actor
                .to_str()
                .ok_or("error: mcp-serve memory actor must be UTF-8\n")?;
            validate_local_memory_actor(actor)
                .map_err(|_| "error: mcp-serve memory actor is invalid\n")?;
            Ok(Some(actor.to_owned()))
        }
        (true, None) => Err("error: --enable-memory-writes requires --memory-actor\n"),
        (false, Some(_)) => Err("error: --memory-actor requires --enable-memory-writes\n"),
    }
}
