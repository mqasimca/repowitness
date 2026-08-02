const CODEX_HELP: &str = concat!(
    "Install or remove the one global RepoWitness Codex catalog integration.\n\n",
    "Usage:\n",
    "  repowitness codex install [--codex-home <path>]\n",
    "  repowitness codex remove [--codex-home <path>]\n",
    "  repowitness codex session-start\n\n",
    "  repowitness codex workspace create --name <lowercase-label>\n",
    "      --repository <path> --repository <path> [--repository <path>...]\n",
    "      [--codex-home <path>]\n",
    "  repowitness codex workspace list [--codex-home <path>]\n",
    "  repowitness codex workspace remove --name <lowercase-label>\n",
    "      [--codex-home <path>]\n\n",
    "Install appends only a marked MCP server and SessionStart hook to Codex's ",
    "global config. A created workspace is an explicit, private bounded set of ",
    "Git worktrees; its next Codex session atomically refreshes every member and ",
    "uses the current member as the default. No command scans parent, sibling, ",
    "or home directories.\n",
);

const CODEX_CONFIG_FILE: &str = "config.toml";
const CODEX_CATALOG_STATE_DIRECTORY: &str = "repowitness-state";
const CODEX_INTEGRATION_BEGIN: &str = "# >>> repowitness codex catalog >>>";
const CODEX_INTEGRATION_END: &str = "# <<< repowitness codex catalog <<<";
const MAX_CODEX_CONFIG_BYTES: usize = 1024 * 1024;

struct CodexInvocation {
    operation: CodexOperation,
    home: Option<PathBuf>,
}

enum CodexOperation {
    Install,
    Remove,
    SessionStart,
    Workspace(CodexWorkspaceOperation),
}

enum CodexWorkspaceOperation {
    Create { name: String, repositories: Vec<PathBuf> },
    List,
    Remove { name: String },
}

fn run_codex(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments = args.take(MAX_CODEX_ARGUMENTS + 1).collect::<Vec<_>>();
    if arguments.len() > MAX_CODEX_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: codex received too many arguments; use codex --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, CODEX_HELP);
    }
    let invocation = match parse_codex_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    execute_codex_invocation(invocation, stdout, stderr)
}

fn execute_codex_invocation(
    invocation: CodexInvocation,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    match invocation.operation {
        CodexOperation::SessionStart => emit_output(
            stdout,
            "RepoWitness catalog is configured. In a Git worktree, its MCP server refreshes the current worktree when Codex connects; prefer its evidence-backed discovery tools.\n",
        ),
        CodexOperation::Install => run_codex_install(invocation.home.as_deref(), stdout, stderr),
        CodexOperation::Remove => run_codex_remove(invocation.home.as_deref(), stdout, stderr),
        CodexOperation::Workspace(operation) => {
            run_codex_workspace(operation, invocation.home.as_deref(), stdout, stderr)
        }
    }
}

#[cfg(unix)]
fn run_codex_install(home: Option<&Path>, stdout: &mut impl Write, stderr: &mut impl Write) -> u8 {
    let home = match resolve_codex_home(home) {
        Ok(home) => home,
        Err(()) => {
            return emit_error(
                stderr,
                EXIT_SOFTWARE,
                "error: Codex global configuration is unavailable\n",
            );
        }
    };
    match install_codex_catalog(&home) {
        Ok(CodexIntegrationChange::Changed) => emit_output(
            stdout,
            "status=ok\noperation=codex-install\nintegration=global-catalog\nrestart=required\n",
        ),
        Ok(CodexIntegrationChange::Unchanged) => emit_output(
            stdout,
            "status=ok\noperation=codex-install\nintegration=already-installed\nrestart=not-required\n",
        ),
        Err(()) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: Codex global configuration could not be updated\n",
        ),
    }
}

#[cfg(not(unix))]
fn run_codex_install(_home: Option<&Path>, _stdout: &mut impl Write, stderr: &mut impl Write) -> u8 {
    emit_error(
        stderr,
        EXIT_SOFTWARE,
        "error: Codex catalog is unavailable on this platform\n",
    )
}

fn run_codex_remove(home: Option<&Path>, stdout: &mut impl Write, stderr: &mut impl Write) -> u8 {
    let home = match resolve_codex_home(home) {
        Ok(home) => home,
        Err(()) => {
            return emit_error(
                stderr,
                EXIT_SOFTWARE,
                "error: Codex global configuration is unavailable\n",
            );
        }
    };
    match remove_codex_catalog(&home) {
        Ok(CodexIntegrationChange::Changed) => emit_output(
            stdout,
            "status=ok\noperation=codex-remove\nintegration=removed\nrestart=required\n",
        ),
        Ok(CodexIntegrationChange::Unchanged) => emit_output(
            stdout,
            "status=ok\noperation=codex-remove\nintegration=absent\nrestart=not-required\n",
        ),
        Err(()) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: Codex global configuration could not be updated\n",
        ),
    }
}

fn run_codex_workspace(
    operation: CodexWorkspaceOperation,
    home: Option<&Path>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let home = match resolve_codex_home(home) {
        Ok(home) => home,
        Err(()) => {
            return emit_error(
                stderr,
                EXIT_SOFTWARE,
                "error: Codex global configuration is unavailable\n",
            );
        }
    };
    match operation {
        CodexWorkspaceOperation::Create { name, repositories } => {
            match create_codex_connected_workspace(&home, &name, &repositories) {
                Ok(member_count) => emit_codex_workspace_create(stdout, &name, member_count),
                Err(()) => emit_error(
                    stderr,
                    EXIT_SOFTWARE,
                    "error: Codex workspace could not be created\n",
                ),
            }
        }
        CodexWorkspaceOperation::List => match list_codex_connected_workspaces(&home) {
            Ok(workspaces) => emit_codex_workspace_list(stdout, &workspaces),
            Err(()) => emit_error(
                stderr,
                EXIT_SOFTWARE,
                "error: Codex workspaces are unavailable\n",
            ),
        },
        CodexWorkspaceOperation::Remove { name } => {
            match remove_codex_connected_workspace(&home, &name) {
                Ok(true) => emit_output(
                    stdout,
                    "status=ok\noperation=codex-workspace-remove\nregistration=removed\nindex_retained=true\n",
                ),
                Ok(false) => emit_output(
                    stdout,
                    "status=ok\noperation=codex-workspace-remove\nregistration=absent\nindex_retained=true\n",
                ),
                Err(()) => emit_error(
                    stderr,
                    EXIT_SOFTWARE,
                    "error: Codex workspace could not be removed\n",
                ),
            }
        }
    }
}

fn parse_codex_arguments(arguments: &[OsString]) -> Result<CodexInvocation, &'static str> {
    let Some(operation) = arguments.first().and_then(|value| value.to_str()) else {
        return Err("error: codex requires install, remove, session-start, or workspace; use codex --help\n");
    };
    let operation = match operation {
        "install" => CodexOperation::Install,
        "remove" => CodexOperation::Remove,
        "session-start" => CodexOperation::SessionStart,
        "workspace" => CodexOperation::Workspace(parse_codex_workspace_arguments(&arguments[1..])?),
        _ => return Err("error: unknown codex command; use codex --help\n"),
    };
    let remaining = &arguments[1..];
    if matches!(operation, CodexOperation::Workspace(_)) {
        let home = parse_codex_home_option(remaining)?;
        return Ok(CodexInvocation {
            operation,
            home,
        });
    }
    if matches!(operation, CodexOperation::SessionStart) {
        return remaining
            .is_empty()
            .then_some(CodexInvocation {
                operation,
                home: None,
            })
            .ok_or("error: codex session-start accepts no options\n");
    }
    if remaining.is_empty() {
        return Ok(CodexInvocation {
            operation,
            home: None,
        });
    }
    if remaining.len() != 2 || remaining[0] != OsStr::new("--codex-home") {
        return Err("error: codex accepts only --codex-home <absolute-path>\n");
    }
    let home = parse_codex_home_option(remaining)?
        .ok_or("error: codex accepts only --codex-home <absolute-path>\n")?;
    Ok(CodexInvocation {
        operation,
        home: Some(home),
    })
}

fn parse_codex_workspace_arguments(
    arguments: &[OsString],
) -> Result<CodexWorkspaceOperation, &'static str> {
    let Some(operation) = arguments.first().and_then(|value| value.to_str()) else {
        return Err("error: codex workspace requires create, list, or remove; use codex --help\n");
    };
    let mut name = None;
    let mut repositories = Vec::new();
    let mut index = 1_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        let value = arguments
            .get(index + 1)
            .ok_or("error: codex workspace options require values; use codex --help\n")?;
        index += 2;
        if option == OsStr::new("--name") {
            let value = value
                .to_str()
                .ok_or("error: codex workspace name must be valid UTF-8\n")?;
            if name.replace(value.to_owned()).is_some() {
                return Err("error: codex workspace accepts --name only once\n");
            }
        } else if option == OsStr::new("--repository") {
            repositories.push(PathBuf::from(value));
        } else if option == OsStr::new("--codex-home") {
            if value.is_empty() || !Path::new(value).is_absolute() {
                return Err("error: codex home must be an absolute non-empty path\n");
            }
        } else {
            return Err("error: unknown codex workspace option; use codex --help\n");
        }
    }
    match operation {
        "create" => {
            let name = name.ok_or("error: codex workspace create requires --name\n")?;
            if repositories.len() < 2 {
                return Err("error: codex workspace create requires at least two --repository values\n");
            }
            Ok(CodexWorkspaceOperation::Create { name, repositories })
        }
        "list" if name.is_none() && repositories.is_empty() => Ok(CodexWorkspaceOperation::List),
        "remove" if repositories.is_empty() => Ok(CodexWorkspaceOperation::Remove {
            name: name.ok_or("error: codex workspace remove requires --name\n")?,
        }),
        "list" => Err("error: codex workspace list accepts only --codex-home <absolute-path>\n"),
        "remove" => Err("error: codex workspace remove accepts only --name and --codex-home\n"),
        _ => Err("error: codex workspace requires create, list, or remove; use codex --help\n"),
    }
}

fn parse_codex_home_option(arguments: &[OsString]) -> Result<Option<PathBuf>, &'static str> {
    let mut home = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        if arguments[index] != OsStr::new("--codex-home") {
            index += 1;
            continue;
        }
        let value = arguments
            .get(index + 1)
            .ok_or("error: codex accepts only --codex-home <absolute-path>\n")?;
        let candidate = PathBuf::from(value);
        if candidate.as_os_str().is_empty() || !candidate.is_absolute() || home.replace(candidate).is_some() {
            return Err("error: codex home must be an absolute non-empty path\n");
        }
        index += 2;
    }
    Ok(home)
}

fn emit_codex_workspace_create(writer: &mut impl Write, name: &str, member_count: usize) -> u8 {
    let result = writeln!(writer, "status=ok")
        .and_then(|()| writeln!(writer, "operation=codex-workspace-create"))
        .and_then(|()| writeln!(writer, "workspace={name}"))
        .and_then(|()| writeln!(writer, "members={member_count}"))
        .and_then(|()| writeln!(writer, "index=published"));
    if result.is_ok() { EXIT_SUCCESS } else { EXIT_IO }
}

fn emit_codex_workspace_list(writer: &mut impl Write, workspaces: &[(String, usize)]) -> u8 {
    let mut result = writeln!(writer, "status=ok")
        .and_then(|()| writeln!(writer, "operation=codex-workspace-list"))
        .and_then(|()| writeln!(writer, "workspaces={}", workspaces.len()));
    for (index, (name, members)) in workspaces.iter().enumerate() {
        result = result
            .and_then(|()| writeln!(writer, "workspace_{index}={name}"))
            .and_then(|()| writeln!(writer, "workspace_{index}_members={members}"));
    }
    if result.is_ok() { EXIT_SUCCESS } else { EXIT_IO }
}

fn resolve_codex_home(requested: Option<&Path>) -> Result<PathBuf, ()> {
    let candidate = match requested {
        Some(path) => path.to_owned(),
        None => std::env::var_os("CODEX_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
            .ok_or(())?,
    };
    if !candidate.is_absolute() {
        return Err(());
    }
    let metadata = std::fs::symlink_metadata(&candidate).map_err(|_| ())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(());
    }
    std::fs::canonicalize(candidate).map_err(|_| ())
}

enum CodexIntegrationChange {
    Changed,
    Unchanged,
}

#[cfg(unix)]
fn install_codex_catalog(home: &Path) -> Result<CodexIntegrationChange, ()> {
    let configuration_path = home.join(CODEX_CONFIG_FILE);
    let configuration = read_codex_configuration(&configuration_path)?;
    if codex_integration_bounds(&configuration)?.is_some() {
        return Ok(CodexIntegrationChange::Unchanged);
    }
    if codex_configuration_has_unmanaged_integration(&configuration) {
        return Err(());
    }
    let mut updated = configuration;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    if !updated.is_empty() {
        updated.push('\n');
    }
    updated.push_str(&codex_integration_toml(home)?);
    write_codex_configuration(&configuration_path, updated.as_bytes())?;
    Ok(CodexIntegrationChange::Changed)
}

#[cfg(unix)]
fn codex_configuration_has_unmanaged_integration(configuration: &str) -> bool {
    let mut in_mcp_servers = false;
    for line in configuration.lines().map(str::trim) {
        if matches!(
            line,
            "[mcp_servers.repowitness]"
                | "[mcp_servers.\"repowitness\"]"
                | "[mcp_servers.'repowitness']"
        ) || line.contains("repowitness codex session-start")
        {
            return true;
        }
        if line == "[mcp_servers]" {
            in_mcp_servers = true;
            continue;
        }
        if line.starts_with('[') {
            in_mcp_servers = false;
            continue;
        }
        if in_mcp_servers
            && line
                .split_once('=')
                .is_some_and(|(key, _)| matches!(key.trim(), "repowitness" | "\"repowitness\"" | "'repowitness'"))
        {
            return true;
        }
    }
    false
}

#[cfg(unix)]
fn codex_integration_toml(home: &Path) -> Result<String, ()> {
    let state_directory = home.join(CODEX_CATALOG_STATE_DIRECTORY);
    let state_directory = state_directory.to_str().ok_or(())?;
    let state_directory = serde_json::to_string(state_directory).map_err(|_| ())?;
    Ok(format!(
        "{CODEX_INTEGRATION_BEGIN}\n[mcp_servers.repowitness]\ncommand = \"repowitness\"\nargs = [\"mcp-serve\", \"--catalog\", \"--catalog-state-dir\", {state_directory}]\n\n[[hooks.SessionStart]]\nmatcher = \"startup|resume|clear|compact\"\n\n[[hooks.SessionStart.hooks]]\ntype = \"command\"\ncommand = \"repowitness codex session-start\"\ntimeout = 2\n{CODEX_INTEGRATION_END}\n"
    ))
}

fn remove_codex_catalog(home: &Path) -> Result<CodexIntegrationChange, ()> {
    let configuration_path = home.join(CODEX_CONFIG_FILE);
    let configuration = read_codex_configuration(&configuration_path)?;
    let Some((mut start, end)) = codex_integration_bounds(&configuration)? else {
        return Ok(CodexIntegrationChange::Unchanged);
    };
    if configuration[..start].ends_with("\n\n") {
        start -= 1;
    }
    let mut updated = String::with_capacity(configuration.len() - (end - start));
    updated.push_str(&configuration[..start]);
    updated.push_str(&configuration[end..]);
    while updated.ends_with("\n\n\n") {
        updated.pop();
    }
    write_codex_configuration(&configuration_path, updated.as_bytes())?;
    Ok(CodexIntegrationChange::Changed)
}

fn codex_integration_bounds(configuration: &str) -> Result<Option<(usize, usize)>, ()> {
    let mut begins = Vec::new();
    let mut ends = Vec::new();
    let mut offset = 0_usize;
    for segment in configuration.split_inclusive('\n') {
        let line = segment.trim_end_matches(['\n', '\r']);
        if line == CODEX_INTEGRATION_BEGIN {
            begins.push(offset);
        } else if line == CODEX_INTEGRATION_END {
            ends.push(offset + segment.len());
        }
        offset += segment.len();
    }
    if begins.is_empty() && ends.is_empty() {
        return Ok(None);
    }
    if begins.len() != 1 || ends.len() != 1 || begins[0] >= ends[0] {
        return Err(());
    }
    Ok(Some((begins[0], ends[0])))
}

fn read_codex_configuration(path: &Path) -> Result<String, ()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => return Err(()),
        Ok(_) => {}
        Err(_) => return Err(()),
    }
    let contents = read_bounded_regular_file_with_parent(path, MAX_CODEX_CONFIG_BYTES)
        .map_err(|_| ())?
        .0;
    std::str::from_utf8(contents.bytes())
        .map(str::to_owned)
        .map_err(|_| ())
}

fn write_codex_configuration(path: &Path, contents: &[u8]) -> Result<(), ()> {
    if contents.len() > MAX_CODEX_CONFIG_BYTES {
        return Err(());
    }
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) | Err(_) => return Err(()),
    }
    let parent = path.parent().ok_or(())?;
    let temporary = parent.join(format!(
        ".repowitness-codex-{}-{}.tmp",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ())?
            .as_nanos(),
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| ())?;
    if file.write_all(contents).is_err() || file.sync_all().is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(());
    }
    if std::fs::rename(&temporary, path).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(());
    }
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| ())?;
    Ok(())
}
