impl OwnedSqliteReader {
    /// Loads only exact, complete, integrity-checked artifacts requested by preparation.
    #[cfg(test)]
    pub(crate) fn load_reusable_artifacts(
        &self,
        requested: &[AnalysisArtifactDigest],
        identity: RustArtifactIdentity,
        limits: RustIndexLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>, SqliteStoreError> {
        self.load_reusable_artifacts_for_language(
            requested,
            SourceLanguage::Rust,
            identity,
            limits,
            cancelled,
            deadline,
        )
    }

    pub(crate) fn load_reusable_artifacts_for_language(
        &self,
        requested: &[AnalysisArtifactDigest],
        language: SourceLanguage,
        identity: RustArtifactIdentity,
        limits: RustIndexLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>, SqliteStoreError> {
        validate_artifact_request(requested, limits)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::LoadArtifacts(Box::new(ArtifactCommand {
                requested: requested.to_vec().into_boxed_slice(),
                language,
                identity,
                limits,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(results) => Ok(results),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
}

fn validate_artifact_request(
    requested: &[AnalysisArtifactDigest],
    limits: RustIndexLimits,
) -> Result<(), SqliteStoreError> {
    let requested_count =
        u64::try_from(requested.len()).map_err(|_| SqliteStoreError::CountNotRepresentable)?;
    if requested_count > limits.max_files()
        || requested.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    Ok(())
}
