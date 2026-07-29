use super::*;

#[test]
fn symbol_get_reports_verified_source_and_passes_the_complete_selector() {
    let inspector = FakeInspector::failure("must not be called");
    let indexer = FakeIndexer::failure("must not be called");
    let searcher = FakeSearcher::failure("must not be called");
    let getter = FakeSymbolGetter::success(symbol_report());
    let identity = format!("rwi1:h:{}", "06".repeat(32));
    let snapshot = "11".repeat(32);
    let content = "33".repeat(32);
    let artifact = "44".repeat(32);
    let (code, stdout, stderr) = invoke_with_symbol_adapter(
        &[
            "symbol-get",
            "--artifact",
            &artifact,
            "--fact",
            "7",
            "--root",
            "../private-repository",
            "--content",
            &content,
            "--path",
            "rwp1:h:7372632F6C69622E7273",
            "--generation",
            "9",
            "--snapshot",
            &snapshot,
            "--database",
            "../private-index.db",
            "--repository-id",
            &identity,
        ],
        &inspector,
        &indexer,
        &searcher,
        &getter,
    );

    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert!(stdout.contains("status=ok\noperation=symbol-get\n"));
    assert!(stdout.contains("schema_version=2\n"));
    assert!(stdout.contains("symbol_profile=3\n"));
    assert!(stdout.contains("resolution=confirmed\n"));
    assert!(stdout.contains("fact_ordinal=7\n"));
    assert!(stdout.contains("symbol_found=true\n"));
    assert!(stdout.contains("evidence_tier=syntax\n"));
    assert!(stdout.contains("language=rust\n"));
    assert!(stdout.contains("name=run\n"));
    assert!(stdout.contains("declaration_encoding=utf8\n"));
    assert!(stdout.contains("declaration_data_json=\"pub fn run() {}\"\n"));
    assert!(!stdout.contains("private"));
    assert!(!stdout.contains(&identity));
    assert_eq!(getter.calls.get(), 1);
    assert_eq!(
        getter.root.borrow().as_deref(),
        Some(Path::new("../private-repository"))
    );
    assert_eq!(
        getter.database.borrow().as_deref(),
        Some(Path::new("../private-index.db"))
    );
    assert_eq!(getter.generation.get(), Some(9));
    assert_eq!(getter.fact_ordinal.get(), Some(7));
    assert_eq!(
        getter.snapshot.borrow().as_deref(),
        Some(OsStr::new(&snapshot))
    );
    assert_eq!(
        getter.path.borrow().as_deref(),
        Some(OsStr::new("rwp1:h:7372632F6C69622E7273"))
    );
    assert_eq!(
        getter.content.borrow().as_deref(),
        Some(OsStr::new(&content))
    );
    assert_eq!(
        getter.artifact.borrow().as_deref(),
        Some(OsStr::new(&artifact))
    );
}

#[test]
fn symbol_get_rejects_incomplete_or_invalid_selectors_before_io() {
    let inspector = FakeInspector::failure("must not be called");
    let indexer = FakeIndexer::failure("must not be called");
    let searcher = FakeSearcher::failure("must not be called");
    let getter = FakeSymbolGetter::failure("must not be called");
    for arguments in [
        vec!["symbol-get"],
        vec!["symbol-get", "--root", "private"],
        vec!["symbol-get", "--unknown", "private"],
        vec![
            "symbol-get",
            "--root",
            "a",
            "--root",
            "b",
            "--database",
            "private",
        ],
    ] {
        let (code, stdout, stderr) =
            invoke_with_symbol_adapter(&arguments, &inspector, &indexer, &searcher, &getter);
        assert_eq!(code, EXIT_USAGE);
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("error:"));
        assert!(!stderr.contains("private"));
    }
    assert_eq!(getter.calls.get(), 0);
}

#[test]
fn symbol_get_rejects_invalid_numeric_selector_parts_before_io() {
    let inspector = FakeInspector::failure("must not be called");
    let indexer = FakeIndexer::failure("must not be called");
    let searcher = FakeSearcher::failure("must not be called");
    let getter = FakeSymbolGetter::failure("must not be called");
    let mut arguments = [
        "symbol-get",
        "--root",
        "root",
        "--database",
        "index.db",
        "--repository-id",
        "rwi1:h:0606060606060606060606060606060606060606060606060606060606060606",
        "--snapshot",
        "1111111111111111111111111111111111111111111111111111111111111111",
        "--generation",
        "0",
        "--path",
        "rwp1:h:7372632F6C69622E7273",
        "--content",
        "3333333333333333333333333333333333333333333333333333333333333333",
        "--artifact",
        "4444444444444444444444444444444444444444444444444444444444444444",
        "--fact",
        "7",
    ];
    let (code, _, _) =
        invoke_with_symbol_adapter(&arguments, &inspector, &indexer, &searcher, &getter);
    assert_eq!(code, EXIT_USAGE);

    arguments[10] = "9";
    arguments[18] = "-1";
    let (code, _, _) =
        invoke_with_symbol_adapter(&arguments, &inspector, &indexer, &searcher, &getter);
    assert_eq!(code, EXIT_USAGE);
    arguments[18] = "9007199254740992";
    let (code, _, _) =
        invoke_with_symbol_adapter(&arguments, &inspector, &indexer, &searcher, &getter);
    assert_eq!(code, EXIT_USAGE);
    assert_eq!(getter.calls.get(), 0);
}

#[test]
fn symbol_get_failures_do_not_leak_inputs() {
    let inspector = FakeInspector::failure("must not be called");
    let indexer = FakeIndexer::failure("must not be called");
    let searcher = FakeSearcher::failure("must not be called");
    let getter = FakeSymbolGetter::failure("sensitive adapter detail: private-root");
    let identity = format!("rwi1:h:{}", "07".repeat(32));
    let digest = "88".repeat(32);
    let (code, stdout, stderr) = invoke_with_symbol_adapter(
        &[
            "symbol-get",
            "--root",
            "private-root",
            "--database",
            "private.db",
            "--repository-id",
            &identity,
            "--snapshot",
            &digest,
            "--generation",
            "1",
            "--path",
            "rwp1:h:70726976617465",
            "--content",
            &digest,
            "--artifact",
            &digest,
            "--fact",
            "0",
        ],
        &inspector,
        &indexer,
        &searcher,
        &getter,
    );
    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, "error: symbol retrieval failed\n");
    assert!(!stderr.contains("private"));
    assert!(!stderr.contains(&identity));
    assert!(!stderr.contains(&digest));
}

#[test]
fn symbol_get_boundary_rejects_an_oversized_encoded_report() {
    let mut report = symbol_report();
    report
        .symbol
        .as_mut()
        .expect("fixture has a symbol")
        .declaration = "0".repeat(MAX_CLI_SYMBOL_OUTPUT_BYTES);
    assert_eq!(emit_symbol_report(&mut io::sink(), &report), EXIT_SOFTWARE);
}

#[test]
fn maximum_declaration_json_expansion_fits_the_cli_report_boundary() {
    let mut report = symbol_report();
    report
        .symbol
        .as_mut()
        .expect("fixture has a symbol")
        .declaration = format!("run{}", "\"".repeat(8 * 1024 * 1024 - 3));
    assert_eq!(emit_symbol_report(&mut io::sink(), &report), EXIT_SUCCESS);
}

#[test]
fn symbol_get_output_failure_returns_the_io_exit_code() {
    assert_eq!(
        emit_symbol_report(&mut FailingWriter, &symbol_report()),
        EXIT_IO
    );
}

#[test]
fn index_and_inspection_help_do_not_run_repository_io() {
    let inspector = FakeInspector::failure("must not be called");
    let (code, stdout, stderr) = invoke(&["index", "--help"], &inspector);
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stdout.contains("atomically activate"));
    assert!(stdout.contains("--repository-id"));
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = invoke(&["search", "--help"], &inspector);
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stdout.contains("proof-carrying results"));
    assert!(stdout.contains("--limit <1-100>"));
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = invoke(&["symbol-get", "--help"], &inspector);
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stdout.contains("exact declaration"));
    assert!(stdout.contains("display-safe UTF-8"));
    assert!(stdout.contains("lowercase hexadecimal"));
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = invoke(&["inspect-paths", "--help"], &inspector);
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stdout.contains("without creating an index"));
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = invoke(&["index", "-h"], &inspector);
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stdout.contains("--database"));
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = invoke(&["inspect-paths", "-h"], &inspector);
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stdout.contains("without creating an index"));
    assert!(stderr.is_empty());
    assert_eq!(inspector.calls.get(), 0);
}

#[test]
fn inspection_success_reports_only_deterministic_aggregates() {
    let inspector = FakeInspector::success(GitPathDiscoveryStats::new(22, 2, 20, 10, 2));
    let (code, stdout, stderr) = invoke(&["inspect-paths", "../private-repo"], &inspector);
    assert_eq!(code, EXIT_SUCCESS);
    assert_eq!(
        stdout,
        concat!(
            "status=ok\n",
            "operation=inspect-paths\n",
            "index_created=false\n",
            "git_output_bytes=22\n",
            "repository_paths=2\n",
            "total_repository_path_bytes=20\n",
            "longest_repository_path_bytes=10\n",
            "maximum_repository_path_components=2\n",
        )
    );
    assert!(stderr.is_empty());
    assert_eq!(inspector.calls.get(), 1);
    assert_eq!(
        inspector.root.borrow().as_deref(),
        Some(Path::new("../private-repo"))
    );
    assert!(!stdout.contains("private-repo"));

    let inspector = FakeInspector::success(GitPathDiscoveryStats::new(22, 2, 20, 10, 2));
    let (code, stdout, stderr) = invoke(&["inspect-paths", "--", "-private-repo"], &inspector);
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stdout.contains("index_created=false"));
    assert!(stderr.is_empty());
    assert_eq!(
        inspector.root.borrow().as_deref(),
        Some(Path::new("-private-repo"))
    );
}

#[test]
fn inspection_failures_are_nonzero_and_do_not_print_the_root() {
    let inspector = FakeInspector::failure("sensitive adapter detail: ../private-repo");
    let (code, stdout, stderr) = invoke(&["inspect-paths", "../private-repo"], &inspector);
    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, "error: repository path inspection failed\n");
    assert!(!stderr.contains("private-repo"));
}

#[test]
fn inspection_argument_errors_do_not_invoke_repository_io() {
    let inspector = FakeInspector::failure("must not be called");
    for arguments in [
        vec!["inspect-paths"],
        vec!["inspect-paths", "--"],
        vec!["inspect-paths", "--unknown"],
        vec!["inspect-paths", ""],
        vec!["inspect-paths", "one", "two"],
        vec!["inspect-paths", "--help", "extra"],
    ] {
        let (code, stdout, stderr) = invoke(&arguments, &inspector);
        assert_eq!(code, EXIT_USAGE);
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("error:"));
    }
    assert_eq!(inspector.calls.get(), 0);
}

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("intentional test failure"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("intentional test failure"))
    }
}

#[test]
fn output_failures_return_the_io_exit_code() {
    let inspector = FakeInspector::failure("must not be called");
    let indexer = FakeIndexer::failure("must not be called");
    let searcher = FakeSearcher::failure("must not be called");
    let symbol_getter = FakeSymbolGetter::failure("must not be called");
    let code = run_with_adapters(
        [OsString::from("repowitness"), OsString::from("--help")],
        FailingWriter,
        io::sink(),
        &inspector,
        &indexer,
        &searcher,
        &symbol_getter,
        &FakeMemory,
        &LocalConfigurationLoader,
    );
    assert_eq!(code, EXIT_IO);

    let code = run_with_adapters(
        [OsString::from("repowitness")],
        io::sink(),
        FailingWriter,
        &inspector,
        &indexer,
        &searcher,
        &symbol_getter,
        &FakeMemory,
        &LocalConfigurationLoader,
    );
    assert_eq!(code, EXIT_IO);

    let code = run_with_adapters(
        [OsString::from("repowitness"), OsString::from("--version")],
        FailingWriter,
        io::sink(),
        &inspector,
        &indexer,
        &searcher,
        &symbol_getter,
        &FakeMemory,
        &LocalConfigurationLoader,
    );
    assert_eq!(code, EXIT_IO);

    let success = FakeInspector::success(GitPathDiscoveryStats::new(1, 0, 0, 0, 0));
    let code = run_with_adapters(
        [
            OsString::from("repowitness"),
            OsString::from("inspect-paths"),
            OsString::from("repository"),
        ],
        FailingWriter,
        io::sink(),
        &success,
        &indexer,
        &searcher,
        &symbol_getter,
        &FakeMemory,
        &LocalConfigurationLoader,
    );
    assert_eq!(code, EXIT_IO);

    let failure = FakeInspector::failure("expected test failure");
    let code = run_with_adapters(
        [
            OsString::from("repowitness"),
            OsString::from("inspect-paths"),
            OsString::from("repository"),
        ],
        io::sink(),
        FailingWriter,
        &failure,
        &indexer,
        &searcher,
        &symbol_getter,
        &FakeMemory,
        &LocalConfigurationLoader,
    );
    assert_eq!(code, EXIT_IO);

    let successful_indexer = FakeIndexer::success(index_report());
    let code = run_with_adapters(
        [
            OsString::from("repowitness"),
            OsString::from("index"),
            OsString::from("--repository-id"),
            OsString::from(concat!(
                "rwi1:h:",
                "0303030303030303030303030303030303030303030303030303030303030303"
            )),
            OsString::from("--database"),
            OsString::from("index.db"),
            OsString::from("repository"),
        ],
        FailingWriter,
        io::sink(),
        &inspector,
        &successful_indexer,
        &searcher,
        &symbol_getter,
        &FakeMemory,
        &LocalConfigurationLoader,
    );
    assert_eq!(code, EXIT_IO);

    let mut writer = FailingWriter;
    assert!(writer.flush().is_err());
}

#[test]
fn search_output_failure_returns_the_io_exit_code() {
    let inspector = FakeInspector::failure("must not be called");
    let indexer = FakeIndexer::failure("must not be called");
    let searcher = FakeSearcher::success(search_report());
    let symbol_getter = FakeSymbolGetter::failure("must not be called");
    let code = run_with_adapters(
        [
            OsString::from("repowitness"),
            OsString::from("search"),
            OsString::from("--repository-id"),
            OsString::from(concat!(
                "rwi1:h:",
                "0404040404040404040404040404040404040404040404040404040404040404"
            )),
            OsString::from("--database"),
            OsString::from("index.db"),
            OsString::from("--query"),
            OsString::from("run"),
        ],
        FailingWriter,
        io::sink(),
        &inspector,
        &indexer,
        &searcher,
        &symbol_getter,
        &FakeMemory,
        &LocalConfigurationLoader,
    );
    assert_eq!(code, EXIT_IO);
}
