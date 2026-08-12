const WORKSPACE_HELP: &str = concat!(
    "Manage one explicit multi-repository workspace for the global MCP catalog.\n\n",
    "Usage:\n",
    "  repowitness codex workspace create --name <label> \\\n",
    "      --repository <path> --repository <path> [...]\n",
    "  repowitness codex workspace list\n",
    "  repowitness codex workspace remove --name <label>\n\n",
    "Workspace membership is explicit. Create indexes every member atomically\n",
    "before the workspace becomes visible to `mcp-serve --catalog`.\n",
);

const MAX_WORKSPACE_ARGUMENTS: usize = 70;
const MAX_WORKSPACE_NAME_BYTES: usize = 64;
const WORKSPACE_MIN_MEMBERS: usize = 2;

struct WorkspaceCreateInvocation {
    name: String,
    repositories: Vec<PathBuf>,
}

fn run_codex(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments = args.take(MAX_WORKSPACE_ARGUMENTS + 1).collect::<Vec<_>>();
    if arguments.len() > MAX_WORKSPACE_ARGUMENTS {
        return emit_error(stderr, EXIT_USAGE, "error: codex received too many arguments; use codex --help\n");
    }
    if arguments.is_empty() || matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, WORKSPACE_HELP);
    }
    if arguments.first().is_some_and(|argument| argument == OsStr::new("workspace")) {
        return run_workspace(
            arguments.into_iter().skip(1),
            stdout,
            stderr,
        );
    }
    emit_error(stderr, EXIT_USAGE, "error: codex accepts only workspace; use codex --help\n")
}

fn run_workspace(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments = args.take(MAX_WORKSPACE_ARGUMENTS + 1).collect::<Vec<_>>();
    if arguments.len() > MAX_WORKSPACE_ARGUMENTS {
        return emit_error(stderr, EXIT_USAGE, "error: workspace received too many arguments; use codex --help\n");
    }
    if arguments.as_slice() == [OsString::from("--help")] || arguments.as_slice() == [OsString::from("-h")] {
        return emit_output(stdout, WORKSPACE_HELP);
    }
    match arguments.first().and_then(|argument| argument.to_str()) {
        Some("create") => run_workspace_create(&arguments[1..], stdout, stderr),
        Some("list") if arguments.len() == 1 => run_workspace_list(stdout, stderr),
        Some("remove") => run_workspace_remove(&arguments[1..], stdout, stderr),
        _ => emit_error(stderr, EXIT_USAGE, "error: workspace use create, list, or remove; use codex --help\n"),
    }
}

fn run_workspace_create(
    arguments: &[OsString],
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let invocation = match parse_workspace_create(arguments) {
        Ok(value) => value,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match read_connected_workspace_catalog(None) {
        Ok(catalog) if catalog.workspaces.iter().any(|workspace| workspace.name == invocation.name) => {
            return emit_error(stderr, EXIT_USAGE, "error: workspace name already exists\n");
        }
        Ok(_) => {}
        Err(()) => return emit_error(stderr, EXIT_SOFTWARE, "error: workspace catalog could not be read\n"),
    }
    let mut roots = BTreeSet::new();
    let mut members = Vec::with_capacity(invocation.repositories.len());
    for repository in invocation.repositories {
        let root = match repowitness_local::discovered_worktree_root(&repository) {
            Ok(root) => root,
            Err(_) => return emit_error(stderr, EXIT_SOFTWARE, "error: workspace repository is not an available Git worktree\n"),
        };
        if !roots.insert(root.clone()) {
            return emit_error(stderr, EXIT_USAGE, "error: workspace repositories must be distinct worktrees\n");
        }
        if root.to_str().is_none() {
            return emit_error(stderr, EXIT_USAGE, "error: workspace repository path must be valid UTF-8\n");
        }
        let repository_id = match generate_local_identity(LocalIdentityKind::Repository) {
            Ok(identity) => identity.into_string(),
            Err(_) => return emit_error(stderr, EXIT_SOFTWARE, "error: workspace identity generation failed\n"),
        };
        let source_slot = match generate_local_identity(LocalIdentityKind::SourceSlot) {
            Ok(identity) => identity.into_string(),
            Err(_) => return emit_error(stderr, EXIT_SOFTWARE, "error: workspace identity generation failed\n"),
        };
        members.push(ConnectedWorkspaceMember {
            repository_id,
            source_slot_id: source_slot,
            root,
        });
    }
    let workspace = match index_workspace(&invocation.name, members) {
        Ok(workspace) => workspace,
        Err(message) => return emit_error(stderr, EXIT_SOFTWARE, message),
    };
    let output_name = workspace.name.clone();
    let output_id = workspace.connected_workspace_id.clone();
    let output_members = workspace.members.len();
    if register_mcp_connected_workspace(None, workspace).is_err() {
        return emit_error(stderr, EXIT_SOFTWARE, "error: workspace catalog registration failed\n");
    }
    let _ = writeln!(stdout, "status=ok");
    let _ = writeln!(stdout, "operation=workspace-create");
    let _ = writeln!(stdout, "name={output_name}");
    let _ = writeln!(stdout, "connected_workspace_id={output_id}");
    let _ = writeln!(stdout, "members={output_members}");
    EXIT_SUCCESS
}

fn index_workspace(
    name: &str,
    members: Vec<ConnectedWorkspaceMember>,
) -> Result<ConnectedWorkspaceEntry, &'static str> {
    let connected_workspace_id = generate_local_identity(LocalIdentityKind::ConnectedWorkspace)
        .map_err(|_| "error: workspace identity generation failed\n")?
        .into_string();
    let state_root = default_onboard_state_root()
        .map_err(|_| "error: private workspace state is unavailable\n")?;
    let workspace_dir = prepare_private_state_directory(
        &state_root,
        ONBOARD_STATE_PRODUCT_DIRECTORY,
        ONBOARD_STATE_WORKSPACES_DIRECTORY,
        &connected_workspace_id,
    )
    .map_err(|_| "error: private workspace state is unavailable\n")?;
    let manifest_path = workspace_dir.join(WORKSPACE_MANIFEST_FILE);
    let database = workspace_dir.join(ONBOARD_DATABASE_FILE);
    let manifest = workspace_manifest(&connected_workspace_id, &members);
    let mut manifest_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manifest_path)
        .map_err(|_| "error: workspace manifest could not be written\n")?;
    manifest_file
        .write_all(manifest.as_bytes())
        .map_err(|_| "error: workspace manifest could not be written\n")?;
    let (contents, parent) = read_bounded_regular_file_with_parent(
        &manifest_path,
        repowitness_local::MAX_LOCAL_CONNECTED_WORKSPACE_MANIFEST_BYTES,
    )
    .map_err(|_| "error: workspace manifest could not be admitted\n")?;
    let configuration = resolve_configuration(&[])
        .map_err(|_| "error: workspace configuration failed\n")?;
    let request = LocalConnectedWorkspaceIndexRequest::new(
        contents.bytes(),
        &parent,
        &database,
        &configuration,
        unix_time_millis(),
    )
    .with_deadline(DEFAULT_LOCAL_CONNECTED_WORKSPACE_DEADLINE)
    .map_err(|_| "error: workspace request is invalid\n")?;
    index_local_connected_workspace(request, Arc::new(AtomicBool::new(false)))
        .map_err(|_| "error: workspace indexing failed; catalog membership was not published\n")?;
    Ok(ConnectedWorkspaceEntry {
        name: name.to_owned(),
        connected_workspace_id,
        members,
    })
}

fn run_workspace_list(stdout: &mut impl Write, stderr: &mut impl Write) -> u8 {
    let catalog = match read_connected_workspace_catalog(None) {
        Ok(catalog) => catalog,
        Err(()) => return emit_error(stderr, EXIT_SOFTWARE, "error: workspace catalog could not be read\n"),
    };
    if writeln!(stdout, "status=ok\noperation=workspace-list\nworkspaces={}", catalog.workspaces.len()).is_err() {
        return EXIT_IO;
    }
    for workspace in catalog.workspaces {
        if writeln!(stdout, "name={} members={} connected_workspace_id={}", workspace.name, workspace.members.len(), workspace.connected_workspace_id).is_err() {
            return EXIT_IO;
        }
    }
    EXIT_SUCCESS
}

fn run_workspace_remove(
    arguments: &[OsString],
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    if arguments.len() != 2 || arguments[0] != "--name" {
        return emit_error(stderr, EXIT_USAGE, "error: workspace remove requires --name <label>\n");
    }
    let name = match arguments[1].to_str() {
        Some(name) if valid_workspace_name(name) => name,
        _ => return emit_error(stderr, EXIT_USAGE, "error: workspace name is invalid\n"),
    };
    match remove_mcp_connected_workspace(None, name) {
        Ok(true) => {
            if writeln!(stdout, "status=ok\noperation=workspace-remove\nname={name}").is_ok() { EXIT_SUCCESS } else { EXIT_IO }
        }
        Ok(false) => emit_error(stderr, EXIT_USAGE, "error: workspace name was not found\n"),
        Err(()) => emit_error(stderr, EXIT_SOFTWARE, "error: workspace catalog could not be updated\n"),
    }
}

fn parse_workspace_create(arguments: &[OsString]) -> Result<WorkspaceCreateInvocation, &'static str> {
    let mut name = None;
    let mut repositories = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--name") => {
                index += 1;
                let value = arguments.get(index).and_then(|argument| argument.to_str()).ok_or("error: --name requires a value\n")?;
                if name.replace(value.to_owned()).is_some() { return Err("error: workspace accepts --name only once\n"); }
            }
            Some("--repository") => {
                index += 1;
                let value = arguments.get(index).ok_or("error: --repository requires a value\n")?;
                repositories.push(PathBuf::from(value));
            }
            _ => return Err("error: workspace create accepts only --name and --repository\n"),
        }
        index += 1;
    }
    let name = name.ok_or("error: workspace create requires --name <label>\n")?;
    if !valid_workspace_name(&name) { return Err("error: workspace name is invalid\n"); }
    if repositories.len() < WORKSPACE_MIN_MEMBERS || repositories.len() > repowitness_mcp::MAX_MCP_REGISTERED_REPOSITORIES {
        return Err("error: workspace requires between 2 and 32 repositories\n");
    }
    Ok(WorkspaceCreateInvocation { name, repositories })
}

fn valid_workspace_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_WORKSPACE_NAME_BYTES
        && name.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || (byte == b'-' && index > 0 && index + 1 < name.len())
        })
}

fn workspace_manifest(id: &str, members: &[ConnectedWorkspaceMember]) -> String {
    let mut text = format!("schema_version = 1\nconnected_workspace_id = {}\n", toml_string(id));
    for member in members {
        text.push_str("\n[[source]]\n");
        text.push_str(&format!("source_slot_id = {}\n", toml_string(&member.source_slot_id)));
        text.push_str(&format!("repository_identity = {}\n", toml_string(&member.repository_id)));
        text.push_str(&format!("worktree_root = {}\n", toml_string(member.root.to_str().unwrap_or_default())));
        text.push_str("selector = { kind = \"worktree-head\" }\nscope = { kind = \"whole-repository\" }\n");
    }
    text
}

fn toml_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => escaped.push_str(&format!("\\u{:04X}", character as u32)),
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

fn unix_time_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).ok().and_then(|duration| u64::try_from(duration.as_millis()).ok()).unwrap_or(0)
}

#[cfg(test)]
mod workspace_command_tests {
    use super::*;

    #[test]
    fn workspace_names_are_lowercase_labels_only() {
        assert!(valid_workspace_name("product-stack"));
        assert!(valid_workspace_name("v2"));
        assert!(!valid_workspace_name("Product"));
        assert!(!valid_workspace_name("-product"));
        assert!(!valid_workspace_name("product-"));
        assert!(!valid_workspace_name("product_stack"));
    }

    #[test]
    fn create_requires_two_distinct_explicit_repository_arguments() {
        let arguments = vec![
            OsString::from("--name"),
            OsString::from("product-stack"),
            OsString::from("--repository"),
            OsString::from("one"),
        ];
        assert!(parse_workspace_create(&arguments).is_err());
        let arguments = vec![
            OsString::from("--name"),
            OsString::from("product-stack"),
            OsString::from("--repository"),
            OsString::from("one"),
            OsString::from("--repository"),
            OsString::from("two"),
        ];
        let invocation = parse_workspace_create(&arguments).expect("two roots are admitted");
        assert_eq!(invocation.name, "product-stack");
        assert_eq!(invocation.repositories.len(), 2);
    }

    #[test]
    fn generated_manifest_quotes_paths_without_changing_the_contract() {
        let manifest = workspace_manifest(
            "cwi1:h:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            &[ConnectedWorkspaceMember {
                repository_id: "rwi1:h:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".to_owned(),
                source_slot_id: "ssi1:h:CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC".to_owned(),
                root: PathBuf::from("/tmp/a\\b"),
            }],
        );
        assert!(manifest.contains("worktree_root = \"/tmp/a\\\\b\""));
        assert!(manifest.contains("selector = { kind = \"worktree-head\" }"));
    }
}
