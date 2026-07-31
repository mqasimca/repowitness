use serde_json::{Value, json};

use super::*;

pub(super) const fn native_tool_names() -> [&'static str; 13] {
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
        MEMORY_RECALL_TOOL_NAME,
        PHASE2_CONTEXT_BUILD_TOOL_NAME,
        SCIP_EVIDENCE_TOOL_NAME,
        SYMBOL_GET_TOOL_NAME,
    ]
}

pub(super) fn tool_requests() -> [(&'static str, Value); 6] {
    [
        (GRAPH_STATUS_TOOL_NAME, json!({})),
        (GRAPH_SEARCH_TOOL_NAME, json!({"query": "run"})),
        (GRAPH_EVIDENCE_TOOL_NAME, json!({"site": graph_site_json()})),
        (GRAPH_ARCHITECTURE_TOOL_NAME, json!({})),
        (
            GRAPH_TRACE_TOOL_NAME,
            json!({
                "start": {"type": "definition", "definition": graph_definition_json()},
                "direction": "outbound",
                "edge_kinds": ["call"],
            }),
        ),
        (
            IMPACT_ANALYZE_TOOL_NAME,
            json!({
                "start": graph_definition_json(),
                "edge_kinds": ["call"],
            }),
        ),
    ]
}
