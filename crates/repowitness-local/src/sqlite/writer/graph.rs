impl WriterState {
    pub(super) fn stage_graph(
        &mut self,
        generation: GenerationId,
        prepared: &crate::sqlite::graph::PreparedRustGraphGeneration,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        self.validate_graph_owner(generation, prepared)?;
        self.require_graph(generation, prepared.resolution().profile_version())?;
        let result = self.stage_graph_inner(generation, prepared, control);
        if let Err(error) = result {
            let target = if error == SqliteStoreError::Cancelled {
                "cancelled"
            } else {
                "failed"
            };
            let _ = self.fail_generation(generation, target);
            return Err(error);
        }
        Ok(())
    }

    fn stage_graph_inner(
        &mut self,
        generation: GenerationId,
        prepared: &crate::sqlite::graph::PreparedRustGraphGeneration,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        for artifact in prepared.artifacts() {
            check_control(control)?;
            self.ensure_graph_artifact(artifact, control)?;
        }
        self.create_graph_publication(generation, prepared)?;
        self.stage_graph_sources(generation, prepared, control)?;
        self.stage_graph_artifacts(generation, prepared, control)?;
        self.stage_graph_definitions(generation, prepared, control)?;
        self.stage_graph_resolutions(generation, prepared, control)?;
        self.stage_graph_candidates(generation, prepared, control)?;
        self.stage_graph_edges(generation, prepared, control)?;
        check_control(control)?;
        self.validate_graph_owner(generation, prepared)?;
        let changed = self
            .connection
            .execute(
                "UPDATE generation_graph_publications
                 SET lifecycle_state = 'complete'
                 WHERE generation_id = ?1 AND lifecycle_state = 'staging'",
                [generation.get()],
            )
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        Ok(())
    }

    fn validate_graph_owner(
        &self,
        generation: GenerationId,
        prepared: &crate::sqlite::graph::PreparedRustGraphGeneration,
    ) -> Result<(), SqliteStoreError> {
        if !prepared
            .sources()
            .iter()
            .any(|source| source.generation() == generation)
        {
            return Err(SqliteStoreError::InvalidGraphPublication);
        }
        let state: Option<(i64, i64, String)> = self
            .connection
            .query_row(
                "SELECT generation.source_epoch, workspace.source_epoch,
                        generation.lifecycle_state
                 FROM index_generations AS generation
                 JOIN workspaces AS workspace
                   ON workspace.workspace_id = generation.workspace_id
                 WHERE generation.generation_id = ?1",
                [generation.get()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        match state {
            Some((generation_epoch, workspace_epoch, lifecycle))
                if generation_epoch == workspace_epoch && lifecycle == "ready" =>
            {
                Ok(())
            }
            Some(_) => Err(SqliteStoreError::StaleSourceEpoch),
            None => Err(SqliteStoreError::GenerationUnavailable),
        }
    }

    fn require_graph(
        &mut self,
        generation: GenerationId,
        resolver_profile: u32,
    ) -> Result<(), SqliteStoreError> {
        let changed = self
            .connection
            .execute(
                "INSERT INTO generation_graph_requirements(
                    generation_id, resolver_profile_version
                 ) VALUES (?1, ?2)",
                params![generation.get(), i64::from(resolver_profile)],
            )
            .map_err(|_| SqliteStoreError::InvalidGraphPublication)?;
        if changed != 1 {
            return Err(SqliteStoreError::InvalidGraphPublication);
        }
        Ok(())
    }

    fn create_graph_publication(
        &mut self,
        generation: GenerationId,
        prepared: &crate::sqlite::graph::PreparedRustGraphGeneration,
    ) -> Result<(), SqliteStoreError> {
        let coverage = prepared.resolution().coverage();
        let changed = self
            .connection
            .execute(
                "INSERT INTO generation_graph_publications(
                    generation_id, connected_workspace_id, lifecycle_state,
                    resolver_profile_version, input_digest, output_digest,
                    source_count, artifact_count, definition_count, site_count,
                    unresolved_count, unique_count, ambiguous_count,
                    unsupported_count, truncated_site_count,
                    retained_candidate_count, edge_count, input_text_bytes,
                    output_bytes, syntax_error_node_count, macro_site_count,
                    test_marker_site_count, heuristic_site_count
                 ) VALUES (
                    ?1, ?2, 'staging', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                    ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22
                 )",
                params![
                    generation.get(),
                    prepared.connected_workspace().as_bytes().as_slice(),
                    i64::from(prepared.resolution().profile_version()),
                    prepared.input_digest().as_slice(),
                    prepared.output_digest().as_slice(),
                    fixed_usize(prepared.sources().len())?,
                    fixed_usize(prepared.artifacts().len())?,
                    fixed_usize(prepared.definitions().len())?,
                    i64::from(coverage.sites()),
                    i64::from(coverage.unresolved()),
                    i64::from(coverage.unique()),
                    i64::from(coverage.ambiguous()),
                    i64::from(coverage.unsupported()),
                    i64::from(coverage.truncated_sites()),
                    fixed_integer(coverage.retained_candidates())?,
                    fixed_integer(prepared.edge_count())?,
                    fixed_integer(prepared.resolution().input_text_bytes())?,
                    fixed_integer(prepared.resolution().output_bytes())?,
                    fixed_integer(prepared.syntax_error_nodes())?,
                    fixed_integer(prepared.macro_sites())?,
                    fixed_integer(prepared.test_marker_sites())?,
                    fixed_integer(prepared.heuristic_sites())?,
                ],
            )
            .map_err(|_| SqliteStoreError::InvalidGraphPublication)?;
        if changed != 1 {
            return Err(SqliteStoreError::InvalidGraphPublication);
        }
        Ok(())
    }

    fn ensure_graph_artifact(
        &mut self,
        artifact: &crate::sqlite::graph::PreparedRustGraphArtifact,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        let inserted = {
            let transaction = self.transaction()?;
            let key = artifact.key();
            let changed = transaction
                .execute(
                    "INSERT OR IGNORE INTO analysis_artifacts(
                        artifact_digest, lifecycle_state, source_content_digest,
                        producer_manifest_digest, configuration_digest,
                        analysis_schema_digest, canonicalization_version,
                        fact_count, visited_nodes, syntax_error_nodes,
                        known_parser_limitation_nodes, payload_digest, language
                     ) VALUES (
                        ?1, 'staging', ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, 0, ?9,
                        'rust'
                     )",
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
                    ],
                )
                .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            if changed == 1 {
                transaction
                    .execute(
                        "INSERT INTO rust_graph_artifacts(
                            artifact_digest, site_profile_version, site_count,
                            max_observed_depth, owned_text_bytes
                         ) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            artifact.artifact_digest().as_bytes().as_slice(),
                            i64::from(repowitness_analysis::RUST_GRAPH_SITE_PROFILE_VERSION),
                            fixed_usize(artifact.analysis().sites().len())?,
                            i64::from(artifact.analysis().max_observed_depth()),
                            fixed_integer(artifact.analysis().owned_text_bytes())?,
                        ],
                    )
                    .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            }
            commit_mutation(transaction)?;
            changed == 1
        };
        if !inserted {
            return self.verify_graph_artifact(artifact, control);
        }
        let result = self.insert_graph_sites(artifact, control).and_then(|()| {
            let changed = self
                .connection
                .execute(
                    "UPDATE analysis_artifacts SET lifecycle_state = 'complete'
                     WHERE artifact_digest = ?1 AND lifecycle_state = 'staging'",
                    [artifact.artifact_digest().as_bytes().as_slice()],
                )
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            if changed == 1 {
                Ok(())
            } else {
                Err(SqliteStoreError::IntegrityCheckFailed)
            }
        });
        if result.is_err() {
            let _ = self.delete_staging_graph_artifact(artifact.artifact_digest());
        }
        result
    }

    fn insert_graph_sites(
        &mut self,
        artifact: &crate::sqlite::graph::PreparedRustGraphArtifact,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        for batch in artifact.analysis().sites().chunks(WRITE_BATCH_ROWS) {
            check_control(control)?;
            let transaction = self.transaction()?;
            for site in batch {
                let enclosing = site.enclosing_definition();
                transaction
                    .execute(
                        "INSERT INTO rust_graph_sites(
                            artifact_digest, ordinal, site_kind,
                            extraction_evidence, occurrence_start, occurrence_end,
                            target_start, target_end, raw_target, enclosing_kind,
                            enclosing_name, enclosing_qualified_name,
                            enclosing_name_start, enclosing_name_end,
                            enclosing_declaration_start, enclosing_declaration_end
                         ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                            ?12, ?13, ?14, ?15, ?16
                         )",
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
                            enclosing.map(|value| value.kind().as_str()),
                            enclosing.map(|value| value.name()),
                            enclosing.map(|value| value.qualified_name()),
                            enclosing
                                .map(|value| fixed_integer(value.name_span().start().get()))
                                .transpose()?,
                            enclosing
                                .map(|value| fixed_integer(value.name_span().end().get()))
                                .transpose()?,
                            enclosing
                                .map(|value| {
                                    fixed_integer(value.declaration_span().start().get())
                                })
                                .transpose()?,
                            enclosing
                                .map(|value| fixed_integer(value.declaration_span().end().get()))
                                .transpose()?,
                        ],
                    )
                    .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            }
            commit_mutation(transaction)?;
        }
        Ok(())
    }

    fn stage_graph_sources(
        &mut self,
        generation: GenerationId,
        prepared: &crate::sqlite::graph::PreparedRustGraphGeneration,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        for (batch_index, batch) in prepared.sources().chunks(WRITE_BATCH_ROWS).enumerate() {
            check_control(control)?;
            let transaction = self.transaction()?;
            for (offset, source) in batch.iter().enumerate() {
                let ordinal = batch_ordinal(batch_index, offset)?;
                transaction
                    .execute(
                        "INSERT INTO generation_graph_sources(
                            generation_id, ordinal, source_slot_id,
                            source_generation_id
                         ) VALUES (?1, ?2, ?3, ?4)",
                        params![
                            generation.get(),
                            fixed_usize(ordinal)?,
                            source.source_slot().as_bytes().as_slice(),
                            source.generation().get(),
                        ],
                    )
                    .map_err(|_| SqliteStoreError::InvalidGraphPublication)?;
            }
            commit_mutation(transaction)?;
        }
        Ok(())
    }

    fn stage_graph_artifacts(
        &mut self,
        generation: GenerationId,
        prepared: &crate::sqlite::graph::PreparedRustGraphGeneration,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        for batch in prepared.artifacts().chunks(WRITE_BATCH_ROWS) {
            check_control(control)?;
            let transaction = self.transaction()?;
            for artifact in batch {
                let source_generation = graph_source_generation(prepared, artifact.source_slot())?;
                transaction
                    .execute(
                        "INSERT INTO generation_graph_artifacts(
                            generation_id, source_slot_id, source_generation_id,
                            repository_path, graph_artifact_digest
                         ) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            generation.get(),
                            artifact.source_slot().as_bytes().as_slice(),
                            source_generation.get(),
                            artifact.path().as_bytes(),
                            artifact.artifact_digest().as_bytes().as_slice(),
                        ],
                    )
                    .map_err(|_| SqliteStoreError::InvalidGraphPublication)?;
            }
            commit_mutation(transaction)?;
        }
        Ok(())
    }

    fn stage_graph_definitions(
        &mut self,
        generation: GenerationId,
        prepared: &crate::sqlite::graph::PreparedRustGraphGeneration,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        for batch in prepared.definitions().chunks(WRITE_BATCH_ROWS) {
            check_control(control)?;
            let transaction = self.transaction()?;
            for definition in batch {
                let source_generation =
                    graph_source_generation(prepared, definition.source_slot())?;
                let fact = definition.fact();
                transaction
                    .execute(
                        "INSERT INTO generation_graph_definitions(
                            generation_id, source_slot_id, source_generation_id,
                            repository_path, artifact_digest, fact_ordinal,
                            symbol_kind, name_start, name_end, declaration_start,
                            declaration_end
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                        params![
                            generation.get(),
                            definition.source_slot().as_bytes().as_slice(),
                            source_generation.get(),
                            definition.path().as_bytes(),
                            definition.artifact().as_bytes().as_slice(),
                            fixed_integer(definition.fact_ordinal())?,
                            fact.kind().as_str(),
                            fixed_integer(fact.name_span().start().get())?,
                            fixed_integer(fact.name_span().end().get())?,
                            fixed_integer(fact.declaration_span().start().get())?,
                            fixed_integer(fact.declaration_span().end().get())?,
                        ],
                    )
                    .map_err(|_| SqliteStoreError::InvalidGraphPublication)?;
            }
            commit_mutation(transaction)?;
        }
        Ok(())
    }

    fn stage_graph_resolutions(
        &mut self,
        generation: GenerationId,
        prepared: &crate::sqlite::graph::PreparedRustGraphGeneration,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        for batch in prepared.resolution().outcomes().chunks(WRITE_BATCH_ROWS) {
            check_control(control)?;
            let transaction = self.transaction()?;
            for resolved in batch {
                let site = resolved.site();
                let source_generation = graph_source_generation(prepared, site.source_slot())?;
                let (outcome, reason) = graph_outcome_fields(resolved.outcome());
                transaction
                    .execute(
                        "INSERT INTO generation_graph_resolutions(
                            generation_id, source_slot_id, source_generation_id,
                            repository_path, site_artifact_digest, site_ordinal,
                            site_kind, occurrence_start, occurrence_end,
                            target_start, target_end, outcome_kind,
                            unresolved_reason, candidate_count,
                            candidates_truncated
                         ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                            ?12, ?13, ?14, ?15
                         )",
                        params![
                            generation.get(),
                            site.source_slot().as_bytes().as_slice(),
                            source_generation.get(),
                            site.path().as_bytes(),
                            site.artifact().as_bytes().as_slice(),
                            i64::from(site.ordinal().get()),
                            site.kind().as_str(),
                            fixed_integer(site.occurrence_span().start().get())?,
                            fixed_integer(site.occurrence_span().end().get())?,
                            fixed_integer(site.target_span().start().get())?,
                            fixed_integer(site.target_span().end().get())?,
                            outcome,
                            reason,
                            i64::from(resolved.candidate_count()),
                            i64::from(resolved.candidates_truncated()),
                        ],
                    )
                    .map_err(|_| SqliteStoreError::InvalidGraphPublication)?;
            }
            commit_mutation(transaction)?;
        }
        Ok(())
    }

    fn stage_graph_candidates(
        &mut self,
        generation: GenerationId,
        prepared: &crate::sqlite::graph::PreparedRustGraphGeneration,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        let mut pending = Vec::with_capacity(WRITE_BATCH_ROWS);
        for resolved in prepared.resolution().outcomes() {
            for (ordinal, candidate) in graph_candidates(resolved.outcome()).enumerate() {
                pending.push((resolved.site(), ordinal, candidate));
                if pending.len() == WRITE_BATCH_ROWS {
                    self.insert_graph_candidate_batch(generation, &pending, control)?;
                    pending.clear();
                }
            }
        }
        if !pending.is_empty() {
            self.insert_graph_candidate_batch(generation, &pending, control)?;
        }
        Ok(())
    }

    fn insert_graph_candidate_batch(
        &mut self,
        generation: GenerationId,
        batch: &[(
            &repowitness_analysis::RustGraphSiteIdentity,
            usize,
            &repowitness_analysis::RustGraphResolutionCandidate,
        )],
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        check_control(control)?;
        let transaction = self.transaction()?;
        for (site, ordinal, candidate) in batch {
            let target = candidate.target();
            transaction
                .execute(
                    "INSERT INTO generation_graph_candidates(
                        generation_id, site_source_slot_id,
                        site_repository_path, site_artifact_digest,
                        site_ordinal, candidate_ordinal, target_source_slot_id,
                        target_repository_path, target_artifact_digest,
                        target_fact_ordinal, target_kind, target_name_start,
                        target_name_end, target_declaration_start,
                        target_declaration_end, resolution_evidence
                     ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                        ?13, ?14, ?15, ?16
                     )",
                    params![
                        generation.get(),
                        site.source_slot().as_bytes().as_slice(),
                        site.path().as_bytes(),
                        site.artifact().as_bytes().as_slice(),
                        i64::from(site.ordinal().get()),
                        fixed_usize(*ordinal)?,
                        target.source_slot().as_bytes().as_slice(),
                        target.path().as_bytes(),
                        target.artifact().as_bytes().as_slice(),
                        fixed_integer(target.fact_ordinal())?,
                        target.kind().as_str(),
                        fixed_integer(target.name_span().start().get())?,
                        fixed_integer(target.name_span().end().get())?,
                        fixed_integer(target.declaration_span().start().get())?,
                        fixed_integer(target.declaration_span().end().get())?,
                        candidate.evidence().as_str(),
                    ],
                )
                .map_err(|_| SqliteStoreError::InvalidGraphPublication)?;
        }
        commit_mutation(transaction)
    }

    fn stage_graph_edges(
        &mut self,
        generation: GenerationId,
        prepared: &crate::sqlite::graph::PreparedRustGraphGeneration,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        let edges = prepared
            .resolution()
            .outcomes()
            .iter()
            .filter_map(|resolved| match resolved.outcome() {
                repowitness_analysis::RustGraphResolutionOutcome::Unique { candidate } => {
                    Some((resolved.site(), candidate))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for batch in edges.chunks(WRITE_BATCH_ROWS) {
            check_control(control)?;
            let transaction = self.transaction()?;
            for (site, candidate) in batch {
                if !matches!(
                    site.kind(),
                    repowitness_analysis::RustGraphSiteKind::Import
                        | repowitness_analysis::RustGraphSiteKind::Reference
                        | repowitness_analysis::RustGraphSiteKind::Call
                ) {
                    return Err(SqliteStoreError::InvalidGraphPublication);
                }
                transaction
                    .execute(
                        "INSERT INTO generation_graph_edges(
                            generation_id, site_source_slot_id,
                            site_repository_path, site_artifact_digest,
                            site_ordinal, candidate_ordinal, edge_kind,
                            resolution_evidence
                         ) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7)",
                        params![
                            generation.get(),
                            site.source_slot().as_bytes().as_slice(),
                            site.path().as_bytes(),
                            site.artifact().as_bytes().as_slice(),
                            i64::from(site.ordinal().get()),
                            site.kind().as_str(),
                            candidate.evidence().as_str(),
                        ],
                    )
                    .map_err(|_| SqliteStoreError::InvalidGraphPublication)?;
            }
            commit_mutation(transaction)?;
        }
        Ok(())
    }

}

fn graph_source_generation(
    prepared: &crate::sqlite::graph::PreparedRustGraphGeneration,
    source_slot: repowitness_domain::SourceSlotId,
) -> Result<GenerationId, SqliteStoreError> {
    prepared
        .sources()
        .binary_search_by_key(&source_slot, |source| source.source_slot())
        .ok()
        .map(|index| prepared.sources()[index].generation())
        .ok_or(SqliteStoreError::InvalidGraphPublication)
}

fn graph_outcome_fields(
    outcome: &repowitness_analysis::RustGraphResolutionOutcome,
) -> (&'static str, Option<&'static str>) {
    match outcome {
        repowitness_analysis::RustGraphResolutionOutcome::Unresolved { reason } => {
            ("unresolved", Some(reason.as_str()))
        }
        repowitness_analysis::RustGraphResolutionOutcome::Unique { .. } => ("unique", None),
        repowitness_analysis::RustGraphResolutionOutcome::Ambiguous { .. } => ("ambiguous", None),
    }
}

fn graph_candidates(
    outcome: &repowitness_analysis::RustGraphResolutionOutcome,
) -> Box<dyn Iterator<Item = &repowitness_analysis::RustGraphResolutionCandidate> + '_> {
    match outcome {
        repowitness_analysis::RustGraphResolutionOutcome::Unresolved { .. } => {
            Box::new(std::iter::empty())
        }
        repowitness_analysis::RustGraphResolutionOutcome::Unique { candidate } => {
            Box::new(std::iter::once(candidate))
        }
        repowitness_analysis::RustGraphResolutionOutcome::Ambiguous { candidates } => {
            Box::new(candidates.iter())
        }
    }
}

fn batch_ordinal(batch: usize, offset: usize) -> Result<usize, SqliteStoreError> {
    batch
        .checked_mul(WRITE_BATCH_ROWS)
        .and_then(|value| value.checked_add(offset))
        .ok_or(SqliteStoreError::CountNotRepresentable)
}
