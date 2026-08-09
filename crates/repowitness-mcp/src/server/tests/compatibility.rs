use serde_json::{Value, json};

use super::*;
use crate::{
    GET_ARCHITECTURE_ALIAS_TOOL_NAME, GET_CODE_SNIPPET_ALIAS_TOOL_NAME,
    GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME, INCUMBENT_COMPATIBLE_PROFILE, INCUMBENT_COMPATIBLE_SURFACE,
    INDEX_STATUS_ALIAS_TOOL_NAME, McpToolSurface, SEARCH_CODE_ALIAS_TOOL_NAME,
    SEARCH_GRAPH_ALIAS_TOOL_NAME, TRACE_PATH_ALIAS_TOOL_NAME,
};

const COMPATIBILITY_TOOL_NAMES: [&str; 31] = [
    ARCHITECTURE_MAP_TOOL_NAME,
    ARCHITECTURE_OVERVIEW_TOOL_NAME,
    CODE_GRAPH_QUERY_TOOL_NAME,
    CODE_SEARCH_TOOL_NAME,
    CONTEXT_BUILD_TOOL_NAME,
    DIAGNOSTICS_TOOL_NAME,
    GET_ARCHITECTURE_ALIAS_TOOL_NAME,
    GET_CODE_SNIPPET_ALIAS_TOOL_NAME,
    GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME,
    GRAPH_ARCHITECTURE_TOOL_NAME,
    GRAPH_EVIDENCE_TOOL_NAME,
    GRAPH_SEARCH_TOOL_NAME,
    GRAPH_STATUS_TOOL_NAME,
    GRAPH_TRACE_TOOL_NAME,
    HISTORICAL_MEMORY_TOOL_NAME,
    IMPACT_ANALYZE_TOOL_NAME,
    INDEX_STATUS_ALIAS_TOOL_NAME,
    RELEVANT_PATHS_TOOL_NAME,
    MEMORY_RECALL_TOOL_NAME,
    OUTBOUND_SITES_TOOL_NAME,
    REPOSITORY_TOPOLOGY_TOOL_NAME,
    SCIP_EVIDENCE_TOOL_NAME,
    SCIP_RELATIONSHIP_TRACE_TOOL_NAME,
    SCIP_SYMBOL_RESOLVE_TOOL_NAME,
    SEARCH_CODE_ALIAS_TOOL_NAME,
    SEARCH_GRAPH_ALIAS_TOOL_NAME,
    SYMBOL_GET_TOOL_NAME,
    SYMBOL_SEARCH_TOOL_NAME,
    SYNTAX_SITE_SEARCH_TOOL_NAME,
    TRACE_PATH_ALIAS_TOOL_NAME,
    CHANGE_REVIEW_TOOL_NAME,
];

const ALIAS_NAMES: [&str; 7] = [
    GET_ARCHITECTURE_ALIAS_TOOL_NAME,
    GET_CODE_SNIPPET_ALIAS_TOOL_NAME,
    GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME,
    INDEX_STATUS_ALIAS_TOOL_NAME,
    SEARCH_CODE_ALIAS_TOOL_NAME,
    SEARCH_GRAPH_ALIAS_TOOL_NAME,
    TRACE_PATH_ALIAS_TOOL_NAME,
];

const RECEIPT_CANARY: &str = "private_receipt_query_canary";
const RECEIPT_PATH_CANARY: &str =
    "rwp1:h:707269766174655F726563656970745F706174685F63616E6172792E7273";
const ERROR_CANARY: &str = "private_error_boundary_canary";

fn sorted_string_array(value: Option<&Value>) -> Vec<String> {
    let mut values = value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| {
            value
                .as_str()
                .expect("schema name must be a string")
                .to_owned()
        })
        .collect::<Vec<_>>();
    values.sort();
    values
}

fn sorted_property_names(value: Option<&Value>) -> Vec<String> {
    let mut names = value
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|properties| properties.keys().cloned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn name_only_tools_list_contract(server: &RepoWitnessMcpServer) -> Value {
    let aliases = server
        .tools
        .iter()
        .filter(|tool| ALIAS_NAMES.contains(&tool.name.as_ref()))
        .map(|tool| {
            let annotations = tool.annotations.as_ref().expect("alias annotations");
            json!({
                "name": tool.name.as_ref(),
                "title": tool.title.as_deref(),
                "description": tool.description.as_deref(),
                "input_properties": sorted_property_names(
                    tool.input_schema.get("properties"),
                ),
                "input_required": sorted_string_array(tool.input_schema.get("required")),
                "input_additional_properties": tool
                    .input_schema
                    .get("additionalProperties"),
                "output_schema_present": tool.output_schema.is_some(),
                "annotations": {
                    "title": annotations.title.as_deref(),
                    "read_only": annotations.read_only_hint,
                    "destructive": annotations.destructive_hint,
                    "idempotent": annotations.idempotent_hint,
                    "open_world": annotations.open_world_hint,
                },
            })
        })
        .collect::<Vec<_>>();
    json!({
        "schema_version": 1,
        "profile": server.surface.profile(),
        "surface": server.surface.identifier(),
        "surface_tool_count": server.tools.len(),
        "aliases": aliases,
    })
}

fn service_call_count(service: &FakeService) -> usize {
    service.search_calls.load(Ordering::Relaxed)
        + service.context_calls.load(Ordering::Relaxed)
        + service.diagnostics_calls.load(Ordering::Relaxed)
        + service.graph_calls.load(Ordering::Relaxed)
        + service.scip_calls.load(Ordering::Relaxed)
        + service.manage_calls.load(Ordering::Relaxed)
        + service.memory_calls.load(Ordering::Relaxed)
        + service.symbol_calls.load(Ordering::Relaxed)
}

fn valid_alias_requests() -> [(&'static str, Value); 7] {
    let mut trace_definition = graph_definition_json();
    trace_definition["qualified_name"] = Value::String(format!("fixture::{RECEIPT_CANARY}"));
    [
        (
            SEARCH_CODE_ALIAS_TOOL_NAME,
            json!({"query": format!("  {RECEIPT_CANARY}  "), "max_results": 7}),
        ),
        (
            GET_CODE_SNIPPET_ALIAS_TOOL_NAME,
            json!({
                "snapshot_sha256": "11".repeat(32),
                "generation": 9,
                "path": RECEIPT_PATH_CANARY,
                "content_sha256": "22".repeat(32),
                "artifact_sha256": "33".repeat(32),
                "fact_ordinal": 7,
            }),
        ),
        (
            SEARCH_GRAPH_ALIAS_TOOL_NAME,
            json!({"query": RECEIPT_CANARY}),
        ),
        (
            TRACE_PATH_ALIAS_TOOL_NAME,
            json!({
                "start": {"type": "definition", "definition": trace_definition},
                "direction": "outbound",
                "edge_kinds": ["call"],
            }),
        ),
        (GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME, json!({})),
        (GET_ARCHITECTURE_ALIAS_TOOL_NAME, json!({})),
        (INDEX_STATUS_ALIAS_TOOL_NAME, json!({})),
    ]
}

fn assert_name_only_receipt(name: &str, content: &Value) {
    assert_eq!(
        content["repowitness"]["receipt"]["alias"],
        Value::String(name.to_owned())
    );
    assert_eq!(
        content["repowitness"]["receipt"]["profile"],
        Value::String(INCUMBENT_COMPATIBLE_PROFILE.to_owned())
    );
    assert_eq!(
        content["repowitness"]["receipt"]["surface"],
        Value::String(INCUMBENT_COMPATIBLE_SURFACE.to_owned())
    );
    assert_eq!(
        content["repowitness"]["receipt"]["compatibility"]["name"],
        Value::String("compatible".to_owned())
    );
    assert_eq!(
        content["repowitness"]["receipt"]["compatibility"]["request"],
        Value::String("incompatible".to_owned())
    );
    assert_eq!(
        content["repowitness"]["receipt"]["compatibility"]["response"],
        Value::String("not_assessed".to_owned())
    );
    assert_eq!(
        content["repowitness"]["receipt"]["compatibility"]["behavior"],
        Value::String("not_assessed".to_owned())
    );
    assert_eq!(
        content["repowitness"]["receipt"]["observation"]["release"],
        Value::String("v0.9.0".to_owned())
    );
    assert!(
        content["repowitness"]["canonical"]["schema_version"].is_number()
            || name == GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME,
        "{name} must preserve the canonical response"
    );
    let encoded = serde_json::to_string(content).expect("compatibility response serializes");
    assert!(!encoded.contains(RECEIPT_CANARY), "{name}");
    assert!(!encoded.contains(RECEIPT_PATH_CANARY), "{name}");
}

#[test]
fn opt_in_surface_is_exact_sorted_read_only_and_excludes_unimplemented_names() {
    let default_server = RepoWitnessMcpServer::new(Arc::new(FakeService::new()));
    assert_eq!(
        default_server
            .tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>(),
        graph::native_tool_names()
    );

    let server = RepoWitnessMcpServer::with_surface(
        Arc::new(FakeService::new()),
        McpToolSurface::NativeV1PlusIncumbentSubsetV1,
    );
    assert_eq!(
        server
            .tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>(),
        COMPATIBILITY_TOOL_NAMES
    );
    for alias in ALIAS_NAMES {
        let tool = server
            .tools
            .iter()
            .find(|tool| tool.name.as_ref() == alias)
            .expect("advertised alias");
        assert_eq!(
            tool.input_schema.get("additionalProperties"),
            Some(&Value::Bool(false))
        );
        assert!(tool.output_schema.is_some());
        let annotations = tool.annotations.as_ref().expect("annotations");
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(false));
    }
    let encoded = serde_json::to_string(&server.tools).expect("tool list serializes");
    for excluded in [
        "detect_changes",
        "list_projects",
        "query_graph",
        "delete_project",
        "manage_adr",
        "ingest_traces",
        "index_repository",
    ] {
        assert!(
            !encoded.contains(excluded),
            "{excluded} must not be advertised"
        );
    }

    let info = server.get_info();
    let instructions = info.instructions.expect("fixed instructions");
    assert!(instructions.contains(INCUMBENT_COMPATIBLE_PROFILE));
    assert!(instructions.contains(INCUMBENT_COMPATIBLE_SURFACE));
    assert!(!instructions.contains("private"));
}

#[test]
fn opt_in_aliases_match_the_exact_name_only_tools_list_golden() {
    let server = RepoWitnessMcpServer::with_surface(
        Arc::new(FakeService::new()),
        McpToolSurface::NativeV1PlusIncumbentSubsetV1,
    );
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/incumbent-subset-v1-tools-list.json"))
            .expect("independently authored local tools/list golden");

    assert_eq!(name_only_tools_list_contract(&server), expected);
}

#[test]
fn memory_capability_adds_only_the_canonical_mutation_tool() {
    let server = RepoWitnessMcpServer::with_surface_and_memory_writes(
        Arc::new(FakeService::new()),
        McpToolSurface::NativeV1PlusIncumbentSubsetV1,
    );
    let names = server
        .tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 32);
    assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(
        names
            .iter()
            .filter(|name| **name == MEMORY_MANAGE_TOOL_NAME)
            .count(),
        1
    );
}

#[tokio::test]
async fn canonical_surface_rejects_aliases_without_invoking_repository_work() {
    let service = Arc::new(FakeService::new());
    let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
    let server = RepoWitnessMcpServer::new(service.clone());
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("server starts")
            .waiting()
            .await
            .expect("server stops")
    });
    let client = ().serve(client_transport).await.expect("client starts");

    let error = client
        .call_tool(
            CallToolRequestParams::new(SEARCH_CODE_ALIAS_TOOL_NAME)
                .with_arguments(json_object(json!({"query": "run"}))),
        )
        .await
        .expect_err("alias is unavailable on the canonical surface");
    assert!(error.to_string().contains("unknown RepoWitness tool"));
    assert_eq!(service.search_calls.load(Ordering::Relaxed), 0);

    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}

#[tokio::test]
async fn compatibility_aliases_forward_to_canonical_use_cases_and_preserve_receipts() {
    let service = Arc::new(FakeService::new());
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let server = RepoWitnessMcpServer::with_surface(
        service.clone(),
        McpToolSurface::NativeV1PlusIncumbentSubsetV1,
    );
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("server starts")
            .waiting()
            .await
            .expect("server stops")
    });
    let client = ().serve(client_transport).await.expect("client starts");

    for (name, arguments) in valid_alias_requests() {
        let response = client
            .call_tool(CallToolRequestParams::new(name).with_arguments(json_object(arguments)))
            .await
            .unwrap_or_else(|error| panic!("{name} failed: {error}"));
        assert_eq!(response.is_error, Some(false), "{name}");
        let content = response
            .structured_content
            .as_ref()
            .expect("structured compatibility result");
        assert_name_only_receipt(name, content);
    }

    assert_eq!(service.search_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.symbol_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.graph_calls.load(Ordering::Relaxed), 4);
    assert_eq!(service.diagnostics_calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        service.search_request.lock().expect("lock").as_ref(),
        Some(&(RECEIPT_CANARY.to_owned(), 7))
    );

    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}

#[tokio::test]
async fn every_alias_rejects_invalid_input_without_service_access_or_canary_disclosure() {
    let service = Arc::new(FakeService::new());
    let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
    let server = RepoWitnessMcpServer::with_surface(
        service.clone(),
        McpToolSurface::NativeV1PlusIncumbentSubsetV1,
    );
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("server starts")
            .waiting()
            .await
            .expect("server stops")
    });
    let client = ().serve(client_transport).await.expect("client starts");
    let mut trace_definition = graph_definition_json();
    trace_definition["qualified_name"] = Value::String(format!("fixture::{ERROR_CANARY}"));
    let cases = [
        (
            SEARCH_CODE_ALIAS_TOOL_NAME,
            json!({"query": ERROR_CANARY, "max_results": 101}),
        ),
        (
            GET_CODE_SNIPPET_ALIAS_TOOL_NAME,
            json!({
                "snapshot_sha256": ERROR_CANARY,
                "generation": 9,
                "path": "rwp1:h:7372632F6C69622E7273",
                "content_sha256": "22".repeat(32),
                "artifact_sha256": "33".repeat(32),
                "fact_ordinal": 7,
            }),
        ),
        (
            SEARCH_GRAPH_ALIAS_TOOL_NAME,
            json!({"query": ERROR_CANARY, "max_results": 0}),
        ),
        (
            TRACE_PATH_ALIAS_TOOL_NAME,
            json!({
                "start": {"type": "definition", "definition": trace_definition},
                "direction": "outbound",
                "edge_kinds": ["call"],
                "max_depth": 6,
            }),
        ),
        (
            GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME,
            json!({"timeout_ms": ERROR_CANARY}),
        ),
        (
            GET_ARCHITECTURE_ALIAS_TOOL_NAME,
            json!({"workspace_view": ERROR_CANARY}),
        ),
        (
            INDEX_STATUS_ALIAS_TOOL_NAME,
            json!({"timeout_ms": ERROR_CANARY}),
        ),
    ];

    for (name, arguments) in cases {
        let result = client
            .call_tool(CallToolRequestParams::new(name).with_arguments(json_object(arguments)))
            .await;
        let error = match result {
            Ok(_) => panic!("{name} accepted invalid input"),
            Err(error) => error,
        };
        let message = error.to_string();
        assert!(!message.contains(ERROR_CANARY), "{name}");
        assert_eq!(service_call_count(&service), 0, "{name}");
    }

    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}
