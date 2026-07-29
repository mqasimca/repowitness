use serde_json::{Value, json};

use super::*;
use crate::{
    GET_ARCHITECTURE_ALIAS_TOOL_NAME, GET_CODE_SNIPPET_ALIAS_TOOL_NAME,
    GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME, INCUMBENT_COMPATIBLE_PROFILE, INCUMBENT_COMPATIBLE_SURFACE,
    INDEX_STATUS_ALIAS_TOOL_NAME, McpToolSurface, SEARCH_CODE_ALIAS_TOOL_NAME,
    SEARCH_GRAPH_ALIAS_TOOL_NAME, TRACE_PATH_ALIAS_TOOL_NAME,
};

const COMPATIBILITY_TOOL_NAMES: [&str; 18] = [
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
    IMPACT_ANALYZE_TOOL_NAME,
    INDEX_STATUS_ALIAS_TOOL_NAME,
    MEMORY_RECALL_TOOL_NAME,
    SEARCH_CODE_ALIAS_TOOL_NAME,
    SEARCH_GRAPH_ALIAS_TOOL_NAME,
    SYMBOL_GET_TOOL_NAME,
    TRACE_PATH_ALIAS_TOOL_NAME,
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
    assert_eq!(names.len(), 19);
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

    let requests = [
        (
            SEARCH_CODE_ALIAS_TOOL_NAME,
            json!({"query": "  run  ", "max_results": 7}),
        ),
        (
            GET_CODE_SNIPPET_ALIAS_TOOL_NAME,
            json!({
                "snapshot_sha256": "11".repeat(32),
                "generation": 9,
                "path": "rwp1:h:7372632F6C69622E7273",
                "content_sha256": "22".repeat(32),
                "artifact_sha256": "33".repeat(32),
                "fact_ordinal": 7,
            }),
        ),
        (SEARCH_GRAPH_ALIAS_TOOL_NAME, json!({"query": "run"})),
        (
            TRACE_PATH_ALIAS_TOOL_NAME,
            json!({
                "start": {"type": "definition", "definition": graph_definition_json()},
                "direction": "outbound",
                "edge_kinds": ["call"],
            }),
        ),
        (GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME, json!({})),
        (GET_ARCHITECTURE_ALIAS_TOOL_NAME, json!({})),
        (INDEX_STATUS_ALIAS_TOOL_NAME, json!({})),
    ];
    for (name, arguments) in requests {
        let response = client
            .call_tool(CallToolRequestParams::new(name).with_arguments(json_object(arguments)))
            .await
            .unwrap_or_else(|error| panic!("{name} failed: {error}"));
        assert_eq!(response.is_error, Some(false), "{name}");
        let content = response
            .structured_content
            .as_ref()
            .expect("structured compatibility result");
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
            content["repowitness"]["receipt"]["compatibility"]["behavior"],
            Value::String("not_assessed".to_owned())
        );
        assert!(
            content["repowitness"]["canonical"]["schema_version"].is_number()
                || name == GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME,
            "{name} must preserve the canonical response"
        );
    }

    assert_eq!(service.search_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.symbol_calls.load(Ordering::Relaxed), 1);
    assert_eq!(service.graph_calls.load(Ordering::Relaxed), 4);
    assert_eq!(service.diagnostics_calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        service.search_request.lock().expect("lock").as_ref(),
        Some(&("run".to_owned(), 7))
    );

    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}

#[tokio::test]
async fn malformed_alias_input_is_redacted_and_never_reaches_the_service() {
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
    let query_canary = "private_customer_query_canary";
    let path_canary = "/private/customer/path/canary";

    let error = client
        .call_tool(
            CallToolRequestParams::new(SEARCH_CODE_ALIAS_TOOL_NAME).with_arguments(json_object(
                json!({"query": query_canary, "repository": path_canary}),
            )),
        )
        .await
        .expect_err("unknown fields fail closed");
    let message = error.to_string();
    assert!(!message.contains(query_canary));
    assert!(!message.contains(path_canary));
    assert_eq!(service.search_calls.load(Ordering::Relaxed), 0);

    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}
