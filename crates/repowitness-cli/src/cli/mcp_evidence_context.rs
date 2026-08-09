fn mcp_evidence_context_output(
    result: repowitness_local::LocalEvidenceContextBuildResult,
) -> Result<EvidenceContextBuildOutput, String> {
    let scope = result.scope();
    interoperable_i64(&[scope.workspace_view(), scope.generation()])?;
    interoperable(&[scope.source_epoch()])?;
    let omissions = result
        .omissions()
        .iter()
        .map(|omission| McpEvidenceContextOmission {
            tier: evidence_context_tier(omission.tier()).to_owned(),
            count: omission.count(),
        })
        .collect();
    let provider_coverage = result
        .provider_coverage()
        .iter()
        .map(|coverage| McpEvidenceContextProviderCoverage {
            tier: evidence_context_tier(coverage.tier()).to_owned(),
            availability: evidence_provider_availability(coverage.availability()).to_owned(),
            candidate_count: coverage.candidate_count(),
        })
        .collect();
    let items = result
        .items()
        .iter()
        .map(mcp_evidence_context_item)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EvidenceContextBuildOutput {
        schema_version: 1,
        profile_id: result.profile().id().to_owned(),
        profile_version: result.profile().version(),
        budget_estimator: "utf8_bytes_upper_bound_v1".to_owned(),
        budget_units: result.budget().units(),
        used_units: result.used_units(),
        scope: McpEvidenceContextScope {
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

fn evidence_provider_availability(
    availability: repowitness_local::EvidenceContextProviderAvailability,
) -> &'static str {
    match availability {
        repowitness_local::EvidenceContextProviderAvailability::Available => "available",
        repowitness_local::EvidenceContextProviderAvailability::Unavailable => "unavailable",
    }
}

fn mcp_evidence_context_item(
    item: &EvidenceContextCandidate<LocalEvidenceContextItem>,
) -> Result<McpEvidenceContextItem, String> {
    let providers = item
        .attributions()
        .iter()
        .map(|attribution| McpEvidenceContextAttribution {
            provider_sha256: hex(attribution.provider().as_bytes()),
            tier: evidence_context_tier(attribution.tier()).to_owned(),
            provider_rank: attribution.provider_rank(),
        })
        .collect();
    let payload = match item.payload() {
        LocalEvidenceContextItem::Syntax(candidate) => {
            let declaration = encoded_source_bytes(candidate.declaration());
            McpEvidenceContextPayload::Syntax {
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
        LocalEvidenceContextItem::Memory(record) => McpEvidenceContextPayload::Memory {
            record_id_sha256: hex(record.record_id().as_bytes()),
            record: Box::new(mcp_memory_record(record)?),
        },
        LocalEvidenceContextItem::History(item) => McpEvidenceContextPayload::History {
            record_id_sha256: hex(item.record().record_id().as_bytes()),
            commit_object_format: match item.commit().object_format() {
                repowitness_local::MemoryObjectFormat::Sha1 => "sha1",
                repowitness_local::MemoryObjectFormat::Sha256 => "sha256",
            }
            .to_owned(),
            commit_object_id_hex: hex(item.commit().as_bytes()),
            record: Box::new(mcp_memory_record(item.record())?),
        },
        LocalEvidenceContextItem::PreciseOverlay(item) => {
            let occurrence = item.occurrence();
            let source = encoded_source_bytes(item.source());
            McpEvidenceContextPayload::PreciseOverlay {
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
        LocalEvidenceContextItem::GraphRelation(item) => {
            let candidate = item.candidate();
            let declaration = encoded_source_bytes(candidate.declaration());
            McpEvidenceContextPayload::GraphRelation {
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
    Ok(McpEvidenceContextItem {
        tier: evidence_context_tier(item.tier()).to_owned(),
        provider_rank: item.provider_rank(),
        estimated_units: item.estimated_units(),
        identity_sha256: hex(item.identity().as_bytes()),
        providers,
        payload,
    })
}

fn evidence_context_tier(tier: EvidenceContextTier) -> &'static str {
    match tier {
        EvidenceContextTier::Anchor => "anchor",
        EvidenceContextTier::PreciseOverlay => "precise_overlay",
        EvidenceContextTier::Syntax => "syntax",
        EvidenceContextTier::Structural => "structural",
        EvidenceContextTier::References => "references",
        EvidenceContextTier::Memory => "memory",
        EvidenceContextTier::History => "history",
        EvidenceContextTier::Unresolved => "unresolved",
    }
}
