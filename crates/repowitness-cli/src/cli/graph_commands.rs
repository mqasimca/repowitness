trait RepositoryGraphReader {
    fn read(
        &self,
        invocation: GraphInvocation,
        configuration: &ResolvedConfiguration,
    ) -> Result<GraphReadServiceOutput, ()>;
}

struct LocalRepositoryGraphReader;

impl RepositoryGraphReader for LocalRepositoryGraphReader {
    fn read(
        &self,
        invocation: GraphInvocation,
        configuration: &ResolvedConfiguration,
    ) -> Result<GraphReadServiceOutput, ()> {
        read_local_graph_service(
            &invocation.database,
            &invocation.workspace,
            invocation.request,
            configuration,
            Arc::new(AtomicBool::new(false)),
        )
        .map_err(|_| ())
    }
}

fn run_graph(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    reader: &impl RepositoryGraphReader,
    configuration_loader: &impl ConfigurationLoader,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_GRAPH_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_GRAPH_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: graph received too many arguments; use graph --help\n",
        );
    }
    if graph_help_requested(&arguments) {
        return emit_output(stdout, GRAPH_HELP);
    }
    let (arguments, configuration_invocation) =
        match extract_configuration_arguments(&arguments, GRAPH_VALUE_FLAGS) {
            Ok(parsed) => parsed,
            Err(message) => return emit_error(stderr, EXIT_USAGE, message),
        };
    let invocation = match parse_graph_arguments(&arguments) {
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
    match reader.read(invocation, &configuration) {
        Ok(output) => emit_graph_output(stdout, &output),
        Err(()) => emit_error(stderr, EXIT_SOFTWARE, "error: graph read failed\n"),
    }
}

fn graph_help_requested(arguments: &[OsString]) -> bool {
    matches!(
        arguments,
        [help] if help == OsStr::new("--help") || help == OsStr::new("-h")
    ) || matches!(
        arguments,
        [operation, help]
            if CliGraphOperation::parse(operation).is_some()
                && (help == OsStr::new("--help") || help == OsStr::new("-h"))
    )
}
