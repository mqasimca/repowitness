impl OwnedSqliteIndex {
    /// Stages the required raw all-language syntax-site projection for a ready generation.
    pub fn stage_raw_syntax_sites(
        &self,
        generation: GenerationId,
        prepared: PreparedRawSyntaxGeneration,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<(), SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::StageSyntaxSites(Box::new(StageSyntaxSitesCommand {
                generation,
                prepared,
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
            Ok(()) => Ok(()),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
}
