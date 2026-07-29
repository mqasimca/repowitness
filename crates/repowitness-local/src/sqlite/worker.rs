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
    CompletedSourceSlotIndex, MemoryImportApproval, MemoryImportReceipt, PreparedRustIndex,
    RustIndexCoverage, RustSourceSnapshotIdentity, SourceSlotEpoch,
};
use repowitness_domain::{
    ConnectedWorkspaceId, MemoryAuditActorId, MemoryObservationSource, MemoryPresentationDigest,
    MemoryRecord, MemoryRecordedAtUnixMillis, RepositoryIdentityDigest, RustSymbolMemoryEvidence,
    SourceSlotId,
};

use super::{
    CheckpointOutcome, GenerationCoverage, GenerationId, GenerationRetentionPolicy,
    PinnedWorkspaceView, PreparedRustGraphGeneration, ProjectionRebuildLimits,
    ProjectionRebuildOutcome, RetentionApplyOutcome, RetentionApplyRequest, RetentionPlan,
    RetentionPlanDigest, RetentionPlanRequest, SourceSlotState, SqliteStoreError,
    WorkspaceSourceSlot, WorkspaceViewId, WorkspaceViewMember, canonical_database_path,
    database_file_identity,
    memory_projection::{
        LoadedMemoryJournal, LoadedRustCandidateSet, MemoryProjectionLoadLimits,
        MemoryProjectionPublication, MemoryProjectionSource, PreparedMemoryProjection,
    },
    memory_review::{
        LoadedCorrespondenceReviews, MemoryCorrespondenceReviewReceipt,
        PreparedMemoryCorrespondenceReview,
    },
    open_index_writer_with_identity_until,
    writer::{PreparedMemoryImport, SourceSlotReservation, WriteControl, WriterState},
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
        Self::acquire_with_cancel(database_path, None, deadline)
    }

    pub(crate) fn acquire_with_cancel(
        database_path: &Path,
        cancelled: Option<&AtomicBool>,
        deadline: Instant,
    ) -> Result<Self, SqliteStoreError> {
        if cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Acquire)) {
            return Err(SqliteStoreError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(SqliteStoreError::DeadlineExceeded);
        }
        let database_path = canonical_database_path(database_path)?;
        let file = acquire_mutation_lease(&database_path, cancelled, deadline)?;
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
    EnsureWorkspace {
        repository: RepositoryIdentityDigest,
        initial_source_epoch: u64,
        deadline: Instant,
        reply: Reply<SourceSlotEpoch>,
    },
    AdvanceEpoch {
        repository: RepositoryIdentityDigest,
        expected: u64,
        next: u64,
        deadline: Instant,
        reply: Reply<()>,
    },
    Stage(Box<StageCommand>),
    StageSourceSlot(Box<StageSourceSlotCommand>),
    StageGraph(Box<StageGraphCommand>),
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
    ConnectWorkspace(Box<ConnectWorkspaceCommand>),
    SourceSlotState(Box<SourceSlotStateCommand>),
    ReserveSourceSlotEpoch(Box<ReserveSourceSlotEpochCommand>),
    CompleteSourceSlotEpoch(Box<CompleteSourceSlotEpochCommand>),
    PublishWorkspaceView(Box<PublishWorkspaceViewCommand>),
    ActiveWorkspaceView {
        connected_workspace: ConnectedWorkspaceId,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
        reply: Reply<Option<PinnedWorkspaceView>>,
    },
    RebuildProjection(Box<RebuildProjectionCommand>),
    PlanRetention(Box<PlanRetentionCommand>),
    ApplyRetention(Box<ApplyRetentionCommand>),
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

struct StageSourceSlotCommand {
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    reserved_epoch: SourceSlotEpoch,
    identity: RustSourceSnapshotIdentity,
    prepared: PreparedRustIndex,
    coverage: RustIndexCoverage,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<GenerationId>,
}

struct StageGraphCommand {
    generation: GenerationId,
    prepared: PreparedRustGraphGeneration,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<()>,
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

struct ConnectWorkspaceCommand {
    connected_workspace: ConnectedWorkspaceId,
    source_slots: Box<[WorkspaceSourceSlot]>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<()>,
}

struct SourceSlotStateCommand {
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<SourceSlotState>,
}

struct ReserveSourceSlotEpochCommand {
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    expected: SourceSlotEpoch,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<SourceSlotEpoch>,
}

struct CompleteSourceSlotEpochCommand {
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    source_epoch: SourceSlotEpoch,
    generation: GenerationId,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<()>,
}

struct PublishWorkspaceViewCommand {
    connected_workspace: ConnectedWorkspaceId,
    members: Box<[WorkspaceViewMember]>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<WorkspaceViewId>,
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
    opened_database_identity: Option<FileIdentity>,
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
        let startup_cancelled = Arc::clone(&cancelled);
        let (commands, receiver) = mpsc::sync_channel(1);
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("repowitness-sqlite-writer".to_owned())
            .spawn(move || {
                // The owner thread retains the lease even if its client is
                // dropped while a queued command is still completing.
                let _mutation_lease = mutation_lease;
                let startup = if startup_cancelled.load(Ordering::Acquire) {
                    Err(SqliteStoreError::Cancelled)
                } else if Instant::now() >= deadline {
                    Err(SqliteStoreError::DeadlineExceeded)
                } else {
                    open_index_writer_with_identity_until(
                        &database_path,
                        database_identity,
                        applied_at_unix_ms,
                        Arc::clone(&startup_cancelled),
                        deadline,
                    )
                    .and_then(|(connection, opened_database_identity)| {
                        let mut state = WriterState::new(connection);
                        let recovered_generations = state.recover(startup_cancelled, deadline)?;
                        Ok((state, recovered_generations, opened_database_identity))
                    })
                };
                let Ok((mut state, recovered_generations, opened_database_identity)) = startup
                else {
                    let error = startup.err().unwrap_or(SqliteStoreError::WorkerUnavailable);
                    let _ = startup_sender.send(Err(error));
                    return;
                };
                if startup_sender
                    .send(Ok((
                        IndexStoreStartup {
                            recovered_generations,
                        },
                        opened_database_identity,
                    )))
                    .is_err()
                {
                    return;
                }
                run_writer(&mut state, receiver);
            })
            .map_err(|_| SqliteStoreError::WorkerUnavailable)?;
        let (startup, opened_database_identity) =
            receive_mutation_reply(&startup_receiver, Some(cancelled.as_ref()), deadline)?;
        Ok((
            Self {
                commands,
                worker: Some(worker),
                opened_database_identity: Some(opened_database_identity),
            },
            startup,
        ))
    }

    /// Returns the exact database identity retained from the guarded writer open.
    pub(crate) fn opened_database_identity(&self) -> &FileIdentity {
        self.opened_database_identity
            .as_ref()
            .expect("a successfully started writer retains its opened database identity")
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
        receive_mutation_reply(&receiver, None, deadline)
    }

    pub(crate) fn ensure_workspace(
        &self,
        repository: RepositoryIdentityDigest,
        initial_source_epoch: u64,
        deadline: Instant,
    ) -> Result<SourceSlotEpoch, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::EnsureWorkspace {
                repository,
                initial_source_epoch,
                deadline,
                reply,
            },
            deadline,
        )?;
        receive_mutation_reply(&receiver, None, deadline)
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
        receive_mutation_reply(&receiver, None, deadline)
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
        match receive_mutation_reply(&receiver, Some(cancelled.as_ref()), deadline) {
            Ok(generation) => Ok(generation),
            Err(error) => {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
                Err(error)
            }
        }
    }

    /// Stages one complete candidate while a source-slot reservation is current.
    #[allow(
        clippy::too_many_arguments,
        reason = "slot identity, source identity, coverage, and control remain explicit"
    )]
    pub fn stage_source_slot(
        &self,
        connected_workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        reserved_epoch: SourceSlotEpoch,
        identity: RustSourceSnapshotIdentity,
        prepared: PreparedRustIndex,
        coverage: GenerationCoverage,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<GenerationId, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::StageSourceSlot(Box::new(StageSourceSlotCommand {
                connected_workspace,
                source_slot,
                reserved_epoch,
                identity,
                prepared,
                coverage,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_mutation_reply(&receiver, Some(cancelled.as_ref()), deadline) {
            Ok(generation) => Ok(generation),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
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
        receive_mutation_reply(&receiver, None, deadline)
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
        match receive_mutation_reply(&receiver, Some(cancelled.as_ref()), deadline) {
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
        receive_mutation_reply(&receiver, None, deadline)
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
    cancelled: Option<&AtomicBool>,
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
        if cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Acquire)) {
            return Err(SqliteStoreError::Cancelled);
        }
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

include!("worker/graph_commands.rs");

include!("worker/ports.rs");
include!("worker/workspace_commands.rs");
include!("worker/retention_commands.rs");
include!("worker/memory_commands.rs");
include!("worker/run_loop.rs");

#[cfg(test)]
mod tests;
