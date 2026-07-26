use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use repowitness_application::{
    PreparedRustIndex, RustIndexCoverage, RustIndexPublicationPort, RustSourceSnapshotIdentity,
};
use repowitness_domain::RepositoryIdentityDigest;

use super::{
    CheckpointOutcome, GenerationCoverage, GenerationId, ProjectionRebuildLimits,
    ProjectionRebuildOutcome, SqliteStoreError, database_file_identity,
    open_index_writer_with_identity_until,
    writer::{WriteControl, WriterState},
};
use crate::contained_source::FileIdentity;

type Reply<T> = SyncSender<Result<T, SqliteStoreError>>;

const MUTATION_LEASE_SUFFIX: &str = ".repowitness-mutation.lock";
const MUTATION_LEASE_RETRY_DELAY: Duration = Duration::from_millis(10);

pub(crate) struct SqliteMutationLease {
    database_path: PathBuf,
    _file: File,
}

impl SqliteMutationLease {
    pub(crate) fn acquire(
        database_path: &Path,
        deadline: Instant,
    ) -> Result<Self, SqliteStoreError> {
        if Instant::now() >= deadline {
            return Err(SqliteStoreError::DeadlineExceeded);
        }
        let database_path = canonical_database_path(database_path)?;
        let file = acquire_mutation_lease(&database_path, deadline)?;
        Ok(Self {
            database_path,
            _file: file,
        })
    }
}

enum WriterCommand {
    Register {
        repository: RepositoryIdentityDigest,
        initial_source_epoch: u64,
        deadline: Instant,
        reply: Reply<()>,
    },
    AdvanceEpoch {
        repository: RepositoryIdentityDigest,
        expected: u64,
        next: u64,
        deadline: Instant,
        reply: Reply<()>,
    },
    Stage(Box<StageCommand>),
    Activate {
        generation: GenerationId,
        expected_source_epoch: u64,
        deadline: Instant,
        reply: Reply<()>,
    },
    ActiveGeneration {
        repository: RepositoryIdentityDigest,
        deadline: Instant,
        reply: Reply<Option<GenerationId>>,
    },
    RebuildProjection(Box<RebuildProjectionCommand>),
    Checkpoint {
        deadline: Instant,
        reply: Reply<CheckpointOutcome>,
    },
    Shutdown {
        reply: Reply<()>,
    },
}

struct StageCommand {
    source_epoch: u64,
    identity: RustSourceSnapshotIdentity,
    prepared: PreparedRustIndex,
    coverage: RustIndexCoverage,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<GenerationId>,
}

struct RebuildProjectionCommand {
    limits: ProjectionRebuildLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<ProjectionRebuildOutcome>,
}

/// Startup facts from deterministic recovery on the owned writer thread.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexStoreStartup {
    recovered_generations: u64,
}

impl IndexStoreStartup {
    /// Returns the number of incomplete generations marked failed.
    #[must_use]
    pub const fn recovered_generations(self) -> u64 {
        self.recovered_generations
    }
}

/// Capacity-one command client for the one SQLite writer-owner thread.
pub struct OwnedSqliteIndex {
    commands: SyncSender<WriterCommand>,
    worker: Option<JoinHandle<()>>,
}

impl OwnedSqliteIndex {
    /// Starts the writer owner, migrates the database, and performs recovery.
    pub fn start(
        path: &Path,
        applied_at_unix_ms: u64,
        deadline: Instant,
    ) -> Result<(Self, IndexStoreStartup), SqliteStoreError> {
        let lease = SqliteMutationLease::acquire(path, deadline)?;
        let database_identity = database_file_identity(&lease.database_path)?;
        Self::start_with_lease(
            lease,
            database_identity,
            applied_at_unix_ms,
            Arc::new(AtomicBool::new(false)),
            deadline,
        )
    }

    pub(crate) fn start_with_lease(
        mutation_lease: SqliteMutationLease,
        database_identity: Option<FileIdentity>,
        applied_at_unix_ms: u64,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<(Self, IndexStoreStartup), SqliteStoreError> {
        if Instant::now() >= deadline {
            return Err(SqliteStoreError::DeadlineExceeded);
        }
        let database_path = mutation_lease.database_path.clone();
        let (commands, receiver) = mpsc::sync_channel(1);
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("repowitness-sqlite-writer".to_owned())
            .spawn(move || {
                // The owner thread retains the lease even if its client is
                // dropped while a queued command is still completing.
                let _mutation_lease = mutation_lease;
                let startup = if cancelled.load(Ordering::Acquire) {
                    Err(SqliteStoreError::Cancelled)
                } else if Instant::now() >= deadline {
                    Err(SqliteStoreError::DeadlineExceeded)
                } else {
                    open_index_writer_with_identity_until(
                        &database_path,
                        database_identity,
                        applied_at_unix_ms,
                        Arc::clone(&cancelled),
                        deadline,
                    )
                    .and_then(|connection| {
                        let mut state = WriterState::new(connection);
                        let recovered_generations = state.recover(cancelled, deadline)?;
                        Ok((state, recovered_generations))
                    })
                };
                let Ok((mut state, recovered_generations)) = startup else {
                    let error = startup.err().unwrap_or(SqliteStoreError::WorkerUnavailable);
                    let _ = startup_sender.send(Err(error));
                    return;
                };
                if startup_sender
                    .send(Ok(IndexStoreStartup {
                        recovered_generations,
                    }))
                    .is_err()
                {
                    return;
                }
                run_writer(&mut state, receiver);
            })
            .map_err(|_| SqliteStoreError::WorkerUnavailable)?;
        let startup = receive_reply(&startup_receiver, deadline)?;
        Ok((
            Self {
                commands,
                worker: Some(worker),
            },
            startup,
        ))
    }

    /// Registers a repository workspace at one explicit initial source epoch.
    pub fn register_workspace(
        &self,
        repository: RepositoryIdentityDigest,
        initial_source_epoch: u64,
        deadline: Instant,
    ) -> Result<(), SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::Register {
                repository,
                initial_source_epoch,
                deadline,
                reply,
            },
            deadline,
        )?;
        receive_reply(&receiver, deadline)
    }

    /// Advances the monotonic source epoch with compare-and-set semantics.
    pub fn advance_source_epoch(
        &self,
        repository: RepositoryIdentityDigest,
        expected: u64,
        next: u64,
        deadline: Instant,
    ) -> Result<(), SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::AdvanceEpoch {
                repository,
                expected,
                next,
                deadline,
                reply,
            },
            deadline,
        )?;
        receive_reply(&receiver, deadline)
    }

    /// Materializes one complete prepared index and leaves it ready to activate.
    pub fn stage(
        &self,
        source_epoch: u64,
        identity: RustSourceSnapshotIdentity,
        prepared: PreparedRustIndex,
        coverage: GenerationCoverage,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<GenerationId, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::Stage(Box::new(StageCommand {
                source_epoch,
                identity,
                prepared,
                coverage,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(generation) => Ok(generation),
            Err(error) => {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
                Err(error)
            }
        }
    }

    /// Atomically activates one ready generation if its source epoch is current.
    pub fn activate(
        &self,
        generation: GenerationId,
        expected_source_epoch: u64,
        deadline: Instant,
    ) -> Result<(), SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::Activate {
                generation,
                expected_source_epoch,
                deadline,
                reply,
            },
            deadline,
        )?;
        receive_reply(&receiver, deadline)
    }

    /// Returns the active generation for one repository without exposing rows.
    pub fn active_generation(
        &self,
        repository: RepositoryIdentityDigest,
        deadline: Instant,
    ) -> Result<Option<GenerationId>, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::ActiveGeneration {
                repository,
                deadline,
                reply,
            },
            deadline,
        )?;
        receive_reply(&receiver, deadline)
    }

    /// Rebuilds the complete disposable FTS5 projection behind an atomic slot switch.
    pub fn rebuild_search_projection(
        &self,
        limits: ProjectionRebuildLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ProjectionRebuildOutcome, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::RebuildProjection(Box::new(RebuildProjectionCommand {
                limits,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
                Err(error)
            }
        }
    }

    /// Runs one explicit truncating checkpoint on the writer connection.
    pub fn checkpoint(&self, deadline: Instant) -> Result<CheckpointOutcome, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(WriterCommand::Checkpoint { deadline, reply }, deadline)?;
        receive_reply(&receiver, deadline)
    }

    /// Stops and joins the owned writer thread.
    pub fn shutdown(mut self, deadline: Instant) -> Result<(), SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(WriterCommand::Shutdown { reply }, deadline)?;
        receive_reply(&receiver, deadline)?;
        self.join_worker()
    }

    fn send(&self, command: WriterCommand, deadline: Instant) -> Result<(), SqliteStoreError> {
        if Instant::now() >= deadline {
            return Err(SqliteStoreError::DeadlineExceeded);
        }
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(_) => SqliteStoreError::QueueFull,
                TrySendError::Disconnected(_) => SqliteStoreError::WorkerUnavailable,
            })
    }

    fn join_worker(&mut self) -> Result<(), SqliteStoreError> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker.join().map_err(|_| SqliteStoreError::WorkerPanicked)
    }
}

fn canonical_database_path(path: &Path) -> Result<PathBuf, SqliteStoreError> {
    match fs::symlink_metadata(path) {
        Ok(_) => fs::canonicalize(path).map_err(|_| SqliteStoreError::OpenFailed),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = match path.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent,
                _ => Path::new("."),
            };
            let parent = fs::canonicalize(parent).map_err(|_| SqliteStoreError::OpenFailed)?;
            let file_name = path.file_name().ok_or(SqliteStoreError::OpenFailed)?;
            Ok(parent.join(file_name))
        }
        Err(_) => Err(SqliteStoreError::OpenFailed),
    }
}

fn acquire_mutation_lease(
    database_path: &Path,
    deadline: Instant,
) -> Result<File, SqliteStoreError> {
    let lease_path = mutation_lease_path(database_path);
    let lease = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lease_path)
        .map_err(|_| SqliteStoreError::MutationLeaseUnavailable)?;

    loop {
        match lease.try_lock() {
            Ok(()) => return Ok(lease),
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(_)) => {
                return Err(SqliteStoreError::MutationLeaseUnavailable);
            }
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(SqliteStoreError::DeadlineExceeded);
        }
        thread::sleep(MUTATION_LEASE_RETRY_DELAY.min(deadline.duration_since(now)));
    }
}

fn mutation_lease_path(database_path: &Path) -> PathBuf {
    let mut lease_name = OsString::from(database_path.as_os_str());
    lease_name.push(MUTATION_LEASE_SUFFIX);
    PathBuf::from(lease_name)
}

impl Drop for OwnedSqliteIndex {
    fn drop(&mut self) {
        if self.worker.is_none() {
            return;
        }
        let (reply, _receiver) = mpsc::sync_channel(1);
        if self
            .commands
            .try_send(WriterCommand::Shutdown { reply })
            .is_ok()
        {
            let _ = self.join_worker();
        } else {
            // Dropping the sender disconnects the worker after any queued
            // command. Do not wait without having delivered a shutdown.
            let _ = self.worker.take();
        }
    }
}

impl RustIndexPublicationPort for OwnedSqliteIndex {
    type Generation = GenerationId;
    type Error = SqliteStoreError;

    fn stage(
        &self,
        source_epoch: u64,
        identity: RustSourceSnapshotIdentity,
        prepared: PreparedRustIndex,
        coverage: RustIndexCoverage,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Self::Generation, Self::Error> {
        Self::stage(
            self,
            source_epoch,
            identity,
            prepared,
            coverage,
            cancelled,
            deadline,
        )
    }

    fn activate(
        &self,
        generation: Self::Generation,
        expected_source_epoch: u64,
        deadline: Instant,
    ) -> Result<(), Self::Error> {
        Self::activate(self, generation, expected_source_epoch, deadline)
    }
}

fn run_writer(state: &mut WriterState, receiver: Receiver<WriterCommand>) {
    while let Ok(command) = receiver.recv() {
        match command {
            WriterCommand::Register {
                repository,
                initial_source_epoch,
                deadline,
                reply,
            } => {
                let result = check_deadline(deadline).and_then(|()| {
                    state
                        .register_workspace(repository, initial_source_epoch)
                        .map(|_| ())
                });
                send_reply(reply, result);
            }
            WriterCommand::AdvanceEpoch {
                repository,
                expected,
                next,
                deadline,
                reply,
            } => {
                let result = check_deadline(deadline)
                    .and_then(|()| state.advance_source_epoch(repository, expected, next));
                send_reply(reply, result);
            }
            WriterCommand::Stage(command) => {
                let StageCommand {
                    source_epoch,
                    identity,
                    prepared,
                    coverage,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.stage(
                    source_epoch,
                    identity,
                    &prepared,
                    coverage,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_reply(reply, result);
            }
            WriterCommand::Activate {
                generation,
                expected_source_epoch,
                deadline,
                reply,
            } => {
                let result = check_deadline(deadline)
                    .and_then(|()| state.activate(generation, expected_source_epoch));
                send_reply(reply, result);
            }
            WriterCommand::ActiveGeneration {
                repository,
                deadline,
                reply,
            } => {
                let result =
                    check_deadline(deadline).and_then(|()| state.active_generation(repository));
                send_reply(reply, result);
            }
            WriterCommand::RebuildProjection(command) => {
                let RebuildProjectionCommand {
                    limits,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.rebuild_search_projection(
                    limits,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_reply(reply, result);
            }
            WriterCommand::Checkpoint { deadline, reply } => {
                let result = check_deadline(deadline).and_then(|()| state.checkpoint());
                send_reply(reply, result);
            }
            WriterCommand::Shutdown { reply } => {
                send_reply(reply, Ok(()));
                break;
            }
        }
    }
}

fn send_reply<T>(reply: Reply<T>, result: Result<T, SqliteStoreError>) {
    let _ = reply.try_send(result);
}

fn receive_reply<T>(
    receiver: &Receiver<Result<T, SqliteStoreError>>,
    deadline: Instant,
) -> Result<T, SqliteStoreError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(SqliteStoreError::DeadlineExceeded);
    }
    receiver
        .recv_timeout(remaining)
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => SqliteStoreError::ReplyTimeout,
            mpsc::RecvTimeoutError::Disconnected => SqliteStoreError::WorkerUnavailable,
        })?
}

fn check_deadline(deadline: Instant) -> Result<(), SqliteStoreError> {
    if Instant::now() >= deadline {
        Err(SqliteStoreError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant},
    };

    use repowitness_application::{
        ImmutableRustSource, RustArtifactIdentity, RustIndexLimits, RustSourceSnapshotIdentity,
        prepare_rust_index,
    };
    use repowitness_domain::{
        AnalysisSchemaDigest, ConfigurationDigest, GitStateDigest, ProducerManifestDigest,
        RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits, WorktreeStateDigest,
    };
    use rusqlite::{Connection, TransactionBehavior, params};

    use crate::{
        BackupLimits, OwnedSqliteReader, ProjectionRebuildLimits, SearchLimits,
        create_online_backup,
    };

    use super::{
        GenerationCoverage, OwnedSqliteIndex, SqliteStoreError, WriterCommand, mutation_lease_path,
    };
    use crate::sqlite::writer::MAX_STARTUP_RECOVERY_GENERATIONS;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
    const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(4096, 256);

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "repowitness-owned-store-{}-{ordinal}",
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

    fn deadline() -> Instant {
        Instant::now()
            .checked_add(Duration::from_secs(5))
            .expect("test deadline should be representable")
    }

    #[test]
    fn saturated_drop_detaches_instead_of_waiting_without_shutdown() {
        let (commands, _receiver) = mpsc::sync_channel(1);
        let (reply, _reply_receiver) = mpsc::sync_channel(1);
        commands
            .send(WriterCommand::Shutdown { reply })
            .expect("fixture queue should accept one command");
        let worker = thread::spawn(|| thread::sleep(Duration::from_millis(500)));
        let store = OwnedSqliteIndex {
            commands,
            worker: Some(worker),
        };

        let started = Instant::now();
        drop(store);

        assert!(started.elapsed() < Duration::from_millis(250));
    }

    fn artifact_identity() -> RustArtifactIdentity {
        RustArtifactIdentity::new(
            ProducerManifestDigest::new([5; 32]),
            ConfigurationDigest::new([4; 32]),
            AnalysisSchemaDigest::new([6; 32]),
            7,
        )
    }

    fn snapshot_identity() -> RustSourceSnapshotIdentity {
        RustSourceSnapshotIdentity::new(
            RepositoryIdentityDigest::new([1; 32]),
            GitStateDigest::new([2; 32]),
            WorktreeStateDigest::new([3; 32]),
            ConfigurationDigest::new([4; 32]),
            ProducerManifestDigest::new([5; 32]),
            AnalysisSchemaDigest::new([6; 32]),
            7,
        )
    }

    fn prepared(suffix: &str) -> repowitness_application::PreparedRustIndex {
        let cancelled = AtomicBool::new(false);
        prepare_rust_index(
            vec![
                ImmutableRustSource::new(
                    RepositoryPath::try_from_bytes(b"src/lib.rs", PATH_LIMITS)
                        .expect("fixture path should be valid"),
                    format!("pub fn stable_{suffix}() {{}}\n")
                        .into_bytes()
                        .into_boxed_slice(),
                ),
                ImmutableRustSource::new(
                    RepositoryPath::try_from_bytes(b"src/model.rs", PATH_LIMITS)
                        .expect("fixture path should be valid"),
                    b"pub struct Model;\n".to_vec().into_boxed_slice(),
                ),
            ],
            artifact_identity(),
            RustIndexLimits::default(),
            &cancelled,
            deadline(),
        )
        .expect("fixture index should prepare")
    }

    fn prepared_many(count: u16) -> repowitness_application::PreparedRustIndex {
        let mut source = String::new();
        for ordinal in 0..count {
            use std::fmt::Write as _;
            writeln!(source, "pub fn symbol_{ordinal:04}() {{}}")
                .expect("fixture source should be writable");
        }
        let cancelled = AtomicBool::new(false);
        prepare_rust_index(
            vec![ImmutableRustSource::new(
                RepositoryPath::try_from_bytes(b"src/many.rs", PATH_LIMITS)
                    .expect("fixture path should be valid"),
                source.into_bytes().into_boxed_slice(),
            )],
            artifact_identity(),
            RustIndexLimits::default(),
            &cancelled,
            deadline(),
        )
        .expect("large fixture index should prepare")
    }

    fn verify_backup_publication(directory: &TempDirectory) -> PathBuf {
        let backup_path = directory.0.join("backup.sqlite3");
        let backup = create_online_backup(
            &directory.database(),
            &backup_path,
            BackupLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("online backup should publish");
        assert!(backup.steps() > 0);
        assert!(backup.source_pages() > 0);
        assert_eq!(
            create_online_backup(
                &directory.database(),
                &backup_path,
                BackupLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect_err("backup publication must not overwrite an existing file"),
            SqliteStoreError::BackupDestinationUnavailable
        );
        let cancelled_backup = directory.0.join("cancelled.sqlite3");
        assert_eq!(
            create_online_backup(
                &directory.database(),
                &cancelled_backup,
                BackupLimits::default(),
                Arc::new(AtomicBool::new(true)),
                deadline(),
            )
            .expect_err("pre-cancelled backup should fail"),
            SqliteStoreError::Cancelled
        );
        assert!(!cancelled_backup.exists());
        let bounded_backup = directory.0.join("bounded.sqlite3");
        let one_page_limit =
            BackupLimits::try_new(1, 1, Duration::ZERO).expect("fixture limit should be valid");
        assert_eq!(
            create_online_backup(
                &directory.database(),
                &bounded_backup,
                one_page_limit,
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect_err("one page cannot contain the Phase 0 schema"),
            SqliteStoreError::BackupStepLimitExceeded
        );
        assert!(!bounded_backup.exists());
        assert!(
            fs::read_dir(&directory.0)
                .expect("fixture directory should remain readable")
                .all(|entry| !entry
                    .expect("fixture entry should be readable")
                    .file_name()
                    .to_string_lossy()
                    .contains("repowitness-partial"))
        );
        backup_path
    }

    fn verify_persisted_generation(
        directory: &TempDirectory,
        generation: super::GenerationId,
        backup_path: &PathBuf,
    ) {
        let connection =
            Connection::open(directory.database()).expect("database should reopen for inspection");
        let counts: (i64, i64, i64, i64) = connection
            .query_row(
                "SELECT
                    (SELECT count(*) FROM source_snapshots WHERE lifecycle_state = 'complete'),
                    (SELECT count(*) FROM analysis_artifacts WHERE lifecycle_state = 'complete'),
                    (SELECT count(*) FROM artifact_facts),
                    (SELECT count(*) FROM generation_search WHERE generation_id = ?1)",
                [generation.get()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("persisted counts should be readable");
        assert_eq!(counts.0, 1);
        assert_eq!(counts.1, 2);
        assert!(counts.2 >= 2);
        assert_eq!(counts.2, counts.3);
        assert!(
            connection
                .execute(
                    "UPDATE source_snapshots SET configuration_digest = zeroblob(32)",
                    [],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "UPDATE analysis_artifacts SET source_content_digest = zeroblob(32)",
                    [],
                )
                .is_err()
        );
        let backup_connection =
            Connection::open(backup_path).expect("published backup should be readable");
        let backup_active: i64 = backup_connection
            .query_row("SELECT active_generation_id FROM workspaces", [], |row| {
                row.get(0)
            })
            .expect("backup should preserve active generation");
        assert_eq!(backup_active, generation.get());
    }

    #[test]
    fn owned_writer_stages_and_atomically_activates_real_prepared_facts() {
        let directory = TempDirectory::new();
        let (store, startup) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
            .expect("owned store should start");
        let repository = snapshot_identity().repository();
        store
            .register_workspace(repository, 0, deadline())
            .expect("workspace should register");
        let generation = store
            .stage(
                0,
                snapshot_identity(),
                prepared("v1"),
                GenerationCoverage::new(2, 0, 0, 0),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("generation should stage");

        assert_eq!(startup.recovered_generations(), 0);
        assert_eq!(
            store
                .active_generation(repository, deadline())
                .expect("active generation should be readable"),
            None
        );
        store
            .activate(generation, 0, deadline())
            .expect("ready generation should activate");
        assert_eq!(
            store
                .active_generation(repository, deadline())
                .expect("active generation should be readable"),
            Some(generation)
        );
        let checkpoint = store
            .checkpoint(deadline())
            .expect("explicit checkpoint should complete");
        assert_eq!(checkpoint.busy(), 0);
        assert!(checkpoint.checkpointed_frames() <= checkpoint.log_frames());
        let backup_path = verify_backup_publication(&directory);
        store.shutdown(deadline()).expect("worker should stop");
        verify_persisted_generation(&directory, generation, &backup_path);
    }

    #[test]
    fn corrupted_complete_artifact_is_never_reused_or_activated() {
        let directory = TempDirectory::new();
        let database = directory.database();
        let repository = snapshot_identity().repository();
        let (store, _) =
            OwnedSqliteIndex::start(&database, 123, deadline()).expect("owned store should start");
        store
            .register_workspace(repository, 0, deadline())
            .expect("workspace should register");
        let active = store
            .stage(
                0,
                snapshot_identity(),
                prepared("v1"),
                GenerationCoverage::new(2, 0, 0, 0),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("baseline generation should stage");
        store
            .activate(active, 0, deadline())
            .expect("baseline generation should activate");
        store.shutdown(deadline()).expect("writer should stop");

        let raw = Connection::open(&database).expect("fixture database should open");
        raw.execute("DROP TRIGGER artifact_facts_no_update", [])
            .expect("fixture should remove the immutable-row guard");
        assert_eq!(
            raw.execute(
                "UPDATE artifact_facts SET name = 'tampered'
                 WHERE artifact_digest = (
                    SELECT artifact_digest FROM artifact_facts
                    ORDER BY artifact_digest LIMIT 1
                 ) AND ordinal = 0",
                [],
            )
            .expect("fixture should corrupt one complete artifact"),
            1
        );
        drop(raw);

        let (store, _) =
            OwnedSqliteIndex::start(&database, 123, deadline()).expect("store should reopen");
        store
            .register_workspace(repository, 0, deadline())
            .expect("workspace should remain registered");
        assert_eq!(
            store
                .stage(
                    0,
                    snapshot_identity(),
                    prepared("v1"),
                    GenerationCoverage::new(2, 0, 0, 0),
                    Arc::new(AtomicBool::new(false)),
                    deadline(),
                )
                .expect_err("tampered immutable facts must fail reuse"),
            SqliteStoreError::IntegrityCheckFailed
        );
        assert_eq!(
            store
                .active_generation(repository, deadline())
                .expect("active generation should remain readable"),
            Some(active)
        );
        store.shutdown(deadline()).expect("writer should stop");
    }

    #[test]
    #[allow(
        clippy::cognitive_complexity,
        clippy::too_many_lines,
        reason = "one end-to-end test keeps projection damage, failed rebuilds, pinned reads, and both slot switches in their required order"
    )]
    fn projection_rebuild_is_bounded_atomic_repeatable_and_recovers_a_missing_slot() {
        let directory = TempDirectory::new();
        let database = directory.database();
        let (store, _) =
            OwnedSqliteIndex::start(&database, 123, deadline()).expect("owned store should start");
        let repository = snapshot_identity().repository();
        store
            .register_workspace(repository, 0, deadline())
            .expect("workspace should register");
        let prepared = prepared_many(300);
        let expected_rows = prepared.total_facts();
        assert!(expected_rows > 256);
        let generation = store
            .stage(
                0,
                snapshot_identity(),
                prepared,
                GenerationCoverage::new(1, 0, 0, 0),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("generation should stage");
        store
            .activate(generation, 0, deadline())
            .expect("generation should activate");
        let reader = OwnedSqliteReader::start(&database, deadline()).expect("reader should start");
        let search = || {
            reader
                .search(
                    repository,
                    "symbol_0299",
                    SearchLimits::default(),
                    Arc::new(AtomicBool::new(false)),
                    deadline(),
                )
                .expect("active projection should remain searchable")
        };
        let baseline = search();
        assert_eq!(baseline.hits().len(), 1);

        let raw = Connection::open(&database).expect("fixture connection should open");
        raw.execute(
            "DELETE FROM generation_search
             WHERE generation_id = ?1 AND name = 'symbol_0299'",
            [generation.get()],
        )
        .expect("fixture should remove one projected fact");
        assert!(
            reader
                .search(
                    repository,
                    "symbol_0299",
                    SearchLimits::default(),
                    Arc::new(AtomicBool::new(false)),
                    deadline(),
                )
                .expect("damaged projection should still be queryable")
                .hits()
                .is_empty()
        );

        let too_small = ProjectionRebuildLimits::try_new(expected_rows - 1)
            .expect("fixture row limit should be valid");
        assert_eq!(
            store
                .rebuild_search_projection(too_small, Arc::new(AtomicBool::new(false)), deadline(),)
                .expect_err("row-limited rebuild should fail closed"),
            SqliteStoreError::ProjectionRebuildRowLimitExceeded
        );
        assert!(
            reader
                .search(
                    repository,
                    "symbol_0299",
                    SearchLimits::default(),
                    Arc::new(AtomicBool::new(false)),
                    deadline(),
                )
                .expect("failed rebuild must not switch projections")
                .hits()
                .is_empty()
        );

        assert_eq!(
            store
                .rebuild_search_projection(
                    ProjectionRebuildLimits::default(),
                    Arc::new(AtomicBool::new(true)),
                    deadline(),
                )
                .expect_err("pre-cancelled rebuild should fail"),
            SqliteStoreError::Cancelled
        );
        let mut pinned_connection =
            Connection::open(&database).expect("pinned fixture connection should open");
        let pinned = pinned_connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .expect("pinned read should begin");
        let pinned_before: i64 = pinned
            .query_row(
                "SELECT active_slot FROM search_projection_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .expect("pinned slot should be readable");
        assert_eq!(pinned_before, 0);

        let first = store
            .rebuild_search_projection(
                ProjectionRebuildLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("projection should rebuild");
        assert_eq!(first.previous_slot(), 0);
        assert_eq!(first.active_slot(), 1);
        assert_eq!(first.rebuilt_rows(), expected_rows);
        assert_eq!(first.write_batches(), expected_rows.div_ceil(256));
        let pinned_after: i64 = pinned
            .query_row(
                "SELECT active_slot FROM search_projection_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .expect("pinned read should keep its original slot");
        assert_eq!(pinned_after, 0);
        let published_slot: i64 = raw
            .query_row(
                "SELECT active_slot FROM search_projection_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .expect("new read should observe the published slot");
        assert_eq!(published_slot, 1);
        assert_eq!(search(), baseline);

        raw.execute_batch("DROP TABLE generation_search")
            .expect("fixture should remove the inactive projection table");
        let second = store
            .rebuild_search_projection(
                ProjectionRebuildLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("missing inactive table should be recreated");
        assert_eq!(second.previous_slot(), 1);
        assert_eq!(second.active_slot(), 0);
        assert_eq!(second.rebuilt_rows(), expected_rows);
        let pinned_rows: i64 = pinned
            .query_row(
                "SELECT count(*) FROM generation_search WHERE generation_id = ?1",
                [generation.get()],
                |row| row.get(0),
            )
            .expect("old read should retain the dropped slot snapshot");
        assert_eq!(
            pinned_rows,
            i64::try_from(expected_rows - 1).expect("fixture row count should fit")
        );
        pinned.commit().expect("pinned read should commit");
        let republished_slot: i64 = pinned_connection
            .query_row(
                "SELECT active_slot FROM search_projection_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .expect("new read should observe the second published slot");
        assert_eq!(republished_slot, 0);
        assert_eq!(search(), baseline);

        let damaged_blocks = raw
            .execute(
                "UPDATE generation_search_rebuild_data SET block = X'00'
                 WHERE id = (SELECT min(id) FROM generation_search_rebuild_data)",
                [],
            )
            .expect("fixture should damage the inactive FTS index");
        assert_eq!(damaged_blocks, 1);
        let third = store
            .rebuild_search_projection(
                ProjectionRebuildLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("damaged inactive FTS internals should be recreated");
        assert_eq!(third.previous_slot(), 0);
        assert_eq!(third.active_slot(), 1);
        assert_eq!(third.rebuilt_rows(), expected_rows);
        assert_eq!(search(), baseline);

        drop(raw);
        let backup_path = directory.0.join("projection-backup.sqlite3");
        create_online_backup(
            &database,
            &backup_path,
            BackupLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("rebuilt projection should back up");
        let backup_reader =
            OwnedSqliteReader::start(&backup_path, deadline()).expect("backup reader should start");
        let backup_results = backup_reader
            .search(
                repository,
                "symbol_0299",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("backed-up active slot should be searchable");
        assert_eq!(backup_results, baseline);
        backup_reader
            .shutdown(deadline())
            .expect("backup reader should stop");
        reader.shutdown(deadline()).expect("reader should stop");
        store.shutdown(deadline()).expect("writer should stop");

        let (restarted, startup) = OwnedSqliteIndex::start(&database, 456, deadline())
            .expect("rebuilt store should restart");
        assert_eq!(startup.recovered_generations(), 0);
        let restarted_reader =
            OwnedSqliteReader::start(&database, deadline()).expect("restarted reader should start");
        let restarted_results = restarted_reader
            .search(
                repository,
                "symbol_0299",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("restarted active slot should be searchable");
        assert_eq!(restarted_results, baseline);
        restarted_reader
            .shutdown(deadline())
            .expect("restarted reader should stop");
        restarted
            .shutdown(deadline())
            .expect("restarted writer should stop");
    }

    #[test]
    fn projection_rebuild_limits_are_explicit_and_inclusive() {
        assert_eq!(
            ProjectionRebuildLimits::try_new(0),
            Err(SqliteStoreError::InvalidProjectionRebuildLimits)
        );
        assert_eq!(
            ProjectionRebuildLimits::try_new(100_000_001),
            Err(SqliteStoreError::InvalidProjectionRebuildLimits)
        );
        assert_eq!(
            ProjectionRebuildLimits::try_new(100_000_000)
                .expect("hard ceiling should be inclusive")
                .max_rows(),
            100_000_000
        );
        assert_eq!(ProjectionRebuildLimits::default().max_rows(), 5_000_000);
    }

    #[test]
    fn process_mutation_lease_prevents_competing_recovery_and_releases_on_shutdown() {
        let directory = TempDirectory::new();
        let database = directory.database();
        let (first, _) =
            OwnedSqliteIndex::start(&database, 123, deadline()).expect("first owner should start");
        let repository = snapshot_identity().repository();
        first
            .register_workspace(repository, 0, deadline())
            .expect("workspace should register");
        let ready = first
            .stage(
                0,
                snapshot_identity(),
                prepared("owned"),
                GenerationCoverage::new(2, 0, 0, 0),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("first owner should retain its ready generation");

        let competing_deadline = Instant::now()
            .checked_add(Duration::from_millis(30))
            .expect("competing deadline should be representable");
        let competing_result = OwnedSqliteIndex::start(
            &directory.0.join(".").join("index.sqlite3"),
            456,
            competing_deadline,
        );
        assert!(matches!(
            competing_result,
            Err(SqliteStoreError::DeadlineExceeded)
        ));
        first
            .activate(ready, 0, deadline())
            .expect("competing startup must not invalidate ready work");
        first.shutdown(deadline()).expect("first owner should stop");

        let (replacement, startup) = OwnedSqliteIndex::start(&database, 789, deadline())
            .expect("the lease should release with its owner");
        assert_eq!(startup.recovered_generations(), 0);
        assert_eq!(
            replacement
                .active_generation(repository, deadline())
                .expect("active generation should survive owner replacement"),
            Some(ready)
        );
        replacement
            .shutdown(deadline())
            .expect("replacement owner should stop");
    }

    #[test]
    fn unavailable_mutation_lease_fails_before_database_creation() {
        let directory = TempDirectory::new();
        let database = directory.database();
        fs::create_dir(mutation_lease_path(&database))
            .expect("fixture should make the lease path unopenable");

        let result = OwnedSqliteIndex::start(&database, 123, deadline());
        assert!(matches!(
            result,
            Err(SqliteStoreError::MutationLeaseUnavailable)
        ));
        assert!(!database.exists());
    }

    #[test]
    fn stale_and_cancelled_candidates_never_replace_active() {
        let directory = TempDirectory::new();
        let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
            .expect("owned store should start");
        let repository = snapshot_identity().repository();
        store
            .register_workspace(repository, 0, deadline())
            .expect("workspace should register");
        let first = store
            .stage(
                0,
                snapshot_identity(),
                prepared("v1"),
                GenerationCoverage::new(2, 0, 0, 0),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("first generation should stage");
        store
            .activate(first, 0, deadline())
            .expect("first generation should activate");

        let cancelled = Arc::new(AtomicBool::new(true));
        assert_eq!(
            store
                .stage(
                    0,
                    snapshot_identity(),
                    prepared("cancelled"),
                    GenerationCoverage::new(2, 0, 0, 0),
                    cancelled,
                    deadline(),
                )
                .expect_err("cancelled work should fail"),
            SqliteStoreError::Cancelled
        );
        store
            .advance_source_epoch(repository, 0, 1, deadline())
            .expect("source epoch should advance");
        assert_eq!(
            store
                .activate(first, 0, deadline())
                .expect_err("stale activation should fail"),
            SqliteStoreError::StaleSourceEpoch
        );
        assert_eq!(
            store
                .active_generation(repository, deadline())
                .expect("previous generation should remain active"),
            Some(first)
        );
        store.shutdown(deadline()).expect("worker should stop");
    }

    #[test]
    fn restart_marks_ready_generation_failed_and_removes_scoped_rows() {
        let directory = TempDirectory::new();
        let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
            .expect("owned store should start");
        let repository = snapshot_identity().repository();
        store
            .register_workspace(repository, 0, deadline())
            .expect("workspace should register");
        let ready = store
            .stage(
                0,
                snapshot_identity(),
                prepared("unpublished"),
                GenerationCoverage::new(2, 0, 0, 0),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("candidate should become ready");
        store.shutdown(deadline()).expect("worker should stop");

        let (store, startup) = OwnedSqliteIndex::start(&directory.database(), 456, deadline())
            .expect("owned store should recover");
        assert_eq!(startup.recovered_generations(), 1);
        assert_eq!(
            store
                .active_generation(repository, deadline())
                .expect("workspace should remain readable"),
            None
        );
        store.shutdown(deadline()).expect("worker should stop");

        let connection =
            Connection::open(directory.database()).expect("database should reopen for inspection");
        let state: String = connection
            .query_row(
                "SELECT lifecycle_state FROM index_generations WHERE generation_id = ?1",
                [ready.get()],
                |row| row.get(0),
            )
            .expect("recovered generation should remain auditable");
        let scoped_rows: i64 = connection
            .query_row(
                "SELECT
                    (SELECT count(*) FROM generation_files WHERE generation_id = ?1) +
                    (SELECT count(*) FROM generation_facts WHERE generation_id = ?1) +
                    (SELECT count(*) FROM generation_search WHERE generation_id = ?1)",
                [ready.get()],
                |row| row.get(0),
            )
            .expect("scoped rows should be inspectable");
        assert_eq!(state, "failed");
        assert_eq!(scoped_rows, 0);
    }

    fn insert_incomplete_generation_fixture(database: &Path, generation_count: usize) {
        let mut connection =
            Connection::open(database).expect("fixture database should reopen for insertion");
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .expect("fixture transaction should start");
        transaction
            .execute(
                "INSERT INTO workspaces(
                    workspace_id, repository_identity, source_epoch
                 ) VALUES (1, zeroblob(32), 0)",
                [],
            )
            .expect("fixture workspace should be inserted");
        transaction
            .execute(
                "INSERT INTO source_snapshots(
                    snapshot_digest, lifecycle_state, repository_identity,
                    git_state_digest, worktree_state_digest, configuration_digest,
                    producer_manifest_digest, analysis_schema_digest,
                    canonicalization_version, manifest_digest, file_count,
                    total_source_bytes, total_syntax_error_nodes
                 ) VALUES (
                    zeroblob(32), 'complete', zeroblob(32), zeroblob(32),
                    zeroblob(32), zeroblob(32), zeroblob(32), zeroblob(32),
                    1, zeroblob(32), 0, 0, 0
                 )",
                [],
            )
            .expect("fixture snapshot should be inserted");
        for generation_id in 1..=generation_count {
            let generation_id =
                i64::try_from(generation_id).expect("fixture generation should fit in SQLite");
            transaction
                .execute(
                    "INSERT INTO index_generations(
                        generation_id, workspace_id, source_epoch,
                        snapshot_digest, lifecycle_state
                     ) VALUES (?1, 1, 0, zeroblob(32), 'discovered')",
                    params![generation_id],
                )
                .expect("fixture generation should be inserted");
        }
        transaction.commit().expect("fixture rows should commit");
    }

    #[test]
    fn startup_recovery_limit_fails_without_partially_changing_generations() {
        let directory = TempDirectory::new();
        let database = directory.database();
        let (store, _) = OwnedSqliteIndex::start(&database, 123, deadline())
            .expect("owned store should initialize the schema");
        store.shutdown(deadline()).expect("worker should stop");

        insert_incomplete_generation_fixture(&database, MAX_STARTUP_RECOVERY_GENERATIONS + 1);

        let error = match OwnedSqliteIndex::start(&database, 456, deadline()) {
            Ok(_) => panic!("over-limit recovery should fail"),
            Err(error) => error,
        };
        assert_eq!(error, SqliteStoreError::RecoveryGenerationLimitExceeded);

        let connection =
            Connection::open(&database).expect("fixture database should reopen for validation");
        let discovered: i64 = connection
            .query_row(
                "SELECT count(*) FROM index_generations
                 WHERE lifecycle_state = 'discovered'",
                [],
                |row| row.get(0),
            )
            .expect("fixture generations should remain queryable");
        assert_eq!(
            discovered,
            i64::try_from(MAX_STARTUP_RECOVERY_GENERATIONS + 1)
                .expect("fixture count should fit in SQLite")
        );
        connection
            .execute(
                "DELETE FROM index_generations
                 WHERE generation_id = ?1",
                params![
                    i64::try_from(MAX_STARTUP_RECOVERY_GENERATIONS + 1)
                        .expect("fixture generation should fit in SQLite")
                ],
            )
            .expect("one fixture generation should be removed");
        drop(connection);

        let (store, startup) = OwnedSqliteIndex::start(&database, 789, deadline())
            .expect("the inclusive recovery limit should succeed");
        assert_eq!(
            startup.recovered_generations(),
            u64::try_from(MAX_STARTUP_RECOVERY_GENERATIONS)
                .expect("fixture count should fit in the report")
        );
        store.shutdown(deadline()).expect("worker should stop");

        let connection =
            Connection::open(&database).expect("recovered database should reopen for validation");
        let failed: i64 = connection
            .query_row(
                "SELECT count(*) FROM index_generations
                 WHERE lifecycle_state = 'failed'",
                [],
                |row| row.get(0),
            )
            .expect("recovered generations should remain queryable");
        assert_eq!(
            failed,
            i64::try_from(MAX_STARTUP_RECOVERY_GENERATIONS)
                .expect("fixture count should fit in SQLite")
        );
    }

    #[test]
    fn restart_removes_incomplete_snapshot_and_artifact_staging() {
        let directory = TempDirectory::new();
        let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
            .expect("owned store should start");
        store.shutdown(deadline()).expect("worker should stop");
        let connection =
            Connection::open(directory.database()).expect("database should reopen for fixture");
        connection
            .execute(
                "INSERT INTO source_snapshots(
                    snapshot_digest, lifecycle_state, repository_identity, git_state_digest,
                    worktree_state_digest, configuration_digest, producer_manifest_digest,
                    analysis_schema_digest, canonicalization_version, manifest_digest,
                    file_count, total_source_bytes, total_syntax_error_nodes
                 ) VALUES (
                    zeroblob(32), 'staging', zeroblob(32), zeroblob(32),
                    zeroblob(32), zeroblob(32), zeroblob(32), zeroblob(32),
                    1, zeroblob(32), 0, 0, 0
                 )",
                [],
            )
            .expect("incomplete snapshot fixture should insert");
        connection
            .execute(
                "INSERT INTO analysis_artifacts(
                    artifact_digest, lifecycle_state, source_content_digest,
                    producer_manifest_digest, configuration_digest, analysis_schema_digest,
                    canonicalization_version, fact_count, visited_nodes, syntax_error_nodes
                 ) VALUES (
                    zeroblob(32), 'staging', zeroblob(32), zeroblob(32),
                    zeroblob(32), zeroblob(32), 1, 0, 0, 0
                 )",
                [],
            )
            .expect("incomplete artifact fixture should insert");
        drop(connection);

        let (store, startup) = OwnedSqliteIndex::start(&directory.database(), 456, deadline())
            .expect("owned store should recover");
        assert_eq!(startup.recovered_generations(), 0);
        store.shutdown(deadline()).expect("worker should stop");
        let connection =
            Connection::open(directory.database()).expect("database should reopen for inspection");
        let staging: i64 = connection
            .query_row(
                "SELECT
                    (SELECT count(*) FROM source_snapshots WHERE lifecycle_state = 'staging') +
                    (SELECT count(*) FROM analysis_artifacts WHERE lifecycle_state = 'staging')",
                [],
                |row| row.get(0),
            )
            .expect("staging counts should be readable");
        assert_eq!(staging, 0);
    }
}
