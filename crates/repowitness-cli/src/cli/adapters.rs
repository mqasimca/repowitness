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
    fn index(
        &self,
        invocation: &IndexInvocation,
        configuration: &ResolvedConfiguration,
    ) -> Result<CliIndexReport, String>;
}

struct LocalRepositoryIndexer;

impl RepositoryIndexer for LocalRepositoryIndexer {
    fn index(
        &self,
        invocation: &IndexInvocation,
        configuration: &ResolvedConfiguration,
    ) -> Result<CliIndexReport, String> {
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
            )
            .with_configuration(configuration),
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
    fn search(
        &self,
        invocation: &SearchInvocation,
        configuration: &ResolvedConfiguration,
    ) -> Result<CliSearchReport, String>;
}

struct LocalRepositorySearcher;

impl RepositorySearcher for LocalRepositorySearcher {
    fn search(
        &self,
        invocation: &SearchInvocation,
        configuration: &ResolvedConfiguration,
    ) -> Result<CliSearchReport, String> {
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
            .map_err(|error| error.to_string())?
            .with_configuration(configuration);
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
    declaration_encoding: &'static str,
    declaration: String,
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
    let declaration = encoded_source_bytes(symbol.declaration());
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
        declaration_encoding: declaration.encoding,
        declaration: declaration.data,
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

struct EncodedSourceBytes {
    encoding: &'static str,
    data: String,
}

fn encoded_source_bytes(bytes: &[u8]) -> EncodedSourceBytes {
    match std::str::from_utf8(bytes) {
        Ok(text) if source_text_is_display_safe(text) => EncodedSourceBytes {
            encoding: "utf8",
            data: text.to_owned(),
        },
        Ok(_) | Err(_) => EncodedSourceBytes {
            encoding: "lowercase_hex",
            data: hex(bytes),
        },
    }
}

fn source_text_is_display_safe(text: &str) -> bool {
    text.chars().all(|character| {
        matches!(character, ' ' | '\n' | '\r' | '\t')
            || (!character.is_control()
                && !character.is_whitespace()
                && !is_unicode_display_control(character))
    })
}

fn is_unicode_display_control(character: char) -> bool {
    matches!(
        character,
        '\u{00ad}'
            | '\u{034f}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061c}'
            | '\u{06dd}'
            | '\u{070f}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08e2}'
            | '\u{115f}'..='\u{1160}'
            | '\u{17b4}'..='\u{17b5}'
            | '\u{180b}'..='\u{180f}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{206f}'
            | '\u{3164}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{feff}'
            | '\u{ffa0}'
            | '\u{fff0}'..='\u{fffb}'
            | '\u{110bd}'
            | '\u{110cd}'
            | '\u{13430}'..='\u{1343f}'
            | '\u{1bca0}'..='\u{1bca3}'
            | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0000}'..='\u{e0fff}'
    )
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
        schema_version: 4,
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
    let declaration = encoded_source_bytes(symbol.declaration());
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
        declaration_encoding: declaration.encoding.to_owned(),
        declaration: declaration.data,
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
    skipped_policy_paths: u64,
    skipped_unsupported_paths: u64,
    total_source_bytes: u64,
    total_facts: u64,
    syntax_error_nodes: u64,
    known_parser_limitation_nodes: u64,
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
            skipped_policy_paths: report.skipped_policy_paths(),
            skipped_unsupported_paths: report.skipped_unsupported_paths(),
            total_source_bytes: report.total_source_bytes(),
            total_facts: report.total_facts(),
            syntax_error_nodes: report.syntax_error_nodes(),
            known_parser_limitation_nodes: report.known_parser_limitation_nodes(),
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
