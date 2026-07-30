const MAX_OBSERVED_MEMORY_HISTORY_ITEMS: usize = 65_536;

pub(crate) struct ObservedMemoryHistoryItem {
    record: MemoryRecord,
    presentation: MemoryPresentationDigest,
    commit: MemoryCommitId,
}

impl ObservedMemoryHistoryItem {
    pub(crate) const fn new(
        record: MemoryRecord,
        presentation: MemoryPresentationDigest,
        commit: MemoryCommitId,
    ) -> Self {
        Self {
            record,
            presentation,
            commit,
        }
    }
}

impl OwnedSqliteIndex {
    /// Appends or verifies one exact semantic memory version and trusted audit receipt.
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], reload the immutable
    /// journal and compare the exact revision, observation, and approval before
    /// retrying.
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
        match receive_mutation_reply(
            &receiver,
            Some(cancelled.as_ref()),
            deadline,
            Some(&self.unresolved_mutation),
        ) {
            Ok(receipt) => Ok(receipt),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Atomically imports one bounded set of observation-only Git history.
    ///
    /// Workspace creation and every immutable version and observation share
    /// one transaction. On [`SqliteStoreError::MutationOutcomeUnknown`], reload
    /// the journal and compare every intended exact Git observation.
    pub(crate) fn import_observed_memory_history(
        &self,
        repository: RepositoryIdentityDigest,
        items: Vec<ObservedMemoryHistoryItem>,
        audit_actor: MemoryAuditActorId,
        recorded_at: MemoryRecordedAtUnixMillis,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Box<[MemoryImportReceipt]>, SqliteStoreError> {
        if items.len() > MAX_OBSERVED_MEMORY_HISTORY_ITEMS {
            return Err(SqliteStoreError::InvalidMemoryImport);
        }
        let mut prepared = Vec::with_capacity(items.len());
        for item in items {
            prepared.push(prepare_memory_import(
                repository,
                item.record,
                item.presentation,
                MemoryObservationSource::Git(item.commit),
                audit_actor.clone(),
                recorded_at,
                MemoryImportApproval::ObservedOnly,
                cancelled.as_ref(),
                deadline,
            )?);
        }
        check_memory_control(cancelled.as_ref(), deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::ImportObservedMemoryHistory(Box::new(ObservedMemoryHistoryCommand {
                repository,
                prepared: prepared.into_boxed_slice(),
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
            Ok(receipts) => Ok(receipts),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Appends one exact immutable correspondence-review event.
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], reload the exact
    /// revision and evidence-ordinal review history before retrying.
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
        receive_mutation_reply(
            &receiver,
            Some(cancelled.as_ref()),
            deadline,
            Some(&self.unresolved_mutation),
        )
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

    /// Publishes one immutable current-memory projection.
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], read the active pinned
    /// memory projection before retrying.
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
        receive_mutation_reply(
            &receiver,
            Some(cancelled.as_ref()),
            deadline,
            Some(&self.unresolved_mutation),
        )
    }
}
