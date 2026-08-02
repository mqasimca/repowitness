impl OwnedSqliteIndex {
    /// Stages the required path-only repository topology inventory for a ready generation.
    pub fn stage_repository_topology(
        &self,
        generation: GenerationId,
        prepared: PreparedRepositoryTopology,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<(), SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::StageRepositoryTopology(Box::new(StageRepositoryTopologyCommand {
                generation,
                prepared,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_mutation_reply(&receiver, Some(cancelled.as_ref()), deadline, Some(&self.unresolved_mutation)) {
            Ok(()) => Ok(()),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
}
