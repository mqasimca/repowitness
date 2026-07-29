fn run_config(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    loader: &impl ConfigurationLoader,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_CONFIG_EXPLAIN_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_CONFIG_EXPLAIN_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: config received too many arguments; use config --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, CONFIG_EXPLAIN_HELP);
    }
    let Some(subcommand) = arguments.first() else {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: config requires explain; use config --help\n",
        );
    };
    if subcommand != OsStr::new("explain") {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: unknown config command; use config --help\n",
        );
    }
    if matches!(arguments.as_slice(), [_, help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, CONFIG_EXPLAIN_HELP);
    }
    let invocation = match parse_configuration_arguments(&arguments[1..], "config explain") {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match loader.load(&invocation) {
        Ok(configuration) => emit_configuration_report(stdout, &configuration),
        Err(_) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: configuration resolution failed\n",
        ),
    }
}

fn parse_configuration_arguments(
    arguments: &[OsString],
    command: &'static str,
) -> Result<ConfigurationInvocation, &'static str> {
    let mut invocation = ConfigurationInvocation::default();
    let mut chunks = arguments.chunks_exact(2);
    for pair in &mut chunks {
        let option = pair[0].as_os_str();
        let value = pair[1].as_os_str();
        if value.is_empty() {
            return Err("error: configuration path must not be empty\n");
        }
        let target = if option == OsStr::new("--user-config") {
            &mut invocation.user
        } else if option == OsStr::new("--workspace-config") {
            &mut invocation.workspace
        } else if option == OsStr::new("--repository-config") {
            &mut invocation.repository
        } else {
            return if command == "doctor" {
                Err("error: unknown doctor option; use doctor --help\n")
            } else {
                Err("error: unknown config option; use config --help\n")
            };
        };
        if target.replace(PathBuf::from(value)).is_some() {
            return Err("error: each configuration layer may be supplied only once\n");
        }
    }
    if !chunks.remainder().is_empty() {
        return Err("error: configuration option requires a path\n");
    }
    Ok(invocation)
}
