fn emit_memory_error(
    writer: &mut impl Write,
    generic_message: &'static str,
    error: CliMemoryError,
) -> u8 {
    let CliMemoryError::MutationOutcomeUnknown {
        request_scope,
        operation,
    } = error
    else {
        return emit_error(writer, EXIT_SOFTWARE, generic_message);
    };
    let guidance = if operation == MemoryMutationOperation::UnknownPhase {
        request_scope.reconciliation_guidance()
    } else {
        operation.reconciliation_guidance()
    };
    let result = writeln!(
        writer,
        "error: memory mutation outcome could not be determined"
    )
    .and_then(|()| writeln!(writer, "request_scope={}", request_scope.as_str()))
    .and_then(|()| writeln!(writer, "operation={}", operation.as_str()))
    .and_then(|()| writeln!(writer, "reconciliation_required_before_retry={}", guidance))
    .and_then(|()| writeln!(writer, "automatic_retry=false"));
    if result.is_ok() {
        EXIT_SOFTWARE
    } else {
        EXIT_IO
    }
}

fn emit_memory_receipt_delivery_failure(
    writer: &mut impl Write,
    request_scope: MemoryMutationRequestScope,
    operation: MemoryMutationOperation,
) -> u8 {
    let guidance = operation.reconciliation_guidance();
    let _ = writeln!(
        writer,
        "error: committed memory receipt could not be written"
    )
    .and_then(|()| writeln!(writer, "request_scope={}", request_scope.as_str()))
    .and_then(|()| writeln!(writer, "operation={}", operation.as_str()))
    .and_then(|()| writeln!(writer, "reconciliation_required_before_retry={}", guidance))
    .and_then(|()| writeln!(writer, "automatic_retry=false"));
    EXIT_IO
}

fn emit_memory_revalidation_report(
    writer: &mut impl Write,
    stderr: &mut impl Write,
    report: CliMemoryRevalidationReport,
) -> u8 {
    let result = writeln!(
        writer,
        "status={}",
        if report.maintenance.complete {
            "ok"
        } else {
            "warning"
        }
    )
    .and_then(|()| writeln!(writer, "operation=memory-revalidate"))
    .and_then(|()| writeln!(writer, "projection_activated=true"))
    .and_then(|()| writeln!(writer, "projection={}", report.projection_id))
    .and_then(|()| writeln!(writer, "generation={}", report.generation))
    .and_then(|()| writeln!(writer, "source_epoch={}", report.source_epoch))
    .and_then(|()| {
        writeln!(
            writer,
            "recovered_generations={}",
            report.recovered_generations
        )
    })
    .and_then(|()| writeln!(writer, "projected_records={}", report.projected_records))
    .and_then(|()| writeln!(writer, "skipped_records={}", report.skipped_records))
    .and_then(|()| writeln!(writer, "unresolved_records={}", report.unresolved_records))
    .and_then(|()| writeln!(writer, "git_queries={}", report.git_queries))
    .and_then(|()| writeln!(writer, "head_available={}", report.head_available))
    .and_then(|()| {
        writeln!(
            writer,
            "maintenance_complete={}",
            report.maintenance.complete
        )
    })
    .and_then(|()| {
        writeln!(
            writer,
            "maintenance_warning_count={}",
            report.maintenance.warning_count
        )
    })
    .and_then(|()| {
        writeln!(
            writer,
            "maintenance_checkpoint={}",
            report.maintenance.checkpoint
        )
    })
    .and_then(|()| {
        writeln!(
            writer,
            "maintenance_shutdown={}",
            report.maintenance.shutdown
        )
    })
    .and_then(|()| {
        writeln!(
            writer,
            "database_identity_fence={}",
            report.maintenance.database_identity
        )
    });
    if result.is_ok() {
        EXIT_SUCCESS
    } else {
        emit_memory_receipt_delivery_failure(
            stderr,
            MemoryMutationRequestScope::Revalidation,
            MemoryMutationOperation::ProjectionPublication,
        )
    }
}

fn emit_memory_recall_report(writer: &mut impl Write, report: &MemoryRecallOutput) -> u8 {
    let mut encoded = Vec::new();
    if write_memory_recall_report(&mut encoded, report).is_err()
        || encoded.len() > MAX_CLI_MEMORY_RECALL_OUTPUT_BYTES
    {
        return EXIT_SOFTWARE;
    }
    if writer.write_all(&encoded).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn write_memory_recall_report(
    writer: &mut impl Write,
    report: &MemoryRecallOutput,
) -> std::io::Result<()> {
    writeln!(writer, "status=ok")?;
    writeln!(writer, "operation=memory-recall")?;
    writeln!(writer, "recall_profile={}", report.recall_profile)?;
    writeln!(
        writer,
        "query_sha256={}",
        report.query_sha256.as_deref().unwrap_or("none")
    )?;
    writeln!(writer, "snapshot_sha256={}", report.snapshot_sha256)?;
    writeln!(writer, "generation={}", report.generation)?;
    writeln!(writer, "projection={}", report.projection)?;
    writeln!(writer, "source_epoch={}", report.source_epoch)?;
    write_memory_target(writer, &report.target)?;
    writeln!(writer, "producer_id={}", report.producer.id)?;
    writeln!(writer, "producer_version={}", report.producer.version)?;
    writeln!(
        writer,
        "producer_profile_sha256={}",
        report.producer.profile_sha256
    )?;
    writeln!(writer, "matches_returned={}", report.matches_returned)?;
    writeln!(writer, "matches_total={}", report.matches_total)?;
    writeln!(writer, "matches_omitted={}", report.matches_omitted)?;
    write_memory_coverage(writer, report.coverage)?;
    writeln!(writer, "limitation={}", report.limitation)?;
    for (index, record) in report.records.iter().enumerate() {
        write_memory_record(writer, index, record)?;
    }
    Ok(())
}

fn write_memory_target(writer: &mut impl Write, target: &McpMemoryTarget) -> std::io::Result<()> {
    writeln!(writer, "target_kind={}", target.kind)?;
    writeln!(
        writer,
        "target_source_snapshot_sha256={}",
        target.source_snapshot_sha256.as_deref().unwrap_or("none")
    )?;
    writeln!(
        writer,
        "target_commit_object_format={}",
        target.commit_object_format.as_deref().unwrap_or("none")
    )?;
    writeln!(
        writer,
        "target_commit_hex={}",
        target.commit_hex.as_deref().unwrap_or("none")
    )
}

fn write_memory_coverage(
    writer: &mut impl Write,
    coverage: McpMemoryCoverage,
) -> std::io::Result<()> {
    writeln!(writer, "coverage_searched={}", coverage.searched)?;
    writeln!(writer, "coverage_skipped={}", coverage.skipped)?;
    writeln!(writer, "coverage_unresolved={}", coverage.unresolved)?;
    writeln!(writer, "coverage_truncated={}", coverage.truncated)?;
    writeln!(writer, "coverage_total={}", coverage.total)?;
    writeln!(writer, "state_current={}", coverage.current)?;
    writeln!(writer, "state_not_applicable={}", coverage.not_applicable)?;
    writeln!(writer, "state_stale={}", coverage.stale)?;
    writeln!(writer, "state_needs_review={}", coverage.needs_review)?;
    writeln!(writer, "state_indeterminate={}", coverage.indeterminate)?;
    writeln!(writer, "state_conflicted={}", coverage.conflicted)?;
    writeln!(writer, "state_contradicted={}", coverage.contradicted)?;
    writeln!(writer, "state_superseded={}", coverage.superseded)?;
    writeln!(writer, "state_quarantined={}", coverage.quarantined)?;
    writeln!(writer, "state_tombstoned={}", coverage.tombstoned)
}

fn write_memory_record(
    writer: &mut impl Write,
    index: usize,
    record: &McpMemoryRecord,
) -> std::io::Result<()> {
    let prefix = format!("record_{index}");
    writeln!(writer, "{prefix}_id={}", record.record_id)?;
    writeln!(
        writer,
        "{prefix}_revision_sha256={}",
        record.revision_sha256.as_deref().unwrap_or("none")
    )?;
    writeln!(
        writer,
        "{prefix}_effective_state={}",
        record.effective_state
    )?;
    writeln!(writer, "{prefix}_validity_state={}", record.validity_state)?;
    writeln!(writer, "{prefix}_evidence_state={}", record.evidence_state)?;
    writeln!(writer, "{prefix}_reason={}", record.reason)?;
    writeln!(writer, "{prefix}_evidence_count={}", record.evidence_count)?;
    writeln!(writer, "{prefix}_resolved_count={}", record.resolved_count)?;
    writeln!(writer, "{prefix}_review_count={}", record.review_count)?;
    writeln!(
        writer,
        "{prefix}_indeterminate_count={}",
        record.indeterminate_count
    )?;
    writeln!(writer, "{prefix}_head_count={}", record.head_count)?;
    writeln!(
        writer,
        "{prefix}_missing_parent_count={}",
        record.missing_parent_count
    )?;
    writeln!(writer, "{prefix}_selected={}", record.selected.is_some())?;
    if let Some(selected) = &record.selected {
        write_selected_memory(writer, &prefix, selected)?;
    }
    for (evidence_index, evidence) in record.evidence.iter().enumerate() {
        write_memory_evidence(writer, &prefix, evidence_index, evidence)?;
    }
    Ok(())
}

fn write_selected_memory(
    writer: &mut impl Write,
    prefix: &str,
    selected: &McpSelectedMemory,
) -> std::io::Result<()> {
    writeln!(
        writer,
        "{prefix}_selected_schema_version={}",
        selected.schema_version
    )?;
    writeln!(
        writer,
        "{prefix}_selected_display_revision={}",
        selected.display_revision
    )?;
    writeln!(writer, "{prefix}_selected_kind={}", selected.kind)?;
    writeln!(writer, "{prefix}_selected_title_encoding=lowercase_hex")?;
    writeln!(
        writer,
        "{prefix}_selected_title_hex={}",
        hex(selected.title.as_bytes())
    )?;
    writeln!(writer, "{prefix}_selected_body_encoding=lowercase_hex")?;
    writeln!(
        writer,
        "{prefix}_selected_body_hex={}",
        hex(selected.body.as_bytes())
    )?;
    writeln!(writer, "{prefix}_selected_assurance={}", selected.assurance)?;
    writeln!(writer, "{prefix}_selected_lifecycle={}", selected.lifecycle)?;
    writeln!(writer, "{prefix}_selected_tombstone={}", selected.tombstone)
}

fn write_memory_evidence(
    writer: &mut impl Write,
    record_prefix: &str,
    index: usize,
    evidence: &McpMemoryEvidence,
) -> std::io::Result<()> {
    let prefix = format!("{record_prefix}_evidence_{index}");
    writeln!(writer, "{prefix}_outcome={}", evidence.outcome)?;
    writeln!(writer, "{prefix}_assurance={}", evidence.assurance)?;
    writeln!(
        writer,
        "{prefix}_candidate_coverage_complete={}",
        evidence.candidate_coverage_complete
    )?;
    writeln!(
        writer,
        "{prefix}_candidate_count_before_limit={}",
        evidence.candidate_count_before_limit
    )?;
    writeln!(writer, "{prefix}_target={}", evidence.target.is_some())?;
    if let Some(target) = &evidence.target {
        write_memory_occurrence(writer, &format!("{prefix}_target"), target)?;
    }
    for (candidate_index, candidate) in evidence.candidates.iter().enumerate() {
        let candidate_prefix = format!("{prefix}_candidate_{candidate_index}");
        writeln!(writer, "{candidate_prefix}_relation={}", candidate.relation)?;
        write_memory_occurrence(writer, &candidate_prefix, &candidate.occurrence)?;
    }
    Ok(())
}

fn write_memory_occurrence(
    writer: &mut impl Write,
    prefix: &str,
    occurrence: &McpMemoryOccurrence,
) -> std::io::Result<()> {
    writeln!(writer, "{prefix}_path={}", occurrence.path)?;
    writeln!(
        writer,
        "{prefix}_content_sha256={}",
        occurrence.content_sha256
    )?;
    writeln!(
        writer,
        "{prefix}_artifact_sha256={}",
        occurrence.artifact_sha256
    )?;
    writeln!(writer, "{prefix}_fact_ordinal={}", occurrence.fact_ordinal)?;
    writeln!(
        writer,
        "{prefix}_declaration_sha256={}",
        occurrence.declaration_sha256
    )?;
    writeln!(
        writer,
        "{prefix}_name_elided_sha256={}",
        occurrence.name_elided_sha256
    )
}
