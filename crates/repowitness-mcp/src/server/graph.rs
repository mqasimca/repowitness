impl RepoWitnessMcpServer {
    async fn call_graph_status(
        &self,
        request: GraphReadServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let output = self.call_graph_read(request, context).await?;
        operation_result(
            graph_variant(output, |output| match output {
                GraphReadServiceOutput::Status(output) => Some(output),
                _ => None,
            }),
            MAX_MCP_GRAPH_OUTPUT_BYTES,
        )
    }

    async fn call_graph_search(
        &self,
        request: GraphReadServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let output = self.call_graph_read(request, context).await?;
        operation_result(
            graph_variant(output, |output| match output {
                GraphReadServiceOutput::Search(output) => Some(output),
                _ => None,
            }),
            MAX_MCP_GRAPH_OUTPUT_BYTES,
        )
    }

    async fn call_graph_evidence(
        &self,
        request: GraphReadServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let output = self.call_graph_read(request, context).await?;
        operation_result(
            graph_variant(output, |output| match output {
                GraphReadServiceOutput::Evidence(output) => Some(output),
                _ => None,
            }),
            MAX_MCP_GRAPH_OUTPUT_BYTES,
        )
    }

    async fn call_graph_architecture(
        &self,
        request: GraphReadServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let output = self.call_graph_read(request, context).await?;
        operation_result(
            graph_variant(output, |output| match output {
                GraphReadServiceOutput::Architecture(output) => Some(output),
                _ => None,
            }),
            MAX_MCP_GRAPH_OUTPUT_BYTES,
        )
    }

    async fn call_graph_trace(
        &self,
        request: GraphReadServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let output = self.call_graph_read(request, context).await?;
        operation_result(
            graph_variant(output, |output| match output {
                GraphReadServiceOutput::Trace(output) => Some(output),
                _ => None,
            }),
            MAX_MCP_GRAPH_OUTPUT_BYTES,
        )
    }

    async fn call_graph_impact(
        &self,
        request: GraphReadServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let output = self.call_graph_read(request, context).await?;
        operation_result(
            graph_variant(output, |output| match output {
                GraphReadServiceOutput::Impact(output) => Some(output),
                _ => None,
            }),
            MAX_MCP_GRAPH_OUTPUT_BYTES,
        )
    }

    async fn call_graph_read(
        &self,
        request: GraphReadServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<Result<GraphReadServiceOutput, RepositoryServiceError>, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        self.run_blocking(timeout, context, move |remaining, cancelled| {
            service.graph_read(request.with_timeout(remaining), cancelled)
        })
        .await
    }
}

fn graph_variant<T>(
    result: Result<GraphReadServiceOutput, RepositoryServiceError>,
    select: impl FnOnce(GraphReadServiceOutput) -> Option<T>,
) -> Result<T, RepositoryServiceError> {
    result.and_then(|output| select(output).ok_or(RepositoryServiceError::GraphRead))
}

fn graph_tools(annotations: &ToolAnnotations) -> Vec<Tool> {
    vec![
        Tool::new(
            GRAPH_STATUS_TOOL_NAME,
            "Inspect whether a complete Rust graph exists for one active or exact immutable view.",
            JsonObject::new(),
        )
        .with_input_schema::<GraphStatusInput>()
        .with_output_schema::<GraphStatusOutput>()
        .annotate(annotations.clone()),
        Tool::new(
            GRAPH_SEARCH_TOOL_NAME,
            "Search exact Rust graph definition names and return selectors for evidence, trace, and impact.",
            JsonObject::new(),
        )
        .with_input_schema::<GraphSearchInput>()
        .with_output_schema::<GraphSearchOutput>()
        .annotate(annotations.clone()),
        Tool::new(
            GRAPH_EVIDENCE_TOOL_NAME,
            "Inspect one exact raw Rust graph site, categorical resolution, candidates, and evidence.",
            JsonObject::new(),
        )
        .with_input_schema::<GraphEvidenceInput>()
        .with_output_schema::<GraphEvidenceOutput>()
        .annotate(annotations.clone()),
        Tool::new(
            GRAPH_ARCHITECTURE_TOOL_NAME,
            "Summarize exact Rust definition and unique-edge counts by stable kind.",
            JsonObject::new(),
        )
        .with_input_schema::<GraphArchitectureInput>()
        .with_output_schema::<GraphArchitectureOutput>()
        .annotate(annotations.clone()),
        Tool::new(
            GRAPH_TRACE_TOOL_NAME,
            "Traverse retained unique and ambiguous Rust relationships with explicit evidence, coverage, and truncation.",
            JsonObject::new(),
        )
        .with_input_schema::<GraphTraceInput>()
        .with_output_schema::<GraphTraceOutput>()
        .annotate(annotations.clone()),
        Tool::new(
            IMPACT_ANALYZE_TOOL_NAME,
            "Compute conservative inbound Rust impact without converting incomplete coverage into certainty.",
            JsonObject::new(),
        )
        .with_input_schema::<GraphImpactInput>()
        .with_output_schema::<GraphImpactOutput>()
        .annotate(annotations.clone()),
    ]
}
