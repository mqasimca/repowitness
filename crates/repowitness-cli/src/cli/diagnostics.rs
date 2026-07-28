struct DiagnosticsInvocation {
    database: PathBuf,
    repository_identity: OsString,
}

trait RepositoryDiagnosticsReader {
    fn diagnose(&self, invocation: &DiagnosticsInvocation) -> Result<DiagnosticsOutput, String>;
}

struct LocalRepositoryDiagnosticsReader;

impl RepositoryDiagnosticsReader for LocalRepositoryDiagnosticsReader {
    fn diagnose(&self, invocation: &DiagnosticsInvocation) -> Result<DiagnosticsOutput, String> {
        let repository_identity = invocation
            .repository_identity
            .to_str()
            .ok_or_else(|| "repository identity must be UTF-8".to_owned())?;
        let request =
            LocalRepositoryDiagnosticsRequest::new(&invocation.database, repository_identity);
        diagnose_local_repository(request, Arc::new(AtomicBool::new(false)))
            .map_err(|error| error.to_string())
            .map(mcp_diagnostics_output)
    }
}

fn mcp_diagnostics_output(result: LocalRepositoryDiagnosticsResult) -> DiagnosticsOutput {
    let memory_projection = result.memory_projection().map(|memory| {
        McpDiagnosticsMemoryProjection {
            projection: *memory.projection(),
            source_epoch: memory.source_epoch(),
            snapshot_sha256: hex(memory.snapshot().as_bytes()),
            coverage: mcp_memory_coverage(memory.coverage()),
        }
    });
    DiagnosticsOutput {
        schema_version: 1,
        diagnostics_profile: result.profile_version(),
        snapshot_sha256: hex(result.snapshot().as_bytes()),
        generation: result.generation().get(),
        source_epoch: result.source_epoch(),
        producer_manifest_sha256: hex(result.producer_manifest().as_bytes()),
        index_coverage: McpCoverage {
            searched: result.index_coverage().searched(),
            skipped: result.index_coverage().skipped(),
            unresolved: result.index_coverage().unresolved(),
            truncated: result.index_coverage().truncated(),
        },
        memory_projection,
        supported_languages: result
            .supported_languages()
            .iter()
            .map(|language| language.as_str().to_owned())
            .collect(),
        capabilities: result
            .capabilities()
            .iter()
            .map(|capability| capability.as_str().to_owned())
            .collect(),
        limitations: result
            .limitations()
            .iter()
            .map(|limitation| limitation.as_str().to_owned())
            .collect(),
    }
}
