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
            "graph_architecture",
            "graph_evidence",
            "graph_search",
            "graph_status",
            "graph_trace",
            "historical_memory",
            "impact_analyze",
            "memory_recall",
            "phase2_context_build",
            "scip_evidence",
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

fn assert_mcp_scip_not_produced(input: &mut ChildStdin, output: &mut BufReader<ChildStdout>) {
    let response = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 17,
            "method": "tools/call",
            "params": {
                "name": "scip_evidence",
                "arguments": {"symbol": "scip-rust pkg 1 Widget#"}
            }
        }),
    );
    let content = &response["result"]["structuredContent"];
    assert_eq!(content["schema_version"], serde_json::json!(1));
    assert_eq!(content["resolution"], serde_json::json!("not_produced"));
    assert!(content["overlay"].is_null());
    assert!(content["occurrences"].as_array().is_some_and(Vec::is_empty));
    assert!(content["relationships"].as_array().is_some_and(Vec::is_empty));
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

fn assert_mcp_phase2_context(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    request_id: usize,
    intent: &str,
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
                "name": "phase2_context_build",
                "arguments": {
                    "intent": intent,
                    "budget_units": 4096,
                    "max_provider_results": 5
                }
            }
        }),
    );
    assert_eq!(context["result"]["isError"], serde_json::json!(false));
    let context = &context["result"]["structuredContent"];
    assert_eq!(context["schema_version"], serde_json::json!(1));
    assert_eq!(
        context["profile_id"],
        serde_json::json!("phase2-evidence-balanced-v1")
    );
    assert!(context["scope"]["workspace_view"]
        .as_i64()
        .is_some_and(|view| view > 0));
    assert!(context["scope"]["source_epoch"]
        .as_u64()
        .is_some_and(|epoch| epoch > 0));
    assert!(context["provider_coverage"]
        .as_array()
        .is_some_and(|coverage| coverage.iter().any(|item| {
            item["tier"] == "syntax" && item["availability"] == "available"
        })));
    let source = context["items"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["payload"]["kind"] == "syntax"))
        .expect("Phase 2 context must include exact syntax evidence");
    assert_eq!(source["tier"], serde_json::json!("syntax"));
    assert_eq!(source["payload"]["declaration"], serde_json::json!(declaration));
    assert_eq!(
        source["providers"][0]["tier"],
        serde_json::json!("syntax")
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

fn assert_mcp_native_graph(input: &mut ChildStdin, output: &mut BufReader<ChildStdout>) {
    let (context, start) = assert_mcp_graph_status_and_search(input, output);
    let edge = assert_mcp_graph_trace(input, output, &context, start);
    assert_mcp_graph_evidence(input, output, &context, &edge);
    assert_mcp_graph_architecture(input, output, &context);
    assert_mcp_graph_impact(input, output, &context, &edge);
}

fn assert_mcp_graph_status_and_search(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
) -> (serde_json::Value, serde_json::Value) {
    let status = mcp_call_graph(input, output, 50, "graph_status", serde_json::json!({}));
    let status = &status["result"]["structuredContent"];
    assert_eq!(status["schema_version"], serde_json::json!(1));
    assert_eq!(status["availability"], serde_json::json!("complete"));
    let context = status["context"].clone();
    assert!(context["workspace_view"].as_i64().is_some_and(|view| view > 0));
    assert!(
        context["graph_generation"]
            .as_i64()
            .is_some_and(|generation| generation > 0)
    );
    assert!(
        context["publication"]["definition_count"]
            .as_u64()
            .is_some_and(|count| count >= 3)
    );

    let search = mcp_call_graph(
        input,
        output,
        51,
        "graph_search",
        serde_json::json!({
            "workspace_view": context["workspace_view"],
            "graph_generation": context["graph_generation"],
            "query": "invoke",
            "max_results": 5,
        }),
    );
    let search = &search["result"]["structuredContent"];
    assert_eq!(search["schema_version"], serde_json::json!(1));
    assert_eq!(search["context"], context);
    assert_eq!(search["matches_returned"], serde_json::json!(1));
    let start = search["definitions"][0].clone();
    (context, start)
}

fn assert_mcp_graph_trace(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    context: &serde_json::Value,
    start: serde_json::Value,
) -> serde_json::Value {
    let trace = mcp_call_graph(
        input,
        output,
        52,
        "graph_trace",
        serde_json::json!({
            "workspace_view": context["workspace_view"],
            "graph_generation": context["graph_generation"],
            "start": {"type": "definition", "definition": start},
            "direction": "outbound",
            "edge_kinds": ["call"],
            "max_results": 5,
        }),
    );
    let trace = &trace["result"]["structuredContent"];
    assert_eq!(trace["schema_version"], serde_json::json!(1));
    let edge = trace["trace"]["edges"]
        .as_array()
        .and_then(|edges| edges.first())
        .expect("invoke must have one retained call edge");
    assert_eq!(edge["edge_kind"], serde_json::json!("call"));
    assert!(edge["extraction_evidence"].as_str().is_some());
    assert!(edge["resolution_evidence"].as_str().is_some());
    edge.clone()
}

fn assert_mcp_graph_evidence(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    context: &serde_json::Value,
    edge: &serde_json::Value,
) {
    let evidence = mcp_call_graph(
        input,
        output,
        53,
        "graph_evidence",
        serde_json::json!({
            "workspace_view": context["workspace_view"],
            "graph_generation": context["graph_generation"],
            "site": edge["site"],
        }),
    );
    let evidence = &evidence["result"]["structuredContent"];
    assert_eq!(evidence["found"], serde_json::json!(true));
    assert_eq!(evidence["evidence"]["site"], edge["site"]);
    assert!(evidence["evidence"]["candidate_count"]
        .as_u64()
        .is_some_and(|count| count >= 1));

    let mut absent_site = edge["site"].clone();
    absent_site["ordinal"] = serde_json::json!(4_000_000_000_u32);
    let absent = mcp_call_graph(
        input,
        output,
        56,
        "graph_evidence",
        serde_json::json!({
            "workspace_view": context["workspace_view"],
            "graph_generation": context["graph_generation"],
            "site": absent_site,
        }),
    );
    let absent = &absent["result"]["structuredContent"];
    assert_eq!(absent["found"], serde_json::json!(false));
    assert_eq!(absent["evidence"], serde_json::Value::Null);
    assert_eq!(absent["context"]["publication"], context["publication"]);
}

fn assert_mcp_graph_architecture(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    context: &serde_json::Value,
) {
    let architecture = mcp_call_graph(
        input,
        output,
        54,
        "graph_architecture",
        serde_json::json!({
            "workspace_view": context["workspace_view"],
            "graph_generation": context["graph_generation"],
        }),
    );
    let architecture = &architecture["result"]["structuredContent"];
    assert_eq!(architecture["schema_version"], serde_json::json!(1));
    assert!(!architecture["definitions_by_kind"]
        .as_array()
        .expect("definition counts")
        .is_empty());
    assert!(!architecture["edges_by_kind"]
        .as_array()
        .expect("edge counts")
        .is_empty());
}

fn assert_mcp_graph_impact(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    context: &serde_json::Value,
    edge: &serde_json::Value,
) {
    let impact = mcp_call_graph(
        input,
        output,
        55,
        "impact_analyze",
        serde_json::json!({
            "workspace_view": context["workspace_view"],
            "graph_generation": context["graph_generation"],
            "start": edge["target"],
            "edge_kinds": ["call"],
            "max_results": 5,
        }),
    );
    let impact = &impact["result"]["structuredContent"];
    assert_eq!(impact["schema_version"], serde_json::json!(1));
    assert!(impact["impacts"]
        .as_array()
        .is_some_and(|impacts| !impacts.is_empty()));
    assert!(impact["unknown_coverage"].as_bool().is_some());
}

fn mcp_call_graph(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    request_id: usize,
    tool: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let response = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/call",
            "params": {"name": tool, "arguments": arguments}
        }),
    );
    assert_eq!(response["id"], serde_json::json!(request_id));
    assert_eq!(
        response["result"]["isError"],
        serde_json::json!(false),
        "{tool} returned an error: {response}"
    );
    response
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
    assert_eq!(diagnostics["schema_version"], serde_json::json!(3));
    assert_eq!(diagnostics["diagnostics_profile"], serde_json::json!(3));
    assert_mcp_configuration_identity(&diagnostics["configuration"]);
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
            "bounded_rust_syntax_graph",
            "current_memory_recall",
            "bounded_context_build"
        ])
    );
    assert_eq!(
        diagnostics["limitations"],
        serde_json::json!([
            "rust_graph_syntax_derived_only",
            "no_package_macro_scip_dynamic_or_cross_language_graph",
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

fn assert_mcp_configuration_identity(configuration: &serde_json::Value) {
    assert_sha256(
        &configuration["digest_sha256"],
        "resolved configuration",
    );
    assert_eq!(configuration["schema_version"], serde_json::json!(1));
    assert_eq!(configuration["resolver_version"], serde_json::json!(1));
    assert_eq!(configuration["profile"], serde_json::json!("local"));
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
