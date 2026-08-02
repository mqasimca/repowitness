//! Test-only SQLite generation, recovery, checkpoint, and backup spike.
//!
//! This fixture deliberately does not define the production persistence API.
//! It validates the accepted storage invariants before that boundary is made
//! stable.

use std::{
    env,
    error::Error,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, backup::Backup, params};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const BACKUP_CANCELLATION_DEADLINE: Duration = Duration::from_secs(2);
const BACKUP_CANCELLATION_TRIGGER_DELAY: Duration = Duration::from_millis(100);
const BACKUP_MAX_STEPS: u32 = 4_096;
const BACKUP_MAX_WAL_BYTES: u64 = 16 * 1024 * 1024;
const BACKUP_WORKER_DEADLINE: Duration = Duration::from_secs(5);
const OWNED_CHECKPOINT_DEADLINE: Duration = Duration::from_secs(2);
const OWNED_MAX_FACTS_PER_GENERATION: usize = 512;
const OWNED_MAX_WAL_BYTES: u64 = 4 * 1024 * 1024;
const OWNED_READER_DEADLINE: Duration = Duration::from_secs(5);
const OWNED_REPLY_TIMEOUT: Duration = Duration::from_secs(15);
const OWNED_WORKER_POLL_INTERVAL: Duration = Duration::from_millis(10);
const CRASH_CHILD_DATABASE: &str = "REPOWITNESS_SQLITE_SPIKE_DATABASE";
const CRASH_CHILD_SENTINEL: &str = "REPOWITNESS_SQLITE_SPIKE_SENTINEL";
const CRASH_CHILD_STATE: &str = "REPOWITNESS_SQLITE_SPIKE_STATE";
const FIXED_WAL_SQLITE_VERSION: i32 = 3_051_003;
const INCOMPLETE_STATES: [&str; 5] = [
    "discovered",
    "extracting",
    "resolving",
    "validating",
    "ready",
];
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS workspaces (
    workspace_id INTEGER PRIMARY KEY CHECK (workspace_id > 0),
    active_generation_id INTEGER
) STRICT;

CREATE TABLE IF NOT EXISTS generations (
    generation_id INTEGER PRIMARY KEY CHECK (generation_id > 0),
    workspace_id INTEGER NOT NULL REFERENCES workspaces(workspace_id),
    source_epoch INTEGER NOT NULL CHECK (source_epoch >= 0),
    lifecycle_state TEXT NOT NULL CHECK (
        lifecycle_state IN (
            'discovered',
            'extracting',
            'resolving',
            'validating',
            'ready',
            'active',
            'retained',
            'failed',
            'cancelled'
        )
    )
) STRICT;

CREATE UNIQUE INDEX IF NOT EXISTS one_active_generation_per_workspace
ON generations(workspace_id)
WHERE lifecycle_state = 'active';

CREATE TABLE IF NOT EXISTS generation_facts (
    generation_id INTEGER NOT NULL REFERENCES generations(generation_id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    payload BLOB NOT NULL CHECK (length(payload) <= 4096),
    PRIMARY KEY (generation_id, ordinal)
) STRICT, WITHOUT ROWID;
"#;

static TEMP_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempDirectory {
    path: PathBuf,
}

impl TempDirectory {
    fn new() -> io::Result<Self> {
        let sequence = TEMP_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "repowitness-sqlite-spike-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _result = fs::remove_dir_all(&self.path);
    }
}

fn configure_common(connection: &Connection) -> rusqlite::Result<()> {
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "trusted_schema", false)?;
    Ok(())
}

fn open_file_database(path: &Path) -> rusqlite::Result<Connection> {
    open_file_database_with_synchronous(path, "FULL")
}

fn open_file_database_with_synchronous(
    path: &Path,
    synchronous: &str,
) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;
    configure_common(&connection)?;
    let journal_mode: String =
        connection.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
    assert_eq!(journal_mode, "wal", "the spike requires WAL mode");
    connection.pragma_update(None, "synchronous", synchronous)?;
    connection.pragma_update(None, "wal_autocheckpoint", 0)?;
    connection.execute_batch(SCHEMA)?;
    Ok(connection)
}

fn open_memory_database() -> rusqlite::Result<Connection> {
    let connection = Connection::open_in_memory()?;
    configure_common(&connection)?;
    connection.execute_batch(SCHEMA)?;
    Ok(connection)
}

fn open_read_database(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;
    configure_common(&connection)?;
    connection.pragma_update(None, "query_only", true)?;
    Ok(connection)
}

fn bootstrap_workspace(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO workspaces(workspace_id, active_generation_id) VALUES (1, NULL)",
        [],
    )?;
    Ok(())
}

fn begin_generation(
    connection: &Connection,
    generation_id: i64,
    source_epoch: i64,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO generations(
            generation_id,
            workspace_id,
            source_epoch,
            lifecycle_state
         ) VALUES (?1, 1, ?2, 'discovered')",
        params![generation_id, source_epoch],
    )?;
    Ok(())
}

fn advance_generation(
    connection: &Connection,
    generation_id: i64,
    expected: &str,
    next: &str,
) -> rusqlite::Result<()> {
    let changed = connection.execute(
        "UPDATE generations
         SET lifecycle_state = ?1
         WHERE generation_id = ?2 AND lifecycle_state = ?3",
        params![next, generation_id, expected],
    )?;
    if changed != 1 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    Ok(())
}

fn advance_to(connection: &Connection, generation_id: i64, target: &str) -> rusqlite::Result<()> {
    for (expected, next) in [
        ("discovered", "extracting"),
        ("extracting", "resolving"),
        ("resolving", "validating"),
        ("validating", "ready"),
    ] {
        if expected == target {
            break;
        }
        advance_generation(connection, generation_id, expected, next)?;
        if next == target {
            break;
        }
    }
    Ok(())
}

fn write_facts_in_bounded_batches(
    connection: &mut Connection,
    generation_id: i64,
    facts: &[Vec<u8>],
    batch_limit: usize,
) -> rusqlite::Result<()> {
    assert!(batch_limit > 0, "the test batch limit must be positive");
    for (chunk_index, chunk) in facts.chunks(batch_limit).enumerate() {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO generation_facts(generation_id, ordinal, payload)
                 VALUES (?1, ?2, ?3)",
            )?;
            for (item_index, payload) in chunk.iter().enumerate() {
                let ordinal = chunk_index
                    .checked_mul(batch_limit)
                    .and_then(|offset| offset.checked_add(item_index))
                    .and_then(|value| i64::try_from(value).ok())
                    .expect("bounded test fact ordinal must fit i64");
                statement.execute(params![generation_id, ordinal, payload])?;
            }
        }
        transaction.commit()?;
    }
    Ok(())
}

fn stage_ready_generation(
    connection: &mut Connection,
    generation_id: i64,
    source_epoch: i64,
    facts: &[Vec<u8>],
    batch_limit: usize,
) -> rusqlite::Result<()> {
    begin_generation(connection, generation_id, source_epoch)?;
    advance_to(connection, generation_id, "ready")?;
    write_facts_in_bounded_batches(connection, generation_id, facts, batch_limit)
}

fn activate_generation(
    connection: &mut Connection,
    generation_id: i64,
    expected_source_epoch: i64,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = active_generation_id(&transaction)?;
    if let Some(current) = current {
        let changed = transaction.execute(
            "UPDATE generations
             SET lifecycle_state = 'retained'
             WHERE generation_id = ?1 AND lifecycle_state = 'active'",
            [current],
        )?;
        if changed != 1 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
    }
    let changed = transaction.execute(
        "UPDATE generations
         SET lifecycle_state = 'active'
         WHERE generation_id = ?1
           AND workspace_id = 1
           AND source_epoch = ?2
           AND lifecycle_state = 'ready'",
        params![generation_id, expected_source_epoch],
    )?;
    if changed != 1 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    transaction.execute(
        "UPDATE workspaces
         SET active_generation_id = ?1
         WHERE workspace_id = 1",
        [generation_id],
    )?;
    transaction.commit()
}

fn active_generation_id(connection: &Connection) -> rusqlite::Result<Option<i64>> {
    connection.query_row(
        "SELECT active_generation_id FROM workspaces WHERE workspace_id = 1",
        [],
        |row| row.get(0),
    )
}

fn generation_state(
    connection: &Connection,
    generation_id: i64,
) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(
            "SELECT lifecycle_state
             FROM generations
             WHERE generation_id = ?1",
            [generation_id],
            |row| row.get(0),
        )
        .optional()
}

fn generation_facts(connection: &Connection, generation_id: i64) -> rusqlite::Result<Vec<Vec<u8>>> {
    let mut statement = connection.prepare(
        "SELECT payload
         FROM generation_facts
         WHERE generation_id = ?1
         ORDER BY ordinal",
    )?;
    statement
        .query_map([generation_id], |row| row.get(0))?
        .collect()
}

fn recover_incomplete_generations(connection: &mut Connection) -> rusqlite::Result<usize> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        "DELETE FROM generation_facts
         WHERE generation_id IN (
             SELECT generation_id
             FROM generations
             WHERE lifecycle_state IN (
                 'discovered',
                 'extracting',
                 'resolving',
                 'validating',
                 'ready'
             )
         )",
        [],
    )?;
    let recovered = transaction.execute(
        "UPDATE generations
         SET lifecycle_state = 'failed'
         WHERE lifecycle_state IN (
             'discovered',
             'extracting',
             'resolving',
             'validating',
             'ready'
         )",
        [],
    )?;
    transaction.commit()?;
    Ok(recovered)
}

fn fact_fixture(count: usize, payload_bytes: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|index| {
            let mut payload = vec![0_u8; payload_bytes];
            let identity = u64::try_from(index)
                .expect("fixture index fits u64")
                .to_be_bytes();
            let prefix_length = identity.len().min(payload.len());
            payload[..prefix_length].copy_from_slice(&identity[..prefix_length]);
            payload
        })
        .collect()
}

fn backup_database(source: &Connection, destination_path: &Path) -> rusqlite::Result<()> {
    let mut destination = Connection::open(destination_path)?;
    {
        let backup = Backup::new(source, &mut destination)?;
        backup.run_to_completion(16, Duration::from_millis(1), None)?;
    }
    Ok(())
}

fn wal_path(database_path: &Path) -> PathBuf {
    let mut path = OsString::from(database_path.as_os_str());
    path.push("-wal");
    PathBuf::from(path)
}

fn truncate_checkpoint(connection: &Connection) -> rusqlite::Result<(i64, i64, i64)> {
    connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })
}

include!("sqlite_generation_spike/owned_workers.rs");
include!("sqlite_generation_spike/core_tests.rs");
include!("sqlite_generation_spike/recovery_tests.rs");
include!("sqlite_generation_spike/backup_tests.rs");
include!("sqlite_generation_spike/benchmark_tests.rs");
