use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Instant;

use super::{
    GixIndexSnapshot, TempRepository, fixture_git_command, git_cli_cached_paths,
    gix_index_snapshot, gix_index_snapshot_from_repository, open_gix_repository, owned_paths,
};

const PERFORMANCE_PATHS: u64 = 50_000;
const PERFORMANCE_SAMPLES: usize = 20;
const INTERRUPT_CHILD_ENV: &str = "REPOWITNESS_GIX_INTERRUPT_CHILD";
const INTERRUPT_TEST_NAME: &str =
    "git_paths::gix_spike_tests::performance::index_loading_does_not_observe_the_gix_global_interrupt";

struct GixInterruptReset;

impl Drop for GixInterruptReset {
    fn drop(&mut self) {
        gix::interrupt::reset();
    }
}

#[derive(Debug)]
struct TimingSummary {
    first_micros: u128,
    minimum_micros: u128,
    median_micros: u128,
    p95_micros: u128,
    maximum_micros: u128,
}

#[test]
fn index_loading_does_not_observe_the_gix_global_interrupt() {
    if std::env::var_os(INTERRUPT_CHILD_ENV).is_none() {
        let status = Command::new(
            std::env::current_exe().expect("the unit-test executable path must be available"),
        )
        .args(["--exact", INTERRUPT_TEST_NAME])
        .env(INTERRUPT_CHILD_ENV, "1")
        .status()
        .expect("the isolated gix-interrupt sentinel must start");
        assert!(
            status.success(),
            "the isolated gix-interrupt sentinel failed with {status}"
        );
        return;
    }

    let repository = TempRepository::new(None);
    repository.write("tracked.rs", b"fn tracked() {}\n");
    repository.git_text(&["add", "--", "tracked.rs"]);

    gix::interrupt::reset();
    let _reset = GixInterruptReset;
    gix::interrupt::trigger();
    assert!(gix::interrupt::is_triggered());

    let snapshot = gix_index_snapshot(repository.root());
    assert_eq!(snapshot.paths, [b"tracked.rs".to_vec()]);
}

#[test]
#[ignore = "opt-in synthetic performance probe"]
fn gix_and_sanitized_git_report_cold_and_warm_performance() {
    let repository = TempRepository::new(None);
    populate_synthetic_index(&repository, PERFORMANCE_PATHS);

    let (expected, gix_first_micros) =
        timed(|| gix_index_snapshot(repository.root()));
    let (cli_first, git_first_micros) =
        timed(|| owned_paths(&git_cli_cached_paths(repository.root())));
    assert_eq!(cli_first, expected.paths);

    let mut gix_fresh_micros = Vec::with_capacity(PERFORMANCE_SAMPLES);
    for _ in 0..PERFORMANCE_SAMPLES {
        let (snapshot, elapsed) = timed(|| gix_index_snapshot(repository.root()));
        assert_same_snapshot(&snapshot, &expected);
        gix_fresh_micros.push(elapsed);
    }

    let retained = open_gix_repository(repository.root());
    let (retained_first, gix_retained_first_micros) =
        timed(|| gix_index_snapshot_from_repository(&retained));
    assert_same_snapshot(&retained_first, &expected);
    let mut gix_retained_micros = Vec::with_capacity(PERFORMANCE_SAMPLES);
    for _ in 0..PERFORMANCE_SAMPLES {
        let (snapshot, elapsed) =
            timed(|| gix_index_snapshot_from_repository(&retained));
        assert_same_snapshot(&snapshot, &expected);
        gix_retained_micros.push(elapsed);
    }

    let mut git_subsequent_micros = Vec::with_capacity(PERFORMANCE_SAMPLES);
    for _ in 0..PERFORMANCE_SAMPLES {
        let (paths, elapsed) =
            timed(|| owned_paths(&git_cli_cached_paths(repository.root())));
        assert_eq!(paths, expected.paths);
        git_subsequent_micros.push(elapsed);
    }

    report(
        "gix_fresh",
        summary(gix_first_micros, gix_fresh_micros),
    );
    report(
        "gix_retained",
        summary(gix_retained_first_micros, gix_retained_micros),
    );
    report(
        "git_cli",
        summary(git_first_micros, git_subsequent_micros),
    );
    println!("git_discovery_paths={PERFORMANCE_PATHS}");
    println!("git_discovery_samples={PERFORMANCE_SAMPLES}");
}

fn populate_synthetic_index(repository: &TempRepository, path_count: u64) {
    let empty_blob = repository.git_output_text(&["hash-object", "-w", "--stdin"]);
    let mut command = fixture_git_command(repository.root());
    command
        .args(["update-index", "--index-info"])
        .stdin(Stdio::piped());
    let mut child = command
        .spawn()
        .expect("synthetic index command must start");
    let mut stdin = child
        .stdin
        .take()
        .expect("synthetic index command must expose stdin");

    for ordinal in 0..path_count {
        writeln!(
            stdin,
            "100644 {empty_blob}\tbenchmark/{ordinal:08}.rs"
        )
        .expect("synthetic index input must be written");
    }
    drop(stdin);

    let status = child
        .wait()
        .expect("synthetic index command must be reaped");
    assert!(
        status.success(),
        "synthetic index command failed with {status}"
    );
}

fn timed<T>(operation: impl FnOnce() -> T) -> (T, u128) {
    let started = Instant::now();
    let result = operation();
    (result, started.elapsed().as_micros())
}

fn assert_same_snapshot(actual: &GixIndexSnapshot, expected: &GixIndexSnapshot) {
    assert_eq!(actual.paths, expected.paths);
    assert_eq!(actual.raw_entry_count, expected.raw_entry_count);
    assert_eq!(actual.sparse_entry_count, expected.sparse_entry_count);
    assert_eq!(
        actual.submodule_entry_count,
        expected.submodule_entry_count
    );
}

fn summary(first_micros: u128, mut samples: Vec<u128>) -> TimingSummary {
    samples.sort_unstable();
    TimingSummary {
        first_micros,
        minimum_micros: samples[0],
        median_micros: percentile(&samples, 50),
        p95_micros: percentile(&samples, 95),
        maximum_micros: samples[samples.len() - 1],
    }
}

fn percentile(sorted_samples: &[u128], percentile: usize) -> u128 {
    let rank = sorted_samples
        .len()
        .saturating_mul(percentile)
        .div_ceil(100)
        .saturating_sub(1);
    sorted_samples[rank]
}

fn report(adapter: &str, summary: TimingSummary) {
    println!("{adapter}_first_micros={}", summary.first_micros);
    println!("{adapter}_minimum_micros={}", summary.minimum_micros);
    println!("{adapter}_median_micros={}", summary.median_micros);
    println!("{adapter}_p95_micros={}", summary.p95_micros);
    println!("{adapter}_maximum_micros={}", summary.maximum_micros);
}
