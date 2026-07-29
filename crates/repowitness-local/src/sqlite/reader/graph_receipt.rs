fn validate_pinned_graph_view(
    transaction: &Transaction<'_>,
    view: &PinnedWorkspaceView,
    graph_generation: GenerationId,
) -> Result<(), GraphFailure> {
    let persisted: Option<(Vec<u8>, String)> = transaction
        .query_row(
            "SELECT connected_workspace_id, lifecycle_state
             FROM workspace_views
             WHERE workspace_view_id = ?1",
            [view.view().get()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((connected_workspace, lifecycle)) = persisted else {
        return Err(GraphFailure::Read(
            RustGraphReadError::GenerationUnavailable,
        ));
    };
    if lifecycle != "published"
        || connected_workspace.as_slice() != view.connected_workspace().as_bytes()
        || view.members().is_empty()
        || view.members().len() > crate::sqlite::MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS
    {
        return Err(corrupt_graph());
    }
    let mut statement = transaction.prepare(
        "SELECT member.ordinal, member.source_slot_id, member.source_epoch,
                slot.repository_identity, member.generation_id
         FROM workspace_view_members AS member
         JOIN workspace_source_slots AS slot
           ON slot.connected_workspace_id = member.connected_workspace_id
          AND slot.source_slot_id = member.source_slot_id
          AND slot.generation_workspace_id = member.generation_workspace_id
         WHERE member.workspace_view_id = ?1
         ORDER BY member.ordinal",
    )?;
    let mut rows = statement.query([view.view().get()])?;
    let mut found_graph_generation = false;
    for expected in view.members() {
        let Some(row) = rows.next()? else {
            return Err(corrupt_graph());
        };
        let ordinal: i64 = row.get(0)?;
        let source_slot: Vec<u8> = row.get(1)?;
        let source_epoch: i64 = row.get(2)?;
        let repository: Vec<u8> = row.get(3)?;
        let generation: i64 = row.get(4)?;
        if ordinal != i64::from(expected.ordinal())
            || source_slot.as_slice() != expected.source_slot().as_bytes()
            || source_epoch
                != i64::try_from(expected.source_epoch().get()).map_err(|_| corrupt_graph())?
            || repository.as_slice() != expected.repository().as_bytes()
            || generation != expected.generation().get()
        {
            return Err(corrupt_graph());
        }
        found_graph_generation |= expected.generation() == graph_generation;
    }
    if rows.next()?.is_some() {
        return Err(corrupt_graph());
    }
    if !found_graph_generation {
        return Err(GraphFailure::Read(
            RustGraphReadError::GenerationUnavailable,
        ));
    }
    Ok(())
}

fn load_graph_availability(
    transaction: &Transaction<'_>,
    view: &PinnedWorkspaceView,
    generation: GenerationId,
) -> Result<RustGraphAvailability, GraphFailure> {
    let required_profile: Option<i64> = transaction
        .query_row(
            "SELECT resolver_profile_version
             FROM generation_graph_requirements
             WHERE generation_id = ?1",
            [generation.get()],
            |row| row.get(0),
        )
        .optional()?;
    let publication = transaction
        .query_row(
            "SELECT lifecycle_state, connected_workspace_id,
                    resolver_profile_version, input_digest, output_digest,
                    source_count, artifact_count, definition_count, site_count,
                    unresolved_count, unique_count, ambiguous_count,
                    unsupported_count, truncated_site_count,
                    retained_candidate_count, edge_count, input_text_bytes,
                    output_bytes, syntax_error_node_count, macro_site_count,
                    test_marker_site_count, heuristic_site_count
             FROM generation_graph_publications
             WHERE generation_id = ?1",
            [generation.get()],
            RawGraphPublication::from_row,
        )
        .optional()?;
    let Some(required_profile) = required_profile else {
        if publication.is_some() {
            return Err(corrupt_graph());
        }
        return Ok(RustGraphAvailability::NotProduced { generation });
    };
    let Some(publication) = publication else {
        return Err(corrupt_graph());
    };
    let publication = publication.decode(generation, required_profile)?;
    if publication.connected_workspace() != view.connected_workspace() {
        return Err(corrupt_graph());
    }
    validate_graph_sources(transaction, view, generation)?;
    let counts = load_actual_graph_counts(transaction, generation)?;
    if !counts.matches(&publication) {
        return Err(corrupt_graph());
    }
    Ok(RustGraphAvailability::Complete(Box::new(publication)))
}

fn validate_graph_sources(
    transaction: &Transaction<'_>,
    view: &PinnedWorkspaceView,
    generation: GenerationId,
) -> Result<(), GraphFailure> {
    let mut statement = transaction.prepare(
        "SELECT ordinal, source_slot_id, source_generation_id
         FROM generation_graph_sources
         WHERE generation_id = ?1
         ORDER BY ordinal",
    )?;
    let mut rows = statement.query([generation.get()])?;
    let mut expected_ordinal = 0_i64;
    let mut prior_source_slot: Option<Vec<u8>> = None;
    let mut includes_selected_generation = false;
    while let Some(row) = rows.next()? {
        let ordinal: i64 = row.get(0)?;
        let source_slot: Vec<u8> = row.get(1)?;
        let source_generation: i64 = row.get(2)?;
        if ordinal != expected_ordinal
            || prior_source_slot
                .as_ref()
                .is_some_and(|prior| prior >= &source_slot)
        {
            return Err(corrupt_graph());
        }
        let belongs_to_pinned_view = view.members().iter().any(|member| {
            source_slot.as_slice() == member.source_slot().as_bytes()
                && source_generation == member.generation().get()
        });
        if !belongs_to_pinned_view {
            return Err(corrupt_graph());
        }
        includes_selected_generation |= source_generation == generation.get();
        prior_source_slot = Some(source_slot);
        expected_ordinal = expected_ordinal.checked_add(1).ok_or_else(corrupt_graph)?;
    }
    if expected_ordinal == 0 || !includes_selected_generation {
        return Err(corrupt_graph());
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "one count-and-integrity SQL statement validates the complete receipt atomically"
)]
fn load_actual_graph_counts(
    transaction: &Transaction<'_>,
    generation: GenerationId,
) -> Result<ActualGraphCounts, GraphFailure> {
    let mut statement = transaction.prepare(
            "SELECT
                (SELECT count(*) FROM generation_graph_sources
                 WHERE generation_id = ?1),
                (SELECT count(*) FROM generation_graph_artifacts
                 WHERE generation_id = ?1),
                (SELECT count(*) FROM generation_graph_definitions
                 WHERE generation_id = ?1),
                (SELECT count(*) FROM generation_graph_resolutions
                 WHERE generation_id = ?1),
                (SELECT count(*) FROM generation_graph_resolutions
                 WHERE generation_id = ?1 AND outcome_kind = 'unresolved'),
                (SELECT count(*) FROM generation_graph_resolutions
                 WHERE generation_id = ?1 AND outcome_kind = 'unique'),
                (SELECT count(*) FROM generation_graph_resolutions
                 WHERE generation_id = ?1 AND outcome_kind = 'ambiguous'),
                (SELECT count(*) FROM generation_graph_resolutions
                 WHERE generation_id = ?1 AND outcome_kind = 'unresolved'
                   AND unresolved_reason != 'no_candidate'),
                (SELECT count(*) FROM generation_graph_resolutions
                 WHERE generation_id = ?1 AND candidates_truncated = 1),
                (SELECT count(*) FROM generation_graph_candidates
                 WHERE generation_id = ?1),
                (SELECT count(*) FROM generation_graph_edges
                 WHERE generation_id = ?1),
                (SELECT coalesce(sum(base.syntax_error_nodes), 0)
                 FROM generation_graph_artifacts AS occurrence
                 JOIN analysis_artifacts AS base
                   ON base.artifact_digest = occurrence.graph_artifact_digest
                 WHERE occurrence.generation_id = ?1),
                (SELECT count(*) FROM generation_graph_artifacts AS occurrence
                 JOIN rust_graph_sites AS site
                   ON site.artifact_digest = occurrence.graph_artifact_digest
                 WHERE occurrence.generation_id = ?1
                   AND site.site_kind = 'macro_call'),
                (SELECT count(*) FROM generation_graph_artifacts AS occurrence
                 JOIN rust_graph_sites AS site
                   ON site.artifact_digest = occurrence.graph_artifact_digest
                 WHERE occurrence.generation_id = ?1
                   AND site.site_kind = 'test_marker'),
                (SELECT count(*) FROM generation_graph_artifacts AS occurrence
                 JOIN rust_graph_sites AS site
                   ON site.artifact_digest = occurrence.graph_artifact_digest
                 WHERE occurrence.generation_id = ?1
                   AND site.extraction_evidence = 'syntax_heuristic'),
                (SELECT count(*) FROM generation_graph_artifacts AS occurrence
                 LEFT JOIN analysis_artifacts AS base
                   ON base.artifact_digest = occurrence.graph_artifact_digest
                 LEFT JOIN rust_graph_artifacts AS graph
                   ON graph.artifact_digest = occurrence.graph_artifact_digest
                 WHERE occurrence.generation_id = ?1
                   AND (
                     base.artifact_digest IS NULL
                     OR base.lifecycle_state != 'complete'
                     OR base.language != 'rust'
                     OR base.fact_count != 0
                     OR graph.artifact_digest IS NULL
                     OR graph.site_profile_version != ?2
                     OR graph.site_count != (
                       SELECT count(*) FROM rust_graph_sites AS site
                       WHERE site.artifact_digest = occurrence.graph_artifact_digest
                     )
                   )),
                (SELECT count(*) FROM generation_graph_definitions AS definition
                 WHERE definition.generation_id = ?1
                   AND NOT EXISTS (
                     SELECT 1
                     FROM generation_files AS file
                     JOIN artifact_facts AS fact
                       ON fact.artifact_digest = file.artifact_digest
                      AND fact.ordinal = definition.fact_ordinal
                      AND fact.kind = definition.symbol_kind
                      AND fact.name_start = definition.name_start
                      AND fact.name_end = definition.name_end
                      AND fact.declaration_start = definition.declaration_start
                      AND fact.declaration_end = definition.declaration_end
                     WHERE file.generation_id = definition.source_generation_id
                       AND file.repository_path = definition.repository_path
                       AND file.artifact_digest = definition.artifact_digest
                   )),
                (SELECT count(*) FROM generation_graph_resolutions AS resolution
                 WHERE resolution.generation_id = ?1
                   AND (
                     (resolution.outcome_kind = 'unresolved' AND EXISTS (
                       SELECT 1 FROM generation_graph_candidates AS candidate
                       WHERE candidate.generation_id = resolution.generation_id
                         AND candidate.site_source_slot_id = resolution.source_slot_id
                         AND candidate.site_repository_path = resolution.repository_path
                         AND candidate.site_artifact_digest = resolution.site_artifact_digest
                         AND candidate.site_ordinal = resolution.site_ordinal
                     ))
                     OR (resolution.outcome_kind = 'unique' AND 1 != (
                       SELECT count(*) FROM generation_graph_candidates AS candidate
                       WHERE candidate.generation_id = resolution.generation_id
                         AND candidate.site_source_slot_id = resolution.source_slot_id
                         AND candidate.site_repository_path = resolution.repository_path
                         AND candidate.site_artifact_digest = resolution.site_artifact_digest
                         AND candidate.site_ordinal = resolution.site_ordinal
                     ))
                     OR (resolution.outcome_kind = 'ambiguous' AND (
                       (SELECT count(*) FROM generation_graph_candidates AS candidate
                        WHERE candidate.generation_id = resolution.generation_id
                          AND candidate.site_source_slot_id = resolution.source_slot_id
                          AND candidate.site_repository_path = resolution.repository_path
                          AND candidate.site_artifact_digest = resolution.site_artifact_digest
                          AND candidate.site_ordinal = resolution.site_ordinal) < 2
                       OR (SELECT count(*) FROM generation_graph_candidates AS candidate
                           WHERE candidate.generation_id = resolution.generation_id
                             AND candidate.site_source_slot_id = resolution.source_slot_id
                             AND candidate.site_repository_path = resolution.repository_path
                             AND candidate.site_artifact_digest = resolution.site_artifact_digest
                             AND candidate.site_ordinal = resolution.site_ordinal) >
                            resolution.candidate_count
                       OR resolution.candidates_truncated != (
                         (SELECT count(*) FROM generation_graph_candidates AS candidate
                          WHERE candidate.generation_id = resolution.generation_id
                            AND candidate.site_source_slot_id = resolution.source_slot_id
                            AND candidate.site_repository_path = resolution.repository_path
                            AND candidate.site_artifact_digest = resolution.site_artifact_digest
                            AND candidate.site_ordinal = resolution.site_ordinal)
                         < resolution.candidate_count
                       )
                     ))
                   )),
                (SELECT count(*) FROM generation_graph_candidates AS candidate
                 WHERE candidate.generation_id = ?1
                   AND candidate.candidate_ordinal != (
                     SELECT count(*) FROM generation_graph_candidates AS prior
                     WHERE prior.generation_id = candidate.generation_id
                       AND prior.site_source_slot_id = candidate.site_source_slot_id
                       AND prior.site_repository_path = candidate.site_repository_path
                       AND prior.site_artifact_digest = candidate.site_artifact_digest
                       AND prior.site_ordinal = candidate.site_ordinal
                       AND prior.candidate_ordinal < candidate.candidate_ordinal
                   )),
                (SELECT count(*) FROM generation_graph_edges AS edge
                 WHERE edge.generation_id = ?1
                   AND NOT EXISTS (
                     SELECT 1
                     FROM generation_graph_resolutions AS resolution
                     JOIN generation_graph_candidates AS candidate
                       ON candidate.generation_id = resolution.generation_id
                      AND candidate.site_source_slot_id = resolution.source_slot_id
                      AND candidate.site_repository_path = resolution.repository_path
                      AND candidate.site_artifact_digest = resolution.site_artifact_digest
                      AND candidate.site_ordinal = resolution.site_ordinal
                      AND candidate.candidate_ordinal = edge.candidate_ordinal
                     WHERE resolution.generation_id = edge.generation_id
                       AND resolution.source_slot_id = edge.site_source_slot_id
                       AND resolution.repository_path = edge.site_repository_path
                       AND resolution.site_artifact_digest = edge.site_artifact_digest
                       AND resolution.site_ordinal = edge.site_ordinal
                       AND resolution.outcome_kind = 'unique'
                       AND resolution.site_kind = edge.edge_kind
                       AND candidate.resolution_evidence = edge.resolution_evidence
                   ))",
        )?;
    let mut rows = statement.query(params![
        generation.get(),
        i64::from(repowitness_analysis::RUST_GRAPH_SITE_PROFILE_VERSION),
    ])?;
    let row = rows.next()?.ok_or_else(corrupt_graph)?;
    let counts = ActualGraphCounts::from_row(row)?;
    if rows.next()?.is_some() {
        return Err(corrupt_graph());
    }
    Ok(counts)
}
