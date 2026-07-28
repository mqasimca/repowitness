//! Controlled Phase 0 comparison against source-only and naive-memory baselines.

use std::{
    env,
    error::Error,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
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

#[path = "phase0_comparison_probe/lexical.rs"]
mod lexical;
#[allow(
    dead_code,
    reason = "the shared helper also contains the product probe's mutation scenario"
)]
#[path = "phase0_product_probe/memory.rs"]
mod memory;

type ProbeResult<T> = Result<T, Box<dyn Error>>;

const COMPARISON_ID: &str = "negative-bulk-temporal-decision";
const MIGRATION_TIMESTAMP: u64 = 1_722_000_000_000;
const APPROVAL_TIMESTAMP: u64 = 1_722_000_000_001;
const MEMORY_ACTOR: &str = "phase0-comparison";
const MEMORY_QUERY: &str = "check";
const CURRENT_RUST_FILES: u64 = 28;
const CHANGED_RUST_FILES: u64 = 2;
const COMPARISON_MEMORY: memory::MemoryInputSpec = memory::MemoryInputSpec::new(
    [0xC7; 16],
    b"src/frame.rs",
    "check",
    "RESP negative bulk length behavior",
    "Frame::check accepts negative bulk lengths other than -1.",
    MEMORY_ACTOR,
);

fn main() {
    if let Err(error) = run() {
        eprintln!("phase0 comparison probe failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> ProbeResult<()> {
    let arguments = Arguments::parse()?;
    validate_inputs(&arguments)?;
    let repository_digest = RepositoryIdentityDigest::new([0xC4; 32]);
    let repository_identity = RepositoryIdentityTextV1::encode(repository_digest);
    let cancelled = Arc::new(AtomicBool::new(false));
    match arguments.phase {
        Phase::Base => run_base(
            &arguments,
            repository_digest,
            repository_identity.as_str(),
            &cancelled,
        ),
        Phase::Changed => run_changed(&arguments, repository_identity.as_str(), &cancelled),
    }
}

fn run_base(
    arguments: &Arguments,
    repository_digest: RepositoryIdentityDigest,
    repository_identity: &str,
    cancelled: &Arc<AtomicBool>,
) -> ProbeResult<()> {
    let index_started = Instant::now();
    let indexed = index(arguments, repository_identity, cancelled)?;
    let index_wall_us = index_started.elapsed().as_micros();
    if indexed.indexed_rust_files() == 0
        || indexed.analyzed_rust_files() != indexed.indexed_rust_files()
        || indexed.reused_rust_files() != 0
        || indexed.syntax_error_nodes() != 0
    {
        return Err("comparison base revision was not indexed completely".into());
    }

    let lexical_started = Instant::now();
    let lexical = lexical::observe(&arguments.repository)?;
    let lexical_wall_us = lexical_started.elapsed().as_micros();
    if lexical.relation() != lexical::EvidenceRelation::Supports {
        return Err("source-only baseline did not identify the base behavior".into());
    }

    let exact_memory = memory::exact_memory_input_for(
        &arguments.repository,
        &arguments.database,
        repository_digest,
        &COMPARISON_MEMORY,
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
        return Err("comparison memory was not newly created".into());
    }
    let approved = approve_local_memory(
        LocalMemoryApprovalRequest::new(
            &arguments.repository,
            &arguments.database,
            repository_identity,
            exact_memory.record_id(),
            MEMORY_ACTOR,
            MIGRATION_TIMESTAMP,
            APPROVAL_TIMESTAMP,
        ),
        Arc::clone(cancelled),
    )?;
    if approved.revision() != written.revision() || !approved.approval_inserted() {
        return Err("comparison memory was not approved exactly once".into());
    }
    let projection = revalidate(arguments, repository_identity, cancelled)?;
    if projection.generation() != indexed.generation()
        || projection.projected_records() != 1
        || projection.unresolved_records() != 0
    {
        return Err("comparison base projection was incomplete".into());
    }
    validate_recall(
        arguments,
        repository_identity,
        MemoryEffectiveState::Current,
        MemoryRecallEvidenceOutcome::Exact,
        cancelled,
    )?;
    validate_context(arguments, repository_identity, true, cancelled)?;
    emit_phase_report(PhaseReport {
        phase: Phase::Base,
        indexed,
        index_wall_us,
        lexical,
        lexical_wall_us,
    });
    Ok(())
}

fn run_changed(
    arguments: &Arguments,
    repository_identity: &str,
    cancelled: &Arc<AtomicBool>,
) -> ProbeResult<()> {
    let index_started = Instant::now();
    let indexed = index(arguments, repository_identity, cancelled)?;
    let index_wall_us = index_started.elapsed().as_micros();
    if indexed.indexed_rust_files() != CURRENT_RUST_FILES
        || indexed.analyzed_rust_files() != CHANGED_RUST_FILES
        || indexed
            .reused_rust_files()
            .checked_add(indexed.analyzed_rust_files())
            != Some(indexed.indexed_rust_files())
        || indexed.syntax_error_nodes() != 0
    {
        return Err("comparison changed revision did not invalidate the exact files".into());
    }

    let lexical_started = Instant::now();
    let lexical = lexical::observe(&arguments.repository)?;
    let lexical_wall_us = lexical_started.elapsed().as_micros();
    if lexical.relation() != lexical::EvidenceRelation::Contradicts {
        return Err("source-only baseline did not identify the changed behavior".into());
    }

    let projection = revalidate(arguments, repository_identity, cancelled)?;
    if projection.generation() != indexed.generation()
        || projection.projected_records() != 1
        || projection.unresolved_records() != 0
    {
        return Err("comparison changed projection was incomplete".into());
    }
    validate_recall(
        arguments,
        repository_identity,
        MemoryEffectiveState::Stale,
        MemoryRecallEvidenceOutcome::Changed,
        cancelled,
    )?;
    validate_context(arguments, repository_identity, false, cancelled)?;
    emit_phase_report(PhaseReport {
        phase: Phase::Changed,
        indexed,
        index_wall_us,
        lexical,
        lexical_wall_us,
    });
    println!("naive_memory_stale_claims_exposed=1");
    println!("repowitness_stale_claims_exposed=0");
    println!("comparative_decision_changed=true");
    println!("comparative_outcome_pass=true");
    Ok(())
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

fn validate_recall(
    arguments: &Arguments,
    repository_identity: &str,
    expected_state: MemoryEffectiveState,
    expected_outcome: MemoryRecallEvidenceOutcome,
    cancelled: &Arc<AtomicBool>,
) -> ProbeResult<()> {
    let recalled = recall_local_memory(
        LocalMemoryRecallRequest::new(
            &arguments.database,
            repository_identity,
            LocalMemoryRecallSelection::Query(MEMORY_QUERY),
        ),
        Arc::clone(cancelled),
    )?;
    if recalled.records().len() != 1
        || recalled.records()[0].effective_state() != expected_state
        || recalled.records()[0].evidence().len() != 1
        || recalled.records()[0].evidence()[0].outcome() != expected_outcome
    {
        return Err("comparison recall did not preserve the expected temporal state".into());
    }
    Ok(())
}

fn validate_context(
    arguments: &Arguments,
    repository_identity: &str,
    expect_memory: bool,
    cancelled: &Arc<AtomicBool>,
) -> ProbeResult<()> {
    let context = build_local_context(
        LocalContextBuildRequest::new(
            &arguments.repository,
            &arguments.database,
            repository_identity,
            MEMORY_QUERY,
        ),
        Arc::clone(cancelled),
    )?;
    let memory_items = context
        .items()
        .iter()
        .filter(|item| matches!(item, ContextItem::Memory(_)))
        .count();
    if context.coverage().source_included() == 0 {
        return Err("comparison context omitted all source evidence".into());
    }
    if expect_memory {
        if context.coverage().memory_included() != 1 || memory_items != 1 {
            return Err("current comparison memory was not included in context".into());
        }
    } else if context.coverage().memory_included() != 0
        || context.coverage().memory_non_current_omitted() != 1
        || memory_items != 0
        || !context
            .omissions()
            .contains(&ContextOmission::MemoryNotCurrent(1))
    {
        return Err("stale comparison memory was not explicitly excluded".into());
    }
    Ok(())
}

fn validate_inputs(arguments: &Arguments) -> ProbeResult<()> {
    if !arguments.repository.is_dir() {
        return Err("comparison repository must be an existing directory".into());
    }
    match arguments.phase {
        Phase::Base if arguments.database.try_exists()? => {
            Err("comparison base database must not already exist".into())
        }
        Phase::Changed if !arguments.database.is_file() => {
            Err("comparison changed database must already exist".into())
        }
        _ => {
            let parent = arguments
                .database
                .parent()
                .ok_or("comparison database must have a parent directory")?;
            if !parent.is_dir() {
                return Err("comparison database parent must already exist".into());
            }
            Ok(())
        }
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Base,
    Changed,
}

impl Phase {
    fn parse(value: &str) -> ProbeResult<Self> {
        match value {
            "base" => Ok(Self::Base),
            "changed" => Ok(Self::Changed),
            _ => Err(Arguments::usage().into()),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Changed => "changed",
        }
    }

    const fn decision(self) -> &'static str {
        match self {
            Self::Base => "bug-present",
            Self::Changed => "bug-fixed",
        }
    }
}

struct Arguments {
    phase: Phase,
    repository: PathBuf,
    database: PathBuf,
}

impl Arguments {
    fn parse() -> ProbeResult<Self> {
        let mut arguments = env::args_os();
        let _program = arguments.next();
        let phase = arguments
            .next()
            .and_then(|value| value.into_string().ok())
            .ok_or(Self::usage())?;
        let repository = PathBuf::from(arguments.next().ok_or(Self::usage())?);
        let database = PathBuf::from(arguments.next().ok_or(Self::usage())?);
        if arguments.next().is_some() {
            return Err(Self::usage().into());
        }
        Ok(Self {
            phase: Phase::parse(&phase)?,
            repository,
            database,
        })
    }

    const fn usage() -> &'static str {
        "usage: phase0_comparison_probe <base|changed> <repository> <database>"
    }
}

struct PhaseReport {
    phase: Phase,
    indexed: LocalIndexReport,
    index_wall_us: u128,
    lexical: lexical::LexicalMetrics,
    lexical_wall_us: u128,
}

fn emit_phase_report(report: PhaseReport) {
    println!("comparison_id={COMPARISON_ID}");
    println!("comparison_phase={}", report.phase.as_str());
    println!("comparison_decision={}", report.phase.decision());
    println!("comparison_index_wall_us={}", report.index_wall_us);
    println!(
        "comparison_generation={}",
        report.indexed.generation().get()
    );
    println!(
        "comparison_analyzed_rust_files={}",
        report.indexed.analyzed_rust_files()
    );
    println!(
        "comparison_reused_rust_files={}",
        report.indexed.reused_rust_files()
    );
    println!("lexical_scan_wall_us={}", report.lexical_wall_us);
    println!("lexical_relation={}", report.lexical.relation().as_str());
    println!(
        "lexical_old_behavior_matches={}",
        report.lexical.old_behavior_matches()
    );
    println!(
        "lexical_fix_evidence_matches={}",
        report.lexical.fix_evidence_matches()
    );
    println!(
        "lexical_scanned_rust_files={}",
        report.lexical.scanned_rust_files()
    );
    println!(
        "lexical_scanned_source_bytes={}",
        report.lexical.scanned_source_bytes()
    );
    println!("naive_memory_claim_exposed=true");
    println!(
        "repowitness_memory_effective_state={}",
        match report.phase {
            Phase::Base => "current",
            Phase::Changed => "stale",
        }
    );
    println!(
        "repowitness_memory_context_included={}",
        matches!(report.phase, Phase::Base)
    );
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{COMPARISON_ID, Phase, lexical};

    #[test]
    fn phase_parser_is_closed_to_the_two_manifest_states() {
        assert!(matches!(Phase::parse("base"), Ok(Phase::Base)));
        assert!(matches!(Phase::parse("changed"), Ok(Phase::Changed)));
        assert!(Phase::parse("other").is_err());
    }

    #[test]
    fn executable_lexical_contract_matches_the_manifest() {
        let manifest: Value =
            serde_json::from_str(include_str!("../../../benchmarks/phase0/manifest.json"))
                .expect("valid benchmark manifest");
        let evaluation = manifest["comparative_evaluations"]
            .as_array()
            .expect("comparative evaluations")
            .iter()
            .find(|evaluation| evaluation["id"] == COMPARISON_ID)
            .expect("compiled comparison");
        assert_eq!(
            evaluation["lexical_signals"]["base_support"]
                .as_str()
                .expect("base signal")
                .as_bytes(),
            lexical::base_support_literal()
        );
        assert_eq!(
            evaluation["lexical_signals"]["changed_contradiction"]
                .as_str()
                .expect("changed signal")
                .as_bytes(),
            lexical::changed_contradiction_literal()
        );
        let limits = &evaluation["lexical_limits"];
        assert_eq!(limits["max_paths"].as_u64(), Some(lexical::max_paths()));
        assert_eq!(
            limits["max_file_bytes"].as_u64(),
            Some(lexical::max_file_bytes())
        );
        assert_eq!(
            limits["max_total_source_bytes"].as_u64(),
            Some(lexical::max_total_source_bytes())
        );
        assert_eq!(limits["deadline_ms"].as_u64(), Some(lexical::deadline_ms()));
    }
}
