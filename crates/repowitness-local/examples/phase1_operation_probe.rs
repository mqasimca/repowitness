//! End-to-end Phase 1 operation and resource probe over the pinned public corpus.

use std::{
    env,
    error::Error,
    ffi::OsString,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use repowitness_application::{
    ConfigurationLayer, ConfigurationLayerKind, ConfigurationPolicyOverrides,
    ConfigurationPreferenceOverrides,
};
use repowitness_local::{
    LocalIndexReport, LocalIndexRequest, LocalRetentionApplyRequest, LocalRetentionPins,
    LocalRetentionPlanRequest, LocalWatchExit, LocalWatchReconciliation, LocalWatchRequest,
    ResolvedConfiguration, apply_local_retention, index_local_repository, plan_local_retention,
    resolve_configuration, watch_local_repository,
};

#[path = "phase1_operation_probe/graph.rs"]
mod graph;
#[path = "phase1_operation_probe/metrics.rs"]
mod metrics;

type ProbeResult<T> = Result<T, Box<dyn Error>>;

const BENCHMARK_ID: &str = "phase1-trustworthy-local-core-v1";
const MIGRATION_TIMESTAMP: u64 = 1_722_000_000_000;
const MAX_RUNS: usize = 1_000;
const MAX_BUDGET_MS: u64 = 3_600_000;
const MAX_RESULT_BYTES: u64 = 24 * 1024 * 1024;
const MAX_STORAGE_BYTES: u64 = 16 * 1024 * 1024 * 1024;

fn main() {
    if let Err(error) = run() {
        eprintln!("phase1 operation probe failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> ProbeResult<()> {
    let arguments = Arguments::parse()?;
    validate_inputs(&arguments)?;
    let configuration = benchmark_configuration(arguments.watch_session_ms())?;
    let full_index_started = Instant::now();
    let initial = index(&arguments, &configuration)?;
    let full_index_wall = full_index_started.elapsed();
    if full_index_wall > Duration::from_millis(arguments.max_full_index_wall_ms) {
        return Err("cold full-index wall time exceeded the resource budget".into());
    }

    let quiet = measure_quiet_reconciliation(&arguments, &configuration, initial)?;
    let graph = graph::measure(
        &arguments.database,
        &arguments.repository_identity,
        &configuration,
        arguments.graph_runs,
        arguments.max_graph_read_ms,
        arguments.max_material_result_bytes,
    )?;
    let retention = measure_retention(&arguments, &configuration)?;
    let database_bytes = metrics::required_file_size(&arguments.database)?;
    let wal_bytes = metrics::wal_file_size(&arguments.database)?;
    validate_storage_budgets(&arguments, database_bytes, wal_bytes)?;

    emit_report(
        &arguments,
        &configuration,
        full_index_wall,
        quiet,
        graph,
        retention,
        StorageMetrics {
            database_bytes,
            wal_bytes,
        },
    );
    Ok(())
}

fn benchmark_configuration(watcher_poll_interval_ms: u64) -> ProbeResult<ResolvedConfiguration> {
    let preferences = ConfigurationPreferenceOverrides::try_new(
        None,
        None,
        None,
        None,
        Some(watcher_poll_interval_ms),
        None,
    )?;
    let layer = ConfigurationLayer::try_new(
        ConfigurationLayerKind::Repository,
        None,
        preferences,
        ConfigurationPolicyOverrides::default(),
    )?;
    Ok(resolve_configuration(&[layer])?)
}

fn index(
    arguments: &Arguments,
    configuration: &ResolvedConfiguration,
) -> ProbeResult<LocalIndexReport> {
    Ok(index_local_repository(
        LocalIndexRequest::new(
            &arguments.repository,
            &arguments.database,
            &arguments.repository_identity,
            MIGRATION_TIMESTAMP,
        )
        .with_configuration(configuration),
        Arc::new(AtomicBool::new(false)),
    )?)
}

fn measure_quiet_reconciliation(
    arguments: &Arguments,
    configuration: &ResolvedConfiguration,
    initial: LocalIndexReport,
) -> ProbeResult<QuietMetrics> {
    let mut samples = Vec::with_capacity(arguments.quiet_runs);
    let session = Duration::from_millis(arguments.watch_session_ms());
    let mut unchanged = 0_u64;
    for _ in 0..arguments.quiet_runs {
        let request = LocalWatchRequest::new(
            LocalIndexRequest::new(
                &arguments.repository,
                &arguments.database,
                &arguments.repository_identity,
                MIGRATION_TIMESTAMP,
            )
            .with_configuration(configuration),
        )
        .with_max_runtime(session)?;
        let started = Instant::now();
        let report = watch_local_repository(request, Arc::new(AtomicBool::new(false)))?;
        let elapsed = started.elapsed();
        if report.exit() != LocalWatchExit::DeadlineExceeded
            || report.last_reconciliation() != Some(LocalWatchReconciliation::Unchanged)
            || report
                .last_index()
                .is_none_or(|last| last.generation() != initial.generation())
        {
            return Err("quiet reconciliation changed or lost the active generation".into());
        }
        unchanged = unchanged
            .checked_add(1)
            .ok_or("quiet reconciliation count overflowed")?;
        samples.push(elapsed);
    }
    let p95 = metrics::nearest_rank_p95(&mut samples)?;
    if p95 > Duration::from_millis(arguments.max_quiet_poll_ms) {
        return Err("quiet reconciliation p95 exceeded the resource budget".into());
    }
    Ok(QuietMetrics {
        p95,
        unchanged,
        generation_delta: 0,
    })
}

fn measure_retention(
    arguments: &Arguments,
    configuration: &ResolvedConfiguration,
) -> ProbeResult<RetentionMetrics> {
    let _second_generation = index(arguments, configuration)?;
    let mut plan_samples = Vec::with_capacity(arguments.retention_runs);
    let mut apply_samples = Vec::with_capacity(arguments.retention_runs);
    let mut deleted_generations = 0_u64;

    for _ in 0..arguments.retention_runs {
        let _new_generation = index(arguments, configuration)?;
        let plan_started = Instant::now();
        let plan = plan_local_retention(plan_request(arguments, configuration)?)?;
        let plan_elapsed = plan_started.elapsed();
        if plan_elapsed > Duration::from_millis(arguments.max_retention_plan_ms)
            || plan.candidate_count() != 1
            || plan.more_work()
            || u64::from(plan.policy().retained_generations_per_source_slot())
                > arguments.max_retained_generations
        {
            return Err("retention planning violated the exact bounded workload".into());
        }

        let apply_started = Instant::now();
        let applied = apply_local_retention(LocalRetentionApplyRequest::try_new(
            &arguments.database,
            MIGRATION_TIMESTAMP,
            configuration,
            LocalRetentionPins::default(),
            plan.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            Duration::from_millis(arguments.max_retention_apply_ms),
        )?)?;
        let apply_elapsed = apply_started.elapsed();
        if apply_elapsed > Duration::from_millis(arguments.max_retention_apply_ms)
            || applied.generation_count() != 1
            || !applied.database_identity_confirmed()
            || !applied.shutdown_complete()
        {
            return Err("retention apply violated the exact bounded workload".into());
        }
        deleted_generations = deleted_generations
            .checked_add(applied.generation_count())
            .ok_or("retention deletion count overflowed")?;
        plan_samples.push(plan_elapsed);
        apply_samples.push(apply_elapsed);
    }

    let final_plan = plan_local_retention(plan_request(arguments, configuration)?)?;
    if final_plan.candidate_count() != 0 || final_plan.more_work() {
        return Err("retention did not converge to an exact no-op plan".into());
    }
    let published_generations = u64::try_from(arguments.retention_runs)?
        .checked_add(2)
        .ok_or("retention publication count overflowed")?;
    let retained_generations = published_generations
        .checked_sub(deleted_generations)
        .ok_or("retention deleted more generations than were published")?;
    if retained_generations > arguments.max_retained_generations {
        return Err("retained generations exceeded the resource budget".into());
    }

    Ok(RetentionMetrics {
        plan_p95: metrics::nearest_rank_p95(&mut plan_samples)?,
        apply_p95: metrics::nearest_rank_p95(&mut apply_samples)?,
        retained_generations,
        deleted_generations,
        final_candidates: final_plan.candidate_count(),
    })
}

fn plan_request<'a>(
    arguments: &Arguments,
    configuration: &'a ResolvedConfiguration,
) -> ProbeResult<LocalRetentionPlanRequest<'a>> {
    Ok(LocalRetentionPlanRequest::try_new(
        &arguments.database,
        MIGRATION_TIMESTAMP,
        configuration,
        LocalRetentionPins::default(),
        Arc::new(AtomicBool::new(false)),
        Duration::from_millis(arguments.max_retention_plan_ms),
    )?)
}

fn validate_inputs(arguments: &Arguments) -> ProbeResult<()> {
    validate_warm_query_contract(
        arguments.warm_query_runs,
        arguments.graph_runs,
        arguments.max_warm_query_ms,
        arguments.max_graph_read_ms,
        arguments.cold_and_warm_runs_required,
    )?;
    if !arguments.repository.is_dir() {
        return Err("repository input must identify an existing directory".into());
    }
    if arguments.database.try_exists()? {
        return Err("operation-probe database must not already exist".into());
    }
    if arguments.repository_identity.is_empty()
        || arguments.repository_identity.len() > 256
        || arguments.repository_identity.chars().any(char::is_control)
    {
        return Err("repository identity input is invalid".into());
    }
    Ok(())
}

fn validate_warm_query_contract(
    warm_query_runs: usize,
    graph_runs: usize,
    max_warm_query_ms: u64,
    max_graph_read_ms: u64,
    cold_and_warm_runs_required: bool,
) -> ProbeResult<()> {
    if !cold_and_warm_runs_required
        || warm_query_runs != graph_runs
        || max_warm_query_ms != max_graph_read_ms
    {
        return Err("warm-query and native-graph benchmark contracts must match".into());
    }
    Ok(())
}

fn validate_storage_budgets(
    arguments: &Arguments,
    database_bytes: u64,
    wal_bytes: u64,
) -> ProbeResult<()> {
    if database_bytes > arguments.max_database_bytes {
        return Err("SQLite database size exceeded the resource budget".into());
    }
    if wal_bytes > arguments.max_wal_bytes_after_completion {
        return Err("SQLite WAL exceeded the post-completion resource budget".into());
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct QuietMetrics {
    p95: Duration,
    unchanged: u64,
    generation_delta: u64,
}

#[derive(Clone, Copy)]
struct RetentionMetrics {
    plan_p95: Duration,
    apply_p95: Duration,
    retained_generations: u64,
    deleted_generations: u64,
    final_candidates: u64,
}

#[derive(Clone, Copy)]
struct StorageMetrics {
    database_bytes: u64,
    wal_bytes: u64,
}

fn emit_report(
    arguments: &Arguments,
    configuration: &ResolvedConfiguration,
    full_index_wall: Duration,
    quiet: QuietMetrics,
    graph: graph::GraphMetrics,
    retention: RetentionMetrics,
    storage: StorageMetrics,
) {
    println!("benchmark_id={BENCHMARK_ID}");
    println!("probe_kind=operation-level");
    println!(
        "resolved_configuration_digest_sha256={}",
        metrics::hex_digest(configuration.digest().as_bytes())
    );
    println!("sqlite_version={}", rusqlite::version());
    println!("cold_index_wall_us={}", full_index_wall.as_micros());
    println!("warm_query_runs={}", arguments.warm_query_runs);
    println!("warm_query_p95_us={}", graph.p95.as_micros());
    println!(
        "cold_and_warm_runs_required={}",
        arguments.cold_and_warm_runs_required
    );
    println!("quiet_poll_session_ms={}", arguments.quiet_poll_session_ms);
    println!("quiet_poll_runs={}", arguments.quiet_runs);
    println!("quiet_poll_p95_us={}", quiet.p95.as_micros());
    println!("quiet_poll_unchanged={}", quiet.unchanged);
    println!("quiet_poll_publications={}", quiet.generation_delta);
    println!("unchanged_generation_delta={}", quiet.generation_delta);
    println!("native_graph_read_runs={}", arguments.graph_runs);
    println!("native_graph_read_p95_us={}", graph.p95.as_micros());
    println!(
        "native_graph_operations_per_run={}",
        graph.operations_per_run
    );
    println!(
        "native_graph_material_result_bound_bytes={}",
        graph.material_result_bound_bytes
    );
    println!("mixed_generation_reads={}", graph.mixed_generation_reads);
    println!("retention_runs={}", arguments.retention_runs);
    println!("retention_plan_p95_us={}", retention.plan_p95.as_micros());
    println!("retention_apply_p95_us={}", retention.apply_p95.as_micros());
    println!(
        "retained_generations_per_source_slot={}",
        retention.retained_generations
    );
    println!(
        "retention_deleted_generations={}",
        retention.deleted_generations
    );
    println!("retention_final_candidates={}", retention.final_candidates);
    println!("database_bytes={}", storage.database_bytes);
    println!("wal_bytes={}", storage.wal_bytes);
    println!(
        "budget_max_full_index_wall_ms={}",
        arguments.max_full_index_wall_ms
    );
    println!(
        "budget_max_warm_query_p95_ms={}",
        arguments.max_warm_query_ms
    );
    println!(
        "budget_max_material_result_bytes={}",
        arguments.max_material_result_bytes
    );
    println!("budget_max_database_bytes={}", arguments.max_database_bytes);
    println!(
        "budget_max_wal_bytes_after_completion={}",
        arguments.max_wal_bytes_after_completion
    );
    println!("correctness_failures=0");
}

struct Arguments {
    repository: PathBuf,
    database: PathBuf,
    repository_identity: String,
    quiet_runs: usize,
    graph_runs: usize,
    retention_runs: usize,
    warm_query_runs: usize,
    cold_and_warm_runs_required: bool,
    quiet_poll_session_ms: u64,
    max_full_index_wall_ms: u64,
    max_quiet_poll_ms: u64,
    max_warm_query_ms: u64,
    max_graph_read_ms: u64,
    max_retention_plan_ms: u64,
    max_retention_apply_ms: u64,
    max_retained_generations: u64,
    max_material_result_bytes: u64,
    max_database_bytes: u64,
    max_wal_bytes_after_completion: u64,
}

impl Arguments {
    fn parse() -> ProbeResult<Self> {
        let mut values = env::args_os();
        let _program = values.next();
        let parsed = Self {
            repository: required_path(values.next(), "repository")?,
            database: required_path(values.next(), "database")?,
            repository_identity: required_text(values.next(), "repository identity")?,
            quiet_runs: parse_runs(values.next(), "quiet runs")?,
            graph_runs: parse_runs(values.next(), "graph runs")?,
            retention_runs: parse_runs(values.next(), "retention runs")?,
            warm_query_runs: parse_runs(values.next(), "warm-query runs")?,
            cold_and_warm_runs_required: parse_required_true(
                values.next(),
                "cold-and-warm requirement",
            )?,
            quiet_poll_session_ms: parse_budget(values.next(), "quiet-poll session", 100, 10_000)?,
            max_full_index_wall_ms: parse_budget(values.next(), "full index", 1, MAX_BUDGET_MS)?,
            max_quiet_poll_ms: parse_budget(values.next(), "quiet poll", 1, MAX_BUDGET_MS)?,
            max_warm_query_ms: parse_budget(values.next(), "warm query", 1, MAX_BUDGET_MS)?,
            max_graph_read_ms: parse_budget(values.next(), "graph read", 1, MAX_BUDGET_MS)?,
            max_retention_plan_ms: parse_budget(values.next(), "retention plan", 1, MAX_BUDGET_MS)?,
            max_retention_apply_ms: parse_budget(
                values.next(),
                "retention apply",
                1,
                MAX_BUDGET_MS,
            )?,
            max_retained_generations: parse_budget(
                values.next(),
                "retained generations",
                1,
                1_000,
            )?,
            max_material_result_bytes: parse_budget(
                values.next(),
                "material result",
                1,
                MAX_RESULT_BYTES,
            )?,
            max_database_bytes: parse_budget(values.next(), "database", 1, MAX_STORAGE_BYTES)?,
            max_wal_bytes_after_completion: parse_budget(
                values.next(),
                "post-completion WAL",
                0,
                MAX_STORAGE_BYTES,
            )?,
        };
        if values.next().is_some() {
            return Err(Self::usage().into());
        }
        Ok(parsed)
    }

    fn watch_session_ms(&self) -> u64 {
        self.quiet_poll_session_ms
    }

    const fn usage() -> &'static str {
        "usage: phase1_operation_probe <repository> <database> <repository-identity> \
         <quiet-runs> <graph-runs> <retention-runs> <warm-query-runs> \
         <cold-and-warm-required> <quiet-session-ms> <full-index-ms> \
         <quiet-ms> <warm-query-ms> \
         <graph-ms> <retention-plan-ms> <retention-apply-ms> \
         <retained-generations> <result-bytes> <database-bytes> <wal-bytes>"
    }
}

fn required_path(value: Option<OsString>, label: &str) -> ProbeResult<PathBuf> {
    value
        .map(PathBuf::from)
        .ok_or_else(|| format!("{label} is required").into())
}

fn required_text(value: Option<OsString>, label: &str) -> ProbeResult<String> {
    value
        .ok_or_else(|| format!("{label} is required"))?
        .into_string()
        .map_err(|_| format!("{label} must be UTF-8").into())
}

fn parse_runs(value: Option<OsString>, label: &str) -> ProbeResult<usize> {
    let value = required_text(value, label)?.parse::<usize>()?;
    if !(2..=MAX_RUNS).contains(&value) {
        return Err(format!("{label} must be between 2 and {MAX_RUNS}").into());
    }
    Ok(value)
}

fn parse_required_true(value: Option<OsString>, label: &str) -> ProbeResult<bool> {
    if required_text(value, label)? != "true" {
        return Err(format!("{label} must be true").into());
    }
    Ok(true)
}

fn parse_budget(
    value: Option<OsString>,
    label: &str,
    minimum: u64,
    maximum: u64,
) -> ProbeResult<u64> {
    let value = required_text(value, label)?.parse::<u64>()?;
    if !(minimum..=maximum).contains(&value) {
        return Err(format!("{label} must be between {minimum} and {maximum}").into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, time::Duration};

    use super::{
        MAX_RUNS, parse_budget, parse_required_true, parse_runs, validate_warm_query_contract,
    };

    #[test]
    fn run_counts_are_bounded_before_sample_allocation() {
        assert_eq!(parse_runs(Some(OsString::from("2")), "runs").unwrap(), 2);
        assert_eq!(
            parse_runs(Some(OsString::from(MAX_RUNS.to_string())), "runs").unwrap(),
            MAX_RUNS
        );
        assert!(parse_runs(Some(OsString::from("1")), "runs").is_err());
        assert!(parse_runs(Some(OsString::from("1001")), "runs").is_err());
    }

    #[test]
    fn budgets_are_positive_and_inclusively_bounded() {
        assert_eq!(
            parse_budget(Some(OsString::from("200")), "quiet", 200, 500).unwrap(),
            200
        );
        assert_eq!(
            parse_budget(Some(OsString::from("500")), "quiet", 200, 500).unwrap(),
            500
        );
        assert!(parse_budget(Some(OsString::from("199")), "quiet", 200, 500).is_err());
        assert!(parse_budget(Some(OsString::from("501")), "quiet", 200, 500).is_err());
    }

    #[test]
    fn duration_budget_comparison_does_not_truncate_submilliseconds() {
        assert!(Duration::from_micros(500_001) > Duration::from_millis(500));
    }

    #[test]
    fn warm_query_contract_is_explicit_and_cannot_drift_from_graph_reads() {
        assert!(validate_warm_query_contract(50, 50, 250, 250, true).is_ok());
        assert!(validate_warm_query_contract(49, 50, 250, 250, true).is_err());
        assert!(validate_warm_query_contract(50, 50, 249, 250, true).is_err());
        assert!(validate_warm_query_contract(50, 50, 250, 250, false).is_err());
        assert!(parse_required_true(Some(OsString::from("true")), "required").unwrap());
        assert!(parse_required_true(Some(OsString::from("false")), "required").is_err());
    }
}
