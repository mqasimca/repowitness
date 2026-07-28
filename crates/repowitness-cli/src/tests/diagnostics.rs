use std::cell::Cell;

use super::*;

struct FakeDiagnosticsReader {
    calls: Cell<u64>,
}

impl RepositoryDiagnosticsReader for FakeDiagnosticsReader {
    fn diagnose(&self, invocation: &DiagnosticsInvocation) -> Result<DiagnosticsOutput, String> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(invocation.database, Path::new("../index.db"));
        assert_eq!(invocation.repository_identity, OsStr::new("repository-id"));
        Ok(diagnostics_output())
    }
}

struct FailingDiagnosticsReader;

impl RepositoryDiagnosticsReader for FailingDiagnosticsReader {
    fn diagnose(&self, _invocation: &DiagnosticsInvocation) -> Result<DiagnosticsOutput, String> {
        Err("sensitive adapter detail: ../private.db".to_owned())
    }
}

fn diagnostics_output() -> DiagnosticsOutput {
    DiagnosticsOutput {
        schema_version: 1,
        diagnostics_profile: 1,
        snapshot_sha256: "11".repeat(32),
        generation: 3,
        source_epoch: 2,
        producer_manifest_sha256: "22".repeat(32),
        index_coverage: McpCoverage {
            searched: 4,
            skipped: 1,
            unresolved: 2,
            truncated: 0,
        },
        memory_projection: None,
        supported_languages: vec![
            "rust".to_owned(),
            "go".to_owned(),
            "typescript".to_owned(),
            "tsx".to_owned(),
            "python".to_owned(),
        ],
        capabilities: vec![
            "lexical_source_search".to_owned(),
            "exact_symbol_source".to_owned(),
        ],
        limitations: vec!["no_reference_index".to_owned()],
    }
}

#[test]
fn diagnostics_command_passes_explicit_inputs_and_emits_safe_aggregates() {
    let reader = FakeDiagnosticsReader {
        calls: Cell::new(0),
    };
    let arguments = [
        "--database",
        "../index.db",
        "--repository-id",
        "repository-id",
    ]
    .into_iter()
    .map(OsString::from);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_diagnostics(arguments, &mut stdout, &mut stderr, &reader);
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert_eq!(reader.calls.get(), 1);
    let output = String::from_utf8(stdout).expect("output");
    assert!(output.contains("operation=diagnostics"));
    assert!(output.contains("schema_version=1"));
    assert!(output.contains("memory_projection_available=false"));
    assert!(output.contains("capabilities=2"));
    assert!(output.contains("supported_language_3=tsx"));
    assert!(output.contains("supported_language_4=python"));
    assert!(output.contains("limitation_0=no_reference_index"));
    assert!(!output.contains("../index.db"));
    assert!(!output.contains("repository-id"));
}

#[test]
fn diagnostics_parser_rejects_missing_duplicate_odd_and_unknown_arguments() {
    for arguments in [
        vec![],
        vec!["--database", "index.db"],
        vec![
            "--database",
            "index.db",
            "--database",
            "other.db",
            "--repository-id",
            "id",
        ],
        vec!["--database", "index.db", "--repository-id"],
        vec![
            "--database",
            "index.db",
            "--repository-id",
            "id",
            "--root",
            "private",
        ],
    ] {
        let arguments = arguments
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        assert!(parse_diagnostics_arguments(&arguments).is_err());
    }
}

#[test]
fn diagnostics_failure_is_generic_and_redacted() {
    let arguments = [
        "--database",
        "../private.db",
        "--repository-id",
        "private-id",
    ]
    .into_iter()
    .map(OsString::from);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_diagnostics(
        arguments,
        &mut stdout,
        &mut stderr,
        &FailingDiagnosticsReader,
    );
    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"error: repository diagnostics failed\n");
}
