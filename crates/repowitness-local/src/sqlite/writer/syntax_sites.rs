type PersistedSyntaxSiteArtifactMetadata = (
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    i64,
    i64,
    i64,
    Option<Vec<u8>>,
    String,
    i64,
    i64,
    i64,
    i64,
    i64,
);

impl WriterState {
    pub(super) fn stage_syntax_sites(
        &mut self,
        generation: GenerationId,
        prepared: &crate::sqlite::PreparedRawSyntaxGeneration,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        self.validate_syntax_site_owner(generation)?;
        self.require_syntax_sites(generation)?;
        let result = self.stage_syntax_sites_inner(generation, prepared, control);
        if let Err(error) = result {
            let target = if error == SqliteStoreError::Cancelled { "cancelled" } else { "failed" };
            let _ = self.fail_generation(generation, target);
            return Err(error);
        }
        Ok(())
    }

    fn validate_syntax_site_owner(&self, generation: GenerationId) -> Result<(), SqliteStoreError> {
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

    fn require_syntax_sites(&mut self, generation: GenerationId) -> Result<(), SqliteStoreError> {
        let changed = self.connection.execute(
            "INSERT INTO generation_syntax_site_requirements(generation_id, site_profile_version)
             VALUES (?1, ?2)",
            params![generation.get(), i64::from(repowitness_analysis::RAW_SYNTAX_SITE_PROFILE_VERSION)],
        ).map_err(|_| SqliteStoreError::InvalidSyntaxSitePublication)?;
        if changed == 1 { Ok(()) } else { Err(SqliteStoreError::InvalidSyntaxSitePublication) }
    }

    fn stage_syntax_sites_inner(
        &mut self,
        generation: GenerationId,
        prepared: &crate::sqlite::PreparedRawSyntaxGeneration,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        for artifact in prepared.artifacts() {
            check_control(control)?;
            self.ensure_syntax_site_artifact(artifact, control)?;
        }
        self.create_syntax_site_publication(generation, prepared)?;
        for batch in prepared.artifacts().chunks(WRITE_BATCH_ROWS) {
            check_control(control)?;
            let transaction = self.transaction()?;
            for artifact in batch {
                transaction.execute(
                    "INSERT INTO generation_syntax_site_artifacts(
                        generation_id, repository_path, syntax_site_artifact_digest
                     ) VALUES (?1, ?2, ?3)",
                    params![
                        generation.get(),
                        artifact.path().as_bytes(),
                        artifact.artifact_digest().as_bytes().as_slice(),
                    ],
                ).map_err(|_| SqliteStoreError::InvalidSyntaxSitePublication)?;
            }
            commit_mutation(transaction)?;
        }
        check_control(control)?;
        self.validate_syntax_site_owner(generation)?;
        let changed = self.connection.execute(
            "UPDATE generation_syntax_site_publications
             SET lifecycle_state = 'complete'
             WHERE generation_id = ?1 AND lifecycle_state = 'staging'",
            [generation.get()],
        ).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        if changed == 1 { Ok(()) } else { Err(SqliteStoreError::IntegrityCheckFailed) }
    }

    fn create_syntax_site_publication(
        &mut self,
        generation: GenerationId,
        prepared: &crate::sqlite::PreparedRawSyntaxGeneration,
    ) -> Result<(), SqliteStoreError> {
        let changed = self.connection.execute(
            "INSERT INTO generation_syntax_site_publications(
                generation_id, lifecycle_state, site_profile_version, artifact_count,
                site_count, visited_node_count, syntax_error_node_count, owned_text_bytes,
                import_site_count, reference_site_count, call_site_count, test_marker_site_count
             ) VALUES (?1, 'staging', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                generation.get(),
                i64::from(repowitness_analysis::RAW_SYNTAX_SITE_PROFILE_VERSION),
                fixed_usize(prepared.artifacts().len())?,
                fixed_integer(prepared.site_count())?,
                fixed_integer(prepared.visited_nodes())?,
                fixed_integer(prepared.syntax_error_nodes())?,
                fixed_integer(prepared.owned_text_bytes())?,
                fixed_integer(prepared.import_sites())?,
                fixed_integer(prepared.reference_sites())?,
                fixed_integer(prepared.call_sites())?,
                fixed_integer(prepared.test_marker_sites())?,
            ],
        ).map_err(|_| SqliteStoreError::InvalidSyntaxSitePublication)?;
        if changed == 1 { Ok(()) } else { Err(SqliteStoreError::InvalidSyntaxSitePublication) }
    }

    fn ensure_syntax_site_artifact(
        &mut self,
        artifact: &crate::sqlite::PreparedRawSyntaxArtifact,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        let inserted = {
            let transaction = self.transaction()?;
            let key = artifact.key();
            let coverage = artifact.analysis().coverage();
            let changed = transaction.execute(
                "INSERT OR IGNORE INTO analysis_artifacts(
                    artifact_digest, lifecycle_state, source_content_digest,
                    producer_manifest_digest, configuration_digest, analysis_schema_digest,
                    canonicalization_version, fact_count, visited_nodes, syntax_error_nodes,
                    known_parser_limitation_nodes, payload_digest, language
                 ) VALUES (?1, 'staging', ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, 0, ?9, ?10)",
                params![
                    artifact.artifact_digest().as_bytes().as_slice(),
                    key.source_digest().as_bytes().as_slice(),
                    key.analyzer_identity().as_bytes().as_slice(),
                    key.configuration_identity().as_bytes().as_slice(),
                    key.schema_identity().as_bytes().as_slice(),
                    i64::from(*key.canonicalization_version()),
                    i64::from(artifact.analysis().visited_nodes()),
                    i64::from(artifact.analysis().syntax_error_nodes()),
                    artifact.payload_digest().as_slice(),
                    artifact.language().as_str(),
                ],
            ).map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            if changed == 1 {
                transaction.execute(
                    "INSERT INTO syntax_site_artifacts(
                        artifact_digest, site_profile_version, site_count, max_observed_depth,
                        owned_text_bytes, import_support, reference_support, call_support,
                        test_marker_support, import_emitted, reference_emitted, call_emitted,
                        test_marker_emitted
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![
                        artifact.artifact_digest().as_bytes().as_slice(),
                        i64::from(repowitness_analysis::RAW_SYNTAX_SITE_PROFILE_VERSION),
                        fixed_usize(artifact.analysis().sites().len())?,
                        i64::from(artifact.analysis().max_observed_depth()),
                        fixed_integer(artifact.analysis().owned_text_bytes())?,
                        support_text(coverage.for_kind(repowitness_analysis::RawSyntaxSiteKind::Import)),
                        support_text(coverage.for_kind(repowitness_analysis::RawSyntaxSiteKind::Reference)),
                        support_text(coverage.for_kind(repowitness_analysis::RawSyntaxSiteKind::Call)),
                        support_text(coverage.for_kind(repowitness_analysis::RawSyntaxSiteKind::TestMarker)),
                        i64::from(coverage.for_kind(repowitness_analysis::RawSyntaxSiteKind::Import).emitted()),
                        i64::from(coverage.for_kind(repowitness_analysis::RawSyntaxSiteKind::Reference).emitted()),
                        i64::from(coverage.for_kind(repowitness_analysis::RawSyntaxSiteKind::Call).emitted()),
                        i64::from(coverage.for_kind(repowitness_analysis::RawSyntaxSiteKind::TestMarker).emitted()),
                    ],
                ).map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            }
            commit_mutation(transaction)?;
            changed == 1
        };
        if !inserted { return self.verify_syntax_site_artifact(artifact, control); }
        let result = self.insert_syntax_sites(artifact, control).and_then(|()| {
            let changed = self.connection.execute(
                "UPDATE analysis_artifacts SET lifecycle_state = 'complete'
                 WHERE artifact_digest = ?1 AND lifecycle_state = 'staging'",
                [artifact.artifact_digest().as_bytes().as_slice()],
            ).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            if changed == 1 { Ok(()) } else { Err(SqliteStoreError::IntegrityCheckFailed) }
        });
        if result.is_err() { let _ = self.delete_staging_syntax_site_artifact(artifact.artifact_digest()); }
        result
    }

    fn insert_syntax_sites(
        &mut self,
        artifact: &crate::sqlite::PreparedRawSyntaxArtifact,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        for batch in artifact.analysis().sites().chunks(WRITE_BATCH_ROWS) {
            check_control(control)?;
            let transaction = self.transaction()?;
            for site in batch {
                transaction.execute(
                    "INSERT INTO syntax_sites(
                        artifact_digest, ordinal, site_kind, extraction_evidence,
                        occurrence_start, occurrence_end, target_start, target_end, raw_target
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        artifact.artifact_digest().as_bytes().as_slice(),
                        i64::from(site.ordinal().get()),
                        site.kind().as_str(),
                        site.evidence().as_str(),
                        fixed_integer(site.occurrence_span().start().get())?,
                        fixed_integer(site.occurrence_span().end().get())?,
                        fixed_integer(site.target_span().start().get())?,
                        fixed_integer(site.target_span().end().get())?,
                        site.raw_target(),
                    ],
                ).map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            }
            commit_mutation(transaction)?;
        }
        Ok(())
    }

    fn verify_syntax_site_artifact(
        &self,
        artifact: &crate::sqlite::PreparedRawSyntaxArtifact,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        check_control(control)?;
        let key = artifact.key();
        let metadata: Option<PersistedSyntaxSiteArtifactMetadata> = self.connection.query_row(
            "SELECT base.source_content_digest, base.producer_manifest_digest,
                    base.configuration_digest, base.analysis_schema_digest,
                    base.canonicalization_version, base.visited_nodes,
                    base.syntax_error_nodes, base.payload_digest, base.language,
                    site.site_profile_version, site.site_count, site.max_observed_depth,
                    site.owned_text_bytes,
                    site.import_emitted + site.reference_emitted + site.call_emitted + site.test_marker_emitted
             FROM analysis_artifacts AS base
             JOIN syntax_site_artifacts AS site USING (artifact_digest)
             WHERE base.artifact_digest = ?1 AND base.lifecycle_state = 'complete' AND base.fact_count = 0",
            [artifact.artifact_digest().as_bytes().as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?, row.get(12)?, row.get(13)?)),
        ).optional().map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let Some(metadata) = metadata else { return Err(SqliteStoreError::IntegrityCheckFailed); };
        if metadata.0 != key.source_digest().as_bytes()
            || metadata.1 != key.analyzer_identity().as_bytes()
            || metadata.2 != key.configuration_identity().as_bytes()
            || metadata.3 != key.schema_identity().as_bytes()
            || metadata.4 != i64::from(*key.canonicalization_version())
            || metadata.5 != i64::from(artifact.analysis().visited_nodes())
            || metadata.6 != i64::from(artifact.analysis().syntax_error_nodes())
            || metadata.8 != artifact.language().as_str()
            || metadata.9 != i64::from(repowitness_analysis::RAW_SYNTAX_SITE_PROFILE_VERSION)
            || metadata.10 != fixed_usize(artifact.analysis().sites().len())?
            || metadata.11 != i64::from(artifact.analysis().max_observed_depth())
            || metadata.12 != fixed_integer(artifact.analysis().owned_text_bytes())?
            || metadata.13 != fixed_usize(artifact.analysis().sites().len())?
        { return Err(SqliteStoreError::IntegrityCheckFailed); }
        let mut statement = self.connection.prepare(
            "SELECT ordinal, site_kind, extraction_evidence, occurrence_start, occurrence_end,
                    target_start, target_end, raw_target
             FROM syntax_sites WHERE artifact_digest = ?1 ORDER BY ordinal",
        ).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let mut rows = statement.query([artifact.artifact_digest().as_bytes().as_slice()])
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        for site in artifact.analysis().sites() {
            check_control(control)?;
            let row = rows.next().map_err(|_| SqliteStoreError::IntegrityCheckFailed)?
                .ok_or(SqliteStoreError::IntegrityCheckFailed)?;
            let persisted: (i64, String, String, i64, i64, i64, i64, String) = (
                row.get(0).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
                row.get(1).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
                row.get(2).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
                row.get(3).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
                row.get(4).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
                row.get(5).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
                row.get(6).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
                row.get(7).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
            );
            if persisted != (
                i64::from(site.ordinal().get()), site.kind().as_str().to_owned(), site.evidence().as_str().to_owned(),
                fixed_integer(site.occurrence_span().start().get())?, fixed_integer(site.occurrence_span().end().get())?,
                fixed_integer(site.target_span().start().get())?, fixed_integer(site.target_span().end().get())?, site.raw_target().to_owned(),
            ) { return Err(SqliteStoreError::IntegrityCheckFailed); }
        }
        if rows.next().map_err(|_| SqliteStoreError::IntegrityCheckFailed)?.is_some() {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        match metadata.7 {
            Some(payload) if payload.as_slice() == artifact.payload_digest().as_slice() => {}
            Some(_) => return Err(SqliteStoreError::IntegrityCheckFailed),
            None => {
                check_control(control)?;
                let changed = self.connection.execute(
                    "UPDATE analysis_artifacts SET payload_digest = ?2
                     WHERE artifact_digest = ?1 AND lifecycle_state = 'complete'
                     AND payload_digest IS NULL",
                    params![
                        artifact.artifact_digest().as_bytes().as_slice(),
                        artifact.payload_digest().as_slice(),
                    ],
                ).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
                if changed != 1 {
                    return Err(SqliteStoreError::IntegrityCheckFailed);
                }
            }
        }
        check_control(control)
    }

    fn delete_staging_syntax_site_artifact(
        &mut self,
        digest: repowitness_domain::AnalysisArtifactDigest,
    ) -> Result<(), SqliteStoreError> {
        let transaction = self.transaction()?;
        transaction.execute("DELETE FROM syntax_sites WHERE artifact_digest = ?1", [digest.as_bytes().as_slice()])
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        transaction.execute("DELETE FROM syntax_site_artifacts WHERE artifact_digest = ?1", [digest.as_bytes().as_slice()])
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        transaction.execute("DELETE FROM analysis_artifacts WHERE artifact_digest = ?1 AND lifecycle_state = 'staging'", [digest.as_bytes().as_slice()])
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        commit_mutation(transaction)
    }
}

fn support_text(coverage: repowitness_analysis::RawSyntaxSiteKindCoverage) -> &'static str {
    match coverage.support() {
        repowitness_analysis::RawSyntaxSiteSupport::Available => "available",
        repowitness_analysis::RawSyntaxSiteSupport::Unsupported => "unsupported",
    }
}
