use std::{
    ffi::OsString,
    fs::{File, OpenOptions, TryLockError},
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
    MemoryImportApproval, MemoryImportReceipt, PreparedRustIndex, RustIndexCoverage,
    RustSourceSnapshotIdentity,
};
use repowitness_domain::{
    MemoryAuditActorId, MemoryObservationSource, MemoryPresentationDigest, MemoryRecord,
    MemoryRecordedAtUnixMillis, RepositoryIdentityDigest, RustSymbolMemoryEvidence,
};

use super::{
    CheckpointOutcome, GenerationCoverage, GenerationId, ProjectionRebuildLimits,
    ProjectionRebuildOutcome, SqliteStoreError, canonical_database_path, database_file_identity,
    memory_projection::{
        LoadedMemoryJournal, LoadedRustCandidateSet, MemoryProjectionLoadLimits,
        MemoryProjectionPublication, MemoryProjectionSource, PreparedMemoryProjection,
    },
    memory_review::{
        LoadedCorrespondenceReviews, MemoryCorrespondenceReviewReceipt,
        PreparedMemoryCorrespondenceReview,
    },
    open_index_writer_with_identity_until,
    writer::{PreparedMemoryImport, WriteControl, WriterState},
};
use crate::{
    contained_source::FileIdentity,
    memory_format::{
        MemoryFormatControl, MemoryFormatError, canonical_memory_json, digest_canonical_bytes,
    },
};

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
    ImportMemory(Box<MemoryImportCommand>),
    AppendMemoryCorrespondenceReview(Box<AppendMemoryCorrespondenceReviewCommand>),
    LoadMemorySource {
        repository: RepositoryIdentityDigest,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
        reply: Reply<MemoryProjectionSource>,
    },
    LoadMemoryJournal(Box<LoadMemoryJournalCommand>),
    LoadRustMemoryCandidates(Box<LoadRustMemoryCandidatesCommand>),
    LoadMemoryCorrespondenceReviews(Box<LoadMemoryCorrespondenceReviewsCommand>),
    PublishMemoryProjection(Box<PublishMemoryProjectionCommand>),
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

struct MemoryImportCommand {
    prepared: PreparedMemoryImport,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<MemoryImportReceipt>,
}

struct AppendMemoryCorrespondenceReviewCommand {
    prepared: PreparedMemoryCorrespondenceReview,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<MemoryCorrespondenceReviewReceipt>,
}

struct LoadMemoryJournalCommand {
    repository: RepositoryIdentityDigest,
    limits: MemoryProjectionLoadLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<LoadedMemoryJournal>,
}

struct LoadRustMemoryCandidatesCommand {
    source: MemoryProjectionSource,
    evidence: RustSymbolMemoryEvidence,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<LoadedRustCandidateSet>,
}

struct LoadMemoryCorrespondenceReviewsCommand {
    source: MemoryProjectionSource,
    record_id: repowitness_domain::MemoryRecordId,
    revision: repowitness_domain::CanonicalMemoryDigest,
    evidence_ordinal: u8,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<LoadedCorrespondenceReviews>,
}

struct PublishMemoryProjectionCommand {
    prepared: PreparedMemoryProjection,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<MemoryProjectionPublication>,
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

include!("worker/ports.rs");
include!("worker/memory_commands.rs");
include!("worker/run_loop.rs");

#[cfg(test)]
mod tests;
