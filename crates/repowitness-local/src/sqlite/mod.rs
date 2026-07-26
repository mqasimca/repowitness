mod backup;
mod reader;
mod schema;
mod worker;
mod writer;

use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use crate::contained_source::FileIdentity;

pub use self::backup::{BackupLimits, BackupOutcome, create_online_backup};
pub use self::reader::{
    OwnedSqliteReader, SearchHit, SearchLimits, SearchResults, SymbolLookupResults,
};
use self::schema::{
    APPLICATION_ID, MIGRATION_1, MIGRATION_1_NAME, MIGRATION_2, MIGRATION_2_NAME, MIGRATION_3,
    MIGRATION_3_NAME, SCHEMA_VERSION,
};
pub(crate) use self::worker::SqliteMutationLease;
pub use self::worker::{IndexStoreStartup, OwnedSqliteIndex};
pub use self::writer::{
    CheckpointOutcome, GenerationId, ProjectionRebuildLimits, ProjectionRebuildOutcome,
};
pub use repowitness_application::RustIndexCoverage as GenerationCoverage;

const MINIMUM_SQLITE_VERSION: i32 = 3_051_003;
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const STARTUP_PROGRESS_INSTRUCTIONS: i32 = 1_000;

/// Stable failure at the Phase 0 SQLite trust boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqliteStoreError {
    /// The bundled SQLite does not contain the required WAL-reset fix.
    UnsupportedSqliteVersion,
    /// The database could not be opened.
    OpenFailed,
    /// Required connection policy could not be applied.
    ConfigurationFailed,
    /// An existing file has a different application identity.
    ApplicationIdMismatch,
    /// The file uses an unknown or unsupported schema version.
    SchemaVersionMismatch,
    /// The exact compiled migration ledger does not match the database.
    MigrationLedgerMismatch,
    /// Schema creation or migration failed.
    MigrationFailed,
    /// Required FTS5 support is unavailable.
    Fts5Unavailable,
    /// The process-level database mutation lease could not be opened or locked.
    MutationLeaseUnavailable,
    /// The database path stopped naming the file authorized before startup.
    DatabaseIdentityChanged,
    /// Incomplete generations exceeded the bounded startup recovery budget.
    RecoveryGenerationLimitExceeded,
    /// A newly reserved database could not be removed after startup failed.
    DatabaseStartupCleanupFailed,
    /// A database operation failed without exposing raw SQLite text.
    DatabaseOperationFailed,
    /// A fixed-width count could not be represented by SQLite.
    CountNotRepresentable,
    /// The supplied source epoch was not the current workspace epoch.
    StaleSourceEpoch,
    /// A requested source epoch transition was not monotonic.
    InvalidSourceEpoch,
    /// The prepared index did not match the declared snapshot semantics.
    PreparedIdentityMismatch,
    /// Persisted rows failed an exact count or identity check.
    IntegrityCheckFailed,
    /// The requested generation does not exist in the required state.
    GenerationUnavailable,
    /// The write was cancelled before a complete result.
    Cancelled,
    /// The absolute write deadline elapsed.
    DeadlineExceeded,
    /// The capacity-one owner queue is occupied.
    QueueFull,
    /// The owner thread is unavailable.
    WorkerUnavailable,
    /// The owner thread panicked.
    WorkerPanicked,
    /// A bounded reply did not arrive before its deadline.
    ReplyTimeout,
    /// Lexical search limits are zero or exceed Phase 0 ceilings.
    InvalidSearchLimits,
    /// Search-projection rebuild limits are zero or exceed Phase 0 ceilings.
    InvalidProjectionRebuildLimits,
    /// Authoritative searchable facts exceed the requested rebuild row limit.
    ProjectionRebuildRowLimitExceeded,
    /// The untrusted lexical query violates the literal query profile.
    InvalidSearchQuery,
    /// Search results exceeded the encoded-output byte limit.
    SearchOutputLimitExceeded,
    /// Persisted reusable artifacts exceeded the bounded load budget.
    ArtifactReuseLimitExceeded,
    /// Online-backup limits are zero or exceed Phase 0 ceilings.
    InvalidBackupLimits,
    /// The backup destination or private temporary path is unavailable.
    BackupDestinationUnavailable,
    /// SQLite online backup or validation failed.
    BackupFailed,
    /// The backup exceeded its maximum number of page steps.
    BackupStepLimitExceeded,
    /// A completed backup was published but its private temporary link remained.
    BackupCleanupFailed,
}

impl fmt::Display for SqliteStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedSqliteVersion => "SQLite version does not meet the Phase 0 minimum",
            Self::OpenFailed => "SQLite database could not be opened",
            Self::ConfigurationFailed => "SQLite connection policy could not be applied",
            Self::ApplicationIdMismatch => "SQLite application identity does not match RepoWitness",
            Self::SchemaVersionMismatch => "SQLite schema version is unsupported",
            Self::MigrationLedgerMismatch => {
                "SQLite migration ledger does not match compiled schema"
            }
            Self::MigrationFailed => "SQLite schema migration failed",
            Self::Fts5Unavailable => "SQLite FTS5 support is unavailable",
            Self::MutationLeaseUnavailable => "SQLite mutation lease is unavailable",
            Self::DatabaseIdentityChanged => "SQLite database identity changed during startup",
            Self::RecoveryGenerationLimitExceeded => "SQLite recovery generation limit exceeded",
            Self::DatabaseStartupCleanupFailed => "SQLite failed-startup database cleanup failed",
            Self::DatabaseOperationFailed => "SQLite index operation failed",
            Self::CountNotRepresentable => "SQLite index count is not representable",
            Self::StaleSourceEpoch => "SQLite source epoch is stale",
            Self::InvalidSourceEpoch => "SQLite source epoch transition is invalid",
            Self::PreparedIdentityMismatch => "prepared index identity is inconsistent",
            Self::IntegrityCheckFailed => "SQLite index integrity validation failed",
            Self::GenerationUnavailable => "SQLite generation is unavailable",
            Self::Cancelled => "SQLite index operation cancelled",
            Self::DeadlineExceeded => "SQLite index operation deadline exceeded",
            Self::QueueFull => "SQLite writer queue is full",
            Self::WorkerUnavailable => "SQLite writer is unavailable",
            Self::WorkerPanicked => "SQLite writer terminated unexpectedly",
            Self::ReplyTimeout => "SQLite writer reply deadline exceeded",
            Self::InvalidSearchLimits => "SQLite search limits are invalid",
            Self::InvalidProjectionRebuildLimits => {
                "SQLite search projection rebuild limits are invalid"
            }
            Self::ProjectionRebuildRowLimitExceeded => {
                "SQLite search projection rebuild row limit exceeded"
            }
            Self::InvalidSearchQuery => "SQLite search query is invalid",
            Self::SearchOutputLimitExceeded => "SQLite search output byte limit exceeded",
            Self::ArtifactReuseLimitExceeded => "SQLite reusable artifact load limit exceeded",
            Self::InvalidBackupLimits => "SQLite backup limits are invalid",
            Self::BackupDestinationUnavailable => "SQLite backup destination is unavailable",
            Self::BackupFailed => "SQLite online backup failed",
            Self::BackupStepLimitExceeded => "SQLite online backup step limit exceeded",
            Self::BackupCleanupFailed => "SQLite backup temporary file cleanup failed",
        })
    }
}

impl std::error::Error for SqliteStoreError {}

/// Opens, configures, migrates, and validates the one writer-owned connection.
#[cfg(test)]
fn open_index_writer(path: &Path, applied_at_unix_ms: u64) -> Result<Connection, SqliteStoreError> {
    let expected_identity = database_file_identity(path)?;
    open_index_writer_with_identity(path, expected_identity, applied_at_unix_ms)
}

#[cfg(test)]
fn open_index_writer_with_identity(
    path: &Path,
    expected_identity: Option<FileIdentity>,
    applied_at_unix_ms: u64,
) -> Result<Connection, SqliteStoreError> {
    open_index_writer_with_identity_and_hook(
        path,
        expected_identity,
        applied_at_unix_ms,
        None,
        None,
        || {},
    )
}

fn open_index_writer_with_identity_until(
    path: &Path,
    expected_identity: Option<FileIdentity>,
    applied_at_unix_ms: u64,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<Connection, SqliteStoreError> {
    open_index_writer_with_identity_and_hook(
        path,
        expected_identity,
        applied_at_unix_ms,
        Some(cancelled),
        Some(deadline),
        || {},
    )
}

fn open_index_writer_with_identity_and_hook(
    path: &Path,
    expected_identity: Option<FileIdentity>,
    applied_at_unix_ms: u64,
    cancelled: Option<Arc<AtomicBool>>,
    deadline: Option<Instant>,
    after_sqlite_open: impl FnOnce(),
) -> Result<Connection, SqliteStoreError> {
    check_startup_control(cancelled.as_deref(), deadline)?;
    validate_runtime()?;
    let database_file = DatabaseFileGuard::open(path, expected_identity.as_ref())?;
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let mut connection = match Connection::open_with_flags(path, flags) {
        Ok(connection) => connection,
        Err(_) => {
            database_file.cleanup_created_path(path)?;
            return Err(SqliteStoreError::OpenFailed);
        }
    };
    after_sqlite_open();
    if let Err(error) = database_file.verify_path(path) {
        drop(connection);
        database_file.cleanup_created_path(path)?;
        return Err(error);
    }
    if let Err(error) = check_startup_control(cancelled.as_deref(), deadline) {
        drop(connection);
        database_file.cleanup_created_path(path)?;
        return Err(error);
    }
    if cancelled.is_some() || deadline.is_some() {
        let progress_cancelled = cancelled.clone();
        if connection
            .progress_handler(
                STARTUP_PROGRESS_INSTRUCTIONS,
                Some(move || {
                    progress_cancelled
                        .as_deref()
                        .is_some_and(|cancelled| cancelled.load(Ordering::Acquire))
                        || deadline.is_some_and(|deadline| Instant::now() >= deadline)
                }),
            )
            .is_err()
        {
            drop(connection);
            database_file.cleanup_created_path(path)?;
            return Err(SqliteStoreError::ConfigurationFailed);
        }
    }
    let initialization = configure_writer(&connection)
        .and_then(|()| {
            check_startup_control(cancelled.as_deref(), deadline)?;
            migrate_or_validate(&mut connection, applied_at_unix_ms)
        })
        .and_then(|()| check_startup_control(cancelled.as_deref(), deadline))
        .map_err(|error| startup_error_at_control(error, cancelled.as_deref(), deadline));
    let clear_result = connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::ConfigurationFailed);
    match (initialization, clear_result) {
        (Err(error), _) | (Ok(()), Err(error)) => {
            drop(connection);
            database_file.cleanup_created_path(path)?;
            return Err(error);
        }
        (Ok(()), Ok(())) => {}
    }
    Ok(connection)
}

fn check_startup_control(
    cancelled: Option<&AtomicBool>,
    deadline: Option<Instant>,
) -> Result<(), SqliteStoreError> {
    if cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Acquire)) {
        Err(SqliteStoreError::Cancelled)
    } else if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        Err(SqliteStoreError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

fn startup_error_at_control(
    error: SqliteStoreError,
    cancelled: Option<&AtomicBool>,
    deadline: Option<Instant>,
) -> SqliteStoreError {
    if cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Acquire)) {
        SqliteStoreError::Cancelled
    } else if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        SqliteStoreError::DeadlineExceeded
    } else {
        error
    }
}

fn database_file_identity(path: &Path) -> Result<Option<FileIdentity>, SqliteStoreError> {
    match FileIdentity::from_path(path) {
        Ok(identity) => Ok(Some(identity)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(SqliteStoreError::OpenFailed),
    }
}

struct DatabaseFileGuard {
    file: File,
    identity: FileIdentity,
    created: bool,
}

impl DatabaseFileGuard {
    fn open(
        path: &Path,
        expected_identity: Option<&FileIdentity>,
    ) -> Result<Self, SqliteStoreError> {
        let (file, created) = match expected_identity {
            Some(_) => (OpenOptions::new().read(true).write(true).open(path), false),
            None => (
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(path),
                true,
            ),
        };
        let file = file.map_err(|source| match source.kind() {
            io::ErrorKind::AlreadyExists | io::ErrorKind::NotFound => {
                SqliteStoreError::DatabaseIdentityChanged
            }
            _ => SqliteStoreError::OpenFailed,
        })?;
        validate_database_file(&file)?;
        let identity =
            FileIdentity::from_file(file.try_clone().map_err(|_| SqliteStoreError::OpenFailed)?)
                .map_err(|_| SqliteStoreError::OpenFailed)?;
        if expected_identity.is_some_and(|expected| expected != &identity) {
            return Err(SqliteStoreError::DatabaseIdentityChanged);
        }
        Ok(Self {
            file,
            identity,
            created,
        })
    }

    fn verify_path(&self, path: &Path) -> Result<(), SqliteStoreError> {
        validate_database_file(&self.file)?;
        let current =
            FileIdentity::from_path(path).map_err(|_| SqliteStoreError::DatabaseIdentityChanged)?;
        if current != self.identity {
            return Err(SqliteStoreError::DatabaseIdentityChanged);
        }
        Ok(())
    }

    fn cleanup_created_path(self, path: &Path) -> Result<(), SqliteStoreError> {
        if !self.created {
            return Ok(());
        }
        self.verify_path(path)
            .map_err(|_| SqliteStoreError::DatabaseStartupCleanupFailed)?;
        drop(self);
        fs::remove_file(path).map_err(|_| SqliteStoreError::DatabaseStartupCleanupFailed)
    }
}

fn validate_database_file(file: &File) -> Result<(), SqliteStoreError> {
    let metadata = file
        .metadata()
        .map_err(|_| SqliteStoreError::DatabaseIdentityChanged)?;
    if !metadata.is_file() {
        return Err(SqliteStoreError::DatabaseIdentityChanged);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        if metadata.nlink() != 1 {
            return Err(SqliteStoreError::DatabaseIdentityChanged);
        }
    }
    Ok(())
}

fn open_index_reader(path: &Path) -> Result<Connection, SqliteStoreError> {
    validate_runtime()?;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let connection =
        Connection::open_with_flags(path, flags).map_err(|_| SqliteStoreError::OpenFailed)?;
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    connection
        .pragma_update(None, "trusted_schema", false)
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    if pragma_i64(&connection, "application_id")? != APPLICATION_ID {
        return Err(SqliteStoreError::ApplicationIdMismatch);
    }
    if pragma_i64(&connection, "user_version")? != SCHEMA_VERSION {
        return Err(SqliteStoreError::SchemaVersionMismatch);
    }
    validate_migration_ledger(&connection)?;
    Ok(connection)
}

fn validate_runtime() -> Result<(), SqliteStoreError> {
    if rusqlite::version_number() < MINIMUM_SQLITE_VERSION {
        return Err(SqliteStoreError::UnsupportedSqliteVersion);
    }
    Ok(())
}

fn configure_writer(connection: &Connection) -> Result<(), SqliteStoreError> {
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    connection
        .pragma_update(None, "trusted_schema", false)
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    let journal_mode: String = connection
        .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    if journal_mode != "wal" {
        return Err(SqliteStoreError::ConfigurationFailed);
    }
    connection
        .pragma_update(None, "wal_autocheckpoint", 0)
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    Ok(())
}

fn migrate_or_validate(
    connection: &mut Connection,
    applied_at_unix_ms: u64,
) -> Result<(), SqliteStoreError> {
    let application_id = pragma_i64(connection, "application_id")?;
    let user_version = pragma_i64(connection, "user_version")?;
    if application_id != 0 && application_id != APPLICATION_ID {
        return Err(SqliteStoreError::ApplicationIdMismatch);
    }
    if !(0..=SCHEMA_VERSION).contains(&user_version)
        || (user_version == 0 && application_id != 0)
        || (user_version != 0 && application_id == 0)
    {
        return Err(SqliteStoreError::SchemaVersionMismatch);
    }
    if user_version > 0 {
        validate_migration_ledger_through(connection, user_version)?;
    }
    for (version, name, sql) in migrations()
        .iter()
        .copied()
        .filter(|(version, _, _)| *version > user_version)
    {
        apply_migration(connection, version, name, sql, applied_at_unix_ms)?;
    }
    validate_migration_ledger(connection)
}

fn pragma_i64(connection: &Connection, name: &str) -> Result<i64, SqliteStoreError> {
    connection
        .pragma_query_value(None, name, |row| row.get(0))
        .map_err(|_| SqliteStoreError::ConfigurationFailed)
}

fn apply_migration(
    connection: &mut Connection,
    version: i64,
    name: &str,
    sql: &str,
    applied_at_unix_ms: u64,
) -> Result<(), SqliteStoreError> {
    let applied_at =
        i64::try_from(applied_at_unix_ms).map_err(|_| SqliteStoreError::MigrationFailed)?;
    let checksum = migration_checksum(sql);
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| SqliteStoreError::MigrationFailed)?;
    transaction
        .execute_batch(sql)
        .map_err(|_| SqliteStoreError::MigrationFailed)?;
    transaction
        .execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at_unix_ms)
             VALUES (?1, ?2, ?3, ?4)",
            params![version, name, checksum.as_slice(), applied_at],
        )
        .map_err(|_| SqliteStoreError::MigrationFailed)?;
    transaction
        .pragma_update(None, "application_id", APPLICATION_ID)
        .map_err(|_| SqliteStoreError::MigrationFailed)?;
    transaction
        .pragma_update(None, "user_version", version)
        .map_err(|_| SqliteStoreError::MigrationFailed)?;
    transaction
        .commit()
        .map_err(|_| SqliteStoreError::MigrationFailed)?;
    Ok(())
}

fn validate_migration_ledger(connection: &Connection) -> Result<(), SqliteStoreError> {
    validate_migration_ledger_through(connection, SCHEMA_VERSION)?;
    let fts5: i64 = connection
        .query_row(
            "SELECT count(*) FROM pragma_compile_options
             WHERE compile_options = 'ENABLE_FTS5'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| SqliteStoreError::Fts5Unavailable)?;
    if fts5 != 1 {
        return Err(SqliteStoreError::Fts5Unavailable);
    }
    Ok(())
}

fn validate_migration_ledger_through(
    connection: &Connection,
    version: i64,
) -> Result<(), SqliteStoreError> {
    let expected = migrations()
        .iter()
        .copied()
        .filter(|(migration_version, _, _)| *migration_version <= version)
        .collect::<Vec<_>>();
    let mut statement = connection
        .prepare(
            "SELECT version, name, checksum
             FROM schema_migrations ORDER BY version",
        )
        .map_err(|_| SqliteStoreError::MigrationLedgerMismatch)?;
    let actual = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map_err(|_| SqliteStoreError::MigrationLedgerMismatch)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SqliteStoreError::MigrationLedgerMismatch)?;
    if actual.len() != expected.len()
        || actual.iter().zip(expected).any(
            |((actual_version, actual_name, actual_checksum), (version, name, sql))| {
                *actual_version != version
                    || actual_name != name
                    || actual_checksum.as_slice() != migration_checksum(sql)
            },
        )
    {
        return Err(SqliteStoreError::MigrationLedgerMismatch);
    }
    Ok(())
}

const fn migrations() -> [(i64, &'static str, &'static str); 3] {
    [
        (1, MIGRATION_1_NAME, MIGRATION_1),
        (2, MIGRATION_2_NAME, MIGRATION_2),
        (3, MIGRATION_3_NAME, MIGRATION_3),
    ]
}

fn migration_checksum(sql: &str) -> [u8; 32] {
    Sha256::digest(sql.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
        time::{Duration, Instant},
    };

    use rusqlite::{Connection, OpenFlags};

    use super::{
        APPLICATION_ID, MIGRATION_1, MIGRATION_1_NAME, MIGRATION_2, MIGRATION_2_NAME, MIGRATION_3,
        MIGRATION_3_NAME, SCHEMA_VERSION, SqliteStoreError, apply_migration,
        database_file_identity, migration_checksum, open_index_writer,
        open_index_writer_with_identity_and_hook,
    };

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "repowitness-schema-{}-{ordinal}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("fixture directory should be created");
            Self(path)
        }

        fn database(&self) -> PathBuf {
            self.0.join("index.sqlite3")
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn raw_connection(path: &Path) -> Connection {
        Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .expect("fixture database should reopen")
    }

    #[test]
    fn migration_checksums_are_stable_golden_vectors() {
        assert_eq!(
            migration_checksum(MIGRATION_1),
            [
                0x47, 0x9a, 0xdd, 0x59, 0xe4, 0xaa, 0x5f, 0x9d, 0x2c, 0xbf, 0xc7, 0xe0, 0x8e, 0x26,
                0x08, 0x11, 0x2d, 0xdc, 0x96, 0xe7, 0x3f, 0xa9, 0x23, 0x2f, 0x6e, 0xd0, 0xdd, 0x13,
                0x36, 0x1c, 0x9e, 0xca,
            ]
        );
        assert_eq!(
            migration_checksum(MIGRATION_2),
            [
                0xcc, 0xa6, 0x3a, 0xdf, 0x66, 0x8c, 0xc9, 0xe1, 0x60, 0x04, 0x49, 0x26, 0x9c, 0xd8,
                0xd8, 0x1d, 0x6c, 0xa4, 0x2f, 0x97, 0xa6, 0xde, 0xe5, 0xf8, 0x95, 0xd1, 0x3c, 0xb6,
                0x67, 0x95, 0x7d, 0xaf,
            ]
        );
        assert_eq!(
            migration_checksum(MIGRATION_3),
            [
                0xB3, 0x40, 0x92, 0x12, 0xAD, 0xEB, 0xC4, 0xC9, 0xA9, 0xF4, 0x43, 0xFD, 0x9A, 0x1C,
                0x4C, 0xAC, 0x34, 0x3A, 0xEC, 0x21, 0x86, 0x7E, 0xC2, 0x73, 0x33, 0x5A, 0x11, 0x50,
                0x63, 0x60, 0xA9, 0xE6,
            ]
        );
    }

    #[test]
    fn fresh_database_has_exact_identity_ledger_and_required_schema() {
        let directory = TempDirectory::new();
        let connection =
            open_index_writer(&directory.database(), 123).expect("migration should succeed");

        let application_id: i64 = connection
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .expect("application ID should be readable");
        let user_version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version should be readable");
        let ledger = {
            let mut statement = connection
                .prepare(
                    "SELECT version, name, checksum, applied_at_unix_ms
                     FROM schema_migrations ORDER BY version",
                )
                .expect("migration ledger should be readable");
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })
                .expect("migration ledger should be queryable")
                .collect::<Result<Vec<_>, _>>()
                .expect("migration ledger rows should decode")
        };
        let tables: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema
                 WHERE name IN (
                    'workspaces', 'source_snapshots', 'source_manifest_entries',
                    'analysis_artifacts', 'artifact_facts', 'index_generations',
                    'generation_files', 'generation_facts', 'generation_search',
                    'generation_search_rebuild', 'search_projection_state'
                 )",
                [],
                |row| row.get(0),
            )
            .expect("schema should be introspectable");

        assert_eq!(application_id, APPLICATION_ID);
        assert_eq!(user_version, SCHEMA_VERSION);
        assert_eq!(
            ledger,
            vec![
                (
                    1,
                    MIGRATION_1_NAME.to_owned(),
                    migration_checksum(MIGRATION_1).to_vec(),
                    123
                ),
                (
                    2,
                    MIGRATION_2_NAME.to_owned(),
                    migration_checksum(MIGRATION_2).to_vec(),
                    123
                ),
                (
                    3,
                    MIGRATION_3_NAME.to_owned(),
                    migration_checksum(MIGRATION_3).to_vec(),
                    123
                )
            ]
        );
        assert_eq!(tables, 11);
        let payload_column: (String, i64) = connection
            .query_row(
                "SELECT type, [notnull] FROM pragma_table_info('analysis_artifacts')
                 WHERE name = 'payload_digest'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("artifact payload column should be present");
        assert_eq!(payload_column, ("BLOB".to_owned(), 0));
    }

    #[cfg(unix)]
    #[test]
    fn writer_revalidates_the_guarded_file_after_sqlite_opens() {
        let directory = TempDirectory::new();
        let database = directory.database();
        drop(open_index_writer(&database, 123).expect("seed migration should succeed"));
        let replacement = directory.0.join("replacement.sqlite3");
        fs::copy(&database, &replacement).expect("replacement should be copied");
        let expected_identity =
            database_file_identity(&database).expect("database identity should be captured");
        let displaced = directory.0.join("displaced.sqlite3");
        let original_bytes = fs::read(&database).expect("seed database should be readable");
        let replacement_bytes = fs::read(&replacement).expect("replacement should be readable");

        let error = open_index_writer_with_identity_and_hook(
            &database,
            expected_identity,
            456,
            None,
            None,
            || {
                fs::rename(&database, &displaced).expect("opened database should be displaced");
                fs::rename(&replacement, &database)
                    .expect("replacement should occupy the database path");
            },
        )
        .expect_err("a post-open path replacement must fail before configuration");

        assert_eq!(error, SqliteStoreError::DatabaseIdentityChanged);
        assert_eq!(
            fs::read(&displaced).expect("displaced database should be readable"),
            original_bytes
        );
        assert_eq!(
            fs::read(&database).expect("replacement database should be readable"),
            replacement_bytes
        );
    }

    #[cfg(unix)]
    #[test]
    fn writer_guard_rejects_a_hard_link_before_sqlite_can_write() {
        let directory = TempDirectory::new();
        let database = directory.database();
        drop(open_index_writer(&database, 123).expect("seed migration should succeed"));
        let original_bytes = fs::read(&database).expect("seed database should be readable");
        fs::hard_link(&database, directory.0.join("database-alias"))
            .expect("database hard link should be created");
        let expected_identity =
            database_file_identity(&database).expect("database identity should be captured");

        let error = open_index_writer_with_identity_and_hook(
            &database,
            expected_identity,
            456,
            None,
            None,
            || {},
        )
        .expect_err("a multiply linked database must fail before SQLite opens");

        assert_eq!(error, SqliteStoreError::DatabaseIdentityChanged);
        assert_eq!(
            fs::read(&database).expect("database should remain readable"),
            original_bytes
        );
    }

    #[test]
    fn writer_startup_cancellation_after_open_prevents_configuration_writes() {
        let directory = TempDirectory::new();
        let database = directory.database();
        drop(open_index_writer(&database, 123).expect("seed migration should succeed"));
        let expected_identity =
            database_file_identity(&database).expect("database identity should be captured");
        let original_bytes = fs::read(&database).expect("seed database should be readable");
        let cancelled = Arc::new(AtomicBool::new(false));
        let hook_cancelled = Arc::clone(&cancelled);
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(5))
            .expect("test deadline should be representable");

        let error = open_index_writer_with_identity_and_hook(
            &database,
            expected_identity,
            456,
            Some(cancelled),
            Some(deadline),
            move || hook_cancelled.store(true, Ordering::Release),
        )
        .expect_err("post-open cancellation must fail before connection configuration");

        assert_eq!(error, SqliteStoreError::Cancelled);
        assert_eq!(
            fs::read(&database).expect("database should remain readable"),
            original_bytes
        );
    }

    #[test]
    fn cancelled_new_database_startup_removes_only_its_reserved_file() {
        let directory = TempDirectory::new();
        let database = directory.database();
        let expected_identity =
            database_file_identity(&database).expect("missing identity should be captured");
        assert!(expected_identity.is_none());
        let cancelled = Arc::new(AtomicBool::new(false));
        let hook_cancelled = Arc::clone(&cancelled);
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(5))
            .expect("test deadline should be representable");

        let error = open_index_writer_with_identity_and_hook(
            &database,
            expected_identity,
            123,
            Some(cancelled),
            Some(deadline),
            move || hook_cancelled.store(true, Ordering::Release),
        )
        .expect_err("cancelled new startup should fail");

        assert_eq!(error, SqliteStoreError::Cancelled);
        assert!(!database.exists());
        assert!(!directory.0.join("index.sqlite3-wal").exists());
        assert!(!directory.0.join("index.sqlite3-shm").exists());
    }

    #[test]
    fn reopening_is_idempotent_and_preserves_the_original_ledger() {
        let directory = TempDirectory::new();
        drop(open_index_writer(&directory.database(), 123).expect("migration should succeed"));
        let connection =
            open_index_writer(&directory.database(), 456).expect("reopen should validate");
        let applied_at: (i64, i64, i64) = connection
            .query_row(
                "SELECT count(*), min(applied_at_unix_ms), max(applied_at_unix_ms)
                 FROM schema_migrations",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("ledger should remain readable");

        assert_eq!(applied_at, (3, 123, 123));
    }

    #[test]
    fn version_one_upgrades_without_rewriting_its_projection_or_ledger() {
        let directory = TempDirectory::new();
        let mut connection = Connection::open(directory.database())
            .expect("version-one fixture database should open");
        apply_migration(&mut connection, 1, MIGRATION_1_NAME, MIGRATION_1, 123)
            .expect("version one should be created");
        connection
            .execute(
                "INSERT INTO generation_search(
                    generation_id, repository_path, fact_ordinal,
                    content_digest, artifact_digest, name_start, name_end,
                    declaration_start, declaration_end, kind, name, qualified_name
                 ) VALUES (1, X'61', 0, zeroblob(32), zeroblob(32), 0, 1, 0, 1,
                           'function', 'kept', 'kept')",
                [],
            )
            .expect("version-one projection row should be inserted");
        drop(connection);

        let connection =
            open_index_writer(&directory.database(), 456).expect("upgrade should succeed");
        let facts: i64 = connection
            .query_row("SELECT count(*) FROM generation_search", [], |row| {
                row.get(0)
            })
            .expect("original projection should remain readable");
        let projection: (i64, i64) = connection
            .query_row(
                "SELECT active_slot,
                        (SELECT count(*) FROM generation_search_rebuild)
                 FROM search_projection_state WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("projection state should be initialized");
        let ledger: Vec<(i64, i64)> = {
            let mut statement = connection
                .prepare(
                    "SELECT version, applied_at_unix_ms
                     FROM schema_migrations ORDER BY version",
                )
                .expect("ledger should be readable");
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .expect("ledger should be queryable")
                .collect::<Result<_, _>>()
                .expect("ledger should decode")
        };

        assert_eq!(facts, 1);
        assert_eq!(projection, (0, 0));
        assert_eq!(ledger, vec![(1, 123), (2, 456), (3, 456)]);
    }

    #[test]
    fn version_two_artifacts_upgrade_with_explicit_null_payload_identity() {
        let directory = TempDirectory::new();
        let mut connection = Connection::open(directory.database())
            .expect("version-two fixture database should open");
        apply_migration(&mut connection, 1, MIGRATION_1_NAME, MIGRATION_1, 123)
            .expect("version one should be created");
        apply_migration(&mut connection, 2, MIGRATION_2_NAME, MIGRATION_2, 123)
            .expect("version two should be created");
        connection
            .execute(
                "INSERT INTO analysis_artifacts(
                    artifact_digest, lifecycle_state, source_content_digest,
                    producer_manifest_digest, configuration_digest,
                    analysis_schema_digest, canonicalization_version,
                    fact_count, visited_nodes, syntax_error_nodes
                 ) VALUES (
                    zeroblob(32), 'complete', zeroblob(32), zeroblob(32),
                    zeroblob(32), zeroblob(32), 1, 0, 1, 0
                 )",
                [],
            )
            .expect("version-two artifact should be inserted");
        drop(connection);

        let connection =
            open_index_writer(&directory.database(), 456).expect("upgrade should succeed");
        let payload_is_null: i64 = connection
            .query_row(
                "SELECT payload_digest IS NULL FROM analysis_artifacts",
                [],
                |row| row.get(0),
            )
            .expect("migrated payload state should be readable");
        let ledger: Vec<(i64, i64)> = {
            let mut statement = connection
                .prepare(
                    "SELECT version, applied_at_unix_ms
                     FROM schema_migrations ORDER BY version",
                )
                .expect("ledger should be readable");
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .expect("ledger should be queryable")
                .collect::<Result<_, _>>()
                .expect("ledger should decode")
        };

        assert_eq!(payload_is_null, 1);
        assert_eq!(ledger, vec![(1, 123), (2, 123), (3, 456)]);
    }

    #[test]
    fn wrong_identity_version_and_ledger_fail_closed() {
        for (pragma, value, expected) in [
            ("application_id", 7, SqliteStoreError::ApplicationIdMismatch),
            ("user_version", 99, SqliteStoreError::SchemaVersionMismatch),
        ] {
            let directory = TempDirectory::new();
            drop(open_index_writer(&directory.database(), 123).expect("migration should succeed"));
            let connection = raw_connection(&directory.database());
            connection
                .pragma_update(None, pragma, value)
                .expect("fixture pragma should change");
            drop(connection);
            let error = open_index_writer(&directory.database(), 456)
                .expect_err("mismatched identity or version should fail");
            assert_eq!(error, expected);
        }

        let directory = TempDirectory::new();
        drop(open_index_writer(&directory.database(), 123).expect("migration should succeed"));
        let connection = raw_connection(&directory.database());
        connection
            .execute("UPDATE schema_migrations SET checksum = zeroblob(32)", [])
            .expect("fixture ledger should change");
        drop(connection);
        let error = open_index_writer(&directory.database(), 456)
            .expect_err("a changed ledger should fail");
        assert_eq!(error, SqliteStoreError::MigrationLedgerMismatch);
    }

    #[test]
    fn errors_are_stable_and_redacted() {
        let diagnostics = [
            SqliteStoreError::UnsupportedSqliteVersion,
            SqliteStoreError::OpenFailed,
            SqliteStoreError::ConfigurationFailed,
            SqliteStoreError::ApplicationIdMismatch,
            SqliteStoreError::SchemaVersionMismatch,
            SqliteStoreError::MigrationLedgerMismatch,
            SqliteStoreError::MigrationFailed,
            SqliteStoreError::Fts5Unavailable,
            SqliteStoreError::MutationLeaseUnavailable,
            SqliteStoreError::DatabaseIdentityChanged,
            SqliteStoreError::RecoveryGenerationLimitExceeded,
            SqliteStoreError::DatabaseStartupCleanupFailed,
            SqliteStoreError::DatabaseOperationFailed,
            SqliteStoreError::CountNotRepresentable,
            SqliteStoreError::StaleSourceEpoch,
            SqliteStoreError::InvalidSourceEpoch,
            SqliteStoreError::PreparedIdentityMismatch,
            SqliteStoreError::IntegrityCheckFailed,
            SqliteStoreError::GenerationUnavailable,
            SqliteStoreError::Cancelled,
            SqliteStoreError::DeadlineExceeded,
            SqliteStoreError::QueueFull,
            SqliteStoreError::WorkerUnavailable,
            SqliteStoreError::WorkerPanicked,
            SqliteStoreError::ReplyTimeout,
            SqliteStoreError::InvalidSearchLimits,
            SqliteStoreError::InvalidProjectionRebuildLimits,
            SqliteStoreError::ProjectionRebuildRowLimitExceeded,
            SqliteStoreError::InvalidSearchQuery,
            SqliteStoreError::SearchOutputLimitExceeded,
            SqliteStoreError::ArtifactReuseLimitExceeded,
            SqliteStoreError::InvalidBackupLimits,
            SqliteStoreError::BackupDestinationUnavailable,
            SqliteStoreError::BackupFailed,
            SqliteStoreError::BackupStepLimitExceeded,
            SqliteStoreError::BackupCleanupFailed,
        ];
        for error in diagnostics {
            let display = error.to_string();
            assert!(!display.contains('/'));
            assert!(!display.contains("sqlite_schema"));
        }
    }
}
