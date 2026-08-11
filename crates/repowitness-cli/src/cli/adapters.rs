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

    fn reconcile(
        &self,
        invocation: &IndexInvocation,
        configuration: &ResolvedConfiguration,
    ) -> Result<CliIndexReport, String> {
        self.index(invocation, configuration)
    }

    fn reconcile_with_cancel(
        &self,
        invocation: &IndexInvocation,
        configuration: &ResolvedConfiguration,
        cancelled: Arc<AtomicBool>,
    ) -> Result<CliIndexReport, String> {
        let _ = cancelled;
        self.reconcile(invocation, configuration)
    }
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

    fn reconcile(
        &self,
        invocation: &IndexInvocation,
        configuration: &ResolvedConfiguration,
    ) -> Result<CliIndexReport, String> {
        self.reconcile_with_cancel(
            invocation,
            configuration,
            Arc::new(AtomicBool::new(false)),
        )
    }

    fn reconcile_with_cancel(
        &self,
        invocation: &IndexInvocation,
        configuration: &ResolvedConfiguration,
        cancelled: Arc<AtomicBool>,
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
        reconcile_local_repository(
            LocalIndexRequest::new(
                &invocation.repository_root,
                &invocation.database,
                repository_identity,
                applied_at_unix_ms,
            )
            .with_configuration(configuration),
            cancelled,
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

fn mcp_architecture_map_output(
    result: LocalArchitectureMapResult,
) -> Result<ArchitectureMapOutput, String> {
    let files = result
        .files()
        .iter()
        .map(|file| {
            Ok(McpArchitectureMapFile {
                path: RepositoryPathTextV1::encode(file.path(), PATH_TEXT_LIMIT)
                    .map_err(|error| error.to_string())?
                    .into_string(),
                language: file.language().as_str().to_owned(),
                content_sha256: hex(file.content_digest().as_bytes()),
                artifact_sha256: hex(file.artifact_digest().as_bytes()),
                producer_manifest_sha256: hex(file.producer_manifest().as_bytes()),
                declaration_count: file.declaration_count(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let files_returned = u64::try_from(files.len())
        .map_err(|_| "architecture-map file count overflowed".to_owned())?;
    if files_returned > result.total_files() {
        return Err("architecture-map file totals are inconsistent".to_owned());
    }
    let languages = result
        .language_summaries()
        .iter()
        .map(|summary| McpArchitectureMapLanguage {
            language: summary.language().as_str().to_owned(),
            files: summary.file_count(),
            declarations: summary.declaration_count(),
        })
        .collect();
    let coverage = result.index_coverage();
    Ok(ArchitectureMapOutput {
        schema_version: 1,
        map_profile: ARCHITECTURE_MAP_PROFILE_VERSION,
        snapshot_sha256: hex(result.snapshot().as_bytes()),
        generation: result.generation().get(),
        coverage: McpCoverage {
            searched: coverage.searched(),
            skipped: coverage.skipped(),
            unresolved: coverage.unresolved(),
            truncated: coverage.truncated(),
        },
        total_files: result.total_files(),
        total_declarations: result.total_declarations(),
        files_returned,
        truncated: result.truncated(),
        output_bytes: result.output_bytes(),
        limitation: "file_inventory_only_no_relationship_inference".to_owned(),
        languages,
        files,
    })
}

fn mcp_repository_topology_output(
    result: repowitness_local::LocalRepositoryTopologyResult,
) -> Result<RepositoryTopologyOutput, String> {
    let entries = result.entries().iter().map(|entry| {
        Ok(McpRepositoryTopologyEntry {
            path: RepositoryPathTextV1::encode(entry.path(), PATH_TEXT_LIMIT)
                .map_err(|error| error.to_string())?
                .into_string(),
            category: entry.category().as_str().to_owned(),
        })
    }).collect::<Result<Vec<_>, String>>()?;
    let paths_returned = u64::try_from(entries.len())
        .map_err(|_| "repository-topology entry count overflowed".to_owned())?;
    if paths_returned > result.total_paths() {
        return Err("repository-topology totals are inconsistent".to_owned());
    }
    let coverage = result.coverage();
    Ok(RepositoryTopologyOutput {
        schema_version: 1,
        topology_profile: result.topology_profile_version(),
        snapshot_sha256: hex(result.snapshot().as_bytes()),
        generation: result.generation().get(),
        topology_sha256: hex(result.topology_digest()),
        coverage: McpRepositoryTopologyCoverage {
            discovered_paths: coverage.discovered_paths(),
            omitted_paths: coverage.omitted_paths(),
        },
        total_paths: result.total_paths(),
        paths_returned,
        truncated: result.truncated(),
        output_bytes: result.output_bytes(),
        limitation: "inventory_only_no_semantic_relationship_inference".to_owned(),
        categories: result.category_summaries().iter().map(|summary| McpRepositoryTopologyCategory {
            category: summary.category().as_str().to_owned(),
            paths: summary.path_count(),
        }).collect(),
        entries,
    })
}

fn mcp_architecture_overview_output(
    result: LocalArchitectureOverviewResult,
) -> Result<ArchitectureOverviewOutput, String> {
    let files = result
        .files()
        .iter()
        .map(mcp_architecture_overview_file)
        .collect::<Result<Vec<_>, _>>()?;
    let entry_point_candidates = result
        .entry_point_candidates()
        .iter()
        .map(mcp_architecture_overview_candidate)
        .collect::<Result<Vec<_>, _>>()?;
    let source_roots = result
        .source_roots()
        .iter()
        .map(|summary| {
            let (kind, path) = match summary.root() {
                ArchitectureOverviewSourceRoot::RepositoryRoot => ("repository_root", None),
                ArchitectureOverviewSourceRoot::TopLevelDirectory(path) => (
                    "top_level_directory",
                    Some(
                        RepositoryPathTextV1::encode(path, PATH_TEXT_LIMIT)
                            .map_err(|error| error.to_string())?
                            .into_string(),
                    ),
                ),
            };
            Ok(McpArchitectureOverviewRoot {
                kind: kind.to_owned(),
                path,
                files: summary.file_count(),
                declarations: summary.declaration_count(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let languages = result
        .language_summaries()
        .iter()
        .map(|summary| McpArchitectureMapLanguage {
            language: summary.language().as_str().to_owned(),
            files: summary.file_count(),
            declarations: summary.declaration_count(),
        })
        .collect();
    let kinds = result
        .kind_summaries()
        .iter()
        .map(|summary| McpArchitectureOverviewKind {
            language: summary.language().as_str().to_owned(),
            kind: summary.kind().as_str().to_owned(),
            declarations: summary.declaration_count(),
        })
        .collect();
    validate_architecture_overview_counts(
        &files,
        &entry_point_candidates,
        &source_roots,
        &result,
    )?;
    let coverage = result.index_coverage();
    Ok(ArchitectureOverviewOutput {
        schema_version: 1,
        overview_profile: ARCHITECTURE_OVERVIEW_PROFILE_VERSION,
        snapshot_sha256: hex(result.snapshot().as_bytes()),
        generation: result.generation().get(),
        source_producer_manifest_sha256: hex(result.source_producer_manifest().as_bytes()),
        coverage: McpCoverage {
            searched: coverage.searched(),
            skipped: coverage.skipped(),
            unresolved: coverage.unresolved(),
            truncated: coverage.truncated(),
        },
        total_files: result.total_files(),
        total_declarations: result.total_declarations(),
        total_source_roots: result.total_source_roots(),
        source_roots_returned: u64::try_from(source_roots.len())
            .map_err(|_| "architecture-overview root count overflowed".to_owned())?,
        source_roots_truncated: result.source_roots_truncated(),
        total_entry_point_candidates: result.total_entry_point_candidates(),
        entry_point_candidates_returned: u64::try_from(entry_point_candidates.len())
            .map_err(|_| "architecture-overview candidate count overflowed".to_owned())?,
        entry_point_candidates_truncated: result.entry_point_candidates_truncated(),
        files_returned: u64::try_from(files.len())
            .map_err(|_| "architecture-overview file count overflowed".to_owned())?,
        files_truncated: result.files_truncated(),
        output_bytes: result.output_bytes(),
        limitations: ARCHITECTURE_OVERVIEW_LIMITATIONS
            .iter()
            .map(|limitation| (*limitation).to_owned())
            .collect(),
        languages,
        kinds,
        source_roots,
        entry_point_candidates,
        files,
    })
}

fn mcp_architecture_overview_file(
    file: &ArchitectureMapFile,
) -> Result<McpArchitectureMapFile, String> {
    Ok(McpArchitectureMapFile {
        path: RepositoryPathTextV1::encode(file.path(), PATH_TEXT_LIMIT)
            .map_err(|error| error.to_string())?
            .into_string(),
        language: file.language().as_str().to_owned(),
        content_sha256: hex(file.content_digest().as_bytes()),
        artifact_sha256: hex(file.artifact_digest().as_bytes()),
        producer_manifest_sha256: hex(file.producer_manifest().as_bytes()),
        declaration_count: file.declaration_count(),
    })
}

fn mcp_architecture_overview_candidate(
    candidate: &ArchitectureOverviewEntryPointCandidate,
) -> Result<McpSearchMatch, String> {
    let occurrence = candidate.occurrence();
    Ok(McpSearchMatch {
        path: RepositoryPathTextV1::encode(candidate.path(), PATH_TEXT_LIMIT)
            .map_err(|error| error.to_string())?
            .into_string(),
        fact_ordinal: occurrence.fact_ordinal(),
        content_sha256: hex(candidate.content_digest().as_bytes()),
        artifact_sha256: hex(occurrence.artifact_digest().as_bytes()),
        producer_manifest_sha256: hex(occurrence.producer_manifest().as_bytes()),
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
    })
}

fn validate_architecture_overview_counts(
    files: &[McpArchitectureMapFile],
    candidates: &[McpSearchMatch],
    roots: &[McpArchitectureOverviewRoot],
    result: &LocalArchitectureOverviewResult,
) -> Result<(), String> {
    let files = u64::try_from(files.len())
        .map_err(|_| "architecture-overview file count overflowed".to_owned())?;
    let candidates = u64::try_from(candidates.len())
        .map_err(|_| "architecture-overview candidate count overflowed".to_owned())?;
    let roots = u64::try_from(roots.len())
        .map_err(|_| "architecture-overview root count overflowed".to_owned())?;
    if files > result.total_files()
        || candidates > result.total_entry_point_candidates()
        || roots > result.total_source_roots()
    {
        return Err("architecture-overview receipt totals are inconsistent".to_owned());
    }
    Ok(())
}

fn mcp_search_output(result: LocalCodeSearchResult) -> Result<CodeSearchOutput, String> {
    let claim = result.claim();
    let coverage = result.coverage();
    mcp_interoperable_i64(&[result.generation().get()])?;
    mcp_interoperable(&[
        claim.returned_matches(),
        claim.total_matches(),
        coverage.searched().get(),
        coverage.skipped().get(),
        coverage.unresolved().get(),
        coverage.truncated().get(),
    ])?;
    let mut matches = Vec::with_capacity(result.evidence().as_slice().len());
    for evidence in result.evidence().as_slice() {
        let EvidenceLocation::SymbolOccurrence(occurrence) = evidence.identity().location() else {
            return Err("code-search evidence location is invalid".to_owned());
        };
        mcp_interoperable(&[
            occurrence.fact_ordinal(),
            occurrence.name_span().start().get(),
            occurrence.name_span().end().get(),
            occurrence.declaration_span().start().get(),
            occurrence.declaration_span().end().get(),
        ])?;
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
    if u64::try_from(matches.len()).ok() != Some(claim.returned_matches()) {
        return Err("code-search evidence count is inconsistent".to_owned());
    }
    Ok(CodeSearchOutput {
        schema_version: 3,
        query_profile: CODE_SEARCH_PROFILE_VERSION,
        snapshot_sha256: hex(result.snapshot().as_bytes()),
        generation: result.generation().get(),
        resolution: resolution_text(result.resolution()).to_owned(),
        query_sha256: hex(result.claim().query().as_bytes()),
        matches_returned: claim.returned_matches(),
        matches_total: claim.total_matches(),
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

fn mcp_relevant_paths_output(
    result: LocalRelevantPathsResult,
) -> Result<RelevantPathsOutput, String> {
    let (search, paths, returned_match_paths_total, returned_match_paths_truncated) =
        result.into_parts();
    let search = mcp_search_output(search)?;
    let paths = paths
        .as_slice()
        .iter()
        .map(|path| {
            mcp_interoperable(&[path.first_fact_ordinal()])?;
            Ok(McpRelevantPath {
                path: RepositoryPathTextV1::encode(path.path(), PATH_TEXT_LIMIT)
                    .map_err(|error| error.to_string())?
                    .into_string(),
                content_sha256: hex(path.content_digest().as_bytes()),
                matching_declarations: path.matching_declarations(),
                first_fact_ordinal: path.first_fact_ordinal(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let paths_returned = u64::try_from(paths.len())
        .map_err(|_| "relevant-path count cannot be represented safely".to_owned())?;
    mcp_interoperable(&[paths_returned, returned_match_paths_total])?;
    Ok(RelevantPathsOutput {
        schema_version: 1,
        path_ranking_profile: RELEVANT_PATHS_PROFILE_VERSION,
        snapshot_sha256: search.snapshot_sha256,
        generation: search.generation,
        resolution: search.resolution,
        query_sha256: search.query_sha256,
        matches_returned: search.matches_returned,
        matches_total: search.matches_total,
        paths_returned,
        returned_match_paths_total,
        returned_match_paths_truncated,
        coverage: search.coverage,
        limitations: vec![
            "indexed_supported_language_declaration_lexical_only".to_owned(),
            "ordered_by_returned_match_count_then_canonical_path".to_owned(),
            "path_summaries_cover_only_returned_declaration_matches".to_owned(),
            "no_relationship_or_semantic_relevance_claim".to_owned(),
        ],
        paths,
        matches: search.matches,
    })
}

fn mcp_interoperable(values: &[u64]) -> Result<(), String> {
    if values
        .iter()
        .any(|value| *value > MAX_MCP_INTEROPERABLE_INTEGER)
    {
        Err("MCP output exceeds the interoperable integer range".to_owned())
    } else {
        Ok(())
    }
}

fn mcp_interoperable_i64(values: &[i64]) -> Result<(), String> {
    if values.iter().any(|value| {
        *value <= 0 || u64::try_from(*value).ok() > Some(MAX_MCP_INTEROPERABLE_INTEGER)
    }) {
        Err("MCP output identity exceeds the interoperable integer range".to_owned())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod mcp_integer_output_tests {
    use super::{mcp_interoperable, mcp_interoperable_i64};
    use repowitness_mcp::MAX_MCP_INTEROPERABLE_INTEGER;

    #[test]
    fn output_rejects_values_that_json_clients_cannot_represent_exactly() {
        assert!(mcp_interoperable(&[MAX_MCP_INTEROPERABLE_INTEGER]).is_ok());
        assert!(mcp_interoperable(&[MAX_MCP_INTEROPERABLE_INTEGER + 1]).is_err());
        assert!(mcp_interoperable_i64(&[i64::try_from(MAX_MCP_INTEROPERABLE_INTEGER)
            .expect("interoperable maximum fits in i64")])
        .is_ok());
        assert!(mcp_interoperable_i64(&[i64::try_from(MAX_MCP_INTEROPERABLE_INTEGER + 1)
            .expect("one above the interoperable maximum fits in i64")])
        .is_err());
    }
}

fn mcp_symbol_search_output(result: LocalSymbolSearchResult) -> Result<SymbolSearchOutput, String> {
    let (result, connected_workspace, workspace_view, source_slot) = result.into_parts();
    interoperable_i64(&[workspace_view])?;
    let connected_workspace = ConnectedWorkspaceIdTextV1::encode(connected_workspace).into_string();
    let source_slot = SourceSlotIdTextV1::encode(source_slot).into_string();
    let mut matches = Vec::with_capacity(result.evidence().as_slice().len());
    for evidence in result.evidence().as_slice() {
        let EvidenceLocation::SymbolOccurrence(occurrence) = evidence.identity().location() else {
            return Err("symbol-search evidence location is invalid".to_owned());
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
        return Err("symbol-search evidence count is inconsistent".to_owned());
    }
    let coverage = result.coverage();
    Ok(SymbolSearchOutput {
        schema_version: 1,
        query_profile: SYMBOL_SEARCH_PROFILE_VERSION,
        connected_workspace,
        workspace_view,
        source_slot,
        snapshot_sha256: hex(result.snapshot().as_bytes()),
        generation: result.generation().get(),
        resolution: resolution_text(result.resolution()).to_owned(),
        query_sha256: hex(result.claim().query().as_bytes()),
        match_mode: result.claim().name_match().as_str().to_owned(),
        matches_returned: result.claim().returned_matches(),
        matches_total: result.claim().total_matches(),
        coverage: McpCoverage {
            searched: coverage.searched().get(),
            skipped: coverage.skipped().get(),
            unresolved: coverage.unresolved().get(),
            truncated: coverage.truncated().get(),
        },
        limitations: vec![
            "direct_syntax_declarations_only".to_owned(),
            "no_name_based_relationship_resolution".to_owned(),
        ],
        matches,
    })
}

fn mcp_raw_syntax_site_output(record: &OutboundSyntaxSite) -> Result<McpOutboundSyntaxSite, String> {
    let site = record.site();
    Ok(McpOutboundSyntaxSite {
        path: RepositoryPathTextV1::encode(record.path(), PATH_TEXT_LIMIT)
            .map_err(|error| error.to_string())?
            .into_string(),
        content_sha256: hex(record.content_digest().as_bytes()),
        artifact_sha256: hex(record.artifact_digest().as_bytes()),
        language: record.language().as_str().to_owned(),
        ordinal: site.ordinal().get(),
        kind: site.kind().as_str().to_owned(),
        evidence: site.evidence().as_str().to_owned(),
        occurrence_span: McpSpan {
            start: site.occurrence_span().start().get(),
            end: site.occurrence_span().end().get(),
        },
        target_span: McpSpan {
            start: site.target_span().start().get(),
            end: site.target_span().end().get(),
        },
        raw_target: site.raw_target().to_owned(),
        target_resolution: "not_attempted_no_resolution_profile".to_owned(),
    })
}

fn mcp_outbound_sites_output(
    result: LocalOutboundSitesResult,
) -> Result<OutboundSitesOutput, String> {
    let selector = result.selector();
    let selector = OutboundSitesSelectorOutput {
        path: RepositoryPathTextV1::encode(selector.path(), PATH_TEXT_LIMIT)
            .map_err(|error| error.to_string())?
            .into_string(),
        content_sha256: hex(selector.content_digest().as_bytes()),
        artifact_sha256: hex(selector.artifact_digest().as_bytes()),
        fact_ordinal: selector.fact_ordinal(),
    };
    let declaration = result.declaration().map(|declaration| McpOutboundSitesDeclaration {
        language: declaration.language().as_str().to_owned(),
        declaration_span: McpSpan {
            start: declaration.declaration_span().start().get(),
            end: declaration.declaration_span().end().get(),
        },
    });
    let sites = result
        .sites()
        .iter()
        .map(mcp_raw_syntax_site_output)
        .collect::<Result<Vec<_>, String>>()?;
    let sites_returned = u64::try_from(sites.len())
        .map_err(|_| "outbound-sites result count overflowed".to_owned())?;
    if sites_returned > result.total_sites() {
        return Err("outbound-sites result totals are inconsistent".to_owned());
    }
    let coverage = result.index_coverage();
    let availability = match result.availability() {
        OutboundSitesAvailability::Complete => "complete",
        OutboundSitesAvailability::NotProduced => "not_produced",
    };
    Ok(OutboundSitesOutput {
        schema_version: 1,
        outbound_sites_profile: OUTBOUND_SITES_PROFILE_VERSION,
        snapshot_sha256: hex(result.snapshot().as_bytes()),
        generation: result.generation().get(),
        selector,
        availability: availability.to_owned(),
        declaration,
        coverage: McpCoverage {
            searched: coverage.searched(),
            skipped: coverage.skipped(),
            unresolved: coverage.unresolved(),
            truncated: coverage.truncated(),
        },
        sites_returned,
        sites_total: result.total_sites(),
        truncated: sites_returned < result.total_sites(),
        output_bytes: result.output_bytes(),
        limitation: "raw_syntax_observations_only_no_target_resolution_or_inferred_edges".to_owned(),
        sites,
    })
}

fn mcp_syntax_site_search_output(
    result: LocalSyntaxSiteSearchResult,
) -> Result<SyntaxSiteSearchOutput, String> {
    let sites = result
        .sites()
        .iter()
        .map(mcp_raw_syntax_site_output)
        .collect::<Result<Vec<_>, String>>()?;
    let sites_returned = u64::try_from(sites.len())
        .map_err(|_| "syntax-site-search result count overflowed".to_owned())?;
    let claim = result.claim();
    if sites_returned != claim.returned_sites() || sites_returned > claim.total_sites() {
        return Err("syntax-site-search result totals are inconsistent".to_owned());
    }
    let coverage = result.index_coverage();
    let availability = match result.availability() {
        OutboundSitesAvailability::Complete => "complete",
        OutboundSitesAvailability::NotProduced => "not_produced",
    };
    Ok(SyntaxSiteSearchOutput {
        schema_version: 1,
        syntax_site_search_profile: SYNTAX_SITE_SEARCH_PROFILE_VERSION,
        target_sha256: hex(claim.query().as_bytes()),
        snapshot_sha256: hex(result.snapshot().as_bytes()),
        generation: result.generation().get(),
        availability: availability.to_owned(),
        coverage: McpCoverage {
            searched: coverage.searched(),
            skipped: coverage.skipped(),
            unresolved: coverage.unresolved(),
            truncated: coverage.truncated(),
        },
        sites_returned,
        sites_total: claim.total_sites(),
        truncated: sites_returned < claim.total_sites(),
        output_bytes: result.output_bytes(),
        limitation: "exact_raw_target_syntax_observations_only_no_target_resolution_or_inferred_edges"
            .to_owned(),
        sites,
    })
}

fn mcp_test_markers_output(
    result: LocalTestMarkersResult,
) -> Result<TestMarkersOutput, String> {
    let markers = result
        .markers()
        .iter()
        .map(mcp_raw_syntax_site_output)
        .collect::<Result<Vec<_>, String>>()?;
    let markers_returned = u64::try_from(markers.len())
        .map_err(|_| "test-marker result count overflowed".to_owned())?;
    if markers_returned > result.total_markers() {
        return Err("test-marker result totals are inconsistent".to_owned());
    }
    let availability = match result.availability() {
        TestMarkersAvailability::Complete => "complete",
        TestMarkersAvailability::NotProduced => "not_produced",
    };
    let coverage = result.index_coverage();
    let language_coverage = result
        .language_coverage()
        .iter()
        .map(|coverage| McpTestMarkerLanguageCoverage {
            language: coverage.language().as_str().to_owned(),
            indexed_files: coverage.indexed_files(),
            supported_files: coverage.supported_files(),
            unsupported_files: coverage.unsupported_files(),
            emitted_markers: coverage.emitted_markers(),
        })
        .collect();
    Ok(TestMarkersOutput {
        schema_version: 1,
        test_markers_profile: TEST_MARKERS_PROFILE_VERSION,
        snapshot_sha256: hex(result.snapshot().as_bytes()),
        generation: result.generation().get(),
        availability: availability.to_owned(),
        coverage: McpCoverage {
            searched: coverage.searched(),
            skipped: coverage.skipped(),
            unresolved: coverage.unresolved(),
            truncated: coverage.truncated(),
        },
        language_coverage,
        markers_returned,
        markers_total: result.total_markers(),
        truncated: result.truncated(),
        output_bytes: result.output_bytes(),
        limitation: "raw_syntax_observations_only_not_test_execution_or_relationship_resolution"
            .to_owned(),
        markers,
    })
}

fn mcp_code_graph_query_output(
    result: LocalCodeGraphQueryResult,
) -> Result<CodeGraphQueryOutput, String> {
    let result = match result {
        CodeGraphQueryResult::Symbols(_) => {
            return Err("code graph symbol results must be emitted through source-pinned symbol-search".to_owned());
        }
        CodeGraphQueryResult::OutboundSites(result) => {
            CodeGraphQueryResultOutput::OutboundSites(mcp_outbound_sites_output(result)?)
        }
        CodeGraphQueryResult::SyntaxSiteSearch(result) => {
            CodeGraphQueryResultOutput::SyntaxSiteSearch(mcp_syntax_site_search_output(result)?)
        }
        CodeGraphQueryResult::Architecture(result) => {
            CodeGraphQueryResultOutput::Architecture(mcp_architecture_overview_output(result)?)
        }
        CodeGraphQueryResult::Files(result) => {
            CodeGraphQueryResultOutput::Files(mcp_architecture_map_output(result)?)
        }
        CodeGraphQueryResult::TestMarkers(result) => {
            CodeGraphQueryResultOutput::TestMarkers(mcp_test_markers_output(result)?)
        }
        CodeGraphQueryResult::RelevantPaths(result) => {
            CodeGraphQueryResultOutput::RelevantPaths(mcp_relevant_paths_output(result)?)
        }
    };
    Ok(CodeGraphQueryOutput::new(result))
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
