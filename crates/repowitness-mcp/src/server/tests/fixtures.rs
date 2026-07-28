use super::*;
use crate::McpDiagnosticsMemoryProjection;

pub(super) fn coverage() -> McpCoverage {
    McpCoverage {
        searched: 1,
        skipped: 0,
        unresolved: 0,
        truncated: 0,
    }
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
        schema_version: 1,
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

pub(super) fn diagnostics_output() -> DiagnosticsOutput {
    DiagnosticsOutput {
        schema_version: 1,
        diagnostics_profile: 1,
        snapshot_sha256: "11".repeat(32),
        generation: 9,
        source_epoch: 2,
        producer_manifest_sha256: "55".repeat(32),
        index_coverage: coverage(),
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
        limitations: vec!["no_reference_index".to_owned()],
    }
}

pub(super) fn symbol_output() -> SymbolGetOutput {
    SymbolGetOutput {
        schema_version: 3,
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
            declaration_encoding: "lowercase_hex".to_owned(),
            declaration_hex: "70756220666e2072756e2829207b7d".to_owned(),
        }),
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
