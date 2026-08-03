/// Parses and executes one CLI invocation with explicit output destinations.
///
/// The first argument is treated as the executable name. The returned value is
/// a process exit code: `0` for success, `64` for invalid usage, `70` for an
/// operation failure, and `74` for output failure. The `inspect-paths` command
/// is read-only and never creates an index.
pub fn run(args: impl IntoIterator<Item = OsString>, stdout: impl Write, stderr: impl Write) -> u8 {
    let mut arguments = args.into_iter();
    let program = arguments.next();
    let command = arguments.next();
    let is_watch = command.as_deref() == Some(OsStr::new("watch"));
    let is_gc = command.as_deref() == Some(OsStr::new("gc"));
    let arguments = program.into_iter().chain(command).chain(arguments);
    if is_watch {
        return run_watch(arguments, stdout, stderr);
    }
    if is_gc {
        return run_gc(arguments, stdout, stderr);
    }
    run_with_adapters(
        arguments,
        stdout,
        stderr,
        &LocalRepositoryPathInspector,
        &LocalRepositoryIndexer,
        &LocalRepositorySearcher,
        &LocalRepositorySymbolGetter,
        &LocalRepositoryMemory,
        &LocalConfigurationLoader,
    )
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "each independently testable CLI boundary adapter remains explicit; dispatch remains intentionally flat"
)]
fn run_with_adapters(
    args: impl IntoIterator<Item = OsString>,
    mut stdout: impl Write,
    mut stderr: impl Write,
    inspector: &impl RepositoryPathInspector,
    indexer: &impl RepositoryIndexer,
    searcher: &impl RepositorySearcher,
    symbol_getter: &impl RepositorySymbolGetter,
    memory: &impl RepositoryMemory,
    configuration_loader: &impl ConfigurationLoader,
) -> u8 {
    let mut args = args.into_iter();
    let _program = args.next();
    let Some(command) = args.next() else {
        return emit_error(
            &mut stderr,
            EXIT_USAGE,
            "error: no command supplied; use --help\n",
        );
    };

    if let Some(exit) = run_metadata_command(
        command.as_os_str(),
        &mut args,
        &mut stdout,
        &mut stderr,
    ) {
        return exit;
    }
    if command == OsStr::new("identity") {
        return run_identity(args, &mut stdout, &mut stderr, &OsIdentityGenerator);
    }
    if command == OsStr::new("onboard") {
        return run_onboard(
            args,
            &mut stdout,
            &mut stderr,
            inspector,
            indexer,
            &OsIdentityGenerator,
            &PrivateOnboardStateDirectory,
        );
    }
    if command == OsStr::new("codex") {
        return run_codex(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("config") {
        return run_config(args, &mut stdout, &mut stderr, configuration_loader);
    }
    if command == OsStr::new("doctor") {
        return run_doctor(args, &mut stdout, &mut stderr, configuration_loader);
    }
    if command == OsStr::new("inspect-paths") {
        return run_inspect_paths(args, &mut stdout, &mut stderr, inspector);
    }
    if command == OsStr::new("index") {
        return run_index(
            args,
            &mut stdout,
            &mut stderr,
            indexer,
            configuration_loader,
        );
    }
    if command == OsStr::new("workspace") {
        return run_workspace(args, &mut stdout, &mut stderr, configuration_loader);
    }
    if command == OsStr::new("context-build") {
        return run_context_build(
            args,
            &mut stdout,
            &mut stderr,
            &LocalRepositoryContextBuilder,
            configuration_loader,
        );
    }
    if command == OsStr::new("phase2-context-build") {
        return run_phase2_context_build(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("diagnostics") {
        return run_diagnostics(
            args,
            &mut stdout,
            &mut stderr,
            &LocalRepositoryDiagnosticsReader,
            configuration_loader,
        );
    }
    if command == OsStr::new("architecture-map") {
        return run_architecture_map(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("repository-topology") {
        return run_repository_topology(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("architecture-overview") {
        return run_architecture_overview(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("graph") {
        return run_graph(
            args,
            &mut stdout,
            &mut stderr,
            &LocalRepositoryGraphReader,
            configuration_loader,
        );
    }
    if command == OsStr::new("scip-evidence") {
        return run_scip_evidence(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("scip-relationship-trace") {
        return run_scip_relationship_trace(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("scip-symbol-resolve") {
        return run_scip_symbol_resolve(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("scip-import") {
        return run_scip_import(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("scip-rust-import") {
        return run_scip_rust_import(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("scip-go-import") {
        return run_scip_go_import(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("search") {
        return run_search(
            args,
            &mut stdout,
            &mut stderr,
            searcher,
            configuration_loader,
        );
    }
    if command == OsStr::new("locate-relevant-paths") {
        return run_relevant_paths(args, &mut stdout, &mut stderr, configuration_loader);
    }
    if command == OsStr::new("symbol-search") {
        return run_symbol_search(args, &mut stdout, &mut stderr, configuration_loader);
    }
    if command == OsStr::new("symbol-get") {
        return run_symbol_get(args, &mut stdout, &mut stderr, symbol_getter);
    }
    if command == OsStr::new("outbound-sites") {
        return run_outbound_sites(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("syntax-site-search") {
        return run_syntax_site_search(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("test-markers") {
        return run_test_markers(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("memory-revalidate") {
        return run_memory_revalidate(args, &mut stdout, &mut stderr, memory);
    }
    if command == OsStr::new("memory-recall") {
        return run_memory_recall(
            args,
            &mut stdout,
            &mut stderr,
            memory,
            configuration_loader,
        );
    }
    if command == OsStr::new("memory-manage") {
        return run_memory_manage(args, &mut stdout, &mut stderr, memory);
    }
    if command == OsStr::new("memory-history") {
        return run_memory_history(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("personal-memory") {
        return run_personal_memory(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("task-status") {
        return run_task_status(args, &mut stdout, &mut stderr);
    }
    if command == OsStr::new("task") {
        return run_task(args, &mut stdout, &mut stderr);
    }

    emit_error(
        &mut stderr,
        EXIT_USAGE,
        "error: unknown command; use --help\n",
    )
}

fn run_metadata_command(
    command: &OsStr,
    args: &mut impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> Option<u8> {
    if command == OsStr::new("--help") || command == OsStr::new("-h") {
        return Some(if args.next().is_some() {
            emit_error(
                stderr,
                EXIT_USAGE,
                "error: --help accepts no additional arguments\n",
            )
        } else {
            emit_output(stdout, HELP)
        });
    }
    if command == OsStr::new("--version") || command == OsStr::new("-V") {
        return Some(if args.next().is_some() {
            emit_error(
                stderr,
                EXIT_USAGE,
                "error: --version accepts no additional arguments\n",
            )
        } else {
            emit_version(stdout)
        });
    }
    None
}

fn run_search(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    searcher: &impl RepositorySearcher,
    configuration_loader: &impl ConfigurationLoader,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_SEARCH_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_SEARCH_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: search received too many arguments; use search --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, SEARCH_HELP);
    }
    let (arguments, configuration_invocation) =
        match extract_configuration_arguments(&arguments, &[]) {
            Ok(parsed) => parsed,
            Err(message) => return emit_error(stderr, EXIT_USAGE, message),
        };
    let invocation = match parse_search_arguments(&arguments) {
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
    match searcher.search(&invocation, &configuration) {
        Ok(report) => emit_search_report(stdout, &report),
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: code search failed\n"),
    }
}

fn parse_search_arguments(arguments: &[OsString]) -> Result<SearchInvocation, &'static str> {
    let mut repository_identity = None;
    let mut database = None;
    let mut query = None;
    let mut max_results = 20_u16;
    let mut limit_seen = false;
    let mut index = 0_usize;

    while index < arguments.len() {
        let option = &arguments[index];
        index += 1;
        if option == OsStr::new("--help") || option == OsStr::new("-h") {
            return Err("error: search --help accepts no additional arguments\n");
        }
        let value = arguments
            .get(index)
            .ok_or("error: search option requires a value; use search --help\n")?;
        index += 1;
        if option == OsStr::new("--repository-id") {
            if repository_identity.replace(value.clone()).is_some() {
                return Err("error: search accepts --repository-id only once\n");
            }
        } else if option == OsStr::new("--database") {
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: search accepts --database only once\n");
            }
        } else if option == OsStr::new("--query") {
            if query.replace(value.clone()).is_some() {
                return Err("error: search accepts --query only once\n");
            }
        } else if option == OsStr::new("--limit") {
            if limit_seen {
                return Err("error: search accepts --limit only once\n");
            }
            max_results = value
                .to_str()
                .and_then(|text| text.parse::<u16>().ok())
                .filter(|limit| (1..=100).contains(limit))
                .ok_or("error: search --limit must be an integer from 1 through 100\n")?;
            limit_seen = true;
        } else {
            return Err("error: unknown search option; use search --help\n");
        }
    }

    let repository_identity =
        repository_identity.ok_or("error: search requires --repository-id; use search --help\n")?;
    if repository_identity.is_empty() {
        return Err("error: search repository identity must not be empty\n");
    }
    let database = database.ok_or("error: search requires --database; use search --help\n")?;
    if database.as_os_str().is_empty() {
        return Err("error: search database path must not be empty\n");
    }
    let query = query.ok_or("error: search requires --query; use search --help\n")?;
    if query.is_empty() {
        return Err("error: search query must not be empty\n");
    }
    Ok(SearchInvocation {
        database,
        repository_identity,
        query,
        max_results,
    })
}

fn run_symbol_get(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    getter: &impl RepositorySymbolGetter,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_SYMBOL_GET_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_SYMBOL_GET_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: symbol-get received too many arguments; use symbol-get --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, SYMBOL_GET_HELP);
    }
    let invocation = match parse_symbol_get_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match getter.get(&invocation) {
        Ok(report) => emit_symbol_report(stdout, &report),
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: symbol retrieval failed\n"),
    }
}

#[derive(Default)]
struct SymbolInvocationBuilder {
    root: Option<PathBuf>,
    database: Option<PathBuf>,
    repository_identity: Option<OsString>,
    snapshot: Option<OsString>,
    generation: Option<OsString>,
    path: Option<OsString>,
    content: Option<OsString>,
    artifact: Option<OsString>,
    fact_ordinal: Option<OsString>,
}

impl SymbolInvocationBuilder {
    fn set(&mut self, option: &OsStr, value: &OsStr) -> Result<(), &'static str> {
        if option == OsStr::new("--root") {
            set_once(&mut self.root, PathBuf::from(value), "root")
        } else if option == OsStr::new("--database") {
            set_once(&mut self.database, PathBuf::from(value), "database")
        } else if option == OsStr::new("--repository-id") {
            set_once(
                &mut self.repository_identity,
                value.to_owned(),
                "repository-id",
            )
        } else if option == OsStr::new("--snapshot") {
            set_once(&mut self.snapshot, value.to_owned(), "snapshot")
        } else if option == OsStr::new("--generation") {
            set_once(&mut self.generation, value.to_owned(), "generation")
        } else if option == OsStr::new("--path") {
            set_once(&mut self.path, value.to_owned(), "path")
        } else if option == OsStr::new("--content") {
            set_once(&mut self.content, value.to_owned(), "content")
        } else if option == OsStr::new("--artifact") {
            set_once(&mut self.artifact, value.to_owned(), "artifact")
        } else if option == OsStr::new("--fact") {
            set_once(&mut self.fact_ordinal, value.to_owned(), "fact")
        } else {
            Err("error: unknown symbol-get option; use symbol-get --help\n")
        }
    }

    fn finish(self) -> Result<SymbolInvocation, &'static str> {
        let root = required(self.root, "error: symbol-get requires --root\n")?;
        let database = required(self.database, "error: symbol-get requires --database\n")?;
        let repository_identity = required(
            self.repository_identity,
            "error: symbol-get requires --repository-id\n",
        )?;
        let snapshot = required(self.snapshot, "error: symbol-get requires --snapshot\n")?;
        let generation_text =
            required(self.generation, "error: symbol-get requires --generation\n")?;
        let path = required(self.path, "error: symbol-get requires --path\n")?;
        let content = required(self.content, "error: symbol-get requires --content\n")?;
        let artifact = required(self.artifact, "error: symbol-get requires --artifact\n")?;
        let fact_text = required(self.fact_ordinal, "error: symbol-get requires --fact\n")?;
        validate_symbol_text(&root, &database, &repository_identity, &path)?;
        let generation = parse_positive_i64(&generation_text)?;
        let fact_ordinal = parse_u64(&fact_text)?;
        Ok(SymbolInvocation {
            root,
            database,
            repository_identity,
            snapshot,
            generation,
            path,
            content,
            artifact,
            fact_ordinal,
        })
    }
}

fn parse_symbol_get_arguments(arguments: &[OsString]) -> Result<SymbolInvocation, &'static str> {
    let mut builder = SymbolInvocationBuilder::default();
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        if option == OsStr::new("--help") || option == OsStr::new("-h") {
            return Err("error: symbol-get --help accepts no additional arguments\n");
        }
        let value = arguments
            .get(index + 1)
            .ok_or("error: symbol-get option requires a value; use symbol-get --help\n")?;
        builder.set(option, value)?;
        index += 2;
    }
    builder.finish()
}

fn set_once<T>(slot: &mut Option<T>, value: T, name: &'static str) -> Result<(), &'static str> {
    if slot.replace(value).is_some() {
        match name {
            "root" => Err("error: symbol-get accepts --root only once\n"),
            "database" => Err("error: symbol-get accepts --database only once\n"),
            "repository-id" => Err("error: symbol-get accepts --repository-id only once\n"),
            "snapshot" => Err("error: symbol-get accepts --snapshot only once\n"),
            "generation" => Err("error: symbol-get accepts --generation only once\n"),
            "path" => Err("error: symbol-get accepts --path only once\n"),
            "content" => Err("error: symbol-get accepts --content only once\n"),
            "artifact" => Err("error: symbol-get accepts --artifact only once\n"),
            "fact" => Err("error: symbol-get accepts --fact only once\n"),
            _ => Err("error: duplicate symbol-get option\n"),
        }
    } else {
        Ok(())
    }
}

fn required<T>(value: Option<T>, message: &'static str) -> Result<T, &'static str> {
    value.ok_or(message)
}

fn validate_symbol_text(
    root: &Path,
    database: &Path,
    repository_identity: &OsStr,
    path: &OsStr,
) -> Result<(), &'static str> {
    if root.as_os_str().is_empty()
        || database.as_os_str().is_empty()
        || repository_identity.is_empty()
        || path.is_empty()
    {
        Err("error: symbol-get option values must not be empty\n")
    } else {
        Ok(())
    }
}

fn parse_positive_i64(value: &OsStr) -> Result<i64, &'static str> {
    value
        .to_str()
        .and_then(|text| text.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .ok_or("error: symbol-get --generation must be a positive integer\n")
}

fn parse_u64(value: &OsStr) -> Result<u64, &'static str> {
    value
        .to_str()
        .and_then(|text| text.parse::<u64>().ok())
        .filter(|ordinal| *ordinal <= MAX_MCP_INTEROPERABLE_INTEGER)
        .ok_or("error: symbol-get --fact must be an integer from 0 to 9007199254740991\n")
}

fn run_index(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    indexer: &impl RepositoryIndexer,
    configuration_loader: &impl ConfigurationLoader,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_INDEX_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_INDEX_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: index received too many arguments; use index --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, INDEX_HELP);
    }

    let (arguments, configuration_invocation) =
        match extract_configuration_arguments(&arguments, &[]) {
            Ok(parsed) => parsed,
            Err(message) => return emit_error(stderr, EXIT_USAGE, message),
        };
    let invocation = match parse_index_arguments(&arguments) {
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
    match indexer.index(&invocation, &configuration) {
        Ok(report) => emit_index_report(stdout, report),
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: indexing failed\n"),
    }
}

fn parse_index_arguments(arguments: &[OsString]) -> Result<IndexInvocation, &'static str> {
    let mut repository_identity = None;
    let mut database = None;
    let mut repository_root = None;
    let mut positional_only = false;
    let mut index = 0_usize;

    while index < arguments.len() {
        let argument = &arguments[index];
        if positional_only {
            set_repository_root(&mut repository_root, argument)?;
            index += 1;
            continue;
        }
        if argument == OsStr::new("--") {
            positional_only = true;
            index += 1;
            continue;
        }
        if argument == OsStr::new("--repository-id") {
            index += 1;
            let value = arguments
                .get(index)
                .ok_or("error: index --repository-id requires a value; use index --help\n")?;
            if repository_identity.replace(value.clone()).is_some() {
                return Err("error: index accepts --repository-id only once\n");
            }
            index += 1;
            continue;
        }
        if argument == OsStr::new("--database") {
            index += 1;
            let value = arguments
                .get(index)
                .ok_or("error: index --database requires a path; use index --help\n")?;
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: index accepts --database only once\n");
            }
            index += 1;
            continue;
        }
        if argument == OsStr::new("--help") || argument == OsStr::new("-h") {
            return Err("error: index --help accepts no additional arguments\n");
        }
        if os_string_starts_with_hyphen(argument) {
            return Err("error: unknown index option; use index --help\n");
        }
        set_repository_root(&mut repository_root, argument)?;
        index += 1;
    }

    let repository_identity =
        repository_identity.ok_or("error: index requires --repository-id; use index --help\n")?;
    if repository_identity.is_empty() {
        return Err("error: index repository identity must not be empty\n");
    }
    let database = database.ok_or("error: index requires --database; use index --help\n")?;
    if database.as_os_str().is_empty() {
        return Err("error: index database path must not be empty\n");
    }
    let repository_root =
        repository_root.ok_or("error: index requires one repository; use index --help\n")?;

    Ok(IndexInvocation {
        repository_root,
        database,
        repository_identity,
    })
}

fn set_repository_root(
    repository_root: &mut Option<PathBuf>,
    value: &OsStr,
) -> Result<(), &'static str> {
    if value.is_empty() {
        return Err("error: index repository must not be empty\n");
    }
    if repository_root.replace(PathBuf::from(value)).is_some() {
        return Err("error: index accepts exactly one repository\n");
    }
    Ok(())
}

fn run_inspect_paths(
    mut args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    inspector: &impl RepositoryPathInspector,
) -> u8 {
    let Some(first) = args.next() else {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: inspect-paths requires one repository; use inspect-paths --help\n",
        );
    };
    if first == OsStr::new("--help") || first == OsStr::new("-h") {
        if args.next().is_none() {
            return emit_output(stdout, INSPECT_HELP);
        }
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: inspect-paths --help accepts no additional arguments\n",
        );
    }

    let root = if first == OsStr::new("--") {
        let Some(root) = args.next() else {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: inspect-paths requires a repository after --\n",
            );
        };
        root
    } else {
        if os_string_starts_with_hyphen(&first) {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: unknown inspect-paths option; use -- before a repository beginning with '-'\n",
            );
        }
        first
    };

    if root.is_empty() {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: inspect-paths repository must not be empty\n",
        );
    }
    if args.next().is_some() {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: inspect-paths accepts exactly one repository\n",
        );
    }

    match inspector.inspect(Path::new(&root)) {
        Ok(stats) => emit_inspection_report(stdout, stats),
        Err(_) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: repository path inspection failed\n",
        ),
    }
}

fn os_string_starts_with_hyphen(value: &OsStr) -> bool {
    value.as_encoded_bytes().first() == Some(&b'-')
}
