impl WriterState {
    fn verify_artifact(
        &self,
        file: &PreparedRustFile,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        check_control(control)?;
        let identity = file.artifact_identity();
        let analysis = file.analysis();
        let expected_payload = hash_analysis_artifact_payload(analysis);
        let expected = (
            file.content_digest().as_bytes().to_vec(),
            identity.producer_manifest().as_bytes().to_vec(),
            identity.configuration().as_bytes().to_vec(),
            identity.schema().as_bytes().to_vec(),
            i64::from(identity.canonicalization_version()),
            fixed_usize(analysis.facts().len())?,
            i64::from(analysis.visited_nodes()),
            i64::from(analysis.syntax_error_nodes()),
            file.language().as_str().to_owned(),
        );
        let actual: PersistedArtifactMetadata = self
            .connection
            .query_row(
                "SELECT source_content_digest, producer_manifest_digest,
                        configuration_digest, analysis_schema_digest,
                        canonicalization_version, fact_count, visited_nodes, syntax_error_nodes,
                        language, payload_digest
                 FROM analysis_artifacts
                 WHERE artifact_digest = ?1 AND lifecycle_state = 'complete'",
                [file.artifact_digest().as_bytes().as_slice()],
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
                        row.get::<_, Option<Vec<u8>>>(9)?,
                    ))
                },
            )
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        if actual.0 != expected.0
            || actual.1 != expected.1
            || actual.2 != expected.2
            || actual.3 != expected.3
            || actual.4 != expected.4
            || actual.5 != expected.5
            || actual.6 != expected.6
            || actual.7 != expected.7
            || actual.8 != expected.8
        {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        self.verify_artifact_facts(file, control)?;
        match actual.9 {
            Some(payload) if payload.as_slice() == expected_payload.as_bytes() => {}
            Some(_) => return Err(SqliteStoreError::IntegrityCheckFailed),
            None => {
                let changed = self
                    .connection
                    .execute(
                        "UPDATE analysis_artifacts SET payload_digest = ?2
                         WHERE artifact_digest = ?1 AND lifecycle_state = 'complete'
                         AND payload_digest IS NULL",
                        params![
                            file.artifact_digest().as_bytes().as_slice(),
                            expected_payload.as_bytes().as_slice()
                        ],
                    )
                    .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
                if changed != 1 {
                    return Err(SqliteStoreError::IntegrityCheckFailed);
                }
            }
        }
        check_control(control)?;
        Ok(())
    }

    fn verify_artifact_facts(
        &self,
        file: &PreparedRustFile,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT facts.ordinal, facts.kind, facts.name, facts.qualified_name,
                        facts.name_start, facts.name_end,
                        facts.declaration_start, facts.declaration_end,
                        correspondence.declaration_digest,
                        correspondence.name_elided_digest
                 FROM artifact_facts AS facts
                 LEFT JOIN artifact_fact_correspondence AS correspondence
                   ON correspondence.artifact_digest = facts.artifact_digest
                  AND correspondence.fact_ordinal = facts.ordinal
                  AND correspondence.profile_id = ?2
                  AND correspondence.profile_version = ?3
                 WHERE facts.artifact_digest = ?1
                 ORDER BY facts.ordinal",
            )
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let mut rows = statement
            .query(params![
                file.artifact_digest().as_bytes().as_slice(),
                RUST_CORRESPONDENCE_PROFILE_ID,
                i64::from(RUST_CORRESPONDENCE_PROFILE_VERSION)
            ])
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        for (ordinal, fact) in file.analysis().facts().iter().enumerate() {
            check_control(control)?;
            let row = rows
                .next()
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?
                .ok_or(SqliteStoreError::IntegrityCheckFailed)?;
            let stored_ordinal: i64 = row
                .get(0)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let kind: String = row
                .get(1)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let name: String = row
                .get(2)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let qualified_name: String = row
                .get(3)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let name_start: i64 = row
                .get(4)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let name_end: i64 = row
                .get(5)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let declaration_start: i64 = row
                .get(6)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let declaration_end: i64 = row
                .get(7)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let declaration_digest: Option<Vec<u8>> = row
                .get(8)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let name_elided_digest: Option<Vec<u8>> = row
                .get(9)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let correspondence_matches = match (
                fact.correspondence(),
                declaration_digest.as_deref(),
                name_elided_digest.as_deref(),
            ) {
                (None, None, None) => true,
                (Some(expected), Some(declaration), Some(name_elided)) => {
                    declaration == expected.declaration().as_bytes()
                        && name_elided == expected.name_elided().as_bytes()
                }
                _ => false,
            };
            if stored_ordinal != fixed_usize(ordinal)?
                || kind != fact.kind().as_str()
                || name != fact.name()
                || qualified_name != fact.qualified_name()
                || name_start != fixed_integer(fact.name_span().start().get())?
                || name_end != fixed_integer(fact.name_span().end().get())?
                || declaration_start != fixed_integer(fact.declaration_span().start().get())?
                || declaration_end != fixed_integer(fact.declaration_span().end().get())?
                || !correspondence_matches
            {
                return Err(SqliteStoreError::IntegrityCheckFailed);
            }
        }
        if rows
            .next()
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?
            .is_some()
        {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        Ok(())
    }

    fn validate_generation(
        &self,
        generation: GenerationId,
        prepared: &PreparedRustIndex,
        projection: SearchProjection,
    ) -> Result<(), SqliteStoreError> {
        let search_count_sql = match projection {
            SearchProjection::Primary => {
                "SELECT count(*) FROM generation_search WHERE generation_id = ?1"
            }
            SearchProjection::Rebuild => {
                "SELECT count(*) FROM generation_search_rebuild WHERE generation_id = ?1"
            }
        };
        let files: i64 = self
            .connection
            .query_row(
                "SELECT count(*) FROM generation_files WHERE generation_id = ?1",
                [generation.get()],
                |row| row.get(0),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let facts: i64 = self
            .connection
            .query_row(search_count_sql, [generation.get()], |row| row.get(0))
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if files != fixed_usize(prepared.files().len())?
            || facts != fixed_integer(prepared.total_facts())?
        {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        Ok(())
    }

    fn transition(
        &self,
        generation: GenerationId,
        expected: &str,
        next: &str,
    ) -> Result<(), SqliteStoreError> {
        let changed = self
            .connection
            .execute(
                "UPDATE index_generations SET lifecycle_state = ?1
                 WHERE generation_id = ?2 AND lifecycle_state = ?3",
                params![next, generation.get(), expected],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::GenerationUnavailable);
        }
        Ok(())
    }

    fn fail_generation(
        &self,
        generation: GenerationId,
        target: &str,
    ) -> Result<(), SqliteStoreError> {
        self.connection
            .execute(
                "UPDATE index_generations SET lifecycle_state = ?1
                 WHERE generation_id = ?2
                 AND lifecycle_state IN (
                    'discovered', 'extracting', 'resolving', 'validating', 'ready'
                 )",
                params![target, generation.get()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        self.connection
            .execute(
                "DELETE FROM generation_search WHERE generation_id = ?1",
                [generation.get()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        self.connection
            .execute(
                "DELETE FROM generation_search_rebuild WHERE generation_id = ?1",
                [generation.get()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        self.connection
            .execute(
                "DELETE FROM generation_facts WHERE generation_id = ?1",
                [generation.get()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        self.connection
            .execute(
                "DELETE FROM generation_files WHERE generation_id = ?1",
                [generation.get()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        Ok(())
    }

    fn delete_staging_snapshot(
        &mut self,
        digest: SourceSnapshotDigest,
    ) -> Result<(), SqliteStoreError> {
        let transaction = self.transaction()?;
        transaction
            .execute(
                "DELETE FROM source_manifest_entries WHERE snapshot_digest = ?1",
                [digest.as_bytes().as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        transaction
            .execute(
                "DELETE FROM source_snapshots
                 WHERE snapshot_digest = ?1 AND lifecycle_state = 'staging'",
                [digest.as_bytes().as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        transaction
            .commit()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
    }

    fn delete_staging_artifact(&mut self, digest: &[u8; 32]) -> Result<(), SqliteStoreError> {
        let transaction = self.transaction()?;
        transaction
            .execute(
                "DELETE FROM artifact_fact_correspondence WHERE artifact_digest = ?1",
                [digest.as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        transaction
            .execute(
                "DELETE FROM artifact_facts WHERE artifact_digest = ?1",
                [digest.as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        transaction
            .execute(
                "DELETE FROM analysis_artifacts
                 WHERE artifact_digest = ?1 AND lifecycle_state = 'staging'",
                [digest.as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        transaction
            .commit()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
    }

    fn transaction(&mut self) -> Result<Transaction<'_>, SqliteStoreError> {
        self.connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
    }
}
