const MCP_CATALOG_FILE: &str = "mcp-catalog-v1.json";
const MCP_CONNECTED_WORKSPACES_FILE: &str = "mcp-connected-workspaces-v1.json";
const WORKSPACE_MANIFEST_FILE: &str = "connected-workspace.toml";
const MCP_CATALOG_BYTES: usize = 64 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct McpCatalogDocument {
    schema_version: u8,
    repositories: Vec<McpCatalogEntry>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct McpCatalogEntry {
    repository_id: String,
    root: String,
    database: String,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct ConnectedWorkspaceCatalogDocument {
    schema_version: u8,
    workspaces: Vec<ConnectedWorkspaceEntry>,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct ConnectedWorkspaceEntry {
    name: String,
    connected_workspace_id: String,
    members: Vec<ConnectedWorkspaceMember>,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct ConnectedWorkspaceMember {
    repository_id: String,
    source_slot_id: String,
    root: PathBuf,
}

#[derive(Clone)]
struct WorkspaceServiceSelection {
    connected_workspace_id: String,
    source_slot_id: String,
}

struct RegisteredMcpRepository {
    repository_identity: String,
    root: PathBuf,
    database: PathBuf,
    graph_workspace: GraphWorkspaceContext,
    workspace: Option<WorkspaceServiceSelection>,
}

fn read_mcp_catalog(state_dir: Option<&Path>) -> Result<Vec<RegisteredMcpRepository>, ()> {
    let state_root = match state_dir {
        Some(path) => canonical_path_with_uncreated_suffix(path)?,
        None => default_onboard_state_root()?,
    };
    let product_root = state_root.join(ONBOARD_STATE_PRODUCT_DIRECTORY);
    let document = match read_optional_catalog_file(&product_root.join(MCP_CATALOG_FILE))? {
        Some(bytes) => serde_json::from_slice::<McpCatalogDocument>(&bytes).map_err(|_| ())?,
        None => McpCatalogDocument { schema_version: 1, repositories: Vec::new() },
    };
    if document.schema_version != 1
        || document.repositories.len() > repowitness_mcp::MAX_MCP_REGISTERED_REPOSITORIES
    { return Err(()); }
    let expected_repositories = state_root
        .join(ONBOARD_STATE_PRODUCT_DIRECTORY)
        .join(ONBOARD_STATE_REPOSITORIES_DIRECTORY);
    let mut identities = std::collections::BTreeSet::new();
    let mut workspace_ids = std::collections::BTreeSet::new();
    let mut source_slots = std::collections::BTreeSet::new();
    let mut roots = std::collections::BTreeSet::new();
    let mut databases = std::collections::BTreeSet::new();
    let mut repositories = Vec::with_capacity(document.repositories.len());
    for entry in document.repositories {
        RepositoryIdentityTextV1::decode(&entry.repository_id).map_err(|_| ())?;
        let root = absolute_catalog_path(&entry.root)?;
        let database = absolute_catalog_path(&entry.database)?;
        let expected_database = expected_repositories
            .join(&entry.repository_id)
            .join("index.sqlite3");
        if database != canonical_path_with_uncreated_suffix(&expected_database)?
            || !identities.insert(entry.repository_id.clone())
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
            workspace: None,
        });
    }
    let connected = read_connected_workspace_catalog(state_dir)?;
    for workspace in connected.workspaces {
        let workspace_id = workspace.connected_workspace_id.clone();
        ConnectedWorkspaceIdTextV1::decode(&workspace_id).map_err(|_| ())?;
        if !valid_workspace_name(&workspace.name) || !workspace_ids.insert(workspace_id.clone()) {
            return Err(());
        }
        let workspace_database = state_root
            .join(ONBOARD_STATE_PRODUCT_DIRECTORY)
            .join(ONBOARD_STATE_WORKSPACES_DIRECTORY)
            .join(&workspace_id)
            .join(ONBOARD_DATABASE_FILE);
        let manifest = state_root
            .join(ONBOARD_STATE_PRODUCT_DIRECTORY)
            .join(ONBOARD_STATE_WORKSPACES_DIRECTORY)
            .join(&workspace_id)
            .join(WORKSPACE_MANIFEST_FILE);
        if workspace.members.len() < 2
            || workspace.members.len() > repowitness_mcp::MAX_MCP_REGISTERED_REPOSITORIES
            || !canonical_path_with_uncreated_suffix(&workspace_database).is_ok()
            || !canonical_path_with_uncreated_suffix(&manifest).is_ok()
        { return Err(()); }
        let mut member_ids = std::collections::BTreeSet::new();
        let mut slot_ids = std::collections::BTreeSet::new();
        for member in workspace.members {
            RepositoryIdentityTextV1::decode(&member.repository_id).map_err(|_| ())?;
            SourceSlotIdTextV1::decode(&member.source_slot_id).map_err(|_| ())?;
            let root = absolute_catalog_path(member.root.to_str().ok_or(())?)?;
            if !member_ids.insert(member.repository_id.clone())
                || !slot_ids.insert(member.source_slot_id.clone())
                || !identities.insert(member.repository_id.clone())
                || !source_slots.insert(member.source_slot_id.clone())
                || roots.contains(&root)
            { return Err(()); }
            roots.insert(root.clone());
            databases.insert(workspace_database.clone());
            repositories.push(RegisteredMcpRepository {
                graph_workspace: GraphWorkspaceContext::ConnectedWorkspace {
                    connected_workspace: workspace_id.clone(),
                    source_slot: member.source_slot_id.clone(),
                },
                repository_identity: member.repository_id,
                root,
                database: workspace_database.clone(),
                workspace: Some(WorkspaceServiceSelection {
                    connected_workspace_id: workspace_id.clone(),
                    source_slot_id: member.source_slot_id,
                }),
            });
        }
    }
    if repositories.is_empty() || repositories.len() > repowitness_mcp::MAX_MCP_REGISTERED_REPOSITORIES {
        return Err(());
    }
    Ok(repositories)
}

fn absolute_catalog_path(value: &str) -> Result<PathBuf, ()> {
    let path = PathBuf::from(value);
    if value.is_empty() || value.contains('\0') || !path.is_absolute() {
        return Err(());
    }
    canonical_path_with_uncreated_suffix(&path)
}

fn build_mcp_catalog_services(
    repositories: Vec<RegisteredMcpRepository>,
    configuration: &ResolvedConfiguration,
    memory_actor: Option<&str>,
) -> Result<std::collections::BTreeMap<String, Arc<dyn RepositoryService>>, ()> {
    let mut services = std::collections::BTreeMap::new();
    for repository in repositories {
        let identity = repository.repository_identity;
        let service: Arc<dyn RepositoryService> = Arc::new(LocalMcpRepositoryService {
            root: repository.root,
            database: repository.database,
            repository_identity: identity.clone(),
            graph_workspace: repository.graph_workspace,
            workspace: repository.workspace,
            memory_actor: memory_actor.map(str::to_owned),
            configuration: configuration.clone(),
        });
        if services.insert(identity, service).is_some() {
            return Err(());
        }
    }
    Ok(services)
}

fn read_connected_workspace_catalog(
    state_dir: Option<&Path>,
) -> Result<ConnectedWorkspaceCatalogDocument, ()> {
    let state_root = match state_dir {
        Some(path) => canonical_path_with_uncreated_suffix(path)?,
        None => default_onboard_state_root()?,
    };
    let path = state_root
        .join(ONBOARD_STATE_PRODUCT_DIRECTORY)
        .join(MCP_CONNECTED_WORKSPACES_FILE);
    match read_optional_catalog_file(&path)? {
        Some(bytes) => {
            let catalog = serde_json::from_slice::<ConnectedWorkspaceCatalogDocument>(&bytes).map_err(|_| ())?;
            validate_connected_workspace_catalog(&catalog)?;
            Ok(catalog)
        }
        None => Ok(ConnectedWorkspaceCatalogDocument { schema_version: 1, workspaces: Vec::new() }),
    }
}

fn validate_connected_workspace_catalog(
    catalog: &ConnectedWorkspaceCatalogDocument,
) -> Result<(), ()> {
    if catalog.schema_version != 1
        || catalog.workspaces.len() > repowitness_mcp::MAX_MCP_REGISTERED_REPOSITORIES
    {
        return Err(());
    }
    let mut names = std::collections::BTreeSet::new();
    let mut workspace_ids = std::collections::BTreeSet::new();
    let mut repository_ids = std::collections::BTreeSet::new();
    let mut source_slots = std::collections::BTreeSet::new();
    let mut roots = std::collections::BTreeSet::new();
    let mut total_members = 0_usize;
    for workspace in &catalog.workspaces {
        if !valid_workspace_name(&workspace.name)
            || !names.insert(workspace.name.clone())
            || ConnectedWorkspaceIdTextV1::decode(&workspace.connected_workspace_id).is_err()
            || !workspace_ids.insert(workspace.connected_workspace_id.clone())
            || workspace.members.len() < WORKSPACE_MIN_MEMBERS
            || workspace.members.len() > repowitness_mcp::MAX_MCP_REGISTERED_REPOSITORIES
        {
            return Err(());
        }
        let mut member_ids = std::collections::BTreeSet::new();
        let mut member_slots = std::collections::BTreeSet::new();
        for member in &workspace.members {
            let root = member.root.to_str().ok_or(())?;
            if absolute_catalog_path(root)? != Path::new(root)
                || RepositoryIdentityTextV1::decode(&member.repository_id).is_err()
                || SourceSlotIdTextV1::decode(&member.source_slot_id).is_err()
                || !member_ids.insert(member.repository_id.clone())
                || !member_slots.insert(member.source_slot_id.clone())
                || !repository_ids.insert(member.repository_id.clone())
                || !source_slots.insert(member.source_slot_id.clone())
                || !roots.insert(root.to_owned())
            {
                return Err(());
            }
        }
        total_members = total_members.checked_add(workspace.members.len()).ok_or(())?;
        if total_members > repowitness_mcp::MAX_MCP_REGISTERED_REPOSITORIES {
            return Err(());
        }
    }
    Ok(())
}

fn read_optional_catalog_file(path: &Path) -> Result<Option<Vec<u8>>, ()> {
    match fs::symlink_metadata(path) {
        Ok(_) => read_bounded_regular_file(path, MCP_CATALOG_BYTES)
            .map(Some)
            .map_err(|_| ()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(()),
    }
}

fn register_mcp_connected_workspace(
    state_dir: Option<&Path>,
    workspace: ConnectedWorkspaceEntry,
) -> Result<(), ()> {
    let state_root = match state_dir {
        Some(path) => canonical_path_with_uncreated_suffix(path)?,
        None => default_onboard_state_root()?,
    };
    let product = state_root.join(ONBOARD_STATE_PRODUCT_DIRECTORY);
    let path = product.join(MCP_CONNECTED_WORKSPACES_FILE);
    let mut catalog = read_connected_workspace_catalog(state_dir)?;
    if catalog.workspaces.iter().any(|entry| entry.name == workspace.name) { return Err(()); }
    catalog.workspaces.push(workspace);
    let encoded = serde_json::to_vec(&catalog).map_err(|_| ())?;
    if encoded.len() > MCP_CATALOG_BYTES { return Err(()); }
    let temporary = path.with_extension("json.tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| ())?;
    file.write_all(&encoded).map_err(|_| ())?;
    fs::rename(temporary, path).map_err(|_| ())
}

fn remove_mcp_connected_workspace(
    state_dir: Option<&Path>,
    name: &str,
) -> Result<bool, ()> {
    let state_root = match state_dir {
        Some(path) => canonical_path_with_uncreated_suffix(path)?,
        None => default_onboard_state_root()?,
    };
    let path = state_root
        .join(ONBOARD_STATE_PRODUCT_DIRECTORY)
        .join(MCP_CONNECTED_WORKSPACES_FILE);
    let mut catalog = read_connected_workspace_catalog(state_dir)?;
    let before = catalog.workspaces.len();
    catalog.workspaces.retain(|entry| entry.name != name);
    if catalog.workspaces.len() == before { return Ok(false); }
    let encoded = serde_json::to_vec(&catalog).map_err(|_| ())?;
    let temporary = path.with_extension("json.tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| ())?;
    file.write_all(&encoded).map_err(|_| ())?;
    fs::rename(temporary, path).map_err(|_| ())?;
    Ok(true)
}

fn load_mcp_catalog(
    state_dir: Option<&Path>,
    configuration: &ResolvedConfiguration,
    memory_actor: Option<&str>,
    refresh_connected: bool,
) -> Result<(McpRepositoryCatalog, Option<String>), ()> {
    if refresh_connected {
        refresh_connected_workspaces(state_dir, configuration)?;
    }
    let repositories = read_mcp_catalog(state_dir)?;
    let default_repository_id = catalog_default_repository_id(&repositories);
    let services = build_mcp_catalog_services(repositories, configuration, memory_actor)?;
    Ok((services, default_repository_id))
}

fn refresh_connected_workspaces(
    state_dir: Option<&Path>,
    configuration: &ResolvedConfiguration,
) -> Result<(), ()> {
    let state_root = match state_dir {
        Some(path) => canonical_path_with_uncreated_suffix(path)?,
        None => default_onboard_state_root()?,
    };
    let catalog = read_connected_workspace_catalog(state_dir)?;
    let current = std::env::current_dir()
        .ok()
        .and_then(|path| fs::canonicalize(path).ok());
    let matching = current.as_ref().map_or(0, |current| {
        catalog
            .workspaces
            .iter()
            .filter(|workspace| {
                workspace
                    .members
                    .iter()
                    .any(|member| member.root == *current || current.starts_with(&member.root))
            })
            .count()
    });
    if matching > 1 {
        return Err(());
    }
    let Some(current) = current else {
        return Ok(());
    };
    for workspace in catalog.workspaces.into_iter().filter(|workspace| {
        workspace
            .members
            .iter()
            .any(|member| member.root == current || current.starts_with(&member.root))
    }) {
        let directory = state_root
            .join(ONBOARD_STATE_PRODUCT_DIRECTORY)
            .join(ONBOARD_STATE_WORKSPACES_DIRECTORY)
            .join(&workspace.connected_workspace_id);
        let manifest_path = directory.join(WORKSPACE_MANIFEST_FILE);
        let database = directory.join(ONBOARD_DATABASE_FILE);
        let (contents, parent) = read_bounded_regular_file_with_parent(
            &manifest_path,
            repowitness_local::MAX_LOCAL_CONNECTED_WORKSPACE_MANIFEST_BYTES,
        ).map_err(|_| ())?;
        if contents.bytes() != workspace_manifest(&workspace.connected_workspace_id, &workspace.members).as_bytes() {
            return Err(());
        }
        let request = LocalConnectedWorkspaceIndexRequest::new(
            contents.bytes(),
            &parent,
            &database,
            configuration,
            unix_time_millis(),
        )
        .with_deadline(DEFAULT_LOCAL_CONNECTED_WORKSPACE_DEADLINE)
        .map_err(|_| ())?;
        index_local_connected_workspace(request, Arc::new(AtomicBool::new(false))).map_err(|_| ())?;
    }
    Ok(())
}

fn catalog_default_repository_id(
    repositories: &[RegisteredMcpRepository],
) -> Option<String> {
    let current = std::env::current_dir().ok().and_then(|path| fs::canonicalize(path).ok())?;
    if let Some(repository) = repositories
        .iter()
        .find(|repository| repository.root == current)
    {
        return Some(repository.repository_identity.clone());
    }
    let mut containing = repositories
        .iter()
        .filter(|repository| current.starts_with(&repository.root));
    let repository = containing.next()?;
    if containing.next().is_some() {
        return None;
    }
    Some(repository.repository_identity.clone())
}

fn register_mcp_catalog_repository(
    state_dir: Option<&Path>,
    repository_id: &str,
    root: &Path,
    database: &Path,
) -> Result<(), ()> {
    let state_root = match state_dir {
        Some(path) => canonical_path_with_uncreated_suffix(path)?,
        None => default_onboard_state_root()?,
    };
    let catalog_path = state_root
        .join(ONBOARD_STATE_PRODUCT_DIRECTORY)
        .join(MCP_CATALOG_FILE);
    let mut document = match fs::symlink_metadata(&catalog_path) {
        Ok(_) => {
            let bytes = read_bounded_regular_file(&catalog_path, MCP_CATALOG_BYTES)
                .map_err(|_| ())?;
            serde_json::from_slice::<McpCatalogDocument>(&bytes).map_err(|_| ())?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => McpCatalogDocument {
            schema_version: 1,
            repositories: Vec::new(),
        },
        Err(_) => return Err(()),
    };
    if document.schema_version != 1
        || document.repositories.len() > repowitness_mcp::MAX_MCP_REGISTERED_REPOSITORIES
    {
        return Err(());
    }
    let root = fs::canonicalize(root).map_err(|_| ())?;
    let database = canonical_path_with_uncreated_suffix(database)?;
    if let Some(entry) = document
        .repositories
        .iter()
        .find(|entry| entry.repository_id == repository_id)
    {
        if absolute_catalog_path(&entry.root)? == root
            && absolute_catalog_path(&entry.database)? == database
        {
            return Ok(());
        }
        return Err(());
    }
    if document
        .repositories
        .iter()
        .any(|entry| absolute_catalog_path(&entry.root).ok().as_ref() == Some(&root))
    {
        return Err(());
    }
    if document.repositories.len() >= repowitness_mcp::MAX_MCP_REGISTERED_REPOSITORIES {
        return Err(());
    }
    document.repositories.push(McpCatalogEntry {
        repository_id: repository_id.to_owned(),
        root: root.to_str().ok_or(())?.to_owned(),
        database: database.to_str().ok_or(())?.to_owned(),
    });
    let encoded = serde_json::to_vec(&document).map_err(|_| ())?;
    if encoded.len() > MCP_CATALOG_BYTES {
        return Err(());
    }
    let temporary = catalog_path.with_extension("json.tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| ())?;
    file.write_all(&encoded).map_err(|_| ())?;
    fs::rename(temporary, catalog_path).map_err(|_| ())
}
