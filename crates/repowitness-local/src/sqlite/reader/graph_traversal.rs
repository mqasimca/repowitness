#[allow(
    clippy::too_many_arguments,
    reason = "one internal boundary keeps projection, semantics, limits, and control explicit"
)]
fn load_and_trace_rust_graph(
    transaction: &Transaction<'_>,
    publication: RustGraphPublicationSummary,
    start: &RustGraphTraceStart,
    direction: RustGraphDirection,
    edge_kinds: RustGraphEdgeKinds,
    limits: RustGraphReadLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<RustGraphTraceResult, GraphFailure> {
    let mut loaded =
        load_rust_graph_relationships(transaction, &publication, limits, cancelled, deadline)?;
    let analysis_start = match start {
        RustGraphTraceStart::Definition(requested) => {
            let persisted =
                load_exact_graph_definition(transaction, publication.generation(), requested)?
                    .ok_or_else(graph_start_unavailable)?;
            if persisted != *requested {
                return Err(graph_start_unavailable());
            }
            let identity = persisted.identity().ok_or_else(corrupt_graph)?;
            insert_graph_definition(&mut loaded.definitions, &identity, &persisted)?;
            repowitness_analysis::RustGraphTraceStart::Definition(identity)
        }
        RustGraphTraceStart::Site(requested) => {
            let identity = requested.identity().ok_or_else(corrupt_graph)?;
            if loaded.sites.get(&identity) != Some(requested) {
                return Err(graph_start_unavailable());
            }
            repowitness_analysis::RustGraphTraceStart::Site(identity)
        }
    };
    let analysis_limits = analysis_graph_limits(limits)?;
    let analysis_direction = match direction {
        RustGraphDirection::Outbound => repowitness_analysis::RustGraphTraceDirection::Outbound,
        RustGraphDirection::Inbound => repowitness_analysis::RustGraphTraceDirection::Inbound,
    };
    let control = repowitness_analysis::RustGraphTraceControl::new(cancelled, deadline);
    let request = repowitness_analysis::RustGraphTraceRequest::new(
        &loaded.edges,
        analysis_start,
        analysis_direction,
        edge_kinds,
        analysis_limits,
        loaded.coverage,
        control,
    );
    let traced =
        repowitness_analysis::trace_rust_graph(request).map_err(map_graph_analysis_error)?;
    convert_graph_trace_result(publication, &loaded, &traced, limits)
}

#[allow(
    clippy::too_many_arguments,
    reason = "one internal boundary keeps projection, semantics, limits, and control explicit"
)]
fn load_and_analyze_rust_graph_impact(
    transaction: &Transaction<'_>,
    publication: RustGraphPublicationSummary,
    start: &RustGraphDefinitionRecord,
    edge_kinds: RustGraphEdgeKinds,
    limits: RustGraphReadLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<RustGraphImpactResult, GraphFailure> {
    let persisted = load_exact_graph_definition(transaction, publication.generation(), start)?
        .ok_or_else(graph_start_unavailable)?;
    if persisted != *start {
        return Err(graph_start_unavailable());
    }
    let start_identity = persisted.identity().ok_or_else(corrupt_graph)?;
    let mut loaded =
        load_rust_graph_relationships(transaction, &publication, limits, cancelled, deadline)?;
    insert_graph_definition(&mut loaded.definitions, &start_identity, &persisted)?;
    let analysis_limits = analysis_graph_limits(limits)?;
    let control = repowitness_analysis::RustGraphTraceControl::new(cancelled, deadline);
    let request = repowitness_analysis::RustGraphImpactRequest::new(
        &loaded.edges,
        start_identity,
        edge_kinds,
        analysis_limits,
        loaded.coverage,
        control,
    );
    let analyzed = repowitness_analysis::analyze_rust_graph_impact(request)
        .map_err(map_graph_analysis_error)?;
    let trace = convert_graph_trace_result(publication, &loaded, analyzed.trace(), limits)?;
    let mut output_bytes = trace
        .output_bytes()
        .checked_add(64)
        .ok_or_else(output_limit)?;
    let mut impacted = Vec::with_capacity(analyzed.impacted().len());
    for impact in analyzed.impacted() {
        graph_control(cancelled, deadline).map_err(GraphFailure::Read)?;
        let definition = loaded
            .definitions
            .get(impact.definition())
            .ok_or_else(corrupt_graph)?
            .clone();
        output_bytes =
            graph_definition_output_bytes(output_bytes, &definition, limits.max_output_bytes())?;
        impacted.push(crate::sqlite::RustGraphImpactedDefinition {
            class: local_graph_impact_class(impact.class()),
            definition,
            minimum_depth: impact.minimum_depth(),
        });
    }
    if output_bytes < analyzed.output_bytes() {
        output_bytes = analyzed.output_bytes();
        if output_bytes > limits.max_output_bytes() {
            return Err(output_limit());
        }
    }
    Ok(RustGraphImpactResult {
        trace,
        impacted: impacted.into_boxed_slice(),
        unknown_coverage: analyzed.unknown_coverage(),
        output_bytes,
    })
}

fn convert_graph_trace_result(
    publication: RustGraphPublicationSummary,
    loaded: &LoadedRustGraphRelationships,
    traced: &repowitness_analysis::RustGraphTraceResult,
    limits: RustGraphReadLimits,
) -> Result<RustGraphTraceResult, GraphFailure> {
    let mut output_bytes = 160_u64;
    let mut edges = Vec::with_capacity(traced.edges().len());
    for traced_edge in traced.edges() {
        let relationship = traced_edge.relationship();
        let source = loaded
            .definitions
            .get(relationship.source())
            .ok_or_else(corrupt_graph)?
            .clone();
        let site = loaded
            .sites
            .get(relationship.site())
            .ok_or_else(corrupt_graph)?
            .clone();
        let target = loaded
            .definitions
            .get(relationship.target())
            .ok_or_else(corrupt_graph)?
            .clone();
        output_bytes =
            graph_definition_output_bytes(output_bytes, &source, limits.max_output_bytes())?;
        output_bytes =
            graph_definition_output_bytes(output_bytes, &target, limits.max_output_bytes())?;
        output_bytes = bounded_graph_output(
            output_bytes,
            site.path()
                .byte_count()
                .get()
                .checked_add(160)
                .ok_or_else(output_limit)?,
            limits.max_output_bytes(),
        )?;
        edges.push(crate::sqlite::RustGraphEdgeRecord {
            depth: traced_edge.depth(),
            kind: local_graph_edge_kind(relationship.kind()),
            extraction_evidence: relationship.extraction_evidence(),
            resolution_evidence: relationship.resolution_evidence(),
            cardinality: relationship.cardinality(),
            site,
            source,
            target,
        });
    }
    output_bytes = output_bytes.max(traced.output_bytes());
    if output_bytes > limits.max_output_bytes() {
        return Err(output_limit());
    }
    let truncation = traced.truncation();
    Ok(RustGraphTraceResult {
        publication,
        edges: edges.into_boxed_slice(),
        visited_nodes: traced.visited_nodes(),
        visited_edges: traced.visited_edges(),
        maximum_completed_depth: traced.maximum_completed_depth(),
        truncation: crate::sqlite::RustGraphTraceTruncation {
            depth: truncation.depth(),
            visited_nodes: truncation.visited_nodes(),
            visited_edges: truncation.visited_edges(),
            frontier: truncation.frontier(),
            results: truncation.results(),
        },
        coverage: traced.coverage(),
        input_bytes: traced.input_bytes(),
        output_bytes,
    })
}

fn load_exact_graph_definition(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    requested: &RustGraphDefinitionRecord,
) -> Result<Option<RustGraphDefinitionRecord>, GraphFailure> {
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
           AND definition.source_slot_id = ?2
           AND definition.source_generation_id = ?3
           AND definition.repository_path = ?4
           AND definition.artifact_digest = ?5
           AND definition.fact_ordinal = ?6
           AND definition.symbol_kind = ?7
           AND definition.name_start = ?8
           AND definition.name_end = ?9
           AND definition.declaration_start = ?10
           AND definition.declaration_end = ?11",
    )?;
    let mut rows = statement.query(params![
        generation.get(),
        requested.source_slot().as_bytes().as_slice(),
        requested.source_generation().get(),
        requested.path().as_bytes(),
        requested.artifact().as_bytes().as_slice(),
        i64::try_from(requested.fact_ordinal()).map_err(|_| corrupt_graph())?,
        requested.kind().as_str(),
        persisted_selector_offset(requested.name_span().start().get())?,
        persisted_selector_offset(requested.name_span().end().get())?,
        persisted_selector_offset(requested.declaration_span().start().get())?,
        persisted_selector_offset(requested.declaration_span().end().get())?,
    ])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let definition = decode_graph_definition(row, 0)?;
    if rows.next()?.is_some() {
        return Err(corrupt_graph());
    }
    if definition == *requested {
        Ok(Some(definition))
    } else {
        Err(graph_start_unavailable())
    }
}

fn analysis_graph_limits(
    limits: RustGraphReadLimits,
) -> Result<repowitness_analysis::RustGraphTraceLimits, GraphFailure> {
    repowitness_analysis::RustGraphTraceLimits::try_new(
        limits.max_input_edges(),
        limits.max_input_bytes(),
        limits.max_depth(),
        limits.max_results(),
        limits.max_visited_nodes(),
        limits.max_visited_edges(),
        limits.max_frontier(),
        limits.max_output_bytes(),
    )
    .map_err(map_graph_analysis_error)
}

fn local_graph_edge_kind(kind: repowitness_analysis::RustGraphEdgeKind) -> RustGraphEdgeKind {
    match kind {
        repowitness_analysis::RustGraphEdgeKind::Import => RustGraphEdgeKind::Import,
        repowitness_analysis::RustGraphEdgeKind::Reference => RustGraphEdgeKind::Reference,
        repowitness_analysis::RustGraphEdgeKind::Call => RustGraphEdgeKind::Call,
    }
}

fn local_graph_impact_class(
    class: repowitness_analysis::RustGraphImpactClass,
) -> crate::sqlite::RustGraphImpactClass {
    match class {
        repowitness_analysis::RustGraphImpactClass::DirectlyConnected => {
            crate::sqlite::RustGraphImpactClass::DirectlyConnected
        }
        repowitness_analysis::RustGraphImpactClass::Possible => {
            crate::sqlite::RustGraphImpactClass::Possible
        }
        repowitness_analysis::RustGraphImpactClass::Unknown => {
            crate::sqlite::RustGraphImpactClass::Unknown
        }
    }
}

fn map_graph_analysis_error(error: repowitness_analysis::RustGraphTraceError) -> GraphFailure {
    use repowitness_analysis::RustGraphTraceError;

    let error = match error {
        RustGraphTraceError::InputEdgeLimitExceeded
        | RustGraphTraceError::InputByteLimitExceeded => RustGraphReadError::InputLimitExceeded,
        RustGraphTraceError::StartUnavailable => RustGraphReadError::StartUnavailable,
        RustGraphTraceError::Cancelled => RustGraphReadError::Cancelled,
        RustGraphTraceError::DeadlineExceeded => RustGraphReadError::DeadlineExceeded,
        RustGraphTraceError::OutputLimitExceeded => RustGraphReadError::OutputLimitExceeded,
        RustGraphTraceError::InvalidLimits
        | RustGraphTraceError::InvalidEdgeKinds
        | RustGraphTraceError::InvalidEdge
        | RustGraphTraceError::DuplicateEdge
        | RustGraphTraceError::CountOverflow => RustGraphReadError::CorruptGraph,
    };
    GraphFailure::Read(error)
}

fn graph_start_unavailable() -> GraphFailure {
    GraphFailure::Read(RustGraphReadError::StartUnavailable)
}
