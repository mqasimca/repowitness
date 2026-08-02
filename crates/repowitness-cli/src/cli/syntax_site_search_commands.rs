struct SyntaxSiteSearchInvocation {
    database: PathBuf,
    repository_identity: String,
    target: String,
    max_sites: u16,
}

fn run_syntax_site_search(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_SYNTAX_SITE_SEARCH_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_SYNTAX_SITE_SEARCH_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: syntax-site-search received too many arguments; use syntax-site-search --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, SYNTAX_SITE_SEARCH_HELP);
    }
    let invocation = match parse_syntax_site_search_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let request = LocalSyntaxSiteSearchRequest::new(
        &invocation.database,
        &invocation.repository_identity,
        &invocation.target,
    )
    .with_max_results(invocation.max_sites);
    let request = match request {
        Ok(request) => request,
        Err(_) => {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: syntax-site-search request is invalid\n",
            );
        }
    };
    let output = search_local_syntax_sites(request, Arc::new(AtomicBool::new(false)))
        .map_err(|_| ())
        .and_then(|result| mcp_syntax_site_search_output(result).map_err(|_| ()));
    match output {
        Ok(output) => emit_syntax_site_search_output(stdout, &output),
        Err(()) => emit_error(stderr, EXIT_SOFTWARE, "error: syntax-site-search read failed\n"),
    }
}

fn parse_syntax_site_search_arguments(
    arguments: &[OsString],
) -> Result<SyntaxSiteSearchInvocation, &'static str> {
    let mut database = None;
    let mut repository_identity = None;
    let mut target = None;
    let mut max_sites = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        let value = arguments.get(index + 1).ok_or(
            "error: syntax-site-search options require a value; use syntax-site-search --help\n",
        )?;
        index += 2;
        if option == OsStr::new("--database") {
            replace_once(
                &mut database,
                PathBuf::from(value),
                "error: syntax-site-search --database was supplied more than once\n",
            )?;
        } else if option == OsStr::new("--repository-id") {
            replace_once(
                &mut repository_identity,
                value.clone(),
                "error: syntax-site-search --repository-id was supplied more than once\n",
            )?;
        } else if option == OsStr::new("--target") {
            replace_once(
                &mut target,
                value.clone(),
                "error: syntax-site-search --target was supplied more than once\n",
            )?;
        } else if option == OsStr::new("--max-sites") {
            let value = value
                .to_str()
                .and_then(|value| value.parse::<u16>().ok())
                .filter(|value| (1..=250).contains(value))
                .ok_or(
                    "error: syntax-site-search --max-sites must be an integer from 1 through 250\n",
                )?;
            replace_once(
                &mut max_sites,
                value,
                "error: syntax-site-search --max-sites was supplied more than once\n",
            )?;
        } else {
            return Err("error: unknown syntax-site-search option; use syntax-site-search --help\n");
        }
    }
    let text = |value: Option<OsString>, missing| {
        value
            .ok_or(missing)?
            .into_string()
            .map_err(|_| "error: syntax-site-search text must be valid UTF-8\n")
    };
    let target = text(target, "error: syntax-site-search requires --target\n")?;
    if SyntaxSiteSearchQuery::try_new(&target).is_err() {
        return Err("error: syntax-site-search --target is outside the exact raw-syntax profile\n");
    }
    Ok(SyntaxSiteSearchInvocation {
        database: database.ok_or("error: syntax-site-search requires --database\n")?,
        repository_identity: text(
            repository_identity,
            "error: syntax-site-search requires --repository-id\n",
        )?,
        target,
        max_sites: max_sites.unwrap_or(100),
    })
}

fn emit_syntax_site_search_output(
    writer: &mut impl Write,
    output: &SyntaxSiteSearchOutput,
) -> u8 {
    let Ok(mut encoded) = serde_json::to_vec(output) else {
        return EXIT_SOFTWARE;
    };
    if encoded.len() > MAX_CLI_SYNTAX_SITE_SEARCH_OUTPUT_BYTES {
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
mod syntax_site_search_command_tests {
    use std::ffi::OsString;

    use super::parse_syntax_site_search_arguments;

    #[test]
    fn parser_admits_one_exact_bounded_raw_target() {
        let parsed = parse_syntax_site_search_arguments(&[
            OsString::from("--repository-id"),
            OsString::from(format!("rwi1:h:{}", "01".repeat(32))),
            OsString::from("--database"),
            OsString::from("index.sqlite3"),
            OsString::from("--target"),
            OsString::from("crate::entry"),
            OsString::from("--max-sites"),
            OsString::from("250"),
        ])
        .expect("exact bounded target should parse");
        assert_eq!(parsed.max_sites, 250);
        assert!(parse_syntax_site_search_arguments(&[
            OsString::from("--target"),
            OsString::new(),
        ])
        .is_err());
    }
}
