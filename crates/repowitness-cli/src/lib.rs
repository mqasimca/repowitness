//! Testable command parsing and human-facing reports for the RepoWitness CLI.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::AtomicBool};
use std::time::{SystemTime, UNIX_EPOCH};

use repowitness_local::{
    CODE_SEARCH_PROFILE_VERSION, EvidenceLocation, GitPathDiscoveryLimits, GitPathDiscoveryStats,
    LocalCodeSearchRequest, LocalCodeSearchResult, LocalIndexReport, LocalIndexRequest,
    LocalSymbolGetRequest, LocalSymbolGetResult, LocalSymbolSelectorText, RepositoryIdentityTextV1,
    RepositoryPathTextByteLimit, RepositoryPathTextV1, ResolutionStatus,
    SYMBOL_GET_PROFILE_VERSION, discover_repository_paths, get_local_rust_symbol,
    index_local_rust_repository, search_local_rust_index,
};
use repowitness_mcp::{
    CodeSearchOutput, CodeSearchServiceRequest, McpCoverage, McpSearchMatch, McpSpan, McpSymbol,
    RepositoryService, RepositoryServiceError, SymbolGetOutput, SymbolGetServiceRequest,
    SymbolSelectorOutput, serve_stdio,
};

const EXIT_SUCCESS: u8 = 0;
const EXIT_USAGE: u8 = 64;
const EXIT_SOFTWARE: u8 = 70;
const EXIT_IO: u8 = 74;
const MAX_INDEX_ARGUMENTS: usize = 7;
const MAX_SEARCH_ARGUMENTS: usize = 8;
const MAX_SYMBOL_GET_ARGUMENTS: usize = 18;
const MAX_MCP_SERVE_ARGUMENTS: usize = 6;
const MAX_CLI_SEARCH_OUTPUT_BYTES: usize = 3 * 1024 * 1024;
const MAX_CLI_SYMBOL_OUTPUT_BYTES: usize = 20 * 1024 * 1024;
const PATH_TEXT_LIMIT: RepositoryPathTextByteLimit = RepositoryPathTextByteLimit::new(2_097_160);
const MCP_RUNTIME_WORKER_THREADS: usize = 2;
const MCP_RUNTIME_BLOCKING_THREADS: usize = 6;

const HELP: &str = concat!(
    "RepoWitness ",
    env!("CARGO_PKG_VERSION"),
    "\n\n",
    "Usage:\n",
    "  repowitness --help\n",
    "  repowitness --version\n",
    "  repowitness inspect-paths [--] <repository>\n",
    "  repowitness index --repository-id <id> --database <path> [--] <repository>\n",
    "  repowitness search --repository-id <id> --database <path> --query <text>\n",
    "  repowitness symbol-get <exact selector options>\n",
    "  repowitness mcp-serve --repository-id <id> --database <path> --root <path>\n\n",
    "Commands:\n",
    "  inspect-paths  Validate bounded Git path discovery without analyzing file\n",
    "                 contents or creating an index.\n",
    "  index          Build and atomically activate a bounded local Rust index.\n",
    "  search         Search active Rust symbol facts with attributed evidence.\n",
    "  symbol-get     Retrieve one exact verified declaration from search output.\n\n",
    "  mcp-serve      Serve code_search and symbol_get over bounded local stdio.\n",
);

const MCP_SERVE_HELP: &str = concat!(
    "Serve the active Phase 0 Rust index over local MCP stdio.\n\n",
    "Usage:\n",
    "  repowitness mcp-serve --repository-id <id> --database <path> --root <path>\n\n",
    "Stdout is reserved exclusively for newline-delimited MCP JSON-RPC. The\n",
    "configured repository, database, and identity are fixed for the process;\n",
    "tool callers cannot select arbitrary local paths. The server exposes only\n",
    "the read-only code_search and symbol_get tools.\n",
);

const INSPECT_HELP: &str = concat!(
    "Validate repository-path discovery without creating an index.\n\n",
    "Usage:\n",
    "  repowitness inspect-paths [--] <repository>\n\n",
    "The command invokes Git without a shell, applies fixed deadline and output\n",
    "bounds, validates exact path bytes, and prints only aggregate statistics.\n",
);

const INDEX_HELP: &str = concat!(
    "Build and atomically activate one local Phase 0 Rust index.\n\n",
    "Usage:\n",
    "  repowitness index --repository-id <id> --database <path> [--] <repository>\n\n",
    "The repository ID must use canonical rwi1:h: text. The database path is\n",
    "explicit and may be new or an existing RepoWitness SQLite index. The\n",
    "command prints only non-sensitive aggregate results.\n",
);

const SEARCH_HELP: &str = concat!(
    "Search one active Phase 0 Rust index with proof-carrying results.\n\n",
    "Usage:\n",
    "  repowitness search --repository-id <id> --database <path> --query <text>\n",
    "                     [--limit <1-100>]\n\n",
    "The query uses bounded literal terms; raw FTS syntax is never exposed. Paths\n",
    "are emitted with the canonical byte-preserving text encoding. Every result\n",
    "reports its snapshot, generation, producer, evidence spans, and coverage.\n",
);

const SYMBOL_GET_HELP: &str = concat!(
    "Retrieve one exact declaration selected from code-search output.\n\n",
    "Usage:\n",
    "  repowitness symbol-get --repository-id <id> --database <path> --root <path>\n",
    "      --snapshot <sha256> --generation <positive-id> --path <rwp1:h:text>\n",
    "      --content <sha256> --artifact <sha256> --fact <ordinal>\n\n",
    "The complete snapshot, generation, path, content, artifact, and fact selector\n",
    "must still identify the active occurrence. Source bytes are read through a\n",
    "no-follow contained root, verified against the indexed digest, and emitted\n",
    "as lowercase hexadecimal so terminal control bytes cannot be injected.\n",
);

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
        index_local_rust_repository(
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
        search_local_rust_index(request, Arc::new(AtomicBool::new(false)))
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
        get_local_rust_symbol(
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
        search_local_rust_index(local_request, cancelled)
            .map_err(|_| RepositoryServiceError::CodeSearch)
            .and_then(|result| {
                mcp_search_output(result).map_err(|_| RepositoryServiceError::CodeSearch)
            })
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
        get_local_rust_symbol(local_request, cancelled)
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
        schema_version: 1,
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
        limitation: "rust_symbol_lexical_only".to_owned(),
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
        schema_version: 1,
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
    skipped_non_rust_paths: u64,
    total_source_bytes: u64,
    total_facts: u64,
    syntax_error_nodes: u64,
    reused_rust_files: u64,
    analyzed_rust_files: u64,
}

impl From<LocalIndexReport> for CliIndexReport {
    fn from(report: LocalIndexReport) -> Self {
        Self {
            generation: report.generation().get(),
            source_epoch: report.source_epoch(),
            recovered_generations: report.recovered_generations(),
            discovered_paths: report.discovered_paths(),
            indexed_rust_files: report.indexed_rust_files(),
            skipped_non_rust_paths: report.skipped_non_rust_paths(),
            total_source_bytes: report.total_source_bytes(),
            total_facts: report.total_facts(),
            syntax_error_nodes: report.syntax_error_nodes(),
            reused_rust_files: report.reused_rust_files(),
            analyzed_rust_files: report.analyzed_rust_files(),
        }
    }
}

/// Parses and executes one CLI invocation with explicit output destinations.
///
/// The first argument is treated as the executable name. The returned value is
/// a process exit code: `0` for success, `64` for invalid usage, `70` for an
/// operation failure, and `74` for output failure. The `inspect-paths` command
/// is read-only and never creates an index.
pub fn run(args: impl IntoIterator<Item = OsString>, stdout: impl Write, stderr: impl Write) -> u8 {
    run_with_adapters(
        args,
        stdout,
        stderr,
        &LocalRepositoryPathInspector,
        &LocalRepositoryIndexer,
        &LocalRepositorySearcher,
        &LocalRepositorySymbolGetter,
    )
}

/// Parses and runs the process-level local stdio MCP command.
///
/// Stdout is owned exclusively by the MCP transport. Usage and lifecycle
/// diagnostics are written only to the supplied stderr destination.
pub fn run_mcp_server(args: impl IntoIterator<Item = OsString>, mut stderr: impl Write) -> u8 {
    let mut args = args.into_iter();
    let _program = args.next();
    if args.next().as_deref() != Some(OsStr::new("mcp-serve")) {
        return emit_error(
            &mut stderr,
            EXIT_USAGE,
            "error: expected mcp-serve command\n",
        );
    }
    let arguments: Vec<OsString> = args.take(MAX_MCP_SERVE_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_MCP_SERVE_ARGUMENTS {
        return emit_error(
            &mut stderr,
            EXIT_USAGE,
            "error: mcp-serve received too many arguments\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return if stderr.write_all(MCP_SERVE_HELP.as_bytes()).is_ok() {
            EXIT_SUCCESS
        } else {
            EXIT_IO
        };
    }
    let invocation = match parse_mcp_serve_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(&mut stderr, EXIT_USAGE, message),
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(MCP_RUNTIME_WORKER_THREADS)
        .max_blocking_threads(MCP_RUNTIME_BLOCKING_THREADS)
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            return emit_error(
                &mut stderr,
                EXIT_SOFTWARE,
                "error: MCP runtime initialization failed\n",
            );
        }
    };
    let service: Arc<dyn RepositoryService> = Arc::new(LocalMcpRepositoryService {
        root: invocation.root,
        database: invocation.database,
        repository_identity: invocation.repository_identity,
    });
    match runtime.block_on(serve_stdio(service)) {
        Ok(()) => EXIT_SUCCESS,
        Err(error) => {
            if writeln!(stderr, "error: {error}").is_ok() {
                EXIT_SOFTWARE
            } else {
                EXIT_IO
            }
        }
    }
}

struct McpServeInvocation {
    root: PathBuf,
    database: PathBuf,
    repository_identity: String,
}

fn parse_mcp_serve_arguments(arguments: &[OsString]) -> Result<McpServeInvocation, &'static str> {
    let mut root = None;
    let mut database = None;
    let mut repository_identity = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        let value = arguments
            .get(index + 1)
            .ok_or("error: mcp-serve option requires a value\n")?;
        if option == OsStr::new("--root") {
            if root.replace(PathBuf::from(value)).is_some() {
                return Err("error: mcp-serve accepts --root only once\n");
            }
        } else if option == OsStr::new("--database") {
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: mcp-serve accepts --database only once\n");
            }
        } else if option == OsStr::new("--repository-id") {
            if repository_identity.replace(value.clone()).is_some() {
                return Err("error: mcp-serve accepts --repository-id only once\n");
            }
        } else {
            return Err("error: unknown mcp-serve option; use mcp-serve --help\n");
        }
        index += 2;
    }

    let root = root.ok_or("error: mcp-serve requires --root\n")?;
    let database = database.ok_or("error: mcp-serve requires --database\n")?;
    let repository_identity =
        repository_identity.ok_or("error: mcp-serve requires --repository-id\n")?;
    if root.as_os_str().is_empty()
        || database.as_os_str().is_empty()
        || repository_identity.is_empty()
    {
        return Err("error: mcp-serve option values must not be empty\n");
    }
    let repository_identity = repository_identity
        .to_str()
        .ok_or("error: mcp-serve repository identity must be UTF-8\n")?;
    RepositoryIdentityTextV1::decode(repository_identity)
        .map_err(|_| "error: mcp-serve repository identity is invalid\n")?;
    Ok(McpServeInvocation {
        root,
        database,
        repository_identity: repository_identity.to_owned(),
    })
}

fn run_with_adapters(
    args: impl IntoIterator<Item = OsString>,
    mut stdout: impl Write,
    mut stderr: impl Write,
    inspector: &impl RepositoryPathInspector,
    indexer: &impl RepositoryIndexer,
    searcher: &impl RepositorySearcher,
    symbol_getter: &impl RepositorySymbolGetter,
) -> u8 {
    let mut args = args.into_iter();
    let _program = args.next();
    let Some(command) = args.next() else {
        return emit_error(
            &mut stderr,
            EXIT_USAGE,
            "error: no command supplied; use --help\n",
        );
    };

    if command == OsStr::new("--help") || command == OsStr::new("-h") {
        if args.next().is_some() {
            return emit_error(
                &mut stderr,
                EXIT_USAGE,
                "error: --help accepts no additional arguments\n",
            );
        }
        return emit_output(&mut stdout, HELP);
    }
    if command == OsStr::new("--version") || command == OsStr::new("-V") {
        if args.next().is_some() {
            return emit_error(
                &mut stderr,
                EXIT_USAGE,
                "error: --version accepts no additional arguments\n",
            );
        }
        return emit_version(&mut stdout);
    }
    if command == OsStr::new("inspect-paths") {
        return run_inspect_paths(args, &mut stdout, &mut stderr, inspector);
    }
    if command == OsStr::new("index") {
        return run_index(args, &mut stdout, &mut stderr, indexer);
    }
    if command == OsStr::new("search") {
        return run_search(args, &mut stdout, &mut stderr, searcher);
    }
    if command == OsStr::new("symbol-get") {
        return run_symbol_get(args, &mut stdout, &mut stderr, symbol_getter);
    }

    emit_error(
        &mut stderr,
        EXIT_USAGE,
        "error: unknown command; use --help\n",
    )
}

fn run_search(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    searcher: &impl RepositorySearcher,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_SEARCH_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_SEARCH_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: search received too many arguments; use search --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, SEARCH_HELP);
    }
    let invocation = match parse_search_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match searcher.search(&invocation) {
        Ok(report) => emit_search_report(stdout, &report),
        Err(error) => {
            if writeln!(stderr, "error: code search failed: {error}").is_err() {
                EXIT_IO
            } else {
                EXIT_SOFTWARE
            }
        }
    }
}

fn parse_search_arguments(arguments: &[OsString]) -> Result<SearchInvocation, &'static str> {
    let mut repository_identity = None;
    let mut database = None;
    let mut query = None;
    let mut max_results = 20_u16;
    let mut limit_seen = false;
    let mut index = 0_usize;

    while index < arguments.len() {
        let option = &arguments[index];
        index += 1;
        if option == OsStr::new("--help") || option == OsStr::new("-h") {
            return Err("error: search --help accepts no additional arguments\n");
        }
        let value = arguments
            .get(index)
            .ok_or("error: search option requires a value; use search --help\n")?;
        index += 1;
        if option == OsStr::new("--repository-id") {
            if repository_identity.replace(value.clone()).is_some() {
                return Err("error: search accepts --repository-id only once\n");
            }
        } else if option == OsStr::new("--database") {
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: search accepts --database only once\n");
            }
        } else if option == OsStr::new("--query") {
            if query.replace(value.clone()).is_some() {
                return Err("error: search accepts --query only once\n");
            }
        } else if option == OsStr::new("--limit") {
            if limit_seen {
                return Err("error: search accepts --limit only once\n");
            }
            max_results = value
                .to_str()
                .and_then(|text| text.parse::<u16>().ok())
                .filter(|limit| (1..=100).contains(limit))
                .ok_or("error: search --limit must be an integer from 1 through 100\n")?;
            limit_seen = true;
        } else {
            return Err("error: unknown search option; use search --help\n");
        }
    }

    let repository_identity =
        repository_identity.ok_or("error: search requires --repository-id; use search --help\n")?;
    if repository_identity.is_empty() {
        return Err("error: search repository identity must not be empty\n");
    }
    let database = database.ok_or("error: search requires --database; use search --help\n")?;
    if database.as_os_str().is_empty() {
        return Err("error: search database path must not be empty\n");
    }
    let query = query.ok_or("error: search requires --query; use search --help\n")?;
    if query.is_empty() {
        return Err("error: search query must not be empty\n");
    }
    Ok(SearchInvocation {
        database,
        repository_identity,
        query,
        max_results,
    })
}

fn run_symbol_get(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    getter: &impl RepositorySymbolGetter,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_SYMBOL_GET_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_SYMBOL_GET_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: symbol-get received too many arguments; use symbol-get --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, SYMBOL_GET_HELP);
    }
    let invocation = match parse_symbol_get_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match getter.get(&invocation) {
        Ok(report) => emit_symbol_report(stdout, &report),
        Err(error) => {
            if writeln!(stderr, "error: symbol get failed: {error}").is_err() {
                EXIT_IO
            } else {
                EXIT_SOFTWARE
            }
        }
    }
}

#[derive(Default)]
struct SymbolInvocationBuilder {
    root: Option<PathBuf>,
    database: Option<PathBuf>,
    repository_identity: Option<OsString>,
    snapshot: Option<OsString>,
    generation: Option<OsString>,
    path: Option<OsString>,
    content: Option<OsString>,
    artifact: Option<OsString>,
    fact_ordinal: Option<OsString>,
}

impl SymbolInvocationBuilder {
    fn set(&mut self, option: &OsStr, value: &OsStr) -> Result<(), &'static str> {
        if option == OsStr::new("--root") {
            set_once(&mut self.root, PathBuf::from(value), "root")
        } else if option == OsStr::new("--database") {
            set_once(&mut self.database, PathBuf::from(value), "database")
        } else if option == OsStr::new("--repository-id") {
            set_once(
                &mut self.repository_identity,
                value.to_owned(),
                "repository-id",
            )
        } else if option == OsStr::new("--snapshot") {
            set_once(&mut self.snapshot, value.to_owned(), "snapshot")
        } else if option == OsStr::new("--generation") {
            set_once(&mut self.generation, value.to_owned(), "generation")
        } else if option == OsStr::new("--path") {
            set_once(&mut self.path, value.to_owned(), "path")
        } else if option == OsStr::new("--content") {
            set_once(&mut self.content, value.to_owned(), "content")
        } else if option == OsStr::new("--artifact") {
            set_once(&mut self.artifact, value.to_owned(), "artifact")
        } else if option == OsStr::new("--fact") {
            set_once(&mut self.fact_ordinal, value.to_owned(), "fact")
        } else {
            Err("error: unknown symbol-get option; use symbol-get --help\n")
        }
    }

    fn finish(self) -> Result<SymbolInvocation, &'static str> {
        let root = required(self.root, "error: symbol-get requires --root\n")?;
        let database = required(self.database, "error: symbol-get requires --database\n")?;
        let repository_identity = required(
            self.repository_identity,
            "error: symbol-get requires --repository-id\n",
        )?;
        let snapshot = required(self.snapshot, "error: symbol-get requires --snapshot\n")?;
        let generation_text =
            required(self.generation, "error: symbol-get requires --generation\n")?;
        let path = required(self.path, "error: symbol-get requires --path\n")?;
        let content = required(self.content, "error: symbol-get requires --content\n")?;
        let artifact = required(self.artifact, "error: symbol-get requires --artifact\n")?;
        let fact_text = required(self.fact_ordinal, "error: symbol-get requires --fact\n")?;
        validate_symbol_text(&root, &database, &repository_identity, &path)?;
        let generation = parse_positive_i64(&generation_text)?;
        let fact_ordinal = parse_u64(&fact_text)?;
        Ok(SymbolInvocation {
            root,
            database,
            repository_identity,
            snapshot,
            generation,
            path,
            content,
            artifact,
            fact_ordinal,
        })
    }
}

fn parse_symbol_get_arguments(arguments: &[OsString]) -> Result<SymbolInvocation, &'static str> {
    let mut builder = SymbolInvocationBuilder::default();
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        if option == OsStr::new("--help") || option == OsStr::new("-h") {
            return Err("error: symbol-get --help accepts no additional arguments\n");
        }
        let value = arguments
            .get(index + 1)
            .ok_or("error: symbol-get option requires a value; use symbol-get --help\n")?;
        builder.set(option, value)?;
        index += 2;
    }
    builder.finish()
}

fn set_once<T>(slot: &mut Option<T>, value: T, name: &'static str) -> Result<(), &'static str> {
    if slot.replace(value).is_some() {
        match name {
            "root" => Err("error: symbol-get accepts --root only once\n"),
            "database" => Err("error: symbol-get accepts --database only once\n"),
            "repository-id" => Err("error: symbol-get accepts --repository-id only once\n"),
            "snapshot" => Err("error: symbol-get accepts --snapshot only once\n"),
            "generation" => Err("error: symbol-get accepts --generation only once\n"),
            "path" => Err("error: symbol-get accepts --path only once\n"),
            "content" => Err("error: symbol-get accepts --content only once\n"),
            "artifact" => Err("error: symbol-get accepts --artifact only once\n"),
            "fact" => Err("error: symbol-get accepts --fact only once\n"),
            _ => Err("error: duplicate symbol-get option\n"),
        }
    } else {
        Ok(())
    }
}

fn required<T>(value: Option<T>, message: &'static str) -> Result<T, &'static str> {
    value.ok_or(message)
}

fn validate_symbol_text(
    root: &Path,
    database: &Path,
    repository_identity: &OsStr,
    path: &OsStr,
) -> Result<(), &'static str> {
    if root.as_os_str().is_empty()
        || database.as_os_str().is_empty()
        || repository_identity.is_empty()
        || path.is_empty()
    {
        Err("error: symbol-get option values must not be empty\n")
    } else {
        Ok(())
    }
}

fn parse_positive_i64(value: &OsStr) -> Result<i64, &'static str> {
    value
        .to_str()
        .and_then(|text| text.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .ok_or("error: symbol-get --generation must be a positive integer\n")
}

fn parse_u64(value: &OsStr) -> Result<u64, &'static str> {
    value
        .to_str()
        .and_then(|text| text.parse::<u64>().ok())
        .ok_or("error: symbol-get --fact must be a non-negative integer\n")
}

fn run_index(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    indexer: &impl RepositoryIndexer,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_INDEX_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_INDEX_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: index received too many arguments; use index --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, INDEX_HELP);
    }

    let invocation = match parse_index_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match indexer.index(&invocation) {
        Ok(report) => emit_index_report(stdout, report),
        Err(error) => {
            if writeln!(stderr, "error: indexing failed: {error}").is_err() {
                EXIT_IO
            } else {
                EXIT_SOFTWARE
            }
        }
    }
}

fn parse_index_arguments(arguments: &[OsString]) -> Result<IndexInvocation, &'static str> {
    let mut repository_identity = None;
    let mut database = None;
    let mut repository_root = None;
    let mut positional_only = false;
    let mut index = 0_usize;

    while index < arguments.len() {
        let argument = &arguments[index];
        if positional_only {
            set_repository_root(&mut repository_root, argument)?;
            index += 1;
            continue;
        }
        if argument == OsStr::new("--") {
            positional_only = true;
            index += 1;
            continue;
        }
        if argument == OsStr::new("--repository-id") {
            index += 1;
            let value = arguments
                .get(index)
                .ok_or("error: index --repository-id requires a value; use index --help\n")?;
            if repository_identity.replace(value.clone()).is_some() {
                return Err("error: index accepts --repository-id only once\n");
            }
            index += 1;
            continue;
        }
        if argument == OsStr::new("--database") {
            index += 1;
            let value = arguments
                .get(index)
                .ok_or("error: index --database requires a path; use index --help\n")?;
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: index accepts --database only once\n");
            }
            index += 1;
            continue;
        }
        if argument == OsStr::new("--help") || argument == OsStr::new("-h") {
            return Err("error: index --help accepts no additional arguments\n");
        }
        if os_string_starts_with_hyphen(argument) {
            return Err("error: unknown index option; use index --help\n");
        }
        set_repository_root(&mut repository_root, argument)?;
        index += 1;
    }

    let repository_identity =
        repository_identity.ok_or("error: index requires --repository-id; use index --help\n")?;
    if repository_identity.is_empty() {
        return Err("error: index repository identity must not be empty\n");
    }
    let database = database.ok_or("error: index requires --database; use index --help\n")?;
    if database.as_os_str().is_empty() {
        return Err("error: index database path must not be empty\n");
    }
    let repository_root =
        repository_root.ok_or("error: index requires one repository; use index --help\n")?;

    Ok(IndexInvocation {
        repository_root,
        database,
        repository_identity,
    })
}

fn set_repository_root(
    repository_root: &mut Option<PathBuf>,
    value: &OsStr,
) -> Result<(), &'static str> {
    if value.is_empty() {
        return Err("error: index repository must not be empty\n");
    }
    if repository_root.replace(PathBuf::from(value)).is_some() {
        return Err("error: index accepts exactly one repository\n");
    }
    Ok(())
}

fn run_inspect_paths(
    mut args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    inspector: &impl RepositoryPathInspector,
) -> u8 {
    let Some(first) = args.next() else {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: inspect-paths requires one repository; use inspect-paths --help\n",
        );
    };
    if first == OsStr::new("--help") || first == OsStr::new("-h") {
        if args.next().is_none() {
            return emit_output(stdout, INSPECT_HELP);
        }
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: inspect-paths --help accepts no additional arguments\n",
        );
    }

    let root = if first == OsStr::new("--") {
        let Some(root) = args.next() else {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: inspect-paths requires a repository after --\n",
            );
        };
        root
    } else {
        if os_string_starts_with_hyphen(&first) {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: unknown inspect-paths option; use -- before a repository beginning with '-'\n",
            );
        }
        first
    };

    if root.is_empty() {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: inspect-paths repository must not be empty\n",
        );
    }
    if args.next().is_some() {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: inspect-paths accepts exactly one repository\n",
        );
    }

    match inspector.inspect(Path::new(&root)) {
        Ok(stats) => emit_inspection_report(stdout, stats),
        Err(error) => {
            if writeln!(stderr, "error: repository path inspection failed: {error}").is_err() {
                EXIT_IO
            } else {
                EXIT_SOFTWARE
            }
        }
    }
}

fn os_string_starts_with_hyphen(value: &OsStr) -> bool {
    value.as_encoded_bytes().first() == Some(&b'-')
}

fn emit_inspection_report(writer: &mut impl Write, stats: GitPathDiscoveryStats) -> u8 {
    let result = writeln!(writer, "status=ok")
        .and_then(|()| writeln!(writer, "operation=inspect-paths"))
        .and_then(|()| writeln!(writer, "index_created=false"))
        .and_then(|()| writeln!(writer, "git_output_bytes={}", stats.output_bytes()))
        .and_then(|()| writeln!(writer, "repository_paths={}", stats.path_count()))
        .and_then(|()| {
            writeln!(
                writer,
                "total_repository_path_bytes={}",
                stats.total_path_bytes()
            )
        })
        .and_then(|()| {
            writeln!(
                writer,
                "longest_repository_path_bytes={}",
                stats.longest_path_bytes()
            )
        })
        .and_then(|()| {
            writeln!(
                writer,
                "maximum_repository_path_components={}",
                stats.most_components()
            )
        });
    if result.is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn emit_index_report(writer: &mut impl Write, report: CliIndexReport) -> u8 {
    let result = writeln!(writer, "status=ok")
        .and_then(|()| writeln!(writer, "operation=index"))
        .and_then(|()| writeln!(writer, "generation_activated=true"))
        .and_then(|()| writeln!(writer, "generation={}", report.generation))
        .and_then(|()| writeln!(writer, "source_epoch={}", report.source_epoch))
        .and_then(|()| {
            writeln!(
                writer,
                "recovered_generations={}",
                report.recovered_generations
            )
        })
        .and_then(|()| writeln!(writer, "repository_paths={}", report.discovered_paths))
        .and_then(|()| writeln!(writer, "indexed_rust_files={}", report.indexed_rust_files))
        .and_then(|()| writeln!(writer, "reused_rust_files={}", report.reused_rust_files))
        .and_then(|()| writeln!(writer, "analyzed_rust_files={}", report.analyzed_rust_files))
        .and_then(|()| {
            writeln!(
                writer,
                "skipped_non_rust_paths={}",
                report.skipped_non_rust_paths
            )
        })
        .and_then(|()| writeln!(writer, "total_source_bytes={}", report.total_source_bytes))
        .and_then(|()| writeln!(writer, "symbol_facts={}", report.total_facts))
        .and_then(|()| writeln!(writer, "syntax_error_nodes={}", report.syntax_error_nodes));
    if result.is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn emit_search_report(writer: &mut impl Write, report: &CliSearchReport) -> u8 {
    let mut encoded = Vec::new();
    if write_search_report(&mut encoded, report).is_err()
        || encoded.len() > MAX_CLI_SEARCH_OUTPUT_BYTES
    {
        return EXIT_SOFTWARE;
    }
    if writer.write_all(&encoded).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn write_search_report(writer: &mut impl Write, report: &CliSearchReport) -> std::io::Result<()> {
    writeln!(writer, "status=ok")
        .and_then(|()| writeln!(writer, "operation=search"))
        .and_then(|()| writeln!(writer, "query_profile={CODE_SEARCH_PROFILE_VERSION}"))
        .and_then(|()| writeln!(writer, "query_sha256={}", report.query_digest))
        .and_then(|()| writeln!(writer, "snapshot_sha256={}", report.snapshot))
        .and_then(|()| writeln!(writer, "generation={}", report.generation))
        .and_then(|()| writeln!(writer, "resolution={}", report.resolution))
        .and_then(|()| writeln!(writer, "matches_returned={}", report.returned_matches))
        .and_then(|()| writeln!(writer, "matches_total={}", report.total_matches))
        .and_then(|()| writeln!(writer, "coverage_searched={}", report.searched))
        .and_then(|()| writeln!(writer, "coverage_skipped={}", report.skipped))
        .and_then(|()| writeln!(writer, "coverage_unresolved={}", report.unresolved))
        .and_then(|()| writeln!(writer, "coverage_truncated={}", report.truncated))
        .and_then(|()| writeln!(writer, "limitation=rust_symbol_lexical_only"))?;
    for (index, candidate) in report.matches.iter().enumerate() {
        emit_search_match(writer, index, candidate)?;
    }
    Ok(())
}

fn emit_search_match(
    writer: &mut impl Write,
    index: usize,
    candidate: &CliSearchMatch,
) -> std::io::Result<()> {
    writeln!(writer, "match_{index}_path={}", candidate.path)?;
    writeln!(
        writer,
        "match_{index}_fact_ordinal={}",
        candidate.fact_ordinal
    )?;
    writeln!(
        writer,
        "match_{index}_content_sha256={}",
        candidate.content_digest
    )?;
    writeln!(
        writer,
        "match_{index}_artifact_sha256={}",
        candidate.artifact_digest
    )?;
    writeln!(
        writer,
        "match_{index}_producer_manifest_sha256={}",
        candidate.producer_manifest
    )?;
    writeln!(writer, "match_{index}_evidence_tier=syntax")?;
    writeln!(writer, "match_{index}_kind={}", candidate.kind)?;
    writeln!(writer, "match_{index}_name={}", candidate.name)?;
    writeln!(
        writer,
        "match_{index}_qualified_name={}",
        candidate.qualified_name
    )?;
    writeln!(
        writer,
        "match_{index}_name_span={}:{}",
        candidate.name_start, candidate.name_end
    )?;
    writeln!(
        writer,
        "match_{index}_declaration_span={}:{}",
        candidate.declaration_start, candidate.declaration_end
    )
}

fn emit_symbol_report(writer: &mut impl Write, report: &CliSymbolReport) -> u8 {
    let mut encoded = Vec::new();
    if write_symbol_report(&mut encoded, report).is_err()
        || encoded.len() > MAX_CLI_SYMBOL_OUTPUT_BYTES
    {
        return EXIT_SOFTWARE;
    }
    if writer.write_all(&encoded).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn write_symbol_report(writer: &mut impl Write, report: &CliSymbolReport) -> std::io::Result<()> {
    writeln!(writer, "status=ok")?;
    writeln!(writer, "operation=symbol-get")?;
    writeln!(writer, "symbol_profile={SYMBOL_GET_PROFILE_VERSION}")?;
    writeln!(writer, "snapshot_sha256={}", report.snapshot)?;
    writeln!(writer, "generation={}", report.generation)?;
    writeln!(writer, "resolution={}", report.resolution)?;
    writeln!(writer, "path={}", report.path)?;
    writeln!(writer, "content_sha256={}", report.content_digest)?;
    writeln!(writer, "artifact_sha256={}", report.artifact_digest)?;
    writeln!(writer, "fact_ordinal={}", report.fact_ordinal)?;
    writeln!(writer, "coverage_searched={}", report.searched)?;
    writeln!(writer, "coverage_skipped={}", report.skipped)?;
    writeln!(writer, "coverage_unresolved={}", report.unresolved)?;
    writeln!(writer, "coverage_truncated={}", report.truncated)?;
    writeln!(writer, "limitation=definition_only_no_references")?;
    writeln!(writer, "symbol_found={}", report.symbol.is_some())?;
    if let Some(symbol) = &report.symbol {
        write_symbol_data(writer, symbol)?;
    }
    Ok(())
}

fn write_symbol_data(writer: &mut impl Write, symbol: &CliSymbolData) -> std::io::Result<()> {
    writeln!(
        writer,
        "producer_manifest_sha256={}",
        symbol.producer_manifest
    )?;
    writeln!(writer, "evidence_tier=syntax")?;
    writeln!(writer, "kind={}", symbol.kind)?;
    writeln!(writer, "name={}", symbol.name)?;
    writeln!(writer, "qualified_name={}", symbol.qualified_name)?;
    writeln!(
        writer,
        "name_span={}:{}",
        symbol.name_start, symbol.name_end
    )?;
    writeln!(
        writer,
        "declaration_span={}:{}",
        symbol.declaration_start, symbol.declaration_end
    )?;
    writeln!(writer, "declaration_encoding=lowercase_hex")?;
    writeln!(writer, "declaration_hex={}", symbol.declaration_hex)
}

fn emit_version(writer: &mut impl Write) -> u8 {
    if writeln!(writer, "repowitness {}", env!("CARGO_PKG_VERSION")).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn emit_output(writer: &mut impl Write, message: &str) -> u8 {
    if writer.write_all(message.as_bytes()).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn emit_error(writer: &mut impl Write, code: u8, message: &str) -> u8 {
    if writer.write_all(message.as_bytes()).is_ok() {
        code
    } else {
        EXIT_IO
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::io;
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn mcp_serve_arguments_are_complete_canonical_and_order_independent() {
        let identity = format!("rwi1:h:{}", "AB".repeat(32));
        let arguments = [
            OsString::from("--root"),
            OsString::from("../repository"),
            OsString::from("--repository-id"),
            OsString::from(&identity),
            OsString::from("--database"),
            OsString::from("../index.db"),
        ];
        let invocation = parse_mcp_serve_arguments(&arguments).expect("valid configuration");
        assert_eq!(invocation.root, Path::new("../repository"));
        assert_eq!(invocation.database, Path::new("../index.db"));
        assert_eq!(invocation.repository_identity, identity);
    }

    #[test]
    fn mcp_serve_rejects_invalid_configuration_without_starting_a_runtime() {
        let valid_identity = format!("rwi1:h:{}", "AB".repeat(32));
        for arguments in [
            vec![],
            vec!["--root", "private"],
            vec![
                "--root",
                "private",
                "--root",
                "other",
                "--database",
                "index.db",
            ],
            vec![
                "--root",
                "private",
                "--database",
                "index.db",
                "--repository-id",
                "invalid",
            ],
            vec![
                "--root",
                "private",
                "--database",
                "index.db",
                "--unknown",
                &valid_identity,
            ],
        ] {
            let arguments = arguments
                .into_iter()
                .map(OsString::from)
                .collect::<Vec<_>>();
            assert!(parse_mcp_serve_arguments(&arguments).is_err());
        }
    }

    #[test]
    fn mcp_serve_help_uses_only_the_diagnostic_stream() {
        let mut stderr = Vec::new();
        let code = run_mcp_server(
            [
                OsString::from("repowitness"),
                OsString::from("mcp-serve"),
                OsString::from("--help"),
            ],
            &mut stderr,
        );
        assert_eq!(code, EXIT_SUCCESS);
        let help = String::from_utf8(stderr).expect("help is UTF-8");
        assert!(help.contains("Stdout is reserved exclusively"));
        assert!(help.contains("code_search and symbol_get"));
    }

    struct FakeInspector {
        outcome: FakeOutcome,
        calls: Cell<u64>,
        root: RefCell<Option<PathBuf>>,
    }

    #[derive(Clone, Copy)]
    enum FakeOutcome {
        Success(GitPathDiscoveryStats),
        Failure(&'static str),
    }

    struct FakeIndexer {
        outcome: FakeIndexOutcome,
        calls: Cell<u64>,
        repository_root: RefCell<Option<PathBuf>>,
        database: RefCell<Option<PathBuf>>,
        repository_identity: RefCell<Option<OsString>>,
    }

    #[derive(Clone, Copy)]
    enum FakeIndexOutcome {
        Success(CliIndexReport),
        Failure(&'static str),
    }

    struct FakeSearcher {
        outcome: RefCell<Option<Result<CliSearchReport, &'static str>>>,
        calls: Cell<u64>,
        database: RefCell<Option<PathBuf>>,
        repository_identity: RefCell<Option<OsString>>,
        query: RefCell<Option<OsString>>,
        max_results: Cell<Option<u16>>,
    }

    struct FakeSymbolGetter {
        outcome: RefCell<Option<Result<CliSymbolReport, &'static str>>>,
        calls: Cell<u64>,
        root: RefCell<Option<PathBuf>>,
        database: RefCell<Option<PathBuf>>,
        repository_identity: RefCell<Option<OsString>>,
        snapshot: RefCell<Option<OsString>>,
        path: RefCell<Option<OsString>>,
        content: RefCell<Option<OsString>>,
        artifact: RefCell<Option<OsString>>,
        generation: Cell<Option<i64>>,
        fact_ordinal: Cell<Option<u64>>,
    }

    impl FakeInspector {
        fn success(stats: GitPathDiscoveryStats) -> Self {
            Self {
                outcome: FakeOutcome::Success(stats),
                calls: Cell::new(0),
                root: RefCell::new(None),
            }
        }

        fn failure(error: &'static str) -> Self {
            Self {
                outcome: FakeOutcome::Failure(error),
                calls: Cell::new(0),
                root: RefCell::new(None),
            }
        }
    }

    impl RepositoryPathInspector for FakeInspector {
        fn inspect(&self, root: &Path) -> Result<GitPathDiscoveryStats, String> {
            self.calls.set(self.calls.get() + 1);
            self.root.replace(Some(root.to_owned()));
            match self.outcome {
                FakeOutcome::Success(stats) => Ok(stats),
                FakeOutcome::Failure(error) => Err(error.to_owned()),
            }
        }
    }

    impl FakeIndexer {
        fn success(report: CliIndexReport) -> Self {
            Self {
                outcome: FakeIndexOutcome::Success(report),
                calls: Cell::new(0),
                repository_root: RefCell::new(None),
                database: RefCell::new(None),
                repository_identity: RefCell::new(None),
            }
        }

        fn failure(error: &'static str) -> Self {
            Self {
                outcome: FakeIndexOutcome::Failure(error),
                calls: Cell::new(0),
                repository_root: RefCell::new(None),
                database: RefCell::new(None),
                repository_identity: RefCell::new(None),
            }
        }
    }

    impl RepositoryIndexer for FakeIndexer {
        fn index(&self, invocation: &IndexInvocation) -> Result<CliIndexReport, String> {
            self.calls.set(self.calls.get() + 1);
            self.repository_root
                .replace(Some(invocation.repository_root.clone()));
            self.database.replace(Some(invocation.database.clone()));
            self.repository_identity
                .replace(Some(invocation.repository_identity.clone()));
            match self.outcome {
                FakeIndexOutcome::Success(report) => Ok(report),
                FakeIndexOutcome::Failure(error) => Err(error.to_owned()),
            }
        }
    }

    impl FakeSearcher {
        fn success(report: CliSearchReport) -> Self {
            Self {
                outcome: RefCell::new(Some(Ok(report))),
                calls: Cell::new(0),
                database: RefCell::new(None),
                repository_identity: RefCell::new(None),
                query: RefCell::new(None),
                max_results: Cell::new(None),
            }
        }

        fn failure(error: &'static str) -> Self {
            Self {
                outcome: RefCell::new(Some(Err(error))),
                calls: Cell::new(0),
                database: RefCell::new(None),
                repository_identity: RefCell::new(None),
                query: RefCell::new(None),
                max_results: Cell::new(None),
            }
        }
    }

    impl RepositorySearcher for FakeSearcher {
        fn search(&self, invocation: &SearchInvocation) -> Result<CliSearchReport, String> {
            self.calls.set(self.calls.get() + 1);
            self.database.replace(Some(invocation.database.clone()));
            self.repository_identity
                .replace(Some(invocation.repository_identity.clone()));
            self.query.replace(Some(invocation.query.clone()));
            self.max_results.set(Some(invocation.max_results));
            self.outcome
                .borrow_mut()
                .take()
                .expect("fake searcher should be called at most once")
                .map_err(str::to_owned)
        }
    }

    impl FakeSymbolGetter {
        fn success(report: CliSymbolReport) -> Self {
            Self {
                outcome: RefCell::new(Some(Ok(report))),
                calls: Cell::new(0),
                root: RefCell::new(None),
                database: RefCell::new(None),
                repository_identity: RefCell::new(None),
                snapshot: RefCell::new(None),
                path: RefCell::new(None),
                content: RefCell::new(None),
                artifact: RefCell::new(None),
                generation: Cell::new(None),
                fact_ordinal: Cell::new(None),
            }
        }

        fn failure(error: &'static str) -> Self {
            Self {
                outcome: RefCell::new(Some(Err(error))),
                calls: Cell::new(0),
                root: RefCell::new(None),
                database: RefCell::new(None),
                repository_identity: RefCell::new(None),
                snapshot: RefCell::new(None),
                path: RefCell::new(None),
                content: RefCell::new(None),
                artifact: RefCell::new(None),
                generation: Cell::new(None),
                fact_ordinal: Cell::new(None),
            }
        }
    }

    impl RepositorySymbolGetter for FakeSymbolGetter {
        fn get(&self, invocation: &SymbolInvocation) -> Result<CliSymbolReport, String> {
            self.calls.set(self.calls.get() + 1);
            self.root.replace(Some(invocation.root.clone()));
            self.database.replace(Some(invocation.database.clone()));
            self.repository_identity
                .replace(Some(invocation.repository_identity.clone()));
            self.snapshot.replace(Some(invocation.snapshot.clone()));
            self.path.replace(Some(invocation.path.clone()));
            self.content.replace(Some(invocation.content.clone()));
            self.artifact.replace(Some(invocation.artifact.clone()));
            self.generation.set(Some(invocation.generation));
            self.fact_ordinal.set(Some(invocation.fact_ordinal));
            self.outcome
                .borrow_mut()
                .take()
                .expect("fake symbol getter should be called at most once")
                .map_err(str::to_owned)
        }
    }

    fn index_report() -> CliIndexReport {
        CliIndexReport {
            generation: 3,
            source_epoch: 0,
            recovered_generations: 1,
            discovered_paths: 5,
            indexed_rust_files: 2,
            skipped_non_rust_paths: 3,
            total_source_bytes: 101,
            total_facts: 7,
            syntax_error_nodes: 0,
            reused_rust_files: 1,
            analyzed_rust_files: 1,
        }
    }

    fn search_report() -> CliSearchReport {
        CliSearchReport {
            generation: 9,
            snapshot: "11".repeat(32),
            resolution: "confirmed",
            query_digest: "22".repeat(32),
            returned_matches: 1,
            total_matches: 3,
            searched: 8,
            skipped: 2,
            unresolved: 1,
            truncated: 2,
            matches: vec![CliSearchMatch {
                path: "rwp1:h:7372632F6C69622E7273".to_owned(),
                fact_ordinal: 7,
                content_digest: "33".repeat(32),
                artifact_digest: "44".repeat(32),
                producer_manifest: "55".repeat(32),
                kind: "function",
                name: "run".to_owned(),
                qualified_name: "fixture::run".to_owned(),
                name_start: 7,
                name_end: 10,
                declaration_start: 0,
                declaration_end: 13,
            }],
        }
    }

    fn symbol_report() -> CliSymbolReport {
        CliSymbolReport {
            generation: 9,
            snapshot: "11".repeat(32),
            resolution: "confirmed",
            path: "rwp1:h:7372632F6C69622E7273".to_owned(),
            content_digest: "33".repeat(32),
            artifact_digest: "44".repeat(32),
            fact_ordinal: 7,
            searched: 8,
            skipped: 2,
            unresolved: 1,
            truncated: 0,
            symbol: Some(CliSymbolData {
                producer_manifest: "55".repeat(32),
                kind: "function",
                name: "run".to_owned(),
                qualified_name: "fixture::run".to_owned(),
                name_start: 7,
                name_end: 10,
                declaration_start: 0,
                declaration_end: 13,
                declaration_hex: "70756220666e2072756e2829207b7d".to_owned(),
            }),
        }
    }

    fn invoke(
        arguments: &[&str],
        inspector: &impl RepositoryPathInspector,
    ) -> (u8, String, String) {
        invoke_with_adapters(
            arguments,
            inspector,
            &FakeIndexer::failure("must not be called"),
            &FakeSearcher::failure("must not be called"),
        )
    }

    fn invoke_with_adapters(
        arguments: &[&str],
        inspector: &impl RepositoryPathInspector,
        indexer: &impl RepositoryIndexer,
        searcher: &impl RepositorySearcher,
    ) -> (u8, String, String) {
        invoke_with_symbol_adapter(
            arguments,
            inspector,
            indexer,
            searcher,
            &FakeSymbolGetter::failure("must not be called"),
        )
    }

    fn invoke_with_symbol_adapter(
        arguments: &[&str],
        inspector: &impl RepositoryPathInspector,
        indexer: &impl RepositoryIndexer,
        searcher: &impl RepositorySearcher,
        symbol_getter: &impl RepositorySymbolGetter,
    ) -> (u8, String, String) {
        let args = std::iter::once(OsString::from("repowitness"))
            .chain(arguments.iter().map(OsString::from));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run_with_adapters(
            args,
            &mut stdout,
            &mut stderr,
            inspector,
            indexer,
            searcher,
            symbol_getter,
        );
        (
            code,
            String::from_utf8(stdout).expect("test stdout is UTF-8"),
            String::from_utf8(stderr).expect("test stderr is UTF-8"),
        )
    }

    #[test]
    fn help_and_version_are_successful_and_truthful() {
        let inspector = FakeInspector::failure("must not be called");
        let (code, stdout, stderr) = invoke(&["--help"], &inspector);
        assert_eq!(code, EXIT_SUCCESS);
        assert!(stdout.contains("index          Build"));
        assert!(stdout.contains("search         Search"));
        assert!(stdout.contains("--repository-id"));
        assert!(stderr.is_empty());
        assert_eq!(inspector.calls.get(), 0);

        let (code, stdout, stderr) = invoke(&["-h"], &inspector);
        assert_eq!(code, EXIT_SUCCESS);
        assert!(stdout.contains("Usage:"));
        assert!(stderr.is_empty());

        let (code, stdout, stderr) = invoke(&["--version"], &inspector);
        assert_eq!(code, EXIT_SUCCESS);
        assert_eq!(
            stdout,
            concat!("repowitness ", env!("CARGO_PKG_VERSION"), "\n")
        );
        assert!(stderr.is_empty());
        assert_eq!(inspector.calls.get(), 0);

        let (code, stdout, stderr) = invoke(&["-V"], &inspector);
        assert_eq!(code, EXIT_SUCCESS);
        assert!(stdout.starts_with("repowitness "));
        assert!(stderr.is_empty());
    }

    #[test]
    fn global_help_and_version_reject_additional_arguments() {
        let inspector = FakeInspector::failure("must not be called");
        for arguments in [
            ["--help", "unexpected"],
            ["-h", "unexpected"],
            ["--version", "unexpected"],
            ["-V", "unexpected"],
        ] {
            let (code, stdout, stderr) = invoke(&arguments, &inspector);
            assert_eq!(code, EXIT_USAGE);
            assert!(stdout.is_empty());
            assert!(stderr.starts_with("error:"));
        }
        assert_eq!(inspector.calls.get(), 0);
    }

    #[test]
    fn no_command_and_unknown_commands_are_usage_errors_without_echoing_input() {
        let inspector = FakeInspector::failure("must not be called");
        let (code, stdout, stderr) = invoke(&[], &inspector);
        assert_eq!(code, EXIT_USAGE);
        assert!(stdout.is_empty());
        assert!(stderr.contains("no command supplied"));

        let (code, stdout, stderr) = invoke(&["private-command-name"], &inspector);
        assert_eq!(code, EXIT_USAGE);
        assert!(stdout.is_empty());
        assert!(stderr.contains("unknown command"));
        assert!(!stderr.contains("private-command-name"));
        assert_eq!(inspector.calls.get(), 0);
    }

    #[test]
    fn index_requires_complete_arguments_without_invoking_adapters() {
        let inspector = FakeInspector::failure("must not be called");
        let indexer = FakeIndexer::failure("must not be called");
        let searcher = FakeSearcher::failure("must not be called");
        for arguments in [
            vec!["index"],
            vec!["index", "../repository"],
            vec!["index", "--repository", "../repository"],
            vec![
                "index",
                "--repository-id",
                "rwi1:h:00",
                "--database",
                "",
                "../repository",
            ],
            vec![
                "index",
                "--repository-id",
                "",
                "--database",
                "index.db",
                "../repository",
            ],
            vec![
                "index",
                "--repository-id",
                "first",
                "--repository-id",
                "second",
            ],
            vec!["index", "--database", "first.db", "--database", "second.db"],
            vec!["index", "--database"],
            vec!["index", "--repository-id"],
            vec!["index", "--help", "unexpected"],
            vec![
                "index", "one", "two", "three", "four", "five", "six", "seven", "eight",
            ],
        ] {
            let (code, stdout, stderr) =
                invoke_with_adapters(&arguments, &inspector, &indexer, &searcher);
            assert_eq!(code, EXIT_USAGE);
            assert!(stdout.is_empty());
            assert!(stderr.starts_with("error:"));
        }
        assert_eq!(inspector.calls.get(), 0);
        assert_eq!(indexer.calls.get(), 0);
        assert_eq!(searcher.calls.get(), 0);
    }

    #[test]
    fn index_success_reports_aggregates_and_passes_explicit_inputs() {
        let inspector = FakeInspector::failure("must not be called");
        let indexer = FakeIndexer::success(index_report());
        let identity = concat!(
            "rwi1:h:",
            "0101010101010101010101010101010101010101010101010101010101010101"
        );
        let (code, stdout, stderr) = invoke_with_adapters(
            &[
                "index",
                "--database",
                "../private-index.db",
                "--repository-id",
                identity,
                "--",
                "-private-repository",
            ],
            &inspector,
            &indexer,
            &FakeSearcher::failure("must not be called"),
        );

        assert_eq!(code, EXIT_SUCCESS);
        assert_eq!(
            stdout,
            concat!(
                "status=ok\n",
                "operation=index\n",
                "generation_activated=true\n",
                "generation=3\n",
                "source_epoch=0\n",
                "recovered_generations=1\n",
                "repository_paths=5\n",
                "indexed_rust_files=2\n",
                "reused_rust_files=1\n",
                "analyzed_rust_files=1\n",
                "skipped_non_rust_paths=3\n",
                "total_source_bytes=101\n",
                "symbol_facts=7\n",
                "syntax_error_nodes=0\n",
            )
        );
        assert!(stderr.is_empty());
        assert_eq!(inspector.calls.get(), 0);
        assert_eq!(indexer.calls.get(), 1);
        assert_eq!(
            indexer.repository_root.borrow().as_deref(),
            Some(Path::new("-private-repository"))
        );
        assert_eq!(
            indexer.database.borrow().as_deref(),
            Some(Path::new("../private-index.db"))
        );
        assert_eq!(
            indexer.repository_identity.borrow().as_deref(),
            Some(OsStr::new(identity))
        );
        assert!(!stdout.contains("private"));
        assert!(!stdout.contains(identity));
    }

    #[test]
    fn index_failures_are_nonzero_and_redacted_by_the_adapter() {
        let inspector = FakeInspector::failure("must not be called");
        let indexer = FakeIndexer::failure("local Rust index preparation failed");
        let identity = concat!(
            "rwi1:h:",
            "0202020202020202020202020202020202020202020202020202020202020202"
        );
        let (code, stdout, stderr) = invoke_with_adapters(
            &[
                "index",
                "--repository-id",
                identity,
                "--database",
                "../private-index.db",
                "../private-repository",
            ],
            &inspector,
            &indexer,
            &FakeSearcher::failure("must not be called"),
        );

        assert_eq!(code, EXIT_SOFTWARE);
        assert!(stdout.is_empty());
        assert_eq!(
            stderr,
            "error: indexing failed: local Rust index preparation failed\n"
        );
        assert!(!stderr.contains("private"));
        assert!(!stderr.contains(identity));
    }

    #[test]
    fn search_requires_bounded_complete_arguments_without_invoking_adapters() {
        let inspector = FakeInspector::failure("must not be called");
        let indexer = FakeIndexer::failure("must not be called");
        let searcher = FakeSearcher::failure("must not be called");
        for arguments in [
            vec!["search"],
            vec!["search", "--database", "index.db"],
            vec![
                "search",
                "--repository-id",
                "id",
                "--database",
                "index.db",
                "--query",
                "",
            ],
            vec!["search", "--limit", "0"],
            vec!["search", "--limit", "101"],
            vec!["search", "--limit", "private"],
            vec!["search", "--query"],
            vec!["search", "--unknown", "private"],
            vec!["search", "--help", "unexpected"],
            vec![
                "search",
                "--query",
                "x",
                "--query",
                "y",
                "--database",
                "index.db",
                "--repository-id",
                "id",
            ],
        ] {
            let (code, stdout, stderr) =
                invoke_with_adapters(&arguments, &inspector, &indexer, &searcher);
            assert_eq!(code, EXIT_USAGE);
            assert!(stdout.is_empty());
            assert!(stderr.starts_with("error:"));
            assert!(!stderr.contains("private"));
        }
        assert_eq!(inspector.calls.get(), 0);
        assert_eq!(indexer.calls.get(), 0);
        assert_eq!(searcher.calls.get(), 0);
    }

    #[test]
    fn search_reports_evidence_coverage_and_passes_explicit_inputs() {
        let inspector = FakeInspector::failure("must not be called");
        let indexer = FakeIndexer::failure("must not be called");
        let searcher = FakeSearcher::success(search_report());
        let identity = concat!(
            "rwi1:h:",
            "0606060606060606060606060606060606060606060606060606060606060606"
        );
        let (code, stdout, stderr) = invoke_with_adapters(
            &[
                "search",
                "--query",
                "private query",
                "--limit",
                "7",
                "--database",
                "../private-index.db",
                "--repository-id",
                identity,
            ],
            &inspector,
            &indexer,
            &searcher,
        );

        assert_eq!(code, EXIT_SUCCESS);
        assert!(stderr.is_empty());
        assert!(stdout.contains("status=ok\noperation=search\n"));
        assert!(stdout.contains("query_profile=1\n"));
        assert!(stdout.contains("generation=9\n"));
        assert!(stdout.contains("resolution=confirmed\n"));
        assert!(stdout.contains("matches_returned=1\nmatches_total=3\n"));
        assert!(stdout.contains("coverage_skipped=2\n"));
        assert!(stdout.contains("coverage_truncated=2\n"));
        assert!(stdout.contains("limitation=rust_symbol_lexical_only\n"));
        assert!(stdout.contains("match_0_path=rwp1:h:7372632F6C69622E7273\n"));
        assert!(stdout.contains("match_0_fact_ordinal=7\n"));
        assert!(stdout.contains("match_0_evidence_tier=syntax\n"));
        assert!(stdout.contains("match_0_qualified_name=fixture::run\n"));
        assert!(stdout.contains("match_0_name_span=7:10\n"));
        assert!(!stdout.contains("private query"));
        assert!(!stdout.contains("../private-index.db"));
        assert!(!stdout.contains(identity));
        assert_eq!(searcher.calls.get(), 1);
        assert_eq!(
            searcher.database.borrow().as_deref(),
            Some(Path::new("../private-index.db"))
        );
        assert_eq!(
            searcher.repository_identity.borrow().as_deref(),
            Some(OsStr::new(identity))
        );
        assert_eq!(
            searcher.query.borrow().as_deref(),
            Some(OsStr::new("private query"))
        );
        assert_eq!(searcher.max_results.get(), Some(7));
    }

    #[test]
    fn search_failures_are_nonzero_and_do_not_echo_inputs() {
        let inspector = FakeInspector::failure("must not be called");
        let indexer = FakeIndexer::failure("must not be called");
        let searcher = FakeSearcher::failure("local code search failed");
        let identity = concat!(
            "rwi1:h:",
            "0707070707070707070707070707070707070707070707070707070707070707"
        );
        let (code, stdout, stderr) = invoke_with_adapters(
            &[
                "search",
                "--repository-id",
                identity,
                "--database",
                "../private-index.db",
                "--query",
                "private query",
            ],
            &inspector,
            &indexer,
            &searcher,
        );

        assert_eq!(code, EXIT_SOFTWARE);
        assert!(stdout.is_empty());
        assert_eq!(
            stderr,
            "error: code search failed: local code search failed\n"
        );
        assert!(!stderr.contains("private"));
        assert!(!stderr.contains(identity));
    }

    #[test]
    fn search_boundary_rejects_an_oversized_encoded_report() {
        let mut report = search_report();
        report.matches[0].name = "x".repeat(MAX_CLI_SEARCH_OUTPUT_BYTES);
        assert_eq!(emit_search_report(&mut io::sink(), &report), EXIT_SOFTWARE);
    }

    #[test]
    fn symbol_get_reports_verified_source_and_passes_the_complete_selector() {
        let inspector = FakeInspector::failure("must not be called");
        let indexer = FakeIndexer::failure("must not be called");
        let searcher = FakeSearcher::failure("must not be called");
        let getter = FakeSymbolGetter::success(symbol_report());
        let identity = format!("rwi1:h:{}", "06".repeat(32));
        let snapshot = "11".repeat(32);
        let content = "33".repeat(32);
        let artifact = "44".repeat(32);
        let (code, stdout, stderr) = invoke_with_symbol_adapter(
            &[
                "symbol-get",
                "--artifact",
                &artifact,
                "--fact",
                "7",
                "--root",
                "../private-repository",
                "--content",
                &content,
                "--path",
                "rwp1:h:7372632F6C69622E7273",
                "--generation",
                "9",
                "--snapshot",
                &snapshot,
                "--database",
                "../private-index.db",
                "--repository-id",
                &identity,
            ],
            &inspector,
            &indexer,
            &searcher,
            &getter,
        );

        assert_eq!(code, EXIT_SUCCESS);
        assert!(stderr.is_empty());
        assert!(stdout.contains("status=ok\noperation=symbol-get\n"));
        assert!(stdout.contains("symbol_profile=1\n"));
        assert!(stdout.contains("resolution=confirmed\n"));
        assert!(stdout.contains("fact_ordinal=7\n"));
        assert!(stdout.contains("symbol_found=true\n"));
        assert!(stdout.contains("evidence_tier=syntax\n"));
        assert!(stdout.contains("name=run\n"));
        assert!(stdout.contains("declaration_encoding=lowercase_hex\n"));
        assert!(stdout.contains("declaration_hex=70756220666e2072756e2829207b7d\n"));
        assert!(!stdout.contains("private"));
        assert!(!stdout.contains(&identity));
        assert_eq!(getter.calls.get(), 1);
        assert_eq!(
            getter.root.borrow().as_deref(),
            Some(Path::new("../private-repository"))
        );
        assert_eq!(
            getter.database.borrow().as_deref(),
            Some(Path::new("../private-index.db"))
        );
        assert_eq!(getter.generation.get(), Some(9));
        assert_eq!(getter.fact_ordinal.get(), Some(7));
        assert_eq!(
            getter.snapshot.borrow().as_deref(),
            Some(OsStr::new(&snapshot))
        );
        assert_eq!(
            getter.path.borrow().as_deref(),
            Some(OsStr::new("rwp1:h:7372632F6C69622E7273"))
        );
        assert_eq!(
            getter.content.borrow().as_deref(),
            Some(OsStr::new(&content))
        );
        assert_eq!(
            getter.artifact.borrow().as_deref(),
            Some(OsStr::new(&artifact))
        );
    }

    #[test]
    fn symbol_get_rejects_incomplete_or_invalid_selectors_before_io() {
        let inspector = FakeInspector::failure("must not be called");
        let indexer = FakeIndexer::failure("must not be called");
        let searcher = FakeSearcher::failure("must not be called");
        let getter = FakeSymbolGetter::failure("must not be called");
        for arguments in [
            vec!["symbol-get"],
            vec!["symbol-get", "--root", "private"],
            vec!["symbol-get", "--unknown", "private"],
            vec![
                "symbol-get",
                "--root",
                "a",
                "--root",
                "b",
                "--database",
                "private",
            ],
        ] {
            let (code, stdout, stderr) =
                invoke_with_symbol_adapter(&arguments, &inspector, &indexer, &searcher, &getter);
            assert_eq!(code, EXIT_USAGE);
            assert!(stdout.is_empty());
            assert!(stderr.starts_with("error:"));
            assert!(!stderr.contains("private"));
        }
        assert_eq!(getter.calls.get(), 0);
    }

    #[test]
    fn symbol_get_rejects_invalid_numeric_selector_parts_before_io() {
        let inspector = FakeInspector::failure("must not be called");
        let indexer = FakeIndexer::failure("must not be called");
        let searcher = FakeSearcher::failure("must not be called");
        let getter = FakeSymbolGetter::failure("must not be called");
        let mut arguments = [
            "symbol-get",
            "--root",
            "root",
            "--database",
            "index.db",
            "--repository-id",
            "rwi1:h:0606060606060606060606060606060606060606060606060606060606060606",
            "--snapshot",
            "1111111111111111111111111111111111111111111111111111111111111111",
            "--generation",
            "0",
            "--path",
            "rwp1:h:7372632F6C69622E7273",
            "--content",
            "3333333333333333333333333333333333333333333333333333333333333333",
            "--artifact",
            "4444444444444444444444444444444444444444444444444444444444444444",
            "--fact",
            "7",
        ];
        let (code, _, _) =
            invoke_with_symbol_adapter(&arguments, &inspector, &indexer, &searcher, &getter);
        assert_eq!(code, EXIT_USAGE);

        arguments[10] = "9";
        arguments[18] = "-1";
        let (code, _, _) =
            invoke_with_symbol_adapter(&arguments, &inspector, &indexer, &searcher, &getter);
        assert_eq!(code, EXIT_USAGE);
        assert_eq!(getter.calls.get(), 0);
    }

    #[test]
    fn symbol_get_failures_do_not_leak_inputs() {
        let inspector = FakeInspector::failure("must not be called");
        let indexer = FakeIndexer::failure("must not be called");
        let searcher = FakeSearcher::failure("must not be called");
        let getter = FakeSymbolGetter::failure("local symbol retrieval failed");
        let identity = format!("rwi1:h:{}", "07".repeat(32));
        let digest = "88".repeat(32);
        let (code, stdout, stderr) = invoke_with_symbol_adapter(
            &[
                "symbol-get",
                "--root",
                "private-root",
                "--database",
                "private.db",
                "--repository-id",
                &identity,
                "--snapshot",
                &digest,
                "--generation",
                "1",
                "--path",
                "rwp1:h:70726976617465",
                "--content",
                &digest,
                "--artifact",
                &digest,
                "--fact",
                "0",
            ],
            &inspector,
            &indexer,
            &searcher,
            &getter,
        );
        assert_eq!(code, EXIT_SOFTWARE);
        assert!(stdout.is_empty());
        assert_eq!(
            stderr,
            "error: symbol get failed: local symbol retrieval failed\n"
        );
        assert!(!stderr.contains("private"));
        assert!(!stderr.contains(&identity));
        assert!(!stderr.contains(&digest));
    }

    #[test]
    fn symbol_get_boundary_rejects_an_oversized_encoded_report() {
        let mut report = symbol_report();
        report
            .symbol
            .as_mut()
            .expect("fixture has a symbol")
            .declaration_hex = "0".repeat(MAX_CLI_SYMBOL_OUTPUT_BYTES);
        assert_eq!(emit_symbol_report(&mut io::sink(), &report), EXIT_SOFTWARE);
    }

    #[test]
    fn symbol_get_output_failure_returns_the_io_exit_code() {
        assert_eq!(
            emit_symbol_report(&mut FailingWriter, &symbol_report()),
            EXIT_IO
        );
    }

    #[test]
    fn index_and_inspection_help_do_not_run_repository_io() {
        let inspector = FakeInspector::failure("must not be called");
        let (code, stdout, stderr) = invoke(&["index", "--help"], &inspector);
        assert_eq!(code, EXIT_SUCCESS);
        assert!(stdout.contains("atomically activate"));
        assert!(stdout.contains("--repository-id"));
        assert!(stderr.is_empty());

        let (code, stdout, stderr) = invoke(&["search", "--help"], &inspector);
        assert_eq!(code, EXIT_SUCCESS);
        assert!(stdout.contains("proof-carrying results"));
        assert!(stdout.contains("--limit <1-100>"));
        assert!(stderr.is_empty());

        let (code, stdout, stderr) = invoke(&["symbol-get", "--help"], &inspector);
        assert_eq!(code, EXIT_SUCCESS);
        assert!(stdout.contains("exact declaration"));
        assert!(stdout.contains("lowercase hexadecimal"));
        assert!(stderr.is_empty());

        let (code, stdout, stderr) = invoke(&["inspect-paths", "--help"], &inspector);
        assert_eq!(code, EXIT_SUCCESS);
        assert!(stdout.contains("without creating an index"));
        assert!(stderr.is_empty());

        let (code, stdout, stderr) = invoke(&["index", "-h"], &inspector);
        assert_eq!(code, EXIT_SUCCESS);
        assert!(stdout.contains("--database"));
        assert!(stderr.is_empty());

        let (code, stdout, stderr) = invoke(&["inspect-paths", "-h"], &inspector);
        assert_eq!(code, EXIT_SUCCESS);
        assert!(stdout.contains("without creating an index"));
        assert!(stderr.is_empty());
        assert_eq!(inspector.calls.get(), 0);
    }

    #[test]
    fn inspection_success_reports_only_deterministic_aggregates() {
        let inspector = FakeInspector::success(GitPathDiscoveryStats::new(22, 2, 20, 10, 2));
        let (code, stdout, stderr) = invoke(&["inspect-paths", "../private-repo"], &inspector);
        assert_eq!(code, EXIT_SUCCESS);
        assert_eq!(
            stdout,
            concat!(
                "status=ok\n",
                "operation=inspect-paths\n",
                "index_created=false\n",
                "git_output_bytes=22\n",
                "repository_paths=2\n",
                "total_repository_path_bytes=20\n",
                "longest_repository_path_bytes=10\n",
                "maximum_repository_path_components=2\n",
            )
        );
        assert!(stderr.is_empty());
        assert_eq!(inspector.calls.get(), 1);
        assert_eq!(
            inspector.root.borrow().as_deref(),
            Some(Path::new("../private-repo"))
        );
        assert!(!stdout.contains("private-repo"));

        let inspector = FakeInspector::success(GitPathDiscoveryStats::new(22, 2, 20, 10, 2));
        let (code, stdout, stderr) = invoke(&["inspect-paths", "--", "-private-repo"], &inspector);
        assert_eq!(code, EXIT_SUCCESS);
        assert!(stdout.contains("index_created=false"));
        assert!(stderr.is_empty());
        assert_eq!(
            inspector.root.borrow().as_deref(),
            Some(Path::new("-private-repo"))
        );
    }

    #[test]
    fn inspection_failures_are_nonzero_and_do_not_print_the_root() {
        let inspector = FakeInspector::failure("Git exited unsuccessfully with code 128");
        let (code, stdout, stderr) = invoke(&["inspect-paths", "../private-repo"], &inspector);
        assert_eq!(code, EXIT_SOFTWARE);
        assert!(stdout.is_empty());
        assert!(stderr.contains("Git exited unsuccessfully with code 128"));
        assert!(!stderr.contains("private-repo"));
    }

    #[test]
    fn inspection_argument_errors_do_not_invoke_repository_io() {
        let inspector = FakeInspector::failure("must not be called");
        for arguments in [
            vec!["inspect-paths"],
            vec!["inspect-paths", "--"],
            vec!["inspect-paths", "--unknown"],
            vec!["inspect-paths", ""],
            vec!["inspect-paths", "one", "two"],
            vec!["inspect-paths", "--help", "extra"],
        ] {
            let (code, stdout, stderr) = invoke(&arguments, &inspector);
            assert_eq!(code, EXIT_USAGE);
            assert!(stdout.is_empty());
            assert!(stderr.starts_with("error:"));
        }
        assert_eq!(inspector.calls.get(), 0);
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("intentional test failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("intentional test failure"))
        }
    }

    #[test]
    fn output_failures_return_the_io_exit_code() {
        let inspector = FakeInspector::failure("must not be called");
        let indexer = FakeIndexer::failure("must not be called");
        let searcher = FakeSearcher::failure("must not be called");
        let symbol_getter = FakeSymbolGetter::failure("must not be called");
        let code = run_with_adapters(
            [OsString::from("repowitness"), OsString::from("--help")],
            FailingWriter,
            io::sink(),
            &inspector,
            &indexer,
            &searcher,
            &symbol_getter,
        );
        assert_eq!(code, EXIT_IO);

        let code = run_with_adapters(
            [OsString::from("repowitness")],
            io::sink(),
            FailingWriter,
            &inspector,
            &indexer,
            &searcher,
            &symbol_getter,
        );
        assert_eq!(code, EXIT_IO);

        let code = run_with_adapters(
            [OsString::from("repowitness"), OsString::from("--version")],
            FailingWriter,
            io::sink(),
            &inspector,
            &indexer,
            &searcher,
            &symbol_getter,
        );
        assert_eq!(code, EXIT_IO);

        let success = FakeInspector::success(GitPathDiscoveryStats::new(1, 0, 0, 0, 0));
        let code = run_with_adapters(
            [
                OsString::from("repowitness"),
                OsString::from("inspect-paths"),
                OsString::from("repository"),
            ],
            FailingWriter,
            io::sink(),
            &success,
            &indexer,
            &searcher,
            &symbol_getter,
        );
        assert_eq!(code, EXIT_IO);

        let failure = FakeInspector::failure("expected test failure");
        let code = run_with_adapters(
            [
                OsString::from("repowitness"),
                OsString::from("inspect-paths"),
                OsString::from("repository"),
            ],
            io::sink(),
            FailingWriter,
            &failure,
            &indexer,
            &searcher,
            &symbol_getter,
        );
        assert_eq!(code, EXIT_IO);

        let successful_indexer = FakeIndexer::success(index_report());
        let code = run_with_adapters(
            [
                OsString::from("repowitness"),
                OsString::from("index"),
                OsString::from("--repository-id"),
                OsString::from(concat!(
                    "rwi1:h:",
                    "0303030303030303030303030303030303030303030303030303030303030303"
                )),
                OsString::from("--database"),
                OsString::from("index.db"),
                OsString::from("repository"),
            ],
            FailingWriter,
            io::sink(),
            &inspector,
            &successful_indexer,
            &searcher,
            &symbol_getter,
        );
        assert_eq!(code, EXIT_IO);

        let mut writer = FailingWriter;
        assert!(writer.flush().is_err());
    }

    #[test]
    fn search_output_failure_returns_the_io_exit_code() {
        let inspector = FakeInspector::failure("must not be called");
        let indexer = FakeIndexer::failure("must not be called");
        let searcher = FakeSearcher::success(search_report());
        let symbol_getter = FakeSymbolGetter::failure("must not be called");
        let code = run_with_adapters(
            [
                OsString::from("repowitness"),
                OsString::from("search"),
                OsString::from("--repository-id"),
                OsString::from(concat!(
                    "rwi1:h:",
                    "0404040404040404040404040404040404040404040404040404040404040404"
                )),
                OsString::from("--database"),
                OsString::from("index.db"),
                OsString::from("--query"),
                OsString::from("run"),
            ],
            FailingWriter,
            io::sink(),
            &inspector,
            &indexer,
            &searcher,
            &symbol_getter,
        );
        assert_eq!(code, EXIT_IO);
    }
}
