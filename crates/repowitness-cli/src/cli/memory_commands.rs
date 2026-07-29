fn run_memory_revalidate(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    memory: &impl RepositoryMemory,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_MEMORY_REVALIDATE_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_MEMORY_REVALIDATE_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: memory-revalidate received too many arguments; use memory-revalidate --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, MEMORY_REVALIDATE_HELP);
    }
    let invocation = match parse_memory_revalidate_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match memory.revalidate(&invocation) {
        Ok(report) => emit_memory_revalidation_report(stdout, report),
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: memory revalidation failed\n"),
    }
}

fn parse_memory_revalidate_arguments(
    arguments: &[OsString],
) -> Result<MemoryRevalidationInvocation, &'static str> {
    let parsed = parse_repository_database_arguments(arguments, "memory-revalidate")?;
    Ok(MemoryRevalidationInvocation {
        repository_root: parsed.repository_root,
        database: parsed.database,
        repository_identity: parsed.repository_identity,
    })
}

fn run_memory_recall(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    memory: &impl RepositoryMemory,
    configuration_loader: &impl ConfigurationLoader,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_MEMORY_RECALL_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_MEMORY_RECALL_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: memory-recall received too many arguments; use memory-recall --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, MEMORY_RECALL_HELP);
    }
    let (arguments, configuration_invocation) =
        match extract_configuration_arguments(&arguments, &["--all"]) {
            Ok(parsed) => parsed,
            Err(message) => return emit_error(stderr, EXIT_USAGE, message),
        };
    let invocation = match parse_memory_recall_arguments(&arguments) {
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
    match memory.recall(&invocation, &configuration) {
        Ok(report) => emit_memory_recall_report(stdout, &report),
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: memory recall failed\n"),
    }
}

fn parse_memory_recall_arguments(
    arguments: &[OsString],
) -> Result<MemoryRecallInvocation, &'static str> {
    let mut repository_identity = None;
    let mut database = None;
    let mut query = None;
    let mut all = false;
    let mut max_results = 20_u16;
    let mut limit_seen = false;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        index += 1;
        if option == OsStr::new("--all") {
            if all {
                return Err("error: memory-recall accepts --all only once\n");
            }
            all = true;
            continue;
        }
        let value = arguments
            .get(index)
            .ok_or("error: memory-recall option requires a value; use memory-recall --help\n")?;
        index += 1;
        if option == OsStr::new("--repository-id") {
            if repository_identity.replace(value.clone()).is_some() {
                return Err("error: memory-recall accepts --repository-id only once\n");
            }
        } else if option == OsStr::new("--database") {
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: memory-recall accepts --database only once\n");
            }
        } else if option == OsStr::new("--query") {
            if query.replace(value.clone()).is_some() {
                return Err("error: memory-recall accepts --query only once\n");
            }
        } else if option == OsStr::new("--limit") {
            if limit_seen {
                return Err("error: memory-recall accepts --limit only once\n");
            }
            max_results = value
                .to_str()
                .and_then(|text| text.parse::<u16>().ok())
                .filter(|limit| (1..=100).contains(limit))
                .ok_or("error: memory-recall --limit must be an integer from 1 through 100\n")?;
            limit_seen = true;
        } else {
            return Err("error: unknown memory-recall option; use memory-recall --help\n");
        }
    }
    let repository_identity = repository_identity
        .ok_or("error: memory-recall requires --repository-id; use memory-recall --help\n")?;
    if repository_identity.is_empty() {
        return Err("error: memory-recall repository identity must not be empty\n");
    }
    let database =
        database.ok_or("error: memory-recall requires --database; use memory-recall --help\n")?;
    if database.as_os_str().is_empty() {
        return Err("error: memory-recall database path must not be empty\n");
    }
    let selection = match (query, all) {
        (Some(query), false) if !query.is_empty() => CliMemoryRecallSelection::Query(query),
        (None, true) => CliMemoryRecallSelection::All,
        _ => {
            return Err("error: memory-recall requires exactly one of --query <text> or --all\n");
        }
    };
    Ok(MemoryRecallInvocation {
        database,
        repository_identity,
        selection,
        max_results,
    })
}

struct RepositoryDatabaseInvocation {
    repository_root: PathBuf,
    database: PathBuf,
    repository_identity: OsString,
}

fn parse_repository_database_arguments(
    arguments: &[OsString],
    command: &'static str,
) -> Result<RepositoryDatabaseInvocation, &'static str> {
    let mut repository_identity = None;
    let mut database = None;
    let mut repository_root = None;
    let mut positional_only = false;
    let mut index = 0_usize;
    while index < arguments.len() {
        let argument = &arguments[index];
        if positional_only {
            set_memory_repository_root(&mut repository_root, argument)?;
            index += 1;
            continue;
        }
        if argument == OsStr::new("--") {
            positional_only = true;
            index += 1;
            continue;
        }
        if argument == OsStr::new("--repository-id") || argument == OsStr::new("--database") {
            let option = argument.clone();
            index += 1;
            let value = arguments.get(index).ok_or(
                "error: memory-revalidate option requires a value; use memory-revalidate --help\n",
            )?;
            if option == OsStr::new("--repository-id") {
                if repository_identity.replace(value.clone()).is_some() {
                    return Err("error: memory-revalidate accepts --repository-id only once\n");
                }
            } else if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: memory-revalidate accepts --database only once\n");
            }
            index += 1;
            continue;
        }
        if argument == OsStr::new("--help") || argument == OsStr::new("-h") {
            return Err("error: memory-revalidate --help accepts no additional arguments\n");
        }
        if os_string_starts_with_hyphen(argument) {
            return Err("error: unknown memory-revalidate option; use memory-revalidate --help\n");
        }
        set_memory_repository_root(&mut repository_root, argument)?;
        index += 1;
    }
    debug_assert_eq!(command, "memory-revalidate");
    let repository_identity = repository_identity.ok_or(
        "error: memory-revalidate requires --repository-id; use memory-revalidate --help\n",
    )?;
    if repository_identity.is_empty() {
        return Err("error: memory-revalidate repository identity must not be empty\n");
    }
    let database = database
        .ok_or("error: memory-revalidate requires --database; use memory-revalidate --help\n")?;
    if database.as_os_str().is_empty() {
        return Err("error: memory-revalidate database path must not be empty\n");
    }
    let repository_root = repository_root.ok_or(
        "error: memory-revalidate requires one repository; use memory-revalidate --help\n",
    )?;
    Ok(RepositoryDatabaseInvocation {
        repository_root,
        database,
        repository_identity,
    })
}

fn set_memory_repository_root(
    repository_root: &mut Option<PathBuf>,
    value: &OsStr,
) -> Result<(), &'static str> {
    if value.is_empty() {
        return Err("error: memory-revalidate repository must not be empty\n");
    }
    if repository_root.replace(PathBuf::from(value)).is_some() {
        return Err("error: memory-revalidate accepts exactly one repository\n");
    }
    Ok(())
}
