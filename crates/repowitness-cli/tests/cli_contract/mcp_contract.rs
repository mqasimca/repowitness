const SUPPORTED_SYMBOL_KIND_QUERIES: [&str; 15] = [
    "function",
    "method",
    "struct",
    "enum",
    "union",
    "trait",
    "module",
    "type_alias",
    "constant",
    "static",
    "macro",
    "interface",
    "defined_type",
    "variable",
    "class",
];
const MEMORY_YAML: &str = include_str!(
    "../../../repowitness-local/tests/fixtures/memory-v1/commit.yaml"
);

#[cfg(windows)]
use repowitness_local::{LocalMemoryWriteRequest, write_local_memory};

#[test]
fn mcp_verify_returns_a_fenced_receipt_and_never_substitutes_stale_source() {
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
        .expect("fixture base should be UTF-8")
        .trim()
        .to_owned();
    let database = directory.database();
    assert!(index(&repository, &database, REPOSITORY_ID).status.success());
    fs::write(
        repository.join("src/lib.rs"),
        "pub struct Widget;\nimpl Widget { pub fn changed() {} }\n",
    )
    .expect("fixture change should be written");

    let (child, mut input, mut output) = start_mcp(&repository, &database);
    initialize_mcp(&mut input, &mut output);
    let receipt = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 299,
            "method": "tools/call",
            "params": {
                "name": "verify",
                "arguments": {"base": base, "intent": "Widget"}
            }
        }),
    );
    assert_eq!(
        receipt["result"]["isError"],
        serde_json::json!(false),
        "verify response: {receipt}"
    );
    let receipt = &receipt["result"]["structuredContent"];
    assert_eq!(receipt["schema_version"], serde_json::json!(1));
    assert_eq!(receipt["base"], serde_json::json!(base));
    assert_eq!(receipt["changes"][0]["kind"], serde_json::json!("modified"));
    assert_eq!(
        receipt["indexed_context_availability"],
        serde_json::json!("unavailable")
    );
    assert_eq!(
        receipt["indexed_context_reason"],
        serde_json::json!("stale_source")
    );
    assert!(receipt.get("indexed_snapshot_sha256").is_none());
    assert_eq!(receipt["index_worktree_alignment"], serde_json::json!("unverified"));
    assert_eq!(receipt["verdict"], serde_json::json!("not_provided"));
    stop_mcp(child, input, output);
}

fn memory_write_state(repository: &Path) -> (bool, bool, bool, bool, bool) {
    let memory = repository.join(".code-memory");
    let records = memory.join("records");
    let target = records.join("mem_00000000000000000000000000.yaml");
    let temporary_exists = fs::read_dir(&records)
        .ok()
        .into_iter()
        .flatten()
        .any(|entry| {
            entry
                .ok()
                .is_some_and(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        });
    (
        memory.is_dir(),
        records.is_dir(),
        memory.join(".repowitness-write.lock").is_file(),
        target.is_file(),
        temporary_exists,
    )
}

#[cfg(windows)]
#[test]
fn local_memory_write_is_available_before_the_mcp_contract() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let yaml = MEMORY_YAML.replace(
        "rwi1:h:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        REPOSITORY_ID,
    );
    let result = write_local_memory(
        LocalMemoryWriteRequest::from_bytes(&repository, yaml.as_bytes(), REPOSITORY_ID),
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );

    assert!(
        result.is_ok(),
        "direct local memory write failed with a safe category: {:?}",
        result.err()
    );
}

#[test]
fn mcp_memory_manage_is_process_level_default_deny_and_explicitly_enabled() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();

    let (child, mut input, mut output) = start_mcp(&repository, &database);
    initialize_mcp(&mut input, &mut output);
    assert_mcp_tools(&mut input, &mut output);
    let denied = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "tools/call",
            "params": {
                "name": "memory_manage",
                "arguments": {"operation": "import_history"}
            }
        }),
    );
    assert!(denied.get("error").is_some());
    stop_mcp(child, input, output);

    let (child, mut input, mut output) =
        start_mcp_with_memory_writes(&repository, &database);
    initialize_mcp(&mut input, &mut output);
    let listed = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 21,
            "method": "tools/list",
            "params": {}
        }),
    );
    let tools = listed["result"]["tools"].as_array().expect("tool list");
    let manage = tools
        .iter()
        .find(|tool| tool["name"] == serde_json::json!("memory_manage"))
        .expect("explicit mutation tool");
    assert_eq!(
        manage["annotations"]["readOnlyHint"],
        serde_json::json!(false)
    );
    assert_eq!(
        manage["annotations"]["destructiveHint"],
        serde_json::json!(true)
    );

    let yaml = MEMORY_YAML.replace(
        "rwi1:h:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        REPOSITORY_ID,
    );
    let written = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 22,
            "method": "tools/call",
            "params": {
                "name": "memory_manage",
                "arguments": {
                    "operation": "write",
                    "record_yaml": yaml,
                }
            }
        }),
    );
    assert_eq!(written["id"], serde_json::json!(22));
    let state = memory_write_state(&repository);
    assert_eq!(
        written["result"]["isError"],
        serde_json::json!(false),
        "memory write returned a tool error; response={written}; \
         state=(memory_directory={}, records_directory={}, write_lease={}, target={}, temporary={})",
        state.0,
        state.1,
        state.2,
        state.3,
        state.4,
    );
    let written = written["result"]["structuredContent"]
        .as_object()
        .expect("successful memory write must include structured content");
    assert_eq!(written["schema_version"], serde_json::json!(2));
    assert_eq!(
        written["receipt"]["operation"],
        serde_json::json!("write")
    );
    assert_eq!(written["receipt"]["created"], serde_json::json!(true));
    assert!(
        repository
            .join(".code-memory/records/mem_00000000000000000000000000.yaml")
            .is_file()
    );
    stop_mcp(child, input, output);
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one installed-binary fixture must keep registry admission, schema, isolation, and path-redaction assertions in one auditable transport round-trip"
)]
fn mcp_registry_routes_two_independent_indexes_without_default_or_path_disclosure() {
    let directory = TempDirectory::new();
    let first_repository = fixture_repository(&directory);
    fs::write(
        first_repository.join("src/lib.rs"),
        "pub fn registry_first() {}\n",
    )
    .expect("first fixture source");
    let status = Command::new("git")
        .current_dir(&first_repository)
        .args(["add", "--", "src/lib.rs"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    let first_database = directory.database();
    let first_id = REPOSITORY_ID;
    assert!(index(&first_repository, &first_database, first_id).status.success());

    let second_repository = directory.0.join("repository-two");
    fs::create_dir_all(second_repository.join("src")).expect("second fixture directory");
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .arg(&second_repository)
        .status()
        .expect("Git should start");
    assert!(status.success());
    fs::write(
        second_repository.join("src/lib.rs"),
        "pub fn registry_second() {}\n",
    )
    .expect("second fixture source");
    let status = Command::new("git")
        .current_dir(&second_repository)
        .args(["add", "--", "src/lib.rs"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    let second_database = directory.0.join("index-two.sqlite3");
    let second_id = concat!(
        "rwi1:h:",
        "C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3"
    );
    assert!(
        index(&second_repository, &second_database, second_id)
            .status
            .success()
    );

    let registry = directory.0.join("mcp-registry.json");
    fs::write(
        &registry,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "repositories": [
                {"repository_id": first_id, "root": first_repository, "database": first_database},
                {"repository_id": second_id, "root": second_repository, "database": second_database}
            ]
        }))
        .expect("registry JSON"),
    )
    .expect("write registry");

    let (child, mut input, mut output) = start_mcp_with_registry(&registry);
    initialize_mcp(&mut input, &mut output);
    let listed = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 200,
            "method": "tools/list",
            "params": {}
        }),
    );
    let tools = listed["result"]["tools"].as_array().expect("tool list");
    assert_eq!(tools.len(), 24);
    assert!(tools.iter().all(|tool| tool["name"] != "memory_manage"));
    let code_search = tools
        .iter()
        .find(|tool| tool["name"] == "code_search")
        .expect("code search schema");
    assert_eq!(
        code_search["inputSchema"]["properties"]["repository_id"]["enum"],
        serde_json::json!([first_id, second_id])
    );
    assert!(code_search["inputSchema"]["required"]
        .as_array()
        .is_some_and(|required| required.contains(&serde_json::json!("repository_id"))));

    for arguments in [
        serde_json::json!({"query": "registry_first"}),
        serde_json::json!({"query": "registry_first", "repository_id": "unknown"}),
    ] {
        let response = mcp_request(
            &mut input,
            &mut output,
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 201,
                "method": "tools/call",
                "params": {"name": "code_search", "arguments": arguments}
            }),
        );
        assert!(response.get("error").is_some());
        let response = response.to_string();
        assert!(!response.contains(directory.0.to_string_lossy().as_ref()));
    }

    for (id, query, expected_name) in [
        (first_id, "registry_first", "registry_first"),
        (second_id, "registry_second", "registry_second"),
    ] {
        let response = mcp_request(
            &mut input,
            &mut output,
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 202,
                "method": "tools/call",
                "params": {
                    "name": "code_search",
                    "arguments": {"repository_id": id, "query": query, "max_results": 5}
                }
            }),
        );
        assert_eq!(response["result"]["isError"], serde_json::json!(false));
        assert!(response.to_string().contains(expected_name));
    }
    stop_mcp(child, input, output);
}

#[test]
fn malformed_mcp_registry_fails_before_transport_startup_without_path_disclosure() {
    let directory = TempDirectory::new();
    let registry = directory.0.join("malformed-registry.json");
    fs::write(&registry, b"{not JSON").expect("write malformed registry");
    let result = Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .args(["mcp-serve", "--registry"])
        .arg(&registry)
        .output()
        .expect("registry startup should return");
    assert_eq!(result.status.code(), Some(70));
    assert!(result.stdout.is_empty());
    assert_eq!(
        result.stderr,
        b"error: MCP repository registry admission failed\n"
    );
    assert!(!String::from_utf8_lossy(&result.stderr).contains(directory.0.to_string_lossy().as_ref()));
}

#[cfg(unix)]
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one installed-binary fixture keeps catalog admission and unchanged, configuration-changed, and source-changed publication checks together"
)]
fn mcp_catalog_onboards_the_current_worktree_and_defaults_to_it() {
    let directory = TempDirectory::new();
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700))
        .expect("fixture parent should be private");
    let repository = fixture_repository(&directory);
    let state_directory = directory.0.join("private-state");
    let (child, mut input, mut output) = start_mcp_with_catalog(&repository, &state_directory);
    initialize_mcp(&mut input, &mut output);
    let listed = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 230,
            "method": "tools/list",
            "params": {}
        }),
    );
    let code_search = listed["result"]["tools"]
        .as_array()
        .expect("tool list")
        .iter()
        .find(|tool| tool["name"] == "code_search")
        .expect("code-search tool");
    assert!(code_search["inputSchema"]["required"]
        .as_array()
        .is_some_and(|required| !required.contains(&serde_json::json!("repository_id"))));
    assert_eq!(
        code_search["inputSchema"]["properties"]["repository_id"]["enum"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let response = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 231,
            "method": "tools/call",
            "params": {"name": "code_search", "arguments": {"query": "Widget"}}
        }),
    );
    assert_eq!(response["result"]["isError"], serde_json::json!(false));
    assert!(response.to_string().contains("Widget"));
    assert!(!response
        .to_string()
        .contains(directory.0.to_string_lossy().as_ref()));
    let first_generation = mcp_diagnostics_generation(&mut input, &mut output, 233);
    stop_mcp(child, input, output);

    let catalog = state_directory.join("repowitness/mcp-catalog-v1.json");
    let catalog = fs::read(catalog).expect("private catalog");
    let catalog: serde_json::Value = serde_json::from_slice(&catalog).expect("catalog JSON");
    assert_eq!(catalog["schema_version"], serde_json::json!(1));
    assert_eq!(catalog["repositories"].as_array().map(Vec::len), Some(1));

    let (child, mut input, mut output) = start_mcp_with_catalog(&repository, &state_directory);
    initialize_mcp(&mut input, &mut output);
    let listed = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 232,
            "method": "tools/list",
            "params": {}
        }),
    );
    let code_search = listed["result"]["tools"]
        .as_array()
        .expect("tool list")
        .iter()
        .find(|tool| tool["name"] == "code_search")
        .expect("code-search tool");
    assert_eq!(
        code_search["inputSchema"]["properties"]["repository_id"]["enum"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let second_generation = mcp_diagnostics_generation(&mut input, &mut output, 234);
    assert_eq!(second_generation, first_generation);
    stop_mcp(child, input, output);

    let configuration = directory.0.join("catalog-user.toml");
    fs::write(
        &configuration,
        "schema_version = 1\n[preferences]\nquery_results = 1\n",
    )
    .expect("catalog user configuration should be written");
    let (child, mut input, mut output) =
        start_mcp_with_catalog_and_user_config(&repository, &state_directory, &configuration);
    initialize_mcp(&mut input, &mut output);
    let configuration_generation = mcp_diagnostics_generation(&mut input, &mut output, 235);
    assert!(configuration_generation > second_generation);
    stop_mcp(child, input, output);

    let (child, mut input, mut output) =
        start_mcp_with_catalog_and_user_config(&repository, &state_directory, &configuration);
    initialize_mcp(&mut input, &mut output);
    let repeated_configuration_generation =
        mcp_diagnostics_generation(&mut input, &mut output, 236);
    assert_eq!(repeated_configuration_generation, configuration_generation);
    stop_mcp(child, input, output);

    fs::write(
        repository.join("src/lib.rs"),
        "pub struct Widget;\nimpl Widget { pub fn run() {} pub fn changed() {} }\n",
    )
    .expect("changed fixture source should be written");
    let (child, mut input, mut output) =
        start_mcp_with_catalog_and_user_config(&repository, &state_directory, &configuration);
    initialize_mcp(&mut input, &mut output);
    let changed_generation = mcp_diagnostics_generation(&mut input, &mut output, 237);
    assert!(changed_generation > repeated_configuration_generation);
    stop_mcp(child, input, output);
}

#[cfg(unix)]
#[test]
#[allow(
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    reason = "one installed-binary fixture keeps explicit product-workspace admission, atomic catalog refresh, cross-member routing, source-view receipts, and path-redaction assertions together"
)]
fn codex_workspace_catalog_refreshes_declared_members_and_routes_connected_source_slots() {
    let directory = TempDirectory::new();
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700))
        .expect("fixture parent should be private");
    let first_repository = fixture_repository(&directory);
    let status = Command::new("git")
        .current_dir(&first_repository)
        .args([
            "-c",
            "user.name=RepoWitness Test",
            "-c",
            "user.email=repowitness-test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "first connected source",
        ])
        .status()
        .expect("first fixture should commit");
    assert!(status.success());

    let second_repository = directory.0.join("repository-two");
    fs::create_dir_all(second_repository.join("src")).expect("second fixture directory");
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .arg(&second_repository)
        .status()
        .expect("second repository should initialize");
    assert!(status.success());
    fs::write(
        second_repository.join("src/lib.rs"),
        "pub struct ConnectedWidget;\npub fn connected_entry() {}\n",
    )
    .expect("second fixture source");
    let status = Command::new("git")
        .current_dir(&second_repository)
        .args([
            "add",
            "--",
            "src/lib.rs",
        ])
        .status()
        .expect("second fixture should stage");
    assert!(status.success());
    let status = Command::new("git")
        .current_dir(&second_repository)
        .args([
            "-c",
            "user.name=RepoWitness Test",
            "-c",
            "user.email=repowitness-test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "second connected source",
        ])
        .status()
        .expect("second fixture should commit");
    assert!(status.success());

    let created = Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .args(["codex", "workspace", "create", "--name", "product-stack"])
        .arg("--repository")
        .arg(&first_repository)
        .arg("--repository")
        .arg(&second_repository)
        .arg("--codex-home")
        .arg(&directory.0)
        .output()
        .expect("Codex workspace creation should start");
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    assert!(created.stderr.is_empty());
    let created = String::from_utf8(created.stdout).expect("workspace receipt");
    assert!(created.contains("operation=codex-workspace-create\n"));
    assert!(created.contains("workspace=product-stack\n"));
    assert!(created.contains("members=2\nindex=published\n"));
    for sensitive in [
        first_repository.to_string_lossy().as_ref(),
        second_repository.to_string_lossy().as_ref(),
        directory.0.to_string_lossy().as_ref(),
    ] {
        assert!(!created.contains(sensitive));
    }

    let listed = Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .args(["codex", "workspace", "list", "--codex-home"])
        .arg(&directory.0)
        .output()
        .expect("Codex workspace list should start");
    assert!(listed.status.success());
    assert_eq!(
        listed.stdout,
        b"status=ok\noperation=codex-workspace-list\nworkspaces=1\nworkspace_0=product-stack\nworkspace_0_members=2\n"
    );
    assert!(listed.stderr.is_empty());

    let state_directory = directory.0.join("repowitness-state");
    let (child, mut input, mut output) = start_mcp_with_catalog(&first_repository, &state_directory);
    initialize_mcp(&mut input, &mut output);
    let listed = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 240,
            "method": "tools/list",
            "params": {}
        }),
    );
    let tools = listed["result"]["tools"].as_array().expect("tool list");
    let code_search = tools
        .iter()
        .find(|tool| tool["name"] == "code_search")
        .expect("code search schema");
    let identities = code_search["inputSchema"]["properties"]["repository_id"]["enum"]
        .as_array()
        .expect("connected member identities");
    assert_eq!(identities.len(), 2);
    assert!(code_search["inputSchema"]["required"]
        .as_array()
        .is_some_and(|required| !required.contains(&serde_json::json!("repository_id"))));

    let first = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 241,
            "method": "tools/call",
            "params": {
                "name": "code_search",
                "arguments": {"query": "Widget", "max_results": 5}
            }
        }),
    );
    assert_eq!(
        first["result"]["isError"],
        serde_json::json!(false),
        "default connected-workspace code search failed: {first}"
    );
    assert!(first.to_string().contains("Widget"));

    let second_identity = identities
        .iter()
        .find(|identity| {
            let response = mcp_request(
                &mut input,
                &mut output,
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 242,
                    "method": "tools/call",
                    "params": {
                        "name": "code_search",
                        "arguments": {
                            "repository_id": identity,
                            "query": "ConnectedWidget",
                            "max_results": 5
                        }
                    }
                }),
            );
            response["result"]["isError"] == serde_json::json!(false)
                && response.to_string().contains("ConnectedWidget")
        })
        .cloned()
        .expect("one explicit connected member should expose its own facts");
    let relevant_paths = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 245,
            "method": "tools/call",
            "params": {
                "name": "locate_relevant_paths",
                "arguments": {
                    "repository_id": second_identity,
                    "query": "ConnectedWidget",
                    "max_paths": 5
                }
            }
        }),
    );
    assert_eq!(
        relevant_paths["result"]["isError"],
        serde_json::json!(false),
        "connected relevant-path search response: {relevant_paths}"
    );
    assert!(relevant_paths["result"]["structuredContent"]["matches_returned"]
        .as_u64()
        .is_some_and(|count| count > 0));
    let code_graph_relevant_paths = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 246,
            "method": "tools/call",
            "params": {
                "name": "code_graph_query",
                "arguments": {
                    "repository_id": second_identity,
                    "operation": "relevant_paths",
                    "query": "ConnectedWidget",
                    "max_paths": 5
                }
            }
        }),
    );
    assert_eq!(
        code_graph_relevant_paths["result"]["isError"],
        serde_json::json!(false),
        "connected code-graph relevant-path response: {code_graph_relevant_paths}"
    );
    assert_eq!(
        code_graph_relevant_paths["result"]["structuredContent"]["operation"],
        serde_json::json!("relevant_paths")
    );
    let architecture_map = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 247,
            "method": "tools/call",
            "params": {
                "name": "architecture_map",
                "arguments": {"repository_id": second_identity, "max_files": 5}
            }
        }),
    );
    assert_eq!(
        architecture_map["result"]["isError"],
        serde_json::json!(false),
        "connected architecture-map response: {architecture_map}"
    );
    assert!(architecture_map["result"]["structuredContent"]["files_returned"]
        .as_u64()
        .is_some_and(|count| count > 0));
    let graph = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 243,
            "method": "tools/call",
            "params": {
                "name": "graph_status",
                "arguments": {"repository_id": second_identity}
            }
        }),
    );
    assert_eq!(graph["result"]["isError"], serde_json::json!(false));
    let graph = &graph["result"]["structuredContent"];
    assert!(graph["context"]["connected_workspace"]
        .as_str()
        .is_some_and(|identity| identity.starts_with("cwi1:h:")));
    let symbols = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 244,
            "method": "tools/call",
            "params": {
                "name": "symbol_search",
                "arguments": {
                    "repository_id": second_identity,
                    "name": "ConnectedWidget",
                    "match_mode": "exact",
                    "max_results": 5
                }
            }
        }),
    );
    assert_eq!(
        symbols["result"]["isError"],
        serde_json::json!(false),
        "symbol search response: {symbols}"
    );
    let symbols = &symbols["result"]["structuredContent"];
    assert!(symbols["source_slot"]
        .as_str()
        .is_some_and(|identity| identity.starts_with("ssi1:h:")));
    let response = format!("{graph}{symbols}");
    assert!(!response.contains(first_repository.to_string_lossy().as_ref()));
    assert!(!response.contains(second_repository.to_string_lossy().as_ref()));
    stop_mcp(child, input, output);

    let removed = Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .args([
            "codex",
            "workspace",
            "remove",
            "--name",
            "product-stack",
            "--codex-home",
        ])
        .arg(&directory.0)
        .output()
        .expect("Codex workspace removal should start");
    assert!(removed.status.success());
    assert_eq!(
        removed.stdout,
        b"status=ok\noperation=codex-workspace-remove\nregistration=removed\nindex_retained=true\n"
    );
    assert!(removed.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn mcp_catalog_rejects_a_non_worktree_before_transport_startup_without_path_disclosure() {
    let directory = TempDirectory::new();
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700))
        .expect("fixture parent should be private");
    let state_directory = directory.0.join("private-state");
    let result = Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .current_dir(&directory.0)
        .args(["mcp-serve", "--catalog", "--catalog-state-dir"])
        .arg(&state_directory)
        .output()
        .expect("catalog startup should return");
    assert_eq!(result.status.code(), Some(70));
    assert!(result.stdout.is_empty());
    assert_eq!(result.stderr, b"error: MCP catalog admission failed\n");
    assert!(
        !String::from_utf8_lossy(&result.stderr).contains(directory.0.to_string_lossy().as_ref())
    );
    assert!(!state_directory.exists());
}

#[cfg(unix)]
#[test]
fn mcp_catalog_rejects_state_inside_the_worktree_before_writing_private_state() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let state_directory = repository.join("private-state");
    let result = Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .current_dir(&repository)
        .args(["mcp-serve", "--catalog", "--catalog-state-dir"])
        .arg(&state_directory)
        .output()
        .expect("catalog startup should return");
    assert_eq!(result.status.code(), Some(70));
    assert!(result.stdout.is_empty());
    assert_eq!(result.stderr, b"error: MCP catalog admission failed\n");
    assert!(!state_directory.exists());
}

#[test]
fn mcp_configuration_policy_fails_before_transport_startup() {
    let directory = TempDirectory::new();
    let repository = directory.repository();
    let database = directory.database();
    let user = directory.0.join("user.toml");
    let repository_configuration = directory.0.join("repository.toml");
    fs::write(
        &user,
        "schema_version = 1\n[policy]\ndeny_memory_writes = true\n",
    )
    .expect("user configuration should be written");
    fs::write(
        &repository_configuration,
        "schema_version = 1\n[policy]\ndeny_memory_writes = false\n",
    )
    .expect("repository configuration should be written");

    let denied = Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .args(["mcp-serve", "--repository-id", REPOSITORY_ID, "--database"])
        .arg(&database)
        .arg("--root")
        .arg(&repository)
        .arg("--repository-config")
        .arg(&repository_configuration)
        .args([
            "--enable-memory-writes",
            "--memory-actor",
            "contract-test-actor",
            "--user-config",
        ])
        .arg(&user)
        .output()
        .expect("denied MCP server should stop");
    assert_eq!(denied.status.code(), Some(70));
    assert!(denied.stdout.is_empty());
    assert_eq!(
        denied.stderr,
        b"error: MCP memory writes are denied by configuration\n"
    );
    assert!(!database.exists());

    fs::write(
        &repository_configuration,
        "schema_version = 1\n[preferences]\nmcp_tool_profile = \"minimal\"\n",
    )
    .expect("unsupported profile configuration should be written");
    let unavailable = Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .args(["mcp-serve", "--repository-id", REPOSITORY_ID, "--database"])
        .arg(&database)
        .arg("--root")
        .arg(&repository)
        .arg("--repository-config")
        .arg(&repository_configuration)
        .output()
        .expect("unsupported MCP profile should stop");
    assert_eq!(unavailable.status.code(), Some(70));
    assert!(unavailable.stdout.is_empty());
    assert_eq!(
        unavailable.stderr,
        b"error: configured MCP tool profile is unavailable\n"
    );
    assert!(!database.exists());
}

#[test]
fn mcp_stdio_indexes_searches_retrieves_and_rejects_a_stale_selector() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    fs::write(
        repository.join("src/lib.rs"),
        "pub struct Widget;\npub fn run() {}\npub fn invoke() { run(); }\n",
    )
    .expect("Rust graph fixture should be written");
    fs::write(
        repository.join("src/frontend.ts"),
        "export function loadFrontend() {}\n",
    )
    .expect("TypeScript fixture should be written");
    fs::write(
        repository.join("src/frontend.tsx"),
        "export function FrontendView() { return <main />; }\n",
    )
    .expect("TSX fixture should be written");
    fs::write(
        repository.join("src/client.py"),
        "class ApiClient:\n    def send(self): pass\n",
    )
    .expect("Python fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args([
            "add",
            "--",
            "src/frontend.ts",
            "src/frontend.tsx",
            "src/client.py",
        ])
        .status()
        .expect("Git should start");
    assert!(status.success());
    let database = directory.database();
    assert!(
        index(&repository, &database, REPOSITORY_ID)
            .status
            .success()
    );
    let (child, mut input, mut output) = start_mcp(&repository, &database);
    initialize_mcp(&mut input, &mut output);
    assert_mcp_primary_discovery_tools(&mut input, &mut output);
    let exact_arguments = mcp_search_selector(&mut input, &mut output);
    assert_mcp_symbol(&mut input, &mut output, &exact_arguments);
    assert_mcp_evidence_context(
        &mut input,
        &mut output,
        14,
        "Widget",
        "pub struct Widget;",
    );
    assert_mcp_evidence_context(&mut input, &mut output, 18, "Widget", "pub struct Widget;");
    assert_mcp_go_round_trip(&mut input, &mut output);
    assert_mcp_supported_language_round_trip(
        &mut input,
        &mut output,
        8,
        9,
        "loadFrontend",
        "typescript",
        b"function loadFrontend() {}",
    );
    assert_mcp_supported_language_round_trip(
        &mut input,
        &mut output,
        10,
        11,
        "FrontendView",
        "tsx",
        b"function FrontendView() { return <main />; }",
    );
    assert_mcp_supported_language_round_trip(
        &mut input,
        &mut output,
        12,
        13,
        "send",
        "python",
        b"def send(self): pass",
    );
    assert_mcp_diagnostics_and_absent_memory(&mut input, &mut output, 15, 16);
    assert_mcp_native_graph(&mut input, &mut output);

    assert!(
        index(&repository, &database, REPOSITORY_ID)
            .status
            .success()
    );
    assert_mcp_stale(&mut input, &mut output, &exact_arguments);
    stop_mcp(child, input, output);
}

#[test]
fn mcp_stdio_discovers_and_retrieves_a_python_only_index() {
    let directory = TempDirectory::new();
    let repository = directory.repository();
    fs::create_dir_all(repository.join("src")).expect("fixture source directory");
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .arg(&repository)
        .status()
        .expect("Git should start");
    assert!(status.success());
    fs::write(
        repository.join("src/client.py"),
        "class ApiClient:\n    def send(self): pass\n",
    )
    .expect("Python fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "src/client.py"])
        .status()
        .expect("Git should start");
    assert!(status.success());

    let database = directory.database();
    assert!(
        index(&repository, &database, REPOSITORY_ID)
            .status
            .success()
    );
    let (child, mut input, mut output) = start_mcp(&repository, &database);
    initialize_mcp(&mut input, &mut output);
    assert_mcp_tools(&mut input, &mut output);
    let exact_arguments =
        mcp_first_supported_symbol_selector(&mut input, &mut output, 10)
            .expect("Python-only index should expose a supported symbol kind");
    let symbol = mcp_call_symbol(&mut input, &mut output, 40, &exact_arguments);
    let symbol = &symbol["result"]["structuredContent"];
    assert_eq!(symbol["resolution"], serde_json::json!("confirmed"));
    assert_eq!(symbol["symbol"]["language"], serde_json::json!("python"));
    assert_eq!(
        symbol["symbol"]["declaration_encoding"],
        serde_json::json!("utf8")
    );
    assert!(
        symbol["symbol"]["declaration"]
            .as_str()
            .is_some_and(|declaration| !declaration.is_empty())
    );
    stop_mcp(child, input, output);
}

#[test]
#[ignore = "requires REPOWITNESS_REAL_REPOSITORY, Git, and a readable supported-language worktree"]
fn mcp_stdio_round_trips_an_exact_symbol_from_a_real_repository() {
    let repository = configured_repository();
    let directory = TempDirectory::new();
    let database = directory.database();
    assert!(
        index(&repository, &database, REPOSITORY_ID)
            .status
            .success(),
        "real repository indexing must succeed"
    );
    let (child, mut input, mut output) = start_mcp(&repository, &database);
    initialize_mcp(&mut input, &mut output);
    assert_mcp_tools(&mut input, &mut output);

    let exact_arguments =
        mcp_first_supported_symbol_selector(&mut input, &mut output, 10)
            .expect("real repository must expose at least one indexed supported-language symbol");
    let symbol = mcp_call_symbol(&mut input, &mut output, 40, &exact_arguments);
    let symbol = &symbol["result"]["structuredContent"];
    assert_eq!(symbol["resolution"], serde_json::json!("confirmed"));
    assert!(
        symbol["symbol"]["declaration"]
            .as_str()
            .is_some_and(|declaration| !declaration.is_empty()),
        "exact retrieval must return a non-empty declaration"
    );
    assert!(
        matches!(
            symbol["symbol"]["declaration_encoding"].as_str(),
            Some("utf8" | "lowercase_hex")
        ),
        "exact retrieval must label its declaration representation"
    );
    let source = &symbol["symbol"];
    assert_mcp_evidence_context(
        &mut input,
        &mut output,
        41,
        source["name"].as_str().expect("symbol name"),
        source["declaration"].as_str().expect("declaration data"),
    );
    assert_mcp_diagnostics_and_absent_memory(&mut input, &mut output, 42, 43);
    stop_mcp(child, input, output);
}

fn configured_repository() -> PathBuf {
    let configured = PathBuf::from(
        std::env::var_os("REPOWITNESS_REAL_REPOSITORY")
            .expect("REPOWITNESS_REAL_REPOSITORY must identify a Git worktree"),
    );
    if configured.is_absolute() {
        configured
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("the CLI crate should have a workspace root")
            .join(configured)
    }
}

fn start_mcp(
    repository: &Path,
    database: &Path,
) -> (std::process::Child, ChildStdin, BufReader<ChildStdout>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .args(["mcp-serve", "--repository-id", REPOSITORY_ID, "--database"])
        .arg(database)
        .arg("--root")
        .arg(repository)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("MCP server must start");
    let input = child.stdin.take().expect("piped stdin");
    let output = BufReader::new(child.stdout.take().expect("piped stdout"));
    (child, input, output)
}

fn start_mcp_with_registry(
    registry: &Path,
) -> (std::process::Child, ChildStdin, BufReader<ChildStdout>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .args(["mcp-serve", "--registry"])
        .arg(registry)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("registry MCP server must start");
    let input = child.stdin.take().expect("piped stdin");
    let output = BufReader::new(child.stdout.take().expect("piped stdout"));
    (child, input, output)
}

#[cfg(unix)]
fn start_mcp_with_catalog(
    repository: &Path,
    state_directory: &Path,
) -> (std::process::Child, ChildStdin, BufReader<ChildStdout>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .current_dir(repository)
        .args(["mcp-serve", "--catalog", "--catalog-state-dir"])
        .arg(state_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("catalog MCP server must start");
    let input = child.stdin.take().expect("piped stdin");
    let output = BufReader::new(child.stdout.take().expect("piped stdout"));
    (child, input, output)
}

#[cfg(unix)]
fn start_mcp_with_catalog_and_user_config(
    repository: &Path,
    state_directory: &Path,
    user_configuration: &Path,
) -> (std::process::Child, ChildStdin, BufReader<ChildStdout>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .current_dir(repository)
        .args(["mcp-serve", "--catalog", "--catalog-state-dir"])
        .arg(state_directory)
        .args(["--user-config"])
        .arg(user_configuration)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("catalog MCP server with user configuration must start");
    let input = child.stdin.take().expect("piped stdin");
    let output = BufReader::new(child.stdout.take().expect("piped stdout"));
    (child, input, output)
}

fn start_mcp_with_graph_workspace(
    repository: &Path,
    database: &Path,
    connected_workspace: &str,
    source_slot: &str,
) -> (std::process::Child, ChildStdin, BufReader<ChildStdout>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .args(["mcp-serve", "--repository-id", REPOSITORY_ID, "--database"])
        .arg(database)
        .arg("--root")
        .arg(repository)
        .args([
            "--connected-workspace-id",
            connected_workspace,
            "--source-slot-id",
            source_slot,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("connected-workspace MCP server must start");
    let input = child.stdin.take().expect("piped stdin");
    let output = BufReader::new(child.stdout.take().expect("piped stdout"));
    (child, input, output)
}

fn start_mcp_with_memory_writes(
    repository: &Path,
    database: &Path,
) -> (std::process::Child, ChildStdin, BufReader<ChildStdout>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .args(["mcp-serve", "--repository-id", REPOSITORY_ID, "--database"])
        .arg(database)
        .arg("--root")
        .arg(repository)
        .args([
            "--enable-memory-writes",
            "--memory-actor",
            "contract-test-actor",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("write-capable MCP server must start");
    let input = child.stdin.take().expect("piped stdin");
    let output = BufReader::new(child.stdout.take().expect("piped stdout"));
    (child, input, output)
}

fn initialize_mcp(input: &mut ChildStdin, output: &mut BufReader<ChildStdout>) {
    let initialized = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "repowitness-contract-test", "version": "1"}
            }
        }),
    );
    assert_eq!(
        initialized["result"]["protocolVersion"],
        serde_json::json!("2025-11-25")
    );
    mcp_notification(
        input,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
    );
}

fn mcp_diagnostics_generation(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    request_id: usize,
) -> i64 {
    let response = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/call",
            "params": {"name": "diagnostics", "arguments": {}}
        }),
    );
    assert_eq!(response["result"]["isError"], serde_json::json!(false));
    response["result"]["structuredContent"]["generation"]
        .as_i64()
        .filter(|generation| *generation > 0)
        .expect("diagnostics generation")
}

include!("mcp_contract/read_tools.rs");

fn stop_mcp(
    mut child: std::process::Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
) {
    drop(input);
    drop(output);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if child.try_wait().expect("MCP server status").is_some() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("MCP server did not stop within the bounded shutdown window");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let completed = child.wait_with_output().expect("MCP server must stop");
    assert!(completed.status.success());
    assert!(completed.stdout.is_empty());
    assert!(completed.stderr.is_empty());
}

fn mcp_request(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    request: serde_json::Value,
) -> serde_json::Value {
    serde_json::to_writer(&mut *input, &request).expect("request must serialize");
    input.write_all(b"\n").expect("request delimiter");
    input.flush().expect("request flush");
    let mut line = String::new();
    let bytes = output.read_line(&mut line).expect("MCP response read");
    assert!(bytes > 0, "MCP server closed before responding");
    serde_json::from_str(&line).expect("stdout must contain only JSON-RPC lines")
}

fn mcp_notification(input: &mut ChildStdin, notification: serde_json::Value) {
    serde_json::to_writer(&mut *input, &notification).expect("notification must serialize");
    input.write_all(b"\n").expect("notification delimiter");
    input.flush().expect("notification flush");
}
