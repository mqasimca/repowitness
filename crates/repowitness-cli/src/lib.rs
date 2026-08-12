//! Testable command parsing and human-facing reports for the RepoWitness CLI.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::AtomicBool};
use std::time::{SystemTime, UNIX_EPOCH};

use repowitness_local::{
    ARCHITECTURE_MAP_PROFILE_VERSION, ARCHITECTURE_OVERVIEW_PROFILE_VERSION, ArchitectureMapFile,
    ArchitectureMapLimits, ArchitectureOverviewEntryPointCandidate, ArchitectureOverviewLimits,
    ArchitectureOverviewSourceRoot, CHANGE_MANIFEST_PROFILE_VERSION, CODE_SEARCH_PROFILE_VERSION,
    CodeGraphQueryOperation, CodeGraphQueryResult, ConfigurationFileLayer, ConfigurationLayer,
    ConfigurationLayerKind, ConnectedWorkspaceIdTextV1, DEFAULT_ARCHITECTURE_MAP_FILES,
    DEFAULT_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES, DEFAULT_ARCHITECTURE_OVERVIEW_FILES,
    DEFAULT_ARCHITECTURE_OVERVIEW_ROOTS, DEFAULT_EVIDENCE_CONTEXT_BUDGET_UNITS,
    DEFAULT_LOCAL_CONNECTED_WORKSPACE_DEADLINE, DEFAULT_LOCAL_EVIDENCE_CONTEXT_PROVIDER_RESULTS,
    EvidenceContextCandidate, EvidenceContextTier, EvidenceLocation, GeneratedLocalIdentity,
    GitObjectId, GitPathDiscoveryLimits, GitPathDiscoveryStats, IndexedContext,
    LocalArchitectureMapRequest, LocalArchitectureMapResult, LocalArchitectureOverviewRequest,
    LocalArchitectureOverviewResult, LocalChangeReviewReceipt, LocalChangeReviewRequest,
    LocalCodeGraphQueryRequest, LocalCodeGraphQueryResult, LocalCodeSearchRequest,
    LocalCodeSearchResult, LocalConnectedWorkspaceIndexRequest, LocalDoctorReport,
    LocalDoctorTargets, LocalEvidenceContextBuildRequest, LocalEvidenceContextItem,
    LocalIdentityGenerationError, LocalIdentityKind, LocalIndexReport, LocalIndexRequest,
    LocalMemoryApprovalRequest, LocalMemoryCorrespondenceReviewRequest,
    LocalMemoryDatabaseIdentity, LocalMemoryHistoryImportRequest, LocalMemoryMaintenance,
    LocalMemoryMaintenanceStep, LocalMemoryManageError, LocalMemoryMutation,
    LocalMemoryRecallRequest, LocalMemoryRecallResult, LocalMemoryRecallSelection,
    LocalMemoryRevalidationError, LocalMemoryRevalidationMutation, LocalMemoryRevalidationReport,
    LocalMemoryRevalidationRequest, LocalMemoryWriteRequest, LocalOutboundSitesRequest,
    LocalOutboundSitesResult, LocalRelevantPathsRequest, LocalRelevantPathsResult,
    LocalRepositoryDiagnosticsRequest, LocalRepositoryDiagnosticsResult,
    LocalRepositoryTopologyRequest, LocalRetentionApplyReport, LocalRetentionApplyRequest,
    LocalRetentionPins, LocalRetentionPlanReport, LocalRetentionPlanRequest,
    LocalRustGraphReadOutput, LocalRustGraphReadRequest, LocalRustGraphReadResult,
    LocalSymbolGetRequest, LocalSymbolGetResult, LocalSymbolSearchRequest, LocalSymbolSearchResult,
    LocalSymbolSelectorText, LocalSyntaxSiteSearchRequest, LocalSyntaxSiteSearchResult,
    LocalTeamMemorySyncRequest, LocalTestMarkersRequest, LocalTestMarkersResult, LocalWatchExit,
    LocalWatchReconciliation, LocalWatchReport, LocalWatchRequest, MAX_ARCHITECTURE_MAP_FILES,
    MAX_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES, MAX_ARCHITECTURE_OVERVIEW_FILES,
    MAX_ARCHITECTURE_OVERVIEW_ROOTS, MAX_CONFIGURATION_FILE_BYTES,
    MAX_EVIDENCE_CONTEXT_BUDGET_UNITS, McpToolProfile, MemoryAssurance, MemoryCommitId,
    MemoryCorrespondenceReviewOperation, MemoryEffectiveState, MemoryFileIdentityStatus,
    MemoryFilePublicationStepStatus, MemoryKind, MemoryLifecycle, MemoryObjectFormat,
    MemoryProjectionValidityState, MemoryRecallCandidateRelation, MemoryRecallEvidence,
    MemoryRecallEvidenceAssurance, MemoryRecallEvidenceOutcome, MemoryRecallEvidenceState,
    MemoryRecallOccurrence, MemoryRecallReason, MemoryRecallRecord, MemoryRecordIdTextV1,
    MemoryRevalidationTarget, OUTBOUND_SITES_PROFILE_VERSION, OutboundSitesAvailability,
    OutboundSyntaxSite, PolicyValue, RELEVANT_PATHS_PROFILE_VERSION, RepositoryIdentityTextV1,
    RepositoryPathTextByteLimit, RepositoryPathTextV1, ResolutionStatus, ResolvedConfiguration,
    ResolvedPreference, ResolvedToolProfilePreference, RustGraphAvailability,
    RustGraphCandidateRecord, RustGraphDefinitionRecord, RustGraphEvidenceResult,
    RustGraphImpactClass, RustGraphOutcomeRecord, RustGraphPublicationSummary,
    RustGraphSiteSelector, RustGraphTraceResult, RustSymbolKind, SYMBOL_GET_PROFILE_VERSION,
    SYMBOL_SEARCH_PROFILE_VERSION, SYNTAX_SITE_SEARCH_PROFILE_VERSION, SourceLanguage,
    SourceSlotIdTextV1, SymbolSearchNameMatch, SyntaxSiteSearchLimits, SyntaxSiteSearchQuery,
    TEST_MARKERS_PROFILE_VERSION, TestMarkersAvailability, TestMarkersLimits, TestMarkersQuery,
    apply_local_retention, approve_local_memory, build_local_change_review,
    build_local_evidence_context, diagnose_local_repository, discover_repository_paths,
    generate_local_identity, get_local_outbound_sites, get_local_symbol,
    import_local_memory_history, index_local_connected_workspace, index_local_repository,
    inspect_local_doctor, locate_local_relevant_paths, map_local_architecture,
    overview_local_architecture, parse_configuration_file, plan_local_retention,
    read_bounded_regular_file_with_parent, read_local_code_graph_query,
    read_local_repository_topology, read_local_rust_graph, read_local_test_markers,
    recall_local_memory, resolve_configuration, revalidate_local_memory,
    review_local_memory_correspondence, search_local_index, search_local_symbols,
    search_local_syntax_sites, sync_local_team_memory, validate_local_memory_actor,
    watch_local_repository, write_local_memory,
};
use repowitness_mcp::{
    ARCHITECTURE_OVERVIEW_LIMITATIONS, ArchitectureMapOutput, ArchitectureMapServiceRequest,
    ArchitectureOverviewOutput, ArchitectureOverviewServiceRequest, CHANGE_REVIEW_SCHEMA_VERSION,
    ChangeReviewOutput, ChangeReviewServiceRequest, CodeGraphQueryOutput,
    CodeGraphQueryResultOutput, CodeGraphQueryServiceRequest, CodeSearchOutput,
    CodeSearchServiceRequest, DiagnosticsOutput, DiagnosticsServiceRequest,
    EvidenceContextBuildOutput, EvidenceContextBuildServiceRequest, GraphArchitectureInput,
    GraphArchitectureOutput, GraphEvidenceInput, GraphEvidenceOutput, GraphImpactInput,
    GraphImpactOutput, GraphReadServiceOutput, GraphReadServiceRequest, GraphSearchInput,
    GraphSearchOutput, GraphStatusInput, GraphStatusOutput, GraphTraceInput, GraphTraceOutput,
    MAX_MCP_INTEROPERABLE_INTEGER, MEMORY_MANAGE_SCHEMA_VERSION, McpArchitectureMapFile,
    McpArchitectureMapLanguage, McpArchitectureOverviewKind, McpArchitectureOverviewRoot,
    McpChangeReviewPath, McpConfigurationIdentity, McpCoverage, McpDiagnosticsMemoryProjection,
    McpEvidenceContextAttribution, McpEvidenceContextItem, McpEvidenceContextOmission,
    McpEvidenceContextPayload, McpEvidenceContextProviderCoverage, McpEvidenceContextScope,
    McpGraphArchitectureCount, McpGraphCandidate, McpGraphCardinality, McpGraphContext,
    McpGraphDefinition, McpGraphEdge, McpGraphEvidence, McpGraphImpact, McpGraphPublication,
    McpGraphSite, McpGraphTrace, McpGraphTraceCoverage, McpGraphTraceTruncation,
    McpMemoryCandidate, McpMemoryCoverage, McpMemoryEvidence, McpMemoryOccurrence,
    McpMemoryProducer, McpMemoryRecord, McpMemoryTarget, McpOutboundSitesDeclaration,
    McpOutboundSyntaxSite, McpRelevantPath, McpRepositoryCatalog, McpRepositoryCatalogLoader,
    McpRepositoryTopologyCategory, McpRepositoryTopologyCoverage, McpRepositoryTopologyEntry,
    McpSearchMatch, McpSelectedMemory, McpSpan, McpSymbol, McpTestMarkerLanguageCoverage,
    MemoryManageDatabaseIdentityStatus, MemoryManageFileIdentityStatus,
    MemoryManageMaintenanceStatus, MemoryManageMaintenanceStepStatus, MemoryManageOutput,
    MemoryManagePublicationStatus, MemoryManagePublicationStepStatus, MemoryManageReviewDecision,
    MemoryManageServiceRequest, MemoryMutationOperation, MemoryMutationRequestScope,
    MemoryRecallOutput, MemoryRecallServiceRequest, MemoryRecallServiceSelection,
    OutboundSitesOutput, OutboundSitesSelectorOutput, OutboundSitesServiceRequest,
    RelevantPathsOutput, RelevantPathsServiceRequest, RepositoryService, RepositoryServiceError,
    RepositoryTopologyOutput, RepositoryTopologyServiceRequest, SymbolGetOutput,
    SymbolGetServiceRequest, SymbolSearchOutput, SymbolSearchServiceRequest, SymbolSelectorOutput,
    SyntaxSiteSearchOutput, SyntaxSiteSearchServiceRequest, TestMarkersOutput, serve_stdio,
    serve_stdio_with_memory_writes, serve_stdio_with_reloadable_repository_catalog,
    serve_stdio_with_reloadable_repository_catalog_with_memory_writes,
};

const EXIT_SUCCESS: u8 = 0;
const EXIT_USAGE: u8 = 64;
const EXIT_SOFTWARE: u8 = 70;
const EXIT_IO: u8 = 74;
const CONFIGURATION_LAYER_ARGUMENTS: usize = 6;
const MAX_CONTEXT_BUILD_ARGUMENTS: usize = 20 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_CONFIG_EXPLAIN_ARGUMENTS: usize = 7;
const MAX_DOCTOR_ARGUMENTS: usize = 10;
const MAX_GRAPH_ARGUMENTS: usize = 52;
const MAX_DIAGNOSTICS_ARGUMENTS: usize = 4 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_INDEX_ARGUMENTS: usize = 7 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_WATCH_ARGUMENTS: usize = 9 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_MEMORY_RECALL_ARGUMENTS: usize = 9 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_MEMORY_MANAGE_ARGUMENTS: usize = 24;
const MAX_MEMORY_REVALIDATE_ARGUMENTS: usize = 7;
const MAX_SEARCH_ARGUMENTS: usize = 8 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_RELEVANT_PATHS_ARGUMENTS: usize = 8 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_SYMBOL_SEARCH_ARGUMENTS: usize = 18 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_ARCHITECTURE_MAP_ARGUMENTS: usize = 6;
const MAX_REPOSITORY_TOPOLOGY_ARGUMENTS: usize = 6;
const MAX_ONBOARD_ARGUMENTS: usize = 7;
const MAX_ARCHITECTURE_OVERVIEW_ARGUMENTS: usize = 12;
const MAX_SYMBOL_GET_ARGUMENTS: usize = 18;
const MAX_OUTBOUND_SITES_ARGUMENTS: usize = 18;
const MAX_SYNTAX_SITE_SEARCH_ARGUMENTS: usize = 8;
const MAX_TEST_MARKERS_ARGUMENTS: usize = 10;
const MAX_MCP_SERVE_ARGUMENTS: usize = 15 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_CLI_CONTEXT_OUTPUT_BYTES: usize = 24 * 1024 * 1024;
const MAX_CLI_CONFIGURATION_OUTPUT_BYTES: usize = 32 * 1024;
const MAX_CLI_DIAGNOSTICS_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_CLI_GRAPH_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
const MAX_CLI_SEARCH_OUTPUT_BYTES: usize = 3 * 1024 * 1024;
const MAX_CLI_ARCHITECTURE_MAP_OUTPUT_BYTES: usize = 10 * 1024 * 1024;
const MAX_CLI_ARCHITECTURE_OVERVIEW_OUTPUT_BYTES: usize = 10 * 1024 * 1024;
const MAX_CLI_MEMORY_RECALL_OUTPUT_BYTES: usize = 20 * 1024 * 1024;
// The application payload can reach 10 MiB. Exact path and declaration
// representations expand by at most two, with room for the report envelope.
const MAX_CLI_SYMBOL_OUTPUT_BYTES: usize = (20 * 1024 * 1024) + (64 * 1024);
const MAX_CLI_OUTBOUND_SITES_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_CLI_SYNTAX_SITE_SEARCH_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const CLI_SYMBOL_REPORT_SCHEMA_VERSION: u16 = 2;
const PATH_TEXT_LIMIT: RepositoryPathTextByteLimit = RepositoryPathTextByteLimit::new(2_097_160);
const MCP_RUNTIME_WORKER_THREADS: usize = 2;
const MCP_RUNTIME_BLOCKING_THREADS: usize = 6;

const HELP: &str = concat!(
    "RepoWitness ",
    env!("CARGO_PKG_VERSION"),
    "\n\n",
    "Usage:\n  repowitness <command> [options]\n\n",
    "Commands:\n",
    "  config explain, doctor, identity, onboard, codex, inspect-paths\n",
    "  index, watch, gc, context-build, verify, diagnostics\n",
    "  architecture-map, architecture-overview, repository-topology, graph\n",
    "  search, locate-relevant-paths, symbol-search, symbol-get\n",
    "  outbound-sites, syntax-site-search, test-markers\n",
    "  memory-revalidate, memory-recall, memory-manage\n",
    "  mcp-serve --repository-id <id> --database <path> --root <path>\n",
    "  codex workspace create|list|remove ...\n\n",
    "Run `repowitness <command> --help` for command details.\n",
    "Configuration: --user-config <path> --workspace-config <path> --repository-config <path>\n",
);

const CONTEXT_BUILD_HELP: &str = concat!(
    "Compile bounded evidence-balanced source and memory context.\n\n",
    "Usage:\n",
    "  repowitness context-build --repository-id <id> --database <path> --root <path>\n",
    "      --intent <literal terms> [--budget <1-1048576>] [--limit <1-100>]\n",
    "      [--user-config <path>] [--workspace-config <path>]\n",
    "      [--repository-config <path>]\n\n",
    "The versioned evidence-balanced profile pins one single-repository\n",
    "workspace view. It reports typed scope,\n",
    "evidence tiers, provider attribution, whole-item omissions, and bounded source\n",
    "or current-memory content.\n",
    "The budget uses the labeled utf8_bytes_upper_bound_v1 estimator, not an exact\n",
    "model token count.\n",
);

const CONFIG_EXPLAIN_HELP: &str = concat!(
    "Resolve and explain versioned local configuration without mutation.\n\n",
    "Usage:\n",
    "  repowitness config explain [--user-config <path>]\n",
    "      [--workspace-config <path>] [--repository-config <path>]\n\n",
    "Only explicitly supplied files are read. Values and policy constraints are\n",
    "reported with path-free provenance categories and a canonical semantic digest.\n",
);

const DOCTOR_HELP: &str = concat!(
    "Validate versioned local configuration and explicit local targets without mutation.\n\n",
    "Usage:\n",
    "  repowitness doctor [--user-config <path>] [--workspace-config <path>]\n",
    "      [--repository-config <path>]\n",
    "      [--repository <path> --database <path>]\n\n",
    "Repository and database targets must be supplied together. Without them, the\n",
    "command performs configuration-only checks and emits a target-check warning.\n",
    "Reports are path-free; no repository, configuration, or database state is created.\n",
);

const MCP_SERVE_HELP: &str = concat!(
    "Serve one repository or a local catalog over MCP stdio.\n\n",
    "Usage:\n",
    "  repowitness mcp-serve --repository-id <id> --database <path> --root <path>\n",
    "  repowitness mcp-serve --catalog [--catalog-state-dir <path>]\n",
    "      [--user-config <path>] [--workspace-config <path>]\n",
    "      [--repository-config <path>]\n",
    "      [--enable-memory-writes --memory-actor <local-actor>]\n\n",
    "MCP automatically loads the shared user config at\n",
    "  $XDG_STATE_HOME/repowitness/config.toml\n",
    "or ~/.local/state/repowitness/config.toml when XDG_STATE_HOME is unset.\n",
    "--user-config overrides that automatic user config.\n\n",
    "Catalog mode exposes every repository registered by `onboard` and every\n",
    "explicit `codex workspace` member through one MCP connection. Tool calls may\n",
    "select an opaque repository_id; the current catalog\n",
    "repository is the default when available. Catalog mode reloads the bounded\n",
    "catalog at request boundaries, so onboarding changes do not require restart.\n",
    "It is read-only by default; explicit memory-write startup adds the fixed-actor\n",
    "memory_manage tool. A bad later catalog keeps the last valid snapshot.\n",
);

const DIAGNOSTICS_HELP: &str = concat!(
    "Inspect one exact active repository state without mutation.\n\n",
    "Usage:\n",
    "  repowitness diagnostics --repository-id <id> --database <path>\n",
    "      [--user-config <path>] [--workspace-config <path>]\n",
    "      [--repository-config <path>]\n\n",
    "Results expose the active snapshot, generation, source epoch, producer, index\n",
    "coverage, optional matching complete memory projection, implemented evidence\n",
    "capabilities, supported languages, path-free configuration identity, and\n",
    "explicit Phase 0 limitations.\n",
);

const GRAPH_HELP: &str = concat!(
    "Read one active or exact immutable native Rust graph without mutation.\n\n",
    "Usage:\n",
    "  repowitness graph status --repository-id <id> --database <path>\n",
    "  repowitness graph search --repository-id <id> --database <path> --query <text>\n",
    "  repowitness graph evidence --repository-id <id> --database <path> --site-json <json>\n",
    "  repowitness graph architecture --repository-id <id> --database <path>\n",
    "  repowitness graph trace --repository-id <id> --database <path>\n",
    "      --start-json <json> --direction <outbound|inbound> --edge-kind <kind>...\n",
    "  repowitness graph impact --repository-id <id> --database <path>\n",
    "      --start-json <definition-json> --edge-kind <kind>...\n\n",
    "Every\n",
    "operation accepts an optional exact --workspace-view/--graph-generation pair,\n",
    "--timeout-ms, configuration layer options, and applicable --max-* bounds. Search\n",
    "accepts --query. Evidence accepts the exact site object emitted previously. Trace\n",
    "accepts a tagged start object; impact accepts an exact definition object. Edge\n",
    "kinds are import, reference, and call. The default graph-read deadline is 30 seconds.\n",
    "Output is bounded single-document JSON with\n",
    "generation context, publication receipt, evidence, coverage, and truncation.\n",
);

const MEMORY_REVALIDATE_HELP: &str = concat!(
    "Rebuild and atomically activate current engineering memory.\n\n",
    "Usage:\n",
    "  repowitness memory-revalidate --repository-id <id> --database <path> [--] <repository>\n\n",
    "The active source generation, complete approved memory journal, Git validity,\n",
    "and precision-first Rust correspondence are fenced into one immutable\n",
    "projection. Failure leaves the previous projection readable.\n",
);

const MEMORY_RECALL_HELP: &str = concat!(
    "Recall bounded records from the complete active memory projection.\n\n",
    "Usage:\n",
    "  repowitness memory-recall --repository-id <id> --database <path>\n",
    "      (--query <literal terms> | --all) [--limit <1-100>]\n",
    "      [--user-config <path>] [--workspace-config <path>]\n",
    "      [--repository-config <path>]\n\n",
    "Results expose conflicts, freshness, exact projection/source identities,\n",
    "correspondence evidence, and complete projection coverage. Titles and bodies\n",
    "use lowercase hexadecimal encoding on the terminal-facing stream.\n",
);

const MEMORY_MANAGE_HELP: &str = concat!(
    "Manage canonical shared engineering memory with explicit local trust.\n\n",
    "Usage:\n",
    "  repowitness memory-manage write --repository-id <id> --input <yaml> [--] <repository>\n",
    "  repowitness memory-manage approve --repository-id <id> --database <path>\n",
    "      --record-id <id> --actor <local-actor> [--] <repository>\n",
    "  repowitness memory-manage sync --repository-id <id> --database <path>\n",
    "      --record-id <id> --actor <local-actor> [--] <repository>\n",
    "  repowitness memory-manage review --repository-id <id> --database <path>\n",
    "      --record-id <id> --revision <sha256> --evidence <0-15>\n",
    "      --operation <approve|reject|manual-link> --target-path <rwp1:h:text>\n",
    "      --target-artifact <sha256> --target-fact <0-9007199254740991>\n",
    "      [--target-snapshot <sha256>]\n",
    "      --actor <local-actor> [--] <repository>\n",
    "  repowitness memory-manage import-history --repository-id <id> --database <path>\n",
    "      --actor <local-actor> [--] <repository>\n\n",
    "Write validates, secret-scans, canonicalizes, and conflict-checks one complete\n",
    "record. Sync admits one repository-authored record and enforces the final\n",
    "multi-parent unresolved-head fence; review may pin a retained target snapshot; approval and correspondence review are separate local audit\n",
    "events. History import walks bounded reachable HEAD trees and appends\n",
    "observations only; repository text cannot approve or review itself.\n",
);

const INSPECT_HELP: &str = concat!(
    "Validate repository-path discovery without creating an index.\n\n",
    "Usage:\n",
    "  repowitness inspect-paths [--] <repository>\n\n",
    "The command invokes Git without a shell, applies fixed deadline and output\n",
    "bounds, validates exact path bytes, and prints only aggregate statistics.\n",
);

const INDEX_HELP: &str = concat!(
    "Build and atomically activate one local Rust/Go/TypeScript/TSX/Python index.\n\n",
    "Usage:\n",
    "  repowitness index --repository-id <id> --database <path>\n",
    "      [--user-config <path>] [--workspace-config <path>]\n",
    "      [--repository-config <path>] [--] <repository>\n\n",
    "The repository ID must use canonical rwi1:h: text. The database path is\n",
    "explicit and may be new or an existing RepoWitness SQLite index. The\n",
    "command prints only non-sensitive aggregate results.\n",
);

const WATCH_HELP: &str = concat!(
    "Reconcile a local Rust/Go/TypeScript/TSX/Python index in the foreground.\n\n",
    "Usage:\n",
    "  repowitness watch --repository-id <id> --database <path>\n",
    "      [--max-runtime-ms <1-86400000>]\n",
    "      [--user-config <path>] [--workspace-config <path>]\n",
    "      [--repository-config <path>] [--] <repository>\n\n",
    "The command performs one complete startup reconciliation, then polls complete\n",
    "source state until the first Ctrl-C, SIGINT, SIGTERM, platform console signal,\n",
    "or optional runtime deadline. It never detaches or starts a daemon. Shutdown\n",
    "is cooperative and bounded; interruption or failure leaves the prior active\n",
    "generation readable. Output contains only a path-free receipt and counters.\n",
);

const SEARCH_HELP: &str = concat!(
    "Search one active Rust/Go/TypeScript/TSX/Python index with proof-carrying results.\n\n",
    "Usage:\n",
    "  repowitness search --repository-id <id> --database <path> --query <text>\n",
    "      [--limit <1-100>] [--user-config <path>]\n",
    "      [--workspace-config <path>] [--repository-config <path>]\n\n",
    "The query uses bounded literal terms; raw FTS syntax is never exposed. Paths\n",
    "are emitted with the canonical byte-preserving text encoding. Every result\n",
    "reports its snapshot, generation, producer, evidence spans, and coverage.\n",
);

const SYMBOL_GET_HELP: &str = concat!(
    "Retrieve one exact declaration selected from code-search output.\n\n",
    "Usage:\n",
    "  repowitness symbol-get --repository-id <id> --database <path> --root <path>\n",
    "      --snapshot <sha256> --generation <positive-id> --path <rwp1:h:text>\n",
    "      --content <sha256> --artifact <sha256> --fact <0-9007199254740991>\n\n",
    "The complete snapshot, generation, path, content, artifact, and fact selector\n",
    "must still identify the active occurrence. Source bytes are read through a\n",
    "no-follow contained root, verified against the indexed digest, and emitted\n",
    "as display-safe UTF-8 or exact lowercase hexadecimal. The data is one\n",
    "JSON-escaped report field so source bytes cannot forge report lines.\n",
);

const OUTBOUND_SITES_HELP: &str = concat!(
    "Read exact unresolved raw syntax observations inside one selected declaration.\n\n",
    "Usage:\n",
    "  repowitness outbound-sites --repository-id <id> --database <path>\n",
    "      --snapshot <sha256> --generation <positive-id> --path <rwp1:h:text>\n",
    "      --content <sha256> --artifact <sha256> --fact <0-9007199254740991>\n",
    "      [--max-sites <1-250>]\n\n",
    "The selector is immutable and is normally copied from symbol-search. This command never opens\n",
    "the repository root and never resolves raw targets, infers relationships, or creates graph edges.\n",
);

const SYNTAX_SITE_SEARCH_HELP: &str = concat!(
    "Find exact parser-attributed raw syntax observations for one target spelling.\n\n",
    "Usage:\n",
    "  repowitness syntax-site-search --repository-id <id> --database <path> --target <exact raw target>\n",
    "      [--max-sites <1-250>]\n\n",
    "The target is compared byte-for-byte to parser-emitted raw target text in one immutable active\n",
    "generation. Equal spelling never resolves a declaration, proves a caller/reference relationship,\n",
    "or creates an inferred edge.\n",
);

const ARCHITECTURE_MAP_HELP: &str = concat!(
    "Inventory exact indexed source files across Rust, Go, TypeScript, TSX, and Python.\n\n",
    "Usage:\n",
    "  repowitness architecture-map --repository-id <id> --database <path> [--max-files <1-1000>]\n\n",
    "The result is pinned to one active generation and returns canonical paths, source and parser\n",
    "receipts, language totals, coverage, and explicit truncation. It is a file inventory only:\n",
    "it makes no import, call, ownership, or cross-language relationship claim.\n",
);

const REPOSITORY_TOPOLOGY_HELP: &str = concat!(
    "Read a bounded immutable path-only repository topology inventory.\n\n",
    "Usage:\n",
    "  repowitness repository-topology --repository-id <id> --database <path> [--max-paths <1-1000>]\n\n",
    "The inventory exposes only canonical repository paths and fixed categories. It never reads or\n",
    "returns file content, configuration values, URLs, package relationships, ownership, or runtime claims.\n",
);

const ARCHITECTURE_OVERVIEW_HELP: &str = concat!(
    "Orient to one indexed source generation without inferring architecture relationships.\n\n",
    "Usage:\n",
    "  repowitness architecture-overview --repository-id <id> --database <path>\n",
    "      [--max-roots <1-500>] [--max-entry-point-candidates <1-500>]\n",
    "      [--max-files <1-1000>]\n\n",
    "The result returns direct-source language/kind totals, structural repository-root and top-level\n",
    "directory buckets, exact per-file receipts, and bounded `function main` navigation candidates.\n",
    "It does not prove runtime entry points or infer package, ownership, import, call, test, hotspot,\n",
    "or cross-language relationships.\n",
);

const SYMBOL_SEARCH_HELP: &str = concat!(
    "Find bounded exact or prefix direct declaration facts across the five indexed source languages.\n\n",
    "Usage:\n",
    "  repowitness symbol-search --repository-id <id> --database <path> --name <symbol>\n",
    "      [--match <exact|prefix>] [--language <rust|go|typescript|tsx|python>]\n",
    "      [--kind <declaration-kind>] [--path-prefix <repository-relative-prefix>]\n",
    "      [--limit <1-100>] [configuration layer options]\n\n",
    "Results are generation-pinned parser declaration evidence. Equal names do not assert\n",
    "identity and never create relationship edges.\n",
);

include!("cli/adapters.rs");
include!("cli/architecture_map_commands.rs");
include!("cli/repository_topology_commands.rs");
include!("cli/architecture_overview_commands.rs");
include!("cli/relevant_paths_commands.rs");
include!("cli/symbol_search_commands.rs");
include!("cli/outbound_sites_commands.rs");
include!("cli/syntax_site_search_commands.rs");
include!("cli/test_markers_commands.rs");
include!("cli/mcp_graph.rs");
include!("cli/mcp_service.rs");
include!("cli/mcp_registry.rs");
include!("cli/graph_arguments.rs");
include!("cli/graph_commands.rs");
include!("cli/graph_output.rs");
include!("cli/identity_commands.rs");
include!("cli/identity_output.rs");
include!("cli/onboard_commands.rs");
include!("cli/workspace_commands.rs");
include!("cli/bounded_file.rs");
include!("cli/configuration.rs");
include!("cli/config_commands.rs");
include!("cli/config_output.rs");
include!("cli/gc_commands.rs");
include!("cli/gc_output.rs");
include!("cli/doctor.rs");
include!("cli/doctor_output.rs");
include!("cli/context_commands.rs");
include!("cli/change_review_commands.rs");
include!("cli/evidence_context_commands.rs");
include!("cli/mcp_evidence_context.rs");
include!("cli/diagnostics.rs");
include!("cli/diagnostics_commands.rs");
include!("cli/diagnostics_output.rs");
include!("cli/memory.rs");
include!("cli/memory_commands.rs");
include!("cli/memory_manage.rs");
include!("cli/memory_manage_commands.rs");
include!("cli/mcp_commands.rs");
include!("cli/mcp_memory_manage.rs");
include!("cli/watch_commands.rs");
include!("cli/watch_output.rs");
include!("cli/commands.rs");
include!("cli/output.rs");
include!("cli/memory_output.rs");

#[cfg(test)]
mod tests;
