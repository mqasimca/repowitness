//! Testable command parsing and human-facing reports for the RepoWitness CLI.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::AtomicBool};
use std::time::{SystemTime, UNIX_EPOCH};

use repowitness_local::{
    CODE_SEARCH_PROFILE_VERSION, CONTEXT_BUILD_RRF_K, ContextItem, ContextOmission,
    ContextProvider, DEFAULT_CONTEXT_BUILD_BUDGET_UNITS, DEFAULT_LOCAL_CONTEXT_PROVIDER_RESULTS,
    EvidenceLocation, GitPathDiscoveryLimits, GitPathDiscoveryStats, LocalCodeSearchRequest,
    LocalCodeSearchResult, LocalContextBuildRequest, LocalContextBuildResult, LocalIndexReport,
    LocalIndexRequest, LocalMemoryApprovalRequest, LocalMemoryCorrespondenceReviewRequest,
    LocalMemoryHistoryImportRequest, LocalMemoryRecallRequest, LocalMemoryRecallResult,
    LocalMemoryRecallSelection, LocalMemoryRevalidationReport, LocalMemoryRevalidationRequest,
    LocalMemoryWriteRequest, LocalRepositoryDiagnosticsRequest, LocalRepositoryDiagnosticsResult,
    LocalSymbolGetRequest, LocalSymbolGetResult, LocalSymbolSelectorText,
    MAX_CONTEXT_BUILD_BUDGET_UNITS, MemoryAssurance, MemoryCommitId,
    MemoryCorrespondenceReviewOperation, MemoryEffectiveState, MemoryKind, MemoryLifecycle,
    MemoryObjectFormat, MemoryProjectionValidityState, MemoryRecallCandidateRelation,
    MemoryRecallEvidence, MemoryRecallEvidenceAssurance, MemoryRecallEvidenceOutcome,
    MemoryRecallEvidenceState, MemoryRecallOccurrence, MemoryRecallReason, MemoryRecallRecord,
    MemoryRecordIdTextV1, MemoryRevalidationTarget, RepositoryIdentityTextV1,
    RepositoryPathTextByteLimit, RepositoryPathTextV1, ResolutionStatus,
    SYMBOL_GET_PROFILE_VERSION, approve_local_memory, build_local_context,
    diagnose_local_repository, discover_repository_paths, get_local_symbol,
    import_local_memory_history, index_local_repository, recall_local_memory,
    revalidate_local_memory, review_local_memory_correspondence, search_local_index,
    validate_local_memory_actor, write_local_memory,
};
use repowitness_mcp::{
    CodeSearchOutput, CodeSearchServiceRequest, ContextBuildOutput, ContextBuildServiceRequest,
    DiagnosticsOutput, DiagnosticsServiceRequest, MAX_MCP_INTEROPERABLE_INTEGER,
    McpContextCoverage, McpContextItem, McpContextMemoryItem, McpContextMemoryProjection,
    McpContextOmission, McpContextSourceItem, McpCoverage, McpDiagnosticsMemoryProjection,
    McpMemoryCandidate, McpMemoryCoverage, McpMemoryEvidence, McpMemoryOccurrence,
    McpMemoryProducer, McpMemoryRecord, McpMemoryTarget, McpSearchMatch, McpSelectedMemory,
    McpSpan, McpSymbol, MemoryManageOutput, MemoryManageReviewDecision, MemoryManageServiceRequest,
    MemoryRecallOutput, MemoryRecallServiceRequest, MemoryRecallServiceSelection,
    RepositoryService, RepositoryServiceError, SymbolGetOutput, SymbolGetServiceRequest,
    SymbolSelectorOutput, serve_stdio, serve_stdio_with_memory_writes,
};

const EXIT_SUCCESS: u8 = 0;
const EXIT_USAGE: u8 = 64;
const EXIT_SOFTWARE: u8 = 70;
const EXIT_IO: u8 = 74;
const MAX_CONTEXT_BUILD_ARGUMENTS: usize = 12;
const MAX_DIAGNOSTICS_ARGUMENTS: usize = 4;
const MAX_INDEX_ARGUMENTS: usize = 7;
const MAX_MEMORY_RECALL_ARGUMENTS: usize = 9;
const MAX_MEMORY_MANAGE_ARGUMENTS: usize = 24;
const MAX_MEMORY_REVALIDATE_ARGUMENTS: usize = 7;
const MAX_SEARCH_ARGUMENTS: usize = 8;
const MAX_SYMBOL_GET_ARGUMENTS: usize = 18;
const MAX_MCP_SERVE_ARGUMENTS: usize = 9;
const MAX_CLI_CONTEXT_OUTPUT_BYTES: usize = 24 * 1024 * 1024;
const MAX_CLI_DIAGNOSTICS_OUTPUT_BYTES: usize = 256 * 1024;
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
    "  repowitness inspect-paths [--] <repository>\n",
    "  repowitness index --repository-id <id> --database <path> [--] <repository>\n",
    "  repowitness context-build --repository-id <id> --database <path> --root <path> --intent <text>\n",
    "  repowitness diagnostics --repository-id <id> --database <path>\n",
    "  repowitness search --repository-id <id> --database <path> --query <text>\n",
    "  repowitness symbol-get <exact selector options>\n",
    "  repowitness memory-revalidate --repository-id <id> --database <path> <repository>\n",
    "  repowitness memory-recall --repository-id <id> --database <path> (--query <text>|--all)\n",
    "  repowitness memory-manage <write|approve|review|import-history> <options>\n",
    "  repowitness mcp-serve --repository-id <id> --database <path> --root <path>\n",
    "      [--enable-memory-writes --memory-actor <local-actor>]\n\n",
    "Commands:\n",
    "  inspect-paths  Validate bounded Git path discovery without analyzing file\n",
    "                 contents or creating an index.\n",
    "  index          Build and atomically activate a bounded Rust/Go/TS/TSX/Python index.\n",
    "  context-build  Compile bounded exact source and current-memory context.\n",
    "  diagnostics    Inspect active source/memory state, coverage, and limitations.\n",
    "  search         Search active Rust, Go, TypeScript, TSX, and Python symbols.\n",
    "  symbol-get     Retrieve one exact verified declaration from search output.\n\n",
    "  memory-revalidate  Atomically rebuild current memory against active source.\n",
    "  memory-recall  Recall bounded projected memory with freshness and evidence.\n",
    "  memory-manage  Write, approve, review correspondence, or observe Git memory.\n",
    "  mcp-serve      Serve context_build and evidence retrieval over stdio.\n",
);

const CONTEXT_BUILD_HELP: &str = concat!(
    "Compile deterministic exact source and current-memory context.\n\n",
    "Usage:\n",
    "  repowitness context-build --repository-id <id> --database <path> --root <path>\n",
    "      --intent <literal terms> [--budget <1-1048576>] [--limit <1-100>]\n\n",
    "The budget uses the labeled utf8_bytes_upper_bound_v1 estimator, not an exact\n",
    "model token count. Results expose exact generation/projection identities,\n",
    "component ranks, evidence, coverage, and every unsupported or omitted source.\n",
);

const MCP_SERVE_HELP: &str = concat!(
    "Serve the active Phase 0 supported-language index over local MCP stdio.\n\n",
    "Usage:\n",
    "  repowitness mcp-serve --repository-id <id> --database <path> --root <path>\n\n",
    "Stdout is reserved exclusively for newline-delimited MCP JSON-RPC. The\n",
    "configured repository, database, and identity are fixed for the process;\n",
    "tool callers cannot select arbitrary local paths. The default server exposes\n",
    "only read tools. memory_manage is available only when both mutation options\n",
    "are supplied; its actor is fixed locally and never accepted from tool input.\n",
);

const DIAGNOSTICS_HELP: &str = concat!(
    "Inspect one exact active repository state without mutation.\n\n",
    "Usage:\n",
    "  repowitness diagnostics --repository-id <id> --database <path>\n\n",
    "Results expose the active snapshot, generation, source epoch, producer, index\n",
    "coverage, optional matching complete memory projection, implemented evidence\n",
    "capabilities, supported languages, and explicit Phase 0 limitations.\n",
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
    "      (--query <literal terms> | --all) [--limit <1-100>]\n\n",
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
    "  repowitness memory-manage review --repository-id <id> --database <path>\n",
    "      --record-id <id> --revision <sha256> --evidence <0-15>\n",
    "      --operation <approve|reject|manual-link> --target-path <rwp1:h:text>\n",
    "      --target-artifact <sha256> --target-fact <0-9007199254740991>\n",
    "      --actor <local-actor> [--] <repository>\n",
    "  repowitness memory-manage import-history --repository-id <id> --database <path>\n",
    "      --actor <local-actor> [--] <repository>\n\n",
    "Write validates, secret-scans, canonicalizes, and conflict-checks one complete\n",
    "record. Approval and correspondence review are separate exact local audit\n",
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
    "  repowitness index --repository-id <id> --database <path> [--] <repository>\n\n",
    "The repository ID must use canonical rwi1:h: text. The database path is\n",
    "explicit and may be new or an existing RepoWitness SQLite index. The\n",
    "command prints only non-sensitive aggregate results.\n",
);

const SEARCH_HELP: &str = concat!(
    "Search one active Rust/Go/TypeScript/TSX/Python index with proof-carrying results.\n\n",
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
    "      --content <sha256> --artifact <sha256> --fact <0-9007199254740991>\n\n",
    "The complete snapshot, generation, path, content, artifact, and fact selector\n",
    "must still identify the active occurrence. Source bytes are read through a\n",
    "no-follow contained root, verified against the indexed digest, and emitted\n",
    "as display-safe UTF-8 or exact lowercase hexadecimal. The data is one\n",
    "JSON-escaped report field so source bytes cannot forge report lines.\n",
);

include!("cli/adapters.rs");
include!("cli/context.rs");
include!("cli/context_commands.rs");
include!("cli/context_output.rs");
include!("cli/diagnostics.rs");
include!("cli/diagnostics_commands.rs");
include!("cli/diagnostics_output.rs");
include!("cli/memory.rs");
include!("cli/memory_commands.rs");
include!("cli/memory_manage.rs");
include!("cli/memory_manage_commands.rs");
include!("cli/mcp_commands.rs");
include!("cli/mcp_memory_manage.rs");
include!("cli/commands.rs");
include!("cli/output.rs");
include!("cli/memory_output.rs");

#[cfg(test)]
mod tests;
