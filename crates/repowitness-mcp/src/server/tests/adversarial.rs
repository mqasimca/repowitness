use std::future;

use super::*;

#[test]
fn tool_schemas_are_deterministic_and_never_delegate_local_authority() {
    let first = RepoWitnessMcpServer::with_memory_writes(Arc::new(FakeService::new()));
    let second = RepoWitnessMcpServer::with_memory_writes(Arc::new(FakeService::new()));
    assert_eq!(
        serde_json::to_vec(first.tools.as_ref()).expect("first schema serializes"),
        serde_json::to_vec(second.tools.as_ref()).expect("second schema serializes")
    );

    let memory = first
        .tools
        .iter()
        .find(|tool| tool.name.as_ref() == MEMORY_MANAGE_TOOL_NAME)
        .expect("enabled memory tool");
    let properties = memory
        .input_schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .expect("object properties");
    for forbidden in ["actor", "database", "repository", "repository_id", "root"] {
        assert!(!properties.contains_key(forbidden));
    }
}

#[tokio::test]
async fn invalid_decoded_symbol_paths_and_large_ordinals_never_reach_the_service() {
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
    let selector = |path: &str, fact_ordinal: u64| {
        CallToolRequestParams::new(SYMBOL_GET_TOOL_NAME).with_arguments(json_object(
            serde_json::json!({
                "snapshot_sha256": "11".repeat(32),
                "generation": 9,
                "path": path,
                "content_sha256": "22".repeat(32),
                "artifact_sha256": "33".repeat(32),
                "fact_ordinal": fact_ordinal,
            }),
        ))
    };

    for request in [
        selector("rwp1:h:00", 0),
        selector(
            "rwp1:h:7372632F6C69622E7273",
            MAX_MCP_INTEROPERABLE_INTEGER + 1,
        ),
    ] {
        client
            .call_tool(request)
            .await
            .expect_err("invalid selector must be a protocol error");
    }
    assert_eq!(service.symbol_calls.load(Ordering::Relaxed), 0);

    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}

#[tokio::test]
async fn request_deadline_cancels_and_joins_cooperative_blocking_work() {
    let service = Arc::new(CancellationService {
        started: AtomicBool::new(false),
        observed: AtomicBool::new(false),
    });
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
            CallToolRequestParams::new(CODE_SEARCH_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({"query": "run", "timeout_ms": 10}),
            )),
        )
        .await
        .expect_err("deadline must be a protocol error");
    assert!(error.to_string().contains("deadline exceeded"));
    assert!(service.started.load(Ordering::Acquire));
    assert!(service.observed.load(Ordering::Acquire));

    client.cancel().await.expect("client closes");
    server_task.await.expect("server task");
}

#[tokio::test]
async fn admission_wait_honors_deadline_and_preexisting_cancellation() {
    let semaphore = Arc::new(Semaphore::new(1));
    let _held = Arc::clone(&semaphore)
        .acquire_owned()
        .await
        .expect("semaphore is open");
    let deadline = Instant::now() + Duration::from_millis(10);
    let error = acquire_permit(Arc::clone(&semaphore), deadline, future::pending())
        .await
        .expect_err("queued admission must time out");
    assert!(error.to_string().contains("deadline exceeded"));

    let deadline = Instant::now() + Duration::from_secs(1);
    let error = acquire_permit(semaphore, deadline, future::ready(()))
        .await
        .expect_err("pre-cancelled admission must fail");
    assert!(error.to_string().contains("request cancelled"));
}

#[test]
fn encoded_output_budget_is_inclusive_at_the_exact_call_result_size() {
    let output = search_output();
    let structured = CallToolResult::structured(
        serde_json::to_value(&output).expect("fixture output serializes"),
    );
    let exact = serde_json::to_vec(&structured)
        .expect("call result serializes")
        .len();
    assert_eq!(
        operation_result(Ok(output.clone()), exact)
            .expect("exact budget succeeds")
            .is_error,
        Some(false)
    );
    assert_eq!(
        operation_result(Ok(output), exact - 1)
            .expect("over-budget response is a tool result")
            .is_error,
        Some(true)
    );
}
