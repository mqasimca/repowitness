const DEFAULT_SCIP_RUST_PRODUCER_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(120);
const MAX_SCIP_RUST_PRODUCER_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(300);
const DEFAULT_SCIP_RUST_IMPORT_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(30);
const PRODUCER_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(20);
const TEMPORARY_DIRECTORY_ATTEMPTS: u8 = 16;

struct ScipRustImportInvocation {
    import: ScipImportInvocation,
    rust_analyzer: PathBuf,
    producer_timeout: std::time::Duration,
}

fn run_scip_rust_import(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args
        .take(MAX_SCIP_RUST_IMPORT_ARGUMENTS + 1)
        .collect();
    if arguments.len() > MAX_SCIP_RUST_IMPORT_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: scip-rust-import received too many arguments; use scip-rust-import --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, SCIP_RUST_IMPORT_HELP);
    }
    let invocation = match parse_scip_rust_import_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
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
    if run_rust_analyzer_scip(
        &invocation.rust_analyzer,
        &invocation.import.root,
        temporary_output.path(),
        invocation.producer_timeout,
    )
    .is_err()
    {
        return emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: rust-analyzer SCIP production failed\n",
        );
    }
    let import = ScipImportInvocation {
        scip_file: temporary_output.path().to_owned(),
        ..invocation.import
    };
    match import_scip_overlay(&import) {
        Ok(result) => emit_scip_import_output(stdout, result),
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: SCIP import failed\n"),
    }
}

fn parse_scip_rust_import_arguments(
    arguments: &[OsString],
) -> Result<ScipRustImportInvocation, &'static str> {
    let mut database = None;
    let mut root = None;
    let mut repository_identity = None;
    let mut connected_workspace = None;
    let mut source_slot = None;
    let mut workspace_view = None;
    let mut rust_analyzer = None;
    let mut producer_timeout = None;
    let mut import_timeout = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        let value = arguments.get(index + 1).ok_or(
            "error: scip-rust-import option requires a value; use scip-rust-import --help\n",
        )?;
        index += 2;
        if option == OsStr::new("--database") {
            if value.is_empty() || database.replace(PathBuf::from(value)).is_some() {
                return Err("error: scip-rust-import accepts one non-empty --database\n");
            }
            continue;
        }
        if option == OsStr::new("--root") {
            if value.is_empty() || root.replace(PathBuf::from(value)).is_some() {
                return Err("error: scip-rust-import accepts one non-empty --root\n");
            }
            continue;
        }
        if accept_scip_rust_import_workspace_selector(
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
                .map_err(|_| "error: scip-rust-import workspace view is too large\n")?;
            if value <= 0 || workspace_view.replace(value).is_some() {
                return Err("error: scip-rust-import accepts one positive --workspace-view\n");
            }
            continue;
        }
        if option == OsStr::new("--rust-analyzer") {
            if value.is_empty() || rust_analyzer.replace(PathBuf::from(value)).is_some() {
                return Err("error: scip-rust-import accepts one non-empty --rust-analyzer\n");
            }
            continue;
        }
        if option == OsStr::new("--producer-timeout-ms") {
            let value = parse_scip_rust_duration(value, MAX_SCIP_RUST_PRODUCER_TIMEOUT)?;
            if producer_timeout.replace(value).is_some() {
                return Err("error: scip-rust-import accepts --producer-timeout-ms only once\n");
            }
            continue;
        }
        if option == OsStr::new("--import-timeout-ms") {
            let value = parse_scip_rust_duration(value, DEFAULT_SCIP_RUST_IMPORT_TIMEOUT)?;
            if import_timeout.replace(value).is_some() {
                return Err("error: scip-rust-import accepts --import-timeout-ms only once\n");
            }
            continue;
        }
        return Err("error: unsupported scip-rust-import option; use scip-rust-import --help\n");
    }
    let (connected_workspace, source_slot) = resolve_scip_rust_import_workspace(
        repository_identity,
        connected_workspace,
        source_slot,
    )?;
    Ok(ScipRustImportInvocation {
        import: ScipImportInvocation {
            database: database.ok_or("error: scip-rust-import requires --database\n")?,
            root: root.ok_or("error: scip-rust-import requires --root\n")?,
            scip_file: PathBuf::new(),
            connected_workspace,
            source_slot,
            workspace_view,
            timeout: import_timeout.unwrap_or(DEFAULT_SCIP_RUST_IMPORT_TIMEOUT),
        },
        rust_analyzer: rust_analyzer.unwrap_or_else(|| PathBuf::from("rust-analyzer")),
        producer_timeout: producer_timeout.unwrap_or(DEFAULT_SCIP_RUST_PRODUCER_TIMEOUT),
    })
}

fn accept_scip_rust_import_workspace_selector(
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
            .ok_or("error: scip-rust-import repository identity must be non-empty Unicode\n")?;
        RepositoryIdentityTextV1::decode(value).map_err(|_| {
            "error: scip-rust-import repository identity must be canonical rwi1:h: text\n"
        })?;
        if repository_identity.replace(value.to_owned()).is_some() {
            return Err("error: scip-rust-import accepts --repository-id only once\n");
        }
        return Ok(true);
    }
    if option == OsStr::new("--connected-workspace-id") {
        let value = value
            .to_str()
            .ok_or("error: scip-rust-import workspace identity must be Unicode\n")?;
        ConnectedWorkspaceIdTextV1::decode(value).map_err(|_| {
            "error: scip-rust-import workspace identity must be canonical cwi1:h: text\n"
        })?;
        if connected_workspace.replace(value.to_owned()).is_some() {
            return Err("error: scip-rust-import accepts --connected-workspace-id only once\n");
        }
        return Ok(true);
    }
    if option == OsStr::new("--source-slot-id") {
        let value = value
            .to_str()
            .ok_or("error: scip-rust-import source slot identity must be Unicode\n")?;
        SourceSlotIdTextV1::decode(value).map_err(|_| {
            "error: scip-rust-import source slot identity must be canonical ssi1:h: text\n"
        })?;
        if source_slot.replace(value.to_owned()).is_some() {
            return Err("error: scip-rust-import accepts --source-slot-id only once\n");
        }
        return Ok(true);
    }
    Ok(false)
}

fn resolve_scip_rust_import_workspace(
    repository_identity: Option<String>,
    connected_workspace: Option<String>,
    source_slot: Option<String>,
) -> Result<(String, String), &'static str> {
    match (repository_identity, connected_workspace, source_slot) {
        (Some(repository_identity), None, None) => {
            let repository = RepositoryIdentityTextV1::decode(&repository_identity).map_err(|_| {
                "error: scip-rust-import repository identity must be canonical rwi1:h: text\n"
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
            "error: scip-rust-import --repository-id cannot be combined with connected workspace selectors\n",
        ),
        (None, None, None) => Err(
            "error: scip-rust-import requires --repository-id or connected workspace selectors\n",
        ),
        (None, None, Some(_)) => {
            Err("error: scip-rust-import --source-slot-id requires --connected-workspace-id\n")
        }
        (None, Some(_), None) => {
            Err("error: scip-rust-import --connected-workspace-id requires --source-slot-id\n")
        }
    }
}

fn parse_scip_rust_duration(
    value: &OsStr,
    maximum: std::time::Duration,
) -> Result<std::time::Duration, &'static str> {
    let milliseconds = parse_graph_u64(value)?;
    let maximum = u64::try_from(maximum.as_millis())
        .map_err(|_| "error: scip-rust-import timeout bound is unavailable\n")?;
    if !(1..=maximum).contains(&milliseconds) {
        return Err("error: scip-rust-import timeout exceeds its resource bound\n");
    }
    Ok(std::time::Duration::from_millis(milliseconds))
}

fn run_rust_analyzer_scip(
    rust_analyzer: &Path,
    root: &Path,
    output: &Path,
    timeout: std::time::Duration,
) -> Result<(), ()> {
    let mut child = std::process::Command::new(rust_analyzer)
        .arg("scip")
        .arg(".")
        .arg("--output")
        .arg(output)
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| ())?;
    let deadline = std::time::Instant::now().checked_add(timeout).ok_or(())?;
    loop {
        if let Some(status) = child.try_wait().map_err(|_| ())? {
            return if status.success() { Ok(()) } else { Err(()) };
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(());
        }
        std::thread::sleep(PRODUCER_POLL_INTERVAL);
    }
}

struct TemporaryScipOutput {
    directory: PathBuf,
    output: PathBuf,
}

impl TemporaryScipOutput {
    fn new() -> Result<Self, ()> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        let parent = std::fs::canonicalize(std::env::temp_dir()).map_err(|_| ())?;
        for _ in 0..TEMPORARY_DIRECTORY_ATTEMPTS {
            let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| ())?
                .as_nanos();
            let directory = parent.join(format!(
                "repowitness-scip-{}-{nonce}-{sequence}",
                std::process::id()
            ));
            if create_private_directory(&directory).is_ok() {
                return Ok(Self {
                    output: directory.join("index.scip"),
                    directory,
                });
            }
        }
        Err(())
    }

    fn path(&self) -> &Path {
        &self.output
    }
}

impl Drop for TemporaryScipOutput {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.output);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;

    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700);
    builder.create(path)
}

#[cfg(not(unix))]
fn create_private_directory(path: &Path) -> std::io::Result<()> {
    std::fs::DirBuilder::new().create(path)
}

#[cfg(test)]
mod scip_rust_import_tests {
    use super::*;

    const REPOSITORY_ID: &str = concat!(
        "rwi1:h:",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    );

    #[test]
    fn parser_derives_the_compatible_single_repository_workspace() {
        let arguments = [
            OsString::from("--database"),
            OsString::from("database.sqlite3"),
            OsString::from("--root"),
            OsString::from("repository"),
            OsString::from("--repository-id"),
            OsString::from(REPOSITORY_ID),
        ];

        let invocation =
            parse_scip_rust_import_arguments(&arguments).expect("single repository selector");

        assert_eq!(
            invocation.import.connected_workspace,
            format!("cwi1:h:{}", "AA".repeat(32))
        );
        assert_eq!(
            invocation.import.source_slot,
            format!("ssi1:h:{}", "AA".repeat(32))
        );
    }
}
