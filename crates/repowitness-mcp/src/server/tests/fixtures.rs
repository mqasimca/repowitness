use super::*;
use crate::{
    ARCHITECTURE_OVERVIEW_LIMITATIONS, ArchitectureMapOutput, ArchitectureOverviewOutput,
    GraphArchitectureOutput, GraphEvidenceOutput, GraphImpactOutput, GraphReadServiceOutput,
    GraphReadServiceRequest, GraphSearchOutput, GraphStatusOutput, GraphTraceOutput,
    McpArchitectureMapFile, McpArchitectureMapLanguage, McpArchitectureOverviewKind,
    McpArchitectureOverviewRoot, McpConfigurationIdentity, McpDiagnosticsMemoryProjection,
    McpGraphContext, McpGraphPublication, McpGraphTrace, McpGraphTraceCoverage,
    McpGraphTraceTruncation, McpPhase2ContextScope, McpRelevantPath, McpRepositoryTopologyCategory,
    McpRepositoryTopologyCoverage, McpRepositoryTopologyEntry, Phase2ContextBuildOutput,
    RelevantPathsOutput, RepositoryTopologyOutput, SyntaxSiteSearchOutput,
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

pub(super) fn relevant_paths_output() -> RelevantPathsOutput {
    let search = search_output();
    RelevantPathsOutput {
        schema_version: 1,
        path_ranking_profile: 1,
        snapshot_sha256: search.snapshot_sha256,
        generation: search.generation,
        resolution: search.resolution,
        query_sha256: search.query_sha256,
        matches_returned: search.matches_returned,
        // A response may have candidate truncation even when every path in its
        // returned-match surface fits the requested path bound.
        matches_total: 2,
        paths_returned: 1,
        returned_match_paths_total: 1,
        returned_match_paths_truncated: false,
        coverage: McpCoverage {
            truncated: 1,
            ..search.coverage
        },
        limitations: vec![
            "indexed_supported_language_declaration_lexical_only".to_owned(),
            "ordered_by_returned_match_count_then_canonical_path".to_owned(),
            "path_summaries_cover_only_returned_declaration_matches".to_owned(),
            "no_relationship_or_semantic_relevance_claim".to_owned(),
        ],
        paths: vec![McpRelevantPath {
            path: "rwp1:h:7372632F6C69622E7273".to_owned(),
            content_sha256: "22".repeat(32),
            matching_declarations: 1,
            first_fact_ordinal: 7,
        }],
        matches: search.matches,
    }
}

pub(super) fn syntax_site_search_output() -> SyntaxSiteSearchOutput {
    SyntaxSiteSearchOutput {
        schema_version: 1,
        syntax_site_search_profile: 1,
        target_sha256: "44".repeat(32),
        snapshot_sha256: "11".repeat(32),
        generation: 9,
        availability: "complete".to_owned(),
        coverage: coverage(),
        sites_returned: 0,
        sites_total: 0,
        truncated: false,
        output_bytes: 0,
        limitation:
            "exact_raw_target_syntax_observations_only_no_target_resolution_or_inferred_edges"
                .to_owned(),
        sites: Vec::new(),
    }
}

pub(super) fn symbol_search_output() -> SymbolSearchOutput {
    let search = search_output();
    SymbolSearchOutput {
        schema_version: 1,
        query_profile: 1,
        connected_workspace: format!("cwi1:h:{}", "AA".repeat(32)),
        workspace_view: 1,
        source_slot: format!("ssi1:h:{}", "BB".repeat(32)),
        snapshot_sha256: search.snapshot_sha256,
        generation: search.generation,
        resolution: search.resolution,
        query_sha256: search.query_sha256,
        match_mode: "exact".to_owned(),
        matches_returned: search.matches_returned,
        matches_total: search.matches_total,
        coverage: search.coverage,
        limitations: vec![
            "direct_syntax_declarations_only".to_owned(),
            "no_name_based_relationship_resolution".to_owned(),
        ],
        matches: search.matches,
    }
}

pub(super) fn architecture_map_output() -> ArchitectureMapOutput {
    ArchitectureMapOutput {
        schema_version: 1,
        map_profile: 1,
        snapshot_sha256: "11".repeat(32),
        generation: 9,
        coverage: coverage(),
        total_files: 2,
        total_declarations: 3,
        files_returned: 1,
        truncated: true,
        output_bytes: 200,
        limitation: "file_inventory_only_no_relationship_inference".to_owned(),
        languages: vec![
            McpArchitectureMapLanguage {
                language: "go".to_owned(),
                files: 1,
                declarations: 1,
            },
            McpArchitectureMapLanguage {
                language: "rust".to_owned(),
                files: 1,
                declarations: 2,
            },
        ],
        files: vec![McpArchitectureMapFile {
            path: "rwp1:h:7372632F6C69622E7273".to_owned(),
            language: "rust".to_owned(),
            content_sha256: "22".repeat(32),
            artifact_sha256: "33".repeat(32),
            producer_manifest_sha256: "55".repeat(32),
            declaration_count: 2,
        }],
    }
}

pub(super) fn architecture_overview_output() -> ArchitectureOverviewOutput {
    let mut entry_point_candidate = search_output()
        .matches
        .into_iter()
        .next()
        .expect("fixture search match");
    entry_point_candidate.name = "main".to_owned();
    entry_point_candidate.qualified_name = "fixture::main".to_owned();
    let file = architecture_map_output()
        .files
        .into_iter()
        .next()
        .expect("fixture architecture file");
    ArchitectureOverviewOutput {
        schema_version: 1,
        overview_profile: 1,
        snapshot_sha256: "11".repeat(32),
        generation: 9,
        source_producer_manifest_sha256: "55".repeat(32),
        coverage: coverage(),
        total_files: 2,
        total_declarations: 3,
        total_source_roots: 2,
        source_roots_returned: 1,
        source_roots_truncated: true,
        total_entry_point_candidates: 1,
        entry_point_candidates_returned: 1,
        entry_point_candidates_truncated: false,
        files_returned: 1,
        files_truncated: true,
        output_bytes: 512,
        limitations: ARCHITECTURE_OVERVIEW_LIMITATIONS
            .iter()
            .map(|limitation| (*limitation).to_owned())
            .collect(),
        languages: vec![
            McpArchitectureMapLanguage {
                language: "go".to_owned(),
                files: 1,
                declarations: 1,
            },
            McpArchitectureMapLanguage {
                language: "rust".to_owned(),
                files: 1,
                declarations: 2,
            },
        ],
        kinds: vec![
            McpArchitectureOverviewKind {
                language: "go".to_owned(),
                kind: "function".to_owned(),
                declarations: 1,
            },
            McpArchitectureOverviewKind {
                language: "rust".to_owned(),
                kind: "function".to_owned(),
                declarations: 2,
            },
        ],
        source_roots: vec![McpArchitectureOverviewRoot {
            kind: "top_level_directory".to_owned(),
            path: Some("rwp1:h:737263".to_owned()),
            files: 1,
            declarations: 2,
        }],
        entry_point_candidates: vec![entry_point_candidate],
        files: vec![file],
    }
}

pub(super) fn repository_topology_output() -> RepositoryTopologyOutput {
    RepositoryTopologyOutput {
        schema_version: 1,
        topology_profile: 1,
        snapshot_sha256: "11".repeat(32),
        generation: 9,
        topology_sha256: "22".repeat(32),
        coverage: McpRepositoryTopologyCoverage {
            discovered_paths: 2,
            omitted_paths: 0,
        },
        total_paths: 2,
        paths_returned: 2,
        truncated: false,
        output_bytes: 512,
        limitation: "inventory_only_no_semantic_relationship_inference".to_owned(),
        categories: vec![
            McpRepositoryTopologyCategory {
                category: "agent_instruction".to_owned(),
                paths: 0,
            },
            McpRepositoryTopologyCategory {
                category: "build_descriptor".to_owned(),
                paths: 0,
            },
            McpRepositoryTopologyCategory {
                category: "configuration_descriptor".to_owned(),
                paths: 0,
            },
            McpRepositoryTopologyCategory {
                category: "documentation".to_owned(),
                paths: 1,
            },
            McpRepositoryTopologyCategory {
                category: "other_tracked_file".to_owned(),
                paths: 1,
            },
            McpRepositoryTopologyCategory {
                category: "package_descriptor".to_owned(),
                paths: 0,
            },
            McpRepositoryTopologyCategory {
                category: "workflow_descriptor".to_owned(),
                paths: 0,
            },
        ],
        entries: vec![
            McpRepositoryTopologyEntry {
                path: "rwp1:h:524541444D452E6D64".to_owned(),
                category: "documentation".to_owned(),
            },
            McpRepositoryTopologyEntry {
                path: "rwp1:h:7372632F6C69622E7273".to_owned(),
                category: "other_tracked_file".to_owned(),
            },
        ],
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

pub(super) fn scip_relationship_trace_output() -> ScipRelationshipTraceOutput {
    ScipRelationshipTraceOutput {
        schema_version: 1,
        connected_workspace: "cwi1:h:00".to_owned(),
        workspace_view: 1,
        source_slot: "ssi1:h:00".to_owned(),
        resolution: "not_produced".to_owned(),
        overlay: None,
        package_scope_sha256: None,
        direction: "outgoing".to_owned(),
        max_depth: 2,
        max_edges: 8,
        visited_symbols: 0,
        unexpanded_frontier_symbols: 0,
        depth_limit_reached: false,
        edge_limit_reached: false,
        symbol_limit_reached: false,
        output_limit_reached: false,
        truncated: false,
        output_bytes: 0,
        edges: Vec::new(),
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
