#[derive(Clone, Debug, Eq, PartialEq)]
enum GraphWorkspaceContext {
    SingleRepository(String),
}

fn read_local_graph_service(
    database: &Path,
    workspace: &GraphWorkspaceContext,
    request: GraphReadServiceRequest,
    configuration: &ResolvedConfiguration,
    cancelled: Arc<AtomicBool>,
) -> Result<GraphReadServiceOutput, String> {
    let exact_pin = request.exact_pin();
    let timeout = request.timeout();
    let operation = request.into_operation();
    let mut local_request = match workspace {
        GraphWorkspaceContext::SingleRepository(repository_identity) => {
            LocalRustGraphReadRequest::new(database, repository_identity, operation)
        }
    }
    .with_configuration(configuration)
    .with_deadline(timeout);
    if let Some((workspace_view, graph_generation)) = exact_pin {
        local_request = local_request
            .with_exact_pin(workspace_view, graph_generation)
            .map_err(|_| "graph immutable selection is invalid".to_owned())?;
    }
    read_local_rust_graph(local_request, cancelled)
        .map_err(|_| "local graph read failed".to_owned())
        .and_then(mcp_graph_output)
}

fn mcp_graph_output(
    result: LocalRustGraphReadResult,
) -> Result<GraphReadServiceOutput, String> {
    let context = GraphOutputContext {
        workspace: result.connected_workspace(),
        workspace_view: result.workspace_view(),
        graph_generation: result.graph_generation(),
    };
    let output = result.into_output();
    match output {
        LocalRustGraphReadOutput::Status(availability) => {
            mcp_graph_status_output(context, availability)
        }
        LocalRustGraphReadOutput::Search(search) => mcp_graph_search_output(context, &search),
        LocalRustGraphReadOutput::Evidence(read) => mcp_graph_evidence_output(context, &read),
        LocalRustGraphReadOutput::Architecture(architecture) => {
            mcp_graph_architecture_output(context, &architecture)
        }
        LocalRustGraphReadOutput::Trace(trace) => mcp_graph_trace_output(context, &trace),
        LocalRustGraphReadOutput::Impact(impact) => mcp_graph_impact_output(context, &impact),
    }
}

struct GraphOutputContext {
    workspace: repowitness_local::ConnectedWorkspaceId,
    workspace_view: i64,
    graph_generation: i64,
}

impl GraphOutputContext {
    fn into_wire(
        self,
        publication: Option<&RustGraphPublicationSummary>,
    ) -> Result<McpGraphContext, String> {
        mcp_graph_context(
            self.workspace,
            self.workspace_view,
            self.graph_generation,
            publication,
        )
    }
}

fn mcp_graph_status_output(
    context: GraphOutputContext,
    availability: RustGraphAvailability,
) -> Result<GraphReadServiceOutput, String> {
    let (publication, availability) = match availability {
        RustGraphAvailability::NotProduced { generation } => {
            if generation.get() != context.graph_generation {
                return Err("graph status generation is inconsistent".to_owned());
            }
            (None, "not_produced")
        }
        RustGraphAvailability::Complete(publication) => (Some(publication), "complete"),
    };
    Ok(GraphReadServiceOutput::Status(GraphStatusOutput {
        schema_version: 1,
        context: context.into_wire(publication.as_deref())?,
        availability: availability.to_owned(),
    }))
}

fn mcp_graph_search_output(
    context: GraphOutputContext,
    search: &repowitness_local::RustGraphSymbolSearchResult,
) -> Result<GraphReadServiceOutput, String> {
    let matches_returned = u64::try_from(search.definitions().len())
        .map_err(|_| "graph search result count overflowed".to_owned())?;
    interoperable(&[
        matches_returned,
        search.total_matches(),
        search.output_bytes(),
    ])?;
    if matches_returned > search.total_matches() {
        return Err("graph search accounting is inconsistent".to_owned());
    }
    let definitions = search
        .definitions()
        .iter()
        .map(mcp_graph_definition)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(GraphReadServiceOutput::Search(GraphSearchOutput {
        schema_version: 1,
        context: context.into_wire(Some(search.publication()))?,
        matches_returned,
        matches_total: search.total_matches(),
        truncated: matches_returned < search.total_matches(),
        output_bytes: search.output_bytes(),
        definitions,
    }))
}

fn mcp_graph_evidence_output(
    context: GraphOutputContext,
    read: &repowitness_local::LocalRustGraphEvidenceRead,
) -> Result<GraphReadServiceOutput, String> {
    let evidence = read.evidence().map(mcp_graph_evidence).transpose()?;
    Ok(GraphReadServiceOutput::Evidence(GraphEvidenceOutput {
        schema_version: 1,
        context: context.into_wire(Some(read.publication()))?,
        found: evidence.is_some(),
        evidence,
    }))
}

fn mcp_graph_architecture_output(
    context: GraphOutputContext,
    architecture: &repowitness_local::RustGraphArchitectureSummary,
) -> Result<GraphReadServiceOutput, String> {
    let definitions_by_kind = architecture
        .definitions_by_kind()
        .iter()
        .map(|(kind, count)| mcp_graph_architecture_count(kind.as_str(), *count))
        .collect::<Result<Vec<_>, _>>()?;
    let edges_by_kind = architecture
        .edges_by_kind()
        .iter()
        .map(|(kind, count)| mcp_graph_architecture_count(kind.as_str(), *count))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(GraphReadServiceOutput::Architecture(
        GraphArchitectureOutput {
            schema_version: 1,
            context: context.into_wire(Some(architecture.publication()))?,
            definitions_by_kind,
            edges_by_kind,
        },
    ))
}

fn mcp_graph_architecture_count(
    kind: &str,
    count: u64,
) -> Result<McpGraphArchitectureCount, String> {
    interoperable(&[count])?;
    Ok(McpGraphArchitectureCount {
        kind: kind.to_owned(),
        count,
    })
}

fn mcp_graph_trace_output(
    context: GraphOutputContext,
    trace: &RustGraphTraceResult,
) -> Result<GraphReadServiceOutput, String> {
    Ok(GraphReadServiceOutput::Trace(GraphTraceOutput {
        schema_version: 1,
        context: context.into_wire(Some(trace.publication()))?,
        trace: mcp_graph_trace(trace)?,
    }))
}

fn mcp_graph_impact_output(
    context: GraphOutputContext,
    impact: &repowitness_local::RustGraphImpactResult,
) -> Result<GraphReadServiceOutput, String> {
    let trace = impact.trace();
    interoperable(&[impact.output_bytes()])?;
    let impacts = impact
        .impacted()
        .iter()
        .map(|impact| {
            Ok(McpGraphImpact {
                class: impact_class(impact.class()).to_owned(),
                definition: mcp_graph_definition(impact.definition())?,
                minimum_depth: impact.minimum_depth(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(GraphReadServiceOutput::Impact(GraphImpactOutput {
        schema_version: 1,
        context: context.into_wire(Some(trace.publication()))?,
        trace: mcp_graph_trace(trace)?,
        impacts,
        unknown_coverage: impact.unknown_coverage(),
        output_bytes: impact.output_bytes(),
    }))
}

fn mcp_graph_context(
    workspace: repowitness_local::ConnectedWorkspaceId,
    workspace_view: i64,
    graph_generation: i64,
    publication: Option<&RustGraphPublicationSummary>,
) -> Result<McpGraphContext, String> {
    interoperable_i64(&[workspace_view, graph_generation])?;
    if let Some(publication) = publication
        && (publication.connected_workspace() != workspace
            || publication.generation().get() != graph_generation)
    {
        return Err("graph publication context is inconsistent".to_owned());
    }
    Ok(McpGraphContext {
        connected_workspace: ConnectedWorkspaceIdTextV1::encode(workspace).into_string(),
        workspace_view,
        graph_generation,
        publication: publication.map(mcp_graph_publication).transpose()?,
    })
}

fn mcp_graph_publication(
    publication: &RustGraphPublicationSummary,
) -> Result<McpGraphPublication, String> {
    interoperable(&[
        publication.artifact_count(),
        publication.definition_count(),
        publication.site_count(),
        publication.unresolved_count(),
        publication.unique_count(),
        publication.ambiguous_count(),
        publication.unsupported_count(),
        publication.truncated_site_count(),
        publication.retained_candidate_count(),
        publication.edge_count(),
        publication.input_text_bytes(),
        publication.output_bytes(),
        publication.syntax_error_nodes(),
        publication.macro_sites(),
        publication.test_marker_sites(),
        publication.heuristic_sites(),
    ])?;
    Ok(McpGraphPublication {
        resolver_profile: publication.resolver_profile_version(),
        input_sha256: hex(publication.input_digest()),
        output_sha256: hex(publication.output_digest()),
        source_count: publication.source_count(),
        artifact_count: publication.artifact_count(),
        definition_count: publication.definition_count(),
        site_count: publication.site_count(),
        unresolved_count: publication.unresolved_count(),
        unique_count: publication.unique_count(),
        ambiguous_count: publication.ambiguous_count(),
        unsupported_count: publication.unsupported_count(),
        truncated_site_count: publication.truncated_site_count(),
        retained_candidate_count: publication.retained_candidate_count(),
        edge_count: publication.edge_count(),
        input_text_bytes: publication.input_text_bytes(),
        output_bytes: publication.output_bytes(),
        syntax_error_nodes: publication.syntax_error_nodes(),
        macro_sites: publication.macro_sites(),
        test_marker_sites: publication.test_marker_sites(),
        heuristic_sites: publication.heuristic_sites(),
    })
}

fn mcp_graph_definition(
    definition: &RustGraphDefinitionRecord,
) -> Result<McpGraphDefinition, String> {
    interoperable(&[
        definition.fact_ordinal(),
        definition.name_span().start().get(),
        definition.name_span().end().get(),
        definition.declaration_span().start().get(),
        definition.declaration_span().end().get(),
    ])?;
    interoperable_i64(&[definition.source_generation().get()])?;
    Ok(McpGraphDefinition {
        source_slot: SourceSlotIdTextV1::encode(definition.source_slot()).into_string(),
        source_generation: definition.source_generation().get(),
        path: RepositoryPathTextV1::encode(definition.path(), PATH_TEXT_LIMIT)
            .map_err(|_| "graph definition path cannot be encoded".to_owned())?
            .into_string(),
        content_sha256: hex(definition.content_digest().as_bytes()),
        artifact_sha256: hex(definition.artifact().as_bytes()),
        fact_ordinal: definition.fact_ordinal(),
        symbol_kind: definition.kind().as_str().to_owned(),
        name: definition.name().to_owned(),
        qualified_name: definition.qualified_name().to_owned(),
        name_span: mcp_span(
            definition.name_span().start().get(),
            definition.name_span().end().get(),
        ),
        declaration_span: mcp_span(
            definition.declaration_span().start().get(),
            definition.declaration_span().end().get(),
        ),
    })
}

fn mcp_graph_site(site: &RustGraphSiteSelector) -> Result<McpGraphSite, String> {
    interoperable(&[
        site.occurrence_span().start().get(),
        site.occurrence_span().end().get(),
        site.target_span().start().get(),
        site.target_span().end().get(),
    ])?;
    Ok(McpGraphSite {
        source_slot: SourceSlotIdTextV1::encode(site.source_slot()).into_string(),
        path: RepositoryPathTextV1::encode(site.path(), PATH_TEXT_LIMIT)
            .map_err(|_| "graph site path cannot be encoded".to_owned())?
            .into_string(),
        artifact_sha256: hex(site.artifact().as_bytes()),
        ordinal: site.ordinal(),
        site_kind: site.kind().as_str().to_owned(),
        occurrence_span: mcp_span(
            site.occurrence_span().start().get(),
            site.occurrence_span().end().get(),
        ),
        target_span: mcp_span(site.target_span().start().get(), site.target_span().end().get()),
    })
}

fn mcp_graph_evidence(evidence: &RustGraphEvidenceResult) -> Result<McpGraphEvidence, String> {
    let (outcome, unresolved_reason, candidates) = match evidence.outcome() {
        RustGraphOutcomeRecord::Unresolved(reason) => (
            "unresolved",
            Some(reason.as_str().to_owned()),
            Vec::new(),
        ),
        RustGraphOutcomeRecord::Unique(candidate) => (
            "unique",
            None,
            vec![mcp_graph_candidate(candidate.as_ref())?],
        ),
        RustGraphOutcomeRecord::Ambiguous(candidates) => (
            "ambiguous",
            None,
            candidates
                .iter()
                .map(mcp_graph_candidate)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    };
    let retained = u32::try_from(candidates.len())
        .map_err(|_| "graph evidence candidate count overflowed".to_owned())?;
    if retained > evidence.candidate_count()
        || evidence.candidates_truncated() != (retained < evidence.candidate_count())
        || (outcome == "unresolved" && evidence.candidate_count() != 0)
        || (outcome == "unique" && evidence.candidate_count() != 1)
        || (outcome == "ambiguous" && evidence.candidate_count() < 2)
    {
        return Err("graph evidence candidate accounting is inconsistent".to_owned());
    }
    Ok(McpGraphEvidence {
        site: mcp_graph_site(evidence.site())?,
        content_sha256: hex(evidence.content_digest().as_bytes()),
        extraction_evidence: evidence.extraction_evidence().as_str().to_owned(),
        outcome: outcome.to_owned(),
        unresolved_reason,
        candidate_count: evidence.candidate_count(),
        candidates_truncated: evidence.candidates_truncated(),
        candidates,
    })
}

fn mcp_graph_candidate(candidate: &RustGraphCandidateRecord) -> Result<McpGraphCandidate, String> {
    Ok(McpGraphCandidate {
        target: mcp_graph_definition(candidate.target())?,
        resolution_evidence: candidate.evidence().as_str().to_owned(),
    })
}

fn mcp_graph_trace(trace: &RustGraphTraceResult) -> Result<McpGraphTrace, String> {
    let coverage = trace.coverage();
    interoperable(&[
        trace.visited_nodes(),
        trace.visited_edges(),
        trace.input_bytes(),
        trace.output_bytes(),
        coverage.unresolved_sites(),
        coverage.unsupported_sites(),
        coverage.ambiguous_sites(),
        coverage.truncated_sites(),
        coverage.unlinked_sites(),
        coverage.macro_sites(),
        coverage.conditional_sites(),
        coverage.heuristic_sites(),
    ])?;
    let edges = trace
        .edges()
        .iter()
        .map(|edge| {
            let cardinality = edge.cardinality();
            Ok(McpGraphEdge {
                depth: edge.depth(),
                edge_kind: edge.kind().as_str().to_owned(),
                extraction_evidence: edge.extraction_evidence().as_str().to_owned(),
                resolution_evidence: edge.resolution_evidence().as_str().to_owned(),
                cardinality: McpGraphCardinality {
                    kind: if cardinality.is_ambiguous() {
                        "ambiguous"
                    } else {
                        "unique"
                    }
                    .to_owned(),
                    candidate_count: cardinality.candidate_count(),
                    retained_candidates: cardinality.retained_candidates(),
                    candidates_truncated: cardinality.candidates_truncated(),
                },
                site: mcp_graph_site(edge.site())?,
                source: mcp_graph_definition(edge.source())?,
                target: mcp_graph_definition(edge.target())?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let truncation = trace.truncation();
    Ok(McpGraphTrace {
        edges,
        visited_nodes: trace.visited_nodes(),
        visited_edges: trace.visited_edges(),
        maximum_completed_depth: trace.maximum_completed_depth(),
        truncation: McpGraphTraceTruncation {
            depth: truncation.depth(),
            visited_nodes: truncation.visited_nodes(),
            visited_edges: truncation.visited_edges(),
            frontier: truncation.frontier(),
            results: truncation.results(),
        },
        coverage: McpGraphTraceCoverage {
            unresolved_sites: coverage.unresolved_sites(),
            unsupported_sites: coverage.unsupported_sites(),
            ambiguous_sites: coverage.ambiguous_sites(),
            truncated_sites: coverage.truncated_sites(),
            unlinked_sites: coverage.unlinked_sites(),
            macro_sites: coverage.macro_sites(),
            conditional_sites: coverage.conditional_sites(),
            heuristic_sites: coverage.heuristic_sites(),
        },
        input_bytes: trace.input_bytes(),
        output_bytes: trace.output_bytes(),
    })
}

const fn mcp_span(start: u64, end: u64) -> McpSpan {
    McpSpan { start, end }
}

const fn impact_class(class: RustGraphImpactClass) -> &'static str {
    match class {
        RustGraphImpactClass::DirectlyConnected => "directly_connected",
        RustGraphImpactClass::Possible => "possible",
        RustGraphImpactClass::Unknown => "unknown",
    }
}

fn interoperable(values: &[u64]) -> Result<(), String> {
    if values
        .iter()
        .any(|value| *value > MAX_MCP_INTEROPERABLE_INTEGER)
    {
        Err("graph output exceeds the interoperable integer range".to_owned())
    } else {
        Ok(())
    }
}

fn interoperable_i64(values: &[i64]) -> Result<(), String> {
    if values.iter().any(|value| {
        *value <= 0 || u64::try_from(*value).ok() > Some(MAX_MCP_INTEROPERABLE_INTEGER)
    }) {
        Err("graph output identity exceeds the interoperable integer range".to_owned())
    } else {
        Ok(())
    }
}
