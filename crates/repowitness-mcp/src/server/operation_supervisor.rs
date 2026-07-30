const MCP_BLOCKING_TASK_JOIN_GRACE: Duration = Duration::from_millis(250);
const MCP_MUTATION_RECEIPT_RESOLUTION_GRACE: Duration = Duration::from_millis(250);
// Keep the supervisor alive strictly beyond the worker's absolute deadline
// and receipt grace so a receipt scheduled at that boundary wins the race.
const MCP_MUTATION_SUPERVISOR_SETTLEMENT_MARGIN: Duration = Duration::from_millis(25);

impl RepoWitnessMcpServer {
    async fn run_blocking<T, F>(
        &self,
        timeout: Duration,
        context: RequestContext<RoleServer>,
        operation: F,
    ) -> Result<Result<T, RepositoryServiceError>, McpError>
    where
        T: Send + 'static,
        F: FnOnce(Duration, Arc<AtomicBool>) -> Result<T, RepositoryServiceError> + Send + 'static,
    {
        let deadline = Instant::now() + timeout;
        let permit = acquire_permit(
            Arc::clone(&self.operations),
            deadline,
            context.ct.cancelled(),
        )
        .await?;
        deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(deadline_error)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let task_cancelled = Arc::clone(&cancelled);
        let mut task = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let remaining = deadline.saturating_duration_since(Instant::now());
            operation(remaining, task_cancelled)
        });

        tokio::select! {
            result = &mut task => join_result(result),
            () = context.ct.cancelled() => {
                cancelled.store(true, Ordering::Release);
                await_cancelled_task(task).await;
                Err(cancelled_error())
            }
            () = tokio::time::sleep_until(deadline) => {
                cancelled.store(true, Ordering::Release);
                await_cancelled_task(task).await;
                Err(deadline_error())
            }
        }
    }

    async fn run_memory_mutation_blocking<T, F>(
        &self,
        timeout: Duration,
        context: RequestContext<RoleServer>,
        request_scope: MemoryMutationRequestScope,
        mutation: F,
    ) -> Result<Result<T, RepositoryServiceError>, McpError>
    where
        T: Send + 'static,
        F: FnOnce(Duration, Arc<AtomicBool>) -> Result<T, RepositoryServiceError> + Send + 'static,
    {
        let deadline = Instant::now() + timeout;
        let permit = acquire_permit(
            Arc::clone(&self.operations),
            deadline,
            context.ct.cancelled(),
        )
        .await?;
        deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(deadline_error)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let task_cancelled = Arc::clone(&cancelled);
        let mut task = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let remaining = deadline.saturating_duration_since(Instant::now());
            mutation(remaining, task_cancelled)
        });

        let deadline_resolution = mutation_resolution_deadline(deadline);
        tokio::select! {
            result = &mut task => join_mutation_result(result, request_scope),
            () = context.ct.cancelled() => {
                cancelled.store(true, Ordering::Release);
                // Client cancellation starts one fresh, fixed receipt-resolution
                // interval; it must not inherit a potentially much later request deadline.
                await_mutation_outcome(
                    task,
                    request_scope,
                    mutation_resolution_deadline_from_now(),
                ).await
            }
            () = tokio::time::sleep_until(deadline) => {
                cancelled.store(true, Ordering::Release);
                await_mutation_outcome(task, request_scope, deadline_resolution).await
            }
        }
    }
}

fn mutation_resolution_deadline(deadline: Instant) -> Instant {
    deadline
        + MCP_MUTATION_RECEIPT_RESOLUTION_GRACE
        + MCP_MUTATION_SUPERVISOR_SETTLEMENT_MARGIN
}

fn mutation_resolution_deadline_from_now() -> Instant {
    mutation_resolution_deadline(Instant::now())
}

fn join_result<T>(
    result: Result<Result<T, RepositoryServiceError>, tokio::task::JoinError>,
) -> Result<Result<T, RepositoryServiceError>, McpError> {
    result.map_err(|_| McpError::internal_error("repository operation task failed", None))
}

fn join_mutation_result<T>(
    result: Result<Result<T, RepositoryServiceError>, tokio::task::JoinError>,
    request_scope: MemoryMutationRequestScope,
) -> Result<Result<T, RepositoryServiceError>, McpError> {
    Ok(result.unwrap_or_else(|_| {
        Err(RepositoryServiceError::memory_mutation_phase_unknown(
            request_scope,
        ))
    }))
}

async fn await_cancelled_task<T>(mut task: JoinHandle<Result<T, RepositoryServiceError>>) {
    let deadline = Instant::now() + MCP_BLOCKING_TASK_JOIN_GRACE;
    tokio::select! {
        _ = &mut task => {}
        () = tokio::time::sleep_until(deadline) => {}
    }
}

async fn await_mutation_outcome<T>(
    mut task: JoinHandle<Result<T, RepositoryServiceError>>,
    request_scope: MemoryMutationRequestScope,
    resolution_deadline: Instant,
) -> Result<Result<T, RepositoryServiceError>, McpError> {
    tokio::select! {
        biased;
        result = &mut task => join_mutation_result(result, request_scope),
        () = tokio::time::sleep_until(resolution_deadline) => {
            if task.is_finished() {
                join_mutation_result(task.await, request_scope)
            } else {
                Ok(Err(RepositoryServiceError::memory_mutation_phase_unknown(
                    request_scope,
                )))
            }
        }
    }
}
