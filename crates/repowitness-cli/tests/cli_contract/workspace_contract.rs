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
    assert_workspace_phase2_context_mcp(&fixture);
}

#[test]
fn phase2_context_build_pins_single_repository_scope_and_labels_evidence() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let indexed = index(&repository, &database, REPOSITORY_ID);
    assert!(
        indexed.status.success(),
        "{}",
        String::from_utf8_lossy(&indexed.stderr)
    );

    let output = repowitness_os([
        OsStr::new("phase2-context-build"),
        OsStr::new("--repository-id"),
        OsStr::new(REPOSITORY_ID),
        OsStr::new("--database"),
        database.as_os_str(),
        OsStr::new("--root"),
        repository.as_os_str(),
        OsStr::new("--intent"),
        OsStr::new("Widget"),
        OsStr::new("--budget"),
        OsStr::new("4096"),
        OsStr::new("--limit"),
        OsStr::new("7"),
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report = String::from_utf8(output.stdout).expect("Phase 2 context report");
    assert_eq!(report_value(&report, "operation"), "phase2-context-build");
    assert_eq!(
        report_value(&report, "profile_id"),
        "phase2-evidence-balanced-v1"
    );
    assert_eq!(report_value(&report, "profile_version"), "1");
    assert!(report_value(&report, "workspace_view")
        .parse::<i64>()
        .is_ok_and(|view| view > 0));
    assert!(report_value(&report, "source_epoch")
        .parse::<i64>()
        .is_ok_and(|epoch| epoch > 0));
    assert!(report.contains("context_item_0_tier=syntax\n"));
    assert!(report.contains("context_item_0_kind=syntax\n"));
    assert!(report.contains("context_item_0_provider_0_tier=syntax\n"));
    assert_eq!(report_value(&report, "provider_coverage"), "6");
    assert!(report.contains("provider_coverage_0_tier=precise_overlay\n"));
    assert!(report.contains("provider_coverage_0_availability=unavailable\n"));
    assert!(report.contains("provider_coverage_1_tier=syntax\n"));
    assert!(report.contains("provider_coverage_1_availability=available\n"));
    for sensitive in [
        repository.to_string_lossy().as_ref(),
        database.to_string_lossy().as_ref(),
        REPOSITORY_ID,
    ] {
        assert!(!report.contains(sensitive));
    }
}

#[test]
fn phase2_context_build_accepts_one_explicit_connected_source_slot() {
    let directory = TempDirectory::new();
    let fixture = index_workspace_fixture(&directory);
    let output = repowitness_os([
        OsStr::new("phase2-context-build"),
        OsStr::new("--repository-id"),
        OsStr::new(REPOSITORY_ID),
        OsStr::new("--database"),
        fixture.database.as_os_str(),
        OsStr::new("--root"),
        fixture.repository.as_os_str(),
        OsStr::new("--intent"),
        OsStr::new("Widget"),
        OsStr::new("--connected-workspace-id"),
        OsStr::new(CONNECTED_WORKSPACE_ID),
        OsStr::new("--source-slot-id"),
        OsStr::new(SOURCE_SLOT_ID),
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = String::from_utf8(output.stdout).expect("Phase 2 context report");
    assert_eq!(report_value(&report, "operation"), "phase2-context-build");
    assert!(report.contains("context_item_0_kind=syntax\n"));
    for sensitive in [
        fixture.repository.to_string_lossy().as_ref(),
        fixture.database.to_string_lossy().as_ref(),
        REPOSITORY_ID,
        CONNECTED_WORKSPACE_ID,
        SOURCE_SLOT_ID,
    ] {
        assert!(!report.contains(sensitive));
    }
}

#[test]
fn scip_import_admits_one_contained_file_and_publishes_an_exact_active_overlay() {
    let directory = TempDirectory::new();
    let fixture = index_workspace_fixture(&directory);
    let scip_file = directory.0.join("producer.scip");
    fs::write(&scip_file, valid_scip_index()).expect("SCIP fixture should be written");

    let imported = repowitness_os([
        OsStr::new("scip-import"),
        OsStr::new("--database"),
        fixture.database.as_os_str(),
        OsStr::new("--root"),
        fixture.repository.as_os_str(),
        OsStr::new("--scip-file"),
        scip_file.as_os_str(),
        OsStr::new("--connected-workspace-id"),
        OsStr::new(CONNECTED_WORKSPACE_ID),
        OsStr::new("--source-slot-id"),
        OsStr::new(SOURCE_SLOT_ID),
    ]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(imported.stderr.is_empty());
    let receipt: serde_json::Value =
        serde_json::from_slice(&imported.stdout).expect("SCIP import receipt JSON");
    assert_eq!(
        receipt["connected_workspace"],
        serde_json::json!(CONNECTED_WORKSPACE_ID)
    );
    assert_eq!(receipt["source_slot"], serde_json::json!(SOURCE_SLOT_ID));
    assert!(receipt["workspace_view"].as_i64().is_some_and(|view| view > 0));
    assert_eq!(receipt["documents"], serde_json::json!(1));
    assert_eq!(receipt["occurrences"], serde_json::json!(1));
    assert_eq!(receipt["relationships"], serde_json::json!(1));

    let evidence = repowitness_os([
        OsStr::new("scip-evidence"),
        OsStr::new("--connected-workspace-id"),
        OsStr::new(CONNECTED_WORKSPACE_ID),
        OsStr::new("--source-slot-id"),
        OsStr::new(SOURCE_SLOT_ID),
        OsStr::new("--database"),
        fixture.database.as_os_str(),
        OsStr::new("--symbol"),
        OsStr::new("scip-rust pkg 0/Widget#"),
    ]);
    assert!(
        evidence.status.success(),
        "{}",
        String::from_utf8_lossy(&evidence.stderr)
    );
    let evidence: serde_json::Value =
        serde_json::from_slice(&evidence.stdout).expect("SCIP evidence JSON");
    assert_eq!(evidence["resolution"], serde_json::json!("found"));
    assert_eq!(evidence["overlay"]["documents"], serde_json::json!(1));
    assert_eq!(evidence["occurrences"].as_array().map(Vec::len), Some(1));
    assert_eq!(evidence["relationships"].as_array().map(Vec::len), Some(1));

    let context = repowitness_os([
        OsStr::new("phase2-context-build"),
        OsStr::new("--repository-id"),
        OsStr::new(REPOSITORY_ID),
        OsStr::new("--database"),
        fixture.database.as_os_str(),
        OsStr::new("--root"),
        fixture.repository.as_os_str(),
        OsStr::new("--intent"),
        OsStr::new("Widget"),
        OsStr::new("--connected-workspace-id"),
        OsStr::new(CONNECTED_WORKSPACE_ID),
        OsStr::new("--source-slot-id"),
        OsStr::new(SOURCE_SLOT_ID),
    ]);
    assert!(
        context.status.success(),
        "{}",
        String::from_utf8_lossy(&context.stderr)
    );
    let context = String::from_utf8(context.stdout).expect("Phase 2 context report");
    assert!(context.contains("context_item_0_tier=precise_overlay\n"));
    assert!(context.contains("context_item_0_kind=precise_overlay\n"));
    assert!(context.contains("context_item_0_relationship_count=1\n"));
    assert!(context.contains("provider_coverage_1_tier=syntax\n"));
    assert!(context.contains("provider_coverage_1_availability=available\n"));
    assert_workspace_phase2_context_mcp_with_scip(&fixture);
}

#[cfg(unix)]
#[test]
fn scip_rust_import_produces_then_imports_an_exact_active_overlay() {
    let directory = TempDirectory::new();
    let fixture = index_workspace_fixture(&directory);
    let producer = directory.0.join("rust-analyzer");
    write_synthetic_rust_analyzer(&producer, &valid_scip_index());

    let imported = repowitness_os([
        OsStr::new("scip-rust-import"),
        OsStr::new("--database"),
        fixture.database.as_os_str(),
        OsStr::new("--root"),
        fixture.repository.as_os_str(),
        OsStr::new("--connected-workspace-id"),
        OsStr::new(CONNECTED_WORKSPACE_ID),
        OsStr::new("--source-slot-id"),
        OsStr::new(SOURCE_SLOT_ID),
        OsStr::new("--rust-analyzer"),
        producer.as_os_str(),
        OsStr::new("--producer-timeout-ms"),
        OsStr::new("1000"),
        OsStr::new("--import-timeout-ms"),
        OsStr::new("1000"),
    ]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(imported.stderr.is_empty());
    let receipt: serde_json::Value =
        serde_json::from_slice(&imported.stdout).expect("SCIP import receipt JSON");
    assert_eq!(receipt["connected_workspace"], CONNECTED_WORKSPACE_ID);
    assert_eq!(receipt["source_slot"], SOURCE_SLOT_ID);
    assert_eq!(receipt["documents"], 1);
    assert_eq!(receipt["occurrences"], 1);
    assert_eq!(receipt["relationships"], 1);

    let evidence = repowitness_os([
        OsStr::new("scip-evidence"),
        OsStr::new("--connected-workspace-id"),
        OsStr::new(CONNECTED_WORKSPACE_ID),
        OsStr::new("--source-slot-id"),
        OsStr::new(SOURCE_SLOT_ID),
        OsStr::new("--database"),
        fixture.database.as_os_str(),
        OsStr::new("--symbol"),
        OsStr::new("scip-rust pkg 0/Widget#"),
    ]);
    assert!(
        evidence.status.success(),
        "{}",
        String::from_utf8_lossy(&evidence.stderr)
    );
    let evidence: serde_json::Value =
        serde_json::from_slice(&evidence.stdout).expect("SCIP evidence JSON");
    assert_eq!(evidence["resolution"], "found");
    assert_eq!(evidence["relationships"].as_array().map(Vec::len), Some(1));
}

#[cfg(unix)]
#[test]
fn scip_rust_import_derives_the_single_repository_source_slot() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    assert!(index(&repository, &database, REPOSITORY_ID).status.success());
    let producer = directory.0.join("rust-analyzer");
    write_synthetic_rust_analyzer(&producer, &valid_scip_index());

    let imported = repowitness_os([
        OsStr::new("scip-rust-import"),
        OsStr::new("--database"),
        database.as_os_str(),
        OsStr::new("--root"),
        repository.as_os_str(),
        OsStr::new("--repository-id"),
        OsStr::new(REPOSITORY_ID),
        OsStr::new("--rust-analyzer"),
        producer.as_os_str(),
        OsStr::new("--producer-timeout-ms"),
        OsStr::new("1000"),
        OsStr::new("--import-timeout-ms"),
        OsStr::new("1000"),
    ]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );

    let evidence = repowitness_os([
        OsStr::new("scip-evidence"),
        OsStr::new("--repository-id"),
        OsStr::new(REPOSITORY_ID),
        OsStr::new("--database"),
        database.as_os_str(),
        OsStr::new("--symbol"),
        OsStr::new("scip-rust pkg 0/Widget#"),
    ]);
    assert!(
        evidence.status.success(),
        "{}",
        String::from_utf8_lossy(&evidence.stderr)
    );
    let evidence: serde_json::Value =
        serde_json::from_slice(&evidence.stdout).expect("SCIP evidence JSON");
    assert_eq!(evidence["resolution"], "found");
}

#[cfg(unix)]
fn write_synthetic_rust_analyzer(path: &Path, index: &[u8]) {
    use std::os::unix::fs::PermissionsExt as _;

    fs::write(path.with_extension("scip"), index)
        .expect("synthetic SCIP fixture should be written");
    fs::write(
        path,
        "#!/bin/sh\nset -eu\noutput=\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = --output ]; then output=$2; shift 2; else shift; fi\ndone\ntest -n \"$output\"\ncp \"$0.scip\" \"$output\"\n",
    )
    .expect("synthetic rust-analyzer should be written");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("synthetic rust-analyzer should be executable");
}

fn valid_scip_index() -> Vec<u8> {
    let symbol = b"scip-rust pkg 0/Widget#";
    let relationship_target = b"scip-rust pkg 0/Base#";
    let range = [0_u8, 11, 17];
    let mut occurrence = scip_field(1, 2, &range);
    occurrence.extend(scip_field(2, 2, symbol));
    occurrence.extend(scip_field(3, 0, &[1]));
    let mut relationship = scip_field(1, 2, relationship_target);
    relationship.extend(scip_field(3, 0, &[1]));
    let mut symbol_information = scip_field(1, 2, symbol);
    symbol_information.extend(scip_field(4, 2, &relationship));
    let mut document = scip_field(1, 2, b"src/lib.rs");
    document.extend(scip_field(2, 2, &occurrence));
    document.extend(scip_field(3, 2, &symbol_information));
    document.extend(scip_field(6, 0, &[1]));
    let mut metadata = scip_field(1, 0, &[0]);
    metadata.extend(scip_field(4, 0, &[1]));
    let mut index = scip_field(1, 2, &metadata);
    index.extend(scip_field(2, 2, &document));
    index
}

fn scip_field(number: u8, wire_type: u8, payload: &[u8]) -> Vec<u8> {
    assert!(number < 16 && wire_type < 8 && payload.len() < 128);
    let mut field = vec![(number << 3) | wire_type];
    if wire_type == 2 {
        field.push(u8::try_from(payload.len()).expect("small test payload"));
    }
    field.extend(payload);
    field
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

fn assert_workspace_phase2_context_mcp(fixture: &WorkspaceGraphFixture) {
    let (child, mut input, mut output) = start_mcp_with_graph_workspace(
        &fixture.repository,
        &fixture.database,
        CONNECTED_WORKSPACE_ID,
        SOURCE_SLOT_ID,
    );
    initialize_mcp(&mut input, &mut output);
    let response = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 61,
            "method": "tools/call",
            "params": {
                "name": "phase2_context_build",
                "arguments": {"intent": "Widget", "budget_units": 4096}
            }
        }),
    );
    assert_eq!(response["result"]["isError"], serde_json::json!(false));
    let context = &response["result"]["structuredContent"];
    assert_eq!(context["schema_version"], serde_json::json!(1));
    assert!(context["scope"]["workspace_view"]
        .as_i64()
        .is_some_and(|view| view > 0));
    assert!(context["items"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item["payload"]["kind"] == "syntax")
    }));
    stop_mcp(child, input, output);
}

fn assert_workspace_phase2_context_mcp_with_scip(fixture: &WorkspaceGraphFixture) {
    let (child, mut input, mut output) = start_mcp_with_graph_workspace(
        &fixture.repository,
        &fixture.database,
        CONNECTED_WORKSPACE_ID,
        SOURCE_SLOT_ID,
    );
    initialize_mcp(&mut input, &mut output);
    let response = mcp_request(
        &mut input,
        &mut output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 62,
            "method": "tools/call",
            "params": {
                "name": "phase2_context_build",
                "arguments": {
                    "intent": "Widget",
                    "budget_units": 4096
                }
            }
        }),
    );
    assert_eq!(response["result"]["isError"], serde_json::json!(false));
    let context = &response["result"]["structuredContent"];
    let item = context["items"]
        .as_array()
        .and_then(|items| items.first())
        .expect("precise Phase 2 item");
    assert_eq!(item["tier"], serde_json::json!("precise_overlay"));
    assert_eq!(item["payload"]["kind"], serde_json::json!("precise_overlay"));
    assert_eq!(item["payload"]["relationship_count"], serde_json::json!(1));
    stop_mcp(child, input, output);
}
