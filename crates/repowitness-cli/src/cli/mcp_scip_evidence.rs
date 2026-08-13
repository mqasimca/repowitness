fn read_local_scip_evidence_service(
    database: &Path,
    workspace: &GraphWorkspaceContext,
    request: ScipEvidenceServiceRequest,
    cancelled: Arc<AtomicBool>,
) -> Result<ScipEvidenceOutput, String> {
    let package_scope_digest = hex(request.package_scope().semantic_digest().as_bytes());
    let mut local_request = match workspace {
        GraphWorkspaceContext::SingleRepository(repository_identity) => LocalScipEvidenceReadRequest::new(
            database,
            repository_identity,
            request.package_scope().clone(),
            request.symbol().clone(),
        ),
        GraphWorkspaceContext::ConnectedWorkspace {
            connected_workspace,
            source_slot,
        } => LocalScipEvidenceReadRequest::for_connected_workspace(
            database,
            connected_workspace,
            source_slot,
            request.package_scope().clone(),
            request.symbol().clone(),
        ),
    }
    .with_deadline(request.timeout());
    if let Some(workspace_view) = request.workspace_view() {
        local_request = local_request
            .with_exact_view(workspace_view)
            .map_err(|_| "SCIP immutable selection is invalid".to_owned())?;
    }
    read_local_scip_evidence(local_request, cancelled)
        .map_err(|_| "local SCIP evidence read failed".to_owned())
        .and_then(|result| mcp_scip_evidence_output(result, package_scope_digest))
}
fn mcp_scip_evidence_output(
    result: repowitness_local::LocalScipEvidenceReadResult,
    package_scope_sha256: String,
) -> Result<ScipEvidenceOutput, String> {
    let workspace_view = result.workspace_view();
    let result_source_slot = result.source_slot();
    interoperable_i64(&[workspace_view])?;
    let source_slot = SourceSlotIdTextV1::encode(result.source_slot()).into_string();
    let connected_workspace =
        ConnectedWorkspaceIdTextV1::encode(result.connected_workspace()).into_string();
    match result.into_output() {
        ScipSymbolEvidenceResult::NotProduced => Ok(ScipEvidenceOutput {
            schema_version: 1,
            connected_workspace,
            workspace_view,
            source_slot,
            resolution: "not_produced".to_owned(),
            overlay: None,
            package_scope_sha256: None,
            occurrences_truncated: false,
            relationships_truncated: false,
            output_bytes: 0,
            occurrences: Vec::new(),
            relationships: Vec::new(),
        }),
        ScipSymbolEvidenceResult::NoMatch(overlay) => Ok(ScipEvidenceOutput {
            schema_version: 1,
            connected_workspace,
            workspace_view,
            source_slot,
            resolution: "no_match".to_owned(),
            overlay: Some(mcp_scip_overlay(overlay)?),
            package_scope_sha256: Some(package_scope_sha256),
            occurrences_truncated: false,
            relationships_truncated: false,
            output_bytes: 0,
            occurrences: Vec::new(),
            relationships: Vec::new(),
        }),
        ScipSymbolEvidenceResult::Found(evidence) => {
            let overlay = evidence.overlay();
            if overlay.source_slot() != result_source_slot {
                return Err("SCIP evidence source slot is inconsistent".to_owned());
            }
            interoperable(&[evidence.output_bytes()])?;
            let occurrences = evidence
                .occurrences()
                .iter()
                .map(mcp_scip_occurrence)
                .collect::<Result<Vec<_>, _>>()?;
            let relationships = evidence
                .relationships()
                .iter()
                .map(mcp_scip_relationship)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ScipEvidenceOutput {
                schema_version: 1,
                connected_workspace,
                workspace_view,
                source_slot,
                resolution: "found".to_owned(),
                overlay: Some(mcp_scip_overlay(overlay)?),
                package_scope_sha256: Some(package_scope_sha256),
                occurrences_truncated: evidence.occurrences_truncated(),
                relationships_truncated: evidence.relationships_truncated(),
                output_bytes: evidence.output_bytes(),
                occurrences,
                relationships,
            })
        }
    }
}

fn mcp_scip_overlay(
    overlay: repowitness_local::ScipOverlaySummary,
) -> Result<McpScipOverlay, String> {
    interoperable(&[
        overlay.documents(),
        overlay.occurrences(),
        overlay.relationships(),
    ])?;
    Ok(McpScipOverlay {
        overlay_sha256: hex(overlay.digest().as_bytes()),
        documents: overlay.documents(),
        occurrences: overlay.occurrences(),
        relationships: overlay.relationships(),
    })
}

fn mcp_scip_occurrence(
    occurrence: &repowitness_local::ScipOccurrenceEvidence,
) -> Result<McpScipOccurrence, String> {
    let span = occurrence.span();
    interoperable(&[span.start().get(), span.end().get()])?;
    let roles = occurrence.roles().bits();
    Ok(McpScipOccurrence {
        path: RepositoryPathTextV1::encode(occurrence.path(), PATH_TEXT_LIMIT)
            .map_err(|_| "SCIP occurrence path cannot be encoded".to_owned())?
            .into_string(),
        content_sha256: hex(occurrence.content().as_bytes()),
        span_start: span.start().get(),
        span_end: span.end().get(),
        definition: roles & 0x1 != 0,
        import: roles & 0x2 != 0,
        write_access: roles & 0x4 != 0,
        read_access: roles & 0x8 != 0,
    })
}

fn mcp_scip_relationship(
    relationship: &repowitness_local::ScipRelationshipEvidence,
) -> Result<McpScipRelationship, String> {
    let kinds = relationship.kinds();
    Ok(McpScipRelationship {
        path: RepositoryPathTextV1::encode(relationship.path(), PATH_TEXT_LIMIT)
            .map_err(|_| "SCIP relationship path cannot be encoded".to_owned())?
            .into_string(),
        content_sha256: hex(relationship.content().as_bytes()),
        direction: match relationship.direction() {
            ScipRelationshipDirection::Outgoing => "outgoing",
            ScipRelationshipDirection::Incoming => "incoming",
        }
        .to_owned(),
        source: relationship.source().as_str().to_owned(),
        target: relationship.target().as_str().to_owned(),
        is_reference: kinds.is_reference(),
        is_implementation: kinds.is_implementation(),
        is_type_definition: kinds.is_type_definition(),
        is_definition: kinds.is_definition(),
    })
}
