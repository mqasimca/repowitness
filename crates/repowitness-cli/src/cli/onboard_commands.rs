use std::{env, fs, path::Component};

#[cfg(unix)]
use std::io;

#[cfg(unix)]
use cap_fs_ext::DirExt;
#[cfg(unix)]
use cap_std::{
    ambient_authority,
    fs::{Dir, Permissions, PermissionsExt as CapPermissionsExt},
};

const ONBOARD_HELP: &str = concat!(
    "Index one explicit repository into a private local state directory.\n\n",
    "Usage:\n",
    "  repowitness onboard --root <path> [--state-dir <path>] [--repository-id <id>]\n",
    "      [--full] [--no-scip] [--scip-go <path>]\n\n",
    "The command never searches parent or sibling repositories or writes repository\n",
    "configuration. It builds no native graph by default, then automatically adds\n",
    "Go SCIP relationships when a root go.mod and scip-go are available.\n",
    "If --repository-id is omitted, it uses operating-system secure randomness.\n",
    "The database is stored under the\n",
    "documented private-state convention named by that opaque identity. Use\n",
    "--full when graph reads are needed, --no-scip to skip Go enrichment, or\n",
    "--scip-go to select the producer. Normal `index` remains source-only.\n",
);

const ONBOARD_STATE_PRODUCT_DIRECTORY: &str = "repowitness";
const ONBOARD_STATE_REPOSITORIES_DIRECTORY: &str = "repositories";
const ONBOARD_STATE_WORKSPACES_DIRECTORY: &str = "workspaces";
const ONBOARD_DATABASE_FILE: &str = "index.sqlite3";

struct PreparedOnboardDatabase {
    database: PathBuf,
}

struct OnboardInvocation {
    root: PathBuf,
    state_dir: Option<PathBuf>,
    repository_identity: Option<String>,
    build_graph: bool,
    no_scip: bool,
    scip_go: PathBuf,
}

#[derive(Clone, Copy)]
enum OnboardScipStatus {
    NotApplicable,
    Skipped(&'static str),
    Imported(repowitness_local::LocalScipOverlayImportResult),
    Failed(&'static str),
}

trait OnboardStateDirectory {
    fn prepare_database(
        &self,
        repository_root: &Path,
        state_dir: Option<&Path>,
        repository_identity: &str,
    ) -> Result<PreparedOnboardDatabase, ()>;
    fn register_catalog(
        &self,
        _repository_root: &Path,
        _state_dir: Option<&Path>,
        _repository_identity: &str,
        _database: &Path,
    ) -> Result<(), ()> {
        Ok(())
    }
}

struct PrivateOnboardStateDirectory;

impl OnboardStateDirectory for PrivateOnboardStateDirectory {
    fn prepare_database(
        &self,
        repository_root: &Path,
        state_dir: Option<&Path>,
        repository_identity: &str,
    ) -> Result<PreparedOnboardDatabase, ()> {
        let state_root = match state_dir {
            Some(path) => path.to_path_buf(),
            None => default_onboard_state_root()?,
        };
        let state_root = canonical_path_with_uncreated_suffix(&state_root)?;
        let repository_state = state_root
            .join(ONBOARD_STATE_PRODUCT_DIRECTORY)
            .join(ONBOARD_STATE_REPOSITORIES_DIRECTORY)
            .join(repository_identity);
        ensure_outside_repository(repository_root, &repository_state)?;
        #[cfg(unix)]
        let _state_directory = {
            let state_root_directory = open_private_state_root(&state_root)?;
            let product = open_or_create_private_directory(
                &state_root_directory,
                OsStr::new(ONBOARD_STATE_PRODUCT_DIRECTORY),
            )?;
            let repositories = open_or_create_private_directory(
                &product,
                OsStr::new(ONBOARD_STATE_REPOSITORIES_DIRECTORY),
            )?;
            open_or_create_private_directory(&repositories, OsStr::new(repository_identity))?
        };
        #[cfg(not(unix))]
        {
            let _ = repository_state;
            Err(())
        }
        #[cfg(unix)]
        {
            ensure_outside_repository(repository_root, &repository_state)?;
            Ok(PreparedOnboardDatabase {
                database: repository_state.join(ONBOARD_DATABASE_FILE),
            })
        }
    }

    fn register_catalog(
        &self,
        repository_root: &Path,
        state_dir: Option<&Path>,
        repository_identity: &str,
        database: &Path,
    ) -> Result<(), ()> {
        register_mcp_catalog_repository(
            state_dir,
            repository_identity,
            repository_root,
            database,
        )
    }
}

fn run_onboard(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    inspector: &impl RepositoryPathInspector,
    indexer: &impl RepositoryIndexer,
    identity_generator: &impl IdentityGenerator,
    state_directory: &impl OnboardStateDirectory,
) -> u8 {
    let arguments = args.take(MAX_ONBOARD_ARGUMENTS + 1).collect::<Vec<_>>();
    if arguments.len() > MAX_ONBOARD_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: onboard received too many arguments; use onboard --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, ONBOARD_HELP);
    }
    let invocation = match parse_onboard_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let repository_identity = match &invocation.repository_identity {
        Some(identity) => identity.clone(),
        None => match identity_generator.generate(LocalIdentityKind::Repository) {
            Ok(identity) => identity,
            Err(_) => {
                return emit_error(stderr, EXIT_SOFTWARE, "error: onboarding identity generation failed\n");
            }
        },
    };
    if RepositoryIdentityTextV1::decode(&repository_identity).is_err() {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: onboard repository identity must be canonical\n",
        );
    }
    if inspector.inspect(&invocation.root).is_err() {
        return emit_error(stderr, EXIT_SOFTWARE, "error: onboarding root is unavailable\n");
    }
    let configuration = match resolve_configuration(&[]) {
        Ok(configuration) => configuration,
        Err(_) => return emit_error(stderr, EXIT_SOFTWARE, "error: onboarding configuration failed\n"),
    };
    let prepared_database = match state_directory.prepare_database(
        &invocation.root,
        invocation.state_dir.as_deref(),
        &repository_identity,
    ) {
        Ok(database) => database,
        Err(()) => {
            return emit_error(stderr, EXIT_SOFTWARE, "error: private onboarding state is unavailable\n");
        }
    };
    let repository_root = invocation.root.clone();
    let state_dir = invocation.state_dir.clone();
    let build_graph = invocation.build_graph;
    let index_report = match indexer.index(
        &IndexInvocation {
            repository_root,
            database: prepared_database.database.clone(),
            repository_identity: OsString::from(&repository_identity),
            build_graph,
        },
        &configuration,
    ) {
        Ok(report) => report,
        Err(_) => return emit_error(stderr, EXIT_SOFTWARE, "error: onboarding indexing failed\n"),
    };
    let scip_status = onboard_scip_status(
        &invocation,
        &prepared_database.database,
        &repository_identity,
        &index_report,
    );
    if let OnboardScipStatus::Failed(reason) = scip_status {
        let _ = writeln!(
            stderr,
            "warning: source index completed but automatic Go SCIP enrichment failed (reason={reason})"
        );
    }
    if state_directory
        .register_catalog(
            &invocation.root,
            state_dir.as_deref(),
            &repository_identity,
            &prepared_database.database,
        )
        .is_err()
    {
        return emit_error(stderr, EXIT_SOFTWARE, "error: onboarding catalog registration failed\n");
    }
    emit_onboard_report(
        stdout,
        &repository_identity,
        index_report,
        build_graph,
        scip_status,
    )
}

fn onboard_scip_status(
    invocation: &OnboardInvocation,
    database: &Path,
    repository_identity: &str,
    report: &CliIndexReport,
) -> OnboardScipStatus {
    if report.indexed_go_files == 0 {
        return OnboardScipStatus::NotApplicable;
    }
    if invocation.no_scip {
        return OnboardScipStatus::Skipped("disabled");
    }
    if !scip_go_root_has_regular_go_mod(&invocation.root) {
        return OnboardScipStatus::Skipped("root_module_required");
    }
    if !scip_go_producer_available(&invocation.scip_go) {
        return OnboardScipStatus::Skipped("producer_unavailable");
    }
    let Ok((connected_workspace, source_slot)) = resolve_scip_go_import_workspace(
        Some(repository_identity.to_owned()),
        None,
        None,
    ) else {
        return OnboardScipStatus::Failed("workspace_view_invalid");
    };
    let invocation = ScipGoImportInvocation {
        import: ScipImportInvocation {
            database: database.to_owned(),
            root: invocation.root.clone(),
            scip_file: PathBuf::new(),
            connected_workspace,
            source_slot,
            workspace_view: None,
            timeout: DEFAULT_SCIP_GO_IMPORT_TIMEOUT,
        },
        scip_go: invocation.scip_go.clone(),
        producer_timeout: DEFAULT_SCIP_GO_PRODUCER_TIMEOUT,
        skip_implementations: false,
        skip_tests: false,
    };
    match produce_and_import_scip_go(&invocation) {
        Ok(result) => OnboardScipStatus::Imported(result),
        Err(ScipGoProductionError::TemporaryOutput | ScipGoProductionError::Producer) => {
            OnboardScipStatus::Failed("producer_failed")
        }
        Err(ScipGoProductionError::Import(ScipImportOverlayError::InvalidWorkspaceView)) => {
            OnboardScipStatus::Failed("workspace_view_invalid")
        }
        Err(ScipGoProductionError::Import(ScipImportOverlayError::Import(_))) => {
            OnboardScipStatus::Failed("import_failed")
        }
    }
}

fn parse_onboard_arguments(arguments: &[OsString]) -> Result<OnboardInvocation, &'static str> {
    let mut root = None;
    let mut state_dir = None;
    let mut repository_identity = None;
    let mut build_graph = false;
    let mut no_scip = false;
    let mut scip_go = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        index += 1;
        if option == OsStr::new("--full") {
            if build_graph {
                return Err("error: onboard accepts --full only once\n");
            }
            build_graph = true;
            continue;
        }
        if option == OsStr::new("--no-scip") {
            if no_scip {
                return Err("error: onboard accepts --no-scip only once\n");
            }
            no_scip = true;
            continue;
        }
        let value = arguments
            .get(index)
            .ok_or("error: onboard options require values; use onboard --help\n")?;
        index += 1;
        if option == OsStr::new("--root") {
            if root.replace(PathBuf::from(value)).is_some() {
                return Err("error: onboard accepts --root only once\n");
            }
        } else if option == OsStr::new("--state-dir") {
            if state_dir.replace(PathBuf::from(value)).is_some() {
                return Err("error: onboard accepts --state-dir only once\n");
            }
        } else if option == OsStr::new("--repository-id") {
            let identity = value
                .to_str()
                .ok_or("error: onboard repository identity must be valid UTF-8\n")?;
            if repository_identity.replace(identity.to_owned()).is_some() {
                return Err("error: onboard accepts --repository-id only once\n");
            }
        } else if option == OsStr::new("--scip-go") {
            if value.is_empty() || scip_go.replace(PathBuf::from(value)).is_some() {
                return Err("error: onboard accepts one non-empty --scip-go\n");
            }
        } else {
            return Err("error: onboard accepts only --root, --state-dir, --repository-id, --full, --no-scip, and --scip-go\n");
        }
    }
    let root = root.ok_or("error: onboard requires --root; use onboard --help\n")?;
    if root.as_os_str().is_empty() {
        return Err("error: onboard root must not be empty\n");
    }
    if state_dir.as_ref().is_some_and(|path| path.as_os_str().is_empty()) {
        return Err("error: onboard state directory must not be empty\n");
    }
    Ok(OnboardInvocation {
        root,
        state_dir,
        repository_identity,
        build_graph,
        no_scip,
        scip_go: scip_go.unwrap_or_else(|| PathBuf::from("scip-go")),
    })
}

fn default_onboard_state_root() -> Result<PathBuf, ()> {
    #[cfg(target_os = "windows")]
    let root = env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let root = env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let root = env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")));
    #[cfg(not(any(unix, target_os = "windows")))]
    let root: Option<PathBuf> = None;
    root.filter(|path| !path.as_os_str().is_empty() && path.is_absolute())
        .ok_or(())
}

fn ensure_outside_repository(repository_root: &Path, target: &Path) -> Result<(), ()> {
    let canonical_repository = fs::canonicalize(repository_root).map_err(|_| ())?;
    let canonical_target = canonical_path_with_uncreated_suffix(target)?;
    if canonical_target.starts_with(canonical_repository) {
        Err(())
    } else {
        Ok(())
    }
}

fn canonical_path_with_uncreated_suffix(path: &Path) -> Result<PathBuf, ()> {
    let mut absolute = if path.is_absolute() {
        PathBuf::new()
    } else {
        env::current_dir().map_err(|_| ())?
    };
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => absolute.push(prefix.as_os_str()),
            Component::RootDir => absolute.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !absolute.pop() {
                    return Err(());
                }
            }
            Component::Normal(component) => absolute.push(component),
        }
    }
    let mut suffix = Vec::new();
    while fs::symlink_metadata(&absolute).is_err() {
        let Some(component) = absolute.file_name().map(OsString::from) else {
            return Err(());
        };
        suffix.push(component);
        if !absolute.pop() {
            return Err(());
        }
    }
    let mut canonical = fs::canonicalize(absolute).map_err(|_| ())?;
    for component in suffix.into_iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

#[cfg(unix)]
fn prepare_private_state_directory(
    state_root: &Path,
    product: &str,
    child: &str,
    identity: &str,
) -> Result<PathBuf, ()> {
    let state_root_directory = open_private_state_root(state_root)?;
    let product_directory = open_or_create_private_directory(
        &state_root_directory,
        OsStr::new(product),
    )?;
    let child_directory = open_or_create_private_directory(
        &product_directory,
        OsStr::new(child),
    )?;
    open_or_create_private_directory(&child_directory, OsStr::new(identity))?;
    Ok(state_root.join(product).join(child).join(identity))
}

#[cfg(not(unix))]
fn prepare_private_state_directory(
    _state_root: &Path,
    _product: &str,
    _child: &str,
    _identity: &str,
) -> Result<PathBuf, ()> {
    Err(())
}

#[cfg(unix)]
fn open_private_state_root(path: &Path) -> Result<Dir, ()> {
    if !path.is_absolute() {
        return Err(());
    }
    let mut directory = Dir::open_ambient_dir("/", ambient_authority()).map_err(|_| ())?;
    let mut contains_normal_component = false;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                contains_normal_component = true;
                ensure_private_parent(&directory)?;
                directory = open_or_create_directory_nofollow(&directory, name)?;
            }
            Component::Prefix(_) | Component::CurDir | Component::ParentDir => return Err(()),
        }
    }
    if !contains_normal_component {
        return Err(());
    }
    set_private_directory_permissions(&directory)?;
    Ok(directory)
}

#[cfg(unix)]
fn open_or_create_private_directory(parent: &Dir, name: &OsStr) -> Result<Dir, ()> {
    ensure_private_parent(parent)?;
    let directory = open_or_create_directory_nofollow(parent, name)?;
    set_private_directory_permissions(&directory)?;
    Ok(directory)
}

#[cfg(unix)]
fn open_or_create_directory_nofollow(parent: &Dir, name: &OsStr) -> Result<Dir, ()> {
    match parent.create_dir(name) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(()),
    }
    parent.open_dir_nofollow(name).map_err(|_| ())
}

#[cfg(unix)]
fn ensure_private_parent(directory: &Dir) -> Result<(), ()> {
    let mode = directory
        .metadata(".")
        .map_err(|_| ())?
        .permissions()
        .mode();
    if mode & 0o022 == 0 || mode & 0o1000 != 0 {
        Ok(())
    } else {
        Err(())
    }
}

#[cfg(unix)]
fn set_private_directory_permissions(directory: &Dir) -> Result<(), ()> {
    directory
        .set_permissions(".", Permissions::from_mode(0o700))
        .map_err(|_| ())
}

fn emit_onboard_report(
    writer: &mut impl Write,
    repository_identity: &str,
    report: CliIndexReport,
    build_graph: bool,
    scip_status: OnboardScipStatus,
) -> u8 {
    if !index_report_is_consistent(&report) {
        return EXIT_SOFTWARE;
    }
    let result = writeln!(writer, "status=ok")
        .and_then(|()| writeln!(writer, "operation=onboard"))
        .and_then(|()| writeln!(writer, "repository_id={repository_identity}"))
        .and_then(|()| {
            writeln!(
                writer,
                "index_profile={}",
                if build_graph { "full" } else { "source-only" }
            )
        })
        .and_then(|()| {
            writeln!(
                writer,
                "state_directory_convention={ONBOARD_STATE_PRODUCT_DIRECTORY}/{ONBOARD_STATE_REPOSITORIES_DIRECTORY}/<repository-id>/{ONBOARD_DATABASE_FILE}",
            )
        })
        .and_then(|()| writeln!(writer, "generation_activated=true"))
        .and_then(|()| writeln!(writer, "generation={}", report.generation))
        .and_then(|()| writeln!(writer, "source_epoch={}", report.source_epoch))
        .and_then(|()| writeln!(writer, "repository_paths={}", report.discovered_paths))
        .and_then(|()| emit_onboard_scip_report(writer, scip_status));
    if result.is_ok() { EXIT_SUCCESS } else { EXIT_IO }
}

fn emit_onboard_scip_report(
    writer: &mut impl Write,
    status: OnboardScipStatus,
) -> std::io::Result<()> {
    match status {
        OnboardScipStatus::NotApplicable => {
            writeln!(writer, "scip_status=not_applicable")?;
            writeln!(writer, "scip_reason=no_go_sources")
        }
        OnboardScipStatus::Skipped(reason) => {
            writeln!(writer, "scip_status=skipped")?;
            writeln!(writer, "scip_reason={reason}")
        }
        OnboardScipStatus::Failed(reason) => {
            writeln!(writer, "scip_status=failed")?;
            writeln!(writer, "scip_reason={reason}")
        }
        OnboardScipStatus::Imported(result) => {
            let overlay = result.overlay();
            writeln!(writer, "scip_status=imported")?;
            writeln!(writer, "scip_documents={}", overlay.documents())?;
            writeln!(writer, "scip_occurrences={}", overlay.occurrences())?;
            writeln!(writer, "scip_relationships={}", overlay.relationships())?;
            writeln!(
                writer,
                "scip_ignored_external_documents={}",
                result.ignored_external_documents()
            )
        }
    }
}
