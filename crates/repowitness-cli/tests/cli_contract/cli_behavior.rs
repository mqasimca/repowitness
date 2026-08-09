#[test]
fn verify_emits_a_fenced_revision_pinned_receipt_without_a_verdict() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let committed = Command::new("git")
        .current_dir(&repository)
        .args([
            "-c",
            "user.name=RepoWitness Test",
            "-c",
            "user.email=repowitness@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "base",
        ])
        .status()
        .expect("Git should commit fixture base");
    assert!(committed.success());
    let base = Command::new("git")
        .current_dir(&repository)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("Git should resolve fixture base");
    assert!(base.status.success());
    let base = String::from_utf8(base.stdout)
        .expect("base should be UTF-8")
        .trim()
        .to_owned();
    let database = directory.database();
    assert!(index(&repository, &database, REPOSITORY_ID).status.success());
    fs::write(
        repository.join("src/lib.rs"),
        "pub struct Widget;\nimpl Widget { pub fn changed() {} }\n",
    )
    .expect("fixture change should be written");

    let receipt = repowitness_os([
        OsStr::new("verify"),
        OsStr::new("--repository-id"),
        OsStr::new(REPOSITORY_ID),
        OsStr::new("--database"),
        database.as_os_str(),
        OsStr::new("--root"),
        repository.as_os_str(),
        OsStr::new("--base"),
        OsStr::new(&base),
        OsStr::new("--intent"),
        OsStr::new("Widget"),
    ]);
    assert!(receipt.status.success());
    assert!(receipt.stderr.is_empty());
    let receipt = String::from_utf8(receipt.stdout).expect("verify receipt should be UTF-8");
    assert_eq!(report_value(&receipt, "operation"), "verify");
    assert_eq!(report_value(&receipt, "base"), base);
    assert_eq!(report_value(&receipt, "index_worktree_alignment"), "unverified");
    assert_eq!(report_value(&receipt, "indexed_context_availability"), "unavailable");
    assert_eq!(report_value(&receipt, "indexed_context_reason"), "stale_source");
    assert_eq!(report_value(&receipt, "verdict"), "not_provided");
    assert_eq!(report_value(&receipt, "change[0].kind"), "modified");

    let available = repowitness_os([
        OsStr::new("verify"),
        OsStr::new("--repository-id"),
        OsStr::new(REPOSITORY_ID),
        OsStr::new("--database"),
        database.as_os_str(),
        OsStr::new("--root"),
        repository.as_os_str(),
        OsStr::new("--base"),
        OsStr::new(&base),
        OsStr::new("--intent"),
        OsStr::new("Gadget"),
    ]);
    assert!(available.status.success());
    let available = String::from_utf8(available.stdout).expect("verify receipt should be UTF-8");
    assert_eq!(report_value(&available, "indexed_context_availability"), "available");
    assert_eq!(report_value(&available, "indexed_context_reason"), "not_applicable");
    assert_eq!(report_value(&available, "indexed_snapshot_sha256").len(), 64);
}

#[test]
fn help_and_version_write_to_stdout_and_succeed() {
    let help = repowitness(&["--help"]);
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    let help = String::from_utf8(help.stdout).expect("help must be UTF-8");
    assert!(help.contains("index          Build"));
    assert!(help.contains("--repository-id"));
    assert!(help.contains("architecture-map --repository-id"));
    assert!(help.contains("architecture-overview --repository-id"));
    assert!(help.contains("test-markers --repository-id"));
    assert!(help.contains("syntax-site-search --repository-id"));

    let version = repowitness(&["--version"]);
    assert!(version.status.success());
    assert!(version.stderr.is_empty());
    assert_eq!(
        String::from_utf8(version.stdout).expect("version must be UTF-8"),
        concat!("repowitness ", env!("CARGO_PKG_VERSION"), "\n")
    );
}

#[test]
fn invalid_commands_and_missing_commands_fail() {
    for arguments in [
        Vec::new(),
        vec!["private-command"],
        vec!["--help", "unexpected"],
        vec!["--version", "unexpected"],
    ] {
        let output = repowitness(&arguments);
        assert_eq!(output.status.code(), Some(64));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).expect("diagnostic must be UTF-8");
        assert!(stderr.starts_with("error:"));
        assert!(!stderr.contains("private-command"));
    }
}

#[test]
fn ambient_git_discovery_settings_do_not_change_nested_repository_resolution() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let nested_repository_path = manifest_dir.join("src");
    let ceiling = manifest_dir
        .parent()
        .expect("the CLI crate must have a workspace parent");
    let output = Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .args(["inspect-paths".as_ref(), nested_repository_path.as_os_str()])
        .env("GIT_CEILING_DIRECTORIES", ceiling)
        .output()
        .expect("the RepoWitness binary must start");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("report must be UTF-8");
    assert!(stdout.contains("status=ok\n"));
    assert!(stdout.contains("index_created=false\n"));
}

#[test]
fn incomplete_index_forms_are_usage_errors_without_running_work() {
    for arguments in [
        vec!["index"],
        vec!["index", "../repository"],
        vec!["index", "--repository", "../repository"],
    ] {
        let output = repowitness(&arguments);
        assert_eq!(output.status.code(), Some(64));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).expect("diagnostic must be UTF-8");
        assert!(stderr.starts_with("error:"));
        assert!(!stderr.contains("../repository"));
    }
}

#[test]
fn index_activates_and_replaces_real_generations_without_leaking_inputs() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();

    let first = index(&repository, &database, REPOSITORY_ID);
    assert!(first.status.success());
    assert!(first.stderr.is_empty());
    let first = String::from_utf8(first.stdout).expect("index report must be UTF-8");
    assert!(first.contains("status=ok\n"));
    assert!(first.contains("operation=index\n"));
    assert!(first.contains("generation_activated=true\n"));
    assert!(first.contains("generation=1\n"));
    assert!(first.contains("repository_paths=3\n"));
    assert!(first.contains("indexed_rust_files=1\n"));
    assert!(first.contains("indexed_go_files=1\n"));
    assert_index_work_counts(&first, 0, 1, 0, 1);
    assert!(first.contains("skipped_unsupported_paths=1\n"));
    assert!(first.contains("symbol_facts=4\n"));
    assert_parser_diagnostics_counts(&first, "0", "0");
    assert!(!first.contains(REPOSITORY_ID));
    assert!(!first.contains(repository.to_string_lossy().as_ref()));
    assert!(!first.contains(database.to_string_lossy().as_ref()));
    assert!(database.is_file());

    let first_search = assert_widget_search_contract(&database);
    assert_symbol_get_success(
        symbol_get_from_search(&repository, &database, REPOSITORY_ID, &first_search),
        "rust",
        "Widget",
        "pub struct Widget;",
    );
    assert_go_search_contract(&repository, &database);
    assert_absent_search_contract(&database);

    let second = index(&repository, &database, REPOSITORY_ID);
    assert!(second.status.success());
    assert!(second.stderr.is_empty());
    let second = String::from_utf8(second.stdout).expect("index report must be UTF-8");
    assert!(second.contains("generation=2\n"));
    assert!(second.contains("symbol_facts=4\n"));
    assert_index_work_counts(&second, 1, 0, 1, 0);

    let stale_generation =
        symbol_get_from_search(&repository, &database, REPOSITORY_ID, &first_search);
    assert_stale_symbol_rejected(stale_generation, &repository, None);

    modify_source_and_assert_stale_rejection(&repository, &database);

    let changed = index(&repository, &database, REPOSITORY_ID);
    assert!(changed.status.success());
    assert!(changed.stderr.is_empty());
    let changed = String::from_utf8(changed.stdout).expect("index report must be UTF-8");
    assert!(changed.contains("generation=3\n"));
    assert!(changed.contains("symbol_facts=5\n"));
    assert_index_work_counts(&changed, 0, 1, 1, 0);

    assert_changed_symbol_contract(&repository, &database);
}

#[test]
fn architecture_map_is_bounded_generation_pinned_and_relationship_free() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let indexed = index(&repository, &database, REPOSITORY_ID);
    assert!(indexed.status.success());

    let mapped = architecture_map(&database, REPOSITORY_ID, "1");
    assert!(mapped.status.success());
    assert!(mapped.stderr.is_empty());
    let mapped = String::from_utf8(mapped.stdout).expect("architecture map must be UTF-8");
    let mapped: serde_json::Value = serde_json::from_str(&mapped).expect("architecture map JSON");
    assert_eq!(mapped["schema_version"], serde_json::json!(1));
    assert_eq!(mapped["map_profile"], serde_json::json!(1));
    assert_eq!(mapped["generation"], serde_json::json!(1));
    assert_eq!(mapped["total_files"], serde_json::json!(2));
    assert_eq!(mapped["files_returned"], serde_json::json!(1));
    assert_eq!(mapped["truncated"], serde_json::json!(true));
    assert_eq!(
        mapped["limitation"],
        serde_json::json!("file_inventory_only_no_relationship_inference")
    );
    assert_eq!(
        mapped["languages"],
        serde_json::json!([
            {"language": "go", "files": 1, "declarations": 2},
            {"language": "rust", "files": 1, "declarations": 2},
        ])
    );
    assert!(mapped["files"].as_array().is_some_and(|files| files.len() == 1));
    let encoded = mapped.to_string();
    assert!(!encoded.contains(REPOSITORY_ID));
    assert!(!encoded.contains(repository.to_string_lossy().as_ref()));
    assert!(!encoded.contains(database.to_string_lossy().as_ref()));
}

#[test]
fn architecture_overview_is_bounded_generation_pinned_and_non_relational() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let indexed = index(&repository, &database, REPOSITORY_ID);
    assert!(indexed.status.success());

    let overview = architecture_overview(&database, REPOSITORY_ID, "1");
    assert!(overview.status.success());
    assert!(overview.stderr.is_empty());
    let overview = String::from_utf8(overview.stdout).expect("architecture overview must be UTF-8");
    let overview: serde_json::Value =
        serde_json::from_str(&overview).expect("architecture overview JSON");
    assert_eq!(overview["schema_version"], serde_json::json!(1));
    assert_eq!(overview["overview_profile"], serde_json::json!(1));
    assert_eq!(overview["generation"], serde_json::json!(1));
    assert_eq!(overview["total_files"], serde_json::json!(2));
    assert_eq!(overview["files_returned"], serde_json::json!(1));
    assert_eq!(overview["files_truncated"], serde_json::json!(true));
    assert_eq!(overview["total_source_roots"], serde_json::json!(1));
    assert_eq!(overview["source_roots_returned"], serde_json::json!(1));
    assert_eq!(overview["total_entry_point_candidates"], serde_json::json!(0));
    assert_eq!(overview["entry_point_candidates_returned"], serde_json::json!(0));
    assert_eq!(
        overview["limitations"],
        serde_json::json!([
            "source_fact_aggregate_only_no_relationship_inference",
            "top_level_path_buckets_are_not_package_or_ownership_boundaries",
            "function_named_main_candidates_are_not_runtime_entry_point_proof"
        ])
    );
    assert_eq!(
        overview["source_roots"],
        serde_json::json!([
            {
                "kind": "top_level_directory",
                "path": "rwp1:h:737263",
                "files": 2,
                "declarations": 4
            }
        ])
    );
    assert!(overview["files"].as_array().is_some_and(|files| files.len() == 1));
    let encoded = overview.to_string();
    assert!(!encoded.contains(REPOSITORY_ID));
    assert!(!encoded.contains(repository.to_string_lossy().as_ref()));
    assert!(!encoded.contains(database.to_string_lossy().as_ref()));
}

#[test]
fn test_markers_are_bounded_generation_pinned_and_do_not_claim_execution() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let marker_path = repository.join("tests/marker.rs");
    fs::create_dir_all(marker_path.parent().expect("test marker parent"))
        .expect("test marker directory should be created");
    fs::write(&marker_path, "#[test]\nfn parser_marker() {}\n")
        .expect("test marker fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "tests/marker.rs"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    let database = directory.database();
    assert!(index(&repository, &database, REPOSITORY_ID).status.success());

    let output = test_markers(&database, REPOSITORY_ID, "rust", "tests/", "1");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let output = String::from_utf8(output.stdout).expect("test marker output must be UTF-8");
    let output: serde_json::Value = serde_json::from_str(&output).expect("test marker JSON");
    assert_eq!(output["schema_version"], serde_json::json!(1));
    assert_eq!(output["test_markers_profile"], serde_json::json!(1));
    assert_eq!(output["generation"], serde_json::json!(1));
    assert_eq!(output["availability"], serde_json::json!("complete"));
    assert_eq!(output["markers_returned"], serde_json::json!(1));
    assert_eq!(output["markers_total"], serde_json::json!(1));
    assert_eq!(output["truncated"], serde_json::json!(false));
    assert_eq!(
        output["language_coverage"],
        serde_json::json!([
            {
                "language": "rust",
                "indexed_files": 1,
                "supported_files": 1,
                "unsupported_files": 0,
                "emitted_markers": 1,
            }
        ])
    );
    assert_eq!(
        output["limitation"],
        serde_json::json!("raw_syntax_observations_only_not_test_execution_or_relationship_resolution")
    );
    assert_eq!(output["markers"][0]["kind"], serde_json::json!("test_marker"));
    assert!(output["markers"][0]["path"]
        .as_str()
        .is_some_and(|path| path.starts_with("rwp1:h:")));
    assert!(
        output["output_bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes >= u64::try_from(output.to_string().len()).expect("JSON length"))
    );
    let encoded = output.to_string();
    assert!(!encoded.contains(REPOSITORY_ID));
    assert!(!encoded.contains(repository.to_string_lossy().as_ref()));
    assert!(!encoded.contains(database.to_string_lossy().as_ref()));
}

#[test]
fn syntax_site_search_is_bounded_generation_pinned_and_not_a_relationship_claim() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let caller_path = repository.join("src/caller.rs");
    fs::write(&caller_path, "pub fn run() {}\npub fn invoke() { run(); }\n")
        .expect("raw target fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "src/caller.rs"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    let database = directory.database();
    assert!(index(&repository, &database, REPOSITORY_ID).status.success());

    let output = syntax_site_search(&database, REPOSITORY_ID, "run", "1");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let output = String::from_utf8(output.stdout).expect("syntax-site output must be UTF-8");
    let output: serde_json::Value = serde_json::from_str(&output).expect("syntax-site JSON");
    assert_eq!(output["schema_version"], serde_json::json!(1));
    assert_eq!(output["syntax_site_search_profile"], serde_json::json!(1));
    assert_eq!(output["generation"], serde_json::json!(1));
    assert_eq!(output["availability"], serde_json::json!("complete"));
    assert_eq!(output["sites_returned"], serde_json::json!(1));
    assert!(output["sites_total"].as_u64().is_some_and(|count| count >= 1));
    assert_eq!(
        output["truncated"],
        serde_json::json!(output["sites_returned"].as_u64() < output["sites_total"].as_u64())
    );
    assert_eq!(
        output["limitation"],
        serde_json::json!(
            "exact_raw_target_syntax_observations_only_no_target_resolution_or_inferred_edges"
        )
    );
    assert_eq!(output["sites"][0]["raw_target"], serde_json::json!("run"));
    assert_eq!(
        output["sites"][0]["target_resolution"],
        serde_json::json!("not_attempted_no_resolution_profile")
    );
    let encoded = output.to_string();
    assert!(!encoded.contains(REPOSITORY_ID));
    assert!(!encoded.contains(repository.to_string_lossy().as_ref()));
    assert!(!encoded.contains(database.to_string_lossy().as_ref()));
}

#[test]
fn architecture_overview_output_bytes_cover_encoded_nested_paths() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let component = "segment".repeat(3);
    let relative = std::iter::repeat_n(component.as_str(), 5)
        .chain(std::iter::once("main.rs"))
        .collect::<Vec<_>>()
        .join("/");
    let source = repository.join(&relative);
    fs::create_dir_all(source.parent().expect("long fixture source should have a parent"))
        .expect("long fixture source parent should be created");
    fs::write(&source, "pub fn main() {}\n").expect("long fixture source should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--"])
        .arg(&relative)
        .status()
        .expect("Git should start");
    assert!(status.success());

    let database = directory.database();
    let indexed = index(&repository, &database, REPOSITORY_ID);
    assert!(indexed.status.success());
    let overview = architecture_overview(&database, REPOSITORY_ID, "1");
    assert!(overview.status.success());
    assert!(overview.stderr.is_empty());
    let encoded = String::from_utf8(overview.stdout).expect("architecture overview must be UTF-8");
    let serialized = encoded
        .strip_suffix('\n')
        .expect("architecture overview output should end with a newline");
    let overview: serde_json::Value =
        serde_json::from_str(serialized).expect("architecture overview JSON");
    let declared = overview["output_bytes"]
        .as_u64()
        .expect("output byte receipt should be an unsigned integer");
    assert!(
        declared >= u64::try_from(serialized.len()).expect("serialized length should fit"),
        "declared output byte receipt must cover the encoded CLI response"
    );
    assert!(declared <= 512 * 1024);
    assert!(overview["files"][0]["path"]
        .as_str()
        .is_some_and(|path| path.starts_with("rwp1:h:")));
    assert!(overview["entry_point_candidates"][0]["path"]
        .as_str()
        .is_some_and(|path| path.starts_with("rwp1:h:")));
}

#[test]
fn cli_indexes_searches_retrieves_and_reuses_typescript_and_tsx() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    fs::create_dir_all(repository.join("web"))
        .expect("TypeScript fixture directory should be created");
    fs::write(
        repository.join("web/api.ts"),
        "export function loadFrontend() {}\n",
    )
    .expect("TypeScript fixture should be written");
    fs::write(
        repository.join("web/view.tsx"),
        "export function FrontendView() { return <main />; }\n",
    )
    .expect("TSX fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "web/api.ts", "web/view.tsx"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    let database = directory.database();

    let first = index(&repository, &database, REPOSITORY_ID);
    assert!(first.status.success());
    assert!(first.stderr.is_empty());
    let first = String::from_utf8(first.stdout).expect("index report must be UTF-8");
    assert!(first.contains("repository_paths=5\n"));
    assert!(first.contains("indexed_typescript_files=1\n"));
    assert!(first.contains("analyzed_typescript_files=1\n"));
    assert!(first.contains("indexed_tsx_files=1\n"));
    assert!(first.contains("analyzed_tsx_files=1\n"));
    assert!(first.contains("skipped_unsupported_paths=1\n"));
    assert!(first.contains("symbol_facts=6\n"));

    let mut producer_manifests = Vec::new();
    for (query, language, name, declaration) in [
        (
            "loadFrontend",
            "typescript",
            "loadFrontend",
            "function loadFrontend() {}",
        ),
        (
            "FrontendView",
            "tsx",
            "FrontendView",
            "function FrontendView() { return <main />; }",
        ),
    ] {
        let searched = search(&database, REPOSITORY_ID, query, "1");
        assert!(searched.status.success());
        assert!(searched.stderr.is_empty());
        let searched = String::from_utf8(searched.stdout).expect("search report must be UTF-8");
        assert_eq!(report_value(&searched, "match_0_language"), language);
        assert_eq!(report_value(&searched, "match_0_kind"), "function");
        assert_eq!(report_value(&searched, "match_0_name"), name);
        producer_manifests
            .push(report_value(&searched, "match_0_producer_manifest_sha256").to_owned());
        assert_symbol_get_success(
            symbol_get_from_search(&repository, &database, REPOSITORY_ID, &searched),
            language,
            name,
            declaration,
        );
    }
    assert_eq!(producer_manifests.len(), 2);
    assert_ne!(producer_manifests[0], producer_manifests[1]);

    let second = index(&repository, &database, REPOSITORY_ID);
    assert!(second.status.success());
    assert!(second.stderr.is_empty());
    let second = String::from_utf8(second.stdout).expect("index report must be UTF-8");
    assert!(second.contains("reused_typescript_files=1\n"));
    assert!(second.contains("analyzed_typescript_files=0\n"));
    assert!(second.contains("reused_tsx_files=1\n"));
    assert!(second.contains("analyzed_tsx_files=0\n"));
}

#[test]
fn cli_indexes_searches_retrieves_and_reuses_python_and_stub_files() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    fs::create_dir_all(repository.join("sdk"))
        .expect("Python fixture directory should be created");
    fs::write(
        repository.join("sdk/client.py"),
        "class Client:\n    def send(self): pass\n",
    )
    .expect("Python fixture should be written");
    fs::write(
        repository.join("sdk/types.pyi"),
        "class Response:\n    status: int\n",
    )
    .expect("Python stub fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "sdk/client.py", "sdk/types.pyi"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    let database = directory.database();

    let first = index(&repository, &database, REPOSITORY_ID);
    assert!(first.status.success());
    assert!(first.stderr.is_empty());
    let first = String::from_utf8(first.stdout).expect("index report must be UTF-8");
    assert!(first.contains("repository_paths=5\n"));
    assert!(first.contains("indexed_python_files=2\n"));
    assert!(first.contains("analyzed_python_files=2\n"));
    assert!(first.contains("skipped_unsupported_paths=1\n"));
    assert!(first.contains("symbol_facts=7\n"));

    for (query, name, declaration) in [
        ("send", "send", "def send(self): pass"),
        (
            "Response",
            "Response",
            "class Response:\n    status: int",
        ),
    ] {
        let searched = search(&database, REPOSITORY_ID, query, "1");
        assert!(searched.status.success());
        assert!(searched.stderr.is_empty());
        let searched = String::from_utf8(searched.stdout).expect("search report must be UTF-8");
        assert_eq!(report_value(&searched, "match_0_language"), "python");
        assert_eq!(report_value(&searched, "match_0_name"), name);
        assert_symbol_get_success(
            symbol_get_from_search(&repository, &database, REPOSITORY_ID, &searched),
            "python",
            name,
            declaration,
        );
    }

    let context = repowitness_os([
        OsStr::new("context-build"),
        OsStr::new("--repository-id"),
        OsStr::new(REPOSITORY_ID),
        OsStr::new("--database"),
        database.as_os_str(),
        OsStr::new("--root"),
        repository.as_os_str(),
        OsStr::new("--intent"),
        OsStr::new("send"),
        OsStr::new("--budget"),
        OsStr::new("4096"),
        OsStr::new("--limit"),
        OsStr::new("1"),
    ]);
    assert!(context.status.success());
    assert!(context.stderr.is_empty());
    let context = String::from_utf8(context.stdout).expect("context report must be UTF-8");
    assert!(context.contains("context_item_0_tier=syntax\n"));
    assert!(context.contains("context_item_0_kind=syntax\n"));
    assert!(context.contains("context_item_0_declaration_encoding=utf8\n"));
    assert!(context.contains(
        "context_item_0_declaration_data_json=\"def send(self): pass\"\n"
    ));

    let second = index(&repository, &database, REPOSITORY_ID);
    assert!(second.status.success());
    assert!(second.stderr.is_empty());
    let second = String::from_utf8(second.stdout).expect("index report must be UTF-8");
    assert!(second.contains("reused_python_files=2\n"));
    assert!(second.contains("analyzed_python_files=0\n"));
}

#[test]
fn explicit_configuration_controls_real_index_search_and_diagnostics() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let configuration = directory.0.join("repository.toml");
    fs::write(
        &configuration,
        concat!(
            "schema_version = 1\n",
            "[preferences]\n",
            "query_results = 1\n",
            "[policy]\n",
            "allowed_languages = [\"rust\"]\n",
        ),
    )
    .expect("configuration fixture should be written");

    let indexed = repowitness_os([
        OsStr::new("index"),
        OsStr::new("--repository-config"),
        configuration.as_os_str(),
        OsStr::new("--repository-id"),
        OsStr::new(REPOSITORY_ID),
        OsStr::new("--database"),
        database.as_os_str(),
        repository.as_os_str(),
    ]);
    assert!(indexed.status.success());
    assert!(indexed.stderr.is_empty());
    let indexed = String::from_utf8(indexed.stdout).expect("index report must be UTF-8");
    assert!(indexed.contains("indexed_rust_files=1\n"));
    assert!(indexed.contains("indexed_go_files=0\n"));
    assert!(indexed.contains("skipped_policy_paths=1\n"));
    assert!(!indexed.contains(configuration.to_string_lossy().as_ref()));

    let searched = repowitness_os([
        OsStr::new("search"),
        OsStr::new("--repository-id"),
        OsStr::new(REPOSITORY_ID),
        OsStr::new("--query"),
        OsStr::new("Widget"),
        OsStr::new("--limit"),
        OsStr::new("20"),
        OsStr::new("--repository-config"),
        configuration.as_os_str(),
        OsStr::new("--database"),
        database.as_os_str(),
    ]);
    assert!(searched.status.success());
    assert!(searched.stderr.is_empty());
    let searched = String::from_utf8(searched.stdout).expect("search report must be UTF-8");
    assert!(searched.contains("matches_returned=1\n"));
    assert!(searched.contains("matches_total=2\n"));
    assert!(searched.contains("coverage_truncated=1\n"));

    let diagnostics = repowitness_os([
        OsStr::new("diagnostics"),
        OsStr::new("--repository-config"),
        configuration.as_os_str(),
        OsStr::new("--database"),
        database.as_os_str(),
        OsStr::new("--repository-id"),
        OsStr::new(REPOSITORY_ID),
    ]);
    assert!(diagnostics.status.success());
    assert!(diagnostics.stderr.is_empty());
    let diagnostics =
        String::from_utf8(diagnostics.stdout).expect("diagnostics report must be UTF-8");
    assert_eq!(report_value(&diagnostics, "schema_version"), "3");
    assert_eq!(
        report_value(&diagnostics, "configuration_profile"),
        "local"
    );
    assert_eq!(
        report_value(&diagnostics, "configuration_digest_sha256").len(),
        64
    );
    assert!(!diagnostics.contains(configuration.to_string_lossy().as_ref()));
}

fn assert_index_work_counts(
    report: &str,
    reused_rust: u64,
    analyzed_rust: u64,
    reused_go: u64,
    analyzed_go: u64,
) {
    assert!(report.contains(&format!("reused_rust_files={reused_rust}\n")));
    assert!(report.contains(&format!("analyzed_rust_files={analyzed_rust}\n")));
    assert!(report.contains(&format!("reused_go_files={reused_go}\n")));
    assert!(report.contains(&format!("analyzed_go_files={analyzed_go}\n")));
}

fn assert_parser_diagnostics_counts(report: &str, raw: &str, known: &str) {
    assert_eq!(report_value(report, "syntax_error_nodes"), raw);
    assert_eq!(
        report_value(report, "known_parser_limitation_nodes"),
        known
    );
}

#[test]
fn invalid_identity_and_repository_fail_without_creating_a_database_or_leaking_inputs() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let private_repository = directory.0.join("private-missing-repository");
    let private_identity = "rwi1:h:PRIVATE";

    let invalid_identity = index(&private_repository, &database, private_identity);
    assert_eq!(invalid_identity.status.code(), Some(70));
    assert!(invalid_identity.stdout.is_empty());
    let stderr = String::from_utf8(invalid_identity.stderr).expect("diagnostic must be UTF-8");
    assert_eq!(stderr, "error: indexing failed\n");
    assert!(!stderr.contains(private_identity));
    assert!(!stderr.contains("private-missing-repository"));
    assert!(!database.exists());

    let missing_repository = index(&private_repository, &database, REPOSITORY_ID);
    assert_eq!(missing_repository.status.code(), Some(70));
    assert!(missing_repository.stdout.is_empty());
    let stderr = String::from_utf8(missing_repository.stderr).expect("diagnostic must be UTF-8");
    assert_eq!(stderr, "error: indexing failed\n");
    assert!(!stderr.contains(REPOSITORY_ID));
    assert!(!stderr.contains("private-missing-repository"));
    assert!(!database.exists());
}

#[test]
fn worktree_local_database_is_rejected_before_indexing_can_create_it() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = repository.join("private-index.sqlite3");

    let output = index(&repository, &database, REPOSITORY_ID);

    assert_eq!(output.status.code(), Some(70));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("diagnostic must be UTF-8");
    assert_eq!(stderr, "error: indexing failed\n");
    assert!(!stderr.contains(REPOSITORY_ID));
    assert!(!stderr.contains("private-index.sqlite3"));
    assert!(!database.exists());
}

#[test]
fn path_inspection_runs_the_concrete_adapter_and_never_claims_an_index() {
    let output = repowitness(&["inspect-paths", env!("CARGO_MANIFEST_DIR")]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("report must be UTF-8");
    assert!(stdout.contains("status=ok\n"));
    assert!(stdout.contains("repository_paths="));
    assert!(stdout.contains("index_created=false\n"));
    assert!(!stdout.contains(env!("CARGO_MANIFEST_DIR")));

    let output = repowitness(&["inspect-paths", "repowitness-path-that-does-not-exist"]);
    assert_eq!(output.status.code(), Some(70));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("diagnostic must be UTF-8");
    assert!(stderr.contains("repository path inspection failed"));
    assert!(!stderr.contains("repowitness-path-that-does-not-exist"));
}

#[test]
fn native_graph_cli_reads_active_and_exact_immutable_contexts() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    fs::write(
        repository.join("src/lib.rs"),
        "pub struct Widget;\npub fn run() {}\npub fn invoke() { run(); }\n",
    )
    .expect("Rust graph fixture should be written");
    let database = directory.database();
    let indexed = index(&repository, &database, REPOSITORY_ID);
    assert!(
        indexed.status.success(),
        "indexing failed: {}",
        String::from_utf8_lossy(&indexed.stderr)
    );

    let status = graph_cli(&database, "status", &[]);
    assert!(status.status.success());
    assert!(status.stderr.is_empty());
    let status: serde_json::Value =
        serde_json::from_slice(&status.stdout).expect("status JSON output");
    assert_eq!(status["schema_version"], serde_json::json!(1));
    assert_eq!(status["availability"], serde_json::json!("complete"));
    let workspace_view = status["context"]["workspace_view"]
        .as_i64()
        .expect("workspace view");
    let graph_generation = status["context"]["graph_generation"]
        .as_i64()
        .expect("graph generation");

    let search = graph_cli(
        &database,
        "search",
        &[
            ("--workspace-view", workspace_view.to_string()),
            ("--graph-generation", graph_generation.to_string()),
            ("--query", "invoke".to_owned()),
            ("--max-results", "5".to_owned()),
        ],
    );
    assert!(
        search.status.success(),
        "graph search failed: {}",
        String::from_utf8_lossy(&search.stderr)
    );
    let search: serde_json::Value =
        serde_json::from_slice(&search.stdout).expect("search JSON output");
    assert_eq!(search["context"], status["context"]);
    assert_eq!(search["matches_returned"], serde_json::json!(1));
    let start = serde_json::json!({
        "type": "definition",
        "definition": search["definitions"][0],
    });

    let trace = graph_cli(
        &database,
        "trace",
        &[
            ("--workspace-view", workspace_view.to_string()),
            ("--graph-generation", graph_generation.to_string()),
            (
                "--start-json",
                serde_json::to_string(&start).expect("start JSON"),
            ),
            ("--direction", "outbound".to_owned()),
            ("--edge-kind", "call".to_owned()),
        ],
    );
    assert!(
        trace.status.success(),
        "graph trace failed: {}",
        String::from_utf8_lossy(&trace.stderr)
    );
    let trace: serde_json::Value =
        serde_json::from_slice(&trace.stdout).expect("trace JSON output");
    assert_eq!(trace["schema_version"], serde_json::json!(1));
    assert!(
        trace["trace"]["edges"]
            .as_array()
            .is_some_and(|edges| !edges.is_empty()),
        "outbound invoke trace should retain its call edge: {trace}"
    );
    let rendered = serde_json::to_string(&trace).expect("rendered output");
    assert!(!rendered.contains(database.to_string_lossy().as_ref()));
    assert!(!rendered.contains(repository.to_string_lossy().as_ref()));
}

fn graph_cli(database: &Path, operation: &str, options: &[(&str, String)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_repowitness"));
    command
        .args(["graph", operation, "--repository-id", REPOSITORY_ID])
        .arg("--database")
        .arg(database);
    for (option, value) in options {
        command.arg(option).arg(value);
    }
    command.output().expect("graph CLI should start")
}
