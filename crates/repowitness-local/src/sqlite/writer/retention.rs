const RETENTION_PROGRESS_INSTRUCTIONS: i32 = 1_000;

impl WriterState {
    pub(super) fn plan_generation_retention(
        &mut self,
        policy: &GenerationRetentionPolicy,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RetentionPlan, SqliteStoreError> {
        check_retention_control(cancelled.as_ref(), deadline)?;
        let progress_cancelled = Arc::clone(&cancelled);
        self.connection
            .progress_handler(
                RETENTION_PROGRESS_INSTRUCTIONS,
                Some(move || {
                    progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline
                }),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let result =
            self.plan_generation_retention_with_control(policy, cancelled.as_ref(), deadline);
        let clear = self
            .connection
            .progress_handler(0, None::<fn() -> bool>)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed);
        match (result, clear) {
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            (Ok(plan), Ok(())) => Ok(plan),
        }
    }

    fn plan_generation_retention_with_control(
        &mut self,
        policy: &GenerationRetentionPolicy,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<RetentionPlan, SqliteStoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|error| retention_database_error(error, cancelled, deadline))?;
        let plan = build_retention_plan(&transaction, policy, cancelled, deadline)?;
        check_retention_control(cancelled, deadline)?;
        transaction
            .commit()
            .map_err(|error| retention_database_error(error, cancelled, deadline))?;
        Ok(plan)
    }

    pub(super) fn apply_generation_retention(
        &mut self,
        policy: &GenerationRetentionPolicy,
        expected_plan: RetentionPlanDigest,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RetentionApplyOutcome, SqliteStoreError> {
        check_retention_control(cancelled.as_ref(), deadline)?;
        let progress_cancelled = Arc::clone(&cancelled);
        self.connection
            .progress_handler(
                RETENTION_PROGRESS_INSTRUCTIONS,
                Some(move || {
                    progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline
                }),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let result = self.apply_generation_retention_with_control(
            policy,
            expected_plan,
            cancelled.as_ref(),
            deadline,
        );
        let clear = self
            .connection
            .progress_handler(0, None::<fn() -> bool>)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed);
        match (result, clear) {
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            (Ok(outcome), Ok(())) => Ok(outcome),
        }
    }

    fn apply_generation_retention_with_control(
        &mut self,
        policy: &GenerationRetentionPolicy,
        expected_plan: RetentionPlanDigest,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<RetentionApplyOutcome, SqliteStoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| retention_database_error(error, cancelled, deadline))?;
        if let Some(outcome) = load_applied_retention(
            &transaction,
            policy.digest(),
            expected_plan,
            cancelled,
            deadline,
        )? {
            check_retention_control(cancelled, deadline)?;
            transaction
                .commit()
                .map_err(|error| retention_database_error(error, cancelled, deadline))?;
            return Ok(outcome);
        }
        let (plan, mut budget) =
            build_retention_plan_with_budget(&transaction, policy, cancelled, deadline)?;
        if plan.plan_digest() != expected_plan {
            return Err(SqliteStoreError::RetentionPlanStale);
        }
        budget.release_reservations();
        let outcome = sweep_retention_plan(
            &transaction,
            policy,
            &plan,
            &mut budget,
            cancelled,
            deadline,
        )?;
        check_retention_control(cancelled, deadline)?;
        commit_mutation(transaction)?;
        Ok(outcome)
    }
}

pub(crate) fn check_retention_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SqliteStoreError> {
    if cancelled.load(Ordering::Acquire) {
        Err(SqliteStoreError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(SqliteStoreError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

pub(crate) fn retention_database_error(
    error: rusqlite::Error,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> SqliteStoreError {
    if matches!(
        error,
        rusqlite::Error::SqliteFailure(code, _) if code.code == ErrorCode::OperationInterrupted
    ) {
        if cancelled.load(Ordering::Acquire) {
            SqliteStoreError::Cancelled
        } else if Instant::now() >= deadline {
            SqliteStoreError::DeadlineExceeded
        } else {
            SqliteStoreError::DatabaseOperationFailed
        }
    } else {
        SqliteStoreError::DatabaseOperationFailed
    }
}

include!("retention/budget.rs");
include!("retention/root_relations.rs");
include!("retention/plan.rs");
include!("retention/sweep.rs");
