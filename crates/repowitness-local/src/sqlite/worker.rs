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
    ConnectedWorkspaceId, MemoryAuditActorId, MemoryCommitId, MemoryObservationSource,
    MemoryPresentationDigest, MemoryRecord, MemoryRecordedAtUnixMillis, RepositoryIdentityDigest,
    RustSymbolMemoryEvidence, SourceSlotId,
};

use super::{
    CheckpointOutcome, GenerationCoverage, GenerationId, GenerationRetentionPolicy,
    PinnedWorkspaceView, PreparedRustGraphGeneration, PreparedScipOverlay, ProjectionRebuildLimits,
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
    writer::{
        PreparedMemoryImport, SourceSlotReservation, WriteControl, WriterMutationResult,
        WriterState,
    },
};
use crate::{
    contained_source::FileIdentity,
    memory_format::{
        MemoryFormatControl, MemoryFormatError, canonical_memory_json, digest_canonical_bytes,
    },
};

type Reply<T> = SyncSender<Result<T, SqliteStoreError>>;

include!("worker/commands.rs");
include!("worker/scip_overlay_commands.rs");

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
    // Sticky after a lost mutation receipt so every mutating command shares
    // one conservative client-side outcome state.
    unresolved_mutation: Arc<AtomicBool>,
}

include!("worker/lifecycle.rs");

impl OwnedSqliteIndex {
    /// Starts the writer owner, migrates the database, and performs recovery.
    ///
    /// If startup returns [`SqliteStoreError::MutationOutcomeUnknown`], reopen
    /// the store and run read-only diagnostics before retrying: bounded recovery
    /// may have committed before its receipt was lost.
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
        Self::start_with_lease_and_hooks(
            mutation_lease,
            database_identity,
            applied_at_unix_ms,
            cancelled,
            deadline,
            WriterHooks::default(),
        )
    }

    #[cfg(test)]
    fn start_with_post_commit_pause(
        path: &Path,
        applied_at_unix_ms: u64,
        deadline: Instant,
        hook: impl FnMut() + Send + 'static,
    ) -> Result<(Self, IndexStoreStartup), SqliteStoreError> {
        Self::start_with_test_hooks(
            path,
            applied_at_unix_ms,
            deadline,
            WriterHooks::with_post_commit_pause(hook),
        )
    }

    #[cfg(test)]
    fn start_with_progress_handler_clear_failure(
        path: &Path,
        applied_at_unix_ms: u64,
        deadline: Instant,
    ) -> Result<(Self, IndexStoreStartup), SqliteStoreError> {
        Self::start_with_test_hooks(
            path,
            applied_at_unix_ms,
            deadline,
            WriterHooks::with_progress_handler_clear_failure(),
        )
    }

    #[cfg(test)]
    fn start_with_commit_failure_control(
        path: &Path,
        applied_at_unix_ms: u64,
        deadline: Instant,
    ) -> Result<(Self, IndexStoreStartup, Arc<AtomicBool>), SqliteStoreError> {
        let fail_next_commit = Arc::new(AtomicBool::new(false));
        let (store, startup) = Self::start_with_test_hooks(
            path,
            applied_at_unix_ms,
            deadline,
            WriterHooks::with_commit_failure_control(Arc::clone(&fail_next_commit)),
        )?;
        Ok((store, startup, fail_next_commit))
    }

    #[cfg(test)]
    fn start_with_read_reply_pause(
        path: &Path,
        applied_at_unix_ms: u64,
        deadline: Instant,
        hook: impl FnMut() + Send + 'static,
    ) -> Result<(Self, IndexStoreStartup), SqliteStoreError> {
        Self::start_with_test_hooks(
            path,
            applied_at_unix_ms,
            deadline,
            WriterHooks::with_read_reply_pause(hook),
        )
    }

    #[cfg(test)]
    fn start_with_shutdown_exit_pause(
        path: &Path,
        applied_at_unix_ms: u64,
        deadline: Instant,
        hook: impl FnMut() + Send + 'static,
    ) -> Result<(Self, IndexStoreStartup), SqliteStoreError> {
        Self::start_with_test_hooks(
            path,
            applied_at_unix_ms,
            deadline,
            WriterHooks::with_shutdown_exit_pause(hook),
        )
    }

    #[cfg(test)]
    fn start_with_test_hooks(
        path: &Path,
        applied_at_unix_ms: u64,
        deadline: Instant,
        hooks: WriterHooks,
    ) -> Result<(Self, IndexStoreStartup), SqliteStoreError> {
        let mutation_lease = SqliteMutationLease::acquire(path, deadline)?;
        let database_identity = database_file_identity(&mutation_lease.database_path)?;
        Self::start_with_lease_and_hooks(
            mutation_lease,
            database_identity,
            applied_at_unix_ms,
            Arc::new(AtomicBool::new(false)),
            deadline,
            hooks,
        )
    }

    fn start_with_lease_and_hooks(
        mutation_lease: SqliteMutationLease,
        database_identity: Option<FileIdentity>,
        applied_at_unix_ms: u64,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
        mut hooks: WriterHooks,
    ) -> Result<(Self, IndexStoreStartup), SqliteStoreError> {
        if Instant::now() >= deadline {
            return Err(SqliteStoreError::DeadlineExceeded);
        }
        let database_path = mutation_lease.database_path.clone();
        let startup_cancelled = Arc::clone(&cancelled);
        let unresolved_mutation = Arc::new(AtomicBool::new(false));
        let writer_unresolved_mutation = Arc::clone(&unresolved_mutation);
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
                    .and_then(
                        |(connection, opened_database_identity, migrated)| {
                            let mut state = WriterState::new(connection);
                            let recovered_generations = state
                                .recover(startup_cancelled, deadline)
                                .map_err(|error| {
                                    if migrated {
                                        SqliteStoreError::MutationOutcomeUnknown
                                    } else {
                                        error
                                    }
                                })?;
                            Ok((state, recovered_generations, opened_database_identity))
                        },
                    )
                };
                let Ok((mut state, recovered_generations, opened_database_identity)) = startup
                else {
                    let error = startup.err().unwrap_or(SqliteStoreError::WorkerUnavailable);
                    let _ = startup_sender.send(Err(error));
                    return;
                };
                if let Err(error) = hooks.install_on(&state) {
                    let _ = startup_sender.send(Err(error));
                    return;
                }
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
                run_writer(
                    &mut state,
                    receiver,
                    &mut hooks,
                    writer_unresolved_mutation.as_ref(),
                );
            })
            .map_err(|_| SqliteStoreError::WorkerUnavailable)?;
        let (startup, opened_database_identity) =
            receive_mutation_reply(&startup_receiver, Some(cancelled.as_ref()), deadline, None)?;
        Ok((
            Self {
                commands,
                worker: Some(worker),
                opened_database_identity: Some(opened_database_identity),
                unresolved_mutation,
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
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], reopen and read the
    /// durable workspace epoch before retrying.
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
        receive_mutation_reply(&receiver, None, deadline, Some(&self.unresolved_mutation))
    }

    /// Ensures one workspace exists and returns its durable source epoch.
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], reopen and read the
    /// durable workspace epoch before retrying.
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
        receive_mutation_reply(&receiver, None, deadline, Some(&self.unresolved_mutation))
    }

    /// Advances the monotonic source epoch with compare-and-set semantics.
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], reopen and read the
    /// durable workspace epoch before retrying.
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
        receive_mutation_reply(&receiver, None, deadline, Some(&self.unresolved_mutation))
    }

    /// Materializes one complete prepared index and leaves it ready to activate.
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], reopen the store, allow
    /// startup recovery to classify incomplete generations, and compare active
    /// state before retrying.
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
        match receive_mutation_reply(
            &receiver,
            Some(cancelled.as_ref()),
            deadline,
            Some(&self.unresolved_mutation),
        ) {
            Ok(generation) => Ok(generation),
            Err(error) => {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
                Err(error)
            }
        }
    }

    /// Stages one complete candidate while a source-slot reservation is current.
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], reopen the store and
    /// inspect the source-slot state and candidate generation before retrying.
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
        match receive_mutation_reply(
            &receiver,
            Some(cancelled.as_ref()),
            deadline,
            Some(&self.unresolved_mutation),
        ) {
            Ok(generation) => Ok(generation),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Atomically activates one ready generation if its source epoch is current.
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], read the active
    /// generation and durable source-slot completion before retrying.
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
        receive_mutation_reply(&receiver, None, deadline, Some(&self.unresolved_mutation))
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
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], reopen the store and
    /// inspect the active projection through a read before rebuilding again.
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
        match receive_mutation_reply(
            &receiver,
            Some(cancelled.as_ref()),
            deadline,
            Some(&self.unresolved_mutation),
        ) {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
                Err(error)
            }
        }
    }

    /// Runs one explicit truncating checkpoint on the writer connection.
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], inspect the already
    /// published active state before retrying this idempotent maintenance step.
    pub fn checkpoint(&self, deadline: Instant) -> Result<CheckpointOutcome, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(WriterCommand::Checkpoint { deadline, reply }, deadline)?;
        receive_mutation_reply(&receiver, None, deadline, Some(&self.unresolved_mutation))
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
