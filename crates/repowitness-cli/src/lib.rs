//! Testable command parsing and human-facing reports for the RepoWitness CLI.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::AtomicBool};
use std::time::{SystemTime, UNIX_EPOCH};

use repowitness_local::{
    CODE_SEARCH_PROFILE_VERSION, CONTEXT_BUILD_RRF_K, ConfigurationFileLayer, ConfigurationLayer,
    ConfigurationLayerKind, ConnectedWorkspaceIdTextV1, ContextItem, ContextOmission,
    ContextProvider, DEFAULT_CONTEXT_BUILD_BUDGET_UNITS, DEFAULT_LOCAL_CONTEXT_PROVIDER_RESULTS,
    EvidenceLocation, GeneratedLocalIdentity, GitPathDiscoveryLimits, GitPathDiscoveryStats,
    KnownAtApplicability, KnownAtEvidenceBasis, KnownAtHistoryCoverage, LocalCodeSearchRequest,
    LocalCodeSearchResult, LocalConnectedWorkspaceIndexReport, LocalConnectedWorkspaceIndexRequest,
    LocalContextBuildRequest, LocalContextBuildResult, LocalDoctorReport, LocalDoctorTargets,
    LocalIdentityGenerationError, LocalIdentityKind, LocalIndexReport, LocalIndexRequest,
    LocalKnownAtHistoryRequest, LocalMemoryApprovalRequest, LocalMemoryCorrespondenceReviewRequest,
    LocalMemoryDatabaseIdentity, LocalMemoryHistoryImportRequest, LocalMemoryMaintenance,
    LocalMemoryMaintenanceStep, LocalMemoryManageError, LocalMemoryMutation,
    LocalMemoryRecallRequest, LocalMemoryRecallResult, LocalMemoryRecallSelection,
    LocalMemoryRevalidationError, LocalMemoryRevalidationMutation, LocalMemoryRevalidationReport,
    LocalMemoryRevalidationRequest, LocalMemoryWriteRequest, LocalPersonalMemoryAppendRequest,
    LocalPersonalMemoryReadRequest, LocalPhase2ContextBuildRequest, LocalPhase2ContextItem,
    LocalRepositoryDiagnosticsRequest, LocalRepositoryDiagnosticsResult, LocalRetentionApplyReport,
    LocalRetentionApplyRequest, LocalRetentionPins, LocalRetentionPlanReport,
    LocalRetentionPlanRequest, LocalRustGraphReadOutput, LocalRustGraphReadRequest,
    LocalRustGraphReadResult, LocalScipEvidenceReadRequest, LocalSymbolGetRequest,
    LocalSymbolGetResult, LocalSymbolSelectorText, LocalTaskCheckpointRequest,
    LocalTaskListRequest, LocalTaskPollRequest, LocalTeamMemorySyncRequest, LocalWatchExit,
    LocalWatchReconciliation, LocalWatchReport, LocalWatchRequest, MAX_CONFIGURATION_FILE_BYTES,
    MAX_CONTEXT_BUILD_BUDGET_UNITS, McpToolProfile, MemoryAssurance, MemoryCommitId,
    MemoryCorrespondenceReviewOperation, MemoryEffectiveState, MemoryFileIdentityStatus,
    MemoryFilePublicationStepStatus, MemoryKind, MemoryLifecycle, MemoryObjectFormat,
    MemoryObservationSource, MemoryProjectionValidityState, MemoryRecallCandidateRelation,
    MemoryRecallEvidence, MemoryRecallEvidenceAssurance, MemoryRecallEvidenceOutcome,
    MemoryRecallEvidenceState, MemoryRecallOccurrence, MemoryRecallReason, MemoryRecallRecord,
    MemoryRecordIdTextV1, MemoryRevalidationTarget, PersonalMemoryKind, PersonalMemoryProfileId,
    Phase2ContextCandidate, Phase2ContextTier, PolicyValue, RepositoryIdentityTextV1,
    RepositoryPathTextByteLimit, RepositoryPathTextV1, ResolutionStatus, ResolvedConfiguration,
    ResolvedPreference, ResolvedToolProfilePreference, RustGraphAvailability,
    RustGraphCandidateRecord, RustGraphDefinitionRecord, RustGraphEvidenceResult,
    RustGraphImpactClass, RustGraphOutcomeRecord, RustGraphPublicationSummary,
    RustGraphSiteSelector, RustGraphTraceResult, SYMBOL_GET_PROFILE_VERSION,
    ScipRelationshipDirection, ScipSymbolEvidenceResult, SourceLanguage, SourceSlotIdTextV1,
    SourceSnapshotDigest, TaskId, TaskState, TaskStatus, append_local_personal_memory,
    append_local_task_checkpoint, apply_local_retention, approve_local_memory, build_local_context,
    build_local_phase2_context, diagnose_local_repository, discover_repository_paths,
    generate_local_identity, get_local_symbol, import_local_memory_history,
    index_local_connected_workspace, index_local_repository, inspect_local_doctor,
    list_local_tasks, parse_configuration_file, plan_local_retention, poll_local_task,
    read_bounded_regular_file_with_parent, read_local_known_at_history, read_local_personal_memory,
    read_local_rust_graph, read_local_scip_evidence, recall_local_memory, resolve_configuration,
    revalidate_local_memory, review_local_memory_correspondence, search_local_index,
    sync_local_team_memory, validate_local_memory_actor, watch_local_repository,
    write_local_memory,
};
use repowitness_mcp::{
    CodeSearchOutput, CodeSearchServiceRequest, ContextBuildOutput, ContextBuildServiceRequest,
    DiagnosticsOutput, DiagnosticsServiceRequest, GraphArchitectureInput, GraphArchitectureOutput,
    GraphEvidenceInput, GraphEvidenceOutput, GraphImpactInput, GraphImpactOutput,
    GraphReadServiceOutput, GraphReadServiceRequest, GraphSearchInput, GraphSearchOutput,
    GraphStatusInput, GraphStatusOutput, GraphTraceInput, GraphTraceOutput,
    HistoricalMemoryApplicability, HistoricalMemoryCoverage, HistoricalMemoryEvidence,
    HistoricalMemoryEvidenceBasis, HistoricalMemoryOutput, HistoricalMemoryServiceRequest,
    HistoricalMemoryTarget, MAX_MCP_INTEROPERABLE_INTEGER, MEMORY_MANAGE_SCHEMA_VERSION,
    McpConfigurationIdentity, McpContextCoverage, McpContextItem, McpContextMemoryItem,
    McpContextMemoryProjection, McpContextOmission, McpContextSourceItem, McpCoverage,
    McpDiagnosticsMemoryProjection, McpGraphArchitectureCount, McpGraphCandidate,
    McpGraphCardinality, McpGraphContext, McpGraphDefinition, McpGraphEdge, McpGraphEvidence,
    McpGraphImpact, McpGraphPublication, McpGraphSite, McpGraphTrace, McpGraphTraceCoverage,
    McpGraphTraceTruncation, McpMemoryCandidate, McpMemoryCoverage, McpMemoryEvidence,
    McpMemoryOccurrence, McpMemoryProducer, McpMemoryRecord, McpMemoryTarget,
    McpPhase2ContextAttribution, McpPhase2ContextItem, McpPhase2ContextOmission,
    McpPhase2ContextPayload, McpPhase2ContextProviderCoverage, McpPhase2ContextScope,
    McpScipOccurrence, McpScipOverlay, McpScipRelationship, McpSearchMatch, McpSelectedMemory,
    McpSpan, McpSymbol, McpToolSurface, MemoryManageDatabaseIdentityStatus,
    MemoryManageFileIdentityStatus, MemoryManageMaintenanceStatus,
    MemoryManageMaintenanceStepStatus, MemoryManageOutput, MemoryManagePublicationStatus,
    MemoryManagePublicationStepStatus, MemoryManageReviewDecision, MemoryManageServiceRequest,
    MemoryMutationOperation, MemoryMutationRequestScope, MemoryRecallOutput,
    MemoryRecallServiceRequest, MemoryRecallServiceSelection, NativeTaskState, NativeTaskStatus,
    PersonalMemoryKind as McpPersonalMemoryKind,
    PersonalMemoryLifecycle as McpPersonalMemoryLifecycle, PersonalMemoryOperation,
    PersonalMemoryOutput, PersonalMemoryRecordOutput, PersonalMemoryServiceRequest,
    Phase2ContextBuildOutput, Phase2ContextBuildServiceRequest, RepositoryService,
    RepositoryServiceError, ScipEvidenceInput, ScipEvidenceOutput, ScipEvidenceServiceRequest,
    SymbolGetOutput, SymbolGetServiceRequest, SymbolSelectorOutput,
    serve_stdio_with_surface_and_native_tasks, serve_stdio_with_surface_tasks_and_personal_memory,
};

const EXIT_SUCCESS: u8 = 0;
const EXIT_USAGE: u8 = 64;
const EXIT_SOFTWARE: u8 = 70;
const EXIT_IO: u8 = 74;
const CONFIGURATION_LAYER_ARGUMENTS: usize = 6;
const MAX_CONTEXT_BUILD_ARGUMENTS: usize = 12 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_PHASE2_CONTEXT_BUILD_ARGUMENTS: usize = 18;
const MAX_CONFIG_EXPLAIN_ARGUMENTS: usize = 7;
const MAX_DOCTOR_ARGUMENTS: usize = 10;
const MAX_GRAPH_ARGUMENTS: usize = 52;
const MAX_SCIP_EVIDENCE_ARGUMENTS: usize = 16;
const MAX_DIAGNOSTICS_ARGUMENTS: usize = 4 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_INDEX_ARGUMENTS: usize = 7 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_WORKSPACE_INDEX_ARGUMENTS: usize = 5 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_WATCH_ARGUMENTS: usize = 9 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_MEMORY_RECALL_ARGUMENTS: usize = 9 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_MEMORY_MANAGE_ARGUMENTS: usize = 24;
const MAX_MEMORY_REVALIDATE_ARGUMENTS: usize = 7;
const MAX_TASK_STATUS_ARGUMENTS: usize = 6;
const MAX_TASK_COMMAND_ARGUMENTS: usize = 14;
const MAX_PERSONAL_MEMORY_COMMAND_ARGUMENTS: usize = 18;
const MAX_SEARCH_ARGUMENTS: usize = 8 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_SYMBOL_GET_ARGUMENTS: usize = 18;
const MAX_MCP_SERVE_ARGUMENTS: usize = 14 + CONFIGURATION_LAYER_ARGUMENTS;
const MAX_CLI_CONTEXT_OUTPUT_BYTES: usize = 24 * 1024 * 1024;
const MAX_CLI_CONFIGURATION_OUTPUT_BYTES: usize = 32 * 1024;
const MAX_CLI_DIAGNOSTICS_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_CLI_GRAPH_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
const MAX_CLI_SEARCH_OUTPUT_BYTES: usize = 3 * 1024 * 1024;
const MAX_CLI_MEMORY_RECALL_OUTPUT_BYTES: usize = 20 * 1024 * 1024;
// The application payload can reach 10 MiB. Exact path and declaration
// representations expand by at most two, with room for the report envelope.
const MAX_CLI_SYMBOL_OUTPUT_BYTES: usize = (20 * 1024 * 1024) + (64 * 1024);
const CLI_SYMBOL_REPORT_SCHEMA_VERSION: u16 = 2;
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
    "  repowitness config explain [configuration layer options]\n",
    "  repowitness doctor [configuration layer options]\n",
    "      [--repository <path> --database <path>]\n",
    "  repowitness identity generate <repository|connected-workspace|source-slot>\n",
    "  repowitness inspect-paths [--] <repository>\n",
    "  repowitness index --repository-id <id> --database <path> [configuration layer options] [--] <repository>\n",
    "  repowitness workspace index --manifest <path> --database <path> [configuration layer options]\n",
    "  repowitness watch --repository-id <id> --database <path> [configuration layer options] [--] <repository>\n",
    "  repowitness gc <plan|apply> --database <path> [retention options]\n",
    "  repowitness context-build --repository-id <id> --database <path> --root <path> --intent <text> [configuration layer options]\n",
    "  repowitness phase2-context-build --repository-id <id> --database <path> --root <path> --intent <text>\n",
    "  repowitness diagnostics --repository-id <id> --database <path> [configuration layer options]\n",
    "  repowitness graph <status|search|evidence|architecture|trace|impact> <options>\n",
    "  repowitness search --repository-id <id> --database <path> --query <text> [configuration layer options]\n",
    "  repowitness symbol-get <exact selector options>\n",
    "  repowitness memory-revalidate --repository-id <id> --database <path> <repository>\n",
    "  repowitness memory-recall --repository-id <id> --database <path> (--query <text>|--all) [configuration layer options]\n",
    "  repowitness memory-manage <write|approve|review|import-history> <options>\n",
    "  repowitness memory-history --repository-id <id> --database <path> --known-at <unix-ms> <exact target> <repository>\n",
    "  repowitness personal-memory <append|read> <options>\n",
    "  repowitness task-status --repository-id <id> --database <path> --task-id <hex>\n",
    "  repowitness task <create|checkpoint> <options>\n",
    "  repowitness mcp-serve --repository-id <id> --database <path> --root <path> [configuration layer options]\n",
    "      [--enable-memory-writes --memory-actor <local-actor>]\n",
    "      [--enable-personal-memory --personal-memory-profile <32 lowercase hex characters>]\n",
    "      [--enable-native-tasks]\n\n",
    "Commands:\n",
    "  config explain Explain effective configuration and path-free provenance.\n",
    "  doctor         Validate effective configuration and explicit local targets.\n",
    "  identity       Generate a canonical local identity from OS secure randomness.\n",
    "  inspect-paths  Validate bounded Git path discovery without analyzing file\n",
    "                 contents or creating an index.\n",
    "  index          Build and atomically activate a bounded Rust/Go/TS/TSX/Python index.\n",
    "  workspace      Atomically index an explicit connected-workspace manifest.\n",
    "  watch          Reconcile and atomically activate source changes in the foreground.\n",
    "  gc             Plan or explicitly apply bounded generation retention.\n",
    "  context-build  Compile bounded exact source and current-memory context.\n",
    "  phase2-context-build  Compile the separate evidence-balanced Phase 2 context.\n",
    "  diagnostics    Inspect active source/memory state, coverage, and limitations.\n",
    "  graph          Read the native immutable Rust syntax graph with exact evidence.\n",
    "  search         Search active Rust, Go, TypeScript, TSX, and Python symbols.\n",
    "  symbol-get     Retrieve one exact verified declaration from search output.\n\n",
    "  memory-revalidate  Atomically rebuild current memory against active source.\n",
    "  memory-recall  Recall bounded projected memory with freshness and evidence.\n",
    "  memory-manage  Write, approve, review correspondence, or observe Git memory.\n",
    "  memory-history Read a bounded historical applicability receipt for one exact target.\n",
    "  personal-memory  Append or read explicitly profile-scoped local-only memory.\n",
    "  task-status    Poll redacted status for one durable local task.\n",
    "  task           Create or append a bounded durable task checkpoint.\n",
    "  mcp-serve      Serve context_build and evidence retrieval over stdio.\n",
    "\nConfiguration layer options:\n",
    "  --user-config <path> --workspace-config <path> --repository-config <path>\n",
);

const CONTEXT_BUILD_HELP: &str = concat!(
    "Compile deterministic exact source and current-memory context.\n\n",
    "Usage:\n",
    "  repowitness context-build --repository-id <id> --database <path> --root <path>\n",
    "      --intent <literal terms> [--budget <1-1048576>] [--limit <1-100>]\n",
    "      [--user-config <path>] [--workspace-config <path>]\n",
    "      [--repository-config <path>]\n\n",
    "The budget uses the labeled utf8_bytes_upper_bound_v1 estimator, not an exact\n",
    "model token count. Results expose exact generation/projection identities,\n",
    "component ranks, evidence, coverage, and every unsupported or omitted source.\n",
);

const PHASE2_CONTEXT_BUILD_HELP: &str = concat!(
    "Compile the versioned Phase 2 evidence-balanced context.\n\n",
    "Usage:\n",
    "  repowitness phase2-context-build --repository-id <id> --database <path> --root <path>\n",
    "      --intent <literal terms> [--budget <1-1048576>] [--limit <1-100>]\n",
    "      [--connected-workspace-id <cwi1:h:...> --source-slot-id <ssi1:h:...>]\n",
    "      [--scip-symbol <opaque-symbol>]\n\n",
    "This separate profile preserves the Phase 0 context-build behavior. It pins one\n",
    "single-repository workspace view by default, or one explicit connected source slot, and\n",
    "exposes typed scope, evidence tier, provider\n",
    "attribution, whole-item omissions, and bounded source or current-memory content. An optional\n",
    "exact SCIP symbol adds one source-verified, unambiguous precision-overlay provider result.\n",
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
    "Serve the active Phase 0 supported-language index over local MCP stdio.\n\n",
    "Usage:\n",
    "  repowitness mcp-serve --repository-id <id> --database <path> --root <path>\n",
    "      [--connected-workspace-id <id> --source-slot-id <id>]\n",
    "      [--user-config <path>] [--workspace-config <path>]\n",
    "      [--repository-config <path>]\n",
    "      [--enable-memory-writes --memory-actor <local-actor>]\n\n",
    "      [--enable-personal-memory --personal-memory-profile <32 lowercase hex characters>]\n\n",
    "Stdout is reserved exclusively for newline-delimited MCP JSON-RPC. The\n",
    "configured repository, database, identities, and optional graph source slot are fixed for the process;\n",
    "tool callers cannot select arbitrary local paths. The default server exposes\n",
    "only read tools. memory_manage is available only when both mutation options\n",
    "are supplied, the effective configuration permits writes, and the selected\n",
    "implemented tool profile is authorized. Its actor is fixed locally and never accepted\n",
    "from tool input. Configuration failures occur before runtime initialization.\n",
    "personal_memory is absent by default. It is available only when its explicit\n",
    "startup profile capability is supplied; callers cannot select a profile, and\n",
    "context_build and memory_recall remain team-only.\n",
    "Native MCP Tasks are disabled by default and are available only when\n",
    "--enable-native-tasks is supplied at startup; durable task state survives\n",
    "a restart, while bounded process-local result payloads do not.\n",
    "The default canonical profile is unchanged. A user-owned configuration may\n",
    "opt into incumbent-compatible, which adds seven bounded read-only aliases.\n",
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
    "  repowitness graph status --connected-workspace-id <id> --source-slot-id <id> --database <path>\n",
    "  repowitness graph search --repository-id <id> --database <path> --query <text>\n",
    "  repowitness graph evidence --repository-id <id> --database <path> --site-json <json>\n",
    "  repowitness graph architecture --repository-id <id> --database <path>\n",
    "  repowitness graph trace --repository-id <id> --database <path>\n",
    "      --start-json <json> --direction <outbound|inbound> --edge-kind <kind>...\n",
    "  repowitness graph impact --repository-id <id> --database <path>\n",
    "      --start-json <definition-json> --edge-kind <kind>...\n\n",
    "Use either --repository-id for the compatible single-repository workspace or\n",
    "--connected-workspace-id with --source-slot-id for an explicit source member. Every\n",
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

const SCIP_EVIDENCE_HELP: &str = concat!(
    "Read exact package-scoped evidence from one imported SCIP overlay.\n\n",
    "Usage:\n",
    "  repowitness scip-evidence --repository-id <id> --database <path> --symbol <scip-symbol>\n",
    "      [--package-root <rwp1:h:text>]... [--workspace-view <positive-id>]\n",
    "      [--timeout-ms <1-30000>]\n",
    "  repowitness scip-evidence --connected-workspace-id <id> --source-slot-id <id>\n",
    "      --database <path> --symbol <scip-symbol> [same optional bounds]\n\n",
    "The command is read-only. It selects an active or exact immutable workspace view and\n",
    "returns categorical `not_produced`, `no_match`, or `found` evidence. Package roots are\n",
    "canonical byte-preserving repository paths; no package manager or host path is used.\n",
);

const MAX_SCIP_IMPORT_ARGUMENTS: usize = 14;
const SCIP_IMPORT_HELP: &str = concat!(
    "Import one bounded SCIP file as an immutable source-slot precision overlay.\n\n",
    "Usage:\n",
    "  repowitness scip-import --database <path> --root <repository-root> --scip-file <path>\n",
    "      --connected-workspace-id <cwi1:h:text> --source-slot-id <ssi1:h:text>\n",
    "      [--workspace-view <positive-id>] [--timeout-ms <1-30000>]\n\n",
    "The file is read once through a no-follow regular-file boundary. Its claims are\n",
    "validated only against the exact current source slot and source snapshot. A failed,\n",
    "changed, stale, or cancelled import leaves the prior active overlay readable.\n",
);

include!("cli/adapters.rs");
include!("cli/mcp_graph.rs");
include!("cli/mcp_scip_evidence.rs");
include!("cli/mcp_service.rs");
include!("cli/graph_arguments.rs");
include!("cli/graph_commands.rs");
include!("cli/graph_output.rs");
include!("cli/scip_evidence_commands.rs");
include!("cli/scip_import_commands.rs");
include!("cli/identity_commands.rs");
include!("cli/identity_output.rs");
include!("cli/bounded_file.rs");
include!("cli/configuration.rs");
include!("cli/config_commands.rs");
include!("cli/config_output.rs");
include!("cli/gc_commands.rs");
include!("cli/gc_output.rs");
include!("cli/doctor.rs");
include!("cli/doctor_output.rs");
include!("cli/context.rs");
include!("cli/context_commands.rs");
include!("cli/context_output.rs");
include!("cli/phase2_context_commands.rs");
include!("cli/mcp_phase2_context.rs");
include!("cli/diagnostics.rs");
include!("cli/diagnostics_commands.rs");
include!("cli/diagnostics_output.rs");
include!("cli/memory.rs");
include!("cli/memory_commands.rs");
include!("cli/memory_manage.rs");
include!("cli/memory_manage_commands.rs");
include!("cli/known_at_history_commands.rs");
include!("cli/task_commands.rs");
include!("cli/personal_memory_commands.rs");
include!("cli/mcp_commands.rs");
include!("cli/mcp_memory_manage.rs");
include!("cli/watch_commands.rs");
include!("cli/watch_output.rs");
include!("cli/workspace_commands.rs");
include!("cli/workspace_output.rs");
include!("cli/commands.rs");
include!("cli/output.rs");
include!("cli/memory_output.rs");

#[cfg(test)]
mod tests;
