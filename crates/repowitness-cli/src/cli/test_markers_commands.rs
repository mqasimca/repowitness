const TEST_MARKERS_HELP: &str = concat!(
    "Read bounded parser-attributed raw test-marker observations from one indexed generation.\n\n",
    "Usage:\n",
    "  repowitness test-markers --repository-id <id> --database <path>\n",
    "      [--language <rust|go|typescript|tsx|python>]\n",
    "      [--path-prefix <repository-relative-prefix>] [--limit <1-1000>]\n\n",
    "Markers are raw syntax observations only. They do not prove test execution, test ownership,\n",
    "a containing declaration, or a resolved relationship.\n",
);

struct TestMarkersInvocation {
    database: PathBuf,
    repository_identity: String,
    language: Option<SourceLanguage>,
    path_prefix: Option<String>,
    max_results: u16,
}

fn run_test_markers(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_TEST_MARKERS_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_TEST_MARKERS_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: test-markers received too many arguments; use test-markers --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, TEST_MARKERS_HELP);
    }
    let invocation = match parse_test_markers_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let request = LocalTestMarkersRequest::new(&invocation.database, &invocation.repository_identity)
        .with_filters(invocation.language, invocation.path_prefix.as_deref())
        .with_max_results(invocation.max_results);
    let request = match request {
        Ok(request) => request,
        Err(_) => return emit_error(stderr, EXIT_USAGE, "error: test-markers request is invalid\n"),
    };
    let output = read_local_test_markers(request, Arc::new(AtomicBool::new(false)))
        .map_err(|_| ())
        .and_then(|result| mcp_test_markers_output(result).map_err(|_| ()));
    match output {
        Ok(output) => emit_test_markers_output(stdout, &output),
        Err(()) => emit_error(stderr, EXIT_SOFTWARE, "error: test-marker read failed\n"),
    }
}

fn parse_test_markers_arguments(
    arguments: &[OsString],
) -> Result<TestMarkersInvocation, &'static str> {
    let mut database = None;
    let mut repository_identity = None;
    let mut language = None;
    let mut path_prefix = None;
    let mut max_results = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        index += 1;
        let value = arguments.get(index).ok_or(
            "error: test-markers options require a value; use test-markers --help\n",
        )?;
        index += 1;
        if option == OsStr::new("--repository-id") {
            replace_once(
                &mut repository_identity,
                value.clone(),
                "error: test-markers --repository-id was supplied more than once\n",
            )?;
        } else if option == OsStr::new("--database") {
            replace_once(
                &mut database,
                PathBuf::from(value),
                "error: test-markers --database was supplied more than once\n",
            )?;
        } else if option == OsStr::new("--language") {
            let parsed = value
                .to_str()
                .and_then(SourceLanguage::from_stable_str)
                .ok_or("error: test-markers --language must be rust, go, typescript, tsx, or python\n")?;
            replace_once(
                &mut language,
                parsed,
                "error: test-markers --language was supplied more than once\n",
            )?;
        } else if option == OsStr::new("--path-prefix") {
            replace_once(
                &mut path_prefix,
                value.clone(),
                "error: test-markers --path-prefix was supplied more than once\n",
            )?;
        } else if option == OsStr::new("--limit") {
            let parsed = value
                .to_str()
                .and_then(|value| value.parse::<u16>().ok())
                .filter(|value| (1..=1_000).contains(value))
                .ok_or("error: test-markers --limit must be an integer from 1 through 1000\n")?;
            replace_once(
                &mut max_results,
                parsed,
                "error: test-markers --limit was supplied more than once\n",
            )?;
        } else {
            return Err("error: unknown test-markers option; use test-markers --help\n");
        }
    }
    let repository_identity = repository_identity
        .ok_or("error: test-markers requires --repository-id\n")?
        .into_string()
        .map_err(|_| "error: test-markers repository identity must be valid UTF-8\n")?;
    let path_prefix = path_prefix
        .map(OsString::into_string)
        .transpose()
        .map_err(|_| "error: test-markers path prefix must be valid UTF-8\n")?;
    Ok(TestMarkersInvocation {
        database: database.ok_or("error: test-markers requires --database\n")?,
        repository_identity,
        language,
        path_prefix,
        max_results: max_results.unwrap_or(100),
    })
}

fn emit_test_markers_output(writer: &mut impl Write, output: &TestMarkersOutput) -> u8 {
    let Ok(mut encoded) = serde_json::to_vec(output) else {
        return EXIT_SOFTWARE;
    };
    if encoded.len() > MAX_CLI_GRAPH_OUTPUT_BYTES {
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
mod test_markers_command_tests {
    use std::ffi::OsString;

    use super::parse_test_markers_arguments;

    #[test]
    fn parser_requires_exact_bounded_inputs() {
        let identity = format!("rwi1:h:{}", "01".repeat(32));
        let invocation = parse_test_markers_arguments(&[
            OsString::from("--database"),
            OsString::from("index.sqlite3"),
            OsString::from("--repository-id"),
            OsString::from(&identity),
            OsString::from("--language"),
            OsString::from("rust"),
            OsString::from("--limit"),
            OsString::from("1000"),
        ]).expect("bounded inputs should parse");
        assert_eq!(invocation.max_results, 1000);
        assert_eq!(invocation.repository_identity, identity);
        assert!(parse_test_markers_arguments(&[
            OsString::from("--database"), OsString::from("index.sqlite3"),
            OsString::from("--repository-id"), OsString::from("invalid"),
            OsString::from("--limit"), OsString::from("1001"),
        ]).is_err());
    }
}
