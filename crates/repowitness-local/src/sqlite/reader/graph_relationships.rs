struct LoadedRustGraphRelationships {
    edges: Vec<repowitness_analysis::RustGraphTraversalEdge>,
    definitions:
        BTreeMap<repowitness_analysis::RustGraphDefinitionIdentity, RustGraphDefinitionRecord>,
    sites: BTreeMap<repowitness_analysis::RustGraphSiteIdentity, RustGraphSiteSelector>,
    coverage: repowitness_analysis::RustGraphTraceCoverage,
}

#[allow(
    clippy::too_many_lines,
    reason = "one exact source/site/target join is decoded and validated as one trust boundary"
)]
fn load_rust_graph_relationships(
    transaction: &Transaction<'_>,
    publication: &RustGraphPublicationSummary,
    limits: RustGraphReadLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<LoadedRustGraphRelationships, GraphFailure> {
    let sql_limit = i64::try_from(limits.max_input_edges())
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(corrupt_graph)?;
    let mut statement = transaction.prepare(
        "SELECT
            resolution.source_slot_id, resolution.source_generation_id,
            resolution.repository_path, resolution.site_artifact_digest,
            resolution.site_ordinal, resolution.site_kind,
            resolution.occurrence_start, resolution.occurrence_end,
            resolution.target_start, resolution.target_end,
            site.extraction_evidence, resolution.outcome_kind,
            resolution.candidate_count, resolution.candidates_truncated,
            candidate.candidate_ordinal, candidate.resolution_evidence,
            source_definition.source_slot_id,
            source_definition.source_generation_id,
            source_definition.repository_path, source_file.content_digest,
            source_definition.artifact_digest, source_definition.fact_ordinal,
            source_fact.kind, source_fact.name, source_fact.qualified_name,
            source_fact.name_start, source_fact.name_end,
            source_fact.declaration_start, source_fact.declaration_end,
            target_definition.source_slot_id,
            target_definition.source_generation_id,
            target_definition.repository_path, target_file.content_digest,
            target_definition.artifact_digest, target_definition.fact_ordinal,
            target_fact.kind, target_fact.name, target_fact.qualified_name,
            target_fact.name_start, target_fact.name_end,
            target_fact.declaration_start, target_fact.declaration_end
         FROM generation_graph_resolutions AS resolution
         JOIN rust_graph_sites AS site
           ON site.artifact_digest = resolution.site_artifact_digest
          AND site.ordinal = resolution.site_ordinal
          AND site.site_kind = resolution.site_kind
          AND site.occurrence_start = resolution.occurrence_start
          AND site.occurrence_end = resolution.occurrence_end
          AND site.target_start = resolution.target_start
          AND site.target_end = resolution.target_end
         JOIN generation_graph_candidates AS candidate
           ON candidate.generation_id = resolution.generation_id
          AND candidate.site_source_slot_id = resolution.source_slot_id
          AND candidate.site_repository_path = resolution.repository_path
          AND candidate.site_artifact_digest = resolution.site_artifact_digest
          AND candidate.site_ordinal = resolution.site_ordinal
         JOIN generation_graph_definitions AS source_definition
           ON source_definition.generation_id = resolution.generation_id
          AND source_definition.source_slot_id = resolution.source_slot_id
          AND source_definition.repository_path = resolution.repository_path
          AND source_definition.symbol_kind = site.enclosing_kind
          AND source_definition.name_start = site.enclosing_name_start
          AND source_definition.name_end = site.enclosing_name_end
          AND source_definition.declaration_start = site.enclosing_declaration_start
          AND source_definition.declaration_end = site.enclosing_declaration_end
         JOIN generation_files AS source_file
           ON source_file.generation_id = source_definition.source_generation_id
          AND source_file.repository_path = source_definition.repository_path
          AND source_file.artifact_digest = source_definition.artifact_digest
         JOIN artifact_facts AS source_fact
           ON source_fact.artifact_digest = source_definition.artifact_digest
          AND source_fact.ordinal = source_definition.fact_ordinal
          AND source_fact.kind = source_definition.symbol_kind
          AND source_fact.name_start = source_definition.name_start
          AND source_fact.name_end = source_definition.name_end
          AND source_fact.declaration_start = source_definition.declaration_start
          AND source_fact.declaration_end = source_definition.declaration_end
         JOIN generation_graph_definitions AS target_definition
           ON target_definition.generation_id = candidate.generation_id
          AND target_definition.source_slot_id = candidate.target_source_slot_id
          AND target_definition.repository_path = candidate.target_repository_path
          AND target_definition.artifact_digest = candidate.target_artifact_digest
          AND target_definition.fact_ordinal = candidate.target_fact_ordinal
          AND target_definition.symbol_kind = candidate.target_kind
          AND target_definition.name_start = candidate.target_name_start
          AND target_definition.name_end = candidate.target_name_end
          AND target_definition.declaration_start = candidate.target_declaration_start
          AND target_definition.declaration_end = candidate.target_declaration_end
         JOIN generation_files AS target_file
           ON target_file.generation_id = target_definition.source_generation_id
          AND target_file.repository_path = target_definition.repository_path
          AND target_file.artifact_digest = target_definition.artifact_digest
         JOIN artifact_facts AS target_fact
           ON target_fact.artifact_digest = target_definition.artifact_digest
          AND target_fact.ordinal = target_definition.fact_ordinal
          AND target_fact.kind = target_definition.symbol_kind
          AND target_fact.name_start = target_definition.name_start
          AND target_fact.name_end = target_definition.name_end
          AND target_fact.declaration_start = target_definition.declaration_start
          AND target_fact.declaration_end = target_definition.declaration_end
         WHERE resolution.generation_id = ?1
           AND resolution.outcome_kind IN ('unique', 'ambiguous')
           AND resolution.site_kind IN ('import', 'reference', 'call')
         ORDER BY resolution.source_slot_id, resolution.repository_path,
                  resolution.site_artifact_digest, resolution.site_ordinal,
                  candidate.candidate_ordinal
         LIMIT ?2",
    )?;
    let mut rows = statement.query(params![publication.generation().get(), sql_limit])?;
    let capacity =
        usize::try_from(limits.max_input_edges().min(65_536)).map_err(|_| corrupt_graph())?;
    let mut decoded_relationships = Vec::with_capacity(capacity);
    let mut input_bytes = 0_u64;
    while let Some(row) = rows.next()? {
        graph_control(cancelled, deadline).map_err(GraphFailure::Read)?;
        if u64::try_from(decoded_relationships.len()).map_err(|_| corrupt_graph())?
            >= limits.max_input_edges()
        {
            return Err(graph_input_limit());
        }
        let decoded = decode_graph_relationship(row)?;
        input_bytes = input_bytes
            .checked_add(decoded.encoded_bytes)
            .ok_or_else(corrupt_graph)?;
        if input_bytes > limits.max_input_bytes() {
            return Err(graph_input_limit());
        }
        decoded_relationships.push(decoded);
    }
    validate_graph_relationship_groups(&mut decoded_relationships)?;
    let (edges, definitions, sites) = materialize_graph_relationships(decoded_relationships)?;
    let unlinked_sites =
        load_unlinked_graph_site_count(transaction, publication.generation(), cancelled, deadline)?;
    let coverage = repowitness_analysis::RustGraphTraceCoverage::new(
        publication.unresolved_count(),
        publication.unsupported_count(),
        publication.ambiguous_count(),
        publication.truncated_site_count(),
        unlinked_sites,
        publication.macro_sites(),
        publication.test_marker_sites(),
        publication.heuristic_sites(),
    );
    Ok(LoadedRustGraphRelationships {
        edges,
        definitions,
        sites,
        coverage,
    })
}

struct DecodedRustGraphRelationship {
    source_identity: repowitness_analysis::RustGraphDefinitionIdentity,
    site_identity: repowitness_analysis::RustGraphSiteIdentity,
    target_identity: repowitness_analysis::RustGraphDefinitionIdentity,
    source: RustGraphDefinitionRecord,
    site: RustGraphSiteSelector,
    target: RustGraphDefinitionRecord,
    extraction_evidence: repowitness_analysis::RustGraphSiteEvidence,
    resolution_evidence: repowitness_analysis::RustGraphResolutionEvidence,
    outcome_kind: String,
    candidate_count: u32,
    candidates_truncated: bool,
    cardinality: Option<repowitness_analysis::RustGraphRelationshipCardinality>,
    candidate_ordinal: u32,
    encoded_bytes: u64,
}

fn decode_graph_relationship(
    row: &rusqlite::Row<'_>,
) -> Result<DecodedRustGraphRelationship, GraphFailure> {
    let source_slot: Vec<u8> = row.get(0)?;
    let site_source_generation: i64 = row.get(1)?;
    let site_path: Vec<u8> = row.get(2)?;
    let site_artifact: Vec<u8> = row.get(3)?;
    let site_ordinal: i64 = row.get(4)?;
    let site_kind: String = row.get(5)?;
    let occurrence_span = persisted_graph_span(row.get(6)?, row.get(7)?)?;
    let target_span = persisted_graph_span(row.get(8)?, row.get(9)?)?;
    let extraction_evidence: String = row.get(10)?;
    let outcome_kind: String = row.get(11)?;
    let candidate_count: i64 = row.get(12)?;
    let candidates_truncated: i64 = row.get(13)?;
    let candidate_ordinal: i64 = row.get(14)?;
    let resolution_evidence: String = row.get(15)?;
    let source_slot = repowitness_domain::SourceSlotId::try_from_slice(&source_slot)
        .map_err(|_| corrupt_graph())?;
    let site_path = RepositoryPath::try_from_vec(site_path, PERSISTED_PATH_LIMITS)
        .map_err(|_| corrupt_graph())?;
    let site_artifact =
        AnalysisArtifactDigest::try_from_slice(&site_artifact).map_err(|_| corrupt_graph())?;
    let site_ordinal = u32::try_from(site_ordinal).map_err(|_| corrupt_graph())?;
    let site_kind = repowitness_analysis::RustGraphSiteKind::from_stable_str(&site_kind)
        .ok_or_else(corrupt_graph)?;
    let extraction_evidence =
        repowitness_analysis::RustGraphSiteEvidence::from_stable_str(&extraction_evidence)
            .ok_or_else(corrupt_graph)?;
    let resolution_evidence =
        parse_resolution_evidence(&resolution_evidence).ok_or_else(corrupt_graph)?;
    let candidate_count = u32::try_from(candidate_count).map_err(|_| corrupt_graph())?;
    let candidate_ordinal = u32::try_from(candidate_ordinal).map_err(|_| corrupt_graph())?;
    let truncated = match candidates_truncated {
        0 => false,
        1 => true,
        _ => return Err(corrupt_graph()),
    };
    if !matches!(outcome_kind.as_str(), "unique" | "ambiguous") {
        return Err(corrupt_graph());
    }
    let source = decode_graph_definition(row, 16)?;
    let target = decode_graph_definition(row, 29)?;
    if source.source_slot() != source_slot
        || source.source_generation().get() != site_source_generation
        || source.path() != &site_path
    {
        return Err(corrupt_graph());
    }
    let site = RustGraphSiteSelector::new(
        source_slot,
        site_path,
        site_artifact,
        site_ordinal,
        site_kind,
        occurrence_span,
        target_span,
    );
    let source_identity = source.identity().ok_or_else(corrupt_graph)?;
    let site_identity = site.identity().ok_or_else(corrupt_graph)?;
    let target_identity = target.identity().ok_or_else(corrupt_graph)?;
    let encoded_bytes = rich_relationship_input_bytes(&source, &site, &target)?;
    Ok(DecodedRustGraphRelationship {
        source_identity,
        site_identity,
        target_identity,
        source,
        site,
        target,
        extraction_evidence,
        resolution_evidence,
        outcome_kind,
        candidate_count,
        candidates_truncated: truncated,
        cardinality: None,
        candidate_ordinal,
        encoded_bytes,
    })
}

fn validate_graph_relationship_groups(
    decoded: &mut [DecodedRustGraphRelationship],
) -> Result<(), GraphFailure> {
    let mut start = 0;
    while start < decoded.len() {
        let mut end = start + 1;
        while end < decoded.len() && decoded[end].site_identity == decoded[start].site_identity {
            end += 1;
        }
        let retained = u32::try_from(end - start).map_err(|_| corrupt_graph())?;
        let first_source = decoded[start].source_identity.clone();
        let first_site = decoded[start].site.clone();
        let first_evidence = decoded[start].extraction_evidence;
        let first_outcome = decoded[start].outcome_kind.clone();
        let candidate_count = decoded[start].candidate_count;
        let candidates_truncated = decoded[start].candidates_truncated;
        let cardinality = match first_outcome.as_str() {
            "unique" if candidate_count == 1 && retained == 1 && !candidates_truncated => {
                repowitness_analysis::RustGraphRelationshipCardinality::Unique
            }
            "ambiguous" => repowitness_analysis::RustGraphRelationshipCardinality::try_ambiguous(
                candidate_count,
                retained,
                candidates_truncated,
            )
            .map_err(map_graph_analysis_error)?,
            _ => return Err(corrupt_graph()),
        };
        for (offset, relationship) in decoded[start..end].iter_mut().enumerate() {
            if relationship.candidate_ordinal
                != u32::try_from(offset).map_err(|_| corrupt_graph())?
                || relationship.source_identity != first_source
                || relationship.site != first_site
                || relationship.extraction_evidence != first_evidence
                || relationship.outcome_kind != first_outcome
                || relationship.candidate_count != candidate_count
                || relationship.candidates_truncated != candidates_truncated
            {
                return Err(corrupt_graph());
            }
            relationship.cardinality = Some(cardinality);
        }
        start = end;
    }
    Ok(())
}

type MaterializedRustGraphRelationships = (
    Vec<repowitness_analysis::RustGraphTraversalEdge>,
    BTreeMap<repowitness_analysis::RustGraphDefinitionIdentity, RustGraphDefinitionRecord>,
    BTreeMap<repowitness_analysis::RustGraphSiteIdentity, RustGraphSiteSelector>,
);

fn materialize_graph_relationships(
    decoded: Vec<DecodedRustGraphRelationship>,
) -> Result<MaterializedRustGraphRelationships, GraphFailure> {
    let mut edges = Vec::with_capacity(decoded.len());
    let mut definitions = BTreeMap::new();
    let mut sites = BTreeMap::new();
    for relationship in decoded {
        insert_graph_definition(
            &mut definitions,
            &relationship.source_identity,
            &relationship.source,
        )?;
        insert_graph_definition(
            &mut definitions,
            &relationship.target_identity,
            &relationship.target,
        )?;
        if let Some(existing) = sites.insert(
            relationship.site_identity.clone(),
            relationship.site.clone(),
        ) && existing != relationship.site
        {
            return Err(corrupt_graph());
        }
        edges.push(
            repowitness_analysis::RustGraphTraversalEdge::try_new(
                relationship.source_identity,
                relationship.site_identity,
                relationship.target_identity,
                relationship.extraction_evidence,
                relationship.resolution_evidence,
                relationship.cardinality.ok_or_else(corrupt_graph)?,
            )
            .map_err(map_graph_analysis_error)?,
        );
    }
    Ok((edges, definitions, sites))
}

fn insert_graph_definition(
    definitions: &mut BTreeMap<
        repowitness_analysis::RustGraphDefinitionIdentity,
        RustGraphDefinitionRecord,
    >,
    identity: &repowitness_analysis::RustGraphDefinitionIdentity,
    definition: &RustGraphDefinitionRecord,
) -> Result<(), GraphFailure> {
    if let Some(existing) = definitions.insert(identity.clone(), definition.clone())
        && existing != *definition
    {
        return Err(corrupt_graph());
    }
    Ok(())
}

fn rich_relationship_input_bytes(
    source: &RustGraphDefinitionRecord,
    site: &RustGraphSiteSelector,
    target: &RustGraphDefinitionRecord,
) -> Result<u64, GraphFailure> {
    [
        source.path().byte_count().get(),
        u64::try_from(source.name().len()).map_err(|_| corrupt_graph())?,
        u64::try_from(source.qualified_name().len()).map_err(|_| corrupt_graph())?,
        site.path().byte_count().get(),
        target.path().byte_count().get(),
        u64::try_from(target.name().len()).map_err(|_| corrupt_graph())?,
        u64::try_from(target.qualified_name().len()).map_err(|_| corrupt_graph())?,
    ]
    .into_iter()
    .try_fold(384_u64, |total, value| total.checked_add(value))
    .ok_or_else(corrupt_graph)
}

fn load_unlinked_graph_site_count(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<u64, GraphFailure> {
    graph_control(cancelled, deadline).map_err(GraphFailure::Read)?;
    let count: i64 = transaction.query_row(
        "SELECT count(*)
         FROM generation_graph_resolutions AS resolution
         JOIN rust_graph_sites AS site
           ON site.artifact_digest = resolution.site_artifact_digest
          AND site.ordinal = resolution.site_ordinal
          AND site.site_kind = resolution.site_kind
          AND site.occurrence_start = resolution.occurrence_start
          AND site.occurrence_end = resolution.occurrence_end
          AND site.target_start = resolution.target_start
          AND site.target_end = resolution.target_end
         WHERE resolution.generation_id = ?1
           AND resolution.outcome_kind IN ('unique', 'ambiguous')
           AND resolution.site_kind IN ('import', 'reference', 'call')
           AND NOT EXISTS (
               SELECT 1
               FROM generation_graph_definitions AS definition
               JOIN generation_files AS file
                 ON file.generation_id = definition.source_generation_id
                AND file.repository_path = definition.repository_path
                AND file.artifact_digest = definition.artifact_digest
               WHERE definition.generation_id = resolution.generation_id
                 AND definition.source_slot_id = resolution.source_slot_id
                 AND definition.repository_path = resolution.repository_path
                 AND definition.symbol_kind = site.enclosing_kind
                 AND definition.name_start = site.enclosing_name_start
                 AND definition.name_end = site.enclosing_name_end
                 AND definition.declaration_start = site.enclosing_declaration_start
                 AND definition.declaration_end = site.enclosing_declaration_end
           )",
        [generation.get()],
        |row| row.get(0),
    )?;
    persisted_u64(count)
}

fn graph_input_limit() -> GraphFailure {
    GraphFailure::Read(RustGraphReadError::InputLimitExceeded)
}
