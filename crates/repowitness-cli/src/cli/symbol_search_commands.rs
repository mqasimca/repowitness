struct SymbolSearchInvocation {
    database: PathBuf,
    workspace: GraphWorkspaceContext,
    name: String,
    name_match: SymbolSearchNameMatch,
    language: Option<SourceLanguage>,
    kind: Option<RustSymbolKind>,
    path_prefix: Option<String>,
    max_results: u16,
}

fn run_symbol_search(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    configuration_loader: &impl ConfigurationLoader,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_SYMBOL_SEARCH_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_SYMBOL_SEARCH_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: symbol-search received too many arguments; use symbol-search --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, SYMBOL_SEARCH_HELP);
    }
    let (arguments, configuration_invocation) = match extract_configuration_arguments(&arguments, &[]) {
        Ok(parsed) => parsed,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let invocation = match parse_symbol_search_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let configuration = match configuration_loader.load(&configuration_invocation) {
        Ok(configuration) => configuration,
        Err(_) => return emit_error(stderr, EXIT_SOFTWARE, "error: configuration resolution failed\n"),
    };
    let request = match &invocation.workspace {
        GraphWorkspaceContext::SingleRepository(repository_identity) => LocalSymbolSearchRequest::new(
            &invocation.database,
            repository_identity,
            &invocation.name,
            invocation.name_match,
        ),
        GraphWorkspaceContext::ConnectedWorkspace {
            connected_workspace,
            source_slot,
        } => LocalSymbolSearchRequest::for_connected_workspace(
            &invocation.database,
            connected_workspace,
            source_slot,
            &invocation.name,
            invocation.name_match,
        ),
    }
    .with_filters(
        invocation.language,
        invocation.kind,
        invocation.path_prefix.as_deref(),
    )
    .with_max_results(invocation.max_results);
    let request = match request {
        Ok(request) => request,
        Err(_) => return emit_error(stderr, EXIT_USAGE, "error: symbol-search request is invalid\n"),
    };
    let request = request.with_configuration(&configuration);
    let output = search_local_symbols(request, Arc::new(AtomicBool::new(false)))
        .map_err(|_| ())
        .and_then(|result| mcp_symbol_search_output(result).map_err(|_| ()));
    match output {
        Ok(output) => emit_symbol_search_output(stdout, &output),
        Err(()) => emit_error(stderr, EXIT_SOFTWARE, "error: symbol search failed\n"),
    }
}

fn parse_symbol_search_arguments(
    arguments: &[OsString],
) -> Result<SymbolSearchInvocation, &'static str> {
    let mut parsed = ParsedSymbolSearchArguments::default();
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        index += 1;
        let value = arguments.get(index).ok_or(
            "error: symbol-search options require a value; use symbol-search --help\n",
        )?;
        index += 1;
        parsed.accept(option, value)?;
    }
    parsed.finish()
}

#[derive(Default)]
struct ParsedSymbolSearchArguments {
    database: Option<PathBuf>,
    workspace_arguments: GraphWorkspaceArguments,
    name: Option<OsString>,
    name_match: Option<SymbolSearchNameMatch>,
    language: Option<SourceLanguage>,
    kind: Option<RustSymbolKind>,
    path_prefix: Option<OsString>,
    max_results: Option<u16>,
}

impl ParsedSymbolSearchArguments {
    fn accept(&mut self, option: &OsStr, value: &OsStr) -> Result<(), &'static str> {
        if self.workspace_arguments.accept_option(option, value)? {
            Ok(())
        } else if option == OsStr::new("--database") {
            replace_once(
                &mut self.database,
                PathBuf::from(value),
                "error: symbol-search --database was supplied more than once\n",
            )
        } else if option == OsStr::new("--name") {
            replace_once(
                &mut self.name,
                value.to_owned(),
                "error: symbol-search --name was supplied more than once\n",
            )
        } else if option == OsStr::new("--match") {
            let name_match = match value.to_str() {
                Some("exact") => SymbolSearchNameMatch::Exact,
                Some("prefix") => SymbolSearchNameMatch::Prefix,
                _ => return Err("error: symbol-search --match must be exact or prefix\n"),
            };
            replace_once(
                &mut self.name_match,
                name_match,
                "error: symbol-search --match was supplied more than once\n",
            )
        } else if option == OsStr::new("--language") {
            let language = value
                .to_str()
                .and_then(SourceLanguage::from_stable_str)
                .ok_or("error: symbol-search --language must be rust, go, typescript, tsx, or python\n")?;
            replace_once(
                &mut self.language,
                language,
                "error: symbol-search --language was supplied more than once\n",
            )
        } else if option == OsStr::new("--kind") {
            let kind = value
                .to_str()
                .and_then(RustSymbolKind::from_stable_str)
                .ok_or("error: symbol-search --kind is not supported\n")?;
            replace_once(
                &mut self.kind,
                kind,
                "error: symbol-search --kind was supplied more than once\n",
            )
        } else if option == OsStr::new("--path-prefix") {
            replace_once(
                &mut self.path_prefix,
                value.to_owned(),
                "error: symbol-search --path-prefix was supplied more than once\n",
            )
        } else if option == OsStr::new("--limit") {
            let limit = value
                .to_str()
                .and_then(|text| text.parse::<u16>().ok())
                .filter(|limit| (1..=100).contains(limit))
                .ok_or("error: symbol-search --limit must be an integer from 1 through 100\n")?;
            replace_once(
                &mut self.max_results,
                limit,
                "error: symbol-search --limit was supplied more than once\n",
            )
        } else {
            Err("error: unknown symbol-search option; use symbol-search --help\n")
        }
    }

    fn finish(self) -> Result<SymbolSearchInvocation, &'static str> {
        let name = self
            .name
            .ok_or("error: symbol-search requires --name\n")?
            .into_string()
            .map_err(|_| "error: symbol-search name must be valid UTF-8\n")?;
        let path_prefix = self
            .path_prefix
            .map(OsString::into_string)
            .transpose()
            .map_err(|_| "error: symbol-search path prefix must be valid UTF-8\n")?;
        Ok(SymbolSearchInvocation {
            database: self
                .database
                .ok_or("error: symbol-search requires --database\n")?,
            workspace: self.workspace_arguments.into_context()?,
            name,
            name_match: self.name_match.unwrap_or(SymbolSearchNameMatch::Exact),
            language: self.language,
            kind: self.kind,
            path_prefix,
            max_results: self.max_results.unwrap_or(20),
        })
    }
}

fn replace_once<T>(
    destination: &mut Option<T>,
    value: T,
    duplicate_error: &'static str,
) -> Result<(), &'static str> {
    if destination.replace(value).is_some() {
        Err(duplicate_error)
    } else {
        Ok(())
    }
}

fn emit_symbol_search_output(writer: &mut impl Write, output: &SymbolSearchOutput) -> u8 {
    let Ok(mut encoded) = serde_json::to_vec(output) else {
        return EXIT_SOFTWARE;
    };
    if encoded.len() > MAX_CLI_SEARCH_OUTPUT_BYTES {
        return EXIT_SOFTWARE;
    }
    encoded.push(b'\n');
    if writer.write_all(&encoded).is_ok() { EXIT_SUCCESS } else { EXIT_IO }
}

#[cfg(test)]
mod symbol_search_command_tests {
    use std::ffi::OsString;

    use super::{
        CONFIGURATION_LAYER_ARGUMENTS, GraphWorkspaceContext, MAX_SYMBOL_SEARCH_ARGUMENTS,
        SymbolSearchNameMatch, parse_symbol_search_arguments,
    };

    #[test]
    fn parser_admits_only_one_bounded_typed_selector() {
        let identity = format!("rwi1:h:{}", "01".repeat(32));
        let parsed = parse_symbol_search_arguments(&[
            OsString::from("--repository-id"), OsString::from(&identity),
            OsString::from("--database"), OsString::from("index.sqlite3"),
            OsString::from("--name"), OsString::from("run"),
            OsString::from("--match"), OsString::from("prefix"),
            OsString::from("--language"), OsString::from("python"),
            OsString::from("--kind"), OsString::from("function"),
            OsString::from("--path-prefix"), OsString::from("tools"),
            OsString::from("--limit"), OsString::from("100"),
        ]).expect("valid bounded selector");
        assert_eq!(parsed.name_match, SymbolSearchNameMatch::Prefix);
        assert_eq!(parsed.max_results, 100);
        assert_eq!(MAX_SYMBOL_SEARCH_ARGUMENTS, 18 + CONFIGURATION_LAYER_ARGUMENTS);
        assert!(parse_symbol_search_arguments(&[
            OsString::from("--repository-id"), OsString::from(identity),
            OsString::from("--database"), OsString::from("index.sqlite3"),
            OsString::from("--name"), OsString::from("run"),
            OsString::from("--match"), OsString::from("regex"),
        ]).is_err());
    }

    #[test]
    fn parser_pins_one_connected_workspace_source_slot() {
        let connected_workspace = format!("cwi1:h:{}", "02".repeat(32));
        let source_slot = format!("ssi1:h:{}", "03".repeat(32));
        let parsed = parse_symbol_search_arguments(&[
            OsString::from("--connected-workspace-id"),
            OsString::from(&connected_workspace),
            OsString::from("--source-slot-id"),
            OsString::from(&source_slot),
            OsString::from("--database"),
            OsString::from("index.sqlite3"),
            OsString::from("--name"),
            OsString::from("run"),
        ])
        .expect("connected source selector should parse");
        assert!(matches!(
            parsed.workspace,
            GraphWorkspaceContext::ConnectedWorkspace {
                connected_workspace: actual_workspace,
                source_slot: actual_slot,
            } if actual_workspace == connected_workspace && actual_slot == source_slot
        ));
    }
}
