use std::cell::Cell;

use super::*;

struct FakeContextBuilder {
    calls: Cell<u64>,
}

impl RepositoryContextBuilder for FakeContextBuilder {
    fn build(&self, invocation: &ContextInvocation) -> Result<ContextBuildOutput, String> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(invocation.root, Path::new("../repository"));
        assert_eq!(invocation.database, Path::new("../index.db"));
        assert_eq!(invocation.repository_identity, OsStr::new("repository-id"));
        assert_eq!(invocation.intent, OsStr::new("Widget"));
        assert_eq!(invocation.budget_units, 4096);
        assert_eq!(invocation.max_provider_results, 7);
        Ok(context_output())
    }
}

struct FailingContextBuilder;

impl RepositoryContextBuilder for FailingContextBuilder {
    fn build(&self, _invocation: &ContextInvocation) -> Result<ContextBuildOutput, String> {
        Err("sensitive adapter detail: ../private-repository".to_owned())
    }
}

fn context_output() -> ContextBuildOutput {
    ContextBuildOutput {
        schema_version: 1,
        context_profile: 1,
        reciprocal_rank_k: 60,
        budget_estimator: "utf8_bytes_upper_bound_v1".to_owned(),
        budget_units: 4096,
        used_units: 6,
        query_sha256: "11".repeat(32),
        snapshot_sha256: "22".repeat(32),
        generation: 3,
        memory: None,
        coverage: McpContextCoverage {
            source_index: McpCoverage {
                searched: 1,
                skipped: 0,
                unresolved: 0,
                truncated: 0,
            },
            source_total_matches: 1,
            source_returned_matches: 1,
            source_expansion_omitted: 0,
            source_budget_omitted: 0,
            source_included: 1,
            memory_total_matches: 0,
            memory_returned_matches: 0,
            memory_non_current_omitted: 0,
            memory_budget_omitted: 0,
            memory_included: 0,
        },
        omissions: vec![McpContextOmission {
            kind: "memory_projection_unavailable".to_owned(),
            provider: Some("memory".to_owned()),
            count: None,
        }],
        items: vec![McpContextItem::Source(McpContextSourceItem {
            provider_rank: 1,
            fused_rank: 1,
            reciprocal_rank_denominator: 61,
            estimated_units: 6,
            path: "rwp1:h:7372632F6C69622E7273".to_owned(),
            content_sha256: "33".repeat(32),
            artifact_sha256: "44".repeat(32),
            fact_ordinal: 2,
            producer_manifest_sha256: "55".repeat(32),
            language: "rust".to_owned(),
            declaration_kind: "struct".to_owned(),
            name: "Widget".to_owned(),
            qualified_name: "crate::Widget".to_owned(),
            name_span: McpSpan { start: 0, end: 6 },
            declaration_span: McpSpan { start: 0, end: 6 },
            declaration_encoding: "lowercase_hex".to_owned(),
            declaration_hex: "576964676574".to_owned(),
        })],
    }
}

#[test]
fn context_command_passes_explicit_bounds_and_emits_safe_evidence() {
    let builder = FakeContextBuilder {
        calls: Cell::new(0),
    };
    let arguments = [
        "--root",
        "../repository",
        "--database",
        "../index.db",
        "--repository-id",
        "repository-id",
        "--intent",
        "Widget",
        "--budget",
        "4096",
        "--limit",
        "7",
    ]
    .into_iter()
    .map(OsString::from);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_context_build(arguments, &mut stdout, &mut stderr, &builder);
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert_eq!(builder.calls.get(), 1);
    let output = String::from_utf8(stdout).expect("output");
    assert!(output.contains("operation=context-build"));
    assert!(output.contains("budget_estimator=utf8_bytes_upper_bound_v1"));
    assert!(output.contains("context_item_0_name_hex=576964676574"));
    assert!(output.contains("context_item_0_declaration_hex=576964676574"));
    assert!(!output.contains("crate::Widget"));
}

#[test]
fn context_parser_rejects_missing_duplicate_and_out_of_range_values() {
    for arguments in [
        vec![],
        vec!["--root", "repository"],
        vec![
            "--root",
            "repository",
            "--root",
            "other",
            "--database",
            "index.db",
            "--repository-id",
            "id",
            "--intent",
            "x",
        ],
        vec![
            "--root",
            "repository",
            "--database",
            "index.db",
            "--repository-id",
            "id",
            "--intent",
            "x",
            "--budget",
            "0",
        ],
        vec![
            "--root",
            "repository",
            "--database",
            "index.db",
            "--repository-id",
            "id",
            "--intent",
            "x",
            "--limit",
            "101",
        ],
    ] {
        let arguments = arguments
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        assert!(parse_context_build_arguments(&arguments).is_err());
    }
}

#[test]
fn context_failure_is_generic_and_redacted() {
    let arguments = [
        "--root",
        "../private-repository",
        "--database",
        "../private.db",
        "--repository-id",
        "private-id",
        "--intent",
        "private query",
    ]
    .into_iter()
    .map(OsString::from);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_context_build(arguments, &mut stdout, &mut stderr, &FailingContextBuilder);
    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"error: context build failed\n");
}

#[test]
fn context_output_reports_every_memory_projection_state() {
    let mut report = context_output();
    report.memory = Some(McpContextMemoryProjection {
        projection: 8,
        source_epoch: 3,
        producer: McpMemoryProducer {
            id: "profile".to_owned(),
            version: 1,
            profile_sha256: "66".repeat(32),
        },
        coverage: McpMemoryCoverage {
            searched: 10,
            skipped: 0,
            unresolved: 3,
            truncated: 0,
            total: 10,
            current: 1,
            not_applicable: 1,
            stale: 1,
            needs_review: 1,
            indeterminate: 1,
            conflicted: 1,
            contradicted: 1,
            superseded: 1,
            quarantined: 1,
            tombstoned: 1,
        },
    });
    let mut output = Vec::new();
    assert_eq!(emit_context_report(&mut output, &report), EXIT_SUCCESS);
    let output = String::from_utf8(output).expect("UTF-8 output");
    for state in [
        "current",
        "not_applicable",
        "stale",
        "needs_review",
        "indeterminate",
        "conflicted",
        "contradicted",
        "superseded",
        "quarantined",
        "tombstoned",
    ] {
        assert!(output.contains(&format!("memory_projection_{state}=1")));
    }
}
