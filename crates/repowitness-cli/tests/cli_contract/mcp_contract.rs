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
    assert_mcp_context(
        &mut input,
        &mut output,
        14,
        "Widget",
        "rust",
        "utf8",
        "pub struct Widget;",
    );
    assert_mcp_phase2_context(&mut input, &mut output, 18, "Widget", "pub struct Widget;");
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
    assert_mcp_context(
        &mut input,
        &mut output,
        41,
        source["name"].as_str().expect("symbol name"),
        source["language"].as_str().expect("symbol language"),
        source["declaration_encoding"]
            .as_str()
            .expect("declaration encoding"),
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
