struct RepositoryTopologyInvocation {
    database: PathBuf,
    repository_identity: String,
    max_paths: u16,
}

fn run_repository_topology(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_REPOSITORY_TOPOLOGY_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_REPOSITORY_TOPOLOGY_ARGUMENTS {
        return emit_error(stderr, EXIT_USAGE, "error: repository-topology received too many arguments; use repository-topology --help\n");
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, REPOSITORY_TOPOLOGY_HELP);
    }
    let invocation = match parse_repository_topology_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let request = match LocalRepositoryTopologyRequest::new(&invocation.database, &invocation.repository_identity)
        .with_max_paths(invocation.max_paths) {
        Ok(request) => request,
        Err(_) => return emit_error(stderr, EXIT_USAGE, "error: repository-topology request is invalid or exceeds a resource bound\n"),
    };
    let output = read_local_repository_topology(request, Arc::new(AtomicBool::new(false)))
        .map_err(|_| ())
        .and_then(|result| mcp_repository_topology_output(result).map_err(|_| ()));
    match output {
        Ok(output) => emit_repository_topology_output(stdout, &output),
        Err(()) => emit_error(stderr, EXIT_SOFTWARE, "error: repository topology failed\n"),
    }
}

fn parse_repository_topology_arguments(arguments: &[OsString]) -> Result<RepositoryTopologyInvocation, &'static str> {
    let mut database = None;
    let mut repository_identity = None;
    let mut max_paths = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        index += 1;
        let value = arguments.get(index).ok_or("error: repository-topology requires --repository-id and --database; use repository-topology --help\n")?;
        index += 1;
        if option == OsStr::new("--repository-id") {
            if repository_identity.replace(value.clone()).is_some() { return Err("error: repository-topology --repository-id was supplied more than once\n"); }
        } else if option == OsStr::new("--database") {
            if database.replace(PathBuf::from(value)).is_some() { return Err("error: repository-topology --database was supplied more than once\n"); }
        } else if option == OsStr::new("--max-paths") {
            let parsed = value.to_str().ok_or("error: repository-topology --max-paths must be valid UTF-8\n")?
                .parse::<u16>().ok().filter(|value| (1..=1_000).contains(value))
                .ok_or("error: repository-topology --max-paths must be between 1 and 1000\n")?;
            if max_paths.replace(parsed).is_some() { return Err("error: repository-topology --max-paths was supplied more than once\n"); }
        } else {
            return Err("error: repository-topology accepts only --repository-id, --database, and --max-paths\n");
        }
    }
    let repository_identity = repository_identity.ok_or("error: repository-topology requires --repository-id\n")?
        .into_string().map_err(|_| "error: repository-topology repository identity must be valid UTF-8\n")?;
    let database = database.ok_or("error: repository-topology requires --database\n")?;
    Ok(RepositoryTopologyInvocation { database, repository_identity, max_paths: max_paths.unwrap_or(200) })
}

fn emit_repository_topology_output(writer: &mut impl Write, output: &RepositoryTopologyOutput) -> u8 {
    let Ok(mut encoded) = serde_json::to_vec(output) else { return EXIT_SOFTWARE; };
    if encoded.len() > MAX_CLI_ARCHITECTURE_MAP_OUTPUT_BYTES { return EXIT_SOFTWARE; }
    encoded.push(b'\n');
    if writer.write_all(&encoded).is_ok() { EXIT_SUCCESS } else { EXIT_IO }
}
