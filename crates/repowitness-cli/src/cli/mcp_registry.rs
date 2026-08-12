const MCP_CATALOG_FILE: &str = "mcp-catalog-v1.json";
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

struct RegisteredMcpRepository {
    repository_identity: String,
    root: PathBuf,
    database: PathBuf,
    graph_workspace: GraphWorkspaceContext,
}

fn read_mcp_catalog(state_dir: Option<&Path>) -> Result<Vec<RegisteredMcpRepository>, ()> {
    let state_root = match state_dir {
        Some(path) => canonical_path_with_uncreated_suffix(path)?,
        None => default_onboard_state_root()?,
    };
    let catalog_path = state_root
        .join(ONBOARD_STATE_PRODUCT_DIRECTORY)
        .join(MCP_CATALOG_FILE);
    let bytes = read_bounded_regular_file(&catalog_path, MCP_CATALOG_BYTES).map_err(|_| ())?;
    let document = serde_json::from_slice::<McpCatalogDocument>(&bytes).map_err(|_| ())?;
    if document.schema_version != 1
        || document.repositories.is_empty()
        || document.repositories.len() > repowitness_mcp::MAX_MCP_REGISTERED_REPOSITORIES
    {
        return Err(());
    }
    let expected_repositories = state_root
        .join(ONBOARD_STATE_PRODUCT_DIRECTORY)
        .join(ONBOARD_STATE_REPOSITORIES_DIRECTORY);
    let mut identities = std::collections::BTreeSet::new();
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
        });
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
            memory_actor: memory_actor.map(str::to_owned),
            configuration: configuration.clone(),
        });
        if services.insert(identity, service).is_some() {
            return Err(());
        }
    }
    Ok(services)
}

fn load_mcp_catalog(
    state_dir: Option<&Path>,
    configuration: &ResolvedConfiguration,
    memory_actor: Option<&str>,
) -> Result<(McpRepositoryCatalog, Option<String>), ()> {
    let repositories = read_mcp_catalog(state_dir)?;
    let default_repository_id = catalog_default_repository_id(&repositories);
    let services = build_mcp_catalog_services(repositories, configuration, memory_actor)?;
    Ok((services, default_repository_id))
}

fn catalog_default_repository_id(
    repositories: &[RegisteredMcpRepository],
) -> Option<String> {
    let current = std::env::current_dir().ok().and_then(|path| fs::canonicalize(path).ok())?;
    repositories
        .iter()
        .find(|repository| repository.root == current)
        .map(|repository| repository.repository_identity.clone())
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
    fs::write(&temporary, encoded).map_err(|_| ())?;
    fs::rename(temporary, catalog_path).map_err(|_| ())
}
