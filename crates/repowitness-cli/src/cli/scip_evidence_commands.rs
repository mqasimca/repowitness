struct ScipEvidenceInvocation {
    database: PathBuf,
    workspace: GraphWorkspaceContext,
    request: ScipEvidenceServiceRequest,
}

fn run_scip_evidence(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_SCIP_EVIDENCE_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_SCIP_EVIDENCE_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: scip-evidence received too many arguments; use scip-evidence --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, SCIP_EVIDENCE_HELP);
    }
    let invocation = match parse_scip_evidence_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match read_local_scip_evidence_service(
        &invocation.database,
        &invocation.workspace,
        invocation.request,
        Arc::new(AtomicBool::new(false)),
    ) {
        Ok(output) => emit_scip_evidence_output(stdout, &output),
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: SCIP evidence read failed\n"),
    }
}

fn parse_scip_evidence_arguments(
    arguments: &[OsString],
) -> Result<ScipEvidenceInvocation, &'static str> {
    let mut workspace_arguments = GraphWorkspaceArguments::default();
    let mut database = None;
    let mut symbol = None;
    let mut package_roots = Vec::new();
    let mut workspace_view = None;
    let mut timeout_ms = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        let value = arguments
            .get(index + 1)
            .ok_or("error: scip-evidence option requires a value; use scip-evidence --help\n")?;
        index += 2;
        if workspace_arguments.accept_option(option, value)? {
            continue;
        }
        if option == OsStr::new("--database") {
            if value.is_empty() {
                return Err("error: scip-evidence database path must not be empty\n");
            }
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: scip-evidence accepts --database only once\n");
            }
            continue;
        }
        if option == OsStr::new("--symbol") {
            let value = value
                .to_str()
                .filter(|value| !value.is_empty())
                .ok_or("error: scip-evidence symbol must be non-empty Unicode\n")?;
            if symbol.replace(value.to_owned()).is_some() {
                return Err("error: scip-evidence accepts --symbol only once\n");
            }
            continue;
        }
        if option == OsStr::new("--package-root") {
            let value = value
                .to_str()
                .filter(|value| !value.is_empty())
                .ok_or("error: scip-evidence package root must be non-empty Unicode\n")?;
            package_roots.push(value.to_owned());
            continue;
        }
        if option == OsStr::new("--workspace-view") {
            let value = i64::try_from(parse_graph_u64(value)?)
                .map_err(|_| "error: scip-evidence workspace view is too large\n")?;
            if workspace_view.replace(value).is_some() {
                return Err("error: scip-evidence accepts --workspace-view only once\n");
            }
            continue;
        }
        if option == OsStr::new("--timeout-ms") {
            let value = parse_graph_u64(value)?;
            if timeout_ms.replace(value).is_some() {
                return Err("error: scip-evidence accepts --timeout-ms only once\n");
            }
            continue;
        }
        return Err("error: unsupported scip-evidence option; use scip-evidence --help\n");
    }
    let database = database
        .ok_or("error: scip-evidence requires --database; use scip-evidence --help\n")?;
    let workspace = workspace_arguments.into_context()?;
    let symbol = symbol
        .ok_or("error: scip-evidence requires --symbol; use scip-evidence --help\n")?;
    let request = ScipEvidenceInput {
        symbol,
        package_roots: (!package_roots.is_empty()).then_some(package_roots),
        workspace_view,
        timeout_ms,
    }
    .validate()
    .map_err(|_| "error: scip-evidence request is invalid or exceeds a resource bound\n")?;
    Ok(ScipEvidenceInvocation {
        database,
        workspace,
        request,
    })
}

fn emit_scip_evidence_output(writer: &mut impl Write, output: &ScipEvidenceOutput) -> u8 {
    let Ok(mut encoded) = serde_json::to_vec(output) else {
        return EXIT_SOFTWARE;
    };
    if encoded.len() >= 4 * 1024 * 1024 {
        return EXIT_SOFTWARE;
    }
    encoded.push(b'\n');
    if writer.write_all(&encoded).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}
