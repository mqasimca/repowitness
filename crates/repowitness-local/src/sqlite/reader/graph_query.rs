enum GraphFailure {
    Sqlite(rusqlite::Error),
    Read(RustGraphReadError),
}

impl From<rusqlite::Error> for GraphFailure {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

fn execute_graph_command(
    connection: &mut Connection,
    command: &GraphCommand,
) -> Result<GraphCommandResult, RustGraphReadError> {
    graph_control(&command.cancelled, command.deadline)?;
    let progress_cancelled = Arc::clone(&command.cancelled);
    let deadline = command.deadline;
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| RustGraphReadError::Store)?;
    let result = graph_read_transaction(connection, command);
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| RustGraphReadError::Store)?;
    match result {
        Ok(result) => {
            graph_control(&command.cancelled, command.deadline)?;
            Ok(result)
        }
        Err(GraphFailure::Sqlite(error)) if is_interrupted(&error) => {
            graph_control(&command.cancelled, command.deadline)?;
            Err(RustGraphReadError::Store)
        }
        Err(GraphFailure::Sqlite(_)) => Err(RustGraphReadError::Store),
        Err(GraphFailure::Read(error)) => Err(error),
    }
}

fn graph_read_transaction(
    connection: &mut Connection,
    command: &GraphCommand,
) -> Result<GraphCommandResult, GraphFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    validate_pinned_graph_view(&transaction, &command.view, command.graph_generation)?;
    let availability =
        load_graph_availability(&transaction, &command.view, command.graph_generation)?;
    let result = match &command.operation {
        GraphOperation::Status => GraphCommandResult::Status(availability),
        GraphOperation::SymbolSearch { query } => {
            let publication = complete_graph_publication(availability)?;
            let limits = command.limits.ok_or_else(corrupt_graph)?;
            GraphCommandResult::SymbolSearch(search_graph_definitions(
                &transaction,
                publication,
                query,
                limits,
            )?)
        }
        GraphOperation::Evidence { site } => {
            let publication = complete_graph_publication(availability)?;
            let limits = command.limits.ok_or_else(corrupt_graph)?;
            GraphCommandResult::Evidence(Box::new(load_graph_evidence(
                &transaction,
                publication,
                site,
                limits,
            )?))
        }
        GraphOperation::Architecture => {
            let publication = complete_graph_publication(availability)?;
            let limits = command.limits.ok_or_else(corrupt_graph)?;
            GraphCommandResult::Architecture(summarize_graph_architecture(
                &transaction,
                publication,
                limits,
            )?)
        }
        GraphOperation::Trace {
            start,
            direction,
            edge_kinds,
        } => {
            let publication = complete_graph_publication(availability)?;
            let limits = command.limits.ok_or_else(corrupt_graph)?;
            GraphCommandResult::Trace(Box::new(load_and_trace_rust_graph(
                &transaction,
                publication,
                start,
                *direction,
                *edge_kinds,
                limits,
                &command.cancelled,
                command.deadline,
            )?))
        }
        GraphOperation::Impact { start, edge_kinds } => {
            let publication = complete_graph_publication(availability)?;
            let limits = command.limits.ok_or_else(corrupt_graph)?;
            GraphCommandResult::Impact(Box::new(load_and_analyze_rust_graph_impact(
                &transaction,
                publication,
                start,
                *edge_kinds,
                limits,
                &command.cancelled,
                command.deadline,
            )?))
        }
    };
    transaction.commit()?;
    Ok(result)
}

fn complete_graph_publication(
    availability: RustGraphAvailability,
) -> Result<RustGraphPublicationSummary, GraphFailure> {
    match availability {
        RustGraphAvailability::Complete(publication) => Ok(*publication),
        RustGraphAvailability::NotProduced { .. } => {
            Err(GraphFailure::Read(RustGraphReadError::GraphNotProduced))
        }
    }
}

fn search_graph_definitions(
    transaction: &Transaction<'_>,
    publication: RustGraphPublicationSummary,
    query: &str,
    limits: RustGraphReadLimits,
) -> Result<RustGraphSymbolSearchResult, GraphFailure> {
    let total_matches: i64 = transaction.query_row(
        "SELECT count(*)
         FROM generation_graph_definitions AS definition
         JOIN generation_files AS file
           ON file.generation_id = definition.source_generation_id
          AND file.repository_path = definition.repository_path
          AND file.artifact_digest = definition.artifact_digest
         JOIN artifact_facts AS fact
           ON fact.artifact_digest = definition.artifact_digest
          AND fact.ordinal = definition.fact_ordinal
          AND fact.kind = definition.symbol_kind
          AND fact.name_start = definition.name_start
          AND fact.name_end = definition.name_end
          AND fact.declaration_start = definition.declaration_start
          AND fact.declaration_end = definition.declaration_end
         WHERE definition.generation_id = ?1
           AND (fact.name = ?2 OR fact.qualified_name = ?2)",
        params![publication.generation().get(), query],
        |row| row.get(0),
    )?;
    let total_matches = persisted_u64(total_matches)?;
    let row_limit = i64::from(limits.max_results());
    let mut statement = transaction.prepare(
        "SELECT definition.source_slot_id, definition.source_generation_id,
                definition.repository_path, file.content_digest,
                definition.artifact_digest, definition.fact_ordinal,
                fact.kind, fact.name, fact.qualified_name, fact.name_start,
                fact.name_end, fact.declaration_start, fact.declaration_end
         FROM generation_graph_definitions AS definition
         JOIN generation_files AS file
           ON file.generation_id = definition.source_generation_id
          AND file.repository_path = definition.repository_path
          AND file.artifact_digest = definition.artifact_digest
         JOIN artifact_facts AS fact
           ON fact.artifact_digest = definition.artifact_digest
          AND fact.ordinal = definition.fact_ordinal
          AND fact.kind = definition.symbol_kind
          AND fact.name_start = definition.name_start
          AND fact.name_end = definition.name_end
          AND fact.declaration_start = definition.declaration_start
          AND fact.declaration_end = definition.declaration_end
         WHERE definition.generation_id = ?1
           AND (fact.name = ?2 OR fact.qualified_name = ?2)
         ORDER BY definition.source_slot_id, definition.repository_path,
                  definition.artifact_digest, definition.fact_ordinal
         LIMIT ?3",
    )?;
    let mut rows = statement.query(params![publication.generation().get(), query, row_limit])?;
    let mut definitions =
        Vec::with_capacity(usize::try_from(limits.max_results()).map_err(|_| corrupt_graph())?);
    let mut output_bytes = 128_u64;
    while let Some(row) = rows.next()? {
        let definition = decode_graph_definition(row, 0)?;
        output_bytes =
            graph_definition_output_bytes(output_bytes, &definition, limits.max_output_bytes())?;
        definitions.push(definition);
    }
    Ok(RustGraphSymbolSearchResult {
        publication,
        definitions: definitions.into_boxed_slice(),
        total_matches,
        output_bytes,
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "one exact evidence query and its categorical decode stay adjacent"
)]
fn load_graph_evidence(
    transaction: &Transaction<'_>,
    publication: RustGraphPublicationSummary,
    site: &RustGraphSiteSelector,
    limits: RustGraphReadLimits,
) -> Result<Option<RustGraphEvidenceResult>, GraphFailure> {
    let occurrence_start = persisted_selector_offset(site.occurrence_span().start().get())?;
    let occurrence_end = persisted_selector_offset(site.occurrence_span().end().get())?;
    let target_start = persisted_selector_offset(site.target_span().start().get())?;
    let target_end = persisted_selector_offset(site.target_span().end().get())?;
    let raw: Option<PersistedGraphEvidence> = transaction
        .query_row(
            "SELECT file.content_digest, raw_site.extraction_evidence,
                    resolution.outcome_kind, resolution.unresolved_reason,
                    resolution.candidate_count, resolution.candidates_truncated
             FROM generation_graph_resolutions AS resolution
             JOIN rust_graph_sites AS raw_site
               ON raw_site.artifact_digest = resolution.site_artifact_digest
              AND raw_site.ordinal = resolution.site_ordinal
              AND raw_site.site_kind = resolution.site_kind
              AND raw_site.occurrence_start = resolution.occurrence_start
              AND raw_site.occurrence_end = resolution.occurrence_end
              AND raw_site.target_start = resolution.target_start
              AND raw_site.target_end = resolution.target_end
             JOIN generation_graph_artifacts AS occurrence
               ON occurrence.generation_id = resolution.generation_id
              AND occurrence.source_slot_id = resolution.source_slot_id
              AND occurrence.source_generation_id = resolution.source_generation_id
              AND occurrence.repository_path = resolution.repository_path
              AND occurrence.graph_artifact_digest = resolution.site_artifact_digest
             JOIN analysis_artifacts AS graph_artifact
               ON graph_artifact.artifact_digest = occurrence.graph_artifact_digest
              AND graph_artifact.lifecycle_state = 'complete'
             JOIN generation_files AS file
               ON file.generation_id = occurrence.source_generation_id
              AND file.repository_path = occurrence.repository_path
              AND file.content_digest = graph_artifact.source_content_digest
             WHERE resolution.generation_id = ?1
               AND resolution.source_slot_id = ?2
               AND resolution.repository_path = ?3
               AND resolution.site_artifact_digest = ?4
               AND resolution.site_ordinal = ?5
               AND resolution.site_kind = ?6
               AND resolution.occurrence_start = ?7
               AND resolution.occurrence_end = ?8
               AND resolution.target_start = ?9
               AND resolution.target_end = ?10",
            params![
                publication.generation().get(),
                site.source_slot().as_bytes().as_slice(),
                site.path().as_bytes(),
                site.artifact().as_bytes().as_slice(),
                i64::from(site.ordinal()),
                site.kind().as_str(),
                occurrence_start,
                occurrence_end,
                target_start,
                target_end,
            ],
            PersistedGraphEvidence::from_row,
        )
        .optional()?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let content_digest =
        SourceContentDigest::try_from_slice(&raw.content_digest).map_err(|_| corrupt_graph())?;
    let extraction_evidence = RustGraphSiteEvidence::from_stable_str(&raw.extraction_evidence)
        .ok_or_else(corrupt_graph)?;
    let candidate_count = u32::try_from(raw.candidate_count).map_err(|_| corrupt_graph())?;
    let candidates_truncated = match raw.candidates_truncated {
        0 => false,
        1 => true,
        _ => return Err(corrupt_graph()),
    };
    let candidates = load_graph_candidates(transaction, publication.generation(), site, limits)?;
    let outcome = decode_graph_outcome(
        &raw.outcome_kind,
        raw.unresolved_reason.as_deref(),
        candidate_count,
        candidates_truncated,
        candidates,
    )?;
    let persisted_site = RustGraphSiteSelector::new(
        site.source_slot(),
        site.path().clone(),
        site.artifact(),
        site.ordinal(),
        site.kind(),
        site.occurrence_span(),
        site.target_span(),
    );
    Ok(Some(RustGraphEvidenceResult {
        publication,
        site: persisted_site,
        content_digest,
        extraction_evidence,
        outcome,
        candidate_count,
        candidates_truncated,
    }))
}

fn load_graph_candidates(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    site: &RustGraphSiteSelector,
    limits: RustGraphReadLimits,
) -> Result<Vec<RustGraphCandidateRecord>, GraphFailure> {
    let sql_limit = i64::from(limits.max_results()) + 1;
    let mut statement = transaction.prepare(
        "SELECT candidate.candidate_ordinal, candidate.resolution_evidence,
                definition.source_slot_id, definition.source_generation_id,
                definition.repository_path, file.content_digest,
                definition.artifact_digest, definition.fact_ordinal,
                fact.kind, fact.name, fact.qualified_name, fact.name_start,
                fact.name_end, fact.declaration_start, fact.declaration_end
         FROM generation_graph_candidates AS candidate
         JOIN generation_graph_definitions AS definition
           ON definition.generation_id = candidate.generation_id
          AND definition.source_slot_id = candidate.target_source_slot_id
          AND definition.repository_path = candidate.target_repository_path
          AND definition.artifact_digest = candidate.target_artifact_digest
          AND definition.fact_ordinal = candidate.target_fact_ordinal
          AND definition.symbol_kind = candidate.target_kind
          AND definition.name_start = candidate.target_name_start
          AND definition.name_end = candidate.target_name_end
          AND definition.declaration_start = candidate.target_declaration_start
          AND definition.declaration_end = candidate.target_declaration_end
         JOIN generation_files AS file
           ON file.generation_id = definition.source_generation_id
          AND file.repository_path = definition.repository_path
          AND file.artifact_digest = definition.artifact_digest
         JOIN artifact_facts AS fact
           ON fact.artifact_digest = definition.artifact_digest
          AND fact.ordinal = definition.fact_ordinal
          AND fact.kind = definition.symbol_kind
          AND fact.name_start = definition.name_start
          AND fact.name_end = definition.name_end
          AND fact.declaration_start = definition.declaration_start
          AND fact.declaration_end = definition.declaration_end
         WHERE candidate.generation_id = ?1
           AND candidate.site_source_slot_id = ?2
           AND candidate.site_repository_path = ?3
           AND candidate.site_artifact_digest = ?4
           AND candidate.site_ordinal = ?5
         ORDER BY candidate.candidate_ordinal
         LIMIT ?6",
    )?;
    let mut rows = statement.query(params![
        generation.get(),
        site.source_slot().as_bytes().as_slice(),
        site.path().as_bytes(),
        site.artifact().as_bytes().as_slice(),
        i64::from(site.ordinal()),
        sql_limit,
    ])?;
    let mut candidates = Vec::new();
    while let Some(row) = rows.next()? {
        if candidates.len() >= usize::try_from(limits.max_results()).map_err(|_| corrupt_graph())? {
            return Err(output_limit());
        }
        let ordinal: i64 = row.get(0)?;
        if ordinal != i64::try_from(candidates.len()).map_err(|_| corrupt_graph())? {
            return Err(corrupt_graph());
        }
        candidates.push(decode_graph_candidate(row, 1, 2)?);
    }
    let _output_bytes = graph_evidence_output_bytes(&candidates, limits.max_output_bytes())?;
    Ok(candidates)
}

fn decode_graph_outcome(
    outcome_kind: &str,
    unresolved_reason: Option<&str>,
    candidate_count: u32,
    candidates_truncated: bool,
    mut candidates: Vec<RustGraphCandidateRecord>,
) -> Result<RustGraphOutcomeRecord, GraphFailure> {
    match outcome_kind {
        "unresolved"
            if candidate_count == 0
                && !candidates_truncated
                && candidates.is_empty()
                && unresolved_reason
                    .and_then(parse_unresolved_reason)
                    .is_some() =>
        {
            Ok(RustGraphOutcomeRecord::Unresolved(
                unresolved_reason
                    .and_then(parse_unresolved_reason)
                    .ok_or_else(corrupt_graph)?,
            ))
        }
        "unique"
            if candidate_count == 1
                && !candidates_truncated
                && candidates.len() == 1
                && unresolved_reason.is_none() =>
        {
            Ok(RustGraphOutcomeRecord::Unique(Box::new(
                candidates.remove(0),
            )))
        }
        "ambiguous"
            if candidate_count >= 2
                && candidates.len() >= 2
                && u32::try_from(candidates.len()).is_ok_and(|count| count <= candidate_count)
                && candidates_truncated
                    == (u32::try_from(candidates.len()).unwrap_or(u32::MAX) < candidate_count)
                && unresolved_reason.is_none() =>
        {
            Ok(RustGraphOutcomeRecord::Ambiguous(
                candidates.into_boxed_slice(),
            ))
        }
        _ => Err(corrupt_graph()),
    }
}

fn summarize_graph_architecture(
    transaction: &Transaction<'_>,
    publication: RustGraphPublicationSummary,
    limits: RustGraphReadLimits,
) -> Result<RustGraphArchitectureSummary, GraphFailure> {
    let mut definitions = BTreeMap::new();
    let mut statement = transaction.prepare(
        "SELECT symbol_kind, count(*)
         FROM generation_graph_definitions
         WHERE generation_id = ?1
         GROUP BY symbol_kind
         ORDER BY symbol_kind",
    )?;
    let mut rows = statement.query([publication.generation().get()])?;
    while let Some(row) = rows.next()? {
        let kind: String = row.get(0)?;
        let count = persisted_u64(row.get(1)?)?;
        let kind = parse_rust_graph_symbol_kind(&kind).ok_or_else(corrupt_graph)?;
        if definitions.insert(kind, count).is_some() {
            return Err(corrupt_graph());
        }
    }
    let mut edges = BTreeMap::new();
    let mut statement = transaction.prepare(
        "SELECT edge_kind, count(*)
         FROM generation_graph_edges
         WHERE generation_id = ?1
         GROUP BY edge_kind
         ORDER BY edge_kind",
    )?;
    let mut rows = statement.query([publication.generation().get()])?;
    while let Some(row) = rows.next()? {
        let kind: String = row.get(0)?;
        let count = persisted_u64(row.get(1)?)?;
        let kind = RustGraphEdgeKind::from_stable_str(&kind).ok_or_else(corrupt_graph)?;
        if edges.insert(kind, count).is_some() {
            return Err(corrupt_graph());
        }
    }
    let definition_total = definitions
        .values()
        .try_fold(0_u64, |total, count| total.checked_add(*count))
        .ok_or_else(corrupt_graph)?;
    let edge_total = edges
        .values()
        .try_fold(0_u64, |total, count| total.checked_add(*count))
        .ok_or_else(corrupt_graph)?;
    if definition_total != publication.definition_count() || edge_total != publication.edge_count()
    {
        return Err(corrupt_graph());
    }
    let item_count = definitions
        .len()
        .checked_add(edges.len())
        .ok_or_else(corrupt_graph)?;
    if item_count > usize::try_from(limits.max_results()).map_err(|_| corrupt_graph())? {
        return Err(output_limit());
    }
    let output_bytes = u64::try_from(item_count)
        .ok()
        .and_then(|count| count.checked_mul(96))
        .and_then(|bytes| bytes.checked_add(128))
        .ok_or_else(output_limit)?;
    if output_bytes > limits.max_output_bytes() {
        return Err(output_limit());
    }
    Ok(RustGraphArchitectureSummary {
        publication,
        definitions_by_kind: definitions
            .into_iter()
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        edges_by_kind: edges.into_iter().collect::<Vec<_>>().into_boxed_slice(),
    })
}

fn persisted_selector_offset(value: u64) -> Result<i64, GraphFailure> {
    i64::try_from(value).map_err(|_| GraphFailure::Read(RustGraphReadError::InvalidSelector))
}

fn graph_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), RustGraphReadError> {
    if cancelled.load(Ordering::Acquire) {
        Err(RustGraphReadError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(RustGraphReadError::DeadlineExceeded)
    } else {
        Ok(())
    }
}
