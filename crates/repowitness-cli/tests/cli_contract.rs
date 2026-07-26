//! Black-box regression coverage for the installed command contract.

use std::{
    ffi::OsStr,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{ChildStdin, ChildStdout, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

const REPOSITORY_ID: &str = concat!(
    "rwi1:h:",
    "B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2"
);
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repowitness-cli-contract-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture directory should be created");
        Self(path)
    }

    fn repository(&self) -> PathBuf {
        self.0.join("repository")
    }

    fn database(&self) -> PathBuf {
        self.0.join("index.sqlite3")
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn repowitness(arguments: &[&str]) -> Output {
    repowitness_os(arguments.iter().map(OsStr::new))
}

fn repowitness_os<'a>(arguments: impl IntoIterator<Item = &'a OsStr>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .args(arguments)
        .output()
        .expect("the RepoWitness binary must start")
}

fn fixture_repository(directory: &TempDirectory) -> PathBuf {
    let repository = directory.repository();
    fs::create_dir_all(repository.join("src")).expect("fixture source directory should be created");
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .arg(&repository)
        .status()
        .expect("Git should start");
    assert!(status.success());
    fs::write(
        repository.join("src/lib.rs"),
        "pub struct Widget;\nimpl Widget { pub fn run() {} }\n",
    )
    .expect("Rust fixture should be written");
    fs::write(repository.join("README.md"), "fixture\n")
        .expect("non-Rust fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "src/lib.rs", "README.md"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    repository
}

fn index(repository: &Path, database: &Path, identity: &str) -> Output {
    repowitness_os([
        OsStr::new("index"),
        OsStr::new("--repository-id"),
        OsStr::new(identity),
        OsStr::new("--database"),
        database.as_os_str(),
        repository.as_os_str(),
    ])
}

fn search(database: &Path, identity: &str, query: &str, limit: &str) -> Output {
    repowitness_os([
        OsStr::new("search"),
        OsStr::new("--repository-id"),
        OsStr::new(identity),
        OsStr::new("--database"),
        database.as_os_str(),
        OsStr::new("--query"),
        OsStr::new(query),
        OsStr::new("--limit"),
        OsStr::new(limit),
    ])
}

fn report_value<'a>(report: &'a str, key: &str) -> &'a str {
    let prefix = format!("{key}=");
    report
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .expect("report field must be present")
}

fn symbol_get_from_search(
    repository: &Path,
    database: &Path,
    identity: &str,
    search_report: &str,
) -> Output {
    repowitness_os([
        OsStr::new("symbol-get"),
        OsStr::new("--repository-id"),
        OsStr::new(identity),
        OsStr::new("--database"),
        database.as_os_str(),
        OsStr::new("--root"),
        repository.as_os_str(),
        OsStr::new("--snapshot"),
        OsStr::new(report_value(search_report, "snapshot_sha256")),
        OsStr::new("--generation"),
        OsStr::new(report_value(search_report, "generation")),
        OsStr::new("--path"),
        OsStr::new(report_value(search_report, "match_0_path")),
        OsStr::new("--content"),
        OsStr::new(report_value(search_report, "match_0_content_sha256")),
        OsStr::new("--artifact"),
        OsStr::new(report_value(search_report, "match_0_artifact_sha256")),
        OsStr::new("--fact"),
        OsStr::new(report_value(search_report, "match_0_fact_ordinal")),
    ])
}

fn assert_symbol_get_success(output: Output, expected_name: &str, expected_source: &str) {
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let report = String::from_utf8(output.stdout).expect("symbol report must be UTF-8");
    assert!(report.contains("status=ok\noperation=symbol-get\n"));
    assert!(report.contains("symbol_profile=1\n"));
    assert!(report.contains("resolution=confirmed\n"));
    assert!(report.contains("symbol_found=true\n"));
    assert!(report.contains("evidence_tier=syntax\n"));
    assert_eq!(report_value(&report, "name"), expected_name);
    assert_eq!(
        report_value(&report, "declaration_hex"),
        hex_bytes(expected_source.as_bytes())
    );
}

fn assert_stale_symbol_rejected(output: Output, repository: &Path, forbidden_digest: Option<&str>) {
    assert_eq!(output.status.code(), Some(70));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("diagnostic must be UTF-8");
    assert!(!diagnostic.contains(repository.to_string_lossy().as_ref()));
    if let Some(digest) = forbidden_digest {
        assert!(!diagnostic.contains(digest));
    }
}

fn modify_source_and_assert_stale_rejection(repository: &Path, database: &Path) {
    let searched = search(database, REPOSITORY_ID, "Widget", "1");
    assert!(searched.status.success());
    let searched = String::from_utf8(searched.stdout).expect("search report must be UTF-8");
    fs::write(
        repository.join("src/lib.rs"),
        "pub struct Widget;\nimpl Widget { pub fn run() {} }\npub fn changed() {}\n",
    )
    .expect("changed Rust fixture should be written");
    let output = symbol_get_from_search(repository, database, REPOSITORY_ID, &searched);
    assert_stale_symbol_rejected(
        output,
        repository,
        Some(report_value(&searched, "match_0_content_sha256")),
    );
}

fn assert_changed_symbol_contract(repository: &Path, database: &Path) {
    assert_changed_search_contract(database);
    let searched = search(database, REPOSITORY_ID, "changed", "1");
    assert!(searched.status.success());
    let searched = String::from_utf8(searched.stdout).expect("search report must be UTF-8");
    assert_symbol_get_success(
        symbol_get_from_search(repository, database, REPOSITORY_ID, &searched),
        "changed",
        "pub fn changed() {}",
    );
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(output, "{byte:02x}").expect("writing to a string cannot fail");
    }
    output
}

fn assert_widget_search_contract(database: &Path) -> String {
    let searched = search(database, REPOSITORY_ID, "Widget", "1");
    assert!(searched.status.success());
    assert!(searched.stderr.is_empty());
    let searched = String::from_utf8(searched.stdout).expect("search report must be UTF-8");
    assert!(searched.contains("status=ok\noperation=search\n"));
    assert!(searched.contains("query_profile=1\n"));
    assert!(searched.contains("generation=1\n"));
    assert!(searched.contains("resolution=confirmed\n"));
    assert!(searched.contains("matches_returned=1\n"));
    assert!(searched.contains("matches_total=2\n"));
    assert!(searched.contains("coverage_searched=1\n"));
    assert!(searched.contains("coverage_skipped=1\n"));
    assert!(searched.contains("coverage_truncated=1\n"));
    assert!(searched.contains("match_0_path=rwp1:h:7372632F6C69622E7273\n"));
    assert!(searched.contains("match_0_fact_ordinal=0\n"));
    assert!(searched.contains("match_0_evidence_tier=syntax\n"));
    assert!(searched.contains("match_0_content_sha256="));
    assert!(searched.contains("match_0_artifact_sha256="));
    assert!(searched.contains("match_0_producer_manifest_sha256="));
    assert!(!searched.contains(REPOSITORY_ID));
    assert!(!searched.contains(database.to_string_lossy().as_ref()));
    searched
}

fn assert_absent_search_contract(database: &Path) {
    let absent = search(database, REPOSITORY_ID, "definitely_absent_symbol", "20");
    assert!(absent.status.success());
    assert!(absent.stderr.is_empty());
    let absent = String::from_utf8(absent.stdout).expect("search report must be UTF-8");
    assert!(absent.contains("resolution=unresolved\n"));
    assert!(absent.contains("matches_returned=0\nmatches_total=0\n"));
    assert!(absent.contains("coverage_unresolved=1\n"));
    assert!(!absent.contains("definitely_absent_symbol"));
}

fn assert_changed_search_contract(database: &Path) {
    let changed = search(database, REPOSITORY_ID, "changed", "20");
    assert!(changed.status.success());
    assert!(changed.stderr.is_empty());
    let changed = String::from_utf8(changed.stdout).expect("search report must be UTF-8");
    assert!(changed.contains("generation=3\n"));
    assert!(changed.contains("matches_returned=1\nmatches_total=1\n"));
    assert!(changed.contains("match_0_name=changed\n"));
}

#[test]
fn help_and_version_write_to_stdout_and_succeed() {
    let help = repowitness(&["--help"]);
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    let help = String::from_utf8(help.stdout).expect("help must be UTF-8");
    assert!(help.contains("index          Build"));
    assert!(help.contains("--repository-id"));

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
    assert!(first.contains("repository_paths=2\n"));
    assert!(first.contains("indexed_rust_files=1\n"));
    assert_index_work_counts(&first, 0, 1);
    assert!(first.contains("skipped_non_rust_paths=1\n"));
    assert!(first.contains("symbol_facts=2\n"));
    assert!(!first.contains(REPOSITORY_ID));
    assert!(!first.contains(repository.to_string_lossy().as_ref()));
    assert!(!first.contains(database.to_string_lossy().as_ref()));
    assert!(database.is_file());

    let first_search = assert_widget_search_contract(&database);
    assert_symbol_get_success(
        symbol_get_from_search(&repository, &database, REPOSITORY_ID, &first_search),
        "Widget",
        "pub struct Widget;",
    );
    assert_absent_search_contract(&database);

    let second = index(&repository, &database, REPOSITORY_ID);
    assert!(second.status.success());
    assert!(second.stderr.is_empty());
    let second = String::from_utf8(second.stdout).expect("index report must be UTF-8");
    assert!(second.contains("generation=2\n"));
    assert!(second.contains("symbol_facts=2\n"));
    assert_index_work_counts(&second, 1, 0);

    let stale_generation =
        symbol_get_from_search(&repository, &database, REPOSITORY_ID, &first_search);
    assert_stale_symbol_rejected(stale_generation, &repository, None);

    modify_source_and_assert_stale_rejection(&repository, &database);

    let changed = index(&repository, &database, REPOSITORY_ID);
    assert!(changed.status.success());
    assert!(changed.stderr.is_empty());
    let changed = String::from_utf8(changed.stdout).expect("index report must be UTF-8");
    assert!(changed.contains("generation=3\n"));
    assert!(changed.contains("symbol_facts=3\n"));
    assert_index_work_counts(&changed, 0, 1);

    assert_changed_symbol_contract(&repository, &database);
}

fn assert_index_work_counts(report: &str, reused: u64, analyzed: u64) {
    assert!(report.contains(&format!("reused_rust_files={reused}\n")));
    assert!(report.contains(&format!("analyzed_rust_files={analyzed}\n")));
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
    assert_eq!(
        stderr,
        "error: indexing failed: repository identity is invalid\n"
    );
    assert!(!stderr.contains(private_identity));
    assert!(!stderr.contains("private-missing-repository"));
    assert!(!database.exists());

    let missing_repository = index(&private_repository, &database, REPOSITORY_ID);
    assert_eq!(missing_repository.status.code(), Some(70));
    assert!(missing_repository.stdout.is_empty());
    let stderr = String::from_utf8(missing_repository.stderr).expect("diagnostic must be UTF-8");
    assert_eq!(
        stderr,
        "error: indexing failed: local Rust index preparation failed\n"
    );
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
    assert_eq!(
        stderr,
        concat!(
            "error: indexing failed: ",
            "local index database must be outside the repository worktree\n"
        )
    );
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
fn mcp_stdio_indexes_searches_retrieves_and_rejects_a_stale_selector() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
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

    assert!(
        index(&repository, &database, REPOSITORY_ID)
            .status
            .success()
    );
    assert_mcp_stale(&mut input, &mut output, &exact_arguments);
    stop_mcp(child, input, output);
}

#[test]
#[ignore = "requires REPOWITNESS_REAL_REPOSITORY, Git, and a readable Rust worktree"]
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

    let exact_arguments = ["fn", "struct", "enum", "impl", "trait"]
        .into_iter()
        .enumerate()
        .find_map(|(ordinal, query)| {
            mcp_first_search_selector(&mut input, &mut output, 10 + ordinal, query)
        })
        .expect("real Rust repository must expose at least one indexed symbol");
    let symbol = mcp_call_symbol(&mut input, &mut output, 20, &exact_arguments);
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
        ["code_search", "symbol_get"]
    );
}

fn mcp_search_selector(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
) -> serde_json::Value {
    let search = mcp_call_search(input, output, 3, "struct Widget");
    let search = &search["result"]["structuredContent"];
    assert_eq!(search["schema_version"], serde_json::json!(1));
    assert_eq!(search["generation"], serde_json::json!(1));
    assert_eq!(search["resolution"], serde_json::json!("confirmed"));
    assert_eq!(search["matches_returned"], serde_json::json!(1));
    let candidate = &search["matches"][0];
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
    assert_eq!(
        symbol["symbol"]["declaration_hex"],
        serde_json::json!(hex_bytes(b"pub struct Widget;"))
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
