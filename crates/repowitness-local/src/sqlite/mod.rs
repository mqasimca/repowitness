mod backup;
pub(crate) mod memory_projection;
mod memory_reader;
pub(crate) mod memory_review;
mod reader;
mod schema;
mod worker;
mod writer;

use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
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
    APPLICATION_ID, MIGRATION_1, MIGRATION_1_NAME, MIGRATION_2, MIGRATION_2_NAME, SCHEMA_VERSION,
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
    /// The requested repository workspace is not registered.
    WorkspaceUnavailable,
    /// Supplied memory import values failed adapter-boundary validation.
    InvalidMemoryImport,
    /// A correspondence review selector or target was not exact and current.
    InvalidMemoryCorrespondenceReview,
    /// Correspondence review history exceeded its fixed per-evidence bound.
    MemoryCorrespondenceReviewLimitExceeded,
    /// The supplied source epoch was not the current workspace epoch.
    StaleSourceEpoch,
    /// A requested source epoch transition was not monotonic.
    InvalidSourceEpoch,
    /// No complete memory projection matches the current active source generation.
    MemoryProjectionUnavailable,
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
    /// Memory-projection load or result limits are invalid.
    InvalidMemoryProjectionLimits,
    /// Memory projection input or output exceeded a declared complete bound.
    MemoryProjectionLimitExceeded,
    /// A prepared memory projection violates the adapter contract.
    InvalidMemoryProjection,
    /// Authoritative searchable facts exceed the requested rebuild row limit.
    ProjectionRebuildRowLimitExceeded,
    /// The untrusted lexical query violates the literal query profile.
    InvalidSearchQuery,
    /// Search results exceeded the encoded-output byte limit.
    SearchOutputLimitExceeded,
    /// Memory recall exceeded its conservative encoded-output byte limit.
    MemoryRecallOutputLimitExceeded,
    /// Memory recall exceeded its canonical-record read byte limit.
    MemoryRecallScanLimitExceeded,
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
            Self::WorkspaceUnavailable => "SQLite workspace is unavailable",
            Self::InvalidMemoryImport => "memory import input is invalid",
            Self::InvalidMemoryCorrespondenceReview => {
                "memory correspondence review input is invalid"
            }
            Self::MemoryCorrespondenceReviewLimitExceeded => {
                "memory correspondence review limit exceeded"
            }
            Self::StaleSourceEpoch => "SQLite source epoch is stale",
            Self::InvalidSourceEpoch => "SQLite source epoch transition is invalid",
            Self::MemoryProjectionUnavailable => "SQLite current-memory projection is unavailable",
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
            Self::InvalidMemoryProjectionLimits => "SQLite memory projection limits are invalid",
            Self::MemoryProjectionLimitExceeded => "SQLite memory projection limit exceeded",
            Self::InvalidMemoryProjection => "SQLite memory projection input is invalid",
            Self::ProjectionRebuildRowLimitExceeded => {
                "SQLite search projection rebuild row limit exceeded"
            }
            Self::InvalidSearchQuery => "SQLite search query is invalid",
            Self::SearchOutputLimitExceeded => "SQLite search output byte limit exceeded",
            Self::MemoryRecallOutputLimitExceeded => {
                "SQLite memory-recall output byte limit exceeded"
            }
            Self::MemoryRecallScanLimitExceeded => {
                "SQLite memory-recall canonical scan byte limit exceeded"
            }
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
    let path = canonical_database_path(path)?;
    let database_file = DatabaseFileGuard::open(&path, expected_identity.as_ref())?;
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let mut connection = match Connection::open_with_flags(&path, flags) {
        Ok(connection) => connection,
        Err(_) => {
            database_file.cleanup_created_path(&path)?;
            return Err(SqliteStoreError::OpenFailed);
        }
    };
    after_sqlite_open();
    if let Err(error) = database_file.verify_path(&path) {
        drop(connection);
        database_file.cleanup_created_path(&path)?;
        return Err(error);
    }
    if let Err(error) = check_startup_control(cancelled.as_deref(), deadline) {
        drop(connection);
        database_file.cleanup_created_path(&path)?;
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
            database_file.cleanup_created_path(&path)?;
            return Err(SqliteStoreError::ConfigurationFailed);
        }
    }
    let database_created = database_file.created;
    let initialization = configure_writer_session(&connection)
        .and_then(|()| {
            check_startup_control(cancelled.as_deref(), deadline)?;
            migrate_or_validate(&mut connection, database_created, applied_at_unix_ms)
        })
        .and_then(|()| check_startup_control(cancelled.as_deref(), deadline))
        .map_err(|error| startup_error_at_control(error, cancelled.as_deref(), deadline));
    let clear_result = connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::ConfigurationFailed);
    match (initialization, clear_result) {
        (Err(error), _) | (Ok(()), Err(error)) => {
            drop(connection);
            database_file.cleanup_created_path(&path)?;
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

fn canonical_database_path(path: &Path) -> Result<PathBuf, SqliteStoreError> {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let parent = fs::canonicalize(parent).map_err(|_| SqliteStoreError::OpenFailed)?;
    let file_name = path.file_name().ok_or(SqliteStoreError::OpenFailed)?;
    Ok(parent.join(file_name))
}

fn open_index_reader(path: &Path) -> Result<Connection, SqliteStoreError> {
    validate_runtime()?;
    let path = canonical_database_path(path)?;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let connection =
        Connection::open_with_flags(&path, flags).map_err(|_| SqliteStoreError::OpenFailed)?;
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

fn configure_writer_session(connection: &Connection) -> Result<(), SqliteStoreError> {
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    connection
        .pragma_update(None, "trusted_schema", false)
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    Ok(())
}

fn configure_writer_journal(connection: &Connection) -> Result<(), SqliteStoreError> {
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
    database_created: bool,
    applied_at_unix_ms: u64,
) -> Result<(), SqliteStoreError> {
    let application_id = pragma_i64(connection, "application_id")?;
    let user_version = pragma_i64(connection, "user_version")?;
    let expected_application_id = if database_created { 0 } else { APPLICATION_ID };
    if application_id != expected_application_id {
        return Err(SqliteStoreError::ApplicationIdMismatch);
    }
    let valid_version = if database_created {
        user_version == 0
    } else {
        (1..=SCHEMA_VERSION).contains(&user_version)
    };
    if !valid_version {
        return Err(SqliteStoreError::SchemaVersionMismatch);
    }
    if user_version > 0 {
        validate_migration_ledger_through(connection, user_version)?;
    }
    configure_writer_journal(connection)?;
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

const fn migrations() -> [(i64, &'static str, &'static str); 2] {
    [
        (1, MIGRATION_1_NAME, MIGRATION_1),
        (2, MIGRATION_2_NAME, MIGRATION_2),
    ]
}

fn migration_checksum(sql: &str) -> [u8; 32] {
    Sha256::digest(sql.as_bytes()).into()
}

#[cfg(test)]
mod tests;
