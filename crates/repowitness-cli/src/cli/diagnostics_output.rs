fn emit_diagnostics_report(writer: &mut impl Write, report: &DiagnosticsOutput) -> u8 {
    if report.known_parser_limitation_nodes > report.syntax_error_nodes {
        return EXIT_SOFTWARE;
    }
    let mut encoded = Vec::new();
    if write_diagnostics_report(&mut encoded, report).is_err()
        || encoded.len() > MAX_CLI_DIAGNOSTICS_OUTPUT_BYTES
    {
        return EXIT_SOFTWARE;
    }
    if writer.write_all(&encoded).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn write_diagnostics_report(
    writer: &mut impl Write,
    report: &DiagnosticsOutput,
) -> std::io::Result<()> {
    writeln!(writer, "status=ok")?;
    writeln!(writer, "operation=diagnostics")?;
    writeln!(writer, "schema_version={}", report.schema_version)?;
    writeln!(writer, "diagnostics_profile={}", report.diagnostics_profile)?;
    writeln!(
        writer,
        "configuration_digest_sha256={}",
        report.configuration.digest_sha256
    )?;
    writeln!(
        writer,
        "configuration_schema_version={}",
        report.configuration.schema_version
    )?;
    writeln!(
        writer,
        "configuration_resolver_version={}",
        report.configuration.resolver_version
    )?;
    writeln!(
        writer,
        "configuration_profile={}",
        report.configuration.profile
    )?;
    writeln!(writer, "snapshot_sha256={}", report.snapshot_sha256)?;
    writeln!(writer, "generation={}", report.generation)?;
    writeln!(writer, "source_epoch={}", report.source_epoch)?;
    writeln!(
        writer,
        "producer_manifest_sha256={}",
        report.producer_manifest_sha256
    )?;
    write_diagnostics_index_coverage(writer, report.index_coverage)?;
    writeln!(writer, "syntax_error_nodes={}", report.syntax_error_nodes)?;
    writeln!(
        writer,
        "known_parser_limitation_nodes={}",
        report.known_parser_limitation_nodes
    )?;
    write_diagnostics_memory(writer, report.memory_projection.as_ref())?;
    write_diagnostics_labels(
        writer,
        "supported_languages",
        "supported_language",
        &report.supported_languages,
    )?;
    write_diagnostics_labels(writer, "capabilities", "capability", &report.capabilities)?;
    write_diagnostics_labels(writer, "limitations", "limitation", &report.limitations)
}

fn write_diagnostics_index_coverage(
    writer: &mut impl Write,
    coverage: McpCoverage,
) -> std::io::Result<()> {
    writeln!(writer, "index_searched={}", coverage.searched)?;
    writeln!(writer, "index_skipped={}", coverage.skipped)?;
    writeln!(writer, "index_unresolved={}", coverage.unresolved)?;
    writeln!(writer, "index_truncated={}", coverage.truncated)
}

fn write_diagnostics_memory(
    writer: &mut impl Write,
    memory: Option<&McpDiagnosticsMemoryProjection>,
) -> std::io::Result<()> {
    writeln!(writer, "memory_projection_available={}", memory.is_some())?;
    let Some(memory) = memory else {
        return Ok(());
    };
    writeln!(writer, "memory_projection={}", memory.projection)?;
    writeln!(writer, "memory_source_epoch={}", memory.source_epoch)?;
    writeln!(writer, "memory_snapshot_sha256={}", memory.snapshot_sha256)?;
    let coverage = memory.coverage;
    writeln!(writer, "memory_searched={}", coverage.searched)?;
    writeln!(writer, "memory_skipped={}", coverage.skipped)?;
    writeln!(writer, "memory_unresolved={}", coverage.unresolved)?;
    writeln!(writer, "memory_truncated={}", coverage.truncated)?;
    writeln!(writer, "memory_total={}", coverage.total)?;
    writeln!(writer, "memory_current={}", coverage.current)?;
    writeln!(writer, "memory_not_applicable={}", coverage.not_applicable)?;
    writeln!(writer, "memory_stale={}", coverage.stale)?;
    writeln!(writer, "memory_needs_review={}", coverage.needs_review)?;
    writeln!(writer, "memory_indeterminate={}", coverage.indeterminate)?;
    writeln!(writer, "memory_conflicted={}", coverage.conflicted)?;
    writeln!(writer, "memory_contradicted={}", coverage.contradicted)?;
    writeln!(writer, "memory_superseded={}", coverage.superseded)?;
    writeln!(writer, "memory_quarantined={}", coverage.quarantined)?;
    writeln!(writer, "memory_tombstoned={}", coverage.tombstoned)
}

fn write_diagnostics_labels(
    writer: &mut impl Write,
    count_label: &str,
    item_label: &str,
    values: &[String],
) -> std::io::Result<()> {
    writeln!(writer, "{count_label}={}", values.len())?;
    for (index, value) in values.iter().enumerate() {
        writeln!(writer, "{item_label}_{index}={value}")?;
    }
    Ok(())
}
