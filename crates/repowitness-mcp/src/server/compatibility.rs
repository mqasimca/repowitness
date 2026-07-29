/// Fixed process-lifetime MCP tool surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpToolSurface {
    /// The default native RepoWitness version-1 surface.
    NativeV1,
    /// Native version 1 plus the independently authored ADR-0030 subset.
    NativeV1PlusIncumbentSubsetV1,
}

impl McpToolSurface {
    /// Returns the resolved configuration profile spelling.
    #[must_use]
    pub const fn profile(self) -> &'static str {
        match self {
            Self::NativeV1 => "canonical",
            Self::NativeV1PlusIncumbentSubsetV1 => {
                crate::wire::INCUMBENT_COMPATIBLE_PROFILE
            }
        }
    }

    /// Returns the concrete stable surface identifier.
    #[must_use]
    pub const fn identifier(self) -> &'static str {
        match self {
            Self::NativeV1 => "native-v1",
            Self::NativeV1PlusIncumbentSubsetV1 => {
                crate::wire::INCUMBENT_COMPATIBLE_SURFACE
            }
        }
    }

    const fn includes_compatibility_aliases(self) -> bool {
        matches!(self, Self::NativeV1PlusIncumbentSubsetV1)
    }
}

impl RepoWitnessMcpServer {
    async fn call_compatibility_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        match request.name.as_ref() {
            crate::wire::SEARCH_CODE_ALIAS_TOOL_NAME => {
                let request = parse_compatibility_input::<crate::wire::SearchCodeInput>(
                    request.arguments,
                )?
                .validate()
                .map_err(invalid_compatibility_params)?;
                self.call_search_code_alias(request, context).await
            }
            crate::wire::GET_CODE_SNIPPET_ALIAS_TOOL_NAME => {
                let request = parse_compatibility_input::<crate::wire::GetCodeSnippetInput>(
                    request.arguments,
                )?
                .validate()
                .map_err(invalid_compatibility_params)?;
                self.call_get_code_snippet_alias(request, context).await
            }
            crate::wire::SEARCH_GRAPH_ALIAS_TOOL_NAME => {
                let request = parse_compatibility_input::<crate::wire::SearchGraphInput>(
                    request.arguments,
                )?
                .validate()
                .map_err(invalid_compatibility_params)?;
                self.call_search_graph_alias(request, context).await
            }
            crate::wire::TRACE_PATH_ALIAS_TOOL_NAME => {
                let request = parse_compatibility_input::<crate::wire::TracePathInput>(
                    request.arguments,
                )?
                .validate()
                .map_err(invalid_compatibility_params)?;
                self.call_trace_path_alias(request, context).await
            }
            crate::wire::GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME => {
                let request = parse_compatibility_input::<crate::wire::GetGraphSchemaInput>(
                    request.arguments,
                )?
                .validate()
                .map_err(invalid_compatibility_params)?;
                self.call_get_graph_schema_alias(request, context).await
            }
            crate::wire::GET_ARCHITECTURE_ALIAS_TOOL_NAME => {
                let request = parse_compatibility_input::<crate::wire::GetArchitectureInput>(
                    request.arguments,
                )?
                .validate()
                .map_err(invalid_compatibility_params)?;
                self.call_get_architecture_alias(request, context).await
            }
            crate::wire::INDEX_STATUS_ALIAS_TOOL_NAME => {
                let request = parse_compatibility_input::<crate::wire::IndexStatusInput>(
                    request.arguments,
                )?
                .validate()
                .map_err(invalid_compatibility_params)?;
                self.call_index_status_alias(request, context).await
            }
            _ => Err(McpError::invalid_params("unknown RepoWitness tool", None)),
        }
    }

    async fn call_search_code_alias(
        &self,
        request: CodeSearchServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.code_search(request.with_timeout(remaining), cancelled)
            })
            .await?;
        compatibility_operation_result(
            output.map(|output| {
                crate::wire::compatibility_output(
                    crate::wire::CompatibilityAlias::SearchCode,
                    output,
                )
            }),
            crate::wire::MAX_MCP_SEARCH_OUTPUT_BYTES,
        )
    }

    async fn call_get_code_snippet_alias(
        &self,
        request: SymbolGetServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.symbol_get(request.with_timeout(remaining), cancelled)
            })
            .await?;
        compatibility_operation_result(
            output.map(|output| {
                crate::wire::compatibility_output(
                    crate::wire::CompatibilityAlias::GetCodeSnippet,
                    output,
                )
            }),
            crate::wire::MAX_MCP_SYMBOL_OUTPUT_BYTES,
        )
    }

    async fn call_search_graph_alias(
        &self,
        request: GraphReadServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let output = self.call_graph_read(request, context).await?;
        compatibility_operation_result(
            graph_variant(output, |output| match output {
                GraphReadServiceOutput::Search(output) => Some(
                    crate::wire::compatibility_output(
                        crate::wire::CompatibilityAlias::SearchGraph,
                        output,
                    ),
                ),
                _ => None,
            }),
            crate::wire::MAX_MCP_GRAPH_OUTPUT_BYTES,
        )
    }

    async fn call_trace_path_alias(
        &self,
        request: GraphReadServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let output = self.call_graph_read(request, context).await?;
        compatibility_operation_result(
            graph_variant(output, |output| match output {
                GraphReadServiceOutput::Trace(output) => Some(
                    crate::wire::compatibility_output(
                        crate::wire::CompatibilityAlias::TracePath,
                        output,
                    ),
                ),
                _ => None,
            }),
            crate::wire::MAX_MCP_GRAPH_OUTPUT_BYTES,
        )
    }

    async fn call_get_graph_schema_alias(
        &self,
        request: GraphReadServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let output = self.call_graph_read(request, context).await?;
        compatibility_operation_result(
            graph_variant(output, |output| match output {
                GraphReadServiceOutput::Status(output) => {
                    Some(crate::wire::graph_schema_output(output))
                }
                _ => None,
            }),
            crate::wire::MAX_MCP_GRAPH_OUTPUT_BYTES,
        )
    }

    async fn call_get_architecture_alias(
        &self,
        request: GraphReadServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let output = self.call_graph_read(request, context).await?;
        compatibility_operation_result(
            graph_variant(output, |output| match output {
                GraphReadServiceOutput::Architecture(output) => Some(
                    crate::wire::compatibility_output(
                        crate::wire::CompatibilityAlias::GetArchitecture,
                        output,
                    ),
                ),
                _ => None,
            }),
            crate::wire::MAX_MCP_GRAPH_OUTPUT_BYTES,
        )
    }

    async fn call_index_status_alias(
        &self,
        request: DiagnosticsServiceRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let service = Arc::clone(&self.service);
        let timeout = request.timeout();
        let output = self
            .run_blocking(timeout, context, move |remaining, cancelled| {
                service.diagnostics(request.with_timeout(remaining), cancelled)
            })
            .await?
            .and_then(|output| {
                if output.parser_diagnostics_are_valid() {
                    Ok(crate::wire::compatibility_output(
                        crate::wire::CompatibilityAlias::IndexStatus,
                        output,
                    ))
                } else {
                    Err(RepositoryServiceError::Diagnostics)
                }
            });
        compatibility_operation_result(output, crate::wire::MAX_MCP_DIAGNOSTICS_OUTPUT_BYTES)
    }
}

fn compatibility_tools(annotations: &ToolAnnotations) -> Vec<Tool> {
    vec![
        compatibility_tool::<crate::wire::GetArchitectureInput, crate::wire::CompatibilityOutput<GraphArchitectureOutput>>(
            crate::wire::GET_ARCHITECTURE_ALIAS_TOOL_NAME,
            "Compatibility subset: return count-only Rust syntax-graph architecture with a versioned RepoWitness receipt.",
            annotations,
        ),
        compatibility_tool::<crate::wire::GetCodeSnippetInput, crate::wire::CompatibilityOutput<SymbolGetOutput>>(
            crate::wire::GET_CODE_SNIPPET_ALIAS_TOOL_NAME,
            "Compatibility subset: retrieve one exact digest-verified declaration with a versioned RepoWitness receipt.",
            annotations,
        ),
        compatibility_tool::<crate::wire::GetGraphSchemaInput, crate::wire::CompatibilityOutput<crate::wire::CompatibilityGraphSchema>>(
            crate::wire::GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME,
            "Compatibility subset: inspect graph kinds, limits, alias capability levels, limitations, and immutable status.",
            annotations,
        ),
        compatibility_tool::<crate::wire::IndexStatusInput, crate::wire::CompatibilityOutput<DiagnosticsOutput>>(
            crate::wire::INDEX_STATUS_ALIAS_TOOL_NAME,
            "Compatibility subset: inspect active immutable index status, coverage, freshness, and limitations.",
            annotations,
        ),
        compatibility_tool::<crate::wire::SearchCodeInput, crate::wire::CompatibilityOutput<CodeSearchOutput>>(
            crate::wire::SEARCH_CODE_ALIAS_TOOL_NAME,
            "Compatibility subset: run bounded literal code search with complete native evidence and coverage.",
            annotations,
        ),
        compatibility_tool::<crate::wire::SearchGraphInput, crate::wire::CompatibilityOutput<GraphSearchOutput>>(
            crate::wire::SEARCH_GRAPH_ALIAS_TOOL_NAME,
            "Compatibility subset: search exact Rust definition names with immutable selectors and coverage.",
            annotations,
        ),
        compatibility_tool::<crate::wire::TracePathInput, crate::wire::CompatibilityOutput<GraphTraceOutput>>(
            crate::wire::TRACE_PATH_ALIAS_TOOL_NAME,
            "Compatibility subset: traverse from an exact Rust selector to depth five with evidence and truncation.",
            annotations,
        ),
    ]
}

fn compatibility_tool<I, O>(
    name: &'static str,
    description: &'static str,
    annotations: &ToolAnnotations,
) -> Tool
where
    I: schemars::JsonSchema + 'static,
    O: schemars::JsonSchema + 'static,
{
    Tool::new(name, description, JsonObject::new())
        .with_input_schema::<I>()
        .with_output_schema::<O>()
        .annotate(annotations.clone())
}

fn parse_compatibility_input<T: DeserializeOwned>(
    arguments: Option<JsonObject>,
) -> Result<T, McpError> {
    parse_arguments(arguments)
}

fn invalid_compatibility_params(message: &'static str) -> McpError {
    McpError::invalid_params(message, None)
}

fn compatibility_operation_result<T: Serialize>(
    result: Result<T, RepositoryServiceError>,
    canonical_output_limit: usize,
) -> Result<CallToolResult, McpError> {
    const RECEIPT_ALLOWANCE: usize = 64 * 1024;
    operation_result(
        result,
        canonical_output_limit.saturating_add(RECEIPT_ALLOWANCE),
    )
}

fn server_instructions(surface: McpToolSurface, memory_writes_enabled: bool) -> String {
    let memory = if memory_writes_enabled {
        " memory_manage is explicitly enabled for one fixed local actor."
    } else {
        ""
    };
    let compatibility = if surface.includes_compatibility_aliases() {
        " Seven opt-in compatibility aliases are available; inspect each namespaced repowitness receipt before relying on it."
    } else {
        ""
    };
    format!(
        "RepoWitness MCP profile={} surface={}. Use context_build for deterministic source-and-memory context; use code_search/graph_search before exact retrieval or traversal. Results are generation-pinned and evidence-bearing.{compatibility}{memory}",
        surface.profile(),
        surface.identifier(),
    )
}
