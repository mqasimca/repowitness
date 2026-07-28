fn emit_inspection_report(writer: &mut impl Write, stats: GitPathDiscoveryStats) -> u8 {
    let result = writeln!(writer, "status=ok")
        .and_then(|()| writeln!(writer, "operation=inspect-paths"))
        .and_then(|()| writeln!(writer, "index_created=false"))
        .and_then(|()| writeln!(writer, "git_output_bytes={}", stats.output_bytes()))
        .and_then(|()| writeln!(writer, "repository_paths={}", stats.path_count()))
        .and_then(|()| {
            writeln!(
                writer,
                "total_repository_path_bytes={}",
                stats.total_path_bytes()
            )
        })
        .and_then(|()| {
            writeln!(
                writer,
                "longest_repository_path_bytes={}",
                stats.longest_path_bytes()
            )
        })
        .and_then(|()| {
            writeln!(
                writer,
                "maximum_repository_path_components={}",
                stats.most_components()
            )
        });
    if result.is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn emit_index_report(writer: &mut impl Write, report: CliIndexReport) -> u8 {
    if report.known_parser_limitation_nodes > report.syntax_error_nodes {
        return EXIT_SOFTWARE;
    }
    let result = writeln!(writer, "status=ok")
        .and_then(|()| writeln!(writer, "operation=index"))
        .and_then(|()| writeln!(writer, "generation_activated=true"))
        .and_then(|()| writeln!(writer, "generation={}", report.generation))
        .and_then(|()| writeln!(writer, "source_epoch={}", report.source_epoch))
        .and_then(|()| {
            writeln!(
                writer,
                "recovered_generations={}",
                report.recovered_generations
            )
        })
        .and_then(|()| writeln!(writer, "repository_paths={}", report.discovered_paths))
        .and_then(|()| writeln!(writer, "indexed_rust_files={}", report.indexed_rust_files))
        .and_then(|()| writeln!(writer, "reused_rust_files={}", report.reused_rust_files))
        .and_then(|()| writeln!(writer, "analyzed_rust_files={}", report.analyzed_rust_files))
        .and_then(|()| writeln!(writer, "indexed_go_files={}", report.indexed_go_files))
        .and_then(|()| writeln!(writer, "reused_go_files={}", report.reused_go_files))
        .and_then(|()| writeln!(writer, "analyzed_go_files={}", report.analyzed_go_files))
        .and_then(|()| {
            writeln!(
                writer,
                "indexed_typescript_files={}",
                report.indexed_typescript_files
            )
        })
        .and_then(|()| {
            writeln!(
                writer,
                "reused_typescript_files={}",
                report.reused_typescript_files
            )
        })
        .and_then(|()| {
            writeln!(
                writer,
                "analyzed_typescript_files={}",
                report.analyzed_typescript_files
            )
        })
        .and_then(|()| writeln!(writer, "indexed_tsx_files={}", report.indexed_tsx_files))
        .and_then(|()| writeln!(writer, "reused_tsx_files={}", report.reused_tsx_files))
        .and_then(|()| writeln!(writer, "analyzed_tsx_files={}", report.analyzed_tsx_files))
        .and_then(|()| {
            writeln!(
                writer,
                "indexed_python_files={}",
                report.indexed_python_files
            )
        })
        .and_then(|()| writeln!(writer, "reused_python_files={}", report.reused_python_files))
        .and_then(|()| {
            writeln!(
                writer,
                "analyzed_python_files={}",
                report.analyzed_python_files
            )
        })
        .and_then(|()| {
            writeln!(
                writer,
                "skipped_unsupported_paths={}",
                report.skipped_unsupported_paths
            )
        })
        .and_then(|()| writeln!(writer, "total_source_bytes={}", report.total_source_bytes))
        .and_then(|()| writeln!(writer, "symbol_facts={}", report.total_facts))
        .and_then(|()| writeln!(writer, "syntax_error_nodes={}", report.syntax_error_nodes))
        .and_then(|()| {
            writeln!(
                writer,
                "known_parser_limitation_nodes={}",
                report.known_parser_limitation_nodes
            )
        });
    if result.is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn emit_search_report(writer: &mut impl Write, report: &CliSearchReport) -> u8 {
    let mut encoded = Vec::new();
    if write_search_report(&mut encoded, report).is_err()
        || encoded.len() > MAX_CLI_SEARCH_OUTPUT_BYTES
    {
        return EXIT_SOFTWARE;
    }
    if writer.write_all(&encoded).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn write_search_report(writer: &mut impl Write, report: &CliSearchReport) -> std::io::Result<()> {
    writeln!(writer, "status=ok")
        .and_then(|()| writeln!(writer, "operation=search"))
        .and_then(|()| writeln!(writer, "query_profile={CODE_SEARCH_PROFILE_VERSION}"))
        .and_then(|()| writeln!(writer, "query_sha256={}", report.query_digest))
        .and_then(|()| writeln!(writer, "snapshot_sha256={}", report.snapshot))
        .and_then(|()| writeln!(writer, "generation={}", report.generation))
        .and_then(|()| writeln!(writer, "resolution={}", report.resolution))
        .and_then(|()| writeln!(writer, "matches_returned={}", report.returned_matches))
        .and_then(|()| writeln!(writer, "matches_total={}", report.total_matches))
        .and_then(|()| writeln!(writer, "coverage_searched={}", report.searched))
        .and_then(|()| writeln!(writer, "coverage_skipped={}", report.skipped))
        .and_then(|()| writeln!(writer, "coverage_unresolved={}", report.unresolved))
        .and_then(|()| writeln!(writer, "coverage_truncated={}", report.truncated))
        .and_then(|()| writeln!(writer, "limitation=supported_language_symbol_lexical_only"))?;
    for (index, candidate) in report.matches.iter().enumerate() {
        emit_search_match(writer, index, candidate)?;
    }
    Ok(())
}

fn emit_search_match(
    writer: &mut impl Write,
    index: usize,
    candidate: &CliSearchMatch,
) -> std::io::Result<()> {
    writeln!(writer, "match_{index}_path={}", candidate.path)?;
    writeln!(
        writer,
        "match_{index}_fact_ordinal={}",
        candidate.fact_ordinal
    )?;
    writeln!(
        writer,
        "match_{index}_content_sha256={}",
        candidate.content_digest
    )?;
    writeln!(
        writer,
        "match_{index}_artifact_sha256={}",
        candidate.artifact_digest
    )?;
    writeln!(
        writer,
        "match_{index}_producer_manifest_sha256={}",
        candidate.producer_manifest
    )?;
    writeln!(writer, "match_{index}_evidence_tier=syntax")?;
    writeln!(writer, "match_{index}_language={}", candidate.language)?;
    writeln!(writer, "match_{index}_kind={}", candidate.kind)?;
    writeln!(writer, "match_{index}_name={}", candidate.name)?;
    writeln!(
        writer,
        "match_{index}_qualified_name={}",
        candidate.qualified_name
    )?;
    writeln!(
        writer,
        "match_{index}_name_span={}:{}",
        candidate.name_start, candidate.name_end
    )?;
    writeln!(
        writer,
        "match_{index}_declaration_span={}:{}",
        candidate.declaration_start, candidate.declaration_end
    )
}

fn emit_symbol_report(writer: &mut impl Write, report: &CliSymbolReport) -> u8 {
    let mut encoded = Vec::new();
    if write_symbol_report(&mut encoded, report).is_err()
        || encoded.len() > MAX_CLI_SYMBOL_OUTPUT_BYTES
    {
        return EXIT_SOFTWARE;
    }
    if writer.write_all(&encoded).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn write_symbol_report(writer: &mut impl Write, report: &CliSymbolReport) -> std::io::Result<()> {
    writeln!(writer, "status=ok")?;
    writeln!(writer, "operation=symbol-get")?;
    writeln!(
        writer,
        "schema_version={CLI_SYMBOL_REPORT_SCHEMA_VERSION}"
    )?;
    writeln!(writer, "symbol_profile={SYMBOL_GET_PROFILE_VERSION}")?;
    writeln!(writer, "snapshot_sha256={}", report.snapshot)?;
    writeln!(writer, "generation={}", report.generation)?;
    writeln!(writer, "resolution={}", report.resolution)?;
    writeln!(writer, "path={}", report.path)?;
    writeln!(writer, "content_sha256={}", report.content_digest)?;
    writeln!(writer, "artifact_sha256={}", report.artifact_digest)?;
    writeln!(writer, "fact_ordinal={}", report.fact_ordinal)?;
    writeln!(writer, "coverage_searched={}", report.searched)?;
    writeln!(writer, "coverage_skipped={}", report.skipped)?;
    writeln!(writer, "coverage_unresolved={}", report.unresolved)?;
    writeln!(writer, "coverage_truncated={}", report.truncated)?;
    writeln!(writer, "limitation=definition_only_no_references")?;
    writeln!(writer, "symbol_found={}", report.symbol.is_some())?;
    if let Some(symbol) = &report.symbol {
        write_symbol_data(writer, symbol)?;
    }
    Ok(())
}

fn write_symbol_data(writer: &mut impl Write, symbol: &CliSymbolData) -> std::io::Result<()> {
    writeln!(
        writer,
        "producer_manifest_sha256={}",
        symbol.producer_manifest
    )?;
    writeln!(writer, "evidence_tier=syntax")?;
    writeln!(writer, "language={}", symbol.language)?;
    writeln!(writer, "kind={}", symbol.kind)?;
    writeln!(writer, "name={}", symbol.name)?;
    writeln!(writer, "qualified_name={}", symbol.qualified_name)?;
    writeln!(
        writer,
        "name_span={}:{}",
        symbol.name_start, symbol.name_end
    )?;
    writeln!(
        writer,
        "declaration_span={}:{}",
        symbol.declaration_start, symbol.declaration_end
    )?;
    writeln!(
        writer,
        "declaration_encoding={}",
        symbol.declaration_encoding
    )?;
    let declaration =
        serde_json::to_string(&symbol.declaration).map_err(std::io::Error::other)?;
    writeln!(writer, "declaration_data_json={declaration}")
}

fn emit_version(writer: &mut impl Write) -> u8 {
    if writeln!(writer, "repowitness {}", env!("CARGO_PKG_VERSION")).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn emit_output(writer: &mut impl Write, message: &str) -> u8 {
    if writer.write_all(message.as_bytes()).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn emit_error(writer: &mut impl Write, code: u8, message: &str) -> u8 {
    if writer.write_all(message.as_bytes()).is_ok() {
        code
    } else {
        EXIT_IO
    }
}
