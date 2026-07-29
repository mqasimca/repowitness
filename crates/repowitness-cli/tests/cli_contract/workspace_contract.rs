use std::ffi::OsString;

const CONNECTED_WORKSPACE_ID: &str = concat!(
    "cwi1:h:",
    "1111111111111111111111111111111111111111111111111111111111111111"
);
const SOURCE_SLOT_ID: &str = concat!(
    "ssi1:h:",
    "2222222222222222222222222222222222222222222222222222222222222222"
);
const MULTI_SOURCE_WORKSPACE_ID: &str = concat!(
    "cwi1:h:",
    "3333333333333333333333333333333333333333333333333333333333333333"
);
const MULTI_SOURCE_SLOT_ONE: &str = concat!(
    "ssi1:h:",
    "4444444444444444444444444444444444444444444444444444444444444444"
);
const MULTI_SOURCE_SLOT_TWO: &str = concat!(
    "ssi1:h:",
    "5555555555555555555555555555555555555555555555555555555555555555"
);
const MULTI_SOURCE_UNKNOWN_SLOT: &str = concat!(
    "ssi1:h:",
    "6666666666666666666666666666666666666666666666666666666666666666"
);
const MULTI_SOURCE_REPOSITORY_TWO: &str = concat!(
    "rwi1:h:",
    "C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3"
);

struct WorkspaceGraphFixture {
    repository: PathBuf,
    database: PathBuf,
}

struct MultiSourceWorkspaceGraphFixture {
    first_repository: PathBuf,
    second_repository: PathBuf,
    database: PathBuf,
}

#[test]
fn workspace_index_rejects_invalid_forms_without_state_or_input_disclosure() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let manifest = directory.0.join("private-workspace.toml");
    let private_option_value = "private-workspace-option";
    let attempts = [
        vec![OsString::from("workspace"), OsString::from("index")],
        vec![
            OsString::from("workspace"),
            OsString::from("index"),
            OsString::from("--manifest"),
            manifest.clone().into_os_string(),
        ],
        vec![
            OsString::from("workspace"),
            OsString::from("index"),
            OsString::from("--database"),
            database.clone().into_os_string(),
        ],
        vec![
            OsString::from("workspace"),
            OsString::from("index"),
            OsString::from("--private-option"),
            OsString::from(private_option_value),
            OsString::from("--manifest"),
            manifest.clone().into_os_string(),
            OsString::from("--database"),
            database.clone().into_os_string(),
        ],
    ];

    for arguments in attempts {
        let output = repowitness_os(arguments.iter().map(OsString::as_os_str));
        assert_eq!(output.status.code(), Some(64));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).expect("diagnostic must be UTF-8");
        assert!(stderr.starts_with("error:"));
        for sensitive in [
            manifest.to_string_lossy().as_ref(),
            database.to_string_lossy().as_ref(),
            private_option_value,
        ] {
            assert!(!stderr.contains(sensitive));
        }
        assert!(!database.exists());
    }
}

#[test]
fn workspace_index_admits_one_explicit_relative_source_without_leaking_inputs() {
    let directory = TempDirectory::new();
    let fixture = index_workspace_fixture(&directory);
    assert_workspace_graph_cli(&fixture);
    assert_workspace_graph_mcp(&fixture);
}

#[test]
fn workspace_graph_reads_are_source_slot_isolated_and_pinned_end_to_end() {
    let directory = TempDirectory::new();
    let fixture = index_multi_source_workspace_fixture(&directory);

    let first = assert_multi_source_graph_cli(
        &fixture,
        MULTI_SOURCE_SLOT_ONE,
        "alpha_source_slot_only",
        "bravo_source_slot_only",
    );
    let second = assert_multi_source_graph_cli(
        &fixture,
        MULTI_SOURCE_SLOT_TWO,
        "bravo_source_slot_only",
        "alpha_source_slot_only",
    );
    assert_eq!(first["context"]["workspace_view"], second["context"]["workspace_view"]);
    assert_ne!(
        first["context"]["graph_generation"],
        second["context"]["graph_generation"]
    );
    assert_unknown_multi_source_slot_is_redacted(&fixture);
    assert_multi_source_graph_mcp(
        &fixture,
        MULTI_SOURCE_SLOT_ONE,
        "alpha_source_slot_only",
        200,
    );
    assert_multi_source_graph_mcp(
        &fixture,
        MULTI_SOURCE_SLOT_TWO,
        "bravo_source_slot_only",
        300,
    );
}

fn index_multi_source_workspace_fixture(directory: &TempDirectory) -> MultiSourceWorkspaceGraphFixture {
    let first_repository = committed_workspace_repository(
        directory,
        "first-source",
        "pub fn alpha_source_slot_only() {}\n",
    );
    let second_repository = committed_workspace_repository(
        directory,
        "second-source",
        "pub fn bravo_source_slot_only() {}\n",
    );
    let database = directory.database();
    let manifest = directory.0.join("multi-source-workspace.toml");
    fs::write(
        &manifest,
        format!(
            "schema_version = 1\nconnected_workspace_id = \"{MULTI_SOURCE_WORKSPACE_ID}\"\n\n\
             [[source]]\nsource_slot_id = \"{MULTI_SOURCE_SLOT_ONE}\"\n\
             repository_identity = \"{REPOSITORY_ID}\"\nworktree_root = \"first-source\"\n\n\
             [source.selector]\nkind = \"worktree-head\"\n\n[source.scope]\nkind = \"whole-repository\"\n\n\
             [[source]]\nsource_slot_id = \"{MULTI_SOURCE_SLOT_TWO}\"\n\
             repository_identity = \"{MULTI_SOURCE_REPOSITORY_TWO}\"\n\
             worktree_root = \"second-source\"\n\n[source.selector]\nkind = \"worktree-head\"\n\n\
             [source.scope]\nkind = \"whole-repository\"\n"
        ),
    )
    .expect("multi-source manifest should be written");
    let output = repowitness_os([
        OsStr::new("workspace"),
        OsStr::new("index"),
        OsStr::new("--manifest"),
        manifest.as_os_str(),
        OsStr::new("--database"),
        database.as_os_str(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let receipt = String::from_utf8(output.stdout).expect("workspace receipt must be UTF-8");
    assert!(receipt.contains("source_slots=2\n"));
    assert!(receipt.contains("outcome=published\n"));
    for sensitive in [
        first_repository.to_string_lossy().as_ref(),
        second_repository.to_string_lossy().as_ref(),
        database.to_string_lossy().as_ref(),
        MULTI_SOURCE_WORKSPACE_ID,
        MULTI_SOURCE_SLOT_ONE,
        MULTI_SOURCE_SLOT_TWO,
        REPOSITORY_ID,
        MULTI_SOURCE_REPOSITORY_TWO,
    ] {
        assert!(!receipt.contains(sensitive));
    }
    MultiSourceWorkspaceGraphFixture {
        first_repository,
        second_repository,
        database,
    }
}

fn committed_workspace_repository(directory: &TempDirectory, name: &str, source: &str) -> PathBuf {
    let repository = directory.0.join(name);
    fs::create_dir_all(repository.join("src")).expect("workspace source directory should be created");
    let initialized = Command::new("git")
        .args(["init", "--quiet"])
        .arg(&repository)
        .status()
        .expect("Git should initialize a workspace source");
    assert!(initialized.success());
    fs::write(repository.join("src/lib.rs"), source).expect("workspace Rust source should be written");
    let committed = Command::new("git")
        .current_dir(&repository)
        .args([
            "-c",
            "user.name=RepoWitness Test",
            "-c",
            "user.email=repowitness-test@example.invalid",
            "add",
            "--",
            "src/lib.rs",
        ])
        .status()
        .expect("Git should stage a workspace source");
    assert!(committed.success());
    let committed = Command::new("git")
        .current_dir(&repository)
        .args([
            "-c",
            "user.name=RepoWitness Test",
            "-c",
            "user.email=repowitness-test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "workspace source",
        ])
        .status()
        .expect("Git should commit a workspace source");
    assert!(committed.success());
    repository
}

fn assert_multi_source_graph_cli(
    fixture: &MultiSourceWorkspaceGraphFixture,
    source_slot: &str,
    expected_name: &str,
    absent_name: &str,
) -> serde_json::Value {
    let status = workspace_graph_status(&fixture.database, source_slot);
    let context = &status["context"];
    assert_eq!(context["connected_workspace"], MULTI_SOURCE_WORKSPACE_ID);
    assert!(context["workspace_view"].as_i64().is_some_and(|value| value > 0));
    assert!(context["graph_generation"].as_i64().is_some_and(|value| value > 0));
    let exact = workspace_graph_search(&fixture.database, source_slot, expected_name, Some(context));
    assert_eq!(exact["context"], *context);
    assert_slot_search(&exact, source_slot, expected_name, 1);
    let absent = workspace_graph_search(&fixture.database, source_slot, absent_name, None);
    assert_slot_search(&absent, source_slot, absent_name, 0);
    status
}

fn workspace_graph_status(database: &Path, source_slot: &str) -> serde_json::Value {
    let output = repowitness_os([
        OsStr::new("graph"),
        OsStr::new("status"),
        OsStr::new("--connected-workspace-id"),
        OsStr::new(MULTI_SOURCE_WORKSPACE_ID),
        OsStr::new("--source-slot-id"),
        OsStr::new(source_slot),
        OsStr::new("--database"),
        database.as_os_str(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).expect("graph status must be JSON")
}

fn workspace_graph_search(
    database: &Path,
    source_slot: &str,
    query: &str,
    exact_context: Option<&serde_json::Value>,
) -> serde_json::Value {
    let mut arguments = vec![
        OsString::from("graph"),
        OsString::from("search"),
        OsString::from("--connected-workspace-id"),
        OsString::from(MULTI_SOURCE_WORKSPACE_ID),
        OsString::from("--source-slot-id"),
        OsString::from(source_slot),
        OsString::from("--database"),
        database.as_os_str().to_os_string(),
        OsString::from("--query"),
        OsString::from(query),
        OsString::from("--max-results"),
        OsString::from("5"),
    ];
    if let Some(context) = exact_context {
        arguments.extend([
            OsString::from("--workspace-view"),
            OsString::from(context["workspace_view"].as_i64().expect("positive workspace view").to_string()),
            OsString::from("--graph-generation"),
            OsString::from(context["graph_generation"].as_i64().expect("positive graph generation").to_string()),
        ]);
    }
    let output = repowitness_os(arguments.iter().map(OsString::as_os_str));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).expect("graph search must be JSON")
}

fn assert_slot_search(
    result: &serde_json::Value,
    source_slot: &str,
    expected_name: &str,
    expected_matches: u64,
) {
    assert_eq!(result["matches_total"], serde_json::json!(expected_matches));
    assert_eq!(result["matches_returned"], serde_json::json!(expected_matches));
    let definitions = result["definitions"].as_array().expect("definitions array");
    assert_eq!(definitions.len(), usize::try_from(expected_matches).expect("small fixture"));
    for definition in definitions {
        assert_eq!(definition["source_slot"], source_slot);
        assert_eq!(definition["name"], expected_name);
    }
}

fn assert_unknown_multi_source_slot_is_redacted(fixture: &MultiSourceWorkspaceGraphFixture) {
    let output = repowitness_os([
        OsStr::new("graph"),
        OsStr::new("status"),
        OsStr::new("--connected-workspace-id"),
        OsStr::new(MULTI_SOURCE_WORKSPACE_ID),
        OsStr::new("--source-slot-id"),
        OsStr::new(MULTI_SOURCE_UNKNOWN_SLOT),
        OsStr::new("--database"),
        fixture.database.as_os_str(),
    ]);
    assert_eq!(output.status.code(), Some(70));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("diagnostic must be UTF-8");
    for sensitive in [
        fixture.first_repository.to_string_lossy().as_ref(),
        fixture.second_repository.to_string_lossy().as_ref(),
        fixture.database.to_string_lossy().as_ref(),
        MULTI_SOURCE_WORKSPACE_ID,
        MULTI_SOURCE_UNKNOWN_SLOT,
    ] {
        assert!(!diagnostic.contains(sensitive));
    }
}

fn assert_multi_source_graph_mcp(
    fixture: &MultiSourceWorkspaceGraphFixture,
    source_slot: &str,
    expected_name: &str,
    request_id: usize,
) {
    let (child, mut input, mut output) = start_mcp_with_graph_workspace(
        &fixture.first_repository,
        &fixture.database,
        MULTI_SOURCE_WORKSPACE_ID,
        source_slot,
    );
    initialize_mcp(&mut input, &mut output);
    let result = mcp_call_graph(
        &mut input,
        &mut output,
        request_id,
        "graph_search",
        serde_json::json!({"query": expected_name, "max_results": 5}),
    );
    let result = &result["result"]["structuredContent"];
    assert_eq!(result["context"]["connected_workspace"], MULTI_SOURCE_WORKSPACE_ID);
    assert_slot_search(result, source_slot, expected_name, 1);
    stop_mcp(child, input, output);
}

fn index_workspace_fixture(directory: &TempDirectory) -> WorkspaceGraphFixture {
    let repository = fixture_repository(directory);
    let status = Command::new("git")
        .current_dir(&repository)
        .args([
            "-c",
            "user.name=RepoWitness Test",
            "-c",
            "user.email=repowitness-test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "workspace fixture",
        ])
        .status()
        .expect("Git should commit the workspace fixture");
    assert!(status.success());
    let database = directory.database();
    let manifest = directory.0.join("workspace.toml");
    let user_configuration = directory.0.join("user.toml");
    let workspace_configuration = directory.0.join("workspace-config.toml");
    let repository_configuration = directory.0.join("repository.toml");
    for configuration in [
        &user_configuration,
        &workspace_configuration,
        &repository_configuration,
    ] {
        fs::write(configuration, "schema_version = 1\n")
            .expect("workspace configuration fixture");
    }
    fs::write(
        &manifest,
        format!(
            "schema_version = 1\nconnected_workspace_id = \"{CONNECTED_WORKSPACE_ID}\"\n\n[[source]]\nsource_slot_id = \"{SOURCE_SLOT_ID}\"\nrepository_identity = \"{REPOSITORY_ID}\"\nworktree_root = \"repository\"\n\n[source.selector]\nkind = \"worktree-head\"\n\n[source.scope]\nkind = \"whole-repository\"\n"
        ),
    )
    .expect("workspace manifest fixture");

    let output = repowitness_os([
        OsStr::new("workspace"),
        OsStr::new("index"),
        OsStr::new("--manifest"),
        manifest.as_os_str(),
        OsStr::new("--database"),
        database.as_os_str(),
        OsStr::new("--user-config"),
        user_configuration.as_os_str(),
        OsStr::new("--workspace-config"),
        workspace_configuration.as_os_str(),
        OsStr::new("--repository-config"),
        repository_configuration.as_os_str(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report = String::from_utf8(output.stdout).expect("workspace receipt");
    assert!(report.contains("operation=workspace_index\n"));
    assert!(report.contains("source_slots=1\n"));
    assert!(report.contains("outcome=published\n"));
    for sensitive in [
        manifest.to_string_lossy().as_ref(),
        repository.to_string_lossy().as_ref(),
        database.to_string_lossy().as_ref(),
        user_configuration.to_string_lossy().as_ref(),
        workspace_configuration.to_string_lossy().as_ref(),
        repository_configuration.to_string_lossy().as_ref(),
        REPOSITORY_ID,
        CONNECTED_WORKSPACE_ID,
        SOURCE_SLOT_ID,
    ] {
        assert!(!report.contains(sensitive));
    }

    WorkspaceGraphFixture {
        repository,
        database,
    }
}

fn assert_workspace_graph_cli(fixture: &WorkspaceGraphFixture) {
    let graph = repowitness_os([
        OsStr::new("graph"),
        OsStr::new("status"),
        OsStr::new("--connected-workspace-id"),
        OsStr::new(CONNECTED_WORKSPACE_ID),
        OsStr::new("--source-slot-id"),
        OsStr::new(SOURCE_SLOT_ID),
        OsStr::new("--database"),
        fixture.database.as_os_str(),
    ]);
    assert!(
        graph.status.success(),
        "{}",
        String::from_utf8_lossy(&graph.stderr)
    );
    assert!(graph.stderr.is_empty());
    let graph: serde_json::Value =
        serde_json::from_slice(&graph.stdout).expect("graph status JSON");
    assert_eq!(
        graph["context"]["connected_workspace"],
        serde_json::json!(CONNECTED_WORKSPACE_ID)
    );
    let workspace_view = graph["context"]["workspace_view"]
        .as_i64()
        .expect("positive workspace view");
    let graph_generation = graph["context"]["graph_generation"]
        .as_i64()
        .expect("positive graph generation");
    assert!(workspace_view > 0);
    assert!(graph_generation > 0);

    let exact_graph = repowitness_os([
        OsStr::new("graph"),
        OsStr::new("status"),
        OsStr::new("--connected-workspace-id"),
        OsStr::new(CONNECTED_WORKSPACE_ID),
        OsStr::new("--source-slot-id"),
        OsStr::new(SOURCE_SLOT_ID),
        OsStr::new("--database"),
        fixture.database.as_os_str(),
        OsStr::new("--workspace-view"),
        OsStr::new(&workspace_view.to_string()),
        OsStr::new("--graph-generation"),
        OsStr::new(&graph_generation.to_string()),
    ]);
    assert!(
        exact_graph.status.success(),
        "{}",
        String::from_utf8_lossy(&exact_graph.stderr)
    );
    assert!(exact_graph.stderr.is_empty());
}

fn assert_workspace_graph_mcp(fixture: &WorkspaceGraphFixture) {
    let (child, mut input, mut output) =
        start_mcp_with_graph_workspace(
            &fixture.repository,
            &fixture.database,
            CONNECTED_WORKSPACE_ID,
            SOURCE_SLOT_ID,
        );
    initialize_mcp(&mut input, &mut output);
    let mcp_graph = mcp_call_graph(
        &mut input,
        &mut output,
        100,
        "graph_status",
        serde_json::json!({}),
    );
    let mcp_graph = &mcp_graph["result"]["structuredContent"];
    assert_eq!(mcp_graph["schema_version"], serde_json::json!(1));
    assert_eq!(
        mcp_graph["context"]["connected_workspace"],
        serde_json::json!(CONNECTED_WORKSPACE_ID)
    );
    assert!(
        mcp_graph["context"]["workspace_view"]
            .as_i64()
            .is_some_and(|view| view > 0)
    );
    assert!(
        mcp_graph["context"]["graph_generation"]
            .as_i64()
            .is_some_and(|generation| generation > 0)
    );
    stop_mcp(child, input, output);
}
