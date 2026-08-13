const DEFAULT_SCIP_GO_PRODUCER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
const MAX_SCIP_GO_PRODUCER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
const DEFAULT_SCIP_GO_IMPORT_TIMEOUT: std::time::Duration =
    repowitness_local::DEFAULT_LOCAL_SCIP_IMPORT_DEADLINE;
const MAX_SCIP_GO_IMPORT_TIMEOUT: std::time::Duration =
    repowitness_local::MAX_LOCAL_SCIP_IMPORT_DEADLINE;
const MAX_SCIP_GO_MOD_BYTES: usize = 1024 * 1024;

struct ScipGoImportInvocation {
    import: ScipImportInvocation,
    scip_go: PathBuf,
    producer_timeout: std::time::Duration,
}
fn run_scip_go_import(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_SCIP_GO_IMPORT_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_SCIP_GO_IMPORT_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: scip-go-import received too many arguments; use scip-go-import --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, SCIP_GO_IMPORT_HELP);
    }
    let invocation = match parse_scip_go_import_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    if !scip_go_root_has_regular_go_mod(&invocation.import.root) {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: scip-go-import requires a regular go.mod at --root\n",
        );
    }
    let temporary_output = match TemporaryScipOutput::new() {
        Ok(output) => output,
        Err(()) => {
            return emit_error(
                stderr,
                EXIT_SOFTWARE,
                "error: SCIP producer temporary output could not be prepared\n",
            );
        }
    };
    let mut producer = std::process::Command::new(&invocation.scip_go);
    producer
        .arg("index")
        .arg("--output")
        .arg(temporary_output.path())
        .arg("--skip-implementations")
        .arg("--skip-tests")
        .arg("--quiet")
        .current_dir(&invocation.import.root)
        .env("GOENV", "off")
        .env("GOPACKAGESDRIVER", "off")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .env("GOTOOLCHAIN", "local")
        .env("GOWORK", "off")
        .env("GOFLAGS", "-mod=readonly")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if run_scip_producer(producer, invocation.producer_timeout).is_err() {
        return emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: scip-go SCIP production failed\n",
        );
    }
    let import = ScipImportInvocation {
        scip_file: temporary_output.path().to_owned(),
        ..invocation.import
    };
    match import_scip_overlay(&import) {
        Ok(result) => emit_scip_import_output(stdout, result),
        Err(ScipImportOverlayError::InvalidWorkspaceView) => {
            emit_error(stderr, EXIT_USAGE, "error: SCIP import workspace view is invalid\n")
        }
        Err(ScipImportOverlayError::Import(error)) => emit_scip_import_failure(stderr, &error),
    }
}

fn scip_go_root_has_regular_go_mod(root: &Path) -> bool {
    read_bounded_regular_file(&root.join("go.mod"), MAX_SCIP_GO_MOD_BYTES).is_ok()
}

fn parse_scip_go_import_arguments(
    arguments: &[OsString],
) -> Result<ScipGoImportInvocation, &'static str> {
    let mut database = None;
    let mut root = None;
    let mut repository_identity = None;
    let mut connected_workspace = None;
    let mut source_slot = None;
    let mut workspace_view = None;
    let mut scip_go = None;
    let mut producer_timeout = None;
    let mut import_timeout = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        let value = arguments
            .get(index + 1)
            .ok_or("error: scip-go-import option requires a value; use scip-go-import --help\n")?;
        index += 2;
        if option == OsStr::new("--database") {
            if value.is_empty() || database.replace(PathBuf::from(value)).is_some() {
                return Err("error: scip-go-import accepts one non-empty --database\n");
            }
            continue;
        }
        if option == OsStr::new("--root") {
            if value.is_empty() || root.replace(PathBuf::from(value)).is_some() {
                return Err("error: scip-go-import accepts one non-empty --root\n");
            }
            continue;
        }
        if accept_scip_go_import_workspace_selector(
            option,
            value,
            &mut repository_identity,
            &mut connected_workspace,
            &mut source_slot,
        )? {
            continue;
        }
        if option == OsStr::new("--workspace-view") {
            let value = i64::try_from(parse_graph_u64(value)?)
                .map_err(|_| "error: scip-go-import workspace view is too large\n")?;
            if value <= 0 || workspace_view.replace(value).is_some() {
                return Err("error: scip-go-import accepts one positive --workspace-view\n");
            }
            continue;
        }
        if option == OsStr::new("--scip-go") {
            if value.is_empty() || scip_go.replace(PathBuf::from(value)).is_some() {
                return Err("error: scip-go-import accepts one non-empty --scip-go\n");
            }
            continue;
        }
        if option == OsStr::new("--producer-timeout-ms") {
            let value = parse_scip_go_duration(value, MAX_SCIP_GO_PRODUCER_TIMEOUT)?;
            if producer_timeout.replace(value).is_some() {
                return Err("error: scip-go-import accepts --producer-timeout-ms only once\n");
            }
            continue;
        }
        if option == OsStr::new("--import-timeout-ms") {
            let value = parse_scip_go_duration(value, MAX_SCIP_GO_IMPORT_TIMEOUT)?;
            if import_timeout.replace(value).is_some() {
                return Err("error: scip-go-import accepts --import-timeout-ms only once\n");
            }
            continue;
        }
        return Err("error: unsupported scip-go-import option; use scip-go-import --help\n");
    }
    let (connected_workspace, source_slot) = resolve_scip_go_import_workspace(
        repository_identity,
        connected_workspace,
        source_slot,
    )?;
    Ok(ScipGoImportInvocation {
        import: ScipImportInvocation {
            database: database.ok_or("error: scip-go-import requires --database\n")?,
            root: root.ok_or("error: scip-go-import requires --root\n")?,
            scip_file: PathBuf::new(),
            connected_workspace,
            source_slot,
            workspace_view,
            timeout: import_timeout.unwrap_or(DEFAULT_SCIP_GO_IMPORT_TIMEOUT),
        },
        scip_go: scip_go.unwrap_or_else(|| PathBuf::from("scip-go")),
        producer_timeout: producer_timeout.unwrap_or(DEFAULT_SCIP_GO_PRODUCER_TIMEOUT),
    })
}

fn accept_scip_go_import_workspace_selector(
    option: &OsStr,
    value: &OsStr,
    repository_identity: &mut Option<String>,
    connected_workspace: &mut Option<String>,
    source_slot: &mut Option<String>,
) -> Result<bool, &'static str> {
    if option == OsStr::new("--repository-id") {
        let value = value
            .to_str()
            .filter(|value| !value.is_empty())
            .ok_or("error: scip-go-import repository identity must be non-empty Unicode\n")?;
        RepositoryIdentityTextV1::decode(value).map_err(|_| {
            "error: scip-go-import repository identity must be canonical rwi1:h: text\n"
        })?;
        if repository_identity.replace(value.to_owned()).is_some() {
            return Err("error: scip-go-import accepts --repository-id only once\n");
        }
        return Ok(true);
    }
    if option == OsStr::new("--connected-workspace-id") {
        let value = value
            .to_str()
            .ok_or("error: scip-go-import workspace identity must be Unicode\n")?;
        ConnectedWorkspaceIdTextV1::decode(value).map_err(|_| {
            "error: scip-go-import workspace identity must be canonical cwi1:h: text\n"
        })?;
        if connected_workspace.replace(value.to_owned()).is_some() {
            return Err("error: scip-go-import accepts --connected-workspace-id only once\n");
        }
        return Ok(true);
    }
    if option == OsStr::new("--source-slot-id") {
        let value = value
            .to_str()
            .ok_or("error: scip-go-import source slot identity must be Unicode\n")?;
        SourceSlotIdTextV1::decode(value).map_err(|_| {
            "error: scip-go-import source slot identity must be canonical ssi1:h: text\n"
        })?;
        if source_slot.replace(value.to_owned()).is_some() {
            return Err("error: scip-go-import accepts --source-slot-id only once\n");
        }
        return Ok(true);
    }
    Ok(false)
}

fn resolve_scip_go_import_workspace(
    repository_identity: Option<String>,
    connected_workspace: Option<String>,
    source_slot: Option<String>,
) -> Result<(String, String), &'static str> {
    match (repository_identity, connected_workspace, source_slot) {
        (Some(repository_identity), None, None) => {
            let repository = RepositoryIdentityTextV1::decode(&repository_identity).map_err(|_| {
                "error: scip-go-import repository identity must be canonical rwi1:h: text\n"
            })?;
            Ok((
                ConnectedWorkspaceIdTextV1::encode(
                    repowitness_local::ConnectedWorkspaceId::for_single_repository(repository),
                )
                .into_string(),
                SourceSlotIdTextV1::encode(repowitness_local::SourceSlotId::for_repository(
                    repository,
                ))
                .into_string(),
            ))
        }
        (None, Some(connected_workspace), Some(source_slot)) => Ok((connected_workspace, source_slot)),
        (Some(_), _, _) => Err(
            "error: scip-go-import --repository-id cannot be combined with connected workspace selectors\n",
        ),
        (None, None, None) => Err(
            "error: scip-go-import requires --repository-id or connected workspace selectors\n",
        ),
        (None, None, Some(_)) => {
            Err("error: scip-go-import --source-slot-id requires --connected-workspace-id\n")
        }
        (None, Some(_), None) => {
            Err("error: scip-go-import --connected-workspace-id requires --source-slot-id\n")
        }
    }
}

fn parse_scip_go_duration(
    value: &OsStr,
    maximum: std::time::Duration,
) -> Result<std::time::Duration, &'static str> {
    let milliseconds = parse_graph_u64(value)?;
    let maximum = u64::try_from(maximum.as_millis())
        .map_err(|_| "error: scip-go-import timeout bound is unavailable\n")?;
    if !(1..=maximum).contains(&milliseconds) {
        return Err("error: scip-go-import timeout exceeds its resource bound\n");
    }
    Ok(std::time::Duration::from_millis(milliseconds))
}

#[cfg(test)]
mod scip_go_import_tests {
    use super::*;

    const REPOSITORY_ID: &str = concat!(
        "rwi1:h:",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    );

    #[test]
    fn parser_derives_a_single_repository_workspace_and_default_deadlines() {
        let arguments = [
            OsString::from("--database"),
            OsString::from("database.sqlite3"),
            OsString::from("--root"),
            OsString::from("repository"),
            OsString::from("--repository-id"),
            OsString::from(REPOSITORY_ID),
        ];

        let invocation =
            parse_scip_go_import_arguments(&arguments).expect("single repository selector");

        assert_eq!(
            invocation.import.connected_workspace,
            format!("cwi1:h:{}", "AA".repeat(32))
        );
        assert_eq!(
            invocation.import.source_slot,
            format!("ssi1:h:{}", "AA".repeat(32))
        );
        assert_eq!(
            invocation.import.timeout,
            repowitness_local::DEFAULT_LOCAL_SCIP_IMPORT_DEADLINE
        );
        assert_eq!(
            invocation.producer_timeout,
            std::time::Duration::from_secs(120)
        );
    }
}
