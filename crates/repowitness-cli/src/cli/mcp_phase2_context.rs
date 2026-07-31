fn mcp_phase2_context_output(
    result: repowitness_local::LocalPhase2ContextBuildResult,
) -> Result<Phase2ContextBuildOutput, String> {
    let scope = result.scope();
    interoperable_i64(&[scope.workspace_view(), scope.generation()])?;
    interoperable(&[scope.source_epoch()])?;
    let omissions = result
        .omissions()
        .iter()
        .map(|omission| McpPhase2ContextOmission {
            tier: phase2_context_tier(omission.tier()).to_owned(),
            count: omission.count(),
        })
        .collect();
    let provider_coverage = result
        .provider_coverage()
        .iter()
        .map(|coverage| McpPhase2ContextProviderCoverage {
            tier: phase2_context_tier(coverage.tier()).to_owned(),
            availability: phase2_provider_availability(coverage.availability()).to_owned(),
            candidate_count: coverage.candidate_count(),
        })
        .collect();
    let items = result
        .items()
        .iter()
        .map(mcp_phase2_context_item)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Phase2ContextBuildOutput {
        schema_version: 1,
        profile_id: result.profile().id().to_owned(),
        profile_version: result.profile().version(),
        budget_estimator: "utf8_bytes_upper_bound_v1".to_owned(),
        budget_units: result.budget().units(),
        used_units: result.used_units(),
        scope: McpPhase2ContextScope {
            repository_sha256: hex(scope.repository().as_bytes()),
            connected_workspace_sha256: hex(scope.connected_workspace().as_bytes()),
            workspace_view: scope.workspace_view(),
            source_slot_sha256: hex(scope.source_slot().as_bytes()),
            source_epoch: scope.source_epoch(),
            generation: scope.generation(),
            snapshot_sha256: hex(scope.snapshot().as_bytes()),
            manifest_sha256: hex(scope.manifest().as_bytes()),
        },
        provider_coverage,
        omissions,
        items,
    })
}

fn phase2_provider_availability(
    availability: repowitness_local::Phase2ContextProviderAvailability,
) -> &'static str {
    match availability {
        repowitness_local::Phase2ContextProviderAvailability::Available => "available",
        repowitness_local::Phase2ContextProviderAvailability::Unavailable => "unavailable",
    }
}

fn mcp_phase2_context_item(
    item: &Phase2ContextCandidate<LocalPhase2ContextItem>,
) -> Result<McpPhase2ContextItem, String> {
    let providers = item
        .attributions()
        .iter()
        .map(|attribution| McpPhase2ContextAttribution {
            provider_sha256: hex(attribution.provider().as_bytes()),
            tier: phase2_context_tier(attribution.tier()).to_owned(),
            provider_rank: attribution.provider_rank(),
        })
        .collect();
    let payload = match item.payload() {
        LocalPhase2ContextItem::Syntax(candidate) => {
            let declaration = encoded_source_bytes(candidate.declaration());
            McpPhase2ContextPayload::Syntax {
                path: RepositoryPathTextV1::encode(candidate.selector().path(), PATH_TEXT_LIMIT)
                    .map_err(|error| error.to_string())?
                    .into_string(),
                content_sha256: hex(candidate.selector().content_digest().as_bytes()),
                artifact_sha256: hex(candidate.selector().artifact_digest().as_bytes()),
                fact_ordinal: candidate.selector().fact_ordinal(),
                declaration_encoding: declaration.encoding.to_owned(),
                declaration: declaration.data,
            }
        }
        LocalPhase2ContextItem::Memory(record) => McpPhase2ContextPayload::Memory {
            record_id_sha256: hex(record.record_id().as_bytes()),
            record: Box::new(mcp_memory_record(record)?),
        },
        LocalPhase2ContextItem::History(item) => McpPhase2ContextPayload::History {
            record_id_sha256: hex(item.record().record_id().as_bytes()),
            commit_object_format: match item.commit().object_format() {
                repowitness_local::MemoryObjectFormat::Sha1 => "sha1",
                repowitness_local::MemoryObjectFormat::Sha256 => "sha256",
            }
            .to_owned(),
            commit_object_id_hex: hex(item.commit().as_bytes()),
            record: Box::new(mcp_memory_record(item.record())?),
        },
        LocalPhase2ContextItem::PreciseOverlay(item) => {
            let occurrence = item.occurrence();
            let source = encoded_source_bytes(item.source());
            McpPhase2ContextPayload::PreciseOverlay {
                overlay_sha256: hex(item.overlay().digest().as_bytes()),
                path: RepositoryPathTextV1::encode(occurrence.path(), PATH_TEXT_LIMIT)
                    .map_err(|error| error.to_string())?
                    .into_string(),
                content_sha256: hex(occurrence.content().as_bytes()),
                span_start: occurrence.span().start().get(),
                span_end: occurrence.span().end().get(),
                roles: occurrence.roles().bits(),
                relationship_count: item.relationship_count(),
                source_encoding: source.encoding.to_owned(),
                source: source.data,
            }
        }
        LocalPhase2ContextItem::GraphRelation(item) => {
            let candidate = item.candidate();
            let declaration = encoded_source_bytes(candidate.declaration());
            McpPhase2ContextPayload::GraphRelation {
                edge_kind: item.edge_kind().as_str().to_owned(),
                depth: item.depth(),
                path: RepositoryPathTextV1::encode(candidate.selector().path(), PATH_TEXT_LIMIT)
                    .map_err(|error| error.to_string())?
                    .into_string(),
                content_sha256: hex(candidate.selector().content_digest().as_bytes()),
                artifact_sha256: hex(candidate.selector().artifact_digest().as_bytes()),
                fact_ordinal: candidate.selector().fact_ordinal(),
                declaration_encoding: declaration.encoding.to_owned(),
                declaration: declaration.data,
            }
        }
    };
    Ok(McpPhase2ContextItem {
        tier: phase2_context_tier(item.tier()).to_owned(),
        provider_rank: item.provider_rank(),
        estimated_units: item.estimated_units(),
        identity_sha256: hex(item.identity().as_bytes()),
        providers,
        payload,
    })
}

fn phase2_context_tier(tier: Phase2ContextTier) -> &'static str {
    match tier {
        Phase2ContextTier::Anchor => "anchor",
        Phase2ContextTier::PreciseOverlay => "precise_overlay",
        Phase2ContextTier::Syntax => "syntax",
        Phase2ContextTier::Structural => "structural",
        Phase2ContextTier::References => "references",
        Phase2ContextTier::Memory => "memory",
        Phase2ContextTier::History => "history",
        Phase2ContextTier::Unresolved => "unresolved",
    }
}
