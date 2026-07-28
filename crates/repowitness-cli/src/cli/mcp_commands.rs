/// Parses and runs the process-level local stdio MCP command.
///
/// Stdout is owned exclusively by the MCP transport. Usage and lifecycle
/// diagnostics are written only to the supplied stderr destination.
pub fn run_mcp_server(args: impl IntoIterator<Item = OsString>, mut stderr: impl Write) -> u8 {
    let mut args = args.into_iter();
    let _program = args.next();
    if args.next().as_deref() != Some(OsStr::new("mcp-serve")) {
        return emit_error(
            &mut stderr,
            EXIT_USAGE,
            "error: expected mcp-serve command\n",
        );
    }
    let arguments: Vec<OsString> = args.take(MAX_MCP_SERVE_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_MCP_SERVE_ARGUMENTS {
        return emit_error(
            &mut stderr,
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
    let invocation = match parse_mcp_serve_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(&mut stderr, EXIT_USAGE, message),
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(MCP_RUNTIME_WORKER_THREADS)
        .max_blocking_threads(MCP_RUNTIME_BLOCKING_THREADS)
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            return emit_error(
                &mut stderr,
                EXIT_SOFTWARE,
                "error: MCP runtime initialization failed\n",
            );
        }
    };
    let service: Arc<dyn RepositoryService> = Arc::new(LocalMcpRepositoryService {
        root: invocation.root,
        database: invocation.database,
        repository_identity: invocation.repository_identity,
        memory_actor: invocation.memory_actor,
    });
    let result = runtime.block_on(async {
        if invocation.memory_writes_enabled {
            serve_stdio_with_memory_writes(service).await
        } else {
            serve_stdio(service).await
        }
    });
    match result {
        Ok(()) => EXIT_SUCCESS,
        Err(error) => {
            if writeln!(stderr, "error: {error}").is_ok() {
                EXIT_SOFTWARE
            } else {
                EXIT_IO
            }
        }
    }
}

struct McpServeInvocation {
    root: PathBuf,
    database: PathBuf,
    repository_identity: String,
    memory_writes_enabled: bool,
    memory_actor: Option<String>,
}

fn parse_mcp_serve_arguments(arguments: &[OsString]) -> Result<McpServeInvocation, &'static str> {
    let mut root = None;
    let mut database = None;
    let mut repository_identity = None;
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
    let memory_actor = match (memory_writes_enabled, memory_actor) {
        (false, None) => None,
        (true, Some(actor)) => {
            let actor = actor
                .to_str()
                .ok_or("error: mcp-serve memory actor must be UTF-8\n")?;
            validate_local_memory_actor(actor)
                .map_err(|_| "error: mcp-serve memory actor is invalid\n")?;
            Some(actor.to_owned())
        }
        (true, None) => {
            return Err(
                "error: --enable-memory-writes requires --memory-actor\n",
            );
        }
        (false, Some(_)) => {
            return Err(
                "error: --memory-actor requires --enable-memory-writes\n",
            );
        }
    };
    Ok(McpServeInvocation {
        root,
        database,
        repository_identity: repository_identity.to_owned(),
        memory_writes_enabled,
        memory_actor,
    })
}
