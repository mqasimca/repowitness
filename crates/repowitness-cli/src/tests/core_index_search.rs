use super::*;

#[test]
fn help_and_version_are_successful_and_truthful() {
    let inspector = FakeInspector::failure("must not be called");
    let (code, stdout, stderr) = invoke(&["--help"], &inspector);
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stdout.contains("index, watch, gc, context-build"));
    assert!(stdout.contains("search, locate-relevant-paths, symbol-search"));
    assert!(stdout.contains("context-build"));
    assert!(!stdout.contains("--profile"));
    assert!(stdout.contains("--repository-id"));
    assert!(stderr.is_empty());
    assert_eq!(inspector.calls.get(), 0);

    let (code, stdout, stderr) = invoke(&["-h"], &inspector);
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stdout.contains("Usage:"));
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = invoke(&["--version"], &inspector);
    assert_eq!(code, EXIT_SUCCESS);
    assert_eq!(
        stdout,
        concat!("repowitness ", env!("CARGO_PKG_VERSION"), "\n")
    );
    assert!(stderr.is_empty());
    assert_eq!(inspector.calls.get(), 0);

    let (code, stdout, stderr) = invoke(&["-V"], &inspector);
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stdout.starts_with("repowitness "));
    assert!(stderr.is_empty());
}

#[test]
fn global_help_and_version_reject_additional_arguments() {
    let inspector = FakeInspector::failure("must not be called");
    for arguments in [
        ["--help", "unexpected"],
        ["-h", "unexpected"],
        ["--version", "unexpected"],
        ["-V", "unexpected"],
    ] {
        let (code, stdout, stderr) = invoke(&arguments, &inspector);
        assert_eq!(code, EXIT_USAGE);
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("error:"));
    }
    assert_eq!(inspector.calls.get(), 0);
}

#[test]
fn nested_help_is_available_before_required_values() {
    let inspector = FakeInspector::failure("must not be called");
    for arguments in [
        vec!["config", "explain", "--help"],
        vec!["graph", "status", "--help"],
        vec!["memory-recall", "--help"],
        vec!["memory-manage", "write", "--help"],
        vec!["memory-manage", "sync", "--help"],
    ] {
        let (code, stdout, stderr) = invoke(&arguments, &inspector);
        assert_eq!(code, EXIT_SUCCESS, "{arguments:?}");
        assert!(!stdout.is_empty(), "{arguments:?}");
        assert!(stderr.is_empty(), "{arguments:?}");
    }
    assert_eq!(inspector.calls.get(), 0);
}

#[test]
fn no_command_and_unknown_commands_are_usage_errors_without_echoing_input() {
    let inspector = FakeInspector::failure("must not be called");
    let (code, stdout, stderr) = invoke(&[], &inspector);
    assert_eq!(code, EXIT_USAGE);
    assert!(stdout.is_empty());
    assert!(stderr.contains("no command supplied"));

    let (code, stdout, stderr) = invoke(&["private-command-name"], &inspector);
    assert_eq!(code, EXIT_USAGE);
    assert!(stdout.is_empty());
    assert!(stderr.contains("unknown command"));
    assert!(!stderr.contains("private-command-name"));
    assert_eq!(inspector.calls.get(), 0);
}

#[test]
fn index_requires_complete_arguments_without_invoking_adapters() {
    let inspector = FakeInspector::failure("must not be called");
    let indexer = FakeIndexer::failure("must not be called");
    let searcher = FakeSearcher::failure("must not be called");
    for arguments in [
        vec!["index"],
        vec!["index", "../repository"],
        vec!["index", "--repository", "../repository"],
        vec![
            "index",
            "--repository-id",
            "rwi1:h:00",
            "--database",
            "",
            "../repository",
        ],
        vec![
            "index",
            "--repository-id",
            "",
            "--database",
            "index.db",
            "../repository",
        ],
        vec![
            "index",
            "--repository-id",
            "first",
            "--repository-id",
            "second",
        ],
        vec!["index", "--database", "first.db", "--database", "second.db"],
        vec!["index", "--database"],
        vec!["index", "--repository-id"],
        vec!["index", "--help", "unexpected"],
        vec![
            "index", "one", "two", "three", "four", "five", "six", "seven", "eight",
        ],
    ] {
        let (code, stdout, stderr) =
            invoke_with_adapters(&arguments, &inspector, &indexer, &searcher);
        assert_eq!(code, EXIT_USAGE);
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("error:"));
    }
    assert_eq!(inspector.calls.get(), 0);
    assert_eq!(indexer.calls.get(), 0);
    assert_eq!(searcher.calls.get(), 0);
}

#[test]
fn index_success_reports_aggregates_and_passes_explicit_inputs() {
    let inspector = FakeInspector::failure("must not be called");
    let indexer = FakeIndexer::success(index_report());
    let identity = concat!(
        "rwi1:h:",
        "0101010101010101010101010101010101010101010101010101010101010101"
    );
    let (code, stdout, stderr) = invoke_with_adapters(
        &[
            "index",
            "--database",
            "../private-index.db",
            "--repository-id",
            identity,
            "--",
            "-private-repository",
        ],
        &inspector,
        &indexer,
        &FakeSearcher::failure("must not be called"),
    );

    assert_eq!(code, EXIT_SUCCESS);
    assert_eq!(
        stdout,
        concat!(
            "status=ok\n",
            "operation=index\n",
            "generation_activated=true\n",
            "generation=3\n",
            "source_epoch=0\n",
            "recovered_generations=1\n",
            "repository_paths=8\n",
            "indexed_rust_files=2\n",
            "reused_rust_files=1\n",
            "analyzed_rust_files=1\n",
            "indexed_go_files=1\n",
            "reused_go_files=0\n",
            "analyzed_go_files=1\n",
            "indexed_typescript_files=1\n",
            "reused_typescript_files=1\n",
            "analyzed_typescript_files=0\n",
            "indexed_tsx_files=1\n",
            "reused_tsx_files=0\n",
            "analyzed_tsx_files=1\n",
            "indexed_python_files=1\n",
            "reused_python_files=1\n",
            "analyzed_python_files=0\n",
            "skipped_policy_paths=0\n",
            "skipped_unsupported_paths=2\n",
            "total_source_bytes=101\n",
            "symbol_facts=7\n",
            "syntax_error_nodes=3\n",
            "known_parser_limitation_nodes=2\n",
        )
    );
    assert!(stderr.is_empty());
    assert_eq!(inspector.calls.get(), 0);
    assert_eq!(indexer.calls.get(), 1);
    assert_eq!(
        indexer.repository_root.borrow().as_deref(),
        Some(Path::new("-private-repository"))
    );
    assert_eq!(
        indexer.database.borrow().as_deref(),
        Some(Path::new("../private-index.db"))
    );
    assert_eq!(
        indexer.repository_identity.borrow().as_deref(),
        Some(OsStr::new(identity))
    );
    assert!(!stdout.contains("private"));
    assert!(!stdout.contains(identity));
}

#[test]
fn index_output_rejects_known_parser_counts_above_raw_syntax_errors() {
    let mut report = index_report();
    report.syntax_error_nodes = 1;
    report.known_parser_limitation_nodes = 2;
    let mut output = Vec::new();
    assert_eq!(emit_index_report(&mut output, report), EXIT_SOFTWARE);
    assert!(output.is_empty());
}

#[test]
fn index_output_rejects_inconsistent_language_accounting() {
    let mut report = index_report();
    report.reused_rust_files = report.indexed_rust_files;
    report.analyzed_rust_files = 1;
    let mut output = Vec::new();
    assert_eq!(emit_index_report(&mut output, report), EXIT_SOFTWARE);
    assert!(output.is_empty());
}

#[test]
fn index_output_rejects_overflowing_path_accounting() {
    let mut report = index_report();
    report.indexed_rust_files = u64::MAX;
    report.reused_rust_files = u64::MAX;
    report.analyzed_rust_files = 0;
    let mut output = Vec::new();
    assert_eq!(emit_index_report(&mut output, report), EXIT_SOFTWARE);
    assert!(output.is_empty());
}

#[test]
fn index_failures_are_nonzero_and_redacted_by_the_adapter() {
    let inspector = FakeInspector::failure("must not be called");
    let indexer = FakeIndexer::failure("sensitive adapter detail: /private/index.db");
    let identity = concat!(
        "rwi1:h:",
        "0202020202020202020202020202020202020202020202020202020202020202"
    );
    let (code, stdout, stderr) = invoke_with_adapters(
        &[
            "index",
            "--repository-id",
            identity,
            "--database",
            "../private-index.db",
            "../private-repository",
        ],
        &inspector,
        &indexer,
        &FakeSearcher::failure("must not be called"),
    );

    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, "error: indexing failed\n");
    assert!(!stderr.contains("private"));
    assert!(!stderr.contains(identity));
}

#[test]
fn search_requires_bounded_complete_arguments_without_invoking_adapters() {
    let inspector = FakeInspector::failure("must not be called");
    let indexer = FakeIndexer::failure("must not be called");
    let searcher = FakeSearcher::failure("must not be called");
    for arguments in [
        vec!["search"],
        vec!["search", "--database", "index.db"],
        vec![
            "search",
            "--repository-id",
            "id",
            "--database",
            "index.db",
            "--query",
            "",
        ],
        vec!["search", "--limit", "0"],
        vec!["search", "--limit", "101"],
        vec!["search", "--limit", "private"],
        vec!["search", "--query"],
        vec!["search", "--unknown", "private"],
        vec!["search", "--help", "unexpected"],
        vec![
            "search",
            "--query",
            "x",
            "--query",
            "y",
            "--database",
            "index.db",
            "--repository-id",
            "id",
        ],
    ] {
        let (code, stdout, stderr) =
            invoke_with_adapters(&arguments, &inspector, &indexer, &searcher);
        assert_eq!(code, EXIT_USAGE);
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("error:"));
        assert!(!stderr.contains("private"));
    }
    assert_eq!(inspector.calls.get(), 0);
    assert_eq!(indexer.calls.get(), 0);
    assert_eq!(searcher.calls.get(), 0);
}

#[test]
fn search_reports_evidence_coverage_and_passes_explicit_inputs() {
    let inspector = FakeInspector::failure("must not be called");
    let indexer = FakeIndexer::failure("must not be called");
    let searcher = FakeSearcher::success(search_report());
    let identity = concat!(
        "rwi1:h:",
        "0606060606060606060606060606060606060606060606060606060606060606"
    );
    let (code, stdout, stderr) = invoke_with_adapters(
        &[
            "search",
            "--query",
            "private query",
            "--limit",
            "7",
            "--database",
            "../private-index.db",
            "--repository-id",
            identity,
        ],
        &inspector,
        &indexer,
        &searcher,
    );

    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert!(stdout.contains("status=ok\noperation=search\n"));
    assert!(stdout.contains("query_profile=3\n"));
    assert!(stdout.contains("generation=9\n"));
    assert!(stdout.contains("resolution=confirmed\n"));
    assert!(stdout.contains("matches_returned=1\nmatches_total=3\n"));
    assert!(stdout.contains("coverage_skipped=2\n"));
    assert!(stdout.contains("coverage_truncated=2\n"));
    assert!(stdout.contains("limitation=supported_language_symbol_lexical_only\n"));
    assert!(stdout.contains("match_0_path=rwp1:h:7372632F6C69622E7273\n"));
    assert!(stdout.contains("match_0_fact_ordinal=7\n"));
    assert!(stdout.contains("match_0_evidence_tier=syntax\n"));
    assert!(stdout.contains("match_0_language=rust\n"));
    assert!(stdout.contains("match_0_qualified_name=fixture::run\n"));
    assert!(stdout.contains("match_0_name_span=7:10\n"));
    assert!(!stdout.contains("private query"));
    assert!(!stdout.contains("../private-index.db"));
    assert!(!stdout.contains(identity));
    assert_eq!(searcher.calls.get(), 1);
    assert_eq!(
        searcher.database.borrow().as_deref(),
        Some(Path::new("../private-index.db"))
    );
    assert_eq!(
        searcher.repository_identity.borrow().as_deref(),
        Some(OsStr::new(identity))
    );
    assert_eq!(
        searcher.query.borrow().as_deref(),
        Some(OsStr::new("private query"))
    );
    assert_eq!(searcher.max_results.get(), Some(7));
}

#[test]
fn search_failures_are_nonzero_and_do_not_echo_inputs() {
    let inspector = FakeInspector::failure("must not be called");
    let indexer = FakeIndexer::failure("must not be called");
    let searcher = FakeSearcher::failure("sensitive adapter detail: private query");
    let identity = concat!(
        "rwi1:h:",
        "0707070707070707070707070707070707070707070707070707070707070707"
    );
    let (code, stdout, stderr) = invoke_with_adapters(
        &[
            "search",
            "--repository-id",
            identity,
            "--database",
            "../private-index.db",
            "--query",
            "private query",
        ],
        &inspector,
        &indexer,
        &searcher,
    );

    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, "error: code search failed\n");
    assert!(!stderr.contains("private"));
    assert!(!stderr.contains(identity));
}

#[test]
fn search_boundary_rejects_an_oversized_encoded_report() {
    let mut report = search_report();
    report.matches[0].name = "x".repeat(MAX_CLI_SEARCH_OUTPUT_BYTES);
    assert_eq!(emit_search_report(&mut io::sink(), &report), EXIT_SOFTWARE);
}
