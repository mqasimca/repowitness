use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use repowitness_application::{
    ConfigurationLayer, ConfigurationLayerKind, ConfigurationPolicyOverrides,
    ConfigurationPreferenceOverrides, McpToolProfile, resolve_configuration,
};

use super::*;
use crate::sqlite::create_valid_test_database;

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "repowitness-local-doctor-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("temporary directory should be created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn default_configuration() -> ResolvedConfiguration {
    resolve_configuration(&[]).expect("built-in configuration should resolve")
}

fn directory_entries(path: &Path) -> Vec<Vec<u8>> {
    let mut entries = fs::read_dir(path)
        .expect("directory should be readable")
        .map(|entry| {
            entry
                .expect("entry should be readable")
                .file_name()
                .as_encoded_bytes()
                .to_vec()
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

#[test]
fn configuration_only_is_successful_with_explicit_target_warning() {
    let report = inspect_local_doctor(&default_configuration(), None);

    assert_eq!(report.status(), DoctorOverallStatus::Warning);
    assert_eq!(report.error_count(), 0);
    assert_eq!(report.warning_count(), 1);
    assert!(!report.target_checks_requested());
    assert_eq!(report.configuration(), DoctorCheckStatus::Ok);
    assert_eq!(report.language_adapters(), DoctorCheckStatus::Ok);
    assert_eq!(report.mcp_tool_profile(), DoctorCheckStatus::Ok);
    assert_eq!(report.incompatible_settings(), DoctorCheckStatus::Ok);
    assert_eq!(report.repository_capability(), DoctorCheckStatus::NotRun);
    assert_eq!(report.sqlite_runtime(), DoctorCheckStatus::NotRun);
    assert_eq!(report.database_state(), DoctorDatabaseState::NotRequested);
    assert_eq!(report.sqlite_runtime_version_number(), None);
}

#[test]
fn empty_language_set_is_a_warning_but_unauthorized_mcp_profile_is_an_error() {
    let mut allowed_profiles = BTreeSet::new();
    allowed_profiles.insert(McpToolProfile::Canonical);
    let layer = ConfigurationLayer::try_new(
        ConfigurationLayerKind::Repository,
        None,
        ConfigurationPreferenceOverrides::try_new(
            None,
            None,
            None,
            None,
            None,
            Some(McpToolProfile::IncumbentCompatible),
        )
        .expect("preference should be valid"),
        ConfigurationPolicyOverrides::try_new(
            Some(BTreeSet::new()),
            Some(allowed_profiles),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("policy should be valid"),
    )
    .expect("layer should be valid");
    let configuration = resolve_configuration(&[layer]).expect("configuration should resolve");

    let report = inspect_local_doctor(&configuration, None);

    assert_eq!(report.status(), DoctorOverallStatus::Error);
    assert_eq!(report.language_adapters(), DoctorCheckStatus::Warning);
    assert_eq!(report.enabled_language_adapter_count(), 0);
    assert_eq!(report.compiled_language_adapter_count(), 5);
    assert_eq!(report.mcp_tool_profile(), DoctorCheckStatus::Error);
    assert_eq!(report.error_count(), 1);
    assert_eq!(report.warning_count(), 2);
}

#[test]
fn missing_database_validates_parent_without_creating_any_state() {
    let directory = TempDirectory::new();
    let repository = directory.path().join("repository");
    let state = directory.path().join("state");
    fs::create_dir(&repository).expect("repository should be created");
    fs::create_dir(&state).expect("state directory should be created");
    fs::write(repository.join("marker"), b"unchanged").expect("marker should be written");
    let database = state.join("missing.sqlite3");
    let before_root = directory_entries(directory.path());
    let before_repository = directory_entries(&repository);
    let before_state = directory_entries(&state);

    let report = inspect_local_doctor(
        &default_configuration(),
        Some(LocalDoctorTargets::new(&repository, &database)),
    );

    assert_eq!(report.status(), DoctorOverallStatus::Warning);
    assert_eq!(report.warning_count(), 1);
    assert_eq!(report.repository_capability(), DoctorCheckStatus::Ok);
    assert_eq!(report.database_placement(), DoctorCheckStatus::Ok);
    assert_eq!(report.database_capability(), DoctorCheckStatus::Ok);
    assert_eq!(report.sqlite_runtime(), DoctorCheckStatus::Ok);
    assert_eq!(report.sqlite_compile_options(), DoctorCheckStatus::Ok);
    assert_eq!(report.database_schema(), DoctorCheckStatus::NotRun);
    assert_eq!(report.database_state(), DoctorDatabaseState::Missing);
    assert!(!database.exists());
    assert_eq!(directory_entries(directory.path()), before_root);
    assert_eq!(directory_entries(&repository), before_repository);
    assert_eq!(directory_entries(&state), before_state);
    assert_eq!(
        fs::read(repository.join("marker")).expect("marker should remain"),
        b"unchanged"
    );
}

#[cfg(unix)]
#[test]
fn missing_database_needs_only_a_readable_parent_capability() {
    use std::os::unix::fs::PermissionsExt;

    let directory = TempDirectory::new();
    let repository = directory.path().join("repository");
    let state = directory.path().join("read-only-state");
    fs::create_dir(&repository).expect("repository should be created");
    fs::create_dir(&state).expect("state directory should be created");
    let database = state.join("missing.sqlite3");
    fs::set_permissions(&state, fs::Permissions::from_mode(0o500))
        .expect("state permissions should be restricted");

    let report = inspect_local_doctor(
        &default_configuration(),
        Some(LocalDoctorTargets::new(&repository, &database)),
    );

    fs::set_permissions(&state, fs::Permissions::from_mode(0o700))
        .expect("state permissions should be restored");
    assert_eq!(report.database_capability(), DoctorCheckStatus::Ok);
    assert_eq!(report.database_state(), DoctorDatabaseState::Missing);
    assert!(!database.exists());
}

#[test]
fn existing_database_is_validated_without_mutation_or_sidecars() {
    let directory = TempDirectory::new();
    let repository = directory.path().join("repository");
    let state = directory.path().join("state");
    fs::create_dir(&repository).expect("repository should be created");
    fs::create_dir(&state).expect("state directory should be created");
    let database = state.join("index.sqlite3");
    assert!(create_valid_test_database(&database));
    let before_bytes = fs::read(&database).expect("database should be readable");
    let before_entries = directory_entries(&state);

    let report = inspect_local_doctor(
        &default_configuration(),
        Some(LocalDoctorTargets::new(&repository, &database)),
    );

    assert_eq!(report.status(), DoctorOverallStatus::Ok);
    assert_eq!(report.error_count(), 0);
    assert_eq!(report.warning_count(), 0);
    assert_eq!(report.database_state(), DoctorDatabaseState::Existing);
    assert_eq!(report.database_schema(), DoctorCheckStatus::Ok);
    assert_eq!(
        fs::read(&database).expect("database should remain readable"),
        before_bytes
    );
    assert_eq!(directory_entries(&state), before_entries);
}

#[test]
fn invalid_database_is_rejected_without_changing_contents() {
    let directory = TempDirectory::new();
    let repository = directory.path().join("repository");
    let state = directory.path().join("state");
    fs::create_dir(&repository).expect("repository should be created");
    fs::create_dir(&state).expect("state directory should be created");
    let database = state.join("invalid.sqlite3");
    let bytes = b"hostile database bytes";
    fs::write(&database, bytes).expect("fixture should be written");
    let before_entries = directory_entries(&state);

    let report = inspect_local_doctor(
        &default_configuration(),
        Some(LocalDoctorTargets::new(&repository, &database)),
    );

    assert_eq!(report.status(), DoctorOverallStatus::Error);
    assert_eq!(report.database_capability(), DoctorCheckStatus::Ok);
    assert_eq!(report.database_schema(), DoctorCheckStatus::Error);
    assert_eq!(report.database_state(), DoctorDatabaseState::Existing);
    assert_eq!(
        fs::read(&database).expect("fixture should remain readable"),
        bytes
    );
    assert_eq!(directory_entries(&state), before_entries);
}

#[test]
fn database_inside_repository_is_rejected_before_it_is_opened_or_created() {
    let directory = TempDirectory::new();
    let repository = directory.path().join("repository");
    fs::create_dir(&repository).expect("repository should be created");
    let database = repository.join("index.sqlite3");
    let before = directory_entries(&repository);

    let report = inspect_local_doctor(
        &default_configuration(),
        Some(LocalDoctorTargets::new(&repository, &database)),
    );

    assert_eq!(report.database_placement(), DoctorCheckStatus::Error);
    assert_eq!(report.database_capability(), DoctorCheckStatus::NotRun);
    assert_eq!(report.database_state(), DoctorDatabaseState::Unavailable);
    assert!(!database.exists());
    assert_eq!(directory_entries(&repository), before);
}

#[cfg(unix)]
#[test]
fn repository_and_database_symlinks_are_rejected_without_following_them() {
    use std::os::unix::fs::symlink;

    let directory = TempDirectory::new();
    let repository = directory.path().join("repository");
    let repository_alias = directory.path().join("repository-alias");
    let state = directory.path().join("state");
    fs::create_dir(&repository).expect("repository should be created");
    fs::create_dir(&state).expect("state should be created");
    symlink(&repository, &repository_alias).expect("repository symlink should be created");
    let database = state.join("index.sqlite3");

    let repository_report = inspect_local_doctor(
        &default_configuration(),
        Some(LocalDoctorTargets::new(&repository_alias, &database)),
    );
    assert_eq!(
        repository_report.repository_capability(),
        DoctorCheckStatus::Error
    );
    assert!(!database.exists());

    let target = state.join("target");
    fs::write(&target, b"not a database").expect("target should be written");
    symlink(&target, &database).expect("database symlink should be created");
    let before = directory_entries(&state);
    let database_report = inspect_local_doctor(
        &default_configuration(),
        Some(LocalDoctorTargets::new(&repository, &database)),
    );
    assert_eq!(
        database_report.database_capability(),
        DoctorCheckStatus::Error
    );
    assert_eq!(database_report.database_schema(), DoctorCheckStatus::NotRun);
    assert_eq!(directory_entries(&state), before);
    assert_eq!(
        fs::read(&target).expect("target should remain"),
        b"not a database"
    );

    let repository_parent_alias = directory.path().join("repository-parent-alias");
    symlink(&repository, &repository_parent_alias)
        .expect("repository parent alias should be created");
    let aliased_inside_database = repository_parent_alias.join("inside.sqlite3");
    let placement_report = inspect_local_doctor(
        &default_configuration(),
        Some(LocalDoctorTargets::new(
            &repository,
            &aliased_inside_database,
        )),
    );
    assert_eq!(
        placement_report.database_placement(),
        DoctorCheckStatus::Error
    );
    assert!(!aliased_inside_database.exists());
}

#[cfg(any(unix, windows))]
#[test]
fn multiply_linked_database_is_rejected_before_sqlite_open() {
    let directory = TempDirectory::new();
    let repository = directory.path().join("repository");
    let state = directory.path().join("state");
    fs::create_dir(&repository).expect("repository should be created");
    fs::create_dir(&state).expect("state should be created");
    let original = state.join("original.sqlite3");
    let database = state.join("alias.sqlite3");
    fs::write(&original, b"unchanged").expect("fixture should be written");
    fs::hard_link(&original, &database).expect("hard link should be created");

    let report = inspect_local_doctor(
        &default_configuration(),
        Some(LocalDoctorTargets::new(&repository, &database)),
    );

    assert_eq!(report.database_capability(), DoctorCheckStatus::Error);
    assert_eq!(report.database_schema(), DoctorCheckStatus::NotRun);
    assert_eq!(
        fs::read(&original).expect("fixture should remain"),
        b"unchanged"
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_missing_database_name_is_bounded_and_not_created() {
    use std::os::unix::ffi::OsStringExt;

    let directory = TempDirectory::new();
    let repository = directory.path().join("repository");
    let state = directory.path().join("state");
    fs::create_dir(&repository).expect("repository should be created");
    fs::create_dir(&state).expect("state should be created");
    let database = state.join(std::ffi::OsString::from_vec(vec![b'd', b'b', 0xFF]));

    let report = inspect_local_doctor(
        &default_configuration(),
        Some(LocalDoctorTargets::new(&repository, &database)),
    );

    assert_eq!(report.status(), DoctorOverallStatus::Warning);
    assert_eq!(report.database_state(), DoctorDatabaseState::Missing);
    assert!(!database.exists());
}

#[test]
fn target_debug_and_report_never_retain_paths() {
    let targets = LocalDoctorTargets::new(
        Path::new("../sensitive-repository"),
        Path::new("../sensitive-database.sqlite3"),
    );
    let targets_debug = format!("{targets:?}");

    assert_eq!(targets_debug.matches("<redacted-path>").count(), 2);
    assert!(!targets_debug.contains("sensitive"));

    let report = inspect_local_doctor(&default_configuration(), Some(targets));
    let report_debug = format!("{report:?}");
    assert!(!report_debug.contains("sensitive"));
    assert!(!report_debug.contains('/'));
    assert!(!report_debug.contains('\\'));
}

#[test]
fn overlong_paths_fail_before_filesystem_access() {
    let repository = PathBuf::from("x".repeat(MAX_DOCTOR_PATH_BYTES + 1));
    let database = PathBuf::from("unused.sqlite3");

    assert!(!valid_path(&repository));
    let report = inspect_local_doctor(
        &default_configuration(),
        Some(LocalDoctorTargets::new(&repository, &database)),
    );

    assert_eq!(report.repository_capability(), DoctorCheckStatus::Error);
}
