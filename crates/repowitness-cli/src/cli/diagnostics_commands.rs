fn run_diagnostics(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    reader: &impl RepositoryDiagnosticsReader,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_DIAGNOSTICS_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_DIAGNOSTICS_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: diagnostics received too many arguments; use diagnostics --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, DIAGNOSTICS_HELP);
    }
    let invocation = match parse_diagnostics_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match reader.diagnose(&invocation) {
        Ok(report) => emit_diagnostics_report(stdout, &report),
        Err(_) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: repository diagnostics failed\n",
        ),
    }
}

fn parse_diagnostics_arguments(
    arguments: &[OsString],
) -> Result<DiagnosticsInvocation, &'static str> {
    let mut database = None;
    let mut repository_identity = None;
    let mut chunks = arguments.chunks_exact(2);
    for pair in &mut chunks {
        let option = pair[0].as_os_str();
        let value = pair[1].as_os_str();
        if option == OsStr::new("--database") {
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: diagnostics accepts --database only once\n");
            }
        } else if option == OsStr::new("--repository-id") {
            if repository_identity.replace(value.to_owned()).is_some() {
                return Err("error: diagnostics accepts --repository-id only once\n");
            }
        } else {
            return Err("error: unknown diagnostics option; use diagnostics --help\n");
        }
    }
    if !chunks.remainder().is_empty() {
        return Err("error: diagnostics option requires a value\n");
    }
    let database = database.ok_or("error: diagnostics requires --database\n")?;
    let repository_identity =
        repository_identity.ok_or("error: diagnostics requires --repository-id\n")?;
    if database.as_os_str().is_empty() || repository_identity.is_empty() {
        return Err("error: diagnostics option values must not be empty\n");
    }
    Ok(DiagnosticsInvocation {
        database,
        repository_identity,
    })
}
