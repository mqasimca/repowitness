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
const BACKUP_MAX_STEPS: u32 = 4_096;
const BACKUP_MAX_WAL_BYTES: u64 = 16 * 1024 * 1024;
const BACKUP_WORKER_DEADLINE: Duration = Duration::from_secs(5);
const OWNED_CHECKPOINT_DEADLINE: Duration = Duration::from_secs(2);
const OWNED_MAX_FACTS_PER_GENERATION: usize = 512;
const OWNED_MAX_WAL_BYTES: u64 = 4 * 1024 * 1024;
const OWNED_READER_DEADLINE: Duration = Duration::from_secs(5);
const OWNED_REPLY_TIMEOUT: Duration = Duration::from_secs(5);
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

type OwnedWorkerResult<T> = Result<T, String>;

#[derive(Clone, Copy, Debug)]
struct OwnedCheckpointObservation {
    busy: i64,
    log_frames: i64,
    checkpointed_frames: i64,
    elapsed: Duration,
    wal_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OwnedReaderExit {
    Cancelled,
    DeadlineExceeded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BackupCancellationObservation {
    completed_steps: u32,
    elapsed: Duration,
}

enum OwnedWriterCommand {
    Publish {
        generation_id: i64,
        source_epoch: i64,
        facts: Vec<Vec<u8>>,
        batch_limit: usize,
        reply: mpsc::SyncSender<OwnedWorkerResult<()>>,
    },
    Checkpoint {
        reply: mpsc::SyncSender<OwnedWorkerResult<OwnedCheckpointObservation>>,
    },
    ActiveGeneration {
        reply: mpsc::SyncSender<OwnedWorkerResult<Option<i64>>>,
    },
    Shutdown {
        reply: mpsc::SyncSender<OwnedWorkerResult<()>>,
    },
}

struct OwnedWriterClient {
    commands: mpsc::SyncSender<OwnedWriterCommand>,
}

impl OwnedWriterClient {
    fn command_result<T>(
        &self,
        command: OwnedWriterCommand,
        receiver: &mpsc::Receiver<OwnedWorkerResult<T>>,
    ) -> TestResult<OwnedWorkerResult<T>> {
        self.commands
            .try_send(command)
            .map_err(|_| io::Error::other("owned writer command queue unavailable"))?;
        Ok(receiver.recv_timeout(OWNED_REPLY_TIMEOUT)?)
    }

    fn publish_result(
        &self,
        generation_id: i64,
        source_epoch: i64,
        facts: Vec<Vec<u8>>,
        batch_limit: usize,
    ) -> TestResult<OwnedWorkerResult<()>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.command_result(
            OwnedWriterCommand::Publish {
                generation_id,
                source_epoch,
                facts,
                batch_limit,
                reply,
            },
            &receiver,
        )
    }

    fn publish(
        &self,
        generation_id: i64,
        source_epoch: i64,
        facts: Vec<Vec<u8>>,
        batch_limit: usize,
    ) -> TestResult {
        require_owned_worker_success(self.publish_result(
            generation_id,
            source_epoch,
            facts,
            batch_limit,
        )?)
    }

    fn checkpoint(&self) -> TestResult<OwnedCheckpointObservation> {
        let (reply, receiver) = mpsc::sync_channel(1);
        require_owned_worker_success(
            self.command_result(OwnedWriterCommand::Checkpoint { reply }, &receiver)?,
        )
    }

    fn active_generation(&self) -> TestResult<Option<i64>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        require_owned_worker_success(
            self.command_result(OwnedWriterCommand::ActiveGeneration { reply }, &receiver)?,
        )
    }

    fn shutdown(&self) -> TestResult {
        let (reply, receiver) = mpsc::sync_channel(1);
        require_owned_worker_success(
            self.command_result(OwnedWriterCommand::Shutdown { reply }, &receiver)?,
        )
    }
}

fn require_owned_worker_success<T>(result: OwnedWorkerResult<T>) -> TestResult<T> {
    result.map_err(|message| Box::new(io::Error::other(message)) as Box<dyn Error>)
}

fn send_owned_reply<T>(
    sender: &mpsc::SyncSender<OwnedWorkerResult<T>>,
    result: OwnedWorkerResult<T>,
) -> OwnedWorkerResult<()> {
    sender
        .send(result)
        .map_err(|_| "owned worker reply receiver disconnected".to_owned())
}

fn publish_owned_generation(
    connection: &mut Connection,
    generation_id: i64,
    source_epoch: i64,
    facts: &[Vec<u8>],
    batch_limit: usize,
) -> OwnedWorkerResult<()> {
    if facts.len() > OWNED_MAX_FACTS_PER_GENERATION {
        return Err("owned writer fact limit exceeded".to_owned());
    }
    if batch_limit == 0 || batch_limit > OWNED_MAX_FACTS_PER_GENERATION {
        return Err("owned writer batch limit invalid".to_owned());
    }
    stage_ready_generation(connection, generation_id, source_epoch, facts, batch_limit)
        .and_then(|()| activate_generation(connection, generation_id, source_epoch))
        .map_err(|error| error.to_string())
}

fn observe_owned_checkpoint(
    connection: &Connection,
    source_wal_path: &Path,
) -> OwnedWorkerResult<OwnedCheckpointObservation> {
    let started_at = Instant::now();
    let (busy, log_frames, checkpointed_frames) =
        truncate_checkpoint(connection).map_err(|error| error.to_string())?;
    let wal_bytes = fs::metadata(source_wal_path)
        .map_err(|error| error.to_string())?
        .len();
    Ok(OwnedCheckpointObservation {
        busy,
        log_frames,
        checkpointed_frames,
        elapsed: started_at.elapsed(),
        wal_bytes,
    })
}

fn process_owned_writer_command(
    connection: &mut Connection,
    source_wal_path: &Path,
    command: OwnedWriterCommand,
) -> OwnedWorkerResult<bool> {
    match command {
        OwnedWriterCommand::Publish {
            generation_id,
            source_epoch,
            facts,
            batch_limit,
            reply,
        } => {
            let result = publish_owned_generation(
                connection,
                generation_id,
                source_epoch,
                &facts,
                batch_limit,
            );
            send_owned_reply(&reply, result)?;
            Ok(false)
        }
        OwnedWriterCommand::Checkpoint { reply } => {
            let result = observe_owned_checkpoint(connection, source_wal_path);
            send_owned_reply(&reply, result)?;
            Ok(false)
        }
        OwnedWriterCommand::ActiveGeneration { reply } => {
            let result = active_generation_id(connection).map_err(|error| error.to_string());
            send_owned_reply(&reply, result)?;
            Ok(false)
        }
        OwnedWriterCommand::Shutdown { reply } => {
            send_owned_reply(&reply, Ok(()))?;
            Ok(true)
        }
    }
}

fn run_owned_writer_worker(
    database_path: &Path,
    commands: &mpsc::Receiver<OwnedWriterCommand>,
) -> OwnedWorkerResult<()> {
    let mut connection = open_file_database(database_path).map_err(|error| error.to_string())?;
    let source_wal_path = wal_path(database_path);

    loop {
        match commands.recv_timeout(OWNED_WORKER_POLL_INTERVAL) {
            Ok(command) => {
                if process_owned_writer_command(&mut connection, &source_wal_path, command)? {
                    return Ok(());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

fn run_owned_reader_worker(
    database_path: &Path,
    cancelled: &AtomicBool,
    reader_lifetime: Duration,
    ready: &mpsc::SyncSender<OwnedWorkerResult<Option<i64>>>,
    exited: &mpsc::SyncSender<OwnedWorkerResult<OwnedReaderExit>>,
) -> OwnedWorkerResult<()> {
    let mut connection = open_read_database(database_path).map_err(|error| error.to_string())?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let pinned_generation =
        active_generation_id(&transaction).map_err(|error| error.to_string())?;
    send_owned_reply(ready, Ok(pinned_generation))?;

    let deadline = Instant::now() + reader_lifetime;
    let outcome = loop {
        if cancelled.load(Ordering::Acquire) {
            break OwnedReaderExit::Cancelled;
        }
        if Instant::now() >= deadline {
            break OwnedReaderExit::DeadlineExceeded;
        }
        thread::sleep(OWNED_WORKER_POLL_INTERVAL);
    };
    transaction.commit().map_err(|error| error.to_string())?;
    send_owned_reply(exited, Ok(outcome))
}

fn run_cancellable_backup_worker(
    source_path: &Path,
    destination_path: &Path,
    cancelled: &AtomicBool,
    ready: &mpsc::SyncSender<OwnedWorkerResult<()>>,
) -> OwnedWorkerResult<BackupCancellationObservation> {
    let source = open_read_database(source_path).map_err(|error| error.to_string())?;
    let mut destination = Connection::open(destination_path).map_err(|error| error.to_string())?;
    let backup = Backup::new(&source, &mut destination).map_err(|error| error.to_string())?;
    let started_at = Instant::now();
    let deadline = started_at
        .checked_add(BACKUP_WORKER_DEADLINE)
        .ok_or_else(|| "backup worker deadline is not representable".to_owned())?;
    let first_step = backup.step(1).map_err(|error| error.to_string())?;
    if first_step == rusqlite::backup::StepResult::Done {
        return Err("backup fixture completed before cancellation".to_owned());
    }
    send_owned_reply(ready, Ok(()))?;

    let mut completed_steps = 1_u32;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Ok(BackupCancellationObservation {
                completed_steps,
                elapsed: started_at.elapsed(),
            });
        }
        if Instant::now() >= deadline {
            return Err("backup worker deadline exceeded".to_owned());
        }
        if completed_steps >= BACKUP_MAX_STEPS {
            return Err("backup worker step limit exceeded".to_owned());
        }
        thread::sleep(OWNED_WORKER_POLL_INTERVAL);
        let step = backup.step(1).map_err(|error| error.to_string())?;
        completed_steps += 1;
        if step == rusqlite::backup::StepResult::Done {
            return Err("backup fixture completed before cancellation".to_owned());
        }
    }
}

fn receive_owned_worker_result<T>(
    receiver: &mpsc::Receiver<OwnedWorkerResult<T>>,
) -> TestResult<T> {
    require_owned_worker_success(receiver.recv_timeout(OWNED_REPLY_TIMEOUT)?)
}

fn join_owned_worker(
    worker: thread::JoinHandle<OwnedWorkerResult<()>>,
    worker_name: &str,
) -> TestResult {
    let result = worker
        .join()
        .map_err(|_| io::Error::other(format!("{worker_name} panicked")))?;
    require_owned_worker_success(result)
}

fn wait_for_sentinel(child: &mut Child, sentinel: &Path) -> io::Result<()> {
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(5))
        .expect("short test deadline must be representable");
    loop {
        if sentinel.is_file() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "crash child exited before synchronization with {status}"
            )));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "crash child did not synchronize before the deadline",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(target_os = "linux")]
fn peak_resident_set_kib() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let value = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))?
        .split_ascii_whitespace()
        .next()?;
    value.parse().ok()
}

#[cfg(not(target_os = "linux"))]
fn peak_resident_set_kib() -> Option<u64> {
    None
}

fn benchmark_direct(
    directory: &TempDirectory,
    facts: &[Vec<u8>],
) -> TestResult<(Duration, PathBuf)> {
    let database_path = directory.join("direct-benchmark.sqlite3");
    let started = Instant::now();
    let mut connection = open_file_database(&database_path)?;
    bootstrap_workspace(&connection)?;
    stage_ready_generation(&mut connection, 1, 1, facts, 256)?;
    activate_generation(&mut connection, 1, 1)?;
    let elapsed = started.elapsed();
    drop(connection);
    Ok((elapsed, database_path))
}

fn benchmark_private_ram_first(
    directory: &TempDirectory,
    facts: &[Vec<u8>],
) -> TestResult<(Duration, PathBuf)> {
    let database_path = directory.join("memory-benchmark.sqlite3");
    let started = Instant::now();
    let mut connection = open_memory_database()?;
    bootstrap_workspace(&connection)?;
    stage_ready_generation(&mut connection, 1, 1, facts, 256)?;
    activate_generation(&mut connection, 1, 1)?;
    backup_database(&connection, &database_path)?;
    Ok((started.elapsed(), database_path))
}

fn benchmark_direct_durability_profile(
    directory: &TempDirectory,
    facts: &[Vec<u8>],
    synchronous: &str,
    batch_limit: usize,
    sample: usize,
) -> TestResult<(Duration, u64)> {
    let database_path = directory.join(&format!(
        "durability-{synchronous}-{batch_limit}-{sample}.sqlite3"
    ));
    let source_wal_path = wal_path(&database_path);
    let started_at = Instant::now();
    let mut connection = open_file_database_with_synchronous(&database_path, synchronous)?;
    bootstrap_workspace(&connection)?;
    stage_ready_generation(&mut connection, 1, 1, facts, batch_limit)?;
    activate_generation(&mut connection, 1, 1)?;
    let elapsed = started_at.elapsed();
    assert_eq!(active_generation_id(&connection)?, Some(1));
    assert_eq!(generation_facts(&connection, 1)?, facts);
    Ok((elapsed, fs::metadata(source_wal_path)?.len()))
}

#[test]
fn bundled_sqlite_meets_runtime_and_compile_requirements() -> TestResult {
    eprintln!("bundled SQLite runtime: {}", rusqlite::version());
    assert!(
        rusqlite::version_number() >= FIXED_WAL_SQLITE_VERSION,
        "bundled SQLite {} is older than the WAL-reset-fixed floor",
        rusqlite::version()
    );

    let directory = TempDirectory::new()?;
    let connection = open_file_database(&directory.join("runtime.sqlite3"))?;
    let mut compile_options = Vec::new();
    connection.pragma_query(None, "compile_options", |row| {
        compile_options.push(row.get::<_, String>(0)?);
        Ok(())
    })?;

    assert!(
        compile_options.iter().any(|option| option == "ENABLE_FTS5"),
        "the bundled SQLite must include FTS5"
    );
    assert!(
        compile_options
            .iter()
            .any(|option| option == "THREADSAFE=1"),
        "the bundled SQLite must use serialized thread safety"
    );
    assert_eq!(
        connection.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))?,
        "wal"
    );
    assert_eq!(
        connection.pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0))?,
        1
    );
    assert_eq!(
        connection.pragma_query_value(None, "trusted_schema", |row| row.get::<_, i64>(0))?,
        0
    );
    assert_eq!(
        connection.pragma_query_value(None, "wal_autocheckpoint", |row| row.get::<_, i64>(0))?,
        0
    );
    assert_eq!(
        connection.pragma_query_value(None, "busy_timeout", |row| row.get::<_, i64>(0))?,
        250
    );
    assert_eq!(
        connection.pragma_query_value(None, "synchronous", |row| row.get::<_, i64>(0))?,
        2
    );
    Ok(())
}

#[test]
fn activation_is_atomic_and_readers_pin_one_generation() -> TestResult {
    let directory = TempDirectory::new()?;
    let database_path = directory.join("activation.sqlite3");
    let mut writer = open_file_database(&database_path)?;
    bootstrap_workspace(&writer)?;
    stage_ready_generation(&mut writer, 1, 1, &[b"old".to_vec()], 1)?;
    activate_generation(&mut writer, 1, 1)?;

    let mut reader = open_read_database(&database_path)?;
    let pinned_reader = reader.transaction()?;
    assert_eq!(active_generation_id(&pinned_reader)?, Some(1));
    assert_eq!(generation_facts(&pinned_reader, 1)?, [b"old".to_vec()]);

    stage_ready_generation(&mut writer, 2, 2, &[b"new".to_vec()], 1)?;
    activate_generation(&mut writer, 2, 2)?;

    assert_eq!(active_generation_id(&pinned_reader)?, Some(1));
    assert_eq!(generation_facts(&pinned_reader, 1)?, [b"old".to_vec()]);
    pinned_reader.commit()?;

    assert_eq!(active_generation_id(&reader)?, Some(2));
    assert_eq!(generation_facts(&reader, 2)?, [b"new".to_vec()]);
    assert_eq!(generation_state(&reader, 1)?.as_deref(), Some("retained"));
    assert_eq!(generation_state(&reader, 2)?.as_deref(), Some("active"));
    Ok(())
}

#[test]
fn full_and_normal_preserve_activation_pinning_and_stale_epoch_safety() -> TestResult {
    for synchronous in ["FULL", "NORMAL"] {
        let directory = TempDirectory::new()?;
        let database_path = directory.join(&format!("durability-invariants-{synchronous}.sqlite3"));
        let mut writer = open_file_database_with_synchronous(&database_path, synchronous)?;
        bootstrap_workspace(&writer)?;
        stage_ready_generation(&mut writer, 1, 1, &[b"old".to_vec()], 1)?;
        activate_generation(&mut writer, 1, 1)?;

        let mut reader = open_read_database(&database_path)?;
        let pinned_reader = reader.transaction()?;
        assert_eq!(active_generation_id(&pinned_reader)?, Some(1));

        stage_ready_generation(&mut writer, 2, 2, &[b"new".to_vec()], 1)?;
        activate_generation(&mut writer, 2, 2)?;
        stage_ready_generation(&mut writer, 3, 3, &[b"stale".to_vec()], 1)?;
        assert!(activate_generation(&mut writer, 3, 4).is_err());

        assert_eq!(active_generation_id(&pinned_reader)?, Some(1));
        assert_eq!(generation_facts(&pinned_reader, 1)?, [b"old".to_vec()]);
        pinned_reader.commit()?;
        assert_eq!(active_generation_id(&reader)?, Some(2));
        assert_eq!(generation_facts(&reader, 2)?, [b"new".to_vec()]);
        assert_eq!(generation_state(&reader, 1)?.as_deref(), Some("retained"));
        assert_eq!(generation_state(&reader, 2)?.as_deref(), Some("active"));
        assert_eq!(generation_state(&reader, 3)?.as_deref(), Some("ready"));
    }
    Ok(())
}

#[test]
fn pinned_reader_blocks_truncation_without_blocking_new_generations() -> TestResult {
    let directory = TempDirectory::new()?;
    let database_path = directory.join("checkpoint-starvation.sqlite3");
    let source_wal_path = wal_path(&database_path);
    let mut writer = open_file_database(&database_path)?;
    bootstrap_workspace(&writer)?;
    stage_ready_generation(&mut writer, 1, 1, &fact_fixture(128, 128), 32)?;
    activate_generation(&mut writer, 1, 1)?;

    let (busy, log_frames, checkpointed_frames) = truncate_checkpoint(&writer)?;
    assert_eq!((busy, log_frames, checkpointed_frames), (0, 0, 0));
    assert_eq!(fs::metadata(&source_wal_path)?.len(), 0);

    let mut reader = open_read_database(&database_path)?;
    let pinned_reader = reader.transaction()?;
    assert_eq!(active_generation_id(&pinned_reader)?, Some(1));

    let generation_two_facts = fact_fixture(512, 256);
    stage_ready_generation(&mut writer, 2, 2, &generation_two_facts, 64)?;
    activate_generation(&mut writer, 2, 2)?;
    let wal_bytes_before_checkpoint = fs::metadata(&source_wal_path)?.len();
    assert!(wal_bytes_before_checkpoint > 0);

    let (busy, log_frames, checkpointed_frames) = truncate_checkpoint(&writer)?;
    assert_eq!(busy, 1);
    assert!(log_frames > 0);
    assert!(checkpointed_frames <= log_frames);
    assert!(fs::metadata(&source_wal_path)?.len() > 0);
    assert_eq!(active_generation_id(&pinned_reader)?, Some(1));

    let generation_three_facts = fact_fixture(256, 192);
    stage_ready_generation(&mut writer, 3, 3, &generation_three_facts, 32)?;
    activate_generation(&mut writer, 3, 3)?;
    assert!(fs::metadata(&source_wal_path)?.len() > wal_bytes_before_checkpoint);
    assert_eq!(active_generation_id(&pinned_reader)?, Some(1));
    assert_eq!(active_generation_id(&writer)?, Some(3));

    pinned_reader.commit()?;
    assert_eq!(active_generation_id(&reader)?, Some(3));
    assert_eq!(generation_facts(&reader, 3)?, generation_three_facts);

    let (busy, log_frames, checkpointed_frames) = truncate_checkpoint(&writer)?;
    assert_eq!(busy, 0);
    assert_eq!(log_frames, checkpointed_frames);
    assert_eq!(fs::metadata(source_wal_path)?.len(), 0);
    Ok(())
}

#[test]
fn owned_connections_bound_checkpoint_contention_and_cancel_reader() -> TestResult {
    let directory = TempDirectory::new()?;
    let database_path = directory.join("owned-checkpoint.sqlite3");
    let mut setup = open_file_database(&database_path)?;
    bootstrap_workspace(&setup)?;
    stage_ready_generation(&mut setup, 1, 1, &fact_fixture(128, 128), 32)?;
    activate_generation(&mut setup, 1, 1)?;
    assert_eq!(truncate_checkpoint(&setup)?, (0, 0, 0));
    drop(setup);

    let (command_sender, command_receiver) = mpsc::sync_channel(1);
    let writer_path = database_path.clone();
    let writer_worker =
        thread::spawn(move || run_owned_writer_worker(&writer_path, &command_receiver));
    let writer = OwnedWriterClient {
        commands: command_sender,
    };

    let cancelled = Arc::new(AtomicBool::new(false));
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (exit_sender, exit_receiver) = mpsc::sync_channel(1);
    let reader_path = database_path.clone();
    let reader_cancelled = Arc::clone(&cancelled);
    let reader_worker = thread::spawn(move || {
        run_owned_reader_worker(
            &reader_path,
            reader_cancelled.as_ref(),
            OWNED_READER_DEADLINE,
            &ready_sender,
            &exit_sender,
        )
    });
    assert_eq!(receive_owned_worker_result(&ready_receiver)?, Some(1));

    let over_limit = vec![Vec::new(); OWNED_MAX_FACTS_PER_GENERATION + 1];
    assert_eq!(
        writer.publish_result(2, 2, over_limit, 64)?,
        Err("owned writer fact limit exceeded".to_owned())
    );
    assert_eq!(
        writer.publish_result(2, 2, fact_fixture(1, 1), 0)?,
        Err("owned writer batch limit invalid".to_owned())
    );
    assert_eq!(writer.active_generation()?, Some(1));

    writer.publish(2, 2, fact_fixture(512, 256), 64)?;
    let first_busy_checkpoint = writer.checkpoint()?;
    assert_eq!(first_busy_checkpoint.busy, 1);
    assert!(first_busy_checkpoint.log_frames > 0);
    assert!(first_busy_checkpoint.checkpointed_frames <= first_busy_checkpoint.log_frames);
    assert!(first_busy_checkpoint.elapsed <= OWNED_CHECKPOINT_DEADLINE);
    assert!(first_busy_checkpoint.wal_bytes > 0);
    assert!(first_busy_checkpoint.wal_bytes <= OWNED_MAX_WAL_BYTES);

    let generation_three_facts = fact_fixture(256, 192);
    writer.publish(3, 3, generation_three_facts.clone(), 32)?;
    let second_busy_checkpoint = writer.checkpoint()?;
    assert_eq!(second_busy_checkpoint.busy, 1);
    assert!(second_busy_checkpoint.elapsed <= OWNED_CHECKPOINT_DEADLINE);
    assert!(second_busy_checkpoint.wal_bytes >= first_busy_checkpoint.wal_bytes);
    assert!(second_busy_checkpoint.wal_bytes <= OWNED_MAX_WAL_BYTES);
    assert_eq!(writer.active_generation()?, Some(3));

    let cancellation_started_at = Instant::now();
    cancelled.store(true, Ordering::Release);
    assert_eq!(
        receive_owned_worker_result(&exit_receiver)?,
        OwnedReaderExit::Cancelled
    );
    let cancellation_elapsed = cancellation_started_at.elapsed();
    assert!(cancellation_elapsed <= OWNED_CHECKPOINT_DEADLINE);
    join_owned_worker(reader_worker, "owned reader worker")?;

    let final_checkpoint = writer.checkpoint()?;
    assert_eq!(final_checkpoint.busy, 0);
    assert_eq!(
        final_checkpoint.log_frames,
        final_checkpoint.checkpointed_frames
    );
    assert_eq!(final_checkpoint.wal_bytes, 0);
    assert!(final_checkpoint.elapsed <= OWNED_CHECKPOINT_DEADLINE);
    eprintln!(
        "owned SQLite topology: busy_checkpoint_ms=[{}, {}], max_wal_bytes={}, \
         cancellation_ms={}, final_checkpoint_ms={}",
        first_busy_checkpoint.elapsed.as_millis(),
        second_busy_checkpoint.elapsed.as_millis(),
        second_busy_checkpoint.wal_bytes,
        cancellation_elapsed.as_millis(),
        final_checkpoint.elapsed.as_millis()
    );
    writer.shutdown()?;
    join_owned_worker(writer_worker, "owned writer worker")?;

    let restored_reader = open_read_database(&database_path)?;
    assert_eq!(active_generation_id(&restored_reader)?, Some(3));
    assert_eq!(
        generation_facts(&restored_reader, 3)?,
        generation_three_facts
    );
    Ok(())
}

#[test]
fn owned_reader_deadline_releases_pinned_generation() -> TestResult {
    let directory = TempDirectory::new()?;
    let database_path = directory.join("owned-reader-deadline.sqlite3");
    let mut setup = open_file_database(&database_path)?;
    bootstrap_workspace(&setup)?;
    stage_ready_generation(&mut setup, 1, 1, &[b"active".to_vec()], 1)?;
    activate_generation(&mut setup, 1, 1)?;
    drop(setup);

    let cancelled = AtomicBool::new(false);
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (exit_sender, exit_receiver) = mpsc::sync_channel(1);
    let reader_path = database_path.clone();
    let reader_worker = thread::spawn(move || {
        run_owned_reader_worker(
            &reader_path,
            &cancelled,
            Duration::from_millis(25),
            &ready_sender,
            &exit_sender,
        )
    });

    assert_eq!(receive_owned_worker_result(&ready_receiver)?, Some(1));
    assert_eq!(
        receive_owned_worker_result(&exit_receiver)?,
        OwnedReaderExit::DeadlineExceeded
    );
    join_owned_worker(reader_worker, "deadline reader worker")?;

    let reader = open_read_database(&database_path)?;
    assert_eq!(active_generation_id(&reader)?, Some(1));
    Ok(())
}

#[test]
fn failed_cancelled_stale_and_rolled_back_staging_never_replace_active() -> TestResult {
    let directory = TempDirectory::new()?;
    let mut connection = open_file_database(&directory.join("failure.sqlite3"))?;
    bootstrap_workspace(&connection)?;
    stage_ready_generation(&mut connection, 1, 1, &[b"active".to_vec()], 1)?;
    activate_generation(&mut connection, 1, 1)?;

    begin_generation(&connection, 2, 2)?;
    advance_generation(&connection, 2, "discovered", "cancelled")?;
    assert_eq!(active_generation_id(&connection)?, Some(1));

    stage_ready_generation(&mut connection, 3, 3, &[b"stale".to_vec()], 1)?;
    assert!(activate_generation(&mut connection, 3, 4).is_err());
    assert_eq!(active_generation_id(&connection)?, Some(1));
    assert_eq!(generation_state(&connection, 1)?.as_deref(), Some("active"));
    assert_eq!(generation_state(&connection, 3)?.as_deref(), Some("ready"));

    {
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO generations(
                generation_id,
                workspace_id,
                source_epoch,
                lifecycle_state
             ) VALUES (4, 1, 4, 'discovered')",
            [],
        )?;
    }
    assert_eq!(generation_state(&connection, 4)?, None);
    assert_eq!(generation_facts(&connection, 1)?, [b"active".to_vec()]);
    Ok(())
}

#[test]
fn crash_writer_child() -> TestResult {
    let Some(database_path) = env::var_os(CRASH_CHILD_DATABASE) else {
        return Ok(());
    };
    let sentinel_path = env::var_os(CRASH_CHILD_SENTINEL)
        .ok_or_else(|| io::Error::other("crash child sentinel is missing"))?;
    let target_state = env::var(CRASH_CHILD_STATE)?;

    let mut connection = open_file_database(Path::new(&database_path))?;
    begin_generation(&connection, 2, 2)?;
    advance_to(&connection, 2, &target_state)?;
    write_facts_in_bounded_batches(
        &mut connection,
        2,
        &[b"partial-a".to_vec(), b"partial-b".to_vec()],
        1,
    )?;
    fs::write(sentinel_path, b"ready")?;
    thread::sleep(Duration::from_secs(60));
    Ok(())
}

#[test]
fn process_termination_in_every_staging_state_recovers_without_replacing_active() -> TestResult {
    for target_state in INCOMPLETE_STATES {
        let directory = TempDirectory::new()?;
        let database_path = directory.join("crash.sqlite3");
        let sentinel_path = directory.join("child-ready");
        let mut connection = open_file_database(&database_path)?;
        bootstrap_workspace(&connection)?;
        stage_ready_generation(&mut connection, 1, 1, &[b"active".to_vec()], 1)?;
        activate_generation(&mut connection, 1, 1)?;
        drop(connection);

        let mut child = Command::new(env::current_exe()?)
            .args(["--exact", "crash_writer_child", "--nocapture"])
            .env(CRASH_CHILD_DATABASE, &database_path)
            .env(CRASH_CHILD_SENTINEL, &sentinel_path)
            .env(CRASH_CHILD_STATE, target_state)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        wait_for_sentinel(&mut child, &sentinel_path)?;
        child.kill()?;
        let status = child.wait()?;
        assert!(!status.success(), "the child must be terminated");

        let mut recovered = open_file_database(&database_path)?;
        assert_eq!(recover_incomplete_generations(&mut recovered)?, 1);
        assert_eq!(active_generation_id(&recovered)?, Some(1));
        assert_eq!(generation_facts(&recovered, 1)?, [b"active".to_vec()]);
        assert!(generation_facts(&recovered, 2)?.is_empty());
        assert_eq!(generation_state(&recovered, 2)?.as_deref(), Some("failed"));
    }
    Ok(())
}

#[test]
fn online_backup_restores_committed_wal_state_and_checkpoint_truncates_it() -> TestResult {
    let directory = TempDirectory::new()?;
    let database_path = directory.join("source.sqlite3");
    let backup_path = directory.join("backup.sqlite3");
    let mut source = open_file_database(&database_path)?;
    bootstrap_workspace(&source)?;
    stage_ready_generation(&mut source, 1, 1, &fact_fixture(128, 256), 17)?;
    activate_generation(&mut source, 1, 1)?;

    let source_wal_path = wal_path(&database_path);
    assert!(
        fs::metadata(&source_wal_path)?.len() > 0,
        "committed data must remain in WAL before the explicit checkpoint"
    );
    backup_database(&source, &backup_path)?;

    let restored = open_read_database(&backup_path)?;
    assert_eq!(
        restored.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))?,
        "ok"
    );
    assert_eq!(active_generation_id(&restored)?, Some(1));
    assert_eq!(generation_facts(&restored, 1)?, fact_fixture(128, 256));
    drop(restored);

    let (busy, log_frames, checkpointed_frames) = truncate_checkpoint(&source)?;
    assert_eq!(busy, 0);
    assert_eq!(log_frames, checkpointed_frames);
    assert_eq!(fs::metadata(source_wal_path)?.len(), 0);
    Ok(())
}

#[test]
fn online_backup_interleaves_with_writes_and_restores_recoverable_state() -> TestResult {
    let directory = TempDirectory::new()?;
    let database_path = directory.join("source.sqlite3");
    let backup_path = directory.join("backup.sqlite3");
    let generation_one_facts = fact_fixture(1_024, 256);
    let generation_two_facts = fact_fixture(768, 192);
    let incomplete_facts = fact_fixture(64, 128);
    let mut writer = open_file_database(&database_path)?;
    bootstrap_workspace(&writer)?;
    stage_ready_generation(&mut writer, 1, 1, &generation_one_facts, 64)?;
    activate_generation(&mut writer, 1, 1)?;

    let (step_sender, step_receiver) = mpsc::sync_channel(0);
    let (resume_sender, resume_receiver) = mpsc::sync_channel(0);
    let backup_source_path = database_path.clone();
    let backup_destination_path = backup_path.clone();
    let backup_thread = thread::spawn(move || -> Result<(), String> {
        let source = open_read_database(&backup_source_path).map_err(|error| error.to_string())?;
        let mut destination =
            Connection::open(&backup_destination_path).map_err(|error| error.to_string())?;
        let backup = Backup::new(&source, &mut destination).map_err(|error| error.to_string())?;

        let first_step = backup.step(1).map_err(|error| error.to_string())?;
        if first_step != rusqlite::backup::StepResult::More {
            return Err(format!(
                "expected an incomplete first backup step, got {first_step:?}"
            ));
        }
        step_sender.send(()).map_err(|error| error.to_string())?;
        resume_receiver.recv().map_err(|error| error.to_string())?;

        let second_step = backup.step(1).map_err(|error| error.to_string())?;
        if second_step != rusqlite::backup::StepResult::More {
            return Err(format!(
                "expected an incomplete second backup step, got {second_step:?}"
            ));
        }
        step_sender.send(()).map_err(|error| error.to_string())?;
        resume_receiver.recv().map_err(|error| error.to_string())?;

        backup
            .run_to_completion(1, Duration::from_millis(1), None)
            .map_err(|error| error.to_string())
    });

    step_receiver.recv()?;
    stage_ready_generation(&mut writer, 2, 2, &generation_two_facts, 64)?;
    resume_sender.send(())?;

    step_receiver.recv()?;
    activate_generation(&mut writer, 2, 2)?;
    begin_generation(&writer, 3, 3)?;
    advance_to(&writer, 3, "extracting")?;
    write_facts_in_bounded_batches(&mut writer, 3, &incomplete_facts, 16)?;
    resume_sender.send(())?;

    backup_thread
        .join()
        .map_err(|_| io::Error::other("online backup thread panicked"))?
        .map_err(io::Error::other)?;

    let mut restored = open_file_database(&backup_path)?;
    assert_eq!(
        restored.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))?,
        "ok"
    );
    assert_eq!(active_generation_id(&restored)?, Some(2));
    assert_eq!(generation_facts(&restored, 2)?, generation_two_facts);
    assert_eq!(
        generation_state(&restored, 3)?.as_deref(),
        Some("extracting")
    );
    assert_eq!(generation_facts(&restored, 3)?, incomplete_facts);

    assert_eq!(recover_incomplete_generations(&mut restored)?, 1);
    assert_eq!(active_generation_id(&restored)?, Some(2));
    assert_eq!(generation_state(&restored, 3)?.as_deref(), Some("failed"));
    assert!(generation_facts(&restored, 3)?.is_empty());
    Ok(())
}

#[test]
fn sustained_writes_bound_wal_and_cancellable_backup_never_publishes_partial_state() -> TestResult {
    let directory = TempDirectory::new()?;
    let database_path = directory.join("backup-cancellation-source.sqlite3");
    let partial_backup_path = directory.join("backup-cancellation.partial");
    let verified_backup_path = directory.join("backup-cancellation.sqlite3");
    let source_wal_path = wal_path(&database_path);
    let mut setup = open_file_database(&database_path)?;
    bootstrap_workspace(&setup)?;
    stage_ready_generation(&mut setup, 1, 1, &fact_fixture(2_048, 512), 64)?;
    activate_generation(&mut setup, 1, 1)?;
    assert_eq!(truncate_checkpoint(&setup)?, (0, 0, 0));
    drop(setup);

    let (command_sender, command_receiver) = mpsc::sync_channel(1);
    let writer_path = database_path.clone();
    let writer_worker =
        thread::spawn(move || run_owned_writer_worker(&writer_path, &command_receiver));
    let writer = OwnedWriterClient {
        commands: command_sender,
    };

    let cancelled = Arc::new(AtomicBool::new(false));
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (exit_sender, exit_receiver) = mpsc::sync_channel(1);
    let backup_source_path = database_path.clone();
    let backup_destination_path = partial_backup_path.clone();
    let backup_cancelled = Arc::clone(&cancelled);
    let backup_worker = thread::spawn(move || {
        let result = run_cancellable_backup_worker(
            &backup_source_path,
            &backup_destination_path,
            backup_cancelled.as_ref(),
            &ready_sender,
        );
        send_owned_reply(&exit_sender, result)
    });
    receive_owned_worker_result(&ready_receiver)?;

    let mut max_wal_bytes = 0_u64;
    let mut final_facts = Vec::new();
    for generation_id in 2_i64..=5 {
        final_facts = fact_fixture(512, 384);
        writer.publish(generation_id, generation_id, final_facts.clone(), 64)?;
        let wal_bytes = fs::metadata(&source_wal_path)?.len();
        max_wal_bytes = max_wal_bytes.max(wal_bytes);
        assert!(wal_bytes > 0);
        assert!(wal_bytes <= BACKUP_MAX_WAL_BYTES);
    }
    assert_eq!(writer.active_generation()?, Some(5));

    let cancellation_started_at = Instant::now();
    cancelled.store(true, Ordering::Release);
    let cancellation = receive_owned_worker_result(&exit_receiver)?;
    let cancellation_acknowledgement = cancellation_started_at.elapsed();
    assert!(cancellation.completed_steps > 0);
    assert!(cancellation.completed_steps <= BACKUP_MAX_STEPS);
    assert!(cancellation.elapsed <= BACKUP_WORKER_DEADLINE);
    assert!(cancellation_acknowledgement <= BACKUP_CANCELLATION_DEADLINE);
    join_owned_worker(backup_worker, "cancellable backup worker")?;

    let checkpoint = writer.checkpoint()?;
    assert_eq!(checkpoint.busy, 0);
    assert_eq!(checkpoint.log_frames, checkpoint.checkpointed_frames);
    assert_eq!(checkpoint.wal_bytes, 0);
    writer.shutdown()?;
    join_owned_worker(writer_worker, "owned writer worker")?;

    let source = open_read_database(&database_path)?;
    backup_database(&source, &verified_backup_path)?;
    let restored = open_read_database(&verified_backup_path)?;
    assert_eq!(
        restored.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))?,
        "ok"
    );
    assert_eq!(active_generation_id(&restored)?, Some(5));
    assert_eq!(generation_facts(&restored, 5)?, final_facts);
    eprintln!(
        "sustained-write backup cancellation: steps={}, backup_ms={}, \
         cancellation_ms={}, max_wal_bytes={max_wal_bytes}",
        cancellation.completed_steps,
        cancellation.elapsed.as_millis(),
        cancellation_acknowledgement.as_millis()
    );
    Ok(())
}

#[test]
fn bounded_direct_and_private_ram_first_staging_are_logically_equivalent() -> TestResult {
    let directory = TempDirectory::new()?;
    let facts = fact_fixture(257, 192);

    let direct_path = directory.join("direct.sqlite3");
    let mut direct = open_file_database(&direct_path)?;
    bootstrap_workspace(&direct)?;
    stage_ready_generation(&mut direct, 1, 1, &facts, 17)?;
    activate_generation(&mut direct, 1, 1)?;

    let mut memory = open_memory_database()?;
    bootstrap_workspace(&memory)?;
    stage_ready_generation(&mut memory, 1, 1, &facts, 17)?;
    activate_generation(&mut memory, 1, 1)?;
    let memory_path = directory.join("memory.sqlite3");
    backup_database(&memory, &memory_path)?;
    let materialized_memory = open_read_database(&memory_path)?;

    assert_eq!(
        active_generation_id(&direct)?,
        active_generation_id(&materialized_memory)?
    );
    assert_eq!(
        generation_facts(&direct, 1)?,
        generation_facts(&materialized_memory, 1)?
    );
    Ok(())
}

#[test]
#[ignore = "manual synthetic ingestion timing probe; not a release budget"]
fn benchmark_bounded_direct_against_private_ram_first_staging() -> TestResult {
    let directory = TempDirectory::new()?;
    let facts = fact_fixture(10_000, 256);

    let (direct_elapsed, direct_path) = benchmark_direct(&directory, &facts)?;
    let (memory_elapsed, memory_path) = benchmark_private_ram_first(&directory, &facts)?;

    let direct = open_read_database(&direct_path)?;
    let materialized_memory = open_read_database(&memory_path)?;
    assert_eq!(
        generation_facts(&direct, 1)?,
        generation_facts(&materialized_memory, 1)?
    );
    eprintln!(
        "synthetic SQLite ingestion: direct={direct_elapsed:?} ({} bytes), \
         private-ram-first={memory_elapsed:?} ({} bytes), facts={}",
        fs::metadata(direct_path)?.len(),
        fs::metadata(memory_path)?.len(),
        facts.len()
    );
    Ok(())
}

#[test]
#[ignore = "manual synthetic direct-staging resource probe; not a release budget"]
fn benchmark_bounded_direct_resource_sample() -> TestResult {
    let directory = TempDirectory::new()?;
    let facts = fact_fixture(10_000, 256);
    let (elapsed, database_path) = benchmark_direct(&directory, &facts)?;
    eprintln!(
        "synthetic direct SQLite ingestion: elapsed={elapsed:?}, bytes={}, facts={}, \
         peak_rss_kib={:?}",
        fs::metadata(database_path)?.len(),
        facts.len(),
        peak_resident_set_kib()
    );
    Ok(())
}

#[test]
#[ignore = "manual synthetic RAM-first resource probe; not a release budget"]
fn benchmark_private_ram_first_resource_sample() -> TestResult {
    let directory = TempDirectory::new()?;
    let facts = fact_fixture(10_000, 256);
    let (elapsed, database_path) = benchmark_private_ram_first(&directory, &facts)?;
    eprintln!(
        "synthetic private-RAM-first SQLite ingestion: elapsed={elapsed:?}, bytes={}, facts={}, \
         peak_rss_kib={:?}",
        fs::metadata(database_path)?.len(),
        facts.len(),
        peak_resident_set_kib()
    );
    Ok(())
}

#[test]
#[ignore = "manual synthetic batch/durability timing probe; not a release budget"]
fn benchmark_batch_sizes_and_synchronous_profiles() -> TestResult {
    let directory = TempDirectory::new()?;
    let facts = fact_fixture(10_000, 256);
    for synchronous in ["FULL", "NORMAL"] {
        for batch_limit in [16_usize, 64, 256, 512] {
            let mut elapsed_samples = Vec::with_capacity(5);
            let mut max_wal_bytes = 0_u64;
            for sample in 0..5 {
                let (elapsed, wal_bytes) = benchmark_direct_durability_profile(
                    &directory,
                    &facts,
                    synchronous,
                    batch_limit,
                    sample,
                )?;
                elapsed_samples.push(elapsed);
                max_wal_bytes = max_wal_bytes.max(wal_bytes);
            }
            elapsed_samples.sort_unstable();
            eprintln!(
                "synthetic SQLite durability: synchronous={synchronous}, batch={batch_limit}, \
                 median={:?}, range={:?}..={:?}, max_wal_bytes={max_wal_bytes}",
                elapsed_samples[2], elapsed_samples[0], elapsed_samples[4]
            );
        }
    }
    Ok(())
}
