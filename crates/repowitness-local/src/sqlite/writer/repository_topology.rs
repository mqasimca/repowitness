use repowitness_application::REPOSITORY_TOPOLOGY_PROFILE_VERSION;
impl WriterState {
    pub(super) fn stage_repository_topology(
        &mut self,
        generation: GenerationId,
        prepared: &crate::PreparedRepositoryTopology,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        self.validate_repository_topology_owner(generation)?;
        self.require_repository_topology(generation)?;
        let result = self.stage_repository_topology_inner(generation, prepared, control);
        if let Err(error) = result {
            let target = if error == SqliteStoreError::Cancelled { "cancelled" } else { "failed" };
            let _ = self.fail_generation(generation, target);
            return Err(error);
        }
        Ok(())
    }

    fn validate_repository_topology_owner(&self, generation: GenerationId) -> Result<(), SqliteStoreError> {
        let state: Option<(i64, i64, String)> = self.connection.query_row(
            "SELECT generation.source_epoch, workspace.source_epoch, generation.lifecycle_state
             FROM index_generations AS generation
             JOIN workspaces AS workspace ON workspace.workspace_id = generation.workspace_id
             WHERE generation.generation_id = ?1",
            [generation.get()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional().map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        match state {
            Some((generation_epoch, workspace_epoch, lifecycle))
                if generation_epoch == workspace_epoch && lifecycle == "ready" => Ok(()),
            Some(_) => Err(SqliteStoreError::StaleSourceEpoch),
            None => Err(SqliteStoreError::GenerationUnavailable),
        }
    }

    fn require_repository_topology(&mut self, generation: GenerationId) -> Result<(), SqliteStoreError> {
        let changed = self.connection.execute(
            "INSERT INTO generation_repository_topology_requirements(generation_id, topology_profile_version)
             VALUES (?1, ?2)",
            params![generation.get(), i64::from(REPOSITORY_TOPOLOGY_PROFILE_VERSION)],
        ).map_err(|_| SqliteStoreError::InvalidRepositoryTopologyPublication)?;
        if changed == 1 { Ok(()) } else { Err(SqliteStoreError::InvalidRepositoryTopologyPublication) }
    }

    fn stage_repository_topology_inner(
        &mut self,
        generation: GenerationId,
        prepared: &crate::PreparedRepositoryTopology,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        let total = fixed_usize(prepared.entries().len())?;
        let changed = self.connection.execute(
            "INSERT INTO generation_repository_topology_publications(
                generation_id, lifecycle_state, topology_profile_version, topology_digest,
                discovered_path_count, omitted_path_count, total_path_count
             ) VALUES (?1, 'staging', ?2, ?3, ?4, 0, ?4)",
            params![generation.get(), i64::from(REPOSITORY_TOPOLOGY_PROFILE_VERSION), prepared.digest().as_slice(), total],
        ).map_err(|_| SqliteStoreError::InvalidRepositoryTopologyPublication)?;
        if changed != 1 { return Err(SqliteStoreError::InvalidRepositoryTopologyPublication); }
        for batch in prepared.entries().chunks(WRITE_BATCH_ROWS) {
            check_control(control)?;
            let transaction = self.transaction()?;
            for (path, category) in batch {
                transaction.execute(
                    "INSERT INTO generation_repository_topology_entries(generation_id, repository_path, category)
                     VALUES (?1, ?2, ?3)",
                    params![generation.get(), path.as_bytes(), category.as_str()],
                ).map_err(|_| SqliteStoreError::InvalidRepositoryTopologyPublication)?;
            }
            commit_mutation(transaction)?;
        }
        check_control(control)?;
        self.validate_repository_topology_owner(generation)?;
        let changed = self.connection.execute(
            "UPDATE generation_repository_topology_publications
             SET lifecycle_state = 'complete'
             WHERE generation_id = ?1 AND lifecycle_state = 'staging'",
            [generation.get()],
        ).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        if changed == 1 { Ok(()) } else { Err(SqliteStoreError::IntegrityCheckFailed) }
    }
}
