fn read_local_scip_relationship_trace_service(
    database: &Path,
    workspace: &GraphWorkspaceContext,
    request: ScipRelationshipTraceServiceRequest,
    cancelled: Arc<AtomicBool>,
) -> Result<ScipRelationshipTraceOutput, String> {
    let package_scope_sha256 = hex(request.package_scope().semantic_digest().as_bytes());
    let direction = request.direction();
    let max_depth = request.max_depth().get();
    let max_edges = request.max_edges().get();
    let mut local_request = match workspace {
        GraphWorkspaceContext::SingleRepository(repository_identity) => {
            LocalScipRelationshipTraceRequest::new(
                database,
                repository_identity,
                request.package_scope().clone(),
                request.symbol().clone(),
                direction,
                request.max_depth(),
                request.max_edges(),
            )
        }
        GraphWorkspaceContext::ConnectedWorkspace {
            connected_workspace,
            source_slot,
        } => LocalScipRelationshipTraceRequest::for_connected_workspace(
            database,
            connected_workspace,
            source_slot,
            request.package_scope().clone(),
            request.symbol().clone(),
            direction,
            request.max_depth(),
            request.max_edges(),
        ),
    }
    .with_deadline(request.timeout());
    if let Some(workspace_view) = request.workspace_view() {
        local_request = local_request
            .with_exact_view(workspace_view)
            .map_err(|_| "SCIP relationship trace immutable selection is invalid".to_owned())?;
    }
    trace_local_scip_relationships(local_request, cancelled)
        .map_err(|_| "local SCIP relationship trace failed".to_owned())
        .and_then(|result| {
            mcp_scip_relationship_trace_output(
                result,
                package_scope_sha256,
                direction,
                max_depth,
                max_edges,
            )
        })
}
#[allow(
    clippy::too_many_lines,
    reason = "the categorical result mapper validates every immutable receipt before constructing one strict wire response"
)]
fn mcp_scip_relationship_trace_output(
    result: repowitness_local::LocalScipRelationshipTraceResult,
    expected_package_scope_sha256: String,
    requested_direction: ScipRelationshipTraceDirection,
    requested_max_depth: u8,
    requested_max_edges: u16,
) -> Result<ScipRelationshipTraceOutput, String> {
    let workspace_view = result.workspace_view();
    let result_source_slot = result.source_slot();
    interoperable_i64(&[workspace_view])?;
    let source_slot = SourceSlotIdTextV1::encode(result.source_slot()).into_string();
    let connected_workspace =
        ConnectedWorkspaceIdTextV1::encode(result.connected_workspace()).into_string();
    let direction = mcp_scip_relationship_trace_direction(requested_direction).to_owned();
    match result.into_output() {
        ScipRelationshipTraceResult::NotProduced => Ok(ScipRelationshipTraceOutput {
            schema_version: SCIP_RELATIONSHIP_TRACE_SCHEMA_VERSION,
            connected_workspace,
            workspace_view,
            source_slot,
            resolution: "not_produced".to_owned(),
            overlay: None,
            package_scope_sha256: None,
            direction,
            max_depth: requested_max_depth,
            max_edges: requested_max_edges,
            visited_symbols: 0,
            unexpanded_frontier_symbols: 0,
            depth_limit_reached: false,
            edge_limit_reached: false,
            symbol_limit_reached: false,
            output_limit_reached: false,
            truncated: false,
            output_bytes: 0,
            edges: Vec::new(),
        }),
        ScipRelationshipTraceResult::NoRelationships(no_relationships) => {
            let package_scope_sha256 = hex(no_relationships.package_scope().as_bytes());
            if package_scope_sha256 != expected_package_scope_sha256 {
                return Err("SCIP relationship trace package scope is inconsistent".to_owned());
            }
            let overlay = no_relationships.overlay();
            if overlay.source_slot() != result_source_slot {
                return Err("SCIP relationship trace source slot is inconsistent".to_owned());
            }
            Ok(ScipRelationshipTraceOutput {
                schema_version: SCIP_RELATIONSHIP_TRACE_SCHEMA_VERSION,
                connected_workspace,
                workspace_view,
                source_slot,
                resolution: "no_relationships".to_owned(),
                overlay: Some(mcp_scip_relationship_trace_overlay(overlay)?),
                package_scope_sha256: Some(package_scope_sha256),
                direction,
                max_depth: requested_max_depth,
                max_edges: requested_max_edges,
                visited_symbols: 1,
                unexpanded_frontier_symbols: 0,
                depth_limit_reached: false,
                edge_limit_reached: false,
                symbol_limit_reached: false,
                output_limit_reached: false,
                truncated: false,
                output_bytes: 0,
                edges: Vec::new(),
            })
        }
        ScipRelationshipTraceResult::Found(trace) => {
            let package_scope_sha256 = hex(trace.package_scope().as_bytes());
            if package_scope_sha256 != expected_package_scope_sha256 {
                return Err("SCIP relationship trace package scope is inconsistent".to_owned());
            }
            if trace.direction() != requested_direction
                || trace.max_depth() != requested_max_depth {
                return Err("SCIP relationship trace controls are inconsistent".to_owned());
            }
            let overlay = trace.overlay();
            if overlay.source_slot() != result_source_slot {
                return Err("SCIP relationship trace source slot is inconsistent".to_owned());
            }
            interoperable(&[trace.output_bytes()])?;
            let edges = trace
                .edges()
                .iter()
                .map(mcp_scip_relationship_trace_edge)
                .collect::<Result<Vec<_>, _>>()?;
            let truncated = scip_relationship_trace_is_truncated(
                trace.depth_limit_reached(),
                trace.edge_limit_reached(),
                trace.symbol_limit_reached(),
                trace.output_limit_reached(),
            );
            Ok(ScipRelationshipTraceOutput {
                schema_version: SCIP_RELATIONSHIP_TRACE_SCHEMA_VERSION,
                connected_workspace,
                workspace_view,
                source_slot,
                resolution: "found".to_owned(),
                overlay: Some(mcp_scip_relationship_trace_overlay(overlay)?),
                package_scope_sha256: Some(package_scope_sha256),
                direction,
                max_depth: trace.max_depth(),
                max_edges: requested_max_edges,
                visited_symbols: trace.visited_symbols(),
                unexpanded_frontier_symbols: trace.unexpanded_frontier_symbols(),
                depth_limit_reached: trace.depth_limit_reached(),
                edge_limit_reached: trace.edge_limit_reached(),
                symbol_limit_reached: trace.symbol_limit_reached(),
                output_limit_reached: trace.output_limit_reached(),
                truncated,
                output_bytes: trace.output_bytes(),
                edges,
            })
        }
    }
}

const fn scip_relationship_trace_is_truncated(
    depth_limit_reached: bool,
    edge_limit_reached: bool,
    symbol_limit_reached: bool,
    output_limit_reached: bool,
) -> bool {
    depth_limit_reached || edge_limit_reached || symbol_limit_reached || output_limit_reached
}

const fn mcp_scip_relationship_trace_direction(
    direction: ScipRelationshipTraceDirection,
) -> &'static str {
    match direction {
        ScipRelationshipTraceDirection::Outgoing => "outgoing",
        ScipRelationshipTraceDirection::Incoming => "incoming",
    }
}

#[cfg(test)]
mod scip_relationship_trace_mapping_tests {
    use super::scip_relationship_trace_is_truncated;

    #[test]
    fn depth_limited_trace_is_categorically_truncated() {
        assert!(scip_relationship_trace_is_truncated(true, false, false, false));
        assert!(!scip_relationship_trace_is_truncated(false, false, false, false));
    }
}

fn mcp_scip_relationship_trace_overlay(
    overlay: repowitness_local::ScipOverlaySummary,
) -> Result<McpScipRelationshipTraceOverlay, String> {
    interoperable(&[
        overlay.documents(),
        overlay.occurrences(),
        overlay.relationships(),
    ])?;
    Ok(McpScipRelationshipTraceOverlay {
        overlay_sha256: hex(overlay.digest().as_bytes()),
        documents: overlay.documents(),
        occurrences: overlay.occurrences(),
        relationships: overlay.relationships(),
    })
}

fn mcp_scip_relationship_trace_edge(
    edge: &repowitness_local::ScipRelationshipTraceEdge,
) -> Result<McpScipRelationshipTraceEdge, String> {
    let relationship = edge.relationship();
    let kinds = relationship.kinds();
    Ok(McpScipRelationshipTraceEdge {
        document_ordinal: edge.document_ordinal(),
        relationship_ordinal: edge.relationship_ordinal(),
        depth: edge.depth(),
        path: RepositoryPathTextV1::encode(relationship.path(), PATH_TEXT_LIMIT)
            .map_err(|_| "SCIP relationship trace path cannot be encoded".to_owned())?
            .into_string(),
        content_sha256: hex(relationship.content().as_bytes()),
        source: relationship.source().as_str().to_owned(),
        target: relationship.target().as_str().to_owned(),
        is_reference: kinds.is_reference(),
        is_implementation: kinds.is_implementation(),
        is_type_definition: kinds.is_type_definition(),
        is_definition: kinds.is_definition(),
        evidence: relationship.evidence().as_str().to_owned(),
    })
}
