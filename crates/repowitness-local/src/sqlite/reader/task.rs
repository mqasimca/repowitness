fn execute_task_status_command(connection: &mut Connection, command: TaskStatusCommand) {
    let TaskStatusCommand {
        repository,
        task_id,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = read_task_status(connection, repository, task_id, &cancelled, deadline);
    let _ = reply.try_send(result);
}

fn execute_task_statuses_command(connection: &mut Connection, command: TaskStatusesCommand) {
    let TaskStatusesCommand {
        repository,
        limit,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = read_task_statuses(connection, repository, limit, &cancelled, deadline);
    let _ = reply.try_send(result);
}

fn read_task_status(
    connection: &Connection,
    repository: RepositoryIdentityDigest,
    task_id: TaskId,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Option<TaskStatus>, SqliteStoreError> {
    check_control(cancelled, deadline)?;
    let task_table_present = connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'engineering_tasks'",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?
        .is_some();
    if !task_table_present {
        return Ok(None);
    }
    let row = connection
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
    check_control(cancelled, deadline)?;
    let Some((stored_repository, state, sequence, verification_count)) = row else {
        return Ok(None);
    };
    if stored_repository.as_slice() != repository.as_bytes().as_slice() {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    let state = match state.as_str() {
        "open" => TaskState::Open,
        "blocked" => TaskState::Blocked,
        "completed" => TaskState::Completed,
        "cancelled" => TaskState::Cancelled,
        _ => return Err(SqliteStoreError::IntegrityCheckFailed),
    };
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

fn read_task_statuses(
    connection: &Connection,
    repository: RepositoryIdentityDigest,
    limit: u16,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<TaskStatus>, SqliteStoreError> {
    check_control(cancelled, deadline)?;
    let task_table_present = connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'engineering_tasks'",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?
        .is_some();
    if !task_table_present {
        return Ok(Vec::new());
    }
    let mut statement = connection
        .prepare(
            "SELECT task.task_id, checkpoint.state, checkpoint.sequence,
                    (SELECT COUNT(*) FROM engineering_task_verifications AS verification
                     WHERE verification.task_id = task.task_id)
               FROM engineering_tasks AS task
               JOIN engineering_task_checkpoints AS checkpoint
                 ON checkpoint.task_id = task.task_id
              WHERE task.repository_identity = ?1
                AND checkpoint.sequence = (
                    SELECT MAX(current_checkpoint.sequence)
                      FROM engineering_task_checkpoints AS current_checkpoint
                     WHERE current_checkpoint.task_id = task.task_id
                )
              ORDER BY task.created_at_unix_ms DESC, task.task_id ASC
              LIMIT ?2",
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let mut rows = statement
        .query(params![
            repository.as_bytes().as_slice(),
            i64::from(limit),
        ])
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let mut statuses = Vec::with_capacity(usize::from(limit));
    while let Some(row) = rows
        .next()
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?
    {
        check_control(cancelled, deadline)?;
        let task_id = row
            .get::<_, Vec<u8>>(0)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let task_id: [u8; 16] = task_id
            .try_into()
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let state = row
            .get::<_, String>(1)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let sequence = row
            .get::<_, i64>(2)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let verification_count = row
            .get::<_, i64>(3)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let state = match state.as_str() {
            "open" => TaskState::Open,
            "blocked" => TaskState::Blocked,
            "completed" => TaskState::Completed,
            "cancelled" => TaskState::Cancelled,
            _ => return Err(SqliteStoreError::IntegrityCheckFailed),
        };
        statuses.push(TaskStatus::new(
            TaskId::new(task_id),
            repository,
            state,
            u32::try_from(sequence).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
            u32::try_from(verification_count)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        ));
    }
    check_control(cancelled, deadline)?;
    Ok(statuses)
}
