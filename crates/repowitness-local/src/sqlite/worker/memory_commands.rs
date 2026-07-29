impl OwnedSqliteIndex {
    /// Appends or verifies one exact semantic memory version and trusted audit receipt.
    #[allow(
        clippy::too_many_arguments,
        reason = "each semantic and audit identity remains explicit at the adapter boundary"
    )]
    pub fn import_memory_version(
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
    ) -> Result<MemoryImportReceipt, SqliteStoreError> {
        let prepared = prepare_memory_import(
            repository,
            record,
            presentation,
            source,
            audit_actor,
            recorded_at,
            approval,
            cancelled.as_ref(),
            deadline,
        )?;
        check_memory_control(cancelled.as_ref(), deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::ImportMemory(Box::new(MemoryImportCommand {
                prepared,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_mutation_reply(&receiver, Some(cancelled.as_ref()), deadline) {
            Ok(receipt) => Ok(receipt),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    pub(crate) fn append_memory_correspondence_review(
        &self,
        prepared: PreparedMemoryCorrespondenceReview,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<MemoryCorrespondenceReviewReceipt, SqliteStoreError> {
        check_memory_control(cancelled.as_ref(), deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::AppendMemoryCorrespondenceReview(Box::new(
                AppendMemoryCorrespondenceReviewCommand {
                    prepared,
                    cancelled: Arc::clone(&cancelled),
                    deadline,
                    reply,
                },
            )),
            deadline,
        )?;
        receive_mutation_reply(&receiver, Some(cancelled.as_ref()), deadline)
    }

    pub(crate) fn load_memory_journal(
        &self,
        repository: RepositoryIdentityDigest,
        limits: MemoryProjectionLoadLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<LoadedMemoryJournal, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::LoadMemoryJournal(Box::new(LoadMemoryJournalCommand {
                repository,
                limits,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        receive_reply(&receiver, deadline)
    }

    pub(crate) fn load_memory_source(
        &self,
        repository: RepositoryIdentityDigest,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<MemoryProjectionSource, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::LoadMemorySource {
                repository,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            },
            deadline,
        )?;
        receive_reply(&receiver, deadline)
    }

    pub(crate) fn load_rust_memory_candidates(
        &self,
        source: MemoryProjectionSource,
        evidence: RustSymbolMemoryEvidence,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<LoadedRustCandidateSet, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::LoadRustMemoryCandidates(Box::new(LoadRustMemoryCandidatesCommand {
                source,
                evidence,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        receive_reply(&receiver, deadline)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the immutable review selector remains explicit at the owner boundary"
    )]
    pub(crate) fn load_memory_correspondence_reviews(
        &self,
        source: MemoryProjectionSource,
        record_id: repowitness_domain::MemoryRecordId,
        revision: repowitness_domain::CanonicalMemoryDigest,
        evidence_ordinal: u8,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<LoadedCorrespondenceReviews, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::LoadMemoryCorrespondenceReviews(Box::new(
                LoadMemoryCorrespondenceReviewsCommand {
                    source,
                    record_id,
                    revision,
                    evidence_ordinal,
                    cancelled: Arc::clone(&cancelled),
                    deadline,
                    reply,
                },
            )),
            deadline,
        )?;
        receive_reply(&receiver, deadline)
    }

    pub(crate) fn publish_memory_projection(
        &self,
        prepared: PreparedMemoryProjection,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<MemoryProjectionPublication, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::PublishMemoryProjection(Box::new(PublishMemoryProjectionCommand {
                prepared,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        receive_mutation_reply(&receiver, Some(cancelled.as_ref()), deadline)
    }
}
