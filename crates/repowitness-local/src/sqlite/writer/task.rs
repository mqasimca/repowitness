/// Durable receipt for one committed task checkpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskCheckpointReceipt {
    task_id: TaskId,
    sequence: u32,
}

impl TaskCheckpointReceipt {
    /// Returns the durable task identity.
    #[must_use]
    pub const fn task_id(self) -> TaskId {
        self.task_id
    }

    /// Returns the committed checkpoint sequence.
    #[must_use]
    pub const fn sequence(self) -> u32 {
        self.sequence
    }
}

/// Durable receipt for one committed verification event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskVerificationReceipt {
    verification_id: i64,
}

impl TaskVerificationReceipt {
    /// Returns the positive SQLite-local immutable verification identity.
    #[must_use]
    pub const fn verification_id(self) -> i64 {
        self.verification_id
    }
}

impl WriterState {
    pub(super) fn task_status(
        &mut self,
        repository: RepositoryIdentityDigest,
        task_id: TaskId,
        control: WriteControl<'_>,
    ) -> Result<Option<TaskStatus>, SqliteStoreError> {
        check_control(control)?;
        let row = self
            .connection
            .query_row(
                "SELECT task.repository_identity, checkpoint.state, checkpoint.sequence,
                        (SELECT COUNT(*) FROM engineering_task_verifications AS verification
                         WHERE verification.task_id = task.task_id)
                   FROM engineering_tasks AS task
                   JOIN engineering_task_checkpoints AS checkpoint
                     ON checkpoint.task_id = task.task_id
                  WHERE task.task_id = ?1 AND task.repository_identity = ?2
                  ORDER BY checkpoint.sequence DESC
                  LIMIT 1",
                params![task_id.as_bytes().as_slice(), repository.as_bytes().as_slice()],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        check_control(control)?;
        let Some((stored_repository, state, sequence, verification_count)) = row else {
            return Ok(None);
        };
        if stored_repository.as_slice() != repository.as_bytes().as_slice() {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        let state = task_state_from_text(&state).ok_or(SqliteStoreError::IntegrityCheckFailed)?;
        let sequence = u32::try_from(sequence).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let verification_count =
            u32::try_from(verification_count).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        Ok(Some(TaskStatus::new(
            task_id,
            repository,
            state,
            sequence,
            verification_count,
        )))
    }

    pub(super) fn append_task_checkpoint(
        &mut self,
        checkpoint: &TaskCheckpoint,
        control: WriteControl<'_>,
    ) -> Result<TaskCheckpointReceipt, SqliteStoreError> {
        check_control(control)?;
        if task_checkpoint_contains_sensitive_text(checkpoint) {
            return Err(SqliteStoreError::InvalidTask);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let task_id = checkpoint.task_id();
        let repository = checkpoint.repository();
        let existing_repository = transaction
            .query_row(
                "SELECT repository_identity FROM engineering_tasks WHERE task_id = ?1",
                params![task_id.as_bytes().as_slice()],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        match existing_repository {
            None if checkpoint.sequence() == 1 => {
                transaction
                    .execute(
                        "INSERT INTO engineering_tasks(task_id, repository_identity, created_at_unix_ms)
                         VALUES (?1, ?2, ?3)",
                        params![
                            task_id.as_bytes().as_slice(),
                            repository.as_bytes().as_slice(),
                            i64::try_from(checkpoint.recorded_at_unix_ms())
                                .map_err(|_| SqliteStoreError::InvalidTask)?,
                        ],
                    )
                    .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            }
            Some(existing) if existing.as_slice() == repository.as_bytes().as_slice() => {
                let expected = transaction
                    .query_row(
                        "SELECT COALESCE(MAX(sequence), 0) + 1
                         FROM engineering_task_checkpoints WHERE task_id = ?1",
                        params![task_id.as_bytes().as_slice()],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
                if expected != i64::from(checkpoint.sequence()) {
                    return Err(SqliteStoreError::InvalidTask);
                }
            }
            _ => return Err(SqliteStoreError::InvalidTask),
        }
        check_control(control)?;
        transaction
            .execute(
                "INSERT INTO engineering_task_checkpoints(
                    task_id, sequence, state, objective, hypothesis, next_safe_action,
                    recorded_at_unix_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    task_id.as_bytes().as_slice(),
                    i64::from(checkpoint.sequence()),
                    task_state_text(checkpoint.state()),
                    checkpoint.objective().as_str(),
                    checkpoint.hypothesis().map(TaskText::as_str),
                    checkpoint.next_safe_action().map(TaskText::as_str),
                    i64::try_from(checkpoint.recorded_at_unix_ms())
                        .map_err(|_| SqliteStoreError::InvalidTask)?,
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        check_control(control)?;
        commit_mutation(transaction)?;
        Ok(TaskCheckpointReceipt {
            task_id,
            sequence: checkpoint.sequence(),
        })
    }

    pub(super) fn append_task_verification(
        &mut self,
        verification: &TaskVerification,
        control: WriteControl<'_>,
    ) -> Result<TaskVerificationReceipt, SqliteStoreError> {
        check_control(control)?;
        if task_verification_contains_sensitive_text(verification) {
            return Err(SqliteStoreError::InvalidTask);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let task_id = verification.task_id();
        let checkpoint_exists = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM engineering_task_checkpoints
                    WHERE task_id = ?1 AND sequence = ?2
                 )",
                params![
                    task_id.as_bytes().as_slice(),
                    i64::from(verification.checkpoint_sequence()),
                ],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if checkpoint_exists != 1 {
            return Err(SqliteStoreError::InvalidTask);
        }
        check_control(control)?;
        transaction
            .execute(
                "INSERT INTO engineering_task_verifications(
                    task_id, checkpoint_sequence, source_snapshot_digest, check_identity,
                    producer, configuration_digest, outcome, captured_output_digest,
                    captured_output_bytes, recorded_at_unix_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    task_id.as_bytes().as_slice(),
                    i64::from(verification.checkpoint_sequence()),
                    verification.source_snapshot().as_bytes().as_slice(),
                    verification.check().as_str(),
                    verification.producer().as_str(),
                    verification.configuration().as_bytes().as_slice(),
                    task_verification_outcome_text(verification.outcome()),
                    verification.captured_output_digest().as_slice(),
                    i64::try_from(verification.captured_output_bytes())
                        .map_err(|_| SqliteStoreError::InvalidTask)?,
                    i64::try_from(verification.recorded_at_unix_ms())
                        .map_err(|_| SqliteStoreError::InvalidTask)?,
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let verification_id = transaction.last_insert_rowid();
        if verification_id <= 0 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        check_control(control)?;
        commit_mutation(transaction)?;
        Ok(TaskVerificationReceipt { verification_id })
    }
}

fn task_checkpoint_contains_sensitive_text(checkpoint: &TaskCheckpoint) -> bool {
    crate::memory_management::secret::contains_sensitive_text(checkpoint.objective().as_str())
        || checkpoint
            .hypothesis()
            .is_some_and(|text| crate::memory_management::secret::contains_sensitive_text(text.as_str()))
        || checkpoint
            .next_safe_action()
            .is_some_and(|text| crate::memory_management::secret::contains_sensitive_text(text.as_str()))
}

fn task_verification_contains_sensitive_text(verification: &TaskVerification) -> bool {
    crate::memory_management::secret::contains_sensitive_text(verification.check().as_str())
        || crate::memory_management::secret::contains_sensitive_text(verification.producer().as_str())
}

const fn task_state_text(state: TaskState) -> &'static str {
    match state {
        TaskState::Open => "open",
        TaskState::Blocked => "blocked",
        TaskState::Completed => "completed",
        TaskState::Cancelled => "cancelled",
    }
}

fn task_state_from_text(state: &str) -> Option<TaskState> {
    match state {
        "open" => Some(TaskState::Open),
        "blocked" => Some(TaskState::Blocked),
        "completed" => Some(TaskState::Completed),
        "cancelled" => Some(TaskState::Cancelled),
        _ => None,
    }
}

const fn task_verification_outcome_text(outcome: TaskVerificationOutcome) -> &'static str {
    match outcome {
        TaskVerificationOutcome::Passed => "passed",
        TaskVerificationOutcome::Failed => "failed",
        TaskVerificationOutcome::Cancelled => "cancelled",
        TaskVerificationOutcome::Incomplete => "incomplete",
    }
}
