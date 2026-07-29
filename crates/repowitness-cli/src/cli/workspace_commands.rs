const WORKSPACE_HELP: &str = concat!(
    "Index one explicit connected workspace.\n\n",
    "Usage:\n",
    "  repowitness workspace index --manifest <path> --database <path>\n",
    "      [--user-config <path>] [--workspace-config <path>]\n",
    "      [--repository-config <path>]\n\n",
    "The manifest is an explicit bounded no-follow authority document. Results\n",
    "contain only opaque digests and aggregate counts; they never include roots,\n",
    "selectors, manifest contents, or source text.\n",
);

struct WorkspaceIndexInvocation {
    manifest: PathBuf,
    database: PathBuf,
}

fn run_workspace(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    configuration_loader: &impl ConfigurationLoader,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_WORKSPACE_INDEX_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_WORKSPACE_INDEX_ARGUMENTS {
        return emit_error(stderr, EXIT_USAGE, "error: workspace received too many arguments; use workspace --help\n");
    }
    let Some(subcommand) = arguments.first() else {
        return emit_error(stderr, EXIT_USAGE, "error: workspace requires index; use workspace --help\n");
    };
    if subcommand == OsStr::new("--help") || subcommand == OsStr::new("-h") {
        return if arguments.len() == 1 {
            emit_output(stdout, WORKSPACE_HELP)
        } else {
            emit_error(stderr, EXIT_USAGE, "error: workspace --help accepts no additional arguments\n")
        };
    }
    if subcommand != OsStr::new("index") {
        return emit_error(stderr, EXIT_USAGE, "error: workspace requires index; use workspace --help\n");
    }
    let (arguments, configuration_invocation) = match extract_configuration_arguments(&arguments[1..], &[]) {
        Ok(parsed) => parsed,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let invocation = match parse_workspace_index_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let configuration = match configuration_loader.load(&configuration_invocation) {
        Ok(configuration) => configuration,
        Err(_) => return emit_error(stderr, EXIT_SOFTWARE, "error: configuration resolution failed\n"),
    };
    let (contents, parent) = match read_bounded_regular_file_with_parent(
        &invocation.manifest,
        repowitness_local::MAX_LOCAL_CONNECTED_WORKSPACE_MANIFEST_BYTES,
    ) {
        Ok(admitted) => admitted,
        Err(_) => return emit_error(stderr, EXIT_SOFTWARE, "error: workspace manifest admission failed\n"),
    };
    let applied_at_unix_ms = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(elapsed) => match u64::try_from(elapsed.as_millis()) {
            Ok(value) => value,
            Err(_) => return emit_error(stderr, EXIT_SOFTWARE, "error: workspace indexing failed\n"),
        },
        Err(_) => return emit_error(stderr, EXIT_SOFTWARE, "error: workspace indexing failed\n"),
    };
    let request = LocalConnectedWorkspaceIndexRequest::new(
        contents.bytes(),
        &parent,
        &invocation.database,
        &configuration,
        applied_at_unix_ms,
    );
    match index_local_connected_workspace(request, Arc::new(AtomicBool::new(false))) {
        Ok(report) => emit_workspace_index_report(stdout, report),
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: workspace indexing failed\n"),
    }
}

fn parse_workspace_index_arguments(
    arguments: &[OsString],
) -> Result<WorkspaceIndexInvocation, &'static str> {
    let mut manifest = None;
    let mut database = None;
    let mut pairs = arguments.chunks_exact(2);
    for pair in &mut pairs {
        let option = pair[0].as_os_str();
        let value = pair[1].as_os_str();
        if value.is_empty() {
            return Err("error: workspace option values must not be empty\n");
        }
        if option == OsStr::new("--manifest") {
            if manifest.replace(PathBuf::from(value)).is_some() {
                return Err("error: workspace index accepts --manifest only once\n");
            }
        } else if option == OsStr::new("--database") {
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: workspace index accepts --database only once\n");
            }
        } else {
            return Err("error: unknown workspace index option; use workspace --help\n");
        }
    }
    if !pairs.remainder().is_empty() {
        return Err("error: workspace index options require values\n");
    }
    Ok(WorkspaceIndexInvocation {
        manifest: manifest.ok_or("error: workspace index requires --manifest\n")?,
        database: database.ok_or("error: workspace index requires --database\n")?,
    })
}
