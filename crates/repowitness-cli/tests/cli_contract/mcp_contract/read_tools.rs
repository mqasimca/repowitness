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
            "architecture_map",
            "architecture_overview",
            "code_graph_query",
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
            "locate_relevant_paths",
            "memory_recall",
            "outbound_sites",
            "phase2_context_build",
            "repository_topology",
            "scip_evidence",
            "scip_relationship_trace",
            "scip_symbol_resolve",
            "symbol_get",
            "symbol_search",
            "syntax_site_search",
            "verify"
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
    let verify = tools
        .iter()
        .find(|tool| tool["name"] == "verify")
        .expect("verify tool");
    assert!(verify["inputSchema"]["properties"].get("base").is_some());
    assert!(verify["outputSchema"]["properties"].get("verdict").is_some());
}

fn assert_mcp_repository_topology(input: &mut ChildStdin, output: &mut BufReader<ChildStdout>) {
    let topology = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 111,
            "method": "tools/call",
            "params": {
                "name": "repository_topology",
                "arguments": {"max_paths": 1}
            }
        }),
    );
    assert_eq!(topology["result"]["isError"], serde_json::json!(false));
    let topology = &topology["result"]["structuredContent"];
    assert_eq!(topology["schema_version"], serde_json::json!(1));
    assert_eq!(topology["topology_profile"], serde_json::json!(1));
    assert_eq!(topology["coverage"]["omitted_paths"], serde_json::json!(0));
    assert_eq!(topology["paths_returned"], serde_json::json!(1));
    assert_eq!(topology["truncated"], serde_json::json!(true));
    assert_eq!(
        topology["limitation"],
        serde_json::json!("inventory_only_no_semantic_relationship_inference")
    );
    assert!(topology["total_paths"].as_u64().is_some_and(|total| total > 1));
    assert!(topology["snapshot_sha256"]
        .as_str()
        .is_some_and(|digest| digest.len() == 64));
    assert!(topology["topology_sha256"]
        .as_str()
        .is_some_and(|digest| digest.len() == 64));
    assert_eq!(topology["entries"].as_array().map(Vec::len), Some(1));
    assert!(topology["entries"][0]["path"]
        .as_str()
        .is_some_and(|path| path.starts_with("rwp1:h:")));
}

fn assert_mcp_relevant_paths(input: &mut ChildStdin, output: &mut BufReader<ChildStdout>) {
    let located = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 112,
            "method": "tools/call",
            "params": {
                "name": "locate_relevant_paths",
                "arguments": {"query": "Widget", "max_paths": 5}
            }
        }),
    );
    assert_eq!(located["result"]["isError"], serde_json::json!(false));
    let located = &located["result"]["structuredContent"];
    assert_mcp_relevant_paths_receipt(located);
}

fn assert_mcp_relevant_paths_receipt(located: &serde_json::Value) {
    assert_eq!(located["schema_version"], serde_json::json!(1));
    assert_eq!(located["path_ranking_profile"], serde_json::json!(1));
    assert_eq!(located["resolution"], serde_json::json!("confirmed"));
    assert!(located["matches_returned"].as_u64().is_some_and(|count| count >= 1));
    assert!(located["matches_total"]
        .as_u64()
        .zip(located["matches_returned"].as_u64())
        .is_some_and(|(total, returned)| total >= returned));
    for field in ["searched", "skipped", "unresolved", "truncated"] {
        assert!(located["coverage"][field].as_u64().is_some());
    }
    assert_eq!(
        located["paths_returned"],
        serde_json::json!(located["paths"].as_array().expect("paths").len())
    );
    assert!(located["returned_match_paths_total"]
        .as_u64()
        .is_some_and(|total| total >= 1));
    assert_eq!(
        located["returned_match_paths_truncated"],
        serde_json::json!(
            located["paths_returned"].as_u64()
                < located["returned_match_paths_total"].as_u64()
        )
    );
    assert!(located["paths"][0]["path"]
        .as_str()
        .is_some_and(|path| path.starts_with("rwp1:h:")));
    assert_eq!(
        located["limitations"],
        serde_json::json!([
            "indexed_supported_language_declaration_lexical_only",
            "ordered_by_returned_match_count_then_canonical_path",
            "path_summaries_cover_only_returned_declaration_matches",
            "no_relationship_or_semantic_relevance_claim"
        ])
    );
    assert_eq!(
        located["matches"].as_array().map(Vec::len),
        located["matches_returned"].as_u64().map(|count| count as usize)
    );
}

fn assert_mcp_primary_discovery_tools(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
) {
    assert_mcp_tools(input, output);
    assert_mcp_architecture_map(input, output);
    assert_mcp_architecture_overview(input, output);
    assert_mcp_repository_topology(input, output);
    assert_mcp_relevant_paths(input, output);
    assert_mcp_code_graph_query(input, output);
    assert_mcp_symbol_search(input, output);
    assert_mcp_syntax_site_search(input, output);
    assert_mcp_outbound_sites(input, output);
    assert_mcp_scip_not_produced(input, output);
    assert_mcp_scip_relationship_trace_not_produced(input, output);
    assert_mcp_scip_symbol_resolve_not_produced(input, output);
}

fn assert_mcp_architecture_overview(input: &mut ChildStdin, output: &mut BufReader<ChildStdout>) {
    let overview = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 89,
            "method": "tools/call",
            "params": {
                "name": "architecture_overview",
                "arguments": {
                    "max_roots": 1,
                    "max_entry_point_candidates": 1,
                    "max_files": 1
                }
            }
        }),
    );
    assert_eq!(overview["result"]["isError"], serde_json::json!(false));
    let overview = &overview["result"]["structuredContent"];
    assert_eq!(overview["schema_version"], serde_json::json!(1));
    assert_eq!(overview["overview_profile"], serde_json::json!(1));
    assert_eq!(overview["total_files"], serde_json::json!(5));
    assert_eq!(overview["files_returned"], serde_json::json!(1));
    assert_eq!(overview["files_truncated"], serde_json::json!(true));
    assert_eq!(overview["total_source_roots"], serde_json::json!(1));
    assert_eq!(overview["source_roots_returned"], serde_json::json!(1));
    assert_eq!(overview["source_roots_truncated"], serde_json::json!(false));
    assert_eq!(overview["total_entry_point_candidates"], serde_json::json!(0));
    assert_eq!(
        overview["limitations"],
        serde_json::json!([
            "source_fact_aggregate_only_no_relationship_inference",
            "top_level_path_buckets_are_not_package_or_ownership_boundaries",
            "function_named_main_candidates_are_not_runtime_entry_point_proof"
        ])
    );
    assert_eq!(
        overview["source_roots"][0]["kind"],
        serde_json::json!("top_level_directory")
    );
    assert!(overview["source_roots"][0]["path"]
        .as_str()
        .is_some_and(|path| path.starts_with("rwp1:h:")));
    assert!(overview["kinds"].as_array().is_some_and(|kinds| !kinds.is_empty()));
}

fn assert_mcp_architecture_map(input: &mut ChildStdin, output: &mut BufReader<ChildStdout>) {
    let mapped = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 90,
            "method": "tools/call",
            "params": {
                "name": "architecture_map",
                "arguments": {"max_files": 1}
            }
        }),
    );
    assert_eq!(mapped["result"]["isError"], serde_json::json!(false));
    let mapped = &mapped["result"]["structuredContent"];
    assert_eq!(mapped["schema_version"], serde_json::json!(1));
    assert_eq!(mapped["map_profile"], serde_json::json!(1));
    assert_eq!(mapped["total_files"], serde_json::json!(5));
    assert_eq!(mapped["files_returned"], serde_json::json!(1));
    assert_eq!(mapped["truncated"], serde_json::json!(true));
    assert_eq!(
        mapped["limitation"],
        serde_json::json!("file_inventory_only_no_relationship_inference")
    );
    assert_eq!(
        mapped["languages"],
        serde_json::json!([
            {"language": "go", "files": 1, "declarations": 2},
            {"language": "python", "files": 1, "declarations": 2},
            {"language": "rust", "files": 1, "declarations": 3},
            {"language": "tsx", "files": 1, "declarations": 1},
            {"language": "typescript", "files": 1, "declarations": 1},
        ])
    );
    let files = mapped["files"].as_array().expect("architecture-map files");
    assert_eq!(files.len(), 1);
    assert!(files[0]["path"]
        .as_str()
        .is_some_and(|path| path.starts_with("rwp1:h:")));
}

fn assert_mcp_code_graph_query(input: &mut ChildStdin, output: &mut BufReader<ChildStdout>) {
    assert_mcp_code_graph_query_general_operations(input, output);
    assert_mcp_code_graph_query_relevant_paths(input, output);
    assert_mcp_code_graph_query_outbound_sites(input, output);
    assert_mcp_code_graph_query_syntax_site_search(input, output);
    assert_mcp_code_graph_query_test_markers(input, output);
}

fn assert_mcp_code_graph_query_general_operations(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
) {
    for (id, operation, assertions) in [
        (94, "symbols", serde_json::json!({"name": "Widget", "max_results": 1})),
        (95, "architecture", serde_json::json!({"max_roots": 1, "max_entry_point_candidates": 1, "max_files": 1})),
        (96, "files", serde_json::json!({"max_files": 1})),
    ] {
        let mut arguments = assertions;
        arguments["operation"] = serde_json::json!(operation);
        let response = mcp_request(
            input,
            output,
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {"name": "code_graph_query", "arguments": arguments}
            }),
        );
        assert_eq!(response["result"]["isError"], serde_json::json!(false));
        let content = &response["result"]["structuredContent"];
        assert_code_graph_query_envelope(content, operation);
        assert!(content["result"].is_object());
    }
}

fn assert_mcp_code_graph_query_relevant_paths(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
) {
    let response = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 100,
            "method": "tools/call",
            "params": {
                "name": "code_graph_query",
                "arguments": {
                    "operation": "relevant_paths",
                    "query": "Widget",
                    "max_paths": 5
                }
            }
        }),
    );
    assert_eq!(response["result"]["isError"], serde_json::json!(false));
    let content = &response["result"]["structuredContent"];
    assert_code_graph_query_envelope(content, "relevant_paths");
    assert_mcp_relevant_paths_receipt(&content["result"]);
}

fn assert_mcp_code_graph_query_outbound_sites(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
) {
    let searched = mcp_call_search(input, output, 97, "invoke");
    let searched = &searched["result"]["structuredContent"];
    let candidate = &searched["matches"][0];
    let response = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 98,
            "method": "tools/call",
            "params": {
                "name": "code_graph_query",
                "arguments": {
                    "operation": "outbound_sites",
                    "snapshot_sha256": searched["snapshot_sha256"],
                    "generation": searched["generation"],
                    "path": candidate["path"],
                    "content_sha256": candidate["content_sha256"],
                    "artifact_sha256": candidate["artifact_sha256"],
                    "fact_ordinal": candidate["fact_ordinal"],
                    "max_sites": 1
                }
            }
        }),
    );
    assert_eq!(response["result"]["isError"], serde_json::json!(false));
    let content = &response["result"]["structuredContent"];
    assert_code_graph_query_envelope(content, "outbound_sites");
    assert_eq!(content["result"]["outbound_sites_profile"], serde_json::json!(1));
}

fn assert_mcp_code_graph_query_syntax_site_search(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
) {
    let response = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 101,
            "method": "tools/call",
            "params": {
                "name": "code_graph_query",
                "arguments": {
                    "operation": "syntax_site_search",
                    "target": "run",
                    "max_sites": 5
                }
            }
        }),
    );
    assert_eq!(response["result"]["isError"], serde_json::json!(false));
    let content = &response["result"]["structuredContent"];
    assert_code_graph_query_envelope(content, "syntax_site_search");
    assert_mcp_syntax_site_search_receipt(&content["result"]);
}

fn assert_mcp_code_graph_query_test_markers(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
) {
    let response = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "tools/call",
            "params": {
                "name": "code_graph_query",
                "arguments": {
                    "operation": "test_markers",
                    "language": "rust",
                    "max_results": 1
                }
            }
        }),
    );
    assert_eq!(response["result"]["isError"], serde_json::json!(false));
    let content = &response["result"]["structuredContent"];
    assert_code_graph_query_envelope(content, "test_markers");
    let markers = &content["result"];
    assert_eq!(markers["schema_version"], serde_json::json!(1));
    assert_eq!(markers["test_markers_profile"], serde_json::json!(1));
    assert_eq!(markers["availability"], serde_json::json!("complete"));
    assert!(
        markers["language_coverage"]
            .as_array()
            .is_some_and(|coverage| coverage.len() == 1)
    );
    assert_eq!(
        markers["language_coverage"],
        serde_json::json!([
            {
                "language": "rust",
                "indexed_files": 1,
                "supported_files": 1,
                "unsupported_files": 0,
                "emitted_markers": 0,
            }
        ])
    );
    assert_eq!(
        markers["limitation"],
        serde_json::json!("raw_syntax_observations_only_not_test_execution_or_relationship_resolution")
    );
}

fn assert_code_graph_query_envelope(content: &serde_json::Value, operation: &str) {
    assert_eq!(content["schema_version"], serde_json::json!(1));
    assert_eq!(content["code_graph_query_profile"], serde_json::json!(1));
    assert_eq!(content["operation"], serde_json::json!(operation));
    assert!(
        content["output_bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes >= u64::try_from(content.to_string().len()).expect("JSON length"))
    );
}

fn assert_mcp_symbol_search(input: &mut ChildStdin, output: &mut BufReader<ChildStdout>) {
    let searched = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 91,
            "method": "tools/call",
            "params": {
                "name": "symbol_search",
                "arguments": {
                    "name": "Widget",
                    "match_mode": "exact",
                    "language": "rust",
                    "kind": "struct",
                    "path_prefix": "src",
                    "max_results": 5
                }
            }
        }),
    );
    assert_eq!(searched["result"]["isError"], serde_json::json!(false));
    let searched = &searched["result"]["structuredContent"];
    assert_eq!(searched["schema_version"], serde_json::json!(1));
    assert_eq!(searched["query_profile"], serde_json::json!(1));
    assert!(searched["connected_workspace"].as_str().is_some());
    assert!(searched["workspace_view"].as_i64().is_some_and(|view| view > 0));
    assert!(searched["source_slot"].as_str().is_some());
    assert_eq!(searched["match_mode"], serde_json::json!("exact"));
    assert_eq!(searched["matches_returned"], serde_json::json!(1));
    assert_eq!(searched["matches"][0]["name"], serde_json::json!("Widget"));
    assert_eq!(
        searched["limitations"],
        serde_json::json!([
            "direct_syntax_declarations_only",
            "no_name_based_relationship_resolution"
        ])
    );
}

fn assert_mcp_outbound_sites(input: &mut ChildStdin, output: &mut BufReader<ChildStdout>) {
    let searched = mcp_call_search(input, output, 92, "invoke");
    let searched = &searched["result"]["structuredContent"];
    assert_eq!(searched["matches_returned"], serde_json::json!(1));
    let candidate = &searched["matches"][0];
    let sites = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 93,
            "method": "tools/call",
            "params": {
                "name": "outbound_sites",
                "arguments": {
                    "snapshot_sha256": searched["snapshot_sha256"],
                    "generation": searched["generation"],
                    "path": candidate["path"],
                    "content_sha256": candidate["content_sha256"],
                    "artifact_sha256": candidate["artifact_sha256"],
                    "fact_ordinal": candidate["fact_ordinal"],
                    "max_sites": 5
                }
            }
        }),
    );
    assert_eq!(sites["result"]["isError"], serde_json::json!(false));
    let sites = &sites["result"]["structuredContent"];
    assert_eq!(sites["schema_version"], serde_json::json!(1));
    assert_eq!(sites["outbound_sites_profile"], serde_json::json!(1));
    assert_eq!(sites["availability"], serde_json::json!("complete"));
    assert_eq!(sites["selector"]["path"], candidate["path"]);
    assert_eq!(sites["declaration"]["language"], serde_json::json!("rust"));
    assert_eq!(
        sites["limitation"],
        serde_json::json!("raw_syntax_observations_only_no_target_resolution_or_inferred_edges")
    );
    assert!(sites["sites"].as_array().is_some_and(|records| {
        records.iter().any(|record| {
            record["kind"] == serde_json::json!("call")
                && record["raw_target"] == serde_json::json!("run")
                && record["target_resolution"]
                    == serde_json::json!("not_attempted_no_resolution_profile")
        })
    }));
}

fn assert_mcp_syntax_site_search(input: &mut ChildStdin, output: &mut BufReader<ChildStdout>) {
    let response = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 102,
            "method": "tools/call",
            "params": {
                "name": "syntax_site_search",
                "arguments": {"target": "run", "max_sites": 5}
            }
        }),
    );
    assert_eq!(response["result"]["isError"], serde_json::json!(false));
    assert_mcp_syntax_site_search_receipt(&response["result"]["structuredContent"]);
}

fn assert_mcp_syntax_site_search_receipt(result: &serde_json::Value) {
    assert_eq!(result["schema_version"], serde_json::json!(1));
    assert_eq!(result["syntax_site_search_profile"], serde_json::json!(1));
    assert_eq!(result["availability"], serde_json::json!("complete"));
    assert!(result["target_sha256"]
        .as_str()
        .is_some_and(|digest| digest.len() == 64));
    assert!(result["sites_returned"].as_u64().is_some_and(|count| count >= 1));
    assert!(result["sites_total"]
        .as_u64()
        .zip(result["sites_returned"].as_u64())
        .is_some_and(|(total, returned)| total >= returned));
    assert_eq!(
        result["truncated"],
        serde_json::json!(
            result["sites_returned"].as_u64() < result["sites_total"].as_u64()
        )
    );
    assert_eq!(
        result["limitation"],
        serde_json::json!(
            "exact_raw_target_syntax_observations_only_no_target_resolution_or_inferred_edges"
        )
    );
    assert!(result["sites"].as_array().is_some_and(|records| {
        records.iter().any(|record| {
            record["raw_target"] == serde_json::json!("run")
                && record["target_resolution"]
                    == serde_json::json!("not_attempted_no_resolution_profile")
        })
    }));
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

fn assert_mcp_scip_relationship_trace_not_produced(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
) {
    let response = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 18,
            "method": "tools/call",
            "params": {
                "name": "scip_relationship_trace",
                "arguments": {
                    "symbol": "scip-rust pkg 1 Widget#",
                    "direction": "outgoing"
                }
            }
        }),
    );
    assert_eq!(response["result"]["isError"], serde_json::json!(false));
    let content = &response["result"]["structuredContent"];
    assert_eq!(content["schema_version"], serde_json::json!(1));
    assert_eq!(content["resolution"], serde_json::json!("not_produced"));
    assert!(content["overlay"].is_null());
    assert!(content["package_scope_sha256"].is_null());
    assert_eq!(content["direction"], serde_json::json!("outgoing"));
    assert_eq!(content["max_depth"], serde_json::json!(2));
    assert_eq!(content["max_edges"], serde_json::json!(100));
    for field in [
        "visited_symbols",
        "unexpanded_frontier_symbols",
        "output_bytes",
    ] {
        assert_eq!(content[field], serde_json::json!(0));
    }
    for field in [
        "depth_limit_reached",
        "edge_limit_reached",
        "symbol_limit_reached",
        "output_limit_reached",
        "truncated",
    ] {
        assert_eq!(content[field], serde_json::json!(false));
    }
    assert!(content["edges"].as_array().is_some_and(Vec::is_empty));
}

fn assert_mcp_scip_symbol_resolve_not_produced(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
) {
    let searched = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 107,
            "method": "tools/call",
            "params": {
                "name": "symbol_search",
                "arguments": {
                    "name": "Widget",
                    "match_mode": "exact",
                    "language": "rust",
                    "kind": "struct"
                }
            }
        }),
    );
    let searched = &searched["result"]["structuredContent"];
    let candidate = &searched["matches"][0];
    let response = mcp_request(
        input,
        output,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 108,
            "method": "tools/call",
            "params": {
                "name": "scip_symbol_resolve",
                "arguments": {
                    "snapshot_sha256": searched["snapshot_sha256"],
                    "generation": searched["generation"],
                    "path": candidate["path"],
                    "content_sha256": candidate["content_sha256"],
                    "artifact_sha256": candidate["artifact_sha256"],
                    "fact_ordinal": candidate["fact_ordinal"],
                    "name_span": candidate["name_span"],
                    "workspace_view": searched["workspace_view"]
                }
            }
        }),
    );
    assert_eq!(response["result"]["isError"], serde_json::json!(false));
    let content = &response["result"]["structuredContent"];
    assert_eq!(content["schema_version"], serde_json::json!(1));
    assert_eq!(content["resolution"], serde_json::json!("not_produced"));
    assert!(content["symbol"].is_null());
    assert!(content["workspace_view"].as_i64().is_some_and(|view| view > 0));
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
