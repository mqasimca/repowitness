trait RepositoryPathInspector {
    fn inspect(&self, root: &Path) -> Result<GitPathDiscoveryStats, String>;
}

struct LocalRepositoryPathInspector;

impl RepositoryPathInspector for LocalRepositoryPathInspector {
    fn inspect(&self, root: &Path) -> Result<GitPathDiscoveryStats, String> {
        discover_repository_paths(root, GitPathDiscoveryLimits::default())
            .map(|discovered| discovered.stats())
            .map_err(|error| error.to_string())
    }
}

struct IndexInvocation {
    repository_root: PathBuf,
    database: PathBuf,
    repository_identity: OsString,
}

trait RepositoryIndexer {
    fn index(&self, invocation: &IndexInvocation) -> Result<CliIndexReport, String>;
}

struct LocalRepositoryIndexer;

impl RepositoryIndexer for LocalRepositoryIndexer {
    fn index(&self, invocation: &IndexInvocation) -> Result<CliIndexReport, String> {
        let repository_identity = invocation
            .repository_identity
            .to_str()
            .ok_or_else(|| "repository identity text is not valid UTF-8".to_owned())?;
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "system clock is before the Unix epoch".to_owned())?;
        let applied_at_unix_ms = u64::try_from(elapsed.as_millis())
            .map_err(|_| "system clock is outside the supported range".to_owned())?;
        index_local_repository(
            LocalIndexRequest::new(
                &invocation.repository_root,
                &invocation.database,
                repository_identity,
                applied_at_unix_ms,
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .map(CliIndexReport::from)
        .map_err(|error| error.to_string())
    }
}

struct SearchInvocation {
    database: PathBuf,
    repository_identity: OsString,
    query: OsString,
    max_results: u16,
}

trait RepositorySearcher {
    fn search(&self, invocation: &SearchInvocation) -> Result<CliSearchReport, String>;
}

struct LocalRepositorySearcher;

impl RepositorySearcher for LocalRepositorySearcher {
    fn search(&self, invocation: &SearchInvocation) -> Result<CliSearchReport, String> {
        let repository_identity = invocation
            .repository_identity
            .to_str()
            .ok_or_else(|| "repository identity text is not valid UTF-8".to_owned())?;
        let query = invocation
            .query
            .to_str()
            .ok_or_else(|| "query text is not valid UTF-8".to_owned())?;
        let request = LocalCodeSearchRequest::new(&invocation.database, repository_identity, query)
            .with_max_results(invocation.max_results)
            .map_err(|error| error.to_string())?;
        search_local_index(request, Arc::new(AtomicBool::new(false)))
            .map_err(|error| error.to_string())
            .and_then(CliSearchReport::try_from)
    }
}

struct CliSearchMatch {
    path: String,
    fact_ordinal: u64,
    content_digest: String,
    artifact_digest: String,
    producer_manifest: String,
    language: &'static str,
    kind: &'static str,
    name: String,
    qualified_name: String,
    name_start: u64,
    name_end: u64,
    declaration_start: u64,
    declaration_end: u64,
}

struct CliSearchReport {
    generation: i64,
    snapshot: String,
    resolution: &'static str,
    query_digest: String,
    returned_matches: u64,
    total_matches: u64,
    searched: u64,
    skipped: u64,
    unresolved: u64,
    truncated: u64,
    matches: Vec<CliSearchMatch>,
}

impl CliSearchReport {
    fn try_from(result: LocalCodeSearchResult) -> Result<Self, String> {
        let mut matches = Vec::with_capacity(result.evidence().as_slice().len());
        for evidence in result.evidence().as_slice() {
            let EvidenceLocation::SymbolOccurrence(occurrence) = evidence.identity().location()
            else {
                return Err("code-search evidence location is invalid".to_owned());
            };
            let path = RepositoryPathTextV1::encode(evidence.identity().path(), PATH_TEXT_LIMIT)
                .map_err(|error| error.to_string())?
                .into_string();
            matches.push(CliSearchMatch {
                path,
                fact_ordinal: occurrence.fact_ordinal(),
                content_digest: hex(evidence.identity().content_digest().as_bytes()),
                artifact_digest: hex(occurrence.artifact_digest().as_bytes()),
                producer_manifest: hex(evidence.producer().version().as_bytes()),
                language: occurrence.language().as_str(),
                kind: occurrence.kind().as_str(),
                name: occurrence.name().to_owned(),
                qualified_name: occurrence.qualified_name().to_owned(),
                name_start: occurrence.name_span().start().get(),
                name_end: occurrence.name_span().end().get(),
                declaration_start: occurrence.declaration_span().start().get(),
                declaration_end: occurrence.declaration_span().end().get(),
            });
        }
        let coverage = result.coverage();
        Ok(Self {
            generation: result.generation().get(),
            snapshot: hex(result.snapshot().as_bytes()),
            resolution: resolution_text(result.resolution()),
            query_digest: hex(result.claim().query().as_bytes()),
            returned_matches: result.claim().returned_matches(),
            total_matches: result.claim().total_matches(),
            searched: coverage.searched().get(),
            skipped: coverage.skipped().get(),
            unresolved: coverage.unresolved().get(),
            truncated: coverage.truncated().get(),
            matches,
        })
    }
}

struct SymbolInvocation {
    root: PathBuf,
    database: PathBuf,
    repository_identity: OsString,
    snapshot: OsString,
    generation: i64,
    path: OsString,
    content: OsString,
    artifact: OsString,
    fact_ordinal: u64,
}

trait RepositorySymbolGetter {
    fn get(&self, invocation: &SymbolInvocation) -> Result<CliSymbolReport, String>;
}

struct LocalRepositorySymbolGetter;

impl RepositorySymbolGetter for LocalRepositorySymbolGetter {
    fn get(&self, invocation: &SymbolInvocation) -> Result<CliSymbolReport, String> {
        let repository_identity = utf8_option(&invocation.repository_identity)?;
        let snapshot = utf8_option(&invocation.snapshot)?;
        let path = utf8_option(&invocation.path)?;
        let content = utf8_option(&invocation.content)?;
        let artifact = utf8_option(&invocation.artifact)?;
        let selector = LocalSymbolSelectorText::new(
            snapshot,
            invocation.generation,
            path,
            content,
            artifact,
            invocation.fact_ordinal,
        );
        get_local_symbol(
            LocalSymbolGetRequest::new(
                &invocation.root,
                &invocation.database,
                repository_identity,
                selector,
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .map_err(|error| error.to_string())
        .and_then(CliSymbolReport::try_from)
    }
}

fn utf8_option(value: &OsStr) -> Result<&str, String> {
    value
        .to_str()
        .ok_or_else(|| "symbol-get option text is not valid UTF-8".to_owned())
}

struct CliSymbolData {
    producer_manifest: String,
    language: &'static str,
    kind: &'static str,
    name: String,
    qualified_name: String,
    name_start: u64,
    name_end: u64,
    declaration_start: u64,
    declaration_end: u64,
    declaration_hex: String,
}

struct CliSymbolReport {
    generation: i64,
    snapshot: String,
    resolution: &'static str,
    path: String,
    content_digest: String,
    artifact_digest: String,
    fact_ordinal: u64,
    searched: u64,
    skipped: u64,
    unresolved: u64,
    truncated: u64,
    symbol: Option<CliSymbolData>,
}

impl CliSymbolReport {
    fn try_from(result: LocalSymbolGetResult) -> Result<Self, String> {
        let selector = result.claim().selector();
        let path = RepositoryPathTextV1::encode(selector.path(), PATH_TEXT_LIMIT)
            .map_err(|error| error.to_string())?
            .into_string();
        let symbol = result
            .claim()
            .symbol()
            .map(|symbol| symbol_report_data(&result, symbol))
            .transpose()?;
        let coverage = result.coverage();
        Ok(Self {
            generation: result.generation().get(),
            snapshot: hex(result.snapshot().as_bytes()),
            resolution: resolution_text(result.resolution()),
            path,
            content_digest: hex(selector.content_digest().as_bytes()),
            artifact_digest: hex(selector.artifact_digest().as_bytes()),
            fact_ordinal: selector.fact_ordinal(),
            searched: coverage.searched().get(),
            skipped: coverage.skipped().get(),
            unresolved: coverage.unresolved().get(),
            truncated: coverage.truncated().get(),
            symbol,
        })
    }
}

fn symbol_report_data(
    result: &LocalSymbolGetResult,
    symbol: &repowitness_local::RetrievedSymbol,
) -> Result<CliSymbolData, String> {
    let evidence = result.evidence().as_slice();
    if evidence.len() != 1 {
        return Err("resolved symbol-get evidence count is invalid".to_owned());
    }
    let occurrence = symbol.occurrence();
    Ok(CliSymbolData {
        producer_manifest: hex(evidence[0].producer().version().as_bytes()),
        language: occurrence.language().as_str(),
        kind: occurrence.kind().as_str(),
        name: occurrence.name().to_owned(),
        qualified_name: occurrence.qualified_name().to_owned(),
        name_start: occurrence.name_span().start().get(),
        name_end: occurrence.name_span().end().get(),
        declaration_start: occurrence.declaration_span().start().get(),
        declaration_end: occurrence.declaration_span().end().get(),
        declaration_hex: hex(symbol.declaration()),
    })
}

fn resolution_text(resolution: ResolutionStatus) -> &'static str {
    match resolution {
        ResolutionStatus::Confirmed => "confirmed",
        ResolutionStatus::Inferred => "inferred",
        ResolutionStatus::Ambiguous => "ambiguous",
        ResolutionStatus::Unresolved => "unresolved",
        ResolutionStatus::Indeterminate => "indeterminate",
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

struct LocalMcpRepositoryService {
    root: PathBuf,
    database: PathBuf,
    repository_identity: String,
    memory_actor: Option<String>,
}

impl RepositoryService for LocalMcpRepositoryService {
    fn code_search(
        &self,
        request: CodeSearchServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<CodeSearchOutput, RepositoryServiceError> {
        let local_request =
            LocalCodeSearchRequest::new(&self.database, &self.repository_identity, request.query())
                .with_max_results(request.max_results())
                .map_err(|_| RepositoryServiceError::CodeSearch)?
                .with_deadline(request.timeout());
        search_local_index(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::CodeSearch)
            .and_then(|result| {
                mcp_search_output(result).map_err(|_| RepositoryServiceError::CodeSearch)
            })
    }

    fn context_build(
        &self,
        request: ContextBuildServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<ContextBuildOutput, RepositoryServiceError> {
        let local_request = LocalContextBuildRequest::new(
            &self.root,
            &self.database,
            &self.repository_identity,
            request.intent(),
        )
        .with_budget_units(request.budget_units())
        .map_err(|_| RepositoryServiceError::ContextBuild)?
        .with_max_provider_results(request.max_provider_results())
        .map_err(|_| RepositoryServiceError::ContextBuild)?
        .with_deadline(request.timeout());
        build_local_context(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::ContextBuild)
            .and_then(|result| {
                mcp_context_output(result).map_err(|_| RepositoryServiceError::ContextBuild)
            })
    }

    fn diagnostics(
        &self,
        request: DiagnosticsServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<DiagnosticsOutput, RepositoryServiceError> {
        let local_request =
            LocalRepositoryDiagnosticsRequest::new(&self.database, &self.repository_identity)
                .with_deadline(request.timeout());
        diagnose_local_repository(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::Diagnostics)
            .map(mcp_diagnostics_output)
    }

    fn memory_recall(
        &self,
        request: MemoryRecallServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<MemoryRecallOutput, RepositoryServiceError> {
        let selection = match request.selection() {
            MemoryRecallServiceSelection::All => LocalMemoryRecallSelection::All,
            MemoryRecallServiceSelection::Query(query) => {
                LocalMemoryRecallSelection::Query(query.as_str())
            }
        };
        let local_request =
            LocalMemoryRecallRequest::new(&self.database, &self.repository_identity, selection)
                .with_max_results(request.max_results())
                .map_err(|_| RepositoryServiceError::MemoryRecall)?
                .with_deadline(request.timeout());
        recall_local_memory(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::MemoryRecall)
            .and_then(|result| {
                mcp_memory_output(result).map_err(|_| RepositoryServiceError::MemoryRecall)
            })
    }

    fn memory_manage(
        &self,
        request: MemoryManageServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<MemoryManageOutput, RepositoryServiceError> {
        manage_mcp_memory(self, request, cancelled)
    }

    fn symbol_get(
        &self,
        request: SymbolGetServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<SymbolGetOutput, RepositoryServiceError> {
        let selector = LocalSymbolSelectorText::new(
            request.snapshot_sha256(),
            request.generation(),
            request.path(),
            request.content_sha256(),
            request.artifact_sha256(),
            request.fact_ordinal(),
        );
        let local_request = LocalSymbolGetRequest::new(
            &self.root,
            &self.database,
            &self.repository_identity,
            selector,
        )
        .with_deadline(request.timeout());
        get_local_symbol(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::SymbolGet)
            .and_then(|result| {
                mcp_symbol_output(result).map_err(|_| RepositoryServiceError::SymbolGet)
            })
    }
}

fn mcp_search_output(result: LocalCodeSearchResult) -> Result<CodeSearchOutput, String> {
    let mut matches = Vec::with_capacity(result.evidence().as_slice().len());
    for evidence in result.evidence().as_slice() {
        let EvidenceLocation::SymbolOccurrence(occurrence) = evidence.identity().location() else {
            return Err("code-search evidence location is invalid".to_owned());
        };
        let path = RepositoryPathTextV1::encode(evidence.identity().path(), PATH_TEXT_LIMIT)
            .map_err(|error| error.to_string())?
            .into_string();
        matches.push(McpSearchMatch {
            path,
            fact_ordinal: occurrence.fact_ordinal(),
            content_sha256: hex(evidence.identity().content_digest().as_bytes()),
            artifact_sha256: hex(occurrence.artifact_digest().as_bytes()),
            producer_manifest_sha256: hex(evidence.producer().version().as_bytes()),
            evidence_tier: "syntax".to_owned(),
            language: occurrence.language().as_str().to_owned(),
            kind: occurrence.kind().as_str().to_owned(),
            name: occurrence.name().to_owned(),
            qualified_name: occurrence.qualified_name().to_owned(),
            name_span: McpSpan {
                start: occurrence.name_span().start().get(),
                end: occurrence.name_span().end().get(),
            },
            declaration_span: McpSpan {
                start: occurrence.declaration_span().start().get(),
                end: occurrence.declaration_span().end().get(),
            },
        });
    }
    if u64::try_from(matches.len()).ok() != Some(result.claim().returned_matches()) {
        return Err("code-search evidence count is inconsistent".to_owned());
    }
    let coverage = result.coverage();
    Ok(CodeSearchOutput {
        schema_version: 3,
        query_profile: CODE_SEARCH_PROFILE_VERSION,
        snapshot_sha256: hex(result.snapshot().as_bytes()),
        generation: result.generation().get(),
        resolution: resolution_text(result.resolution()).to_owned(),
        query_sha256: hex(result.claim().query().as_bytes()),
        matches_returned: result.claim().returned_matches(),
        matches_total: result.claim().total_matches(),
        coverage: McpCoverage {
            searched: coverage.searched().get(),
            skipped: coverage.skipped().get(),
            unresolved: coverage.unresolved().get(),
            truncated: coverage.truncated().get(),
        },
        limitation: "supported_language_symbol_lexical_only".to_owned(),
        matches,
    })
}

fn mcp_symbol_output(result: LocalSymbolGetResult) -> Result<SymbolGetOutput, String> {
    let selector = result.claim().selector();
    let selector_output = SymbolSelectorOutput {
        path: RepositoryPathTextV1::encode(selector.path(), PATH_TEXT_LIMIT)
            .map_err(|error| error.to_string())?
            .into_string(),
        content_sha256: hex(selector.content_digest().as_bytes()),
        artifact_sha256: hex(selector.artifact_digest().as_bytes()),
        fact_ordinal: selector.fact_ordinal(),
    };
    let symbol = result
        .claim()
        .symbol()
        .map(|symbol| mcp_symbol_data(&result, symbol))
        .transpose()?;
    let coverage = result.coverage();
    Ok(SymbolGetOutput {
        schema_version: 3,
        symbol_profile: SYMBOL_GET_PROFILE_VERSION,
        snapshot_sha256: hex(result.snapshot().as_bytes()),
        generation: result.generation().get(),
        resolution: resolution_text(result.resolution()).to_owned(),
        selector: selector_output,
        coverage: McpCoverage {
            searched: coverage.searched().get(),
            skipped: coverage.skipped().get(),
            unresolved: coverage.unresolved().get(),
            truncated: coverage.truncated().get(),
        },
        limitation: "references_not_implemented".to_owned(),
        symbol,
    })
}

fn mcp_symbol_data(
    result: &LocalSymbolGetResult,
    symbol: &repowitness_local::RetrievedSymbol,
) -> Result<McpSymbol, String> {
    let evidence = result.evidence().as_slice();
    if evidence.len() != 1 {
        return Err("resolved symbol-get evidence count is invalid".to_owned());
    }
    let occurrence = symbol.occurrence();
    Ok(McpSymbol {
        producer_manifest_sha256: hex(evidence[0].producer().version().as_bytes()),
        evidence_tier: "syntax".to_owned(),
        language: occurrence.language().as_str().to_owned(),
        kind: occurrence.kind().as_str().to_owned(),
        name: occurrence.name().to_owned(),
        qualified_name: occurrence.qualified_name().to_owned(),
        name_span: McpSpan {
            start: occurrence.name_span().start().get(),
            end: occurrence.name_span().end().get(),
        },
        declaration_span: McpSpan {
            start: occurrence.declaration_span().start().get(),
            end: occurrence.declaration_span().end().get(),
        },
        declaration_encoding: "lowercase_hex".to_owned(),
        declaration_hex: hex(symbol.declaration()),
    })
}

#[derive(Clone, Copy)]
struct CliIndexReport {
    generation: i64,
    source_epoch: u64,
    recovered_generations: u64,
    discovered_paths: u64,
    indexed_rust_files: u64,
    indexed_go_files: u64,
    indexed_typescript_files: u64,
    indexed_tsx_files: u64,
    indexed_python_files: u64,
    skipped_unsupported_paths: u64,
    total_source_bytes: u64,
    total_facts: u64,
    syntax_error_nodes: u64,
    reused_rust_files: u64,
    analyzed_rust_files: u64,
    reused_go_files: u64,
    analyzed_go_files: u64,
    reused_typescript_files: u64,
    analyzed_typescript_files: u64,
    reused_tsx_files: u64,
    analyzed_tsx_files: u64,
    reused_python_files: u64,
    analyzed_python_files: u64,
}

impl From<LocalIndexReport> for CliIndexReport {
    fn from(report: LocalIndexReport) -> Self {
        Self {
            generation: report.generation().get(),
            source_epoch: report.source_epoch(),
            recovered_generations: report.recovered_generations(),
            discovered_paths: report.discovered_paths(),
            indexed_rust_files: report.indexed_rust_files(),
            indexed_go_files: report.indexed_go_files(),
            indexed_typescript_files: report.indexed_typescript_files(),
            indexed_tsx_files: report.indexed_tsx_files(),
            indexed_python_files: report.indexed_python_files(),
            skipped_unsupported_paths: report.skipped_unsupported_paths(),
            total_source_bytes: report.total_source_bytes(),
            total_facts: report.total_facts(),
            syntax_error_nodes: report.syntax_error_nodes(),
            reused_rust_files: report.reused_rust_files(),
            analyzed_rust_files: report.analyzed_rust_files(),
            reused_go_files: report.reused_go_files(),
            analyzed_go_files: report.analyzed_go_files(),
            reused_typescript_files: report.reused_typescript_files(),
            analyzed_typescript_files: report.analyzed_typescript_files(),
            reused_tsx_files: report.reused_tsx_files(),
            analyzed_tsx_files: report.analyzed_tsx_files(),
            reused_python_files: report.reused_python_files(),
            analyzed_python_files: report.analyzed_python_files(),
        }
    }
}
