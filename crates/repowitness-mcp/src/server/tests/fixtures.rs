use super::*;
use crate::{
    GraphArchitectureOutput, GraphEvidenceOutput, GraphImpactOutput, GraphReadServiceOutput,
    GraphReadServiceRequest, GraphSearchOutput, GraphStatusOutput, GraphTraceOutput,
    McpConfigurationIdentity, McpDiagnosticsMemoryProjection, McpGraphContext, McpGraphPublication,
    McpGraphTrace, McpGraphTraceCoverage, McpGraphTraceTruncation, McpPhase2ContextScope,
    Phase2ContextBuildOutput,
};
use rmcp::model::JsonObject;

pub(super) fn coverage() -> McpCoverage {
    McpCoverage {
        searched: 1,
        skipped: 0,
        unresolved: 0,
        truncated: 0,
    }
}

pub(super) fn json_object(value: serde_json::Value) -> JsonObject {
    value.as_object().expect("fixture is an object").clone()
}

pub(super) fn search_output() -> CodeSearchOutput {
    CodeSearchOutput {
        schema_version: 3,
        query_profile: 3,
        snapshot_sha256: "11".repeat(32),
        generation: 9,
        resolution: "confirmed".to_owned(),
        query_sha256: "44".repeat(32),
        matches_returned: 1,
        matches_total: 1,
        coverage: coverage(),
        limitation: "supported_language_symbol_lexical_only".to_owned(),
        matches: vec![McpSearchMatch {
            path: "rwp1:h:7372632F6C69622E7273".to_owned(),
            fact_ordinal: 7,
            content_sha256: "22".repeat(32),
            artifact_sha256: "33".repeat(32),
            producer_manifest_sha256: "55".repeat(32),
            evidence_tier: "syntax".to_owned(),
            language: "rust".to_owned(),
            kind: "function".to_owned(),
            name: "run".to_owned(),
            qualified_name: "fixture::run".to_owned(),
            name_span: McpSpan { start: 7, end: 10 },
            declaration_span: McpSpan { start: 0, end: 13 },
        }],
    }
}

pub(super) fn scip_evidence_output() -> ScipEvidenceOutput {
    ScipEvidenceOutput {
        schema_version: 1,
        connected_workspace: "cwi1:h:00".to_owned(),
        workspace_view: 1,
        source_slot: "ssi1:h:00".to_owned(),
        resolution: "not_produced".to_owned(),
        overlay: None,
        package_scope_sha256: None,
        occurrences_truncated: false,
        relationships_truncated: false,
        output_bytes: 0,
        occurrences: Vec::new(),
        relationships: Vec::new(),
    }
}

pub(super) fn memory_output() -> MemoryRecallOutput {
    MemoryRecallOutput {
        schema_version: 1,
        recall_profile: 1,
        query_sha256: None,
        snapshot_sha256: "11".repeat(32),
        generation: 9,
        projection: 4,
        source_epoch: 2,
        target: McpMemoryTarget {
            kind: "worktree".to_owned(),
            source_snapshot_sha256: Some("11".repeat(32)),
            commit_object_format: Some("sha256".to_owned()),
            commit_hex: Some("22".repeat(32)),
        },
        producer: McpMemoryProducer {
            id: "rust-correspondence-v1".to_owned(),
            version: 1,
            profile_sha256: "44".repeat(32),
        },
        matches_returned: 0,
        matches_total: 0,
        matches_omitted: 0,
        coverage: empty_memory_coverage(),
        limitation: "rust_symbol_memory_only".to_owned(),
        records: Vec::new(),
    }
}

pub(super) fn context_output() -> ContextBuildOutput {
    ContextBuildOutput {
        schema_version: 2,
        context_profile: 1,
        reciprocal_rank_k: 60,
        budget_estimator: "utf8_bytes_upper_bound_v1".to_owned(),
        budget_units: 4096,
        used_units: 0,
        query_sha256: "44".repeat(32),
        snapshot_sha256: "11".repeat(32),
        generation: 9,
        memory: None,
        coverage: McpContextCoverage {
            source_index: coverage(),
            source_total_matches: 0,
            source_returned_matches: 0,
            source_expansion_omitted: 0,
            source_budget_omitted: 0,
            source_included: 0,
            memory_total_matches: 0,
            memory_returned_matches: 0,
            memory_non_current_omitted: 0,
            memory_budget_omitted: 0,
            memory_included: 0,
        },
        omissions: Vec::new(),
        items: Vec::new(),
    }
}

pub(super) fn phase2_context_output() -> Phase2ContextBuildOutput {
    Phase2ContextBuildOutput {
        schema_version: 1,
        profile_id: "phase2-evidence-balanced-v1".to_owned(),
        profile_version: 1,
        budget_estimator: "utf8_bytes_upper_bound_v1".to_owned(),
        budget_units: 4096,
        used_units: 0,
        scope: McpPhase2ContextScope {
            repository_sha256: "11".repeat(32),
            connected_workspace_sha256: "22".repeat(32),
            workspace_view: 1,
            source_slot_sha256: "33".repeat(32),
            source_epoch: 2,
            generation: 9,
            snapshot_sha256: "44".repeat(32),
            manifest_sha256: "55".repeat(32),
        },
        provider_coverage: Vec::new(),
        omissions: Vec::new(),
        items: Vec::new(),
    }
}

pub(super) fn diagnostics_output() -> DiagnosticsOutput {
    DiagnosticsOutput {
        schema_version: 3,
        diagnostics_profile: 3,
        configuration: McpConfigurationIdentity {
            digest_sha256: "66".repeat(32),
            schema_version: 1,
            resolver_version: 1,
            profile: "local".to_owned(),
        },
        snapshot_sha256: "11".repeat(32),
        generation: 9,
        source_epoch: 2,
        producer_manifest_sha256: "55".repeat(32),
        index_coverage: coverage(),
        syntax_error_nodes: 4,
        known_parser_limitation_nodes: 1,
        memory_projection: Some(McpDiagnosticsMemoryProjection {
            projection: 4,
            source_epoch: 2,
            snapshot_sha256: "11".repeat(32),
            coverage: empty_memory_coverage(),
        }),
        supported_languages: vec![
            "rust".to_owned(),
            "go".to_owned(),
            "typescript".to_owned(),
            "tsx".to_owned(),
            "python".to_owned(),
        ],
        capabilities: vec!["lexical_source_search".to_owned()],
        limitations: vec!["rust_graph_syntax_derived_only".to_owned()],
    }
}

pub(super) fn symbol_output() -> SymbolGetOutput {
    SymbolGetOutput {
        schema_version: 4,
        symbol_profile: 3,
        snapshot_sha256: "11".repeat(32),
        generation: 9,
        resolution: "confirmed".to_owned(),
        selector: SymbolSelectorOutput {
            path: "rwp1:h:7372632F6C69622E7273".to_owned(),
            content_sha256: "22".repeat(32),
            artifact_sha256: "33".repeat(32),
            fact_ordinal: 7,
        },
        coverage: coverage(),
        limitation: "references_not_implemented".to_owned(),
        symbol: Some(McpSymbol {
            producer_manifest_sha256: "55".repeat(32),
            evidence_tier: "syntax".to_owned(),
            language: "rust".to_owned(),
            kind: "function".to_owned(),
            name: "run".to_owned(),
            qualified_name: "fixture::run".to_owned(),
            name_span: McpSpan { start: 7, end: 10 },
            declaration_span: McpSpan { start: 0, end: 13 },
            declaration_encoding: "utf8".to_owned(),
            declaration: "pub fn run() {}".to_owned(),
        }),
    }
}

pub(super) fn graph_output(request: GraphReadServiceRequest) -> GraphReadServiceOutput {
    use repowitness_application::RustGraphReadOperation;

    let operation = request.into_operation();
    match operation {
        RustGraphReadOperation::Status => GraphReadServiceOutput::Status(GraphStatusOutput {
            schema_version: 1,
            context: graph_context(),
            availability: "complete".to_owned(),
        }),
        RustGraphReadOperation::Search { .. } => {
            GraphReadServiceOutput::Search(GraphSearchOutput {
                schema_version: 1,
                context: graph_context(),
                matches_returned: 0,
                matches_total: 0,
                truncated: false,
                output_bytes: 0,
                definitions: Vec::new(),
            })
        }
        RustGraphReadOperation::Evidence { .. } => {
            GraphReadServiceOutput::Evidence(GraphEvidenceOutput {
                schema_version: 1,
                context: graph_context(),
                found: false,
                evidence: None,
            })
        }
        RustGraphReadOperation::Architecture { .. } => {
            GraphReadServiceOutput::Architecture(GraphArchitectureOutput {
                schema_version: 1,
                context: graph_context(),
                definitions_by_kind: Vec::new(),
                edges_by_kind: Vec::new(),
            })
        }
        RustGraphReadOperation::Trace { .. } => GraphReadServiceOutput::Trace(GraphTraceOutput {
            schema_version: 1,
            context: graph_context(),
            trace: empty_graph_trace(),
        }),
        RustGraphReadOperation::Impact { .. } => {
            GraphReadServiceOutput::Impact(GraphImpactOutput {
                schema_version: 1,
                context: graph_context(),
                trace: empty_graph_trace(),
                impacts: Vec::new(),
                unknown_coverage: false,
                output_bytes: 0,
            })
        }
    }
}

pub(super) fn graph_definition_json() -> serde_json::Value {
    serde_json::json!({
        "source_slot": format!("ssi1:h:{}", "11".repeat(32).to_uppercase()),
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

pub(super) fn graph_site_json() -> serde_json::Value {
    serde_json::json!({
        "source_slot": format!("ssi1:h:{}", "11".repeat(32).to_uppercase()),
        "path": "rwp1:h:7372632F6C69622E7273",
        "artifact_sha256": "33".repeat(32),
        "ordinal": 1,
        "site_kind": "call",
        "occurrence_span": {"start": 0, "end": 13},
        "target_span": {"start": 7, "end": 10},
    })
}

fn graph_context() -> McpGraphContext {
    McpGraphContext {
        connected_workspace: format!("cwi1:h:{}", "11".repeat(32).to_uppercase()),
        workspace_view: 4,
        graph_generation: 9,
        publication: Some(McpGraphPublication {
            resolver_profile: 1,
            input_sha256: "44".repeat(32),
            output_sha256: "55".repeat(32),
            source_count: 1,
            artifact_count: 1,
            definition_count: 1,
            site_count: 0,
            unresolved_count: 0,
            unique_count: 0,
            ambiguous_count: 0,
            unsupported_count: 0,
            truncated_site_count: 0,
            retained_candidate_count: 0,
            edge_count: 0,
            input_text_bytes: 0,
            output_bytes: 0,
            syntax_error_nodes: 0,
            macro_sites: 0,
            test_marker_sites: 0,
            heuristic_sites: 0,
        }),
    }
}

fn empty_graph_trace() -> McpGraphTrace {
    McpGraphTrace {
        edges: Vec::new(),
        visited_nodes: 1,
        visited_edges: 0,
        maximum_completed_depth: 0,
        truncation: McpGraphTraceTruncation {
            depth: false,
            visited_nodes: false,
            visited_edges: false,
            frontier: false,
            results: false,
        },
        coverage: McpGraphTraceCoverage {
            unresolved_sites: 0,
            unsupported_sites: 0,
            ambiguous_sites: 0,
            truncated_sites: 0,
            unlinked_sites: 0,
            macro_sites: 0,
            conditional_sites: 0,
            heuristic_sites: 0,
        },
        input_bytes: 0,
        output_bytes: 0,
    }
}

fn empty_memory_coverage() -> McpMemoryCoverage {
    McpMemoryCoverage {
        searched: 0,
        skipped: 0,
        unresolved: 0,
        truncated: 0,
        total: 0,
        current: 0,
        not_applicable: 0,
        stale: 0,
        needs_review: 0,
        indeterminate: 0,
        conflicted: 0,
        contradicted: 0,
        superseded: 0,
        quarantined: 0,
        tombstoned: 0,
    }
}
