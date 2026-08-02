struct ScipSymbolResolveInvocation {
    database: PathBuf,
    workspace: GraphWorkspaceContext,
    request: ScipSymbolResolveServiceRequest,
}

fn run_scip_symbol_resolve(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_SCIP_SYMBOL_RESOLVE_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_SCIP_SYMBOL_RESOLVE_ARGUMENTS {
        return emit_error(stderr, EXIT_USAGE, "error: scip-symbol-resolve received too many arguments; use scip-symbol-resolve --help\n");
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, SCIP_SYMBOL_RESOLVE_HELP);
    }
    let invocation = match parse_scip_symbol_resolve_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match read_local_scip_symbol_resolve_service(
        &invocation.database,
        &invocation.workspace,
        invocation.request,
        Arc::new(AtomicBool::new(false)),
    ) {
        Ok(output) => emit_scip_symbol_resolve_output(stdout, &output),
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: SCIP symbol resolution failed\n"),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "the strict option allow-list keeps every untrusted exact-selector field auditable at one CLI boundary"
)]
fn parse_scip_symbol_resolve_arguments(
    arguments: &[OsString],
) -> Result<ScipSymbolResolveInvocation, &'static str> {
    let mut workspace_arguments = GraphWorkspaceArguments::default();
    let mut database = None;
    let mut snapshot_sha256 = None;
    let mut generation = None;
    let mut path = None;
    let mut content_sha256 = None;
    let mut artifact_sha256 = None;
    let mut fact_ordinal = None;
    let mut name_start = None;
    let mut name_end = None;
    let mut workspace_view = None;
    let mut timeout_ms = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        let value = arguments
            .get(index + 1)
            .ok_or("error: scip-symbol-resolve options require a value; use scip-symbol-resolve --help\n")?;
        index += 2;
        if workspace_arguments.accept_option(option, value)? {
            continue;
        }
        if option == OsStr::new("--database") {
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: scip-symbol-resolve accepts --database only once\n");
            }
        } else if option == OsStr::new("--snapshot") {
            replace_once(&mut snapshot_sha256, value.clone(), "error: scip-symbol-resolve accepts --snapshot only once\n")?;
        } else if option == OsStr::new("--generation") {
            replace_once(&mut generation, value.clone(), "error: scip-symbol-resolve accepts --generation only once\n")?;
        } else if option == OsStr::new("--path") {
            replace_once(&mut path, value.clone(), "error: scip-symbol-resolve accepts --path only once\n")?;
        } else if option == OsStr::new("--content") {
            replace_once(&mut content_sha256, value.clone(), "error: scip-symbol-resolve accepts --content only once\n")?;
        } else if option == OsStr::new("--artifact") {
            replace_once(&mut artifact_sha256, value.clone(), "error: scip-symbol-resolve accepts --artifact only once\n")?;
        } else if option == OsStr::new("--fact-ordinal") {
            replace_once(&mut fact_ordinal, value.clone(), "error: scip-symbol-resolve accepts --fact-ordinal only once\n")?;
        } else if option == OsStr::new("--name-start") {
            replace_once(&mut name_start, value.clone(), "error: scip-symbol-resolve accepts --name-start only once\n")?;
        } else if option == OsStr::new("--name-end") {
            replace_once(&mut name_end, value.clone(), "error: scip-symbol-resolve accepts --name-end only once\n")?;
        } else if option == OsStr::new("--workspace-view") {
            replace_once(&mut workspace_view, value.clone(), "error: scip-symbol-resolve accepts --workspace-view only once\n")?;
        } else if option == OsStr::new("--timeout-ms") {
            replace_once(&mut timeout_ms, value.clone(), "error: scip-symbol-resolve accepts --timeout-ms only once\n")?;
        } else {
            return Err("error: unsupported scip-symbol-resolve option; use scip-symbol-resolve --help\n");
        }
    }
    let text = |value: Option<OsString>, missing| {
        value
            .ok_or(missing)?
            .into_string()
            .map_err(|_| "error: scip-symbol-resolve text must be valid UTF-8\n")
    };
    let name_start = parse_graph_u64(
        name_start
            .as_deref()
            .ok_or("error: scip-symbol-resolve requires --name-start\n")?,
    )?;
    let name_end = parse_graph_u64(
        name_end
            .as_deref()
            .ok_or("error: scip-symbol-resolve requires --name-end\n")?,
    )?;
    let workspace_view = match workspace_view {
        Some(value) => Some(i64::try_from(parse_graph_u64(&value)?).map_err(|_| "error: scip-symbol-resolve workspace view is too large\n")?),
        None => None,
    };
    let timeout_ms = match timeout_ms {
        Some(value) => Some(parse_graph_u64(&value)?),
        None => None,
    };
    let request = ScipSymbolResolveInput {
        snapshot_sha256: text(snapshot_sha256, "error: scip-symbol-resolve requires --snapshot\n")?,
        generation: i64::try_from(parse_graph_u64(
            generation
                .as_deref()
                .ok_or("error: scip-symbol-resolve requires --generation\n")?,
        )?)
        .map_err(|_| "error: scip-symbol-resolve generation is too large\n")?,
        path: text(path, "error: scip-symbol-resolve requires --path\n")?,
        content_sha256: text(content_sha256, "error: scip-symbol-resolve requires --content\n")?,
        artifact_sha256: text(artifact_sha256, "error: scip-symbol-resolve requires --artifact\n")?,
        fact_ordinal: parse_graph_u64(
            fact_ordinal
                .as_deref()
                .ok_or("error: scip-symbol-resolve requires --fact-ordinal\n")?,
        )?,
        name_span: McpSpan { start: name_start, end: name_end },
        workspace_view,
        timeout_ms,
    }
    .validate()
    .map_err(|_| "error: scip-symbol-resolve request is invalid or exceeds a resource bound\n")?;
    Ok(ScipSymbolResolveInvocation {
        database: database.ok_or("error: scip-symbol-resolve requires --database\n")?,
        workspace: workspace_arguments.into_context()?,
        request,
    })
}

fn emit_scip_symbol_resolve_output(writer: &mut impl Write, output: &ScipSymbolResolveOutput) -> u8 {
    let Ok(mut encoded) = serde_json::to_vec(output) else { return EXIT_SOFTWARE; };
    if encoded.len() >= 4 * 1024 * 1024 { return EXIT_SOFTWARE; }
    encoded.push(b'\n');
    if writer.write_all(&encoded).is_ok() { EXIT_SUCCESS } else { EXIT_IO }
}

#[cfg(test)]
mod scip_symbol_resolve_command_tests {
    use std::ffi::OsString;

    use super::parse_scip_symbol_resolve_arguments;

    #[test]
    fn parser_accepts_only_an_exact_bounded_span() {
        let parsed = parse_scip_symbol_resolve_arguments(&[
            OsString::from("--repository-id"), OsString::from(format!("rwi1:h:{}", "01".repeat(32))),
            OsString::from("--database"), OsString::from("index.sqlite3"),
            OsString::from("--snapshot"), OsString::from("cd".repeat(32)),
            OsString::from("--generation"), OsString::from("3"),
            OsString::from("--path"), OsString::from("rwp1:h:7372632F6C69622E7273"),
            OsString::from("--content"), OsString::from("ab".repeat(32)),
            OsString::from("--artifact"), OsString::from("ef".repeat(32)),
            OsString::from("--fact-ordinal"), OsString::from("7"),
            OsString::from("--name-start"), OsString::from("4"),
            OsString::from("--name-end"), OsString::from("8"),
        ]).expect("exact bounded span should parse");
        assert_eq!(parsed.request.name_span().start, 4);
        assert!(parse_scip_symbol_resolve_arguments(&[
            OsString::from("--name-start"), OsString::from("8"),
            OsString::from("--name-end"), OsString::from("8"),
        ]).is_err());
    }
}
