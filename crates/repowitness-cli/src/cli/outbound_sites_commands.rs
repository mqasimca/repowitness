struct OutboundSitesInvocation {
    database: PathBuf,
    repository_identity: String,
    snapshot: String,
    generation: i64,
    path: String,
    content: String,
    artifact: String,
    fact_ordinal: u64,
    max_sites: u16,
}

fn run_outbound_sites(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_OUTBOUND_SITES_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_OUTBOUND_SITES_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: outbound-sites received too many arguments; use outbound-sites --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, OUTBOUND_SITES_HELP);
    }
    let invocation = match parse_outbound_sites_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let selector = LocalSymbolSelectorText::new(
        &invocation.snapshot,
        invocation.generation,
        &invocation.path,
        &invocation.content,
        &invocation.artifact,
        invocation.fact_ordinal,
    );
    let request = LocalOutboundSitesRequest::new(
        &invocation.database,
        &invocation.repository_identity,
        selector,
    )
    .with_max_results(invocation.max_sites);
    let request = match request {
        Ok(request) => request,
        Err(_) => return emit_error(stderr, EXIT_USAGE, "error: outbound-sites request is invalid\n"),
    };
    let output = get_local_outbound_sites(request, Arc::new(AtomicBool::new(false)))
        .map_err(|_| ())
        .and_then(|result| mcp_outbound_sites_output(result).map_err(|_| ()));
    match output {
        Ok(output) => emit_outbound_sites_output(stdout, &output),
        Err(()) => emit_error(stderr, EXIT_SOFTWARE, "error: outbound-sites read failed\n"),
    }
}

fn parse_outbound_sites_arguments(
    arguments: &[OsString],
) -> Result<OutboundSitesInvocation, &'static str> {
    let mut database = None;
    let mut repository_identity = None;
    let mut snapshot = None;
    let mut generation = None;
    let mut path = None;
    let mut content = None;
    let mut artifact = None;
    let mut fact = None;
    let mut max_sites = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        let value = arguments.get(index + 1).ok_or(
            "error: outbound-sites options require a value; use outbound-sites --help\n",
        )?;
        index += 2;
        if option == OsStr::new("--database") {
            replace_once(&mut database, PathBuf::from(value), "error: outbound-sites --database was supplied more than once\n")?;
        } else if option == OsStr::new("--repository-id") {
            replace_once(&mut repository_identity, value.clone(), "error: outbound-sites --repository-id was supplied more than once\n")?;
        } else if option == OsStr::new("--snapshot") {
            replace_once(&mut snapshot, value.clone(), "error: outbound-sites --snapshot was supplied more than once\n")?;
        } else if option == OsStr::new("--generation") {
            replace_once(&mut generation, value.clone(), "error: outbound-sites --generation was supplied more than once\n")?;
        } else if option == OsStr::new("--path") {
            replace_once(&mut path, value.clone(), "error: outbound-sites --path was supplied more than once\n")?;
        } else if option == OsStr::new("--content") {
            replace_once(&mut content, value.clone(), "error: outbound-sites --content was supplied more than once\n")?;
        } else if option == OsStr::new("--artifact") {
            replace_once(&mut artifact, value.clone(), "error: outbound-sites --artifact was supplied more than once\n")?;
        } else if option == OsStr::new("--fact") {
            replace_once(&mut fact, value.clone(), "error: outbound-sites --fact was supplied more than once\n")?;
        } else if option == OsStr::new("--max-sites") {
            let value = value.to_str().and_then(|value| value.parse::<u16>().ok())
                .filter(|value| (1..=250).contains(value))
                .ok_or("error: outbound-sites --max-sites must be an integer from 1 through 250\n")?;
            replace_once(&mut max_sites, value, "error: outbound-sites --max-sites was supplied more than once\n")?;
        } else {
            return Err("error: unknown outbound-sites option; use outbound-sites --help\n");
        }
    }
    let text = |value: Option<OsString>, missing| {
        value.ok_or(missing)?.into_string().map_err(|_| "error: outbound-sites text must be valid UTF-8\n")
    };
    let generation = text(generation, "error: outbound-sites requires --generation\n")?
        .parse::<i64>().ok().filter(|value| *value > 0)
        .ok_or("error: outbound-sites --generation must be a positive integer\n")?;
    let fact_ordinal = text(fact, "error: outbound-sites requires --fact\n")?
        .parse::<u64>().ok().filter(|value| *value <= MAX_MCP_INTEROPERABLE_INTEGER)
        .ok_or("error: outbound-sites --fact is outside the interoperable integer range\n")?;
    Ok(OutboundSitesInvocation {
        database: database.ok_or("error: outbound-sites requires --database\n")?,
        repository_identity: text(repository_identity, "error: outbound-sites requires --repository-id\n")?,
        snapshot: text(snapshot, "error: outbound-sites requires --snapshot\n")?,
        generation,
        path: text(path, "error: outbound-sites requires --path\n")?,
        content: text(content, "error: outbound-sites requires --content\n")?,
        artifact: text(artifact, "error: outbound-sites requires --artifact\n")?,
        fact_ordinal,
        max_sites: max_sites.unwrap_or(100),
    })
}

fn emit_outbound_sites_output(writer: &mut impl Write, output: &OutboundSitesOutput) -> u8 {
    let Ok(mut encoded) = serde_json::to_vec(output) else {
        return EXIT_SOFTWARE;
    };
    if encoded.len() > MAX_CLI_OUTBOUND_SITES_OUTPUT_BYTES {
        return EXIT_SOFTWARE;
    }
    encoded.push(b'\n');
    if writer.write_all(&encoded).is_ok() { EXIT_SUCCESS } else { EXIT_IO }
}

#[cfg(test)]
mod outbound_sites_command_tests {
    use std::ffi::OsString;

    use super::parse_outbound_sites_arguments;

    #[test]
    fn parser_accepts_only_an_exact_bounded_declaration_selector() {
        let parsed = parse_outbound_sites_arguments(&[
            OsString::from("--repository-id"), OsString::from(format!("rwi1:h:{}", "01".repeat(32))),
            OsString::from("--database"), OsString::from("index.sqlite3"),
            OsString::from("--snapshot"), OsString::from("ab".repeat(32)),
            OsString::from("--generation"), OsString::from("1"),
            OsString::from("--path"), OsString::from("rwp1:h:61"),
            OsString::from("--content"), OsString::from("cd".repeat(32)),
            OsString::from("--artifact"), OsString::from("ef".repeat(32)),
            OsString::from("--fact"), OsString::from("0"),
            OsString::from("--max-sites"), OsString::from("250"),
        ]).expect("exact bounded selector should parse");
        assert_eq!(parsed.max_sites, 250);
        assert!(parse_outbound_sites_arguments(&[OsString::from("--unknown"), OsString::from("x")]).is_err());
    }
}
