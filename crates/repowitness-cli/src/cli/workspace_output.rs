fn emit_workspace_index_report(
    writer: &mut impl Write,
    report: LocalConnectedWorkspaceIndexReport,
) -> u8 {
    let coverage = report.coverage();
    let result = writeln!(writer, "status=ok")
        .and_then(|()| writeln!(writer, "operation=workspace_index"))
        .and_then(|()| writeln!(writer, "workspace_report_version={}", report.report_version()))
        .and_then(|()| writeln!(writer, "manifest_schema_version={}", report.manifest_schema_version()))
        .and_then(|()| writeln!(writer, "view_receipt_version={}", report.view_receipt_version()))
        .and_then(|()| writeln!(writer, "configuration_sha256={}", hex(report.configuration_digest().as_bytes())))
        .and_then(|()| writeln!(writer, "view_receipt_sha256={}", report.view_digest()))
        .and_then(|()| writeln!(writer, "source_slots={}", report.source_count()))
        .and_then(|()| writeln!(writer, "distinct_generations={}", report.generation_count()))
        .and_then(|()| writeln!(writer, "recovered_generations={}", report.recovered_generations()))
        .and_then(|()| writeln!(writer, "repository_paths={}", coverage.discovered_paths()))
        .and_then(|()| writeln!(writer, "indexed_files={}", coverage.indexed_files()))
        .and_then(|()| writeln!(writer, "reused_files={}", coverage.reused_files()))
        .and_then(|()| writeln!(writer, "analyzed_files={}", coverage.analyzed_files()))
        .and_then(|()| writeln!(writer, "skipped_policy_paths={}", coverage.skipped_policy_paths()))
        .and_then(|()| writeln!(writer, "skipped_unsupported_paths={}", coverage.skipped_unsupported_paths()))
        .and_then(|()| writeln!(writer, "outcome={}", report.outcome().as_str()))
        .and_then(|()| writeln!(writer, "maintenance={}", report.maintenance().as_str()));
    if result.is_ok() { EXIT_SUCCESS } else { EXIT_IO }
}
