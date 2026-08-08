const MAX_MCP_REPOSITORY_REGISTRY_BYTES: usize = 64 * 1024;

const MCP_CATALOG_FILE: &str = "mcp-catalog-v1.json";
const MCP_CONNECTED_WORKSPACE_CATALOG_FILE: &str = "mcp-connected-workspaces-v1.json";
const ONBOARD_STATE_WORKSPACES_DIRECTORY: &str = "workspaces";
const CONNECTED_WORKSPACE_MANIFEST_FILE: &str = "connected-workspace.toml";
const MAX_CODEX_CONNECTED_WORKSPACES: usize = 32;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct McpRepositoryRegistryDocument {
    schema_version: u8,
    repositories: Vec<McpRepositoryRegistryEntry>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct McpRepositoryRegistryEntry {
    repository_id: String,
    root: String,
    database: String,
}

struct RegisteredMcpRepository {
    repository_identity: String,
    root: PathBuf,
    database: PathBuf,
    graph_workspace: GraphWorkspaceContext,
}

struct PreparedMcpRepositoryCatalog {
    repositories: Vec<RegisteredMcpRepository>,
    default_repository_identity: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CodexConnectedWorkspaceCatalogDocument {
    schema_version: u8,
    workspaces: Vec<CodexConnectedWorkspaceCatalogEntry>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CodexConnectedWorkspaceCatalogEntry {
    name: String,
    connected_workspace_id: String,
    members: Vec<CodexConnectedWorkspaceCatalogMember>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CodexConnectedWorkspaceCatalogMember {
    repository_id: String,
    source_slot_id: String,
    root: String,
}

#[derive(Clone)]
struct RegisteredCodexConnectedWorkspace {
    name: String,
    connected_workspace_identity: String,
    members: Vec<RegisteredCodexConnectedWorkspaceMember>,
}

#[derive(Clone)]
struct RegisteredCodexConnectedWorkspaceMember {
    repository_identity: String,
    source_slot_identity: String,
    root: PathBuf,
}

fn read_mcp_repository_registry(path: &Path) -> Result<Vec<RegisteredMcpRepository>, ()> {
    let contents = read_bounded_regular_file_with_parent(path, MAX_MCP_REPOSITORY_REGISTRY_BYTES)
        .map_err(|_| ())?
        .0;
    let document = serde_json::from_slice::<McpRepositoryRegistryDocument>(contents.bytes())
        .map_err(|_| ())?;
    if document.schema_version != 1
        || document.repositories.is_empty()
        || document.repositories.len() > repowitness_mcp::MAX_MCP_REGISTERED_REPOSITORIES
    {
        return Err(());
    }

    let mut repository_ids = std::collections::BTreeSet::new();
    let mut roots = std::collections::BTreeSet::new();
    let mut databases = std::collections::BTreeSet::new();
    let mut repositories = Vec::with_capacity(document.repositories.len());
    for entry in document.repositories {
        RepositoryIdentityTextV1::decode(&entry.repository_id).map_err(|_| ())?;
        let root = registry_host_path(&entry.root)?;
        let database = registry_host_path(&entry.database)?;
        if !repository_ids.insert(entry.repository_id.clone())
            || !roots.insert(root.clone())
            || !databases.insert(database.clone())
        {
            return Err(());
        }
        repositories.push(RegisteredMcpRepository {
            graph_workspace: GraphWorkspaceContext::SingleRepository(entry.repository_id.clone()),
            repository_identity: entry.repository_id,
            root,
            database,
        });
    }
    Ok(repositories)
}

fn registry_host_path(value: &str) -> Result<PathBuf, ()> {
    if value.is_empty() || value.contains('\0') {
        return Err(());
    }
    let path = PathBuf::from(value);
    path.is_absolute().then_some(path).ok_or(())
}

fn prepare_current_worktree_mcp_catalog(
    requested_state_root: Option<&Path>,
    configuration: &ResolvedConfiguration,
) -> Result<PreparedMcpRepositoryCatalog, ()> {
    let selected_state_root = match requested_state_root {
        Some(path) => path.to_owned(),
        None => default_onboard_state_root()?,
    };
    let state_root = canonical_path_with_uncreated_suffix(&selected_state_root)?;
    let catalog_path = mcp_catalog_path(&state_root);
    let current_root = resolve_current_worktree_root()?;
    let connected_workspaces = read_codex_connected_workspaces(
        &codex_connected_workspace_catalog_path(&state_root),
        &state_root,
    )?;
    let matching_workspaces = connected_workspaces
        .iter()
        .filter(|workspace| {
            workspace
                .members
                .iter()
                .any(|member| member.root == current_root)
        })
        .collect::<Vec<_>>();
    if matching_workspaces.len() > 1 {
        return Err(());
    }
    if let Some(workspace) = matching_workspaces.first() {
        return prepare_connected_workspace_mcp_catalog(
            workspace,
            &current_root,
            &state_root,
            configuration,
        );
    }
    let mut repositories = read_catalog_repositories(&catalog_path, &state_root)?;
    let current_index = repositories
        .iter()
        .position(|repository| repository.root == current_root);
    let default_repository_identity = match current_index {
        Some(index) => repositories[index].repository_identity.clone(),
        None => {
            if repositories.len() >= repowitness_mcp::MAX_MCP_REGISTERED_REPOSITORIES {
                return Err(());
            }
            OsIdentityGenerator
                .generate(LocalIdentityKind::Repository)
                .map_err(|_| ())?
        }
    };

    let prepared_database = PrivateOnboardStateDirectory.prepare_database(
        &current_root,
        Some(&state_root),
        &default_repository_identity,
    )?;
    if let Some(index) = current_index
        && repositories[index].database != prepared_database.database
    {
        return Err(());
    }
    LocalRepositoryIndexer.reconcile(
        &IndexInvocation {
            repository_root: current_root.clone(),
            database: prepared_database.database.clone(),
            repository_identity: OsString::from(&default_repository_identity),
        },
        configuration,
    )
    .map_err(|_| ())?;

    if current_index.is_none() {
        repositories.push(RegisteredMcpRepository {
            repository_identity: default_repository_identity.clone(),
            root: current_root,
            database: prepared_database.database,
            graph_workspace: GraphWorkspaceContext::SingleRepository(
                default_repository_identity.clone(),
            ),
        });
        write_catalog_repositories(&catalog_path, &repositories)?;
    }
    Ok(PreparedMcpRepositoryCatalog {
        repositories,
        default_repository_identity,
    })
}

fn mcp_catalog_path(state_root: &Path) -> PathBuf {
    state_root
        .join(ONBOARD_STATE_PRODUCT_DIRECTORY)
        .join(MCP_CATALOG_FILE)
}

fn codex_connected_workspace_catalog_path(state_root: &Path) -> PathBuf {
    state_root
        .join(ONBOARD_STATE_PRODUCT_DIRECTORY)
        .join(MCP_CONNECTED_WORKSPACE_CATALOG_FILE)
}

fn connected_workspace_directory(state_root: &Path, identity: &str) -> PathBuf {
    state_root
        .join(ONBOARD_STATE_PRODUCT_DIRECTORY)
        .join(ONBOARD_STATE_WORKSPACES_DIRECTORY)
        .join(identity)
}

fn connected_workspace_manifest_path(state_root: &Path, identity: &str) -> PathBuf {
    connected_workspace_directory(state_root, identity).join(CONNECTED_WORKSPACE_MANIFEST_FILE)
}

fn connected_workspace_database_path(state_root: &Path, identity: &str) -> PathBuf {
    connected_workspace_directory(state_root, identity).join(ONBOARD_DATABASE_FILE)
}

fn prepare_connected_workspace_mcp_catalog(
    workspace: &RegisteredCodexConnectedWorkspace,
    current_root: &Path,
    state_root: &Path,
    configuration: &ResolvedConfiguration,
) -> Result<PreparedMcpRepositoryCatalog, ()> {
    let manifest_path = connected_workspace_manifest_path(
        state_root,
        &workspace.connected_workspace_identity,
    );
    let (manifest, parent) = read_bounded_regular_file_with_parent(
        &manifest_path,
        repowitness_local::MAX_LOCAL_CONNECTED_WORKSPACE_MANIFEST_BYTES,
    )
    .map_err(|_| ())?;
    let expected_manifest = render_connected_workspace_manifest(workspace)?;
    if manifest.bytes() != expected_manifest.as_bytes() {
        return Err(());
    }
    let applied_at_unix_ms = unix_time_millis()?;
    let database =
        connected_workspace_database_path(state_root, &workspace.connected_workspace_identity);
    let request = LocalConnectedWorkspaceIndexRequest::new(
        manifest.bytes(),
        &parent,
        &database,
        configuration,
        applied_at_unix_ms,
    );
    index_local_connected_workspace(request, Arc::new(AtomicBool::new(false))).map_err(|_| ())?;

    let mut default_repository_identity = None;
    let mut repositories = Vec::with_capacity(workspace.members.len());
    for member in &workspace.members {
        if member.root == current_root {
            default_repository_identity = Some(member.repository_identity.clone());
        }
        repositories.push(RegisteredMcpRepository {
            repository_identity: member.repository_identity.clone(),
            root: member.root.clone(),
            database: connected_workspace_database_path(
                state_root,
                &workspace.connected_workspace_identity,
            ),
            graph_workspace: GraphWorkspaceContext::ConnectedWorkspace {
                connected_workspace: workspace.connected_workspace_identity.clone(),
                source_slot: member.source_slot_identity.clone(),
            },
        });
    }
    Ok(PreparedMcpRepositoryCatalog {
        repositories,
        default_repository_identity: default_repository_identity.ok_or(())?,
    })
}

fn read_catalog_repositories(
    path: &Path,
    state_root: &Path,
) -> Result<Vec<RegisteredMcpRepository>, ()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(()),
    }
    let repositories = read_mcp_repository_registry(path)?;
    for repository in &repositories {
        let expected_database = state_root
            .join(ONBOARD_STATE_PRODUCT_DIRECTORY)
            .join(ONBOARD_STATE_REPOSITORIES_DIRECTORY)
            .join(&repository.repository_identity)
            .join(ONBOARD_DATABASE_FILE);
        if canonical_path_with_uncreated_suffix(&repository.root)? != repository.root
            || canonical_path_with_uncreated_suffix(&repository.database)? != repository.database
            || repository.database != expected_database
        {
            return Err(());
        }
    }
    Ok(repositories)
}

fn read_codex_connected_workspaces(
    path: &Path,
    state_root: &Path,
) -> Result<Vec<RegisteredCodexConnectedWorkspace>, ()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(()),
    }
    let contents = read_bounded_regular_file_with_parent(path, MAX_MCP_REPOSITORY_REGISTRY_BYTES)
        .map_err(|_| ())?
        .0;
    let document = serde_json::from_slice::<CodexConnectedWorkspaceCatalogDocument>(contents.bytes())
        .map_err(|_| ())?;
    if document.schema_version != 1 || document.workspaces.len() > MAX_CODEX_CONNECTED_WORKSPACES {
        return Err(());
    }
    let mut names = std::collections::BTreeSet::new();
    let mut workspace_ids = std::collections::BTreeSet::new();
    let mut roots = std::collections::BTreeSet::new();
    let mut workspaces = Vec::with_capacity(document.workspaces.len());
    for entry in document.workspaces {
        if !valid_codex_workspace_name(&entry.name)
            || !names.insert(entry.name.clone())
            || !workspace_ids.insert(entry.connected_workspace_id.clone())
        {
            return Err(());
        }
        ConnectedWorkspaceIdTextV1::decode(&entry.connected_workspace_id).map_err(|_| ())?;
        if !(2..=repowitness_mcp::MAX_MCP_REGISTERED_REPOSITORIES).contains(&entry.members.len()) {
            return Err(());
        }
        let mut repository_ids = std::collections::BTreeSet::new();
        let mut source_slots = std::collections::BTreeSet::new();
        let mut members = Vec::with_capacity(entry.members.len());
        for member in entry.members {
            RepositoryIdentityTextV1::decode(&member.repository_id).map_err(|_| ())?;
            SourceSlotIdTextV1::decode(&member.source_slot_id).map_err(|_| ())?;
            let root = registry_host_path(&member.root)?;
            if !repository_ids.insert(member.repository_id.clone())
                || !source_slots.insert(member.source_slot_id.clone())
                || !roots.insert(root.clone())
                || canonical_path_with_uncreated_suffix(&root)? != root
            {
                return Err(());
            }
            members.push(RegisteredCodexConnectedWorkspaceMember {
                repository_identity: member.repository_id,
                source_slot_identity: member.source_slot_id,
                root,
            });
        }
        let workspace = RegisteredCodexConnectedWorkspace {
            name: entry.name,
            connected_workspace_identity: entry.connected_workspace_id,
            members,
        };
        let expected_manifest = render_connected_workspace_manifest(&workspace)?;
        let manifest_path = connected_workspace_manifest_path(
            state_root,
            &workspace.connected_workspace_identity,
        );
        let (manifest, _) = read_bounded_regular_file_with_parent(
            &manifest_path,
            repowitness_local::MAX_LOCAL_CONNECTED_WORKSPACE_MANIFEST_BYTES,
        )
        .map_err(|_| ())?;
        if manifest.bytes() != expected_manifest.as_bytes()
            || canonical_path_with_uncreated_suffix(&connected_workspace_database_path(
                state_root,
                &workspace.connected_workspace_identity,
            ))?
                != connected_workspace_database_path(state_root, &workspace.connected_workspace_identity)
        {
            return Err(());
        }
        workspaces.push(workspace);
    }
    Ok(workspaces)
}

fn write_catalog_repositories(
    path: &Path,
    repositories: &[RegisteredMcpRepository],
) -> Result<(), ()> {
    if repositories.is_empty()
        || repositories.len() > repowitness_mcp::MAX_MCP_REGISTERED_REPOSITORIES
    {
        return Err(());
    }
    let mut entries = repositories
        .iter()
        .map(|repository| {
            Ok(McpRepositoryRegistryEntry {
                repository_id: repository.repository_identity.clone(),
                root: repository.root.to_str().ok_or(())?.to_owned(),
                database: repository.database.to_str().ok_or(())?.to_owned(),
            })
        })
        .collect::<Result<Vec<_>, ()>>()?;
    entries.sort_by(|left, right| left.repository_id.cmp(&right.repository_id));
    let document = McpRepositoryRegistryDocument {
        schema_version: 1,
        repositories: entries,
    };
    let encoded = serde_json::to_vec(&document).map_err(|_| ())?;
    if encoded.len() > MAX_MCP_REPOSITORY_REGISTRY_BYTES {
        return Err(());
    }
    write_private_catalog_document(path, MCP_CATALOG_FILE, &encoded)
}

fn write_codex_connected_workspaces(
    path: &Path,
    workspaces: &[RegisteredCodexConnectedWorkspace],
) -> Result<(), ()> {
    if workspaces.len() > MAX_CODEX_CONNECTED_WORKSPACES {
        return Err(());
    }
    let mut entries = workspaces
        .iter()
        .map(|workspace| {
            let mut members = workspace
                .members
                .iter()
                .map(|member| {
                    Ok(CodexConnectedWorkspaceCatalogMember {
                        repository_id: member.repository_identity.clone(),
                        source_slot_id: member.source_slot_identity.clone(),
                        root: member.root.to_str().ok_or(())?.to_owned(),
                    })
                })
                .collect::<Result<Vec<_>, ()>>()?;
            members.sort_by(|left, right| left.source_slot_id.cmp(&right.source_slot_id));
            Ok(CodexConnectedWorkspaceCatalogEntry {
                name: workspace.name.clone(),
                connected_workspace_id: workspace.connected_workspace_identity.clone(),
                members,
            })
        })
        .collect::<Result<Vec<_>, ()>>()?;
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let encoded = serde_json::to_vec(&CodexConnectedWorkspaceCatalogDocument {
        schema_version: 1,
        workspaces: entries,
    })
    .map_err(|_| ())?;
    if encoded.len() > MAX_MCP_REPOSITORY_REGISTRY_BYTES {
        return Err(());
    }
    write_private_catalog_document(path, MCP_CONNECTED_WORKSPACE_CATALOG_FILE, &encoded)
}

fn write_private_catalog_document(path: &Path, label: &str, encoded: &[u8]) -> Result<(), ()> {
    let parent = path.parent().ok_or(())?;
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) | Err(_) => return Err(()),
    }
    let temporary = parent.join(format!(
        ".{label}-{}-{}.tmp",
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
    if file.write_all(encoded).is_err() || file.sync_all().is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(());
    }
    if std::fs::rename(&temporary, path).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(());
    }
    Ok(())
}

fn valid_codex_workspace_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .copied()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn render_connected_workspace_manifest(
    workspace: &RegisteredCodexConnectedWorkspace,
) -> Result<String, ()> {
    let mut members = workspace.members.clone();
    members.sort_by(|left, right| left.source_slot_identity.cmp(&right.source_slot_identity));
    let mut manifest = format!(
        "schema_version = 1\nconnected_workspace_id = {}\n",
        serde_json::to_string(&workspace.connected_workspace_identity).map_err(|_| ())?,
    );
    for member in members {
        let root = member.root.to_str().ok_or(())?;
        manifest.push_str("\n[[source]]\n");
        manifest.push_str(&format!(
            "source_slot_id = {}\nrepository_identity = {}\nworktree_root = {}\n\n[source.selector]\nkind = \"worktree-head\"\n\n[source.scope]\nkind = \"whole-repository\"\n",
            serde_json::to_string(&member.source_slot_identity).map_err(|_| ())?,
            serde_json::to_string(&member.repository_identity).map_err(|_| ())?,
            serde_json::to_string(root).map_err(|_| ())?,
        ));
    }
    (manifest.len() <= repowitness_local::MAX_LOCAL_CONNECTED_WORKSPACE_MANIFEST_BYTES)
        .then_some(manifest)
        .ok_or(())
}

fn unix_time_millis() -> Result<u64, ()> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ())?
            .as_millis(),
    )
    .map_err(|_| ())
}

fn create_codex_connected_workspace(
    codex_home: &Path,
    name: &str,
    requested_roots: &[PathBuf],
) -> Result<usize, ()> {
    if !valid_codex_workspace_name(name)
        || !(2..=repowitness_mcp::MAX_MCP_REGISTERED_REPOSITORIES)
            .contains(&requested_roots.len())
    {
        return Err(());
    }
    let state_root = codex_catalog_state_root(codex_home)?;
    let catalog_path = codex_connected_workspace_catalog_path(&state_root);
    let existing = read_codex_connected_workspaces(&catalog_path, &state_root)?;
    if existing.iter().any(|workspace| workspace.name == name) {
        return Err(());
    }

    let mut roots = std::collections::BTreeSet::new();
    let mut members = Vec::with_capacity(requested_roots.len());
    for requested_root in requested_roots {
        let root = resolve_explicit_worktree_root(requested_root)?;
        if !roots.insert(root.clone()) || LocalRepositoryPathInspector.inspect(&root).is_err() {
            return Err(());
        }
        if existing
            .iter()
            .flat_map(|workspace| &workspace.members)
            .any(|member| member.root == root)
        {
            return Err(());
        }
        let repository_identity = OsIdentityGenerator
            .generate(LocalIdentityKind::Repository)
            .map_err(|_| ())?;
        let source_slot_identity = OsIdentityGenerator
            .generate(LocalIdentityKind::SourceSlot)
            .map_err(|_| ())?;
        members.push(RegisteredCodexConnectedWorkspaceMember {
            repository_identity,
            source_slot_identity,
            root,
        });
    }
    let workspace = RegisteredCodexConnectedWorkspace {
        name: name.to_owned(),
        connected_workspace_identity: OsIdentityGenerator
            .generate(LocalIdentityKind::ConnectedWorkspace)
            .map_err(|_| ())?,
        members,
    };
    let workspace_directory = prepare_private_connected_workspace_directory(&state_root, &workspace)?;
    let manifest_path = workspace_directory.join(CONNECTED_WORKSPACE_MANIFEST_FILE);
    let manifest = render_connected_workspace_manifest(&workspace)?;
    write_private_catalog_document(
        &manifest_path,
        CONNECTED_WORKSPACE_MANIFEST_FILE,
        manifest.as_bytes(),
    )?;
    let (admitted_manifest, parent) = read_bounded_regular_file_with_parent(
        &manifest_path,
        repowitness_local::MAX_LOCAL_CONNECTED_WORKSPACE_MANIFEST_BYTES,
    )
    .map_err(|_| ())?;
    if admitted_manifest.bytes() != manifest.as_bytes() {
        return Err(());
    }
    let configuration = resolve_configuration(&[]).map_err(|_| ())?;
    let database = workspace_directory.join(ONBOARD_DATABASE_FILE);
    let request = LocalConnectedWorkspaceIndexRequest::new(
        admitted_manifest.bytes(),
        &parent,
        &database,
        &configuration,
        unix_time_millis()?,
    );
    index_local_connected_workspace(request, Arc::new(AtomicBool::new(false))).map_err(|_| ())?;

    let mut updated = existing;
    updated.push(workspace);
    write_codex_connected_workspaces(&catalog_path, &updated)?;
    Ok(requested_roots.len())
}

fn list_codex_connected_workspaces(
    codex_home: &Path,
) -> Result<Vec<(String, usize)>, ()> {
    let state_root = codex_catalog_state_root(codex_home)?;
    let mut result = read_codex_connected_workspaces(
        &codex_connected_workspace_catalog_path(&state_root),
        &state_root,
    )?
    .into_iter()
    .map(|workspace| (workspace.name, workspace.members.len()))
    .collect::<Vec<_>>();
    result.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(result)
}

fn remove_codex_connected_workspace(codex_home: &Path, name: &str) -> Result<bool, ()> {
    if !valid_codex_workspace_name(name) {
        return Err(());
    }
    let state_root = codex_catalog_state_root(codex_home)?;
    let catalog_path = codex_connected_workspace_catalog_path(&state_root);
    let mut workspaces = read_codex_connected_workspaces(&catalog_path, &state_root)?;
    let before = workspaces.len();
    workspaces.retain(|workspace| workspace.name != name);
    if workspaces.len() == before {
        return Ok(false);
    }
    write_codex_connected_workspaces(&catalog_path, &workspaces)?;
    Ok(true)
}

fn codex_catalog_state_root(codex_home: &Path) -> Result<PathBuf, ()> {
    let home = resolve_codex_home(Some(codex_home))?;
    canonical_path_with_uncreated_suffix(&home.join(CODEX_CATALOG_STATE_DIRECTORY))
}

fn resolve_explicit_worktree_root(path: &Path) -> Result<PathBuf, ()> {
    let mut current = std::fs::canonicalize(path).map_err(|_| ())?;
    loop {
        let marker = current.join(".git");
        match std::fs::symlink_metadata(&marker) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(()),
            Ok(metadata) if metadata.is_dir() || metadata.is_file() => return Ok(current),
            Ok(_) => return Err(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(()),
        }
        if !current.pop() {
            return Err(());
        }
    }
}

fn prepare_private_connected_workspace_directory(
    state_root: &Path,
    workspace: &RegisteredCodexConnectedWorkspace,
) -> Result<PathBuf, ()> {
    let workspace_directory = connected_workspace_directory(
        state_root,
        &workspace.connected_workspace_identity,
    );
    for member in &workspace.members {
        ensure_outside_repository(&member.root, &workspace_directory)?;
    }
    #[cfg(unix)]
    {
        let state_root_directory = open_private_state_root(state_root)?;
        let product = open_or_create_private_directory(
            &state_root_directory,
            OsStr::new(ONBOARD_STATE_PRODUCT_DIRECTORY),
        )?;
        let workspaces = open_or_create_private_directory(
            &product,
            OsStr::new(ONBOARD_STATE_WORKSPACES_DIRECTORY),
        )?;
        open_or_create_private_directory(
            &workspaces,
            OsStr::new(&workspace.connected_workspace_identity),
        )?;
        for member in &workspace.members {
            ensure_outside_repository(&member.root, &workspace_directory)?;
        }
        Ok(workspace_directory)
    }
    #[cfg(not(unix))]
    {
        let _ = (state_root, workspace);
        Err(())
    }
}

fn resolve_current_worktree_root() -> Result<PathBuf, ()> {
    let mut current = std::fs::canonicalize(std::env::current_dir().map_err(|_| ())?)
        .map_err(|_| ())?;
    loop {
        let marker = current.join(".git");
        match std::fs::symlink_metadata(&marker) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(()),
            Ok(metadata) if metadata.is_dir() || metadata.is_file() => return Ok(current),
            Ok(_) => return Err(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(()),
        }
        if !current.pop() {
            return Err(());
        }
    }
}
