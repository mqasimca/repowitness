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
    "  repowitness onboard --root <path> [--state-dir <path>] [--repository-id <id>]\n\n",
    "The command never searches parent or sibling repositories, writes repository\n",
    "configuration, or creates a root registry. If --repository-id is omitted, it\n",
    "uses operating-system secure randomness. The database is stored under the\n",
    "documented private-state convention named by that opaque identity.\n",
);

const ONBOARD_STATE_PRODUCT_DIRECTORY: &str = "repowitness";
const ONBOARD_STATE_REPOSITORIES_DIRECTORY: &str = "repositories";
const ONBOARD_DATABASE_FILE: &str = "index.sqlite3";

struct PreparedOnboardDatabase {
    database: PathBuf,
}

struct OnboardInvocation {
    root: PathBuf,
    state_dir: Option<PathBuf>,
    repository_identity: Option<String>,
}

trait OnboardStateDirectory {
    fn prepare_database(
        &self,
        repository_root: &Path,
        state_dir: Option<&Path>,
        repository_identity: &str,
    ) -> Result<PreparedOnboardDatabase, ()>;

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
    let repository_identity = match invocation.repository_identity {
        Some(identity) => identity,
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
    let index_report = match indexer.index(
        &IndexInvocation {
            repository_root: invocation.root,
            database: prepared_database.database.clone(),
            repository_identity: OsString::from(&repository_identity),
        },
        &configuration,
    ) {
        Ok(report) => report,
        Err(_) => return emit_error(stderr, EXIT_SOFTWARE, "error: onboarding indexing failed\n"),
    };
    emit_onboard_report(stdout, &repository_identity, index_report)
}

fn parse_onboard_arguments(arguments: &[OsString]) -> Result<OnboardInvocation, &'static str> {
    let mut root = None;
    let mut state_dir = None;
    let mut repository_identity = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        index += 1;
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
        } else {
            return Err("error: onboard accepts only --root, --state-dir, and --repository-id\n");
        }
    }
    let root = root.ok_or("error: onboard requires --root; use onboard --help\n")?;
    if root.as_os_str().is_empty() {
        return Err("error: onboard root must not be empty\n");
    }
    if state_dir.as_ref().is_some_and(|path| path.as_os_str().is_empty()) {
        return Err("error: onboard state directory must not be empty\n");
    }
    Ok(OnboardInvocation { root, state_dir, repository_identity })
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
                "state_directory_convention={ONBOARD_STATE_PRODUCT_DIRECTORY}/{ONBOARD_STATE_REPOSITORIES_DIRECTORY}/<repository-id>/{ONBOARD_DATABASE_FILE}",
            )
        })
        .and_then(|()| writeln!(writer, "generation_activated=true"))
        .and_then(|()| writeln!(writer, "generation={}", report.generation))
        .and_then(|()| writeln!(writer, "source_epoch={}", report.source_epoch))
        .and_then(|()| writeln!(writer, "repository_paths={}", report.discovered_paths));
    if result.is_ok() { EXIT_SUCCESS } else { EXIT_IO }
}
