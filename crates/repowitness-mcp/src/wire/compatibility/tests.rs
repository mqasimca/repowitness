use repowitness_application::RustGraphReadOperation;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::*;

fn definition() -> Value {
    json!({
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

fn rejects<T: DeserializeOwned>(value: Value) {
    assert!(serde_json::from_value::<T>(value).is_err());
}

#[test]
fn alias_inputs_reject_unknown_wrong_type_and_pagination_fields() {
    rejects::<SearchCodeInput>(json!({"query": "run", "repository": "/private"}));
    rejects::<GetCodeSnippetInput>(json!({"generation": "nine"}));
    rejects::<SearchGraphInput>(json!({"query": "run", "cursor": "opaque"}));
    rejects::<TracePathInput>(json!({
        "start": {"type": "definition", "definition": definition()},
        "direction": "outbound",
        "edge_kinds": ["call"],
        "page": 2,
    }));
    rejects::<GetGraphSchemaInput>(json!({"continuation_token": "opaque"}));
    rejects::<GetArchitectureInput>(json!({"project": "private"}));
    rejects::<IndexStatusInput>(json!({"include_tasks": true}));
}

#[test]
fn search_and_exact_retrieval_aliases_share_native_validation() {
    let search: SearchCodeInput =
        serde_json::from_value(json!({"query": "  alpha   beta  ", "max_results": 7}))
            .expect("wire shape");
    let request = search.validate().expect("shared search subset");
    assert_eq!(request.query(), "alpha beta");
    assert_eq!(request.max_results(), 7);

    for value in [
        json!({"query": ""}),
        json!({"query": "run", "max_results": 0}),
        json!({"query": "run", "max_results": 101}),
        json!({"query": "run", "timeout_ms": 30_001}),
    ] {
        let input: SearchCodeInput = serde_json::from_value(value).expect("wire shape");
        assert!(input.validate().is_err());
    }

    let snippet: GetCodeSnippetInput = serde_json::from_value(json!({
        "snapshot_sha256": "11".repeat(32),
        "generation": 9,
        "path": "rwp1:h:7372632F6C69622E7273",
        "content_sha256": "22".repeat(32),
        "artifact_sha256": "33".repeat(32),
        "fact_ordinal": 7,
    }))
    .expect("wire shape");
    let request = snippet.validate().expect("shared exact selector");
    assert_eq!(request.generation(), 9);
    assert_eq!(request.fact_ordinal(), 7);
}

#[test]
fn graph_aliases_are_exact_pinned_and_depth_five_bounded() {
    let search: SearchGraphInput = serde_json::from_value(json!({
        "workspace_view": 4,
        "graph_generation": 9,
        "query": "run",
        "max_results": 7,
    }))
    .expect("wire shape");
    let request = search.validate().expect("shared graph search subset");
    assert_eq!(request.exact_pin(), Some((4, 9)));
    assert!(matches!(
        request.into_operation(),
        RustGraphReadOperation::Search { .. }
    ));

    for depth in [0, 6] {
        let trace: TracePathInput = serde_json::from_value(json!({
            "start": {"type": "definition", "definition": definition()},
            "direction": "outbound",
            "edge_kinds": ["call"],
            "max_depth": depth,
        }))
        .expect("wire shape");
        assert!(trace.validate().is_err());
    }

    let trace: TracePathInput = serde_json::from_value(json!({
        "start": {"type": "definition", "definition": definition()},
        "direction": "inbound",
        "edge_kinds": ["call", "reference"],
    }))
    .expect("wire shape");
    assert!(matches!(
        trace
            .validate()
            .expect("default bounded trace")
            .into_operation(),
        RustGraphReadOperation::Trace { .. }
    ));
}

#[test]
fn compatibility_debug_and_receipts_do_not_expose_untrusted_text() {
    let query_canary = "private_customer_query_canary";
    let search: SearchGraphInput =
        serde_json::from_value(json!({"query": query_canary})).expect("wire shape");
    let debug = format!("{search:?}");
    assert!(!debug.contains(query_canary));

    let path_canary = "rwp1:h:707269766174655F706174685F63616E617279";
    let snippet: GetCodeSnippetInput = serde_json::from_value(json!({
        "snapshot_sha256": "11".repeat(32),
        "generation": 9,
        "path": path_canary,
        "content_sha256": "22".repeat(32),
        "artifact_sha256": "33".repeat(32),
        "fact_ordinal": 7,
    }))
    .expect("wire shape");
    assert!(!format!("{snippet:?}").contains(path_canary));

    let output = compatibility_output(CompatibilityAlias::SearchCode, json!({"coverage": 7}));
    let receipt = &output.repowitness.receipt;
    assert_eq!(receipt.schema_version, COMPATIBILITY_PROFILE_VERSION);
    assert_eq!(receipt.profile, INCUMBENT_COMPATIBLE_PROFILE);
    assert_eq!(receipt.surface, INCUMBENT_COMPATIBLE_SURFACE);
    assert_eq!(receipt.alias, SEARCH_CODE_ALIAS_TOOL_NAME);
    assert_eq!(receipt.canonical_tool, super::super::CODE_SEARCH_TOOL_NAME);
    assert_eq!(receipt.compatibility.name, "compatible");
    assert_eq!(receipt.compatibility.request, "subset");
    assert_eq!(receipt.compatibility.response, "extended");
    assert_eq!(receipt.compatibility.behavior, "not_assessed");
    let encoded = serde_json::to_string(&receipt).expect("receipt serializes");
    assert!(!encoded.contains(query_canary));
    assert!(!encoded.contains(path_canary));
}
