mod backup;
mod doctor;
mod error;
mod graph;
pub(crate) mod memory_projection;
mod memory_reader;
pub(crate) mod memory_review;
mod reader;
mod retention;
mod retention_read;
mod schema;
mod scip_overlay;
mod worker;
mod workspace;
mod writer;

use std::{
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

use crate::contained_source::{FileIdentity, file_has_single_link};

pub use self::backup::{
    BackupIdentityStatus, BackupLimits, BackupMaintenanceStatus, BackupOutcome,
    BackupPublicationStatus, create_online_backup,
};
#[cfg(test)]
pub(crate) use self::doctor::create_valid_test_database;
pub(crate) use self::doctor::{inspect_sqlite_environment, validate_database_read_only};
pub use self::error::SqliteStoreError;
pub use self::graph::{
    PreparedRustGraphArtifact, PreparedRustGraphGeneration, RustGraphArchitectureSummary,
    RustGraphAvailability, RustGraphCandidateRecord, RustGraphDefinitionRecord, RustGraphDirection,
    RustGraphEdgeKind, RustGraphEdgeKinds, RustGraphEdgeRecord, RustGraphEvidenceResult,
    RustGraphImpactClass, RustGraphImpactResult, RustGraphImpactedDefinition,
    RustGraphOutcomeRecord, RustGraphPreparationControl, RustGraphPreparationError,
    RustGraphPublicationSummary, RustGraphReadError, RustGraphReadLimits,
    RustGraphRelationshipCardinality, RustGraphSiteSelector, RustGraphSource,
    RustGraphSymbolSearchResult, RustGraphTraceCoverage, RustGraphTraceResult, RustGraphTraceStart,
    RustGraphTraceTruncation, prepare_rust_graph_generation,
};
pub use self::reader::{
    GitHistoryEvidence, KnownAtApplicability, KnownAtEvidenceBasis, KnownAtHistoryCoverage,
    KnownAtHistoryReceipt, KnownAtObservationEvidence, OwnedSqliteReader, SearchHit, SearchLimits,
    SearchResults, SymbolLookupResults,
};
pub use self::retention::*;
pub(crate) use self::retention_read::load_retention_apply_outcome_read_only;
pub use self::retention_read::plan_generation_retention_read_only;
use self::schema::{
    APPLICATION_ID, MIGRATION_1, MIGRATION_1_NAME, MIGRATION_2, MIGRATION_2_NAME, MIGRATION_3,
    MIGRATION_3_NAME, MIGRATION_4, MIGRATION_4_NAME, MIGRATION_5, MIGRATION_5_NAME, SCHEMA_VERSION,
};
pub use self::scip_overlay::{
    MAX_SCIP_OVERLAY_DOCUMENTS, PreparedScipOverlay, ScipEvidenceReadLimits,
    ScipEvidenceReadLimitsError, ScipOccurrenceEvidence, ScipOverlayAvailability,
    ScipOverlayImportScope, ScipOverlayPreparationError, ScipOverlaySummary,
    ScipRelationshipDirection, ScipRelationshipEvidence, ScipSymbolEvidence,
    ScipSymbolEvidenceResult, ScipSyntaxSymbolResolution,
};
pub(crate) use self::worker::ObservedMemoryHistoryItem;
pub(crate) use self::worker::{CompletedWorkspaceSource, SqliteMutationLease};
pub use self::worker::{IndexStoreStartup, OwnedSqliteIndex};
pub use self::workspace::{
    MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS, PinnedWorkspaceView, PinnedWorkspaceViewMember,
    SourceSlotGeneration, SourceSlotState, WorkspaceSourceSlot, WorkspaceViewId,
    WorkspaceViewMember,
};
pub use self::writer::{
    CheckpointOutcome, GenerationId, PersonalMemoryReceipt, ProjectionRebuildLimits,
    ProjectionRebuildOutcome, TaskCheckpointReceipt, TaskVerificationReceipt,
};
pub use repowitness_application::RustIndexCoverage as GenerationCoverage;
pub use repowitness_application::SourceSlotEpoch;

const MINIMUM_SQLITE_VERSION: i32 = 3_051_003;
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const STARTUP_PROGRESS_INSTRUCTIONS: i32 = 1_000;

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
) -> Result<(Connection, FileIdentity, bool), SqliteStoreError> {
    open_index_writer_with_identity_and_identity_hooks(
        path,
        expected_identity,
        applied_at_unix_ms,
        Some(cancelled),
        Some(deadline),
        || {},
        |_| {},
    )
}

#[cfg(test)]
fn open_index_writer_with_identity_and_hook(
    path: &Path,
    expected_identity: Option<FileIdentity>,
    applied_at_unix_ms: u64,
    cancelled: Option<Arc<AtomicBool>>,
    deadline: Option<Instant>,
    after_sqlite_open: impl FnOnce(),
) -> Result<Connection, SqliteStoreError> {
    open_index_writer_with_identity_and_identity_hooks(
        path,
        expected_identity,
        applied_at_unix_ms,
        cancelled,
        deadline,
        after_sqlite_open,
        |_| {},
    )
    .map(|(connection, _identity, _migrated)| connection)
}

#[cfg(test)]
fn open_index_writer_with_identity_and_migration_hook(
    path: &Path,
    expected_identity: Option<FileIdentity>,
    applied_at_unix_ms: u64,
    cancelled: Option<Arc<AtomicBool>>,
    deadline: Option<Instant>,
    after_migration: impl FnOnce(bool),
) -> Result<Connection, SqliteStoreError> {
    open_index_writer_with_identity_and_identity_hooks(
        path,
        expected_identity,
        applied_at_unix_ms,
        cancelled,
        deadline,
        || {},
        after_migration,
    )
    .map(|(connection, _identity, _migrated)| connection)
}

fn open_index_writer_with_identity_and_identity_hooks(
    path: &Path,
    expected_identity: Option<FileIdentity>,
    applied_at_unix_ms: u64,
    cancelled: Option<Arc<AtomicBool>>,
    deadline: Option<Instant>,
    after_sqlite_open: impl FnOnce(),
    after_migration: impl FnOnce(bool),
) -> Result<(Connection, FileIdentity, bool), SqliteStoreError> {
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
        .and_then(|migrated| {
            after_migration(migrated);
            check_startup_control(cancelled.as_deref(), deadline)
                .map(|()| migrated)
                .map_err(|error| {
                    if migrated {
                        SqliteStoreError::MutationOutcomeUnknown
                    } else {
                        error
                    }
                })
        })
        .map_err(|error| startup_error_at_control(error, cancelled.as_deref(), deadline));
    let clear_result = connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::ConfigurationFailed);
    let migrated = match (initialization, clear_result) {
        (Err(error), _) => {
            drop(connection);
            if database_file.cleanup_created_path(&path).is_err() {
                return Err(SqliteStoreError::MutationOutcomeUnknown);
            }
            return Err(error);
        }
        (Ok(migrated), Err(error)) => {
            drop(connection);
            if database_file.cleanup_created_path(&path).is_err() || migrated {
                return Err(SqliteStoreError::MutationOutcomeUnknown);
            }
            return Err(error);
        }
        (Ok(migrated), Ok(())) => migrated,
    };
    Ok((connection, database_file.identity, migrated))
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
    if error == SqliteStoreError::MutationOutcomeUnknown {
        error
    } else if cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Acquire)) {
        SqliteStoreError::Cancelled
    } else if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        SqliteStoreError::DeadlineExceeded
    } else {
        error
    }
}

pub(crate) fn database_file_identity(
    path: &Path,
) -> Result<Option<FileIdentity>, SqliteStoreError> {
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
    if !file_has_single_link(file).map_err(|_| SqliteStoreError::DatabaseIdentityChanged)? {
        return Err(SqliteStoreError::DatabaseIdentityChanged);
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
) -> Result<bool, SqliteStoreError> {
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
    let mut migrated = false;
    for (version, name, sql) in migrations()
        .iter()
        .copied()
        .filter(|(version, _, _)| *version > user_version)
    {
        if let Err(error) = apply_migration(connection, version, name, sql, applied_at_unix_ms) {
            return Err(if migrated {
                SqliteStoreError::MutationOutcomeUnknown
            } else {
                error
            });
        }
        migrated = true;
    }
    validate_migration_ledger(connection)
        .map(|()| migrated)
        .map_err(|error| {
            if migrated {
                SqliteStoreError::MutationOutcomeUnknown
            } else {
                error
            }
        })
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
    writer::commit_mutation(transaction)?;
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

const fn migrations() -> [(i64, &'static str, &'static str); 5] {
    [
        (1, MIGRATION_1_NAME, MIGRATION_1),
        (2, MIGRATION_2_NAME, MIGRATION_2),
        (3, MIGRATION_3_NAME, MIGRATION_3),
        (4, MIGRATION_4_NAME, MIGRATION_4),
        (5, MIGRATION_5_NAME, MIGRATION_5),
    ]
}

fn migration_checksum(sql: &str) -> [u8; 32] {
    Sha256::digest(sql.as_bytes()).into()
}

#[cfg(test)]
mod tests;
