use super::writer::{PersonalMemoryReceipt, TaskCheckpointReceipt, TaskVerificationReceipt};

impl OwnedSqliteIndex {
    /// Returns the polling-safe status for one task in the exact repository scope.
    pub fn task_status(
        &self,
        repository: RepositoryIdentityDigest,
        task_id: TaskId,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
        deadline: Instant,
    ) -> Result<Option<TaskStatus>, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::TaskStatus(Box::new(TaskStatusCommand {
                repository,
                task_id,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        receive_reply(&receiver, deadline).inspect_err(|_| {
            cancelled.store(true, std::sync::atomic::Ordering::Release);
        })
    }

    /// Appends one local-only immutable personal-memory revision through the sole SQLite owner.
    pub fn append_personal_memory(
        &self,
        record: PersonalMemoryRecord,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
        deadline: Instant,
    ) -> Result<PersonalMemoryReceipt, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::AppendPersonalMemory(Box::new(PersonalMemoryCommand {
                record,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        receive_mutation_reply(
            &receiver,
            Some(cancelled.as_ref()),
            deadline,
            Some(&self.unresolved_mutation),
        )
    }

    /// Appends one structured durable task checkpoint through the sole SQLite owner.
    pub fn append_task_checkpoint(
        &self,
        checkpoint: TaskCheckpoint,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
        deadline: Instant,
    ) -> Result<TaskCheckpointReceipt, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::AppendTaskCheckpoint(Box::new(TaskCheckpointCommand {
                checkpoint,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        receive_mutation_reply(
            &receiver,
            Some(cancelled.as_ref()),
            deadline,
            Some(&self.unresolved_mutation),
        )
    }

    /// Appends one immutable verification receipt for an existing checkpoint.
    pub fn append_task_verification(
        &self,
        verification: TaskVerification,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
        deadline: Instant,
    ) -> Result<TaskVerificationReceipt, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::AppendTaskVerification(Box::new(TaskVerificationCommand {
                verification,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        receive_mutation_reply(
            &receiver,
            Some(cancelled.as_ref()),
            deadline,
            Some(&self.unresolved_mutation),
        )
    }
}

pub(super) struct TaskCheckpointCommand {
    pub(super) checkpoint: TaskCheckpoint,
    pub(super) cancelled: Arc<std::sync::atomic::AtomicBool>,
    pub(super) deadline: Instant,
    pub(super) reply: Reply<TaskCheckpointReceipt>,
}

pub(super) struct PersonalMemoryCommand {
    pub(super) record: PersonalMemoryRecord,
    pub(super) cancelled: Arc<std::sync::atomic::AtomicBool>,
    pub(super) deadline: Instant,
    pub(super) reply: Reply<PersonalMemoryReceipt>,
}

pub(super) struct TaskStatusCommand {
    pub(super) repository: RepositoryIdentityDigest,
    pub(super) task_id: TaskId,
    pub(super) cancelled: Arc<std::sync::atomic::AtomicBool>,
    pub(super) deadline: Instant,
    pub(super) reply: Reply<Option<TaskStatus>>,
}

pub(super) struct TaskVerificationCommand {
    pub(super) verification: TaskVerification,
    pub(super) cancelled: Arc<std::sync::atomic::AtomicBool>,
    pub(super) deadline: Instant,
    pub(super) reply: Reply<TaskVerificationReceipt>,
}
