struct ScipRelationshipTraceInvocation {
    database: PathBuf,
    workspace: GraphWorkspaceContext,
    request: ScipRelationshipTraceServiceRequest,
}

fn run_scip_relationship_trace(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args
        .take(MAX_SCIP_RELATIONSHIP_TRACE_ARGUMENTS + 1)
        .collect();
    if arguments.len() > MAX_SCIP_RELATIONSHIP_TRACE_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: scip-relationship-trace received too many arguments; use scip-relationship-trace --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, SCIP_RELATIONSHIP_TRACE_HELP);
    }
    let invocation = match parse_scip_relationship_trace_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match read_local_scip_relationship_trace_service(
        &invocation.database,
        &invocation.workspace,
        invocation.request,
        Arc::new(AtomicBool::new(false)),
    ) {
        Ok(output) => emit_scip_relationship_trace_output(stdout, &output),
        Err(_) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: SCIP relationship trace failed\n",
        ),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one command parser keeps duplicate-option rejection and every bounded CLI control visibly complete"
)]
fn parse_scip_relationship_trace_arguments(
    arguments: &[OsString],
) -> Result<ScipRelationshipTraceInvocation, &'static str> {
    let mut workspace_arguments = GraphWorkspaceArguments::default();
    let mut database = None;
    let mut symbol = None;
    let mut package_roots = Vec::new();
    let mut workspace_view = None;
    let mut direction = None;
    let mut max_depth = None;
    let mut max_edges = None;
    let mut timeout_ms = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        let value = arguments.get(index + 1).ok_or(
            "error: scip-relationship-trace option requires a value; use scip-relationship-trace --help\n",
        )?;
        index += 2;
        if workspace_arguments.accept_option(option, value)? {
            continue;
        }
        if option == OsStr::new("--database") {
            if value.is_empty() {
                return Err("error: scip-relationship-trace database path must not be empty\n");
            }
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: scip-relationship-trace accepts --database only once\n");
            }
            continue;
        }
        if option == OsStr::new("--symbol") {
            let value = value
                .to_str()
                .filter(|value| !value.is_empty())
                .ok_or("error: scip-relationship-trace symbol must be non-empty Unicode\n")?;
            if symbol.replace(value.to_owned()).is_some() {
                return Err("error: scip-relationship-trace accepts --symbol only once\n");
            }
            continue;
        }
        if option == OsStr::new("--package-root") {
            let value = value
                .to_str()
                .filter(|value| !value.is_empty())
                .ok_or("error: scip-relationship-trace package root must be non-empty Unicode\n")?;
            package_roots.push(value.to_owned());
            continue;
        }
        if option == OsStr::new("--workspace-view") {
            let value = i64::try_from(parse_graph_u64(value)?)
                .map_err(|_| "error: scip-relationship-trace workspace view is too large\n")?;
            if workspace_view.replace(value).is_some() {
                return Err("error: scip-relationship-trace accepts --workspace-view only once\n");
            }
            continue;
        }
        if option == OsStr::new("--direction") {
            let value = value
                .to_str()
                .filter(|value| !value.is_empty())
                .ok_or("error: scip-relationship-trace direction must be non-empty Unicode\n")?;
            if direction.replace(value.to_owned()).is_some() {
                return Err("error: scip-relationship-trace accepts --direction only once\n");
            }
            continue;
        }
        if option == OsStr::new("--max-depth") {
            let value = u8::try_from(parse_graph_u64(value)?)
                .map_err(|_| "error: scip-relationship-trace max depth is too large\n")?;
            if max_depth.replace(value).is_some() {
                return Err("error: scip-relationship-trace accepts --max-depth only once\n");
            }
            continue;
        }
        if option == OsStr::new("--max-edges") {
            let value = u16::try_from(parse_graph_u64(value)?)
                .map_err(|_| "error: scip-relationship-trace max edges is too large\n")?;
            if max_edges.replace(value).is_some() {
                return Err("error: scip-relationship-trace accepts --max-edges only once\n");
            }
            continue;
        }
        if option == OsStr::new("--timeout-ms") {
            let value = parse_graph_u64(value)?;
            if timeout_ms.replace(value).is_some() {
                return Err("error: scip-relationship-trace accepts --timeout-ms only once\n");
            }
            continue;
        }
        return Err("error: unsupported scip-relationship-trace option; use scip-relationship-trace --help\n");
    }
    let database = database.ok_or(
        "error: scip-relationship-trace requires --database; use scip-relationship-trace --help\n",
    )?;
    let workspace = workspace_arguments.into_context()?;
    let symbol = symbol.ok_or(
        "error: scip-relationship-trace requires --symbol; use scip-relationship-trace --help\n",
    )?;
    let direction = direction.ok_or(
        "error: scip-relationship-trace requires --direction; use scip-relationship-trace --help\n",
    )?;
    let request = ScipRelationshipTraceInput {
        symbol,
        package_roots: (!package_roots.is_empty()).then_some(package_roots),
        workspace_view,
        direction,
        max_depth,
        max_edges,
        timeout_ms,
    }
    .validate()
    .map_err(|_| "error: scip-relationship-trace request is invalid or exceeds a resource bound\n")?;
    Ok(ScipRelationshipTraceInvocation {
        database,
        workspace,
        request,
    })
}

fn emit_scip_relationship_trace_output(
    writer: &mut impl Write,
    output: &ScipRelationshipTraceOutput,
) -> u8 {
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

#[cfg(test)]
mod scip_relationship_trace_command_tests {
    use std::ffi::OsString;

    use super::parse_scip_relationship_trace_arguments;

    #[test]
    fn parser_requires_bounded_explicit_direction() {
        let parsed = parse_scip_relationship_trace_arguments(&[
            OsString::from("--repository-id"),
            OsString::from(format!("rwi1:h:{}", "01".repeat(32))),
            OsString::from("--database"),
            OsString::from("index.sqlite3"),
            OsString::from("--symbol"),
            OsString::from("scip-rust pkg 1 Root."),
            OsString::from("--direction"),
            OsString::from("outgoing"),
            OsString::from("--max-depth"),
            OsString::from("2"),
            OsString::from("--max-edges"),
            OsString::from("8"),
        ])
        .expect("bounded request should parse");
        assert_eq!(parsed.request.max_depth().get(), 2);
        assert_eq!(parsed.request.max_edges().get(), 8);
        assert!(parse_scip_relationship_trace_arguments(&[
            OsString::from("--direction"),
            OsString::from("outgoing"),
        ])
        .is_err());
        assert!(parse_scip_relationship_trace_arguments(&[
            OsString::from("--repository-id"),
            OsString::from(format!("rwi1:h:{}", "01".repeat(32))),
            OsString::from("--database"),
            OsString::from("index.sqlite3"),
            OsString::from("--symbol"),
            OsString::from("scip-rust pkg 1 Root."),
            OsString::from("--direction"),
            OsString::from("both"),
        ])
        .is_err());
    }
}
