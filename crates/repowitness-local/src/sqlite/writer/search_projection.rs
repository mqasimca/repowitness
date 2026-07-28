impl WriterState {
    pub(super) fn rebuild_search_projection(
        &mut self,
        limits: ProjectionRebuildLimits,
        control: WriteControl<'_>,
    ) -> Result<ProjectionRebuildOutcome, SqliteStoreError> {
        check_control(control)?;
        let progress_cancelled = Arc::clone(control.cancelled);
        let deadline = control.deadline;
        self.connection
            .progress_handler(
                1_000,
                Some(move || {
                    progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline
                }),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let result = self.rebuild_search_projection_inner(limits, control);
        let clear_result = self
            .connection
            .progress_handler(0, None::<fn() -> bool>)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed);
        clear_result?;
        match result {
            Err(SqliteStoreError::DatabaseOperationFailed) => {
                check_control(control)?;
                Err(SqliteStoreError::DatabaseOperationFailed)
            }
            other => other,
        }
    }

    fn rebuild_search_projection_inner(
        &mut self,
        limits: ProjectionRebuildLimits,
        control: WriteControl<'_>,
    ) -> Result<ProjectionRebuildOutcome, SqliteStoreError> {
        let current = self.active_search_projection()?;
        let target = current.inactive();
        let expected_rows = self.projection_source_row_count()?;
        if expected_rows > limits.max_rows() {
            return Err(SqliteStoreError::ProjectionRebuildRowLimitExceeded);
        }
        check_control(control)?;
        self.reset_projection(target)?;
        let (rebuilt_rows, write_batches) =
            self.populate_projection(target, expected_rows, control)?;
        self.verify_projection(target, expected_rows)?;
        check_control(control)?;
        self.publish_projection(current, target)?;
        Ok(ProjectionRebuildOutcome {
            previous_slot: u8::try_from(current.slot())
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
            active_slot: u8::try_from(target.slot())
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
            rebuilt_rows,
            write_batches,
        })
    }

    fn projection_source_row_count(&self) -> Result<u64, SqliteStoreError> {
        let rows: i64 = self
            .connection
            .query_row(
                "SELECT count(*)
                 FROM index_generations AS generation
                 JOIN generation_files AS file USING (generation_id)
                 JOIN artifact_facts AS fact USING (artifact_digest)
                 WHERE generation.lifecycle_state IN ('ready', 'active', 'retained')",
                [],
                |row| row.get(0),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        positive_database_count(rows)
    }

    fn reset_projection(&mut self, target: SearchProjection) -> Result<(), SqliteStoreError> {
        let reset = self.transaction()?;
        reset
            .execute_batch(target.recreate_sql())
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        reset
            .commit()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
    }

    fn populate_projection(
        &mut self,
        target: SearchProjection,
        expected_rows: u64,
        control: WriteControl<'_>,
    ) -> Result<(u64, u64), SqliteStoreError> {
        let mut rebuilt_rows = 0_u64;
        let mut write_batches = 0_u64;
        let mut cursor = INITIAL_PROJECTION_CURSOR;
        while rebuilt_rows < expected_rows {
            check_control(control)?;
            let transaction = self.transaction()?;
            let inserted = transaction
                .execute(
                    target.rebuild_insert_sql(),
                    params![
                        cursor.generation,
                        cursor.file_ordinal,
                        cursor.fact_ordinal,
                        fixed_usize(WRITE_BATCH_ROWS)?
                    ],
                )
                .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            if inserted == 0 || inserted > WRITE_BATCH_ROWS {
                return Err(SqliteStoreError::IntegrityCheckFailed);
            }
            let offset = inserted
                .checked_sub(1)
                .ok_or(SqliteStoreError::IntegrityCheckFailed)?;
            cursor = transaction
                .query_row(
                    NEXT_PROJECTION_CURSOR,
                    params![
                        cursor.generation,
                        cursor.file_ordinal,
                        cursor.fact_ordinal,
                        fixed_usize(offset)?
                    ],
                    |row| {
                        Ok(ProjectionCursor {
                            generation: row.get(0)?,
                            file_ordinal: row.get(1)?,
                            fact_ordinal: row.get(2)?,
                        })
                    },
                )
                .map_err(projection_validation_error)?;
            transaction
                .commit()
                .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            rebuilt_rows = rebuilt_rows
                .checked_add(
                    u64::try_from(inserted).map_err(|_| SqliteStoreError::CountNotRepresentable)?,
                )
                .ok_or(SqliteStoreError::CountNotRepresentable)?;
            write_batches = write_batches
                .checked_add(1)
                .ok_or(SqliteStoreError::CountNotRepresentable)?;
        }
        Ok((rebuilt_rows, write_batches))
    }

    fn verify_projection(
        &self,
        target: SearchProjection,
        expected_rows: u64,
    ) -> Result<(), SqliteStoreError> {
        let actual_rows: i64 = self
            .connection
            .query_row(target.count_sql(), [], |row| row.get(0))
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if positive_database_count(actual_rows)? != expected_rows {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        self.connection
            .execute(target.integrity_sql(), [])
            .map_err(projection_validation_error)?;
        Ok(())
    }

    fn publish_projection(
        &mut self,
        current: SearchProjection,
        target: SearchProjection,
    ) -> Result<(), SqliteStoreError> {
        let publication = self.transaction()?;
        let changed = publication
            .execute(
                "UPDATE search_projection_state SET active_slot = ?1
                 WHERE singleton = 1 AND active_slot = ?2",
                params![target.slot(), current.slot()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        publication
            .commit()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
    }

    fn active_search_projection(&self) -> Result<SearchProjection, SqliteStoreError> {
        let slot = self
            .connection
            .query_row(
                "SELECT active_slot FROM search_projection_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        SearchProjection::from_slot(slot)
    }

    fn workspace(
        &self,
        repository: RepositoryIdentityDigest,
        source_epoch: u64,
    ) -> Result<i64, SqliteStoreError> {
        let epoch = fixed_integer(source_epoch)?;
        self.connection
            .query_row(
                "SELECT workspace_id FROM workspaces
                 WHERE repository_identity = ?1 AND source_epoch = ?2",
                params![repository.as_bytes().as_slice(), epoch],
                |row| row.get(0),
            )
            .map_err(|_| SqliteStoreError::StaleSourceEpoch)
    }
}
