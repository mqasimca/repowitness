fn run_evidence_balanced_context_build(
    invocation: ContextBuildInvocation,
    configuration: &ResolvedConfiguration,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let ContextBuildInvocation {
        invocation,
    } = invocation;
    let repository_identity = match invocation.repository_identity.to_str() {
        Some(value) => value,
        None => {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: context-build repository identity must be valid UTF-8\n",
            );
        }
    };
    let intent = match invocation.intent.to_str() {
        Some(value) => value,
        None => {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: context-build intent must be valid UTF-8\n",
            );
        }
    };
    let request = LocalEvidenceContextBuildRequest::new(
        &invocation.root,
        &invocation.database,
        repository_identity,
        intent,
    );
    let request = match request.with_budget_units(invocation.budget_units) {
        Ok(request) => request,
        Err(_) => {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: invalid context-build limits; use context-build --help\n",
            );
        }
    };
    let request = match request.with_max_provider_results(invocation.max_provider_results) {
        Ok(request) => request,
        Err(_) => {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: invalid context-build limits; use context-build --help\n",
            );
        }
    };
    match build_local_evidence_context(
        request.with_configuration(configuration),
        Arc::new(AtomicBool::new(false)),
    ) {
        Ok(result) => emit_evidence_context_report(stdout, &result),
        Err(_) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: context build failed\n",
        ),
    }
}

fn emit_evidence_context_report(
    writer: &mut impl Write,
    result: &repowitness_local::LocalEvidenceContextBuildResult,
) -> u8 {
    let mut encoded = Vec::new();
    if write_evidence_context_report(&mut encoded, result).is_err()
        || encoded.len() > MAX_CLI_CONTEXT_OUTPUT_BYTES
    {
        return EXIT_SOFTWARE;
    }
    if writer.write_all(&encoded).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn write_evidence_context_report(
    writer: &mut impl Write,
    result: &repowitness_local::LocalEvidenceContextBuildResult,
) -> std::io::Result<()> {
    let scope = result.scope();
    writeln!(writer, "status=ok")?;
    writeln!(writer, "operation=context-build")?;
    writeln!(writer, "profile_id={}", result.profile().id())?;
    writeln!(writer, "profile_version={}", result.profile().version())?;
    writeln!(writer, "budget_estimator=utf8_bytes_upper_bound_v1")?;
    writeln!(writer, "budget_units={}", result.budget().units())?;
    writeln!(writer, "used_units={}", result.used_units())?;
    writeln!(writer, "repository_sha256={}", hex(scope.repository().as_bytes()))?;
    writeln!(
        writer,
        "connected_workspace_sha256={}",
        hex(scope.connected_workspace().as_bytes())
    )?;
    writeln!(writer, "workspace_view={}", scope.workspace_view())?;
    writeln!(writer, "source_slot_sha256={}", hex(scope.source_slot().as_bytes()))?;
    writeln!(writer, "source_epoch={}", scope.source_epoch())?;
    writeln!(writer, "generation={}", scope.generation())?;
    writeln!(writer, "snapshot_sha256={}", hex(scope.snapshot().as_bytes()))?;
    writeln!(writer, "manifest_sha256={}", hex(scope.manifest().as_bytes()))?;
    writeln!(writer, "provider_coverage={}", result.provider_coverage().len())?;
    for (index, coverage) in result.provider_coverage().iter().enumerate() {
        writeln!(writer, "provider_coverage_{index}_tier={}", evidence_tier(coverage.tier()))?;
        let availability = match coverage.availability() {
            repowitness_local::EvidenceContextProviderAvailability::Available => "available",
            repowitness_local::EvidenceContextProviderAvailability::Unavailable => "unavailable",
        };
        writeln!(writer, "provider_coverage_{index}_availability={availability}")?;
        writeln!(writer, "provider_coverage_{index}_candidate_count={}", coverage.candidate_count())?;
    }
    writeln!(writer, "omissions={}", result.omissions().len())?;
    for (index, omission) in result.omissions().iter().enumerate() {
        writeln!(writer, "omission_{index}_tier={}", evidence_tier(omission.tier()))?;
        writeln!(writer, "omission_{index}_count={}", omission.count())?;
    }
    writeln!(writer, "items={}", result.items().len())?;
    for (index, item) in result.items().iter().enumerate() {
        write_evidence_context_item(writer, index, item)?;
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive payload encoder keeps the externally versioned evidence-balanced receipt shape auditable"
)]
fn write_evidence_context_item(
    writer: &mut impl Write,
    index: usize,
    item: &EvidenceContextCandidate<LocalEvidenceContextItem>,
) -> std::io::Result<()> {
    let prefix = format!("context_item_{index}");
    writeln!(writer, "{prefix}_tier={}", evidence_tier(item.tier()))?;
    writeln!(writer, "{prefix}_provider_rank={}", item.provider_rank())?;
    writeln!(writer, "{prefix}_estimated_units={}", item.estimated_units())?;
    writeln!(writer, "{prefix}_identity_sha256={}", hex(item.identity().as_bytes()))?;
    writeln!(writer, "{prefix}_providers={}", item.attributions().len())?;
    for (provider_index, attribution) in item.attributions().iter().enumerate() {
        writeln!(
            writer,
            "{prefix}_provider_{provider_index}_identity_sha256={}",
            hex(attribution.provider().as_bytes())
        )?;
        writeln!(
            writer,
            "{prefix}_provider_{provider_index}_tier={}",
            evidence_tier(attribution.tier())
        )?;
        writeln!(
            writer,
            "{prefix}_provider_{provider_index}_rank={}",
            attribution.provider_rank()
        )?;
    }
    match item.payload() {
        LocalEvidenceContextItem::Syntax(candidate) => {
            writeln!(writer, "{prefix}_kind=syntax")?;
            writeln!(writer, "{prefix}_path_encoding=lowercase_hex")?;
            writeln!(writer, "{prefix}_path_hex={}", hex(candidate.selector().path().as_bytes()))?;
            writeln!(
                writer,
                "{prefix}_content_sha256={}",
                hex(candidate.selector().content_digest().as_bytes())
            )?;
            writeln!(
                writer,
                "{prefix}_artifact_sha256={}",
                hex(candidate.selector().artifact_digest().as_bytes())
            )?;
            writeln!(writer, "{prefix}_fact_ordinal={}", candidate.selector().fact_ordinal())?;
            writeln!(
                writer,
                "{prefix}_declaration_encoding={}",
                encoded_source_bytes(candidate.declaration()).encoding
            )?;
            let declaration = encoded_source_bytes(candidate.declaration());
            let declaration = serde_json::to_string(&declaration.data)
                .map_err(std::io::Error::other)?;
            writeln!(writer, "{prefix}_declaration_data_json={declaration}")
        }
        LocalEvidenceContextItem::Memory(record) => {
            let memory = record
                .record()
                .ok_or_else(|| std::io::Error::other("invalid current memory payload"))?;
            writeln!(writer, "{prefix}_kind=memory")?;
            writeln!(
                writer,
                "{prefix}_record_id_sha256={}",
                hex(record.record_id().as_bytes())
            )?;
            let title = serde_json::to_string(memory.claim().title().as_str())
                .map_err(std::io::Error::other)?;
            let body = serde_json::to_string(memory.claim().body().as_str())
                .map_err(std::io::Error::other)?;
            writeln!(writer, "{prefix}_title_json={title}")?;
            writeln!(writer, "{prefix}_body_json={body}")
        }
        LocalEvidenceContextItem::History(item) => {
            let memory = item
                .record()
                .record()
                .ok_or_else(|| std::io::Error::other("invalid current history payload"))?;
            writeln!(writer, "{prefix}_kind=history")?;
            writeln!(writer, "{prefix}_record_id_sha256={}", hex(item.record().record_id().as_bytes()))?;
            let format = match item.commit().object_format() {
                repowitness_local::MemoryObjectFormat::Sha1 => "sha1",
                repowitness_local::MemoryObjectFormat::Sha256 => "sha256",
            };
            writeln!(writer, "{prefix}_commit_object_format={format}")?;
            writeln!(writer, "{prefix}_commit_object_id_hex={}", hex(item.commit().as_bytes()))?;
            let title = serde_json::to_string(memory.claim().title().as_str())
                .map_err(std::io::Error::other)?;
            let body = serde_json::to_string(memory.claim().body().as_str())
                .map_err(std::io::Error::other)?;
            writeln!(writer, "{prefix}_title_json={title}")?;
            writeln!(writer, "{prefix}_body_json={body}")
        }
        LocalEvidenceContextItem::PreciseOverlay(item) => {
            let occurrence = item.occurrence();
            writeln!(writer, "{prefix}_kind=precise_overlay")?;
            writeln!(writer, "{prefix}_overlay_sha256={}", hex(item.overlay().digest().as_bytes()))?;
            writeln!(writer, "{prefix}_path_encoding=lowercase_hex")?;
            writeln!(writer, "{prefix}_path_hex={}", hex(occurrence.path().as_bytes()))?;
            writeln!(writer, "{prefix}_content_sha256={}", hex(occurrence.content().as_bytes()))?;
            writeln!(writer, "{prefix}_span_start={}", occurrence.span().start().get())?;
            writeln!(writer, "{prefix}_span_end={}", occurrence.span().end().get())?;
            writeln!(writer, "{prefix}_roles={}", occurrence.roles().bits())?;
            writeln!(writer, "{prefix}_relationship_count={}", item.relationship_count())?;
            let source = encoded_source_bytes(item.source());
            writeln!(writer, "{prefix}_source_encoding={}", source.encoding)?;
            let source = serde_json::to_string(&source.data).map_err(std::io::Error::other)?;
            writeln!(writer, "{prefix}_source_data_json={source}")
        }
        LocalEvidenceContextItem::GraphRelation(item) => {
            let candidate = item.candidate();
            writeln!(writer, "{prefix}_kind=graph_relation")?;
            writeln!(writer, "{prefix}_edge_kind={}", item.edge_kind().as_str())?;
            writeln!(writer, "{prefix}_depth={}", item.depth())?;
            writeln!(writer, "{prefix}_path_encoding=lowercase_hex")?;
            writeln!(writer, "{prefix}_path_hex={}", hex(candidate.selector().path().as_bytes()))?;
            writeln!(writer, "{prefix}_content_sha256={}", hex(candidate.selector().content_digest().as_bytes()))?;
            writeln!(writer, "{prefix}_artifact_sha256={}", hex(candidate.selector().artifact_digest().as_bytes()))?;
            writeln!(writer, "{prefix}_fact_ordinal={}", candidate.selector().fact_ordinal())?;
            let declaration = encoded_source_bytes(candidate.declaration());
            writeln!(writer, "{prefix}_declaration_encoding={}", declaration.encoding)?;
            let declaration = serde_json::to_string(&declaration.data).map_err(std::io::Error::other)?;
            writeln!(writer, "{prefix}_declaration_data_json={declaration}")
        }
    }
}

fn evidence_tier(tier: EvidenceContextTier) -> &'static str {
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
