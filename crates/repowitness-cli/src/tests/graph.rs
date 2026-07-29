use std::cell::{Cell, RefCell};

use super::*;

const REPOSITORY_ID: &str = concat!(
    "rwi1:h:",
    "ABABABABABABABABABABABABABABABABABABABABABABABABABABABABABABABAB"
);
const CONNECTED_WORKSPACE_ID: &str = concat!(
    "cwi1:h:",
    "1111111111111111111111111111111111111111111111111111111111111111"
);
const SOURCE_SLOT_ID: &str = concat!(
    "ssi1:h:",
    "2222222222222222222222222222222222222222222222222222222222222222"
);

struct FakeGraphReader {
    calls: Cell<u64>,
    exact_pin: Cell<Option<(i64, i64)>>,
    operation: RefCell<Option<&'static str>>,
    workspace: RefCell<Option<GraphWorkspaceContext>>,
}

impl FakeGraphReader {
    fn new() -> Self {
        Self {
            calls: Cell::new(0),
            exact_pin: Cell::new(None),
            operation: RefCell::new(None),
            workspace: RefCell::new(None),
        }
    }
}

impl RepositoryGraphReader for FakeGraphReader {
    fn read(
        &self,
        invocation: GraphInvocation,
        _configuration: &ResolvedConfiguration,
    ) -> Result<GraphReadServiceOutput, ()> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(invocation.database, Path::new("../graph.db"));
        self.workspace.replace(Some(invocation.workspace.clone()));
        self.exact_pin.set(invocation.request.exact_pin());
        let operation = match invocation.request.into_operation() {
            repowitness_local::RustGraphReadOperation::Status => "status",
            repowitness_local::RustGraphReadOperation::Search { .. } => "search",
            repowitness_local::RustGraphReadOperation::Evidence { .. } => "evidence",
            repowitness_local::RustGraphReadOperation::Architecture { .. } => "architecture",
            repowitness_local::RustGraphReadOperation::Trace { .. } => "trace",
            repowitness_local::RustGraphReadOperation::Impact { .. } => "impact",
        };
        self.operation.replace(Some(operation));
        Ok(status_output())
    }
}

#[test]
fn graph_status_forwards_an_explicit_connected_workspace_source_slot() {
    let reader = FakeGraphReader::new();
    let arguments = [
        "status",
        "--connected-workspace-id",
        CONNECTED_WORKSPACE_ID,
        "--source-slot-id",
        SOURCE_SLOT_ID,
        "--database",
        "../graph.db",
    ]
    .into_iter()
    .map(OsString::from);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_graph(
        arguments,
        &mut stdout,
        &mut stderr,
        &reader,
        &LocalConfigurationLoader,
    );

    assert_eq!(code, EXIT_SUCCESS, "{}", String::from_utf8_lossy(&stderr));
    assert!(stderr.is_empty());
    assert!(matches!(
        reader.workspace.borrow().as_ref(),
        Some(GraphWorkspaceContext::ConnectedWorkspace {
            connected_workspace,
            source_slot,
        }) if connected_workspace == CONNECTED_WORKSPACE_ID && source_slot == SOURCE_SLOT_ID
    ));
}

fn status_output() -> GraphReadServiceOutput {
    GraphReadServiceOutput::Status(GraphStatusOutput {
        schema_version: 1,
        context: McpGraphContext {
            connected_workspace: format!("cwi1:h:{}", "AB".repeat(32)),
            workspace_view: 4,
            graph_generation: 9,
            publication: None,
        },
        availability: "not_produced".to_owned(),
    })
}

fn definition() -> serde_json::Value {
    serde_json::json!({
        "source_slot": format!("ssi1:h:{}", "AB".repeat(32)),
        "source_generation": 9,
        "path": "rwp1:h:7372632F6C69622E7273",
        "content_sha256": "22".repeat(32),
        "artifact_sha256": "33".repeat(32),
        "fact_ordinal": 7,
        "symbol_kind": "function",
        "name": "run",
        "qualified_name": "fixture::run",
        "name_span": {"start": 7, "end": 10},
        "declaration_span": {"start": 0, "end": 13},
    })
}

fn site() -> serde_json::Value {
    serde_json::json!({
        "source_slot": format!("ssi1:h:{}", "AB".repeat(32)),
        "path": "rwp1:h:7372632F6C69622E7273",
        "artifact_sha256": "33".repeat(32),
        "ordinal": 1,
        "site_kind": "call",
        "occurrence_span": {"start": 0, "end": 13},
        "target_span": {"start": 7, "end": 10},
    })
}

fn base(operation: &str) -> Vec<OsString> {
    [
        operation,
        "--repository-id",
        REPOSITORY_ID,
        "--database",
        "../graph.db",
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

#[test]
fn graph_status_forwards_exact_pin_and_emits_bounded_json() {
    let reader = FakeGraphReader::new();
    let arguments = [
        "status",
        "--repository-id",
        REPOSITORY_ID,
        "--database",
        "../graph.db",
        "--workspace-view",
        "4",
        "--graph-generation",
        "9",
    ]
    .into_iter()
    .map(OsString::from);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_graph(
        arguments,
        &mut stdout,
        &mut stderr,
        &reader,
        &LocalConfigurationLoader,
    );
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert_eq!(reader.calls.get(), 1);
    assert_eq!(reader.exact_pin.get(), Some((4, 9)));
    assert!(matches!(
        reader.workspace.borrow().as_ref(),
        Some(GraphWorkspaceContext::SingleRepository(repository_identity))
            if repository_identity == REPOSITORY_ID
    ));
    assert_eq!(*reader.operation.borrow(), Some("status"));
    assert_eq!(stdout.last(), Some(&b'\n'));
    let output: serde_json::Value =
        serde_json::from_slice(&stdout).expect("graph output is one JSON document");
    assert_eq!(output["schema_version"], 1);
    assert_eq!(output["availability"], "not_produced");
    assert_eq!(output["context"]["workspace_view"], 4);
    assert!(
        !String::from_utf8(stdout)
            .expect("utf8")
            .contains("../graph.db")
    );
}

#[test]
fn every_native_graph_operation_accepts_its_canonical_shape() {
    let cases = [
        ("status", Vec::<(&str, String)>::new()),
        ("search", vec![("--query", "run".to_owned())]),
        (
            "evidence",
            vec![(
                "--site-json",
                serde_json::to_string(&site()).expect("site JSON"),
            )],
        ),
        ("architecture", vec![("--max-results", "7".to_owned())]),
        (
            "trace",
            vec![
                (
                    "--start-json",
                    serde_json::to_string(&serde_json::json!({
                        "type": "definition",
                        "definition": definition(),
                    }))
                    .expect("trace start JSON"),
                ),
                ("--direction", "outbound".to_owned()),
                ("--edge-kind", "call".to_owned()),
                ("--edge-kind", "reference".to_owned()),
            ],
        ),
        (
            "impact",
            vec![
                (
                    "--start-json",
                    serde_json::to_string(&definition()).expect("definition JSON"),
                ),
                ("--edge-kind", "call".to_owned()),
            ],
        ),
    ];

    for (expected, options) in cases {
        let mut arguments = base(expected);
        for (option, value) in options {
            arguments.push(OsString::from(option));
            arguments.push(OsString::from(value));
        }
        let invocation = parse_graph_arguments(&arguments).expect("canonical operation");
        let actual = match invocation.request.into_operation() {
            repowitness_local::RustGraphReadOperation::Status => "status",
            repowitness_local::RustGraphReadOperation::Search { .. } => "search",
            repowitness_local::RustGraphReadOperation::Evidence { .. } => "evidence",
            repowitness_local::RustGraphReadOperation::Architecture { .. } => "architecture",
            repowitness_local::RustGraphReadOperation::Trace { .. } => "trace",
            repowitness_local::RustGraphReadOperation::Impact { .. } => "impact",
        };
        assert_eq!(actual, expected);
    }
}

#[test]
fn invalid_graph_inputs_fail_before_configuration_or_repository_io() {
    let reader = FakeGraphReader::new();
    for arguments in invalid_graph_argument_sets() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run_graph(
            arguments.into_iter().map(OsString::from),
            &mut stdout,
            &mut stderr,
            &reader,
            &LocalConfigurationLoader,
        );
        assert_eq!(code, EXIT_USAGE);
        assert!(stdout.is_empty());
        assert!(!stderr.is_empty());
        assert!(
            !String::from_utf8(stderr)
                .expect("utf8")
                .contains("private-invalid-json")
        );
    }
    assert_eq!(reader.calls.get(), 0);
}

fn invalid_graph_argument_sets() -> Vec<Vec<&'static str>> {
    vec![
        vec!["status", "--repository-id", REPOSITORY_ID],
        vec![
            "status",
            "--connected-workspace-id",
            CONNECTED_WORKSPACE_ID,
            "--database",
            "../graph.db",
        ],
        vec![
            "status",
            "--source-slot-id",
            SOURCE_SLOT_ID,
            "--database",
            "../graph.db",
        ],
        vec![
            "status",
            "--repository-id",
            REPOSITORY_ID,
            "--connected-workspace-id",
            CONNECTED_WORKSPACE_ID,
            "--source-slot-id",
            SOURCE_SLOT_ID,
            "--database",
            "../graph.db",
        ],
        vec![
            "status",
            "--connected-workspace-id",
            "not-an-identity",
            "--source-slot-id",
            SOURCE_SLOT_ID,
            "--database",
            "../graph.db",
        ],
        vec![
            "status",
            "--repository-id",
            "not-an-identity",
            "--database",
            "../graph.db",
        ],
        vec![
            "status",
            "--repository-id",
            REPOSITORY_ID,
            "--database",
            "../graph.db",
            "--workspace-view",
            "4",
        ],
        vec![
            "architecture",
            "--repository-id",
            REPOSITORY_ID,
            "--database",
            "../graph.db",
            "--max-output-bytes",
            "16777217",
        ],
        vec![
            "evidence",
            "--repository-id",
            REPOSITORY_ID,
            "--database",
            "../graph.db",
            "--site-json",
            "{private-invalid-json",
        ],
        vec![
            "trace",
            "--repository-id",
            REPOSITORY_ID,
            "--database",
            "../graph.db",
            "--start-json",
            "{}",
            "--direction",
            "outbound",
            "--edge-kind",
            "call",
            "--edge-kind",
            "call",
        ],
    ]
}

#[test]
fn graph_help_does_not_open_configuration_or_repository_state() {
    let reader = FakeGraphReader::new();
    for arguments in [vec!["--help"], vec!["trace", "--help"]] {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run_graph(
            arguments.into_iter().map(OsString::from),
            &mut stdout,
            &mut stderr,
            &reader,
            &LocalConfigurationLoader,
        );
        assert_eq!(code, EXIT_SUCCESS);
        assert!(stderr.is_empty());
        assert!(
            String::from_utf8(stdout)
                .expect("utf8")
                .contains("graph trace")
        );
    }
    assert_eq!(reader.calls.get(), 0);
}
