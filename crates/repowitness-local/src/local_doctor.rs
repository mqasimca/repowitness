use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use repowitness_application::{
    CONFIGURATION_RESOLVER_VERSION, CONFIGURATION_SCHEMA_VERSION, MAX_CONFIGURATION_CONTEXT_BYTES,
    MAX_CONFIGURATION_GRAPH_DEPTH, MAX_CONFIGURATION_GRAPH_RESULTS,
    MAX_CONFIGURATION_QUERY_RESULTS, MAX_CONFIGURATION_SOURCE_FILE_BYTES,
    MAX_CONFIGURATION_SOURCE_FILES, MAX_CONFIGURATION_WATCHER_POLL_INTERVAL_MS,
    MIN_CONFIGURATION_WATCHER_POLL_INTERVAL_MS, McpToolProfile, ResolvedConfiguration,
    ResolvedPreference, SourceLanguage,
};

use crate::{
    contained_source::{FileIdentity, file_has_single_link},
    sqlite::{inspect_sqlite_environment, validate_database_read_only},
};

const COMPILED_LANGUAGE_ADAPTER_COUNT: u8 = 5;
const MAX_DOCTOR_ANCESTORS: usize = 256;
const MAX_DOCTOR_PATH_BYTES: usize = 32 * 1024;

/// Result of one stable doctor check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DoctorCheckStatus {
    /// The invariant was satisfied.
    Ok,
    /// The setting is valid but materially narrows or omits behavior.
    Warning,
    /// A required invariant failed.
    Error,
    /// A prerequisite or explicit target was absent.
    NotRun,
}

impl DoctorCheckStatus {
    /// Returns the stable terminal spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::NotRun => "not_run",
        }
    }
}

/// Aggregate doctor outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DoctorOverallStatus {
    /// Every requested check passed.
    Ok,
    /// No required invariant failed, but one or more warnings remain.
    Warning,
    /// At least one required invariant failed.
    Error,
}

impl DoctorOverallStatus {
    /// Returns the stable terminal spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// State of the explicitly requested database target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DoctorDatabaseState {
    /// Repository and database targets were not supplied.
    NotRequested,
    /// The database is absent but its parent capability is usable.
    Missing,
    /// An existing regular database file was inspected.
    Existing,
    /// The target could not be safely resolved or opened.
    Unavailable,
}

impl DoctorDatabaseState {
    /// Returns the stable terminal spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotRequested => "not_requested",
            Self::Missing => "missing",
            Self::Existing => "existing",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Explicit paired repository and database targets for read-only diagnostics.
#[derive(Clone, Copy)]
pub struct LocalDoctorTargets<'a> {
    repository: &'a Path,
    database: &'a Path,
}

impl<'a> LocalDoctorTargets<'a> {
    /// Pairs the exact repository and database targets authorized by the caller.
    #[must_use]
    pub const fn new(repository: &'a Path, database: &'a Path) -> Self {
        Self {
            repository,
            database,
        }
    }
}

impl fmt::Debug for LocalDoctorTargets<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalDoctorTargets")
            .field("repository", &"<redacted-path>")
            .field("database", &"<redacted-path>")
            .finish()
    }
}

/// Complete path-free result of the bounded local doctor operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalDoctorReport {
    configuration: DoctorCheckStatus,
    language_adapters: DoctorCheckStatus,
    mcp_tool_profile: DoctorCheckStatus,
    incompatible_settings: DoctorCheckStatus,
    repository_capability: DoctorCheckStatus,
    database_placement: DoctorCheckStatus,
    database_capability: DoctorCheckStatus,
    sqlite_runtime: DoctorCheckStatus,
    sqlite_compile_options: DoctorCheckStatus,
    database_schema: DoctorCheckStatus,
    database_state: DoctorDatabaseState,
    enabled_language_adapter_count: u8,
    sqlite_runtime_version_number: Option<i32>,
}

impl LocalDoctorReport {
    /// Returns the aggregate outcome.
    #[must_use]
    pub fn status(self) -> DoctorOverallStatus {
        if self.error_count() > 0 {
            DoctorOverallStatus::Error
        } else if self.warning_count() > 0 {
            DoctorOverallStatus::Warning
        } else {
            DoctorOverallStatus::Ok
        }
    }

    /// Returns the number of failed required checks.
    #[must_use]
    pub fn error_count(self) -> u8 {
        self.checks()
            .into_iter()
            .filter(|status| *status == DoctorCheckStatus::Error)
            .count()
            .try_into()
            .expect("fixed doctor check count fits u8")
    }

    /// Returns the number of bounded warnings.
    #[must_use]
    pub fn warning_count(self) -> u8 {
        let check_warnings = self
            .checks()
            .into_iter()
            .filter(|status| *status == DoctorCheckStatus::Warning)
            .count();
        let target_warning = usize::from(self.database_state == DoctorDatabaseState::NotRequested);
        let missing_warning = usize::from(self.database_state == DoctorDatabaseState::Missing);
        (check_warnings + target_warning + missing_warning)
            .try_into()
            .expect("fixed doctor warning count fits u8")
    }

    /// Returns whether target checks were explicitly requested.
    #[must_use]
    pub const fn target_checks_requested(self) -> bool {
        !matches!(self.database_state, DoctorDatabaseState::NotRequested)
    }

    /// Returns the resolved-configuration check.
    #[must_use]
    pub const fn configuration(self) -> DoctorCheckStatus {
        self.configuration
    }

    /// Returns the compiled-language-adapter check.
    #[must_use]
    pub const fn language_adapters(self) -> DoctorCheckStatus {
        self.language_adapters
    }

    /// Returns the enabled compiled-language-adapter count.
    #[must_use]
    pub const fn enabled_language_adapter_count(self) -> u8 {
        self.enabled_language_adapter_count
    }

    /// Returns the fixed number of compiled language adapters.
    #[must_use]
    pub const fn compiled_language_adapter_count(self) -> u8 {
        COMPILED_LANGUAGE_ADAPTER_COUNT
    }

    /// Returns the requested and authorized MCP-profile check.
    #[must_use]
    pub const fn mcp_tool_profile(self) -> DoctorCheckStatus {
        self.mcp_tool_profile
    }

    /// Returns the cross-setting compatibility check.
    #[must_use]
    pub const fn incompatible_settings(self) -> DoctorCheckStatus {
        self.incompatible_settings
    }

    /// Returns the repository capability check.
    #[must_use]
    pub const fn repository_capability(self) -> DoctorCheckStatus {
        self.repository_capability
    }

    /// Returns the database-outside-worktree check.
    #[must_use]
    pub const fn database_placement(self) -> DoctorCheckStatus {
        self.database_placement
    }

    /// Returns the database or parent-directory capability check.
    #[must_use]
    pub const fn database_capability(self) -> DoctorCheckStatus {
        self.database_capability
    }

    /// Returns the SQLite runtime-version check.
    #[must_use]
    pub const fn sqlite_runtime(self) -> DoctorCheckStatus {
        self.sqlite_runtime
    }

    /// Returns the required SQLite compile-options check.
    #[must_use]
    pub const fn sqlite_compile_options(self) -> DoctorCheckStatus {
        self.sqlite_compile_options
    }

    /// Returns the existing database schema check.
    #[must_use]
    pub const fn database_schema(self) -> DoctorCheckStatus {
        self.database_schema
    }

    /// Returns the database target state.
    #[must_use]
    pub const fn database_state(self) -> DoctorDatabaseState {
        self.database_state
    }

    /// Returns SQLite's monotonic integer version number when target checks ran.
    #[must_use]
    pub const fn sqlite_runtime_version_number(self) -> Option<i32> {
        self.sqlite_runtime_version_number
    }

    fn checks(self) -> [DoctorCheckStatus; 10] {
        [
            self.configuration,
            self.language_adapters,
            self.mcp_tool_profile,
            self.incompatible_settings,
            self.repository_capability,
            self.database_placement,
            self.database_capability,
            self.sqlite_runtime,
            self.sqlite_compile_options,
            self.database_schema,
        ]
    }
}

/// Runs bounded, read-only local diagnostics for one resolved configuration.
#[must_use]
pub fn inspect_local_doctor(
    configuration: &ResolvedConfiguration,
    targets: Option<LocalDoctorTargets<'_>>,
) -> LocalDoctorReport {
    let configuration_status = check_configuration(configuration);
    let language_count = configuration
        .policy()
        .allowed_languages()
        .effective()
        .len()
        .try_into()
        .unwrap_or(u8::MAX);
    let language_status = if language_count == 0 {
        DoctorCheckStatus::Warning
    } else if all_languages_compiled(configuration) {
        DoctorCheckStatus::Ok
    } else {
        DoctorCheckStatus::Error
    };
    let mut report = LocalDoctorReport {
        configuration: configuration_status,
        language_adapters: language_status,
        mcp_tool_profile: check_mcp_profile(configuration),
        incompatible_settings: check_incompatible_settings(configuration),
        repository_capability: DoctorCheckStatus::NotRun,
        database_placement: DoctorCheckStatus::NotRun,
        database_capability: DoctorCheckStatus::NotRun,
        sqlite_runtime: DoctorCheckStatus::NotRun,
        sqlite_compile_options: DoctorCheckStatus::NotRun,
        database_schema: DoctorCheckStatus::NotRun,
        database_state: DoctorDatabaseState::NotRequested,
        enabled_language_adapter_count: language_count,
        sqlite_runtime_version_number: None,
    };
    if let Some(targets) = targets {
        inspect_targets(&mut report, targets);
    }
    report
}

fn inspect_targets(report: &mut LocalDoctorReport, targets: LocalDoctorTargets<'_>) {
    report.database_state = DoctorDatabaseState::Unavailable;
    let sqlite = inspect_sqlite_environment();
    report.sqlite_runtime_version_number = Some(sqlite.runtime_version_number);
    report.sqlite_runtime = check_status(sqlite.runtime_supported);
    report.sqlite_compile_options = check_status(sqlite.compile_options_supported);

    let Some(repository) = ValidatedRepository::open(targets.repository) else {
        report.repository_capability = DoctorCheckStatus::Error;
        return;
    };
    report.repository_capability = DoctorCheckStatus::Ok;

    let Some(database_path) = canonical_database_target(targets.database) else {
        report.database_placement = DoctorCheckStatus::Error;
        return;
    };
    if !database_is_outside_repository(&database_path, &repository) {
        report.database_placement = DoctorCheckStatus::Error;
        return;
    }
    report.database_placement = DoctorCheckStatus::Ok;

    match ValidatedDatabase::open(&database_path) {
        DatabaseOpenOutcome::Missing => {
            report.database_capability = DoctorCheckStatus::Ok;
            report.database_state = DoctorDatabaseState::Missing;
        }
        DatabaseOpenOutcome::Unavailable => {
            report.database_capability = DoctorCheckStatus::Error;
        }
        DatabaseOpenOutcome::Existing(database) => {
            report.database_capability = DoctorCheckStatus::Ok;
            report.database_state = DoctorDatabaseState::Existing;
            if report.sqlite_runtime == DoctorCheckStatus::Ok
                && report.sqlite_compile_options == DoctorCheckStatus::Ok
            {
                let schema_valid = validate_database_read_only(&database.canonical_path);
                if !database.identity_is_current() {
                    report.database_capability = DoctorCheckStatus::Error;
                    report.database_schema = DoctorCheckStatus::NotRun;
                } else {
                    report.database_schema = check_status(schema_valid);
                }
            }
        }
    }
}

fn check_configuration(configuration: &ResolvedConfiguration) -> DoctorCheckStatus {
    check_status(
        configuration.schema_version() == CONFIGURATION_SCHEMA_VERSION
            && configuration.resolver_version() == CONFIGURATION_RESOLVER_VERSION,
    )
}

fn all_languages_compiled(configuration: &ResolvedConfiguration) -> bool {
    configuration
        .policy()
        .allowed_languages()
        .effective()
        .iter()
        .all(|language| {
            matches!(
                language,
                SourceLanguage::Rust
                    | SourceLanguage::Go
                    | SourceLanguage::TypeScript
                    | SourceLanguage::Tsx
                    | SourceLanguage::Python
            )
        })
}

fn check_mcp_profile(configuration: &ResolvedConfiguration) -> DoctorCheckStatus {
    let profile = configuration.preferences().mcp_tool_profile();
    check_status(matches!(
        (profile.requested(), profile.authorized()),
        (McpToolProfile::Canonical, Some(McpToolProfile::Canonical))
    ))
}

fn check_incompatible_settings(configuration: &ResolvedConfiguration) -> DoctorCheckStatus {
    let preferences = configuration.preferences();
    let policy = configuration.policy();
    let valid = bounded_preference(
        preferences.query_results(),
        *policy.max_query_results().effective(),
        MAX_CONFIGURATION_QUERY_RESULTS,
    ) && bounded_preference(
        preferences.context_bytes(),
        *policy.max_context_bytes().effective(),
        MAX_CONFIGURATION_CONTEXT_BYTES,
    ) && bounded_preference(
        preferences.graph_depth(),
        *policy.max_graph_depth().effective(),
        MAX_CONFIGURATION_GRAPH_DEPTH,
    ) && bounded_preference(
        preferences.graph_results(),
        *policy.max_graph_results().effective(),
        MAX_CONFIGURATION_GRAPH_RESULTS,
    ) && in_range(
        *preferences.watcher_poll_interval_ms().effective(),
        MIN_CONFIGURATION_WATCHER_POLL_INTERVAL_MS,
        MAX_CONFIGURATION_WATCHER_POLL_INTERVAL_MS,
    ) && in_range(
        *preferences.watcher_poll_interval_ms().requested(),
        MIN_CONFIGURATION_WATCHER_POLL_INTERVAL_MS,
        MAX_CONFIGURATION_WATCHER_POLL_INTERVAL_MS,
    ) && bounded_policy(
        *policy.max_source_file_bytes().effective(),
        MAX_CONFIGURATION_SOURCE_FILE_BYTES,
    ) && bounded_policy(
        *policy.max_source_files().effective(),
        MAX_CONFIGURATION_SOURCE_FILES,
    ) && !*policy.follow_symlinks().effective();
    check_status(valid)
}

fn bounded_preference(preference: &ResolvedPreference<u64>, policy: u64, maximum: u64) -> bool {
    let requested = *preference.requested();
    let effective = *preference.effective();
    bounded_policy(requested, maximum)
        && bounded_policy(policy, maximum)
        && effective > 0
        && effective <= requested
        && effective <= policy
}

const fn bounded_policy(value: u64, maximum: u64) -> bool {
    value > 0 && value <= maximum
}

const fn in_range(value: u64, minimum: u64, maximum: u64) -> bool {
    value >= minimum && value <= maximum
}

const fn check_status(passed: bool) -> DoctorCheckStatus {
    if passed {
        DoctorCheckStatus::Ok
    } else {
        DoctorCheckStatus::Error
    }
}

struct ValidatedRepository {
    canonical_path: PathBuf,
    identity: FileIdentity,
    _capability: Dir,
}

impl ValidatedRepository {
    fn open(path: &Path) -> Option<Self> {
        if !valid_path(path) {
            return None;
        }
        let metadata = fs::symlink_metadata(path).ok()?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return None;
        }
        let capability = open_explicit_directory_nofollow(path)?;
        let opened_identity =
            FileIdentity::from_file(capability.try_clone().ok()?.into_std_file()).ok()?;
        let current_metadata = fs::symlink_metadata(path).ok()?;
        if current_metadata.file_type().is_symlink() || !current_metadata.is_dir() {
            return None;
        }
        let current_identity = FileIdentity::from_path(path).ok()?;
        if opened_identity != current_identity {
            return None;
        }
        let canonical_path = fs::canonicalize(path).ok()?;
        Some(Self {
            canonical_path,
            identity: opened_identity,
            _capability: capability,
        })
    }
}

fn open_explicit_directory_nofollow(path: &Path) -> Option<Dir> {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => {
            let parent = if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            };
            let parent = fs::canonicalize(parent).ok()?;
            Dir::open_ambient_dir(parent, ambient_authority())
                .ok()?
                .open_dir_nofollow(name)
                .ok()
        }
        _ => Dir::open_ambient_dir(path, ambient_authority()).ok(),
    }
}

fn canonical_database_target(path: &Path) -> Option<PathBuf> {
    if !valid_path(path) {
        return None;
    }
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let parent = fs::canonicalize(parent).ok()?;
    let file_name = path.file_name()?;
    Some(parent.join(file_name))
}

fn database_is_outside_repository(database_path: &Path, repository: &ValidatedRepository) -> bool {
    if database_path.starts_with(&repository.canonical_path) {
        return false;
    }
    let Some(parent) = database_path.parent() else {
        return false;
    };
    for (index, ancestor) in parent.ancestors().enumerate() {
        if index >= MAX_DOCTOR_ANCESTORS {
            return false;
        }
        let Ok(identity) = FileIdentity::from_path(ancestor) else {
            return false;
        };
        if identity == repository.identity {
            return false;
        }
    }
    true
}

enum DatabaseOpenOutcome {
    Missing,
    Existing(ValidatedDatabase),
    Unavailable,
}

struct ValidatedDatabase {
    canonical_path: PathBuf,
    identity: FileIdentity,
    _capability: std::fs::File,
}

impl ValidatedDatabase {
    fn open(path: &Path) -> DatabaseOpenOutcome {
        let Some(parent) = path.parent() else {
            return DatabaseOpenOutcome::Unavailable;
        };
        let Some(name) = path.file_name() else {
            return DatabaseOpenOutcome::Unavailable;
        };
        let Ok(parent) = Dir::open_ambient_dir(parent, ambient_authority()) else {
            return DatabaseOpenOutcome::Unavailable;
        };
        if let Ok(metadata) = fs::symlink_metadata(path)
            && (metadata.file_type().is_symlink() || !metadata.is_file())
        {
            return DatabaseOpenOutcome::Unavailable;
        }
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        configure_nonblocking_open(&mut options);
        let file = match parent.open_with(name, &options) {
            Ok(file) => file.into_std(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return DatabaseOpenOutcome::Missing;
            }
            Err(_) => return DatabaseOpenOutcome::Unavailable,
        };
        let Ok(metadata) = file.metadata() else {
            return DatabaseOpenOutcome::Unavailable;
        };
        if !metadata.is_file() || !file_has_single_link(&file).is_ok_and(|single_link| single_link)
        {
            return DatabaseOpenOutcome::Unavailable;
        }
        let Ok(current_metadata) = fs::symlink_metadata(path) else {
            return DatabaseOpenOutcome::Unavailable;
        };
        if current_metadata.file_type().is_symlink() || !current_metadata.is_file() {
            return DatabaseOpenOutcome::Unavailable;
        }
        let Ok(identity) = file.try_clone().and_then(FileIdentity::from_file) else {
            return DatabaseOpenOutcome::Unavailable;
        };
        let Ok(current_identity) = FileIdentity::from_path(path) else {
            return DatabaseOpenOutcome::Unavailable;
        };
        if identity != current_identity {
            return DatabaseOpenOutcome::Unavailable;
        }
        DatabaseOpenOutcome::Existing(Self {
            canonical_path: path.to_owned(),
            identity,
            _capability: file,
        })
    }

    fn identity_is_current(&self) -> bool {
        file_has_single_link(&self._capability).is_ok_and(|single_link| single_link)
            && fs::symlink_metadata(&self.canonical_path).is_ok_and(|metadata| {
                !metadata.file_type().is_symlink()
                    && metadata.is_file()
                    && FileIdentity::from_path(&self.canonical_path)
                        .is_ok_and(|current| current == self.identity)
            })
    }
}

fn valid_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.as_os_str().as_encoded_bytes().len() <= MAX_DOCTOR_PATH_BYTES
}

#[cfg(unix)]
fn configure_nonblocking_open(options: &mut OpenOptions) {
    use cap_std::fs::OpenOptionsExt;

    options.custom_flags(
        i32::try_from(rustix::fs::OFlags::NONBLOCK.bits())
            .expect("O_NONBLOCK flag bits fit the platform open flag type"),
    );
}

#[cfg(not(unix))]
fn configure_nonblocking_open(_options: &mut OpenOptions) {}

#[cfg(test)]
mod tests;
