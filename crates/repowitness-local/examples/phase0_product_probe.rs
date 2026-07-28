//! End-to-end Phase 0 product and resource probe over the pinned benchmark corpus.

use std::{
    env,
    error::Error,
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
const MIGRATION_TIMESTAMP: u64 = 1_722_000_000_000;
const APPROVAL_TIMESTAMP: u64 = 1_722_000_000_001;
const MAX_FULL_INDEX_WALL: Duration = Duration::from_secs(10);

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
    )?;
    let configuration =
        metrics::active_configuration_digest(&arguments.database, repository_digest)?;
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
    validate_cold_index(cold, cold_wall)?;
    let warm_started = Instant::now();
    let warm = index(arguments, repository_identity, cancelled)?;
    let warm_wall = warm_started.elapsed();
    validate_warm_index(cold, warm)?;
    let search = search::verify_manifest_evidence(
        &arguments.database,
        repository_identity,
        arguments.runs,
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

fn validate_cold_index(report: LocalIndexReport, elapsed: Duration) -> ProbeResult<()> {
    if elapsed > MAX_FULL_INDEX_WALL {
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
        let runs = match arguments.next() {
            Some(value) => value
                .to_str()
                .ok_or("run count must be UTF-8")?
                .parse::<usize>()?,
            None => DEFAULT_RUNS,
        };
        if runs < 2 || arguments.next().is_some() {
            return Err(Self::usage().into());
        }
        Ok(Self {
            repository,
            database,
            cli,
            runs,
        })
    }

    const fn usage() -> &'static str {
        "usage: phase0_product_probe <repository> <database> <repowitness-cli> [runs]"
    }
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
    println!(
        "database_bytes={}",
        metrics::file_size(&report.arguments.database)
    );
    println!(
        "wal_bytes={}",
        metrics::wal_file_size(&report.arguments.database)
    );
    println!("memory_current_before_change=true");
    println!("memory_stale_after_change=true");
    println!("stale_memory_context_excluded=true");
    println!("default_mcp_memory_writes_enabled=false");
    println!("correctness_failures=0");
}
