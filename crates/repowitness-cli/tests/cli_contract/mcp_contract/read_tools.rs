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
    let tools = listed["result"]["tools"].as_array().expect("tool list");
    assert_eq!(
        tools
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
    let diagnostics = tools
        .iter()
        .find(|tool| tool["name"] == "diagnostics")
        .expect("diagnostics tool");
    let output_properties = diagnostics["outputSchema"]["properties"]
        .as_object()
        .expect("diagnostics output properties");
    assert!(output_properties.contains_key("syntax_error_nodes"));
    assert!(output_properties.contains_key("known_parser_limitation_nodes"));
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
    assert_eq!(symbol["schema_version"], serde_json::json!(4));
    assert_eq!(symbol["symbol_profile"], serde_json::json!(3));
    assert_eq!(symbol["symbol"]["language"], serde_json::json!("rust"));
    assert_eq!(
        symbol["symbol"]["declaration"],
        serde_json::json!("pub struct Widget;")
    );
    assert_eq!(
        symbol["symbol"]["declaration_encoding"],
        serde_json::json!("utf8")
    );
}

fn assert_mcp_context(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    request_id: usize,
    intent: &str,
    language: &str,
    declaration_encoding: &str,
    declaration: &str,
) {
    let context = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/call",
            "params": {
                "name": "context_build",
                "arguments": {
                    "intent": intent,
                    "budget_units": 4096,
                    "max_provider_results": 5
                }
            }
        }),
    );
    let context = &context["result"]["structuredContent"];
    assert_eq!(context["schema_version"], serde_json::json!(2));
    let source = context["items"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["kind"] == "source"))
        .expect("context must include exact source");
    assert_eq!(source["language"], serde_json::json!(language));
    assert_eq!(source["name"], serde_json::json!(intent));
    assert_eq!(
        source["declaration_encoding"],
        serde_json::json!(declaration_encoding)
    );
    assert_eq!(source["declaration"], serde_json::json!(declaration));
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
        symbol["symbol"]["declaration"],
        serde_json::json!("func (Gadget) Launch() {}")
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
        symbol["symbol"]["declaration"],
        serde_json::json!(std::str::from_utf8(declaration).expect("fixture declaration is UTF-8"))
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

fn assert_mcp_diagnostics_and_absent_memory(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    diagnostics_request_id: usize,
    memory_request_id: usize,
) {
    let response = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": diagnostics_request_id,
            "method": "tools/call",
            "params": {"name": "diagnostics", "arguments": {}}
        }),
    );
    assert_eq!(response["id"], serde_json::json!(diagnostics_request_id));
    assert_eq!(response["result"]["isError"], serde_json::json!(false));
    let diagnostics = &response["result"]["structuredContent"];
    assert_eq!(diagnostics["schema_version"], serde_json::json!(2));
    assert_eq!(diagnostics["diagnostics_profile"], serde_json::json!(2));
    assert_sha256(&diagnostics["snapshot_sha256"], "source snapshot");
    assert_sha256(
        &diagnostics["producer_manifest_sha256"],
        "producer manifest",
    );
    assert!(
        diagnostics["generation"]
            .as_i64()
            .is_some_and(|generation| generation > 0)
    );
    assert!(diagnostics["source_epoch"].as_u64().is_some());
    let coverage = &diagnostics["index_coverage"];
    assert!(
        coverage["searched"]
            .as_u64()
            .is_some_and(|searched| searched > 0),
        "a repository with a retrieved symbol must report searched source"
    );
    for field in ["skipped", "unresolved", "truncated"] {
        assert!(
            coverage[field].as_u64().is_some(),
            "index coverage must expose {field}"
        );
    }
    let syntax_error_nodes = diagnostics["syntax_error_nodes"]
        .as_u64()
        .expect("raw parser diagnostics");
    let known_parser_limitation_nodes = diagnostics["known_parser_limitation_nodes"]
        .as_u64()
        .expect("known parser limitation diagnostics");
    assert!(known_parser_limitation_nodes <= syntax_error_nodes);
    assert_eq!(diagnostics["memory_projection"], serde_json::Value::Null);
    assert_eq!(
        diagnostics["supported_languages"],
        serde_json::json!(["rust", "go", "typescript", "tsx", "python"])
    );
    assert_eq!(
        diagnostics["capabilities"],
        serde_json::json!([
            "lexical_source_search",
            "exact_symbol_source",
            "current_memory_recall",
            "bounded_context_build"
        ])
    );
    assert_eq!(
        diagnostics["limitations"],
        serde_json::json!([
            "no_reference_index",
            "no_structural_graph",
            "no_history_search",
            "no_vector_retrieval",
            "no_model_tokenizer",
            "no_remote_transport"
        ])
    );

    let recall = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": memory_request_id,
            "method": "tools/call",
            "params": {
                "name": "memory_recall",
                "arguments": {"all_records": true, "max_results": 5}
            }
        }),
    );
    assert_eq!(recall["id"], serde_json::json!(memory_request_id));
    assert_eq!(recall["result"]["isError"], serde_json::json!(true));
    assert_eq!(
        recall["result"]["content"][0]["text"],
        serde_json::json!("memory recall failed")
    );
    assert_eq!(
        recall["result"]["structuredContent"],
        serde_json::Value::Null
    );
}

fn assert_sha256(value: &serde_json::Value, label: &str) {
    let digest = value.as_str().expect("SHA-256 value must be a string");
    assert_eq!(digest.len(), 64, "{label} SHA-256 length");
    assert!(
        digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{label} SHA-256 must use lowercase hexadecimal"
    );
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
