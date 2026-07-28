fn emit_context_report(writer: &mut impl Write, report: &ContextBuildOutput) -> u8 {
    let mut encoded = Vec::new();
    if write_context_report(&mut encoded, report).is_err()
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

fn write_context_report(
    writer: &mut impl Write,
    report: &ContextBuildOutput,
) -> std::io::Result<()> {
    writeln!(writer, "status=ok")?;
    writeln!(writer, "operation=context-build")?;
    writeln!(writer, "schema_version={}", report.schema_version)?;
    writeln!(writer, "context_profile={}", report.context_profile)?;
    writeln!(writer, "reciprocal_rank_k={}", report.reciprocal_rank_k)?;
    writeln!(writer, "budget_estimator={}", report.budget_estimator)?;
    writeln!(writer, "budget_units={}", report.budget_units)?;
    writeln!(writer, "used_units={}", report.used_units)?;
    writeln!(writer, "query_sha256={}", report.query_sha256)?;
    writeln!(writer, "snapshot_sha256={}", report.snapshot_sha256)?;
    writeln!(writer, "generation={}", report.generation)?;
    write_context_memory_projection(writer, report)?;
    write_context_coverage(writer, report.coverage)?;
    writeln!(writer, "omissions={}", report.omissions.len())?;
    for (index, omission) in report.omissions.iter().enumerate() {
        writeln!(writer, "omission_{index}_kind={}", omission.kind)?;
        writeln!(
            writer,
            "omission_{index}_provider={}",
            omission.provider.as_deref().unwrap_or("none")
        )?;
        writeln!(
            writer,
            "omission_{index}_count={}",
            omission
                .count
                .map_or_else(|| "none".to_owned(), |count| count.to_string())
        )?;
    }
    writeln!(writer, "items={}", report.items.len())?;
    for (index, item) in report.items.iter().enumerate() {
        write_context_item(writer, index, item)?;
    }
    Ok(())
}

fn write_context_memory_projection(
    writer: &mut impl Write,
    report: &ContextBuildOutput,
) -> std::io::Result<()> {
    writeln!(
        writer,
        "memory_projection_available={}",
        report.memory.is_some()
    )?;
    let Some(memory) = &report.memory else {
        return Ok(());
    };
    writeln!(writer, "memory_projection={}", memory.projection)?;
    writeln!(writer, "memory_source_epoch={}", memory.source_epoch)?;
    writeln!(writer, "memory_producer_id={}", memory.producer.id)?;
    writeln!(
        writer,
        "memory_producer_version={}",
        memory.producer.version
    )?;
    writeln!(
        writer,
        "memory_producer_profile_sha256={}",
        memory.producer.profile_sha256
    )?;
    let coverage = memory.coverage;
    writeln!(writer, "memory_projection_searched={}", coverage.searched)?;
    writeln!(writer, "memory_projection_skipped={}", coverage.skipped)?;
    writeln!(
        writer,
        "memory_projection_unresolved={}",
        coverage.unresolved
    )?;
    writeln!(
        writer,
        "memory_projection_truncated={}",
        coverage.truncated
    )?;
    writeln!(writer, "memory_projection_total={}", coverage.total)?;
    writeln!(writer, "memory_projection_current={}", coverage.current)?;
    writeln!(
        writer,
        "memory_projection_not_applicable={}",
        coverage.not_applicable
    )?;
    writeln!(writer, "memory_projection_stale={}", coverage.stale)?;
    writeln!(
        writer,
        "memory_projection_needs_review={}",
        coverage.needs_review
    )?;
    writeln!(
        writer,
        "memory_projection_indeterminate={}",
        coverage.indeterminate
    )?;
    writeln!(
        writer,
        "memory_projection_conflicted={}",
        coverage.conflicted
    )?;
    writeln!(
        writer,
        "memory_projection_contradicted={}",
        coverage.contradicted
    )?;
    writeln!(
        writer,
        "memory_projection_superseded={}",
        coverage.superseded
    )?;
    writeln!(
        writer,
        "memory_projection_quarantined={}",
        coverage.quarantined
    )?;
    writeln!(
        writer,
        "memory_projection_tombstoned={}",
        coverage.tombstoned
    )
}

fn write_context_coverage(
    writer: &mut impl Write,
    coverage: McpContextCoverage,
) -> std::io::Result<()> {
    writeln!(
        writer,
        "source_index_searched={}",
        coverage.source_index.searched
    )?;
    writeln!(
        writer,
        "source_index_skipped={}",
        coverage.source_index.skipped
    )?;
    writeln!(
        writer,
        "source_index_unresolved={}",
        coverage.source_index.unresolved
    )?;
    writeln!(
        writer,
        "source_index_truncated={}",
        coverage.source_index.truncated
    )?;
    writeln!(
        writer,
        "source_total_matches={}",
        coverage.source_total_matches
    )?;
    writeln!(
        writer,
        "source_returned_matches={}",
        coverage.source_returned_matches
    )?;
    writeln!(
        writer,
        "source_expansion_omitted={}",
        coverage.source_expansion_omitted
    )?;
    writeln!(
        writer,
        "source_budget_omitted={}",
        coverage.source_budget_omitted
    )?;
    writeln!(writer, "source_included={}", coverage.source_included)?;
    writeln!(
        writer,
        "memory_total_matches={}",
        coverage.memory_total_matches
    )?;
    writeln!(
        writer,
        "memory_returned_matches={}",
        coverage.memory_returned_matches
    )?;
    writeln!(
        writer,
        "memory_non_current_omitted={}",
        coverage.memory_non_current_omitted
    )?;
    writeln!(
        writer,
        "memory_budget_omitted={}",
        coverage.memory_budget_omitted
    )?;
    writeln!(writer, "memory_included={}", coverage.memory_included)
}

fn write_context_item(
    writer: &mut impl Write,
    index: usize,
    item: &McpContextItem,
) -> std::io::Result<()> {
    match item {
        McpContextItem::Memory(item) => {
            write_context_rank(
                writer,
                index,
                "memory",
                item.provider_rank,
                item.fused_rank,
                item.reciprocal_rank_denominator,
                item.estimated_units,
            )?;
            write_memory_record(writer, index, &item.record)
        }
        McpContextItem::Source(item) => {
            write_context_rank(
                writer,
                index,
                "source",
                item.provider_rank,
                item.fused_rank,
                item.reciprocal_rank_denominator,
                item.estimated_units,
            )?;
            let prefix = format!("context_item_{index}");
            writeln!(writer, "{prefix}_path={}", item.path)?;
            writeln!(
                writer,
                "{prefix}_content_sha256={}",
                item.content_sha256
            )?;
            writeln!(
                writer,
                "{prefix}_artifact_sha256={}",
                item.artifact_sha256
            )?;
            writeln!(writer, "{prefix}_fact_ordinal={}", item.fact_ordinal)?;
            writeln!(
                writer,
                "{prefix}_producer_manifest_sha256={}",
                item.producer_manifest_sha256
            )?;
            writeln!(writer, "{prefix}_language={}", item.language)?;
            writeln!(
                writer,
                "{prefix}_declaration_kind={}",
                item.declaration_kind
            )?;
            writeln!(writer, "{prefix}_name_encoding=lowercase_hex")?;
            writeln!(
                writer,
                "{prefix}_name_hex={}",
                hex(item.name.as_bytes())
            )?;
            writeln!(writer, "{prefix}_qualified_name_encoding=lowercase_hex")?;
            writeln!(
                writer,
                "{prefix}_qualified_name_hex={}",
                hex(item.qualified_name.as_bytes())
            )?;
            writeln!(
                writer,
                "{prefix}_name_span={}:{}",
                item.name_span.start, item.name_span.end
            )?;
            writeln!(
                writer,
                "{prefix}_declaration_span={}:{}",
                item.declaration_span.start, item.declaration_span.end
            )?;
            writeln!(
                writer,
                "{prefix}_declaration_encoding={}",
                item.declaration_encoding
            )?;
            let declaration = serde_json::to_string(&item.declaration)
                .map_err(std::io::Error::other)?;
            writeln!(writer, "{prefix}_declaration_data_json={declaration}")
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the emitted rank fields are a single fixed wire tuple"
)]
fn write_context_rank(
    writer: &mut impl Write,
    index: usize,
    kind: &str,
    provider_rank: u16,
    fused_rank: u16,
    denominator: u16,
    estimated_units: u64,
) -> std::io::Result<()> {
    let prefix = format!("context_item_{index}");
    writeln!(writer, "{prefix}_kind={kind}")?;
    writeln!(writer, "{prefix}_provider_rank={provider_rank}")?;
    writeln!(writer, "{prefix}_fused_rank={fused_rank}")?;
    writeln!(
        writer,
        "{prefix}_reciprocal_rank_denominator={denominator}"
    )?;
    writeln!(writer, "{prefix}_estimated_units={estimated_units}")
}
