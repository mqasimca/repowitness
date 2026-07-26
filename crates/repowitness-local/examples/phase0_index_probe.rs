//! Reproducible cold/warm Phase 0 local Rust preparation probe.

use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use repowitness_application::RustArtifactIdentity;
use repowitness_domain::{
    AnalysisSchemaDigest, ConfigurationDigest, ProducerManifestDigest, SourceManifestDigest,
};
use repowitness_local::{
    LocalRustIndexLimits, LocalRustIndexPreparation, prepare_local_rust_index,
};

const DEFAULT_RUNS: usize = 5;

fn main() {
    if let Err(error) = run() {
        eprintln!("phase0 index probe failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let root = arguments
        .next()
        .ok_or("usage: phase0_index_probe <repository> [runs]")?;
    let runs = match arguments.next() {
        Some(value) => value
            .to_str()
            .ok_or("run count must be UTF-8")?
            .parse::<usize>()?,
        None => DEFAULT_RUNS,
    };
    if runs < 2 {
        return Err("at least two runs are required for cold/warm reporting".into());
    }
    if arguments.next().is_some() {
        return Err("usage: phase0_index_probe <repository> [runs]".into());
    }

    let identity = RustArtifactIdentity::new(
        ProducerManifestDigest::new([0x11; 32]),
        ConfigurationDigest::new([0x22; 32]),
        AnalysisSchemaDigest::new([0x33; 32]),
        1,
    );
    let cancelled = AtomicBool::new(false);
    let mut baseline = None;
    let mut warm_microseconds = Vec::with_capacity(runs.saturating_sub(1));

    println!("benchmark_id=phase0-rust-evidence-memory-v1");
    println!("probe_runs={runs}");
    for run_index in 0..runs {
        let started = Instant::now();
        let preparation = prepare_local_rust_index(
            Path::new(&root),
            identity,
            LocalRustIndexLimits::default(),
            &cancelled,
        )?;
        let elapsed_microseconds = u64::try_from(started.elapsed().as_micros())
            .map_err(|_| "elapsed probe duration is not representable")?;
        validate_stable_output(&preparation, &mut baseline)?;
        let run_number = run_index.checked_add(1).ok_or("run number overflowed")?;
        let temperature = if run_index == 0 { "cold" } else { "warm" };
        println!("run={run_number} temperature={temperature} wall_us={elapsed_microseconds}");
        if run_index > 0 {
            warm_microseconds.push(elapsed_microseconds);
        }
    }

    warm_microseconds.sort_unstable();
    let p50 = nearest_rank(&warm_microseconds, 50)?;
    let p95 = nearest_rank(&warm_microseconds, 95)?;
    let baseline = baseline.ok_or("probe produced no baseline")?;
    println!("warm_p50_us={p50}");
    println!("warm_p95_us={p95}");
    println!("repository_paths={}", baseline.discovered_paths);
    println!("rust_files={}", baseline.rust_files);
    println!("skipped_non_rust_paths={}", baseline.skipped_non_rust_paths);
    println!("source_bytes={}", baseline.source_bytes);
    println!("symbol_facts={}", baseline.symbol_facts);
    println!("syntax_error_nodes={}", baseline.syntax_error_nodes);
    println!(
        "manifest_digest_sha256={}",
        hex_digest(baseline.manifest_digest)
    );
    println!("resolved_configuration_digest_sha256={}", "22".repeat(32));
    match peak_rss_kib() {
        Some(value) => println!("peak_rss_kib={value}"),
        None => println!("peak_rss_kib=unavailable"),
    }
    Ok(())
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct StableOutput {
    manifest_digest: SourceManifestDigest,
    discovered_paths: u64,
    rust_files: u64,
    skipped_non_rust_paths: u64,
    source_bytes: u64,
    symbol_facts: u64,
    syntax_error_nodes: u64,
}

impl StableOutput {
    fn from_preparation(preparation: &LocalRustIndexPreparation) -> Self {
        Self {
            manifest_digest: preparation.prepared().manifest_digest(),
            discovered_paths: preparation.discovered_paths(),
            rust_files: preparation.selected_rust_files(),
            skipped_non_rust_paths: preparation.skipped_non_rust_paths(),
            source_bytes: preparation.prepared().total_source_bytes(),
            symbol_facts: preparation.prepared().total_facts(),
            syntax_error_nodes: preparation.prepared().total_syntax_error_nodes(),
        }
    }
}

fn validate_stable_output(
    preparation: &LocalRustIndexPreparation,
    baseline: &mut Option<StableOutput>,
) -> Result<(), Box<dyn Error>> {
    let current = StableOutput::from_preparation(preparation);
    match baseline {
        Some(expected) if *expected != current => {
            Err("repeated preparation produced different logical output".into())
        }
        Some(_) => Ok(()),
        None => {
            *baseline = Some(current);
            Ok(())
        }
    }
}

fn nearest_rank(sorted: &[u64], percentile: usize) -> Result<u64, Box<dyn Error>> {
    if sorted.is_empty() || percentile == 0 || percentile > 100 {
        return Err("nearest-rank input is invalid".into());
    }
    let numerator = percentile
        .checked_mul(sorted.len())
        .ok_or("percentile rank overflowed")?;
    let rank = numerator
        .checked_add(99)
        .ok_or("percentile rank overflowed")?
        / 100;
    sorted
        .get(rank.saturating_sub(1))
        .copied()
        .ok_or_else(|| "percentile rank is outside the sample".into())
}

fn hex_digest(digest: SourceManifestDigest) -> String {
    let mut output = String::with_capacity(64);
    for byte in digest.as_bytes() {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[cfg(target_os = "linux")]
fn peak_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))?
        .trim()
        .strip_suffix(" kB")?
        .trim()
        .parse::<u64>()
        .ok()
}

#[cfg(not(target_os = "linux"))]
fn peak_rss_kib() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::{nearest_rank, peak_rss_kib};

    #[test]
    fn nearest_rank_is_deterministic_for_small_warm_samples() {
        assert_eq!(nearest_rank(&[10, 20, 30, 40], 50).expect("valid p50"), 20);
        assert_eq!(nearest_rank(&[10, 20, 30, 40], 95).expect("valid p95"), 40);
        assert!(nearest_rank(&[], 50).is_err());
        assert!(nearest_rank(&[1], 0).is_err());
        assert!(nearest_rank(&[1], 101).is_err());
        if let Some(peak_rss_kib) = peak_rss_kib() {
            assert!(peak_rss_kib > 0, "Linux high-water RSS must be nonzero");
        }
    }
}
