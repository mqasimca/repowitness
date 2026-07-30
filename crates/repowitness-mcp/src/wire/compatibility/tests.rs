use repowitness_application::RustGraphReadOperation;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityObservationFixture {
    schema_version: u16,
    observed_on: String,
    public_source_url: String,
    release: String,
    revision: String,
    license: String,
    darwin_arm64_archive_sha256: String,
    observation: String,
    aliases: Vec<CompatibilityAliasFixture>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityAliasFixture {
    alias: String,
    incumbent_required_request_fields: Vec<String>,
    repowitness_request: Value,
    expected_compatibility: ExpectedCompatibility,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedCompatibility {
    name: String,
    request: String,
    response: String,
    behavior: String,
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
    assert_eq!(receipt.compatibility.request, "incompatible");
    assert_eq!(receipt.compatibility.response, "not_assessed");
    assert_eq!(receipt.compatibility.behavior, "not_assessed");
    let encoded = serde_json::to_string(&receipt).expect("receipt serializes");
    assert!(!encoded.contains(query_canary));
    assert!(!encoded.contains(path_canary));
}

#[test]
fn pinned_public_observation_keeps_every_compatibility_claim_conservative() {
    let fixture: CompatibilityObservationFixture =
        serde_json::from_str(include_str!("fixtures/codebase-memory-mcp-v0.9.0.json"))
            .expect("independently authored observation fixture");
    assert_eq!(fixture.schema_version, COMPATIBILITY_PROFILE_VERSION);
    assert_eq!(fixture.aliases.len(), CompatibilityAlias::ALL.len());
    assert_eq!(fixture.release, "v0.9.0");
    assert_eq!(fixture.license, "MIT");
    assert_eq!(fixture.revision.len(), 40);
    assert!(
        fixture
            .revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    );
    assert_eq!(fixture.darwin_arm64_archive_sha256.len(), 64);
    assert!(
        fixture
            .darwin_arm64_archive_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    );
    assert!(fixture.public_source_url.starts_with("https://github.com/"));
    assert!(fixture.observation.contains("No upstream source"));

    for (alias, expected) in CompatibilityAlias::ALL.into_iter().zip(fixture.aliases) {
        assert_local_request_valid(&expected.alias, expected.repowitness_request);
        assert_incumbent_minimum_is_not_a_local_request(
            &expected.alias,
            &expected.incumbent_required_request_fields,
        );

        let receipt = compatibility_output(alias, Value::Null).repowitness.receipt;
        assert_eq!(receipt.alias, expected.alias);
        assert_eq!(
            receipt.compatibility.name,
            expected.expected_compatibility.name
        );
        assert_eq!(
            receipt.compatibility.request,
            expected.expected_compatibility.request
        );
        assert_eq!(
            receipt.compatibility.response,
            expected.expected_compatibility.response
        );
        assert_eq!(
            receipt.compatibility.behavior,
            expected.expected_compatibility.behavior
        );
        assert_eq!(receipt.observation.observed_on, fixture.observed_on);
        assert_eq!(
            receipt.observation.public_source_url,
            fixture.public_source_url
        );
        assert_eq!(receipt.observation.release, fixture.release);
        assert_eq!(receipt.observation.revision, fixture.revision);
        assert_eq!(receipt.observation.license, fixture.license);
        assert_eq!(
            receipt.observation.observed_artifact_sha256,
            fixture.darwin_arm64_archive_sha256
        );
        assert_eq!(
            receipt.observation.provenance,
            "independently_authored_public_protocol_observation"
        );
    }
}

fn assert_local_request_valid(alias: &str, request: Value) {
    match alias {
        GET_ARCHITECTURE_ALIAS_TOOL_NAME => {
            serde_json::from_value::<GetArchitectureInput>(request)
                .expect("local request")
                .validate()
                .expect("valid local request");
        }
        GET_CODE_SNIPPET_ALIAS_TOOL_NAME => {
            serde_json::from_value::<GetCodeSnippetInput>(request)
                .expect("local request")
                .validate()
                .expect("valid local request");
        }
        GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME => {
            serde_json::from_value::<GetGraphSchemaInput>(request)
                .expect("local request")
                .validate()
                .expect("valid local request");
        }
        INDEX_STATUS_ALIAS_TOOL_NAME => {
            serde_json::from_value::<IndexStatusInput>(request)
                .expect("local request")
                .validate()
                .expect("valid local request");
        }
        SEARCH_CODE_ALIAS_TOOL_NAME => {
            serde_json::from_value::<SearchCodeInput>(request)
                .expect("local request")
                .validate()
                .expect("valid local request");
        }
        SEARCH_GRAPH_ALIAS_TOOL_NAME => {
            serde_json::from_value::<SearchGraphInput>(request)
                .expect("local request")
                .validate()
                .expect("valid local request");
        }
        TRACE_PATH_ALIAS_TOOL_NAME => {
            serde_json::from_value::<TracePathInput>(request)
                .expect("local request")
                .validate()
                .expect("valid local request");
        }
        _ => panic!("unexpected compatibility alias"),
    }
}

fn assert_incumbent_minimum_is_not_a_local_request(alias: &str, required: &[String]) {
    assert!(
        !required.is_empty(),
        "incumbent request must select a project"
    );
    let mut request = Map::new();
    for field in required {
        let value = match field.as_str() {
            "project" => Value::String("fixture-project".to_owned()),
            "pattern" => Value::String("run".to_owned()),
            "qualified_name" | "function_name" => Value::String("fixture::run".to_owned()),
            _ => panic!("unexpected incumbent required field"),
        };
        assert!(request.insert(field.clone(), value).is_none());
    }
    let request = Value::Object(request);
    let rejected = match alias {
        GET_ARCHITECTURE_ALIAS_TOOL_NAME => {
            serde_json::from_value::<GetArchitectureInput>(request).is_err()
        }
        GET_CODE_SNIPPET_ALIAS_TOOL_NAME => {
            serde_json::from_value::<GetCodeSnippetInput>(request).is_err()
        }
        GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME => {
            serde_json::from_value::<GetGraphSchemaInput>(request).is_err()
        }
        INDEX_STATUS_ALIAS_TOOL_NAME => {
            serde_json::from_value::<IndexStatusInput>(request).is_err()
        }
        SEARCH_CODE_ALIAS_TOOL_NAME => serde_json::from_value::<SearchCodeInput>(request).is_err(),
        SEARCH_GRAPH_ALIAS_TOOL_NAME => {
            serde_json::from_value::<SearchGraphInput>(request).is_err()
        }
        TRACE_PATH_ALIAS_TOOL_NAME => serde_json::from_value::<TracePathInput>(request).is_err(),
        _ => panic!("unexpected compatibility alias"),
    };
    assert!(rejected, "{alias} must not claim request compatibility");
}
