//! End-to-end Phase 0 product and resource probe over the pinned benchmark corpus.

use std::{
    env,
    error::Error,
    ffi::OsString,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use repowitness_application::RepositoryIdentityTextV1;
use repowitness_domain::RepositoryIdentityDigest;
use repowitness_local::{
    ContextItem, ContextOmission, LocalContextBuildRequest, LocalIndexReport, LocalIndexRequest,
    LocalMemoryApprovalRequest, LocalMemoryRecallRequest, LocalMemoryRecallSelection,
    LocalMemoryRevalidationRequest, LocalMemoryWriteRequest, MemoryEffectiveState,
    MemoryRecallEvidenceOutcome, approve_local_memory, build_local_context, index_local_repository,
    recall_local_memory, revalidate_local_memory, write_local_memory,
};

#[path = "phase0_product_probe/mcp.rs"]
mod mcp;
#[path = "phase0_product_probe/memory.rs"]
mod memory;
#[path = "phase0_product_probe/metrics.rs"]
mod metrics;
#[path = "phase0_product_probe/search.rs"]
mod search;

type ProbeResult<T> = Result<T, Box<dyn Error>>;

const BENCHMARK_ID: &str = "phase0-rust-evidence-memory-v1";
const DEFAULT_RUNS: usize = 5;
const MAX_RUNS: usize = 100;
const MIGRATION_TIMESTAMP: u64 = 1_722_000_000_000;
const APPROVAL_TIMESTAMP: u64 = 1_722_000_000_001;
const MAX_ADMITTED_WALL_MS: u64 = 3_600_000;
const MAX_ADMITTED_RESULT_BYTES: u64 = 24 * 1024 * 1024;
const MAX_ADMITTED_STORAGE_BYTES: u64 = 16 * 1024 * 1024 * 1024;

fn main() {
    if let Err(error) = run() {
        eprintln!("phase0 product probe failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> ProbeResult<()> {
    let arguments = Arguments::parse()?;
    validate_inputs(&arguments)?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let repository_digest = RepositoryIdentityDigest::new([0xB4; 32]);
    let repository_identity = RepositoryIdentityTextV1::encode(repository_digest);
    let initial = run_initial_phase(&arguments, repository_identity.as_str(), &cancelled)?;
    let current = establish_current_memory(
        &arguments,
        repository_digest,
        repository_identity.as_str(),
        initial.warm,
        &cancelled,
    )?;
    let changed = validate_stale_memory(
        &arguments,
        repository_identity.as_str(),
        initial.warm,
        &current.exact_memory,
        &cancelled,
    )?;
    let mcp = mcp::probe_default_surface(
        &arguments.cli,
        &arguments.repository,
        &arguments.database,
        repository_identity.as_str(),
        arguments.budgets.max_material_result_bytes,
    )?;
    let configuration =
        metrics::active_configuration_digest(&arguments.database, repository_digest)?;
    let database_bytes = metrics::required_file_size(&arguments.database)?;
    let wal_bytes = metrics::wal_file_size(&arguments.database)?;
    validate_storage_budgets(database_bytes, wal_bytes, arguments.budgets)?;
    emit_report(Report {
        arguments: &arguments,
        cold: initial.cold,
        cold_wall: initial.cold_wall,
        warm: initial.warm,
        warm_wall: initial.warm_wall,
        changed,
        search: initial.search,
        mcp,
        configuration,
        canonical_memory_bytes: current.canonical_memory_bytes,
        database_bytes,
        wal_bytes,
    });
    Ok(())
}

fn run_initial_phase(
    arguments: &Arguments,
    repository_identity: &str,
    cancelled: &Arc<AtomicBool>,
) -> ProbeResult<InitialPhase> {
    let cold_started = Instant::now();
    let cold = index(arguments, repository_identity, cancelled)?;
    let cold_wall = cold_started.elapsed();
    validate_cold_index(cold, cold_wall, arguments.budgets)?;
    let warm_started = Instant::now();
    let warm = index(arguments, repository_identity, cancelled)?;
    let warm_wall = warm_started.elapsed();
    validate_warm_index(cold, warm)?;
    let search = search::verify_manifest_evidence(
        &arguments.database,
        repository_identity,
        arguments.runs,
        arguments.budgets.max_warm_query_p95_us()?,
        cancelled,
    )?;
    let source_only = build_context(arguments, repository_identity, "into_frame", cancelled)?;
    if source_only.coverage().source_included() == 0
        || source_only.coverage().memory_included() != 0
    {
        return Err("source-only context coverage was not exact".into());
    }
    Ok(InitialPhase {
        cold,
        cold_wall,
        warm,
        warm_wall,
        search,
    })
}

fn establish_current_memory(
    arguments: &Arguments,
    repository_digest: RepositoryIdentityDigest,
    repository_identity: &str,
    warm: LocalIndexReport,
    cancelled: &Arc<AtomicBool>,
) -> ProbeResult<CurrentMemory> {
    let exact_memory = memory::exact_memory_input(
        &arguments.repository,
        &arguments.database,
        repository_digest,
    )?;
    let written = write_local_memory(
        LocalMemoryWriteRequest::from_bytes(
            &arguments.repository,
            exact_memory.yaml(),
            repository_identity,
        ),
        Arc::clone(cancelled),
    )?;
    if !written.created() {
        return Err("the benchmark memory record was not newly created".into());
    }
    let approved = approve_local_memory(
        LocalMemoryApprovalRequest::new(
            &arguments.repository,
            &arguments.database,
            repository_identity,
            exact_memory.record_id(),
            "phase0-benchmark",
            MIGRATION_TIMESTAMP,
            APPROVAL_TIMESTAMP,
        ),
        Arc::clone(cancelled),
    )?;
    if approved.revision() != written.revision() || !approved.approval_inserted() {
        return Err("the exact benchmark memory revision was not approved".into());
    }

    let current_projection = revalidate(arguments, repository_identity, cancelled)?;
    if current_projection.generation() != warm.generation()
        || current_projection.projected_records() != 1
        || current_projection.unresolved_records() != 0
    {
        return Err("the current benchmark memory projection was incomplete".into());
    }
    validate_recall(
        arguments,
        repository_identity,
        MemoryEffectiveState::Current,
        MemoryRecallEvidenceOutcome::Exact,
        cancelled,
    )?;
    let current_context = build_context(arguments, repository_identity, "into_frame", cancelled)?;
    if current_context.coverage().source_included() == 0
        || current_context.coverage().memory_included() != 1
        || !current_context
            .items()
            .iter()
            .any(|item| matches!(item, ContextItem::Memory(_)))
    {
        return Err("current source and memory did not compile into one context".into());
    }
    Ok(CurrentMemory {
        exact_memory,
        canonical_memory_bytes: written.canonical_bytes(),
    })
}

fn validate_stale_memory(
    arguments: &Arguments,
    repository_identity: &str,
    warm: LocalIndexReport,
    exact_memory: &memory::ExactMemoryInput,
    cancelled: &Arc<AtomicBool>,
) -> ProbeResult<LocalIndexReport> {
    exact_memory.mutate_target(&arguments.repository)?;
    let changed = index(arguments, repository_identity, cancelled)?;
    validate_changed_index(warm, changed)?;
    let stale_projection = revalidate(arguments, repository_identity, cancelled)?;
    if stale_projection.generation() != changed.generation()
        || stale_projection.projected_records() != 1
    {
        return Err("the changed benchmark memory projection was incomplete".into());
    }
    validate_recall(
        arguments,
        repository_identity,
        MemoryEffectiveState::Stale,
        MemoryRecallEvidenceOutcome::Changed,
        cancelled,
    )?;
    let stale_context = build_context(arguments, repository_identity, "into_frame", cancelled)?;
    if stale_context.coverage().source_included() == 0
        || stale_context.coverage().memory_included() != 0
        || stale_context.coverage().memory_non_current_omitted() != 1
        || stale_context
            .items()
            .iter()
            .any(|item| matches!(item, ContextItem::Memory(_)))
        || !stale_context
            .omissions()
            .contains(&ContextOmission::MemoryNotCurrent(1))
    {
        return Err("stale memory was not explicitly excluded from context".into());
    }
    Ok(changed)
}

fn index(
    arguments: &Arguments,
    repository_identity: &str,
    cancelled: &Arc<AtomicBool>,
) -> ProbeResult<LocalIndexReport> {
    Ok(index_local_repository(
        LocalIndexRequest::new(
            &arguments.repository,
            &arguments.database,
            repository_identity,
            MIGRATION_TIMESTAMP,
        ),
        Arc::clone(cancelled),
    )?)
}

fn revalidate(
    arguments: &Arguments,
    repository_identity: &str,
    cancelled: &Arc<AtomicBool>,
) -> ProbeResult<repowitness_local::LocalMemoryRevalidationReport> {
    Ok(revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(
            &arguments.repository,
            &arguments.database,
            repository_identity,
            MIGRATION_TIMESTAMP,
        ),
        Arc::clone(cancelled),
    )?)
}

fn build_context(
    arguments: &Arguments,
    repository_identity: &str,
    query: &str,
    cancelled: &Arc<AtomicBool>,
) -> ProbeResult<repowitness_local::LocalContextBuildResult> {
    Ok(build_local_context(
        LocalContextBuildRequest::new(
            &arguments.repository,
            &arguments.database,
            repository_identity,
            query,
        ),
        Arc::clone(cancelled),
    )?)
}

fn validate_recall(
    arguments: &Arguments,
    repository_identity: &str,
    state: MemoryEffectiveState,
    outcome: MemoryRecallEvidenceOutcome,
    cancelled: &Arc<AtomicBool>,
) -> ProbeResult<()> {
    let recall = recall_local_memory(
        LocalMemoryRecallRequest::new(
            &arguments.database,
            repository_identity,
            LocalMemoryRecallSelection::Query("into_frame"),
        ),
        Arc::clone(cancelled),
    )?;
    let records = recall.records();
    if records.len() != 1
        || records[0].effective_state() != state
        || records[0].evidence().len() != 1
        || records[0].evidence()[0].outcome() != outcome
    {
        return Err("memory recall did not preserve the expected categorical state".into());
    }
    Ok(())
}

fn validate_cold_index(
    report: LocalIndexReport,
    elapsed: Duration,
    budgets: BenchmarkBudgets,
) -> ProbeResult<()> {
    if elapsed > Duration::from_millis(budgets.max_full_index_wall_ms) {
        return Err("cold full-index wall time exceeded the proposed budget".into());
    }
    if report.indexed_rust_files() == 0
        || report.analyzed_rust_files() != report.indexed_rust_files()
        || report.reused_rust_files() != 0
        || report.total_facts() == 0
    {
        return Err("cold indexing did not analyze the complete Rust corpus".into());
    }
    Ok(())
}

fn validate_warm_index(cold: LocalIndexReport, warm: LocalIndexReport) -> ProbeResult<()> {
    if warm.generation() == cold.generation()
        || warm.indexed_rust_files() != cold.indexed_rust_files()
        || warm.total_source_bytes() != cold.total_source_bytes()
        || warm.total_facts() != cold.total_facts()
        || warm.reused_rust_files() != warm.indexed_rust_files()
        || warm.analyzed_rust_files() != 0
    {
        return Err("unchanged indexing did not reuse the exact complete artifact set".into());
    }
    Ok(())
}

fn validate_storage_budgets(
    database_bytes: u64,
    wal_bytes: u64,
    budgets: BenchmarkBudgets,
) -> ProbeResult<()> {
    if database_bytes > budgets.max_database_bytes {
        return Err("SQLite database size exceeded the proposed budget".into());
    }
    if wal_bytes > budgets.max_wal_bytes_after_completion {
        return Err("SQLite WAL was not empty after benchmark completion".into());
    }
    Ok(())
}

fn validate_changed_index(warm: LocalIndexReport, changed: LocalIndexReport) -> ProbeResult<()> {
    if changed.generation() == warm.generation()
        || changed.indexed_rust_files() != warm.indexed_rust_files()
        || changed.analyzed_rust_files() != 1
        || changed.reused_rust_files().checked_add(1) != Some(changed.indexed_rust_files())
    {
        return Err("one-file source mutation did not produce exact incremental reuse".into());
    }
    Ok(())
}

fn validate_inputs(arguments: &Arguments) -> ProbeResult<()> {
    if !arguments.repository.is_dir() || !arguments.cli.is_file() {
        return Err("repository and CLI inputs must identify existing filesystem objects".into());
    }
    if arguments.database.try_exists()? {
        return Err("benchmark database must not already exist".into());
    }
    let database_parent = arguments
        .database
        .parent()
        .ok_or("benchmark database must have a parent directory")?;
    if !database_parent.is_dir() {
        return Err("benchmark database parent must already exist".into());
    }
    Ok(())
}

struct Arguments {
    repository: PathBuf,
    database: PathBuf,
    cli: PathBuf,
    runs: usize,
    budgets: BenchmarkBudgets,
}

#[derive(Clone, Copy)]
struct BenchmarkBudgets {
    max_full_index_wall_ms: u64,
    max_warm_query_p95_ms: u64,
    max_material_result_bytes: usize,
    max_database_bytes: u64,
    max_wal_bytes_after_completion: u64,
}

impl BenchmarkBudgets {
    fn max_warm_query_p95_us(self) -> ProbeResult<u64> {
        self.max_warm_query_p95_ms
            .checked_mul(1_000)
            .ok_or_else(|| "warm-query budget was not representable".into())
    }
}

struct InitialPhase {
    cold: LocalIndexReport,
    cold_wall: Duration,
    warm: LocalIndexReport,
    warm_wall: Duration,
    search: search::SearchMetrics,
}

struct CurrentMemory {
    exact_memory: memory::ExactMemoryInput,
    canonical_memory_bytes: u64,
}

impl Arguments {
    fn parse() -> ProbeResult<Self> {
        let mut arguments = env::args_os();
        let _program = arguments.next();
        let repository = PathBuf::from(arguments.next().ok_or(Self::usage())?);
        let database = PathBuf::from(arguments.next().ok_or(Self::usage())?);
        let cli = PathBuf::from(arguments.next().ok_or(Self::usage())?);
        let runs = parse_runs(arguments.next())?;
        let max_full_index_wall_ms = parse_budget(
            arguments.next(),
            "full-index wall milliseconds",
            1,
            MAX_ADMITTED_WALL_MS,
        )?;
        let max_warm_query_p95_ms = parse_budget(
            arguments.next(),
            "warm-query P95 milliseconds",
            1,
            MAX_ADMITTED_WALL_MS,
        )?;
        let max_material_result_bytes = usize::try_from(parse_budget(
            arguments.next(),
            "material-result bytes",
            1,
            MAX_ADMITTED_RESULT_BYTES,
        )?)?;
        let max_database_bytes = parse_budget(
            arguments.next(),
            "database bytes",
            1,
            MAX_ADMITTED_STORAGE_BYTES,
        )?;
        let max_wal_bytes_after_completion = parse_budget(
            arguments.next(),
            "post-completion WAL bytes",
            0,
            MAX_ADMITTED_STORAGE_BYTES,
        )?;
        if arguments.next().is_some() {
            return Err(Self::usage().into());
        }
        Ok(Self {
            repository,
            database,
            cli,
            runs,
            budgets: BenchmarkBudgets {
                max_full_index_wall_ms,
                max_warm_query_p95_ms,
                max_material_result_bytes,
                max_database_bytes,
                max_wal_bytes_after_completion,
            },
        })
    }

    const fn usage() -> &'static str {
        "usage: phase0_product_probe <repository> <database> <repowitness-cli> \
         <runs> <full-index-ms> <warm-p95-ms> <result-bytes> <database-bytes> <wal-bytes>"
    }
}

fn parse_runs(value: Option<OsString>) -> ProbeResult<usize> {
    let runs = match value {
        Some(value) => value
            .to_str()
            .ok_or("run count must be UTF-8")?
            .parse::<usize>()?,
        None => DEFAULT_RUNS,
    };
    if !(2..=MAX_RUNS).contains(&runs) {
        return Err("run count must be between 2 and 100".into());
    }
    Ok(runs)
}

fn parse_budget(
    value: Option<OsString>,
    label: &str,
    minimum: u64,
    maximum: u64,
) -> ProbeResult<u64> {
    let value = value
        .ok_or_else(|| format!("{label} budget is required"))?
        .into_string()
        .map_err(|_| format!("{label} budget must be UTF-8"))?
        .parse::<u64>()?;
    if !(minimum..=maximum).contains(&value) {
        return Err(format!("{label} budget must be between {minimum} and {maximum}").into());
    }
    Ok(value)
}

struct Report<'a> {
    arguments: &'a Arguments,
    cold: LocalIndexReport,
    cold_wall: Duration,
    warm: LocalIndexReport,
    warm_wall: Duration,
    changed: LocalIndexReport,
    search: search::SearchMetrics,
    mcp: mcp::McpMetrics,
    configuration: [u8; 32],
    canonical_memory_bytes: u64,
    database_bytes: u64,
    wal_bytes: u64,
}

fn emit_report(report: Report<'_>) {
    println!("benchmark_id={BENCHMARK_ID}");
    println!("probe_runs={}", report.arguments.runs);
    println!("cold_index_wall_us={}", report.cold_wall.as_micros());
    println!("warm_index_wall_us={}", report.warm_wall.as_micros());
    println!("warm_query_p50_us={}", report.search.warm_p50_us);
    println!("warm_query_p95_us={}", report.search.warm_p95_us);
    println!(
        "required_evidence_verified={}",
        report.search.required_evidence_verified
    );
    println!("initial_generation={}", report.cold.generation().get());
    println!("warm_generation={}", report.warm.generation().get());
    println!("changed_generation={}", report.changed.generation().get());
    println!("repository_paths={}", report.cold.discovered_paths());
    println!("rust_files={}", report.cold.indexed_rust_files());
    println!("source_bytes={}", report.cold.total_source_bytes());
    println!("symbol_facts={}", report.cold.total_facts());
    println!("syntax_error_nodes={}", report.cold.syntax_error_nodes());
    println!("warm_reused_rust_files={}", report.warm.reused_rust_files());
    println!(
        "changed_reused_rust_files={}",
        report.changed.reused_rust_files()
    );
    println!("canonical_memory_bytes={}", report.canonical_memory_bytes);
    println!("mcp_tool_count={}", report.mcp.tool_count);
    println!(
        "mcp_material_result_bytes={}",
        report.mcp.material_result_bytes
    );
    println!(
        "resolved_configuration_digest_sha256={}",
        metrics::hex_digest(&report.configuration)
    );
    println!("database_bytes={}", report.database_bytes);
    println!("wal_bytes={}", report.wal_bytes);
    println!(
        "budget_max_full_index_wall_ms={}",
        report.arguments.budgets.max_full_index_wall_ms
    );
    println!(
        "budget_max_warm_query_p95_ms={}",
        report.arguments.budgets.max_warm_query_p95_ms
    );
    println!(
        "budget_max_material_result_bytes={}",
        report.arguments.budgets.max_material_result_bytes
    );
    println!(
        "budget_max_database_bytes={}",
        report.arguments.budgets.max_database_bytes
    );
    println!(
        "budget_max_wal_bytes_after_completion={}",
        report.arguments.budgets.max_wal_bytes_after_completion
    );
    println!("memory_current_before_change=true");
    println!("memory_stale_after_change=true");
    println!("stale_memory_context_excluded=true");
    println!("default_mcp_memory_writes_enabled=false");
    println!("correctness_failures=0");
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{BenchmarkBudgets, parse_budget, parse_runs, validate_storage_budgets};

    const BUDGETS: BenchmarkBudgets = BenchmarkBudgets {
        max_full_index_wall_ms: 10_000,
        max_warm_query_p95_ms: 250,
        max_material_result_bytes: 49_152,
        max_database_bytes: 4 * 1024 * 1024,
        max_wal_bytes_after_completion: 0,
    };

    #[test]
    fn run_count_is_bounded_before_allocating_samples() {
        assert_eq!(parse_runs(None).expect("default"), 5);
        assert_eq!(parse_runs(Some(OsString::from("2"))).expect("minimum"), 2);
        assert_eq!(
            parse_runs(Some(OsString::from("100"))).expect("maximum"),
            100
        );
        assert!(parse_runs(Some(OsString::from("1"))).is_err());
        assert!(parse_runs(Some(OsString::from("101"))).is_err());
        assert!(parse_runs(Some(OsString::from("not-a-count"))).is_err());
    }

    #[test]
    fn storage_budgets_are_inclusive_and_fail_closed() {
        assert!(validate_storage_budgets(BUDGETS.max_database_bytes, 0, BUDGETS).is_ok());
        assert!(validate_storage_budgets(BUDGETS.max_database_bytes + 1, 0, BUDGETS).is_err());
        assert!(validate_storage_budgets(1, 1, BUDGETS).is_err());
    }

    #[test]
    fn manifest_budget_arguments_are_bounded() {
        assert_eq!(
            parse_budget(Some(OsString::from("250")), "query", 1, 1_000).expect("valid"),
            250
        );
        assert!(parse_budget(Some(OsString::from("0")), "query", 1, 1_000).is_err());
        assert!(parse_budget(Some(OsString::from("1001")), "query", 1, 1_000).is_err());
        assert!(parse_budget(Some(OsString::from("invalid")), "query", 1, 1_000).is_err());
    }
}
