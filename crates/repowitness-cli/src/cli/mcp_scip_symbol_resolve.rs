fn read_local_scip_symbol_resolve_service(
    database: &Path,
    workspace: &GraphWorkspaceContext,
    request: ScipSymbolResolveServiceRequest,
    cancelled: Arc<AtomicBool>,
) -> Result<ScipSymbolResolveOutput, String> {
    let span = request.name_span();
    let selector = LocalScipSymbolResolveSelectorText::new(
        request.snapshot_sha256(),
        request.generation(),
        request.path(),
        request.content_sha256(),
        request.artifact_sha256(),
        request.fact_ordinal(),
        (span.start, span.end),
    );
    let mut local_request = match workspace {
        GraphWorkspaceContext::SingleRepository(repository_identity) => LocalScipSymbolResolveRequest::new(
            database,
            repository_identity,
            selector,
        ),
        GraphWorkspaceContext::ConnectedWorkspace { connected_workspace, source_slot } => {
            LocalScipSymbolResolveRequest::for_connected_workspace(
                database,
                connected_workspace,
                source_slot,
                selector,
            )
        }
    }
    .with_deadline(request.timeout());
    if let Some(workspace_view) = request.workspace_view() {
        local_request = local_request
            .with_exact_view(workspace_view)
            .map_err(|_| "SCIP immutable selection is invalid".to_owned())?;
    }
    resolve_local_scip_symbol(local_request, cancelled)
        .map_err(|_| "local SCIP symbol resolution failed".to_owned())
        .and_then(mcp_scip_symbol_resolve_output)
}
fn mcp_scip_symbol_resolve_output(
    result: repowitness_local::LocalScipSymbolResolveResult,
) -> Result<ScipSymbolResolveOutput, String> {
    let workspace_view = result.workspace_view();
    interoperable_i64(&[workspace_view])?;
    let connected_workspace =
        ConnectedWorkspaceIdTextV1::encode(result.connected_workspace()).into_string();
    let source_slot = SourceSlotIdTextV1::encode(result.source_slot()).into_string();
    let (resolution, symbol) = match result.into_output() {
        repowitness_local::ScipSyntaxSymbolResolution::NotProduced => ("not_produced", None),
        repowitness_local::ScipSyntaxSymbolResolution::NoExactMatch => ("no_exact_match", None),
        repowitness_local::ScipSyntaxSymbolResolution::Ambiguous => ("ambiguous", None),
        repowitness_local::ScipSyntaxSymbolResolution::Exact(symbol) => ("exact", Some(symbol.as_str().to_owned())),
    };
    Ok(ScipSymbolResolveOutput {
        schema_version: 1,
        connected_workspace,
        workspace_view,
        source_slot,
        resolution: resolution.to_owned(),
        symbol,
    })
}
