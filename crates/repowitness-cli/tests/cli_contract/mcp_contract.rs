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
    let written = &written["result"]["structuredContent"];
    assert_eq!(written["schema_version"], serde_json::json!(1));
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
fn mcp_stdio_indexes_searches_retrieves_and_rejects_a_stale_selector() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
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
    assert_mcp_tools(&mut input, &mut output);
    let exact_arguments = mcp_search_selector(&mut input, &mut output);
    assert_mcp_symbol(&mut input, &mut output, &exact_arguments);
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
    assert!(
        symbol["symbol"]["declaration_hex"]
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
        symbol["symbol"]["declaration_hex"]
            .as_str()
            .is_some_and(|declaration| !declaration.is_empty()),
        "exact retrieval must return a non-empty declaration"
    );
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

fn assert_mcp_tools(input: &mut ChildStdin, output: &mut BufReader<ChildStdout>) {
    let listed = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
    );
    assert_eq!(
        listed["result"]["tools"]
            .as_array()
            .expect("tool list")
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect::<Vec<_>>(),
        [
            "code_search",
            "context_build",
            "diagnostics",
            "memory_recall",
            "symbol_get"
        ]
    );
}

fn mcp_search_selector(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
) -> serde_json::Value {
    let search = mcp_call_search(input, output, 3, "struct Widget");
    let search = &search["result"]["structuredContent"];
    assert_eq!(search["schema_version"], serde_json::json!(3));
    assert_eq!(search["query_profile"], serde_json::json!(3));
    assert_eq!(search["generation"], serde_json::json!(1));
    assert_eq!(search["resolution"], serde_json::json!("confirmed"));
    assert_eq!(search["matches_returned"], serde_json::json!(1));
    let candidate = &search["matches"][0];
    assert_eq!(candidate["language"], serde_json::json!("rust"));
    serde_json::json!({
        "snapshot_sha256": search["snapshot_sha256"],
        "generation": search["generation"],
        "path": candidate["path"],
        "content_sha256": candidate["content_sha256"],
        "artifact_sha256": candidate["artifact_sha256"],
        "fact_ordinal": candidate["fact_ordinal"]
    })
}

fn mcp_first_search_selector(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    request_id: usize,
    query: &str,
) -> Option<serde_json::Value> {
    let search = mcp_call_search(input, output, request_id, query);
    let search = &search["result"]["structuredContent"];
    let candidate = search["matches"].as_array()?.first()?;
    Some(serde_json::json!({
        "snapshot_sha256": search["snapshot_sha256"],
        "generation": search["generation"],
        "path": candidate["path"],
        "content_sha256": candidate["content_sha256"],
        "artifact_sha256": candidate["artifact_sha256"],
        "fact_ordinal": candidate["fact_ordinal"]
    }))
}

fn mcp_first_supported_symbol_selector(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    first_request_id: usize,
) -> Option<serde_json::Value> {
    SUPPORTED_SYMBOL_KIND_QUERIES
        .into_iter()
        .enumerate()
        .find_map(|(ordinal, query)| {
            mcp_first_search_selector(input, output, first_request_id + ordinal, query)
        })
}

fn mcp_call_search(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    request_id: usize,
    query: &str,
) -> serde_json::Value {
    mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/call",
            "params": {
                "name": "code_search",
                "arguments": {"query": query, "max_results": 5}
            }
        }),
    )
}

fn assert_mcp_symbol(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    exact_arguments: &serde_json::Value,
) {
    let symbol = mcp_call_symbol(input, output, 4, exact_arguments);
    let symbol = &symbol["result"]["structuredContent"];
    assert_eq!(symbol["resolution"], serde_json::json!("confirmed"));
    assert_eq!(symbol["schema_version"], serde_json::json!(3));
    assert_eq!(symbol["symbol_profile"], serde_json::json!(3));
    assert_eq!(symbol["symbol"]["language"], serde_json::json!("rust"));
    assert_eq!(
        symbol["symbol"]["declaration_hex"],
        serde_json::json!(hex_bytes(b"pub struct Widget;"))
    );
}

fn assert_mcp_go_round_trip(input: &mut ChildStdin, output: &mut BufReader<ChildStdout>) {
    let search = mcp_call_search(input, output, 6, "Launch");
    let search = &search["result"]["structuredContent"];
    assert_eq!(search["schema_version"], serde_json::json!(3));
    assert_eq!(search["query_profile"], serde_json::json!(3));
    assert_eq!(search["matches_returned"], serde_json::json!(1));
    let candidate = &search["matches"][0];
    assert_eq!(candidate["language"], serde_json::json!("go"));
    let selector = serde_json::json!({
        "snapshot_sha256": search["snapshot_sha256"],
        "generation": search["generation"],
        "path": candidate["path"],
        "content_sha256": candidate["content_sha256"],
        "artifact_sha256": candidate["artifact_sha256"],
        "fact_ordinal": candidate["fact_ordinal"]
    });
    let symbol = mcp_call_symbol(input, output, 7, &selector);
    let symbol = &symbol["result"]["structuredContent"];
    assert_eq!(symbol["symbol"]["language"], serde_json::json!("go"));
    assert_eq!(symbol["symbol"]["name"], serde_json::json!("Launch"));
    assert_eq!(
        symbol["symbol"]["declaration_hex"],
        serde_json::json!(hex_bytes(b"func (Gadget) Launch() {}"))
    );
}

fn assert_mcp_supported_language_round_trip(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    search_request_id: usize,
    symbol_request_id: usize,
    query: &str,
    language: &str,
    declaration: &[u8],
) {
    let search = mcp_call_search(input, output, search_request_id, query);
    let search = &search["result"]["structuredContent"];
    assert_eq!(search["schema_version"], serde_json::json!(3));
    assert_eq!(search["query_profile"], serde_json::json!(3));
    assert_eq!(search["matches_returned"], serde_json::json!(1));
    let candidate = &search["matches"][0];
    assert_eq!(candidate["language"], serde_json::json!(language));
    let selector = serde_json::json!({
        "snapshot_sha256": search["snapshot_sha256"],
        "generation": search["generation"],
        "path": candidate["path"],
        "content_sha256": candidate["content_sha256"],
        "artifact_sha256": candidate["artifact_sha256"],
        "fact_ordinal": candidate["fact_ordinal"]
    });
    let symbol = mcp_call_symbol(input, output, symbol_request_id, &selector);
    let symbol = &symbol["result"]["structuredContent"];
    assert_eq!(symbol["symbol"]["language"], serde_json::json!(language));
    assert_eq!(symbol["symbol"]["name"], serde_json::json!(query));
    assert_eq!(
        symbol["symbol"]["declaration_hex"],
        serde_json::json!(hex_bytes(declaration))
    );
}

fn mcp_call_symbol(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    request_id: usize,
    exact_arguments: &serde_json::Value,
) -> serde_json::Value {
    mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/call",
            "params": {"name": "symbol_get", "arguments": exact_arguments}
        }),
    )
}

fn assert_mcp_stale(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    exact_arguments: &serde_json::Value,
) {
    let stale = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {"name": "symbol_get", "arguments": exact_arguments}
        }),
    );
    assert_eq!(stale["result"]["isError"], serde_json::json!(true));
    assert_eq!(
        stale["result"]["content"][0]["text"],
        serde_json::json!("symbol retrieval failed")
    );
}

fn stop_mcp(child: std::process::Child, input: ChildStdin, output: BufReader<ChildStdout>) {
    drop(input);
    drop(output);
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
