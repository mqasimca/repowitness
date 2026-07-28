struct ContextInvocation {
    root: PathBuf,
    database: PathBuf,
    repository_identity: OsString,
    intent: OsString,
    budget_units: u64,
    max_provider_results: u16,
}

trait RepositoryContextBuilder {
    fn build(&self, invocation: &ContextInvocation) -> Result<ContextBuildOutput, String>;
}

struct LocalRepositoryContextBuilder;

impl RepositoryContextBuilder for LocalRepositoryContextBuilder {
    fn build(&self, invocation: &ContextInvocation) -> Result<ContextBuildOutput, String> {
        let repository_identity = invocation
            .repository_identity
            .to_str()
            .ok_or_else(|| "context repository identity is not valid UTF-8".to_owned())?;
        let intent = invocation
            .intent
            .to_str()
            .ok_or_else(|| "context intent is not valid UTF-8".to_owned())?;
        let request = LocalContextBuildRequest::new(
            &invocation.root,
            &invocation.database,
            repository_identity,
            intent,
        )
        .with_budget_units(invocation.budget_units)
        .map_err(|error| error.to_string())?
        .with_max_provider_results(invocation.max_provider_results)
        .map_err(|error| error.to_string())?;
        build_local_context(request, Arc::new(AtomicBool::new(false)))
            .map_err(|error| error.to_string())
            .and_then(mcp_context_output)
    }
}

fn mcp_context_output(result: LocalContextBuildResult) -> Result<ContextBuildOutput, String> {
    let coverage = result.coverage();
    let source_index = coverage.source_index();
    let memory = result
        .memory()
        .map(|memory| McpContextMemoryProjection {
            projection: *memory.projection(),
            source_epoch: memory.source_epoch(),
            producer: McpMemoryProducer {
                id: memory.producer().id().to_owned(),
                version: memory.producer().version(),
                profile_sha256: hex(memory.producer().digest().as_bytes()),
            },
            coverage: mcp_memory_coverage(memory.coverage()),
        });
    let mut omissions = Vec::with_capacity(result.omissions().len());
    for omission in result.omissions() {
        omissions.push(mcp_context_omission(*omission));
    }
    let mut items = Vec::with_capacity(result.items().len());
    for item in result.items() {
        items.push(mcp_context_item(item)?);
    }
    Ok(ContextBuildOutput {
        schema_version: 1,
        context_profile: result.profile_version(),
        reciprocal_rank_k: CONTEXT_BUILD_RRF_K,
        budget_estimator: result.budget_estimator().label().to_owned(),
        budget_units: result.budget().units(),
        used_units: result.used_units(),
        query_sha256: hex(result.query().as_bytes()),
        snapshot_sha256: hex(result.snapshot().as_bytes()),
        generation: result.generation().get(),
        memory,
        coverage: McpContextCoverage {
            source_index: McpCoverage {
                searched: source_index.searched().get(),
                skipped: source_index.skipped().get(),
                unresolved: source_index.unresolved().get(),
                truncated: source_index.truncated().get(),
            },
            source_total_matches: coverage.source_total_matches(),
            source_returned_matches: coverage.source_returned_matches(),
            source_expansion_omitted: coverage.source_expansion_omitted(),
            source_budget_omitted: coverage.source_budget_omitted(),
            source_included: coverage.source_included(),
            memory_total_matches: coverage.memory_total_matches(),
            memory_returned_matches: coverage.memory_returned_matches(),
            memory_non_current_omitted: coverage.memory_non_current_omitted(),
            memory_budget_omitted: coverage.memory_budget_omitted(),
            memory_included: coverage.memory_included(),
        },
        omissions,
        items,
    })
}

fn mcp_context_omission(omission: ContextOmission) -> McpContextOmission {
    match omission {
        ContextOmission::SourceSearchLimit(count) => McpContextOmission {
            kind: "source_search_limit".to_owned(),
            provider: Some("source".to_owned()),
            count: Some(count),
        },
        ContextOmission::SourceExpansionLimit(count) => McpContextOmission {
            kind: "source_expansion_limit".to_owned(),
            provider: Some("source".to_owned()),
            count: Some(count),
        },
        ContextOmission::MemoryProjectionUnavailable => McpContextOmission {
            kind: "memory_projection_unavailable".to_owned(),
            provider: Some("memory".to_owned()),
            count: None,
        },
        ContextOmission::MemoryRecallLimit(count) => McpContextOmission {
            kind: "memory_recall_limit".to_owned(),
            provider: Some("memory".to_owned()),
            count: Some(count),
        },
        ContextOmission::MemoryNotCurrent(count) => McpContextOmission {
            kind: "memory_not_current".to_owned(),
            provider: Some("memory".to_owned()),
            count: Some(count),
        },
        ContextOmission::Budget { provider, count } => McpContextOmission {
            kind: "budget".to_owned(),
            provider: Some(context_provider(provider).to_owned()),
            count: Some(count),
        },
        ContextOmission::ProviderUnavailable(provider) => McpContextOmission {
            kind: "provider_unavailable".to_owned(),
            provider: Some(context_provider(provider).to_owned()),
            count: None,
        },
    }
}

fn mcp_context_item(item: &ContextItem) -> Result<McpContextItem, String> {
    match item {
        ContextItem::Memory(item) => {
            let rank = item.rank();
            Ok(McpContextItem::Memory(McpContextMemoryItem {
                provider_rank: rank.provider_rank(),
                fused_rank: rank.fused_rank(),
                reciprocal_rank_denominator: rank.reciprocal_rank_denominator(),
                estimated_units: item.estimated_units(),
                record: mcp_memory_record(item.record())?,
            }))
        }
        ContextItem::Source(item) => {
            let rank = item.rank();
            let candidate = item.candidate();
            let selector = candidate.selector();
            let occurrence = candidate.occurrence();
            Ok(McpContextItem::Source(McpContextSourceItem {
                provider_rank: rank.provider_rank(),
                fused_rank: rank.fused_rank(),
                reciprocal_rank_denominator: rank.reciprocal_rank_denominator(),
                estimated_units: item.estimated_units(),
                path: RepositoryPathTextV1::encode(selector.path(), PATH_TEXT_LIMIT)
                    .map_err(|error| error.to_string())?
                    .into_string(),
                content_sha256: hex(selector.content_digest().as_bytes()),
                artifact_sha256: hex(selector.artifact_digest().as_bytes()),
                fact_ordinal: selector.fact_ordinal(),
                producer_manifest_sha256: hex(occurrence.producer_manifest().as_bytes()),
                language: occurrence.language().as_str().to_owned(),
                declaration_kind: occurrence.kind().as_str().to_owned(),
                name: occurrence.name().to_owned(),
                qualified_name: occurrence.qualified_name().to_owned(),
                name_span: McpSpan {
                    start: occurrence.name_span().start().get(),
                    end: occurrence.name_span().end().get(),
                },
                declaration_span: McpSpan {
                    start: occurrence.declaration_span().start().get(),
                    end: occurrence.declaration_span().end().get(),
                },
                declaration_encoding: "lowercase_hex".to_owned(),
                declaration_hex: hex(candidate.declaration()),
            }))
        }
    }
}

fn context_provider(provider: ContextProvider) -> &'static str {
    match provider {
        ContextProvider::Memory => "memory",
        ContextProvider::Source => "source",
        ContextProvider::Structural => "structural",
        ContextProvider::References => "references",
        ContextProvider::History => "history",
    }
}
