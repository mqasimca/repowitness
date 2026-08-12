/// Runs the local MCP server over stdio.
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
        return emit_error(stderr, EXIT_USAGE, "error: expected mcp-serve command\n");
    }
    let arguments: Vec<OsString> = args.take(MAX_MCP_SERVE_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_MCP_SERVE_ARGUMENTS {
        return emit_error(stderr, EXIT_USAGE, "error: mcp-serve received too many arguments\n");
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return if stderr.write_all(MCP_SERVE_HELP.as_bytes()).is_ok() {
            EXIT_SUCCESS
        } else {
            EXIT_IO
        };
    }
    let (arguments, configuration_invocation) = match extract_configuration_arguments(
        &arguments,
        &["--catalog", "--enable-memory-writes"],
    ) {
        Ok(parsed) => parsed,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let invocation = match parse_mcp_serve_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let configuration = match configuration_loader.load_mcp(&configuration_invocation) {
        Ok(configuration) => configuration,
        Err(_) => return emit_error(stderr, EXIT_SOFTWARE, "error: configuration resolution failed\n"),
    };
    if let Err(message) = validate_mcp_startup_configuration(&invocation, &configuration) {
        return emit_error(stderr, EXIT_SOFTWARE, message);
    }
    match launcher.launch(invocation, configuration) {
        Ok(()) => EXIT_SUCCESS,
        Err(McpLaunchError::RuntimeInitialization) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: MCP runtime initialization failed\n",
        ),
        Err(McpLaunchError::Catalog) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: MCP catalog could not be loaded; run repowitness onboard for each repository\n",
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
) -> Result<(), &'static str> {
    let tool_profile = configuration.preferences().mcp_tool_profile();
    if !matches!(
        (tool_profile.requested(), tool_profile.authorized()),
        (McpToolProfile::Canonical, Some(McpToolProfile::Canonical))
    ) {
        return Err("error: only the canonical MCP tool profile is supported\n");
    }
    if invocation.memory_writes_enabled
        && *configuration.policy().deny_memory_writes().effective()
    {
        return Err("error: MCP memory writes are denied by configuration\n");
    }
    if matches!(invocation.target, McpServeTarget::Catalog { .. }) && invocation.memory_writes_enabled {
        return Err("error: MCP memory writes are not supported in catalog mode\n");
    }
    Ok(())
}

trait McpServerLauncher {
    fn launch(
        &self,
        invocation: McpServeInvocation,
        configuration: ResolvedConfiguration,
    ) -> Result<(), McpLaunchError>;
}

struct TokioMcpServerLauncher;

impl McpServerLauncher for TokioMcpServerLauncher {
    fn launch(
        &self,
        invocation: McpServeInvocation,
        configuration: ResolvedConfiguration,
    ) -> Result<(), McpLaunchError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(MCP_RUNTIME_WORKER_THREADS)
            .max_blocking_threads(MCP_RUNTIME_BLOCKING_THREADS)
            .enable_all()
            .build()
            .map_err(|_| McpLaunchError::RuntimeInitialization)?;
        match invocation.target {
            McpServeTarget::Catalog { state_dir } => {
                let repositories = read_mcp_catalog(state_dir.as_deref())
                    .map_err(|_| McpLaunchError::Catalog)?;
                let default_repository_id = catalog_default_repository_id(&repositories);
                let services = build_mcp_catalog_services(repositories, &configuration)
                    .map_err(|_| McpLaunchError::Catalog)?;
                runtime
                    .block_on(serve_stdio_with_repository_catalog(services, default_repository_id))
                    .map_err(McpLaunchError::Serve)
            }
            McpServeTarget::Single {
                root,
                database,
                repository_identity,
                graph_workspace,
            } => {
                let service: Arc<dyn RepositoryService> = Arc::new(LocalMcpRepositoryService {
                    root,
                    database,
                    repository_identity,
                    graph_workspace,
                    memory_actor: invocation.memory_actor,
                    configuration,
                });
                let result = if invocation.memory_writes_enabled {
                    runtime.block_on(serve_stdio_with_memory_writes(service))
                } else {
                    runtime.block_on(serve_stdio(service))
                };
                result.map_err(McpLaunchError::Serve)
            }
        }
    }
}

enum McpLaunchError {
    RuntimeInitialization,
    Catalog,
    Serve(repowitness_mcp::McpServeError),
}

struct McpServeInvocation {
    target: McpServeTarget,
    memory_writes_enabled: bool,
    memory_actor: Option<String>,
}

enum McpServeTarget {
    Single {
        root: PathBuf,
        database: PathBuf,
        repository_identity: String,
        graph_workspace: GraphWorkspaceContext,
    },
    Catalog {
        state_dir: Option<PathBuf>,
    },
}

fn parse_mcp_serve_arguments(arguments: &[OsString]) -> Result<McpServeInvocation, &'static str> {
    let mut root = None;
    let mut database = None;
    let mut repository_identity = None;
    let mut catalog = false;
    let mut catalog_state_dir = None;
    let mut memory_writes_enabled = false;
    let mut memory_actor = None;
    let mut index = 0;
    while index < arguments.len() {
        let option = &arguments[index];
        if option == OsStr::new("--enable-memory-writes") {
            if memory_writes_enabled {
                return Err("error: mcp-serve accepts --enable-memory-writes only once\n");
            }
            memory_writes_enabled = true;
            index += 1;
            continue;
        }
        if option == OsStr::new("--catalog") {
            if catalog {
                return Err("error: mcp-serve accepts --catalog only once\n");
            }
            catalog = true;
            index += 1;
            continue;
        }
        let value = arguments
            .get(index + 1)
            .ok_or("error: mcp-serve option requires a value\n")?;
        let target = if option == OsStr::new("--root") {
            &mut root
        } else if option == OsStr::new("--database") {
            &mut database
        } else if option == OsStr::new("--repository-id") {
            &mut repository_identity
        } else if option == OsStr::new("--catalog-state-dir") {
            &mut catalog_state_dir
        } else if option == OsStr::new("--memory-actor") {
            &mut memory_actor
        } else {
            return Err("error: unknown mcp-serve option; use mcp-serve --help\n");
        };
        if target.replace(value.clone()).is_some() {
            return Err("error: mcp-serve option may be supplied only once\n");
        }
        index += 2;
    }
    if catalog {
        if root.is_some() || database.is_some() || repository_identity.is_some() {
            return Err("error: mcp-serve --catalog cannot be combined with explicit repository options\n");
        }
        if catalog_state_dir.as_ref().is_some_and(|path| path.as_os_str().is_empty()) {
            return Err("error: mcp-serve catalog state directory must not be empty\n");
        }
        let memory_actor = resolve_mcp_memory_actor(memory_writes_enabled, memory_actor)?;
        return Ok(McpServeInvocation {
            target: McpServeTarget::Catalog {
                state_dir: catalog_state_dir.map(PathBuf::from),
            },
            memory_writes_enabled,
            memory_actor,
        });
    }
    if catalog_state_dir.is_some() {
        return Err("error: mcp-serve --catalog-state-dir requires --catalog\n");
    }
    let root = root.ok_or("error: mcp-serve requires --root\n")?;
    let database = database.ok_or("error: mcp-serve requires --database\n")?;
    let repository_identity = repository_identity
        .ok_or("error: mcp-serve requires --repository-id\n")?;
    if root.as_os_str().is_empty() || database.as_os_str().is_empty() || repository_identity.is_empty() {
        return Err("error: mcp-serve option values must not be empty\n");
    }
    let repository_identity = repository_identity
        .to_str()
        .ok_or("error: mcp-serve repository identity must be UTF-8\n")?;
    RepositoryIdentityTextV1::decode(repository_identity)
        .map_err(|_| "error: mcp-serve repository identity is invalid\n")?;
    let graph_workspace = GraphWorkspaceContext::SingleRepository(repository_identity.to_owned());
    let memory_actor = resolve_mcp_memory_actor(memory_writes_enabled, memory_actor)?;
    Ok(McpServeInvocation {
        target: McpServeTarget::Single {
            root: root.into(),
            database: database.into(),
            repository_identity: repository_identity.to_owned(),
            graph_workspace,
        },
        memory_writes_enabled,
        memory_actor,
    })
}

fn resolve_mcp_memory_actor(
    memory_writes_enabled: bool,
    memory_actor: Option<OsString>,
) -> Result<Option<String>, &'static str> {
    match (memory_writes_enabled, memory_actor) {
        (false, None) => Ok(None),
        (true, Some(actor)) => {
            let actor = actor.to_str().ok_or("error: mcp-serve memory actor must be UTF-8\n")?;
            validate_local_memory_actor(actor)
                .map_err(|_| "error: mcp-serve memory actor is invalid\n")?;
            Ok(Some(actor.to_owned()))
        }
        (true, None) => Err("error: --enable-memory-writes requires --memory-actor\n"),
        (false, Some(_)) => Err("error: --memory-actor requires --enable-memory-writes\n"),
    }
}
