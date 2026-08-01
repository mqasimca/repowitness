impl repowitness_application::RustIndexPublicationPort for OwnedSqliteIndex {
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

impl repowitness_application::SourceSlotPublicationPort for OwnedSqliteIndex {
    type Error = SqliteStoreError;
    type Generation = GenerationId;

    fn stage_source_slot(
        &self,
        request: repowitness_application::StageSourceSlotIndexRequest,
    ) -> Result<Self::Generation, Self::Error> {
        let connected_workspace = request.connected_workspace();
        let source_slot = request.source_slot();
        let reserved_epoch = request.reserved_epoch();
        let identity = request.identity();
        let coverage = request.coverage();
        let cancelled = request.cancelled();
        let deadline = request.deadline();
        let prepared = request.into_prepared();
        Self::stage_source_slot(
            self,
            connected_workspace,
            source_slot,
            reserved_epoch,
            identity,
            prepared,
            coverage,
            cancelled,
            deadline,
        )
    }

    fn complete_source_slot(
        &self,
        connected_workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        reserved_epoch: SourceSlotEpoch,
        generation: Self::Generation,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<(), Self::Error> {
        Self::complete_source_slot_epoch(
            self,
            connected_workspace,
            source_slot,
            reserved_epoch,
            generation,
            cancelled,
            deadline,
        )
    }
}

impl repowitness_application::MemoryVersionImportPort for OwnedSqliteIndex {
    type Error = SqliteStoreError;

    fn import_memory_version(
        &self,
        repository: RepositoryIdentityDigest,
        record: MemoryRecord,
        presentation: MemoryPresentationDigest,
        source: MemoryObservationSource,
        audit_actor: MemoryAuditActorId,
        recorded_at: MemoryRecordedAtUnixMillis,
        approval: MemoryImportApproval,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<MemoryImportReceipt, Self::Error> {
        Self::import_memory_version(
            self,
            repository,
            record,
            presentation,
            source,
            audit_actor,
            recorded_at,
            approval,
            cancelled,
            deadline,
        )
    }
}

impl repowitness_application::TaskStatusPort for OwnedSqliteIndex {
    type Error = SqliteStoreError;

    fn task_status(
        &self,
        repository: RepositoryIdentityDigest,
        task_id: TaskId,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Option<TaskStatus>, Self::Error> {
        Self::task_status(self, repository, task_id, cancelled, deadline)
    }
}

impl repowitness_application::EngineeringTaskPort for OwnedSqliteIndex {

    fn append_checkpoint(
        &self,
        checkpoint: TaskCheckpoint,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<repowitness_application::TaskCheckpointReceipt, Self::Error> {
        let receipt = Self::append_task_checkpoint(self, checkpoint, cancelled, deadline)?;
        Ok(repowitness_application::TaskCheckpointReceipt::new(
            receipt.task_id(),
            receipt.sequence(),
        ))
    }

    fn append_verification(
        &self,
        verification: TaskVerification,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<repowitness_application::TaskVerificationReceipt, Self::Error> {
        let receipt = Self::append_task_verification(self, verification, cancelled, deadline)?;
        repowitness_application::TaskVerificationReceipt::new(receipt.verification_id())
            .ok_or(SqliteStoreError::IntegrityCheckFailed)
    }

}

impl repowitness_application::PersonalMemoryPort for OwnedSqliteIndex {
    type Error = SqliteStoreError;

    fn append_personal_memory(
        &self,
        record: PersonalMemoryRecord,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<repowitness_application::PersonalMemoryReceipt, Self::Error> {
        let receipt = Self::append_personal_memory(self, record, cancelled, deadline)?;
        Ok(repowitness_application::PersonalMemoryReceipt::new(
            receipt.record_id(),
            receipt.revision(),
            receipt.inserted(),
        ))
    }
}
