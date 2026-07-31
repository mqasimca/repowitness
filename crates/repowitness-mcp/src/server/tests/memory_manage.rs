use super::*;

#[test]
fn memory_manage_tool_is_default_deny_and_conservatively_annotated() {
    let read_only = RepoWitnessMcpServer::new(Arc::new(FakeService::new()));
    assert!(
        read_only
            .tools
            .iter()
            .all(|tool| tool.name.as_ref() != MEMORY_MANAGE_TOOL_NAME)
    );

    let enabled = RepoWitnessMcpServer::with_memory_writes(Arc::new(FakeService::new()));
    let names = enabled
        .tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            CODE_SEARCH_TOOL_NAME,
            CONTEXT_BUILD_TOOL_NAME,
            DIAGNOSTICS_TOOL_NAME,
            GRAPH_ARCHITECTURE_TOOL_NAME,
            GRAPH_EVIDENCE_TOOL_NAME,
            GRAPH_SEARCH_TOOL_NAME,
            GRAPH_STATUS_TOOL_NAME,
            GRAPH_TRACE_TOOL_NAME,
            IMPACT_ANALYZE_TOOL_NAME,
            MEMORY_MANAGE_TOOL_NAME,
            MEMORY_RECALL_TOOL_NAME,
            PHASE2_CONTEXT_BUILD_TOOL_NAME,
            SCIP_EVIDENCE_TOOL_NAME,
            SYMBOL_GET_TOOL_NAME,
        ]
    );
    let tool = enabled
        .tools
        .iter()
        .find(|tool| tool.name.as_ref() == MEMORY_MANAGE_TOOL_NAME)
        .expect("enabled memory-manage tool");
    let annotations = tool.annotations.as_ref().expect("annotations");
    assert_eq!(annotations.read_only_hint, Some(false));
    assert_eq!(annotations.destructive_hint, Some(true));
    assert_eq!(annotations.idempotent_hint, Some(false));
    assert_eq!(annotations.open_world_hint, Some(false));
    assert!(tool.output_schema.is_some());
}

#[tokio::test]
async fn enabled_server_validates_and_forwards_memory_manage() {
    let service = Arc::new(FakeService::new());
    let (server_transport, client_transport) = tokio::io::duplex(32 * 1024);
    let server = RepoWitnessMcpServer::with_memory_writes(service.clone());
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

    let response = client
        .call_tool(
            CallToolRequestParams::new(MEMORY_MANAGE_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({
                    "operation": "approve",
                    "record_id": "mem_00000000000000000000000000",
                    "timeout_ms": 1000,
                }),
            )),
        )
        .await
        .expect("memory management response");
    assert_eq!(response.is_error, Some(false));
    assert_eq!(
        response
            .structured_content
            .as_ref()
            .and_then(|value| value.get("schema_version"))
            .and_then(serde_json::Value::as_u64),
        Some(u64::from(MEMORY_MANAGE_SCHEMA_VERSION))
    );
    assert_eq!(
        response
            .structured_content
            .as_ref()
            .and_then(|value| value.get("receipt"))
            .and_then(|value| value.get("operation"))
            .and_then(serde_json::Value::as_str),
        Some("review")
    );
    assert_eq!(service.manage_calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        service.manage_request.lock().expect("lock").as_ref(),
        Some(&MemoryManageOperation::Approve)
    );

    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}

#[tokio::test]
async fn read_only_server_rejects_unlisted_memory_manage_without_invocation() {
    let service = Arc::new(FakeService::new());
    let (server_transport, client_transport) = tokio::io::duplex(32 * 1024);
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
            CallToolRequestParams::new(MEMORY_MANAGE_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({
                    "operation": "import_history",
                }),
            )),
        )
        .await
        .expect_err("unlisted mutation tool must be rejected");
    assert!(error.to_string().contains("unknown RepoWitness tool"));
    assert_eq!(service.manage_calls.load(Ordering::Relaxed), 0);

    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}
