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
    let (arguments, configuration_invocation) =
        match extract_configuration_arguments(&arguments, &["--enable-memory-writes"]) {
            Ok(parsed) => parsed,
            Err(message) => return emit_error(stderr, EXIT_USAGE, message),
        };
    let invocation = match parse_mcp_serve_arguments(&arguments) {
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
    fn launch(
        &self,
        invocation: McpServeInvocation,
        configuration: ResolvedConfiguration,
        surface: McpToolSurface,
    ) -> Result<(), McpLaunchError> {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(MCP_RUNTIME_WORKER_THREADS)
            .max_blocking_threads(MCP_RUNTIME_BLOCKING_THREADS)
            .enable_time()
            .build()
        {
            Ok(runtime) => runtime,
            Err(_) => return Err(McpLaunchError::RuntimeInitialization),
        };
        let service: Arc<dyn RepositoryService> = Arc::new(LocalMcpRepositoryService {
            root: invocation.root,
            database: invocation.database,
            repository_identity: invocation.repository_identity,
            graph_workspace: invocation.graph_workspace,
            memory_actor: invocation.memory_actor,
            configuration,
        });
        let result = runtime.block_on(serve_stdio_with_surface(
            service,
            surface,
            invocation.memory_writes_enabled,
        ));
        result.map_err(McpLaunchError::Serve)
    }
}

enum McpLaunchError {
    RuntimeInitialization,
    Serve(repowitness_mcp::McpServeError),
}

struct McpServeInvocation {
    root: PathBuf,
    database: PathBuf,
    repository_identity: String,
    graph_workspace: GraphWorkspaceContext,
    memory_writes_enabled: bool,
    memory_actor: Option<String>,
}

fn parse_mcp_serve_arguments(arguments: &[OsString]) -> Result<McpServeInvocation, &'static str> {
    let mut root = None;
    let mut database = None;
    let mut repository_identity = None;
    let mut connected_workspace = None;
    let mut source_slot = None;
    let mut memory_writes_enabled = false;
    let mut memory_actor = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
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
        } else {
            return Err("error: unknown mcp-serve option; use mcp-serve --help\n");
        }
        index += 2;
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
    let memory_actor = resolve_mcp_memory_actor(memory_writes_enabled, memory_actor)?;
    Ok(McpServeInvocation {
        root,
        database,
        repository_identity: repository_identity.to_owned(),
        graph_workspace,
        memory_writes_enabled,
        memory_actor,
    })
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
