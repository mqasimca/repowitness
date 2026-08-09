struct ContextInvocation {
    root: PathBuf,
    database: PathBuf,
    repository_identity: OsString,
    intent: OsString,
    budget_units: u64,
    max_provider_results: u16,
}

fn run_context_build(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    configuration_loader: &impl ConfigurationLoader,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_CONTEXT_BUILD_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_CONTEXT_BUILD_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: context-build received too many arguments; use context-build --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, CONTEXT_BUILD_HELP);
    }
    let (arguments, configuration_invocation) =
        match extract_configuration_arguments(&arguments, &[]) {
            Ok(parsed) => parsed,
            Err(message) => return emit_error(stderr, EXIT_USAGE, message),
        };
    let invocation = match parse_context_build_arguments(&arguments) {
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
    run_evidence_balanced_context_build(invocation, &configuration, stdout, stderr)
}

#[derive(Default)]
struct ContextInvocationBuilder {
    root: Option<PathBuf>,
    database: Option<PathBuf>,
    repository_identity: Option<OsString>,
    intent: Option<OsString>,
    budget_units: Option<u64>,
    max_provider_results: Option<u16>,
}

impl ContextInvocationBuilder {
    fn set(&mut self, option: &OsStr, value: &OsStr) -> Result<(), &'static str> {
        if option == OsStr::new("--root") {
            context_set_once(&mut self.root, PathBuf::from(value), "root")
        } else if option == OsStr::new("--database") {
            context_set_once(&mut self.database, PathBuf::from(value), "database")
        } else if option == OsStr::new("--repository-id") {
            context_set_once(
                &mut self.repository_identity,
                value.to_owned(),
                "repository-id",
            )
        } else if option == OsStr::new("--intent") {
            context_set_once(&mut self.intent, value.to_owned(), "intent")
        } else if option == OsStr::new("--budget") {
            let budget = value
                .to_str()
                .and_then(|text| text.parse::<u64>().ok())
                .filter(|value| (1..=MAX_EVIDENCE_CONTEXT_BUDGET_UNITS).contains(value))
                .ok_or(
                    "error: context-build --budget must be an integer from 1 through 1048576\n",
                )?;
            context_set_once(&mut self.budget_units, budget, "budget")
        } else if option == OsStr::new("--limit") {
            let limit = value
                .to_str()
                .and_then(|text| text.parse::<u16>().ok())
                .filter(|value| (1..=100).contains(value))
                .ok_or("error: context-build --limit must be an integer from 1 through 100\n")?;
            context_set_once(&mut self.max_provider_results, limit, "limit")
        } else {
            Err("error: unknown context-build option; use context-build --help\n")
        }
    }

    fn finish(self) -> Result<ContextInvocation, &'static str> {
        let root = self.root.ok_or("error: context-build requires --root\n")?;
        let database = self
            .database
            .ok_or("error: context-build requires --database\n")?;
        let repository_identity = self
            .repository_identity
            .ok_or("error: context-build requires --repository-id\n")?;
        let intent = self
            .intent
            .ok_or("error: context-build requires --intent\n")?;
        if root.as_os_str().is_empty()
            || database.as_os_str().is_empty()
            || repository_identity.is_empty()
            || intent.is_empty()
        {
            return Err("error: context-build option values must not be empty\n");
        }
        Ok(ContextInvocation {
            root,
            database,
            repository_identity,
            intent,
            budget_units: self
                .budget_units
                .unwrap_or(DEFAULT_EVIDENCE_CONTEXT_BUDGET_UNITS),
            max_provider_results: self
                .max_provider_results
                .unwrap_or(DEFAULT_LOCAL_EVIDENCE_CONTEXT_PROVIDER_RESULTS),
        })
    }
}

fn parse_context_build_arguments(
    arguments: &[OsString],
) -> Result<ContextBuildInvocation, &'static str> {
    let mut context_arguments = Vec::with_capacity(arguments.len());
    let mut connected_workspace = None;
    let mut source_slot = None;
    let mut scip_symbol = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        if option == OsStr::new("--help") || option == OsStr::new("-h") {
            return Err("error: context-build --help accepts no additional arguments\n");
        }
        let value = arguments
            .get(index + 1)
            .ok_or("error: context-build option requires a value; use context-build --help\n")?;
        if option == OsStr::new("--connected-workspace-id") {
            let value = value
                .to_str()
                .filter(|value| !value.is_empty())
                .ok_or("error: context-build --connected-workspace-id must be valid UTF-8 and non-empty\n")?;
            context_set_once(&mut connected_workspace, value.to_owned(), "connected-workspace-id")?;
        } else if option == OsStr::new("--source-slot-id") {
            let value = value
                .to_str()
                .filter(|value| !value.is_empty())
                .ok_or("error: context-build --source-slot-id must be valid UTF-8 and non-empty\n")?;
            context_set_once(&mut source_slot, value.to_owned(), "source-slot-id")?;
        } else if option == OsStr::new("--scip-symbol") {
            let value = value
                .to_str()
                .filter(|value| !value.is_empty())
                .ok_or("error: context-build --scip-symbol must be valid UTF-8 and non-empty\n")?;
            context_set_once(&mut scip_symbol, value.to_owned(), "scip-symbol")?;
        } else {
            context_arguments.push(option.clone());
            context_arguments.push(value.clone());
        }
        index += 2;
    }
    let mut builder = ContextInvocationBuilder::default();
    let mut index = 0_usize;
    while index < context_arguments.len() {
        let option = &context_arguments[index];
        let value = context_arguments
            .get(index + 1)
            .ok_or("error: context-build option requires a value; use context-build --help\n")?;
        builder.set(option, value)?;
        index += 2;
    }
    let workspace = match (connected_workspace, source_slot) {
        (None, None) => None,
        (Some(connected_workspace), Some(source_slot)) => Some((connected_workspace, source_slot)),
        (None, Some(_)) | (Some(_), None) => {
            return Err(
                "error: context-build requires --connected-workspace-id and --source-slot-id together\n",
            );
        }
    };
    builder.finish().map(|invocation| ContextBuildInvocation {
        invocation,
        workspace,
        scip_symbol,
    })
}

struct ContextBuildInvocation {
    invocation: ContextInvocation,
    workspace: Option<(String, String)>,
    scip_symbol: Option<String>,
}

fn context_set_once<T>(
    slot: &mut Option<T>,
    value: T,
    name: &'static str,
) -> Result<(), &'static str> {
    if slot.replace(value).is_none() {
        return Ok(());
    }
    match name {
        "root" => Err("error: context-build accepts --root only once\n"),
        "database" => Err("error: context-build accepts --database only once\n"),
        "repository-id" => Err("error: context-build accepts --repository-id only once\n"),
        "intent" => Err("error: context-build accepts --intent only once\n"),
        "budget" => Err("error: context-build accepts --budget only once\n"),
        "limit" => Err("error: context-build accepts --limit only once\n"),
        "connected-workspace-id" => {
            Err("error: context-build accepts --connected-workspace-id only once\n")
        }
        "source-slot-id" => Err("error: context-build accepts --source-slot-id only once\n"),
        "scip-symbol" => Err("error: context-build accepts --scip-symbol only once\n"),
        _ => Err("error: duplicate context-build option\n"),
    }
}
