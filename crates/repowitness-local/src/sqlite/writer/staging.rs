impl WriterState {
    fn ensure_snapshot(
        &mut self,
        digest: SourceSnapshotDigest,
        identity: RustSourceSnapshotIdentity,
        prepared: &PreparedRustIndex,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        let existing = self
            .connection
            .query_row(
                "SELECT lifecycle_state FROM source_snapshots WHERE snapshot_digest = ?1",
                [digest.as_bytes().as_slice()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if existing.as_deref() == Some("complete") {
            return self.verify_snapshot(digest, identity, prepared);
        }
        if existing.is_some() {
            self.delete_staging_snapshot(digest)?;
        }
        let file_count = fixed_integer(prepared.manifest().count().get())?;
        let source_bytes = fixed_integer(prepared.total_source_bytes())?;
        let syntax_errors = fixed_integer(prepared.total_syntax_error_nodes())?;
        self.connection
            .execute(
                "INSERT INTO source_snapshots(
                    snapshot_digest, lifecycle_state, repository_identity, git_state_digest,
                    worktree_state_digest, configuration_digest, producer_manifest_digest,
                    analysis_schema_digest, canonicalization_version, manifest_digest,
                    file_count, total_source_bytes, total_syntax_error_nodes
                 ) VALUES (
                    ?1, 'staging', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12
                 )",
                params![
                    digest.as_bytes().as_slice(),
                    identity.repository().as_bytes().as_slice(),
                    identity.git_state().as_bytes().as_slice(),
                    identity.worktree_state().as_bytes().as_slice(),
                    identity.configuration().as_bytes().as_slice(),
                    identity.producer_manifest().as_bytes().as_slice(),
                    identity.analysis_schema().as_bytes().as_slice(),
                    i64::from(identity.canonicalization_version()),
                    prepared.manifest_digest().as_bytes().as_slice(),
                    file_count,
                    source_bytes,
                    syntax_errors
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        for (batch_index, batch) in prepared
            .manifest()
            .as_slice()
            .chunks(WRITE_BATCH_ROWS)
            .enumerate()
        {
            check_control(control)?;
            let transaction = self.transaction()?;
            for (offset, entry) in batch.iter().enumerate() {
                let ordinal = batch_index
                    .checked_mul(WRITE_BATCH_ROWS)
                    .and_then(|value| value.checked_add(offset))
                    .ok_or(SqliteStoreError::CountNotRepresentable)?;
                transaction
                    .execute(
                        "INSERT INTO source_manifest_entries(
                            snapshot_digest, ordinal, repository_path, file_kind, content_digest
                         ) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            digest.as_bytes().as_slice(),
                            fixed_usize(ordinal)?,
                            entry.path().as_bytes(),
                            file_kind(*entry.file_type()),
                            entry.content_digest().as_bytes().as_slice()
                        ],
                    )
                    .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            }
            transaction
                .commit()
                .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        }
        check_control(control)?;
        let changed = self
            .connection
            .execute(
                "UPDATE source_snapshots SET lifecycle_state = 'complete'
                 WHERE snapshot_digest = ?1 AND lifecycle_state = 'staging'
                 AND file_count = (
                    SELECT count(*) FROM source_manifest_entries
                    WHERE snapshot_digest = ?1
                 )",
                [digest.as_bytes().as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        Ok(())
    }

    fn ensure_artifact(
        &mut self,
        file: &PreparedRustFile,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        let identity = file.artifact_identity();
        let expected_digest = hash_analysis_artifact_key(&AnalysisArtifactKey::new(
            file.content_digest(),
            identity.producer_manifest(),
            identity.configuration(),
            identity.schema(),
            identity.canonicalization_version(),
        ));
        if expected_digest != file.artifact_digest() {
            return Err(SqliteStoreError::PreparedIdentityMismatch);
        }
        let existing = self
            .connection
            .query_row(
                "SELECT lifecycle_state FROM analysis_artifacts WHERE artifact_digest = ?1",
                [expected_digest.as_bytes().as_slice()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if existing.as_deref() == Some("complete") {
            return self.verify_artifact(file, control);
        }
        if existing.is_some() {
            self.delete_staging_artifact(expected_digest.as_bytes())?;
        }
        let analysis = file.analysis();
        let payload_digest = hash_analysis_artifact_payload(analysis);
        self.connection
            .execute(
                "INSERT INTO analysis_artifacts(
                    artifact_digest, lifecycle_state, source_content_digest,
                    producer_manifest_digest, configuration_digest, analysis_schema_digest,
                    canonicalization_version, fact_count, visited_nodes, syntax_error_nodes,
                    payload_digest, language
                 ) VALUES (?1, 'staging', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    expected_digest.as_bytes().as_slice(),
                    file.content_digest().as_bytes().as_slice(),
                    identity.producer_manifest().as_bytes().as_slice(),
                    identity.configuration().as_bytes().as_slice(),
                    identity.schema().as_bytes().as_slice(),
                    i64::from(identity.canonicalization_version()),
                    fixed_usize(analysis.facts().len())?,
                    i64::from(analysis.visited_nodes()),
                    i64::from(analysis.syntax_error_nodes()),
                    payload_digest.as_bytes().as_slice(),
                    file.language().as_str()
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        self.stage_artifact_facts(file, expected_digest, control)?;
        check_control(control)?;
        let changed = self
            .connection
            .execute(
                "UPDATE analysis_artifacts SET lifecycle_state = 'complete'
                 WHERE artifact_digest = ?1 AND lifecycle_state = 'staging'
                 AND fact_count = (
                    SELECT count(*) FROM artifact_facts WHERE artifact_digest = ?1
                 )
                 AND (
                    language != 'rust'
                    OR fact_count = (
                        SELECT count(*) FROM artifact_fact_correspondence
                        WHERE artifact_digest = ?1
                          AND profile_id = ?2
                          AND profile_version = ?3
                    )
                 )",
                params![
                    expected_digest.as_bytes().as_slice(),
                    RUST_CORRESPONDENCE_PROFILE_ID,
                    i64::from(RUST_CORRESPONDENCE_PROFILE_VERSION)
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        Ok(())
    }

    fn stage_artifact_facts(
        &mut self,
        file: &PreparedRustFile,
        artifact_digest: repowitness_domain::AnalysisArtifactDigest,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        for (batch_index, batch) in file.analysis().facts().chunks(WRITE_BATCH_ROWS).enumerate() {
            check_control(control)?;
            let transaction = self.transaction()?;
            for (offset, fact) in batch.iter().enumerate() {
                let ordinal = batch_index
                    .checked_mul(WRITE_BATCH_ROWS)
                    .and_then(|value| value.checked_add(offset))
                    .ok_or(SqliteStoreError::CountNotRepresentable)?;
                transaction
                    .execute(
                        "INSERT INTO artifact_facts(
                            artifact_digest, ordinal, kind, name, qualified_name,
                            name_start, name_end, declaration_start, declaration_end
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        params![
                            artifact_digest.as_bytes().as_slice(),
                            fixed_usize(ordinal)?,
                            fact.kind().as_str(),
                            fact.name(),
                            fact.qualified_name(),
                            fixed_integer(fact.name_span().start().get())?,
                            fixed_integer(fact.name_span().end().get())?,
                            fixed_integer(fact.declaration_span().start().get())?,
                            fixed_integer(fact.declaration_span().end().get())?
                        ],
                    )
                    .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
                if let Some(correspondence) = fact.correspondence() {
                    transaction
                        .execute(
                            "INSERT INTO artifact_fact_correspondence(
                                artifact_digest, fact_ordinal, profile_id, profile_version,
                                declaration_digest, name_elided_digest
                             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            params![
                                artifact_digest.as_bytes().as_slice(),
                                fixed_usize(ordinal)?,
                                RUST_CORRESPONDENCE_PROFILE_ID,
                                i64::from(RUST_CORRESPONDENCE_PROFILE_VERSION),
                                correspondence.declaration().as_bytes().as_slice(),
                                correspondence.name_elided().as_bytes().as_slice()
                            ],
                        )
                        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
                }
            }
            transaction
                .commit()
                .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        }
        Ok(())
    }

    fn create_generation(
        &mut self,
        workspace_id: i64,
        source_epoch: u64,
        snapshot: SourceSnapshotDigest,
    ) -> Result<GenerationId, SqliteStoreError> {
        self.connection
            .execute(
                "INSERT INTO index_generations(
                    workspace_id, source_epoch, snapshot_digest, lifecycle_state
                 ) VALUES (?1, ?2, ?3, 'discovered')",
                params![
                    workspace_id,
                    fixed_integer(source_epoch)?,
                    snapshot.as_bytes().as_slice()
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        Ok(GenerationId(self.connection.last_insert_rowid()))
    }

    fn stage_generation_rows(
        &mut self,
        generation: GenerationId,
        prepared: &PreparedRustIndex,
        coverage: RustIndexCoverage,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        let projection = self.active_search_projection()?;
        self.transition(generation, "discovered", "extracting")?;
        for (batch_index, batch) in prepared.files().chunks(WRITE_BATCH_ROWS).enumerate() {
            check_control(control)?;
            let transaction = self.transaction()?;
            for (offset, file) in batch.iter().enumerate() {
                let ordinal = batch_index
                    .checked_mul(WRITE_BATCH_ROWS)
                    .and_then(|value| value.checked_add(offset))
                    .ok_or(SqliteStoreError::CountNotRepresentable)?;
                transaction
                    .execute(
                        "INSERT INTO generation_files(
                            generation_id, ordinal, repository_path,
                            content_digest, artifact_digest
                         ) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            generation.get(),
                            fixed_usize(ordinal)?,
                            file.path().as_bytes(),
                            file.content_digest().as_bytes().as_slice(),
                            file.artifact_digest().as_bytes().as_slice()
                        ],
                    )
                    .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            }
            transaction
                .commit()
                .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        }
        self.transition(generation, "extracting", "resolving")?;
        for file in prepared.files() {
            for (batch_index, batch) in file.analysis().facts().chunks(WRITE_BATCH_ROWS).enumerate()
            {
                check_control(control)?;
                let transaction = self.transaction()?;
                for (offset, fact) in batch.iter().enumerate() {
                    let ordinal = batch_index
                        .checked_mul(WRITE_BATCH_ROWS)
                        .and_then(|value| value.checked_add(offset))
                        .ok_or(SqliteStoreError::CountNotRepresentable)?;
                    transaction
                        .execute(
                            projection.stage_insert_sql(),
                            params![
                                generation.get(),
                                file.path().as_bytes(),
                                fixed_usize(ordinal)?,
                                file.content_digest().as_bytes().as_slice(),
                                file.artifact_digest().as_bytes().as_slice(),
                                fixed_integer(fact.name_span().start().get())?,
                                fixed_integer(fact.name_span().end().get())?,
                                fixed_integer(fact.declaration_span().start().get())?,
                                fixed_integer(fact.declaration_span().end().get())?,
                                fact.kind().as_str(),
                                fact.name(),
                                fact.qualified_name()
                            ],
                        )
                        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
                }
                transaction
                    .commit()
                    .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            }
        }
        self.transition(generation, "resolving", "validating")?;
        check_control(control)?;
        self.validate_generation(generation, prepared, projection)?;
        let changed = self
            .connection
            .execute(
                "UPDATE index_generations
                 SET searched_count = ?1, skipped_count = ?2,
                     unresolved_count = ?3, truncated_count = ?4,
                     lifecycle_state = 'ready'
                 WHERE generation_id = ?5 AND lifecycle_state = 'validating'",
                params![
                    fixed_integer(coverage.searched())?,
                    fixed_integer(coverage.skipped())?,
                    fixed_integer(coverage.unresolved())?,
                    fixed_integer(coverage.truncated())?,
                    generation.get()
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        Ok(())
    }

    fn verify_snapshot(
        &self,
        digest: SourceSnapshotDigest,
        identity: RustSourceSnapshotIdentity,
        prepared: &PreparedRustIndex,
    ) -> Result<(), SqliteStoreError> {
        let expected = (
            identity.repository().as_bytes().to_vec(),
            identity.git_state().as_bytes().to_vec(),
            identity.worktree_state().as_bytes().to_vec(),
            identity.configuration().as_bytes().to_vec(),
            identity.producer_manifest().as_bytes().to_vec(),
            identity.analysis_schema().as_bytes().to_vec(),
            i64::from(identity.canonicalization_version()),
            prepared.manifest_digest().as_bytes().to_vec(),
            fixed_integer(prepared.manifest().count().get())?,
            fixed_integer(prepared.total_source_bytes())?,
            fixed_integer(prepared.total_syntax_error_nodes())?,
        );
        let actual = self
            .connection
            .query_row(
                "SELECT repository_identity, git_state_digest, worktree_state_digest,
                        configuration_digest, producer_manifest_digest, analysis_schema_digest,
                        canonicalization_version, manifest_digest, file_count,
                        total_source_bytes, total_syntax_error_nodes
                 FROM source_snapshots WHERE snapshot_digest = ?1
                 AND lifecycle_state = 'complete'",
                [digest.as_bytes().as_slice()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                    ))
                },
            )
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        if actual != expected {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        Ok(())
    }
}
