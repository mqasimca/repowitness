struct PlanRetentionCommand {
    policy: GenerationRetentionPolicy,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<RetentionPlan>,
}

struct ApplyRetentionCommand {
    policy: GenerationRetentionPolicy,
    expected_plan: RetentionPlanDigest,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<RetentionApplyOutcome>,
}

impl OwnedSqliteIndex {
    /// Computes one deterministic bounded retention plan without persistent writes.
    pub fn plan_generation_retention(
        &self,
        request: RetentionPlanRequest,
    ) -> Result<RetentionPlan, SqliteStoreError> {
        let (policy, cancelled, deadline) = request.into_parts();
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::PlanRetention(Box::new(PlanRetentionCommand {
                policy,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(plan) => Ok(plan),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Revalidates and atomically applies one prior retention plan.
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], first look up the
    /// aggregate audit receipt by the exact policy and plan digests. Only when
    /// no committed receipt exists may a caller compute a fresh read-only plan
    /// and compare current roots with the expected plan before retrying apply.
    pub fn apply_generation_retention(
        &self,
        request: RetentionApplyRequest,
    ) -> Result<RetentionApplyOutcome, SqliteStoreError> {
        let (policy, expected_plan, cancelled, deadline) = request.into_parts();
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::ApplyRetention(Box::new(ApplyRetentionCommand {
                policy,
                expected_plan,
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
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
}
