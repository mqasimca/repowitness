impl WriterState {
    pub(super) const fn new(connection: Connection) -> Self {
        Self { connection }
    }

    pub(super) fn load_memory_source(
        &mut self,
        repository: RepositoryIdentityDigest,
        control: WriteControl<'_>,
    ) -> Result<MemoryProjectionSource, SqliteStoreError> {
        load_memory_source(&mut self.connection, repository, control)
    }

    pub(super) fn load_memory_journal(
        &mut self,
        repository: RepositoryIdentityDigest,
        limits: MemoryProjectionLoadLimits,
        control: WriteControl<'_>,
    ) -> Result<LoadedMemoryJournal, SqliteStoreError> {
        load_memory_journal(&mut self.connection, repository, limits, control)
    }

    pub(super) fn load_rust_memory_candidates(
        &mut self,
        source: MemoryProjectionSource,
        evidence: &repowitness_domain::RustSymbolMemoryEvidence,
        control: WriteControl<'_>,
    ) -> Result<LoadedRustCandidateSet, SqliteStoreError> {
        load_rust_candidates(&mut self.connection, source, evidence, control)
    }

    pub(super) fn append_memory_correspondence_review(
        &mut self,
        prepared: &PreparedMemoryCorrespondenceReview,
        control: WriteControl<'_>,
    ) -> Result<MemoryCorrespondenceReviewReceipt, SqliteStoreError> {
        append_memory_correspondence_review(&mut self.connection, prepared, control)
    }

    pub(super) fn load_memory_correspondence_reviews(
        &mut self,
        source: MemoryProjectionSource,
        record_id: repowitness_domain::MemoryRecordId,
        revision: CanonicalMemoryDigest,
        evidence_ordinal: u8,
        control: WriteControl<'_>,
    ) -> Result<LoadedCorrespondenceReviews, SqliteStoreError> {
        load_memory_correspondence_reviews(
            &mut self.connection,
            source,
            record_id,
            revision,
            evidence_ordinal,
            control,
        )
    }

    pub(super) fn publish_memory_projection(
        &mut self,
        prepared: &PreparedMemoryProjection,
        control: WriteControl<'_>,
    ) -> Result<MemoryProjectionPublication, SqliteStoreError> {
        publish_memory_projection(&mut self.connection, prepared, control)
    }

    pub(super) fn recover(
        &mut self,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<u64, SqliteStoreError> {
        check_recovery_control(cancelled.as_ref(), deadline)?;
        let progress_cancelled = Arc::clone(&cancelled);
        self.connection
            .progress_handler(
                RECOVERY_PROGRESS_INSTRUCTIONS,
                Some(move || {
                    progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline
                }),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let result = self.recover_with_control(cancelled.as_ref(), deadline);
        let clear_result = self
            .connection
            .progress_handler(0, None::<fn() -> bool>)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed);
        match (result, clear_result) {
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            (Ok(recovered), Ok(())) => Ok(recovered),
        }
    }

    fn recover_with_control(
        &mut self,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<u64, SqliteStoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
        let query_limit = i64::try_from(MAX_STARTUP_RECOVERY_GENERATIONS + 1)
            .map_err(|_| SqliteStoreError::CountNotRepresentable)?;
        let incomplete: Vec<i64> = {
            let mut statement = transaction
                .prepare(
                    "SELECT generation_id FROM index_generations
                     WHERE lifecycle_state IN (
                        'discovered', 'extracting', 'resolving', 'validating', 'ready'
                     )
                     ORDER BY generation_id
                     LIMIT ?1",
                )
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
            let rows = statement
                .query_map([query_limit], |row| row.get(0))
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
            rows.collect::<Result<_, _>>()
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?
        };
        if incomplete.len() > MAX_STARTUP_RECOVERY_GENERATIONS {
            return Err(SqliteStoreError::RecoveryGenerationLimitExceeded);
        }
        for generation_id in &incomplete {
            check_recovery_control(cancelled, deadline)?;
            transaction
                .execute(
                    "DELETE FROM generation_search WHERE generation_id = ?1",
                    [generation_id],
                )
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
            transaction
                .execute(
                    "DELETE FROM generation_search_rebuild WHERE generation_id = ?1",
                    [generation_id],
                )
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
            transaction
                .execute(
                    "DELETE FROM generation_facts WHERE generation_id = ?1",
                    [generation_id],
                )
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
            transaction
                .execute(
                    "DELETE FROM generation_files WHERE generation_id = ?1",
                    [generation_id],
                )
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
            transaction
                .execute(
                    "UPDATE index_generations SET lifecycle_state = 'failed'
                     WHERE generation_id = ?1",
                    [generation_id],
                )
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
        }
        check_recovery_control(cancelled, deadline)?;
        delete_staging_content(&transaction).map_err(|error| {
            if cancelled.load(Ordering::Acquire) {
                SqliteStoreError::Cancelled
            } else if Instant::now() >= deadline {
                SqliteStoreError::DeadlineExceeded
            } else {
                error
            }
        })?;
        check_recovery_control(cancelled, deadline)?;
        transaction
            .commit()
            .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
        u64::try_from(incomplete.len()).map_err(|_| SqliteStoreError::CountNotRepresentable)
    }

    pub(super) fn register_workspace(
        &mut self,
        repository: RepositoryIdentityDigest,
        initial_source_epoch: u64,
    ) -> Result<i64, SqliteStoreError> {
        let epoch = fixed_integer(initial_source_epoch)?;
        let transaction = self.transaction()?;
        transaction
            .execute(
                "INSERT INTO workspaces(repository_identity, source_epoch)
                 VALUES (?1, ?2)
                 ON CONFLICT(repository_identity) DO NOTHING",
                params![repository.as_bytes().as_slice(), epoch],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let (workspace_id, stored_epoch): (i64, i64) = transaction
            .query_row(
                "SELECT workspace_id, source_epoch FROM workspaces
                 WHERE repository_identity = ?1",
                [repository.as_bytes().as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if stored_epoch != epoch {
            return Err(SqliteStoreError::StaleSourceEpoch);
        }
        transaction
            .commit()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        Ok(workspace_id)
    }

    pub(super) fn import_memory_version(
        &mut self,
        prepared: &PreparedMemoryImport,
        control: WriteControl<'_>,
    ) -> Result<MemoryImportReceipt, SqliteStoreError> {
        check_control(control)?;
        if prepared.record.scope().repository() != prepared.repository {
            return Err(SqliteStoreError::InvalidMemoryImport);
        }

        let transaction = self.transaction()?;
        let workspace_id = transaction
            .query_row(
                "SELECT workspace_id FROM workspaces WHERE repository_identity = ?1",
                [prepared.repository.as_bytes().as_slice()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?
            .ok_or(SqliteStoreError::WorkspaceUnavailable)?;
        let record_id = prepared.record.header().record_id();
        let persisted_canonical = transaction
            .query_row(
                "SELECT canonical_json FROM memory_versions
                 WHERE workspace_id = ?1 AND record_id = ?2 AND revision_digest = ?3",
                params![
                    workspace_id,
                    record_id.as_bytes().as_slice(),
                    prepared.revision.as_bytes().as_slice()
                ],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let version_inserted = if let Some(persisted_canonical) = persisted_canonical {
            if persisted_canonical != prepared.canonical_json {
                return Err(SqliteStoreError::IntegrityCheckFailed);
            }
            false
        } else {
            insert_memory_children(&transaction, workspace_id, prepared, control)?;
            insert_memory_version(&transaction, workspace_id, prepared)?;
            true
        };
        verify_memory_version(&transaction, workspace_id, prepared, control)?;
        check_control(control)?;
        let observation_inserted =
            insert_memory_audit(&transaction, workspace_id, prepared, "observed")?;
        check_control(control)?;
        let approval_inserted = match prepared.approval {
            repowitness_application::MemoryImportApproval::ObservedOnly => false,
            repowitness_application::MemoryImportApproval::LocallyApproved => {
                insert_memory_audit(&transaction, workspace_id, prepared, "locally_approved")?
            }
        };
        check_control(control)?;
        transaction
            .commit()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        Ok(MemoryImportReceipt::new(
            prepared.revision,
            version_inserted,
            observation_inserted,
            approval_inserted,
        ))
    }

    pub(super) fn advance_source_epoch(
        &mut self,
        repository: RepositoryIdentityDigest,
        expected: u64,
        next: u64,
    ) -> Result<(), SqliteStoreError> {
        if next <= expected {
            return Err(SqliteStoreError::InvalidSourceEpoch);
        }
        let expected = fixed_integer(expected)?;
        let next = fixed_integer(next)?;
        let changed = self
            .connection
            .execute(
                "UPDATE workspaces SET source_epoch = ?1
                 WHERE repository_identity = ?2 AND source_epoch = ?3",
                params![next, repository.as_bytes().as_slice(), expected],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::StaleSourceEpoch);
        }
        Ok(())
    }

    pub(super) fn stage(
        &mut self,
        source_epoch: u64,
        identity: RustSourceSnapshotIdentity,
        prepared: &PreparedRustIndex,
        coverage: RustIndexCoverage,
        control: WriteControl<'_>,
    ) -> Result<GenerationId, SqliteStoreError> {
        check_control(control)?;
        let workspace_id = self.workspace(identity.repository(), source_epoch)?;
        validate_prepared_identity(prepared)?;
        let snapshot_digest = hash_source_snapshot(identity, prepared.manifest_digest());
        self.ensure_snapshot(snapshot_digest, identity, prepared, control)?;
        for file in prepared.files() {
            check_control(control)?;
            self.ensure_artifact(file, control)?;
        }
        let generation = self.create_generation(workspace_id, source_epoch, snapshot_digest)?;
        let result = self.stage_generation_rows(generation, prepared, coverage, control);
        if let Err(error) = result {
            let target = if error == SqliteStoreError::Cancelled {
                "cancelled"
            } else {
                "failed"
            };
            let _ = self.fail_generation(generation, target);
            return Err(error);
        }
        Ok(generation)
    }

    pub(super) fn activate(
        &mut self,
        generation: GenerationId,
        expected_source_epoch: u64,
    ) -> Result<(), SqliteStoreError> {
        let expected_epoch = fixed_integer(expected_source_epoch)?;
        let transaction = self.transaction()?;
        let (workspace_id, generation_epoch, state): (i64, i64, String) = transaction
            .query_row(
                "SELECT workspace_id, source_epoch, lifecycle_state
                 FROM index_generations WHERE generation_id = ?1",
                [generation.get()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|_| SqliteStoreError::GenerationUnavailable)?;
        let workspace_epoch: i64 = transaction
            .query_row(
                "SELECT source_epoch FROM workspaces WHERE workspace_id = ?1",
                [workspace_id],
                |row| row.get(0),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if state != "ready"
            || generation_epoch != expected_epoch
            || workspace_epoch != expected_epoch
        {
            return Err(SqliteStoreError::StaleSourceEpoch);
        }
        transaction
            .execute(
                "UPDATE index_generations SET lifecycle_state = 'retained'
                 WHERE workspace_id = ?1 AND lifecycle_state = 'active'",
                [workspace_id],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let changed = transaction
            .execute(
                "UPDATE index_generations SET lifecycle_state = 'active'
                 WHERE generation_id = ?1 AND lifecycle_state = 'ready'",
                [generation.get()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::GenerationUnavailable);
        }
        transaction
            .execute(
                "UPDATE workspaces SET active_generation_id = ?1 WHERE workspace_id = ?2",
                params![generation.get(), workspace_id],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        transaction
            .commit()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
    }

    pub(super) fn active_generation(
        &self,
        repository: RepositoryIdentityDigest,
    ) -> Result<Option<GenerationId>, SqliteStoreError> {
        self.connection
            .query_row(
                "SELECT active_generation_id FROM workspaces
                 WHERE repository_identity = ?1",
                [repository.as_bytes().as_slice()],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map(|value| value.flatten().map(GenerationId))
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
    }

    pub(super) fn checkpoint(&self) -> Result<CheckpointOutcome, SqliteStoreError> {
        let (busy, log_frames, checkpointed_frames): (i64, i64, i64) = self
            .connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        Ok(CheckpointOutcome {
            busy: positive_database_count(busy)?,
            log_frames: positive_database_count(log_frames)?,
            checkpointed_frames: positive_database_count(checkpointed_frames)?,
        })
    }
}
