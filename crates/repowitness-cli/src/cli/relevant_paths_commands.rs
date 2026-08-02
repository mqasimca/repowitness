const RELEVANT_PATHS_HELP: &str = concat!(
    "Locate bounded lexically matching source paths.\n\n",
    "Usage:\n",
    "  repowitness locate-relevant-paths --repository-id <id> --database <path> --query <terms>\n",
    "      [--limit <1-50>] [configuration layer options]\n\n",
    "The command groups one generation-pinned literal declaration search by canonical path.\n",
    "Its order is returned declaration-match count then canonical path; it does not claim\n",
    "semantic relevance, dependencies, ownership, or relationships. The JSON result retains\n",
    "the complete attributed declaration evidence, coverage, and truncation receipt.\n",
);

struct RelevantPathsInvocation {
    database: PathBuf,
    repository_identity: String,
    query: String,
    max_paths: u16,
}

fn run_relevant_paths(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    configuration_loader: &impl ConfigurationLoader,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_RELEVANT_PATHS_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_RELEVANT_PATHS_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: locate-relevant-paths received too many arguments; use locate-relevant-paths --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, RELEVANT_PATHS_HELP);
    }
    let (arguments, configuration_invocation) = match extract_configuration_arguments(&arguments, &[]) {
        Ok(parsed) => parsed,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let invocation = match parse_relevant_paths_arguments(&arguments) {
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
    let request = LocalRelevantPathsRequest::new(
        &invocation.database,
        &invocation.repository_identity,
        &invocation.query,
    )
    .with_max_paths(invocation.max_paths)
    .expect("CLI parser already validates the relevant-path limit")
    .with_configuration(&configuration);
    let output = locate_local_relevant_paths(request, Arc::new(AtomicBool::new(false)))
        .map_err(|_| ())
        .and_then(|result| mcp_relevant_paths_output(result).map_err(|_| ()));
    match output {
        Ok(output) => emit_relevant_paths_output(stdout, &output),
        Err(()) => emit_error(stderr, EXIT_SOFTWARE, "error: relevant-path navigation failed\n"),
    }
}

fn parse_relevant_paths_arguments(
    arguments: &[OsString],
) -> Result<RelevantPathsInvocation, &'static str> {
    let mut database = None;
    let mut repository_identity = None;
    let mut query = None;
    let mut max_paths = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        index += 1;
        let value = arguments.get(index).ok_or(
            "error: locate-relevant-paths options require a value; use locate-relevant-paths --help\n",
        )?;
        index += 1;
        if option == OsStr::new("--repository-id") {
            replace_once(
                &mut repository_identity,
                value.to_owned(),
                "error: locate-relevant-paths --repository-id was supplied more than once\n",
            )?;
        } else if option == OsStr::new("--database") {
            replace_once(
                &mut database,
                PathBuf::from(value),
                "error: locate-relevant-paths --database was supplied more than once\n",
            )?;
        } else if option == OsStr::new("--query") {
            replace_once(
                &mut query,
                value.to_owned(),
                "error: locate-relevant-paths --query was supplied more than once\n",
            )?;
        } else if option == OsStr::new("--limit") {
            let limit = value
                .to_str()
                .and_then(|text| text.parse::<u16>().ok())
                .filter(|limit| (1..=50).contains(limit))
                .ok_or(
                    "error: locate-relevant-paths --limit must be an integer from 1 through 50\n",
                )?;
            replace_once(
                &mut max_paths,
                limit,
                "error: locate-relevant-paths --limit was supplied more than once\n",
            )?;
        } else {
            return Err(
                "error: unknown locate-relevant-paths option; use locate-relevant-paths --help\n",
            );
        }
    }
    let repository_identity = repository_identity
        .ok_or("error: locate-relevant-paths requires --repository-id\n")?
        .into_string()
        .map_err(|_| "error: locate-relevant-paths repository identity must be valid UTF-8\n")?;
    let query = query
        .ok_or("error: locate-relevant-paths requires --query\n")?
        .into_string()
        .map_err(|_| "error: locate-relevant-paths query must be valid UTF-8\n")?;
    Ok(RelevantPathsInvocation {
        database: database.ok_or("error: locate-relevant-paths requires --database\n")?,
        repository_identity,
        query,
        max_paths: max_paths.unwrap_or(12),
    })
}

fn emit_relevant_paths_output(writer: &mut impl Write, output: &RelevantPathsOutput) -> u8 {
    let Ok(mut encoded) = serde_json::to_vec(output) else {
        return EXIT_SOFTWARE;
    };
    if encoded.len() > MAX_CLI_SEARCH_OUTPUT_BYTES {
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
mod relevant_paths_command_tests {
    use std::ffi::OsString;

    use super::{
        CONFIGURATION_LAYER_ARGUMENTS, MAX_RELEVANT_PATHS_ARGUMENTS, parse_relevant_paths_arguments,
    };

    #[test]
    fn parser_admits_one_bounded_literal_path_query() {
        let identity = format!("rwi1:h:{}", "01".repeat(32));
        let parsed = parse_relevant_paths_arguments(&[
            OsString::from("--repository-id"),
            OsString::from(identity),
            OsString::from("--database"),
            OsString::from("index.sqlite3"),
            OsString::from("--query"),
            OsString::from("Widget run"),
            OsString::from("--limit"),
            OsString::from("50"),
        ])
        .expect("valid bounded query");
        assert_eq!(parsed.max_paths, 50);
        assert_eq!(MAX_RELEVANT_PATHS_ARGUMENTS, 8 + CONFIGURATION_LAYER_ARGUMENTS);
        assert!(parse_relevant_paths_arguments(&[
            OsString::from("--repository-id"),
            OsString::from("id"),
            OsString::from("--database"),
            OsString::from("index.sqlite3"),
            OsString::from("--query"),
            OsString::from("Widget"),
            OsString::from("--limit"),
            OsString::from("51"),
        ])
        .is_err());
    }
}
