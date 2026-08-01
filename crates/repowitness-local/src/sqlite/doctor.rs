use std::{
    fmt::Write as _,
    path::Path,
    time::{Duration, Instant},
};

use rusqlite::{Connection, OpenFlags};

use super::{
    APPLICATION_ID, BUSY_TIMEOUT, MINIMUM_SQLITE_VERSION, SCHEMA_VERSION, SqliteStoreError,
    canonical_database_path, migration_checksum, migrations, pragma_i64,
};

const DOCTOR_PROGRESS_INSTRUCTIONS: i32 = 1_000;
const DOCTOR_SCHEMA_DEADLINE: Duration = Duration::from_millis(250);
const MAX_MIGRATION_NAME_BYTES: i64 = 64;
const REQUIRED_COMPILE_OPTION_COUNT: i64 = 2;

pub(crate) struct SqliteEnvironmentDiagnostic {
    pub(crate) runtime_version_number: i32,
    pub(crate) runtime_supported: bool,
    pub(crate) compile_options_supported: bool,
}

pub(crate) fn inspect_sqlite_environment() -> SqliteEnvironmentDiagnostic {
    let runtime_version_number = rusqlite::version_number();
    let compile_options_supported = Connection::open_in_memory()
        .and_then(|connection| {
            connection.query_row(
                "SELECT count(*) FROM pragma_compile_options
                 WHERE compile_options IN ('ENABLE_FTS5', 'THREADSAFE=1')",
                [],
                |row| row.get::<_, i64>(0),
            )
        })
        .is_ok_and(|count| count == REQUIRED_COMPILE_OPTION_COUNT);
    SqliteEnvironmentDiagnostic {
        runtime_version_number,
        runtime_supported: runtime_version_number >= MINIMUM_SQLITE_VERSION,
        compile_options_supported,
    }
}

pub(crate) fn validate_database_read_only(path: &Path) -> bool {
    open_immutable_reader(path).is_ok()
}

#[cfg(test)]
pub(crate) fn create_valid_test_database(path: &Path) -> bool {
    super::open_index_writer(path, 123).is_ok()
}

fn open_immutable_reader(path: &Path) -> Result<Connection, SqliteStoreError> {
    if rusqlite::version_number() < MINIMUM_SQLITE_VERSION {
        return Err(SqliteStoreError::UnsupportedSqliteVersion);
    }
    let path = canonical_database_path(path)?;
    let uri = immutable_file_uri(&path).ok_or(SqliteStoreError::OpenFailed)?;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW
        | OpenFlags::SQLITE_OPEN_URI;
    let connection =
        Connection::open_with_flags(uri, flags).map_err(|_| SqliteStoreError::OpenFailed)?;
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    connection
        .pragma_update(None, "trusted_schema", false)
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    let deadline = Instant::now()
        .checked_add(DOCTOR_SCHEMA_DEADLINE)
        .ok_or(SqliteStoreError::ConfigurationFailed)?;
    connection
        .progress_handler(
            DOCTOR_PROGRESS_INSTRUCTIONS,
            Some(move || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    if pragma_i64(&connection, "application_id")? != APPLICATION_ID {
        return Err(SqliteStoreError::ApplicationIdMismatch);
    }
    if pragma_i64(&connection, "user_version")? != SCHEMA_VERSION {
        return Err(SqliteStoreError::SchemaVersionMismatch);
    }
    validate_bounded_migration_ledger(&connection)?;
    Ok(connection)
}

fn validate_bounded_migration_ledger(connection: &Connection) -> Result<(), SqliteStoreError> {
    let mut statement = connection
        .prepare(
            "SELECT version,
                    length(CAST(name AS BLOB)),
                    substr(CAST(name AS BLOB), 1, 64),
                    typeof(checksum),
                    length(checksum),
                    substr(checksum, 1, 32)
             FROM schema_migrations ORDER BY version LIMIT 5",
        )
        .map_err(|_| SqliteStoreError::MigrationLedgerMismatch)?;
    let mut rows = statement
        .query([])
        .map_err(|_| SqliteStoreError::MigrationLedgerMismatch)?;
    for (version, name, sql) in migrations() {
        let row = rows
            .next()
            .map_err(|_| SqliteStoreError::MigrationLedgerMismatch)?
            .ok_or(SqliteStoreError::MigrationLedgerMismatch)?;
        let actual_version = row
            .get::<_, i64>(0)
            .map_err(|_| SqliteStoreError::MigrationLedgerMismatch)?;
        let actual_name_length = row
            .get::<_, i64>(1)
            .map_err(|_| SqliteStoreError::MigrationLedgerMismatch)?;
        let actual_name = row
            .get::<_, Vec<u8>>(2)
            .map_err(|_| SqliteStoreError::MigrationLedgerMismatch)?;
        let actual_checksum_kind = row
            .get::<_, String>(3)
            .map_err(|_| SqliteStoreError::MigrationLedgerMismatch)?;
        let actual_checksum_length = row
            .get::<_, i64>(4)
            .map_err(|_| SqliteStoreError::MigrationLedgerMismatch)?;
        let actual_checksum = row
            .get::<_, Vec<u8>>(5)
            .map_err(|_| SqliteStoreError::MigrationLedgerMismatch)?;
        let expected_name_length =
            i64::try_from(name.len()).map_err(|_| SqliteStoreError::MigrationLedgerMismatch)?;
        if actual_version != version
            || actual_name_length != expected_name_length
            || actual_name_length > MAX_MIGRATION_NAME_BYTES
            || actual_name.as_slice() != name.as_bytes()
            || actual_checksum_kind != "blob"
            || actual_checksum_length != 32
            || actual_checksum.as_slice() != migration_checksum(sql)
        {
            return Err(SqliteStoreError::MigrationLedgerMismatch);
        }
    }
    if rows
        .next()
        .map_err(|_| SqliteStoreError::MigrationLedgerMismatch)?
        .is_some()
    {
        return Err(SqliteStoreError::MigrationLedgerMismatch);
    }
    Ok(())
}

fn immutable_file_uri(path: &Path) -> Option<String> {
    let bytes = path_bytes(path)?;
    let required_capacity = bytes.len().checked_mul(3)?.checked_add(17)?;
    let mut uri = String::new();
    uri.try_reserve(required_capacity).ok()?;
    uri.push_str("file:");
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            uri.push(char::from(byte));
        } else {
            write!(uri, "%{byte:02X}").ok()?;
        }
    }
    uri.push_str("?immutable=1");
    Some(uri)
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Option<&[u8]> {
    use std::os::unix::ffi::OsStrExt;

    Some(path.as_os_str().as_bytes())
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Option<&[u8]> {
    path.to_str().map(str::as_bytes)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "repowitness-sqlite-doctor-{}-{}",
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
    fn bundled_runtime_and_compile_options_satisfy_the_contract() {
        let diagnostic = inspect_sqlite_environment();

        assert!(diagnostic.runtime_supported);
        assert!(diagnostic.compile_options_supported);
        assert!(diagnostic.runtime_version_number >= MINIMUM_SQLITE_VERSION);
    }

    #[test]
    fn valid_database_is_checked_without_mutating_it_or_creating_sidecars() {
        let directory = TempDirectory::new();
        let database = directory.path().join("index #%.sqlite3");
        drop(
            super::super::open_index_writer(&database, 123)
                .expect("test database should be initialized"),
        );
        let before_bytes = fs::read(&database).expect("database should be readable");
        let before_entries = directory_entries(directory.path());

        assert!(validate_database_read_only(&database));

        assert_eq!(
            fs::read(&database).expect("database should remain readable"),
            before_bytes
        );
        assert_eq!(directory_entries(directory.path()), before_entries);
    }

    #[test]
    fn invalid_database_is_rejected_without_mutation() {
        let directory = TempDirectory::new();
        let database = directory.path().join("invalid.sqlite3");
        let contents = b"not a RepoWitness database";
        fs::write(&database, contents).expect("invalid fixture should be written");
        let before_entries = directory_entries(directory.path());

        assert!(!validate_database_read_only(&database));

        assert_eq!(
            fs::read(&database).expect("fixture should remain readable"),
            contents
        );
        assert_eq!(directory_entries(directory.path()), before_entries);
    }

    #[test]
    fn hostile_migration_view_is_bounded_and_does_not_mutate() {
        let directory = TempDirectory::new();
        let database = directory.path().join("hostile.sqlite3");
        let connection = Connection::open(&database).expect("fixture should open");
        connection
            .pragma_update(None, "application_id", APPLICATION_ID)
            .expect("application id should be set");
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .expect("schema version should be set");
        connection
            .execute_batch(
                "CREATE VIEW schema_migrations AS
                 WITH RECURSIVE generated(version) AS (
                     VALUES(1)
                     UNION ALL
                     SELECT version + 1 FROM generated WHERE version < 100000000
                 )
                 SELECT version, printf('%09d', version) AS name,
                        zeroblob(32) AS checksum
                 FROM generated;",
            )
            .expect("hostile view should be created");
        drop(connection);
        let before_bytes = fs::read(&database).expect("fixture should be readable");
        let before_entries = directory_entries(directory.path());
        let started = Instant::now();

        assert!(!validate_database_read_only(&database));

        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            fs::read(&database).expect("fixture should remain readable"),
            before_bytes
        );
        assert_eq!(directory_entries(directory.path()), before_entries);
    }

    #[test]
    fn immutable_uri_encodes_filename_metacharacters() {
        let path = PathBuf::from("/tmp/a file?#%.sqlite3");
        let uri = immutable_file_uri(&path).expect("URI should encode");

        assert_eq!(uri, "file:/tmp/a%20file%3F%23%25.sqlite3?immutable=1");
    }
}
