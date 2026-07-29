impl OwnedSqliteIndex {
    /// Stages and validates the required Rust graph for one ready generation.
    ///
    /// A failed, cancelled, or expired operation makes the candidate
    /// generation ineligible for activation and leaves the prior active
    /// generation readable.
    pub fn stage_rust_graph(
        &self,
        generation: GenerationId,
        prepared: PreparedRustGraphGeneration,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<(), SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::StageGraph(Box::new(StageGraphCommand {
                generation,
                prepared,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_mutation_reply(&receiver, Some(cancelled.as_ref()), deadline) {
            Ok(()) => Ok(()),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
}
