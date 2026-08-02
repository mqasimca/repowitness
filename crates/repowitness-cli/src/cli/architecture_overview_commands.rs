struct ArchitectureOverviewInvocation {
    database: PathBuf,
    repository_identity: String,
    max_roots: u16,
    max_entry_point_candidates: u16,
    max_files: u16,
}

fn run_architecture_overview(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args
        .take(MAX_ARCHITECTURE_OVERVIEW_ARGUMENTS + 1)
        .collect();
    if arguments.len() > MAX_ARCHITECTURE_OVERVIEW_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: architecture-overview received too many arguments; use architecture-overview --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, ARCHITECTURE_OVERVIEW_HELP);
    }
    let invocation = match parse_architecture_overview_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let request = match LocalArchitectureOverviewRequest::new(
        &invocation.database,
        &invocation.repository_identity,
    )
    .with_limits(
        invocation.max_roots,
        invocation.max_entry_point_candidates,
        invocation.max_files,
    ) {
        Ok(request) => request,
        Err(_) => {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: architecture-overview request is invalid or exceeds a resource bound\n",
            );
        }
    };
    let output = overview_local_architecture(request, Arc::new(AtomicBool::new(false)))
        .map_err(|_| ())
        .and_then(|result| mcp_architecture_overview_output(result).map_err(|_| ()));
    match output {
        Ok(output) => emit_architecture_overview_output(stdout, &output),
        Err(()) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: architecture overview failed\n",
        ),
    }
}

fn parse_architecture_overview_arguments(
    arguments: &[OsString],
) -> Result<ArchitectureOverviewInvocation, &'static str> {
    let mut database = None;
    let mut repository_identity = None;
    let mut max_roots = None;
    let mut max_entry_point_candidates = None;
    let mut max_files = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        index += 1;
        let value = arguments.get(index).ok_or(
            "error: architecture-overview requires --repository-id and --database; use architecture-overview --help\n",
        )?;
        index += 1;
        if option == OsStr::new("--repository-id") {
            if repository_identity.replace(value.clone()).is_some() {
                return Err(
                    "error: architecture-overview --repository-id was supplied more than once\n",
                );
            }
        } else if option == OsStr::new("--database") {
            if database.replace(PathBuf::from(value)).is_some() {
                return Err(
                    "error: architecture-overview --database was supplied more than once\n",
                );
            }
        } else if option == OsStr::new("--max-roots") {
            if max_roots.replace(parse_architecture_overview_bound(
                value,
                MAX_ARCHITECTURE_OVERVIEW_ROOTS,
                "--max-roots",
            )?).is_some() {
                return Err("error: architecture-overview --max-roots was supplied more than once\n");
            }
        } else if option == OsStr::new("--max-entry-point-candidates") {
            if max_entry_point_candidates.replace(parse_architecture_overview_bound(
                value,
                MAX_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES,
                "--max-entry-point-candidates",
            )?).is_some() {
                return Err(
                    "error: architecture-overview --max-entry-point-candidates was supplied more than once\n",
                );
            }
        } else if option == OsStr::new("--max-files") {
            if max_files.replace(parse_architecture_overview_bound(
                value,
                MAX_ARCHITECTURE_OVERVIEW_FILES,
                "--max-files",
            )?).is_some() {
                return Err("error: architecture-overview --max-files was supplied more than once\n");
            }
        } else {
            return Err(
                "error: architecture-overview accepts only --repository-id, --database, --max-roots, --max-entry-point-candidates, and --max-files\n",
            );
        }
    }
    let repository_identity = repository_identity
        .ok_or("error: architecture-overview requires --repository-id\n")?
        .into_string()
        .map_err(|_| "error: architecture-overview repository identity must be valid UTF-8\n")?;
    let database = database.ok_or("error: architecture-overview requires --database\n")?;
    Ok(ArchitectureOverviewInvocation {
        database,
        repository_identity,
        max_roots: max_roots.unwrap_or(DEFAULT_ARCHITECTURE_OVERVIEW_ROOTS),
        max_entry_point_candidates: max_entry_point_candidates
            .unwrap_or(DEFAULT_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES),
        max_files: max_files.unwrap_or(DEFAULT_ARCHITECTURE_OVERVIEW_FILES),
    })
}

fn parse_architecture_overview_bound(
    value: &OsString,
    maximum: u16,
    option: &'static str,
) -> Result<u16, &'static str> {
    let valid = value
        .to_str()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| (1..=maximum).contains(value));
    if let Some(valid) = valid {
        return Ok(valid);
    }
    match option {
        "--max-roots" => Err("error: architecture-overview --max-roots must be between 1 and 500\n"),
        "--max-entry-point-candidates" => Err(
            "error: architecture-overview --max-entry-point-candidates must be between 1 and 500\n",
        ),
        "--max-files" => Err("error: architecture-overview --max-files must be between 1 and 1000\n"),
        _ => Err("error: architecture-overview received an invalid bound\n"),
    }
}

fn emit_architecture_overview_output(
    writer: &mut impl Write,
    output: &ArchitectureOverviewOutput,
) -> u8 {
    let Ok(mut encoded) = serde_json::to_vec(output) else {
        return EXIT_SOFTWARE;
    };
    if encoded.len() > MAX_CLI_ARCHITECTURE_OVERVIEW_OUTPUT_BYTES {
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
mod architecture_overview_command_tests {
    use std::ffi::OsString;

    use super::parse_architecture_overview_arguments;

    #[test]
    fn parser_requires_independent_bounded_inputs() {
        let identity = format!("rwi1:h:{}", "01".repeat(32));
        let invocation = parse_architecture_overview_arguments(&[
            OsString::from("--database"),
            OsString::from("index.sqlite3"),
            OsString::from("--max-roots"),
            OsString::from("500"),
            OsString::from("--max-entry-point-candidates"),
            OsString::from("500"),
            OsString::from("--max-files"),
            OsString::from("1000"),
            OsString::from("--repository-id"),
            OsString::from(&identity),
        ])
        .expect("complete inputs should parse");
        assert_eq!(invocation.max_roots, 500);
        assert_eq!(invocation.max_entry_point_candidates, 500);
        assert_eq!(invocation.max_files, 1000);
        assert!(parse_architecture_overview_arguments(&[
            OsString::from("--database"),
            OsString::from("index.sqlite3"),
            OsString::from("--repository-id"),
            OsString::from("invalid"),
            OsString::from("--max-files"),
            OsString::from("1001"),
        ])
        .is_err());
    }
}
