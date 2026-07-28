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
