#![no_main]

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use libfuzzer_sys::fuzz_target;
use repowitness_local::{MemoryFormatControl, generate_memory_yaml, parse_memory_record};

const COMMIT_RECORD: &[u8] =
    include_bytes!("../../crates/repowitness-local/tests/fixtures/memory-v1/commit.yaml");
const WORKTREE_RECORD: &[u8] = include_bytes!(
    "../../crates/repowitness-local/tests/fixtures/memory-v1/worktree-relationship.yaml"
);

fn control(cancelled: &AtomicBool) -> MemoryFormatControl<'_> {
    MemoryFormatControl::new(cancelled, Instant::now() + Duration::from_secs(1))
}

fn exercise(input: &[u8]) {
    let cancelled = AtomicBool::new(false);
    let Ok(parsed) = parse_memory_record(input, control(&cancelled)) else {
        return;
    };
    let generated = generate_memory_yaml(parsed.record(), control(&cancelled))
        .expect("a validated memory record must have a bounded YAML representation");
    let reparsed = parse_memory_record(&generated, control(&cancelled))
        .expect("generated memory YAML must parse");
    assert_eq!(reparsed.record(), parsed.record());
    assert_eq!(reparsed.canonical_json(), parsed.canonical_json());
    assert_eq!(reparsed.digest(), parsed.digest());
}

fn mutate_seed(data: &[u8], seed: &[u8]) {
    let mut candidate = seed.to_vec();
    let offset = data
        .first()
        .copied()
        .map_or(0, |value| usize::from(value) % candidate.len());
    for (target, replacement) in candidate[offset..].iter_mut().zip(&data[1..]) {
        *target = *replacement;
    }
    exercise(&candidate);
}

fuzz_target!(|data: &[u8]| {
    exercise(data);
    if data.is_empty() {
        exercise(COMMIT_RECORD);
        exercise(WORKTREE_RECORD);
    } else if data[0] & 1 == 0 {
        mutate_seed(data, COMMIT_RECORD);
    } else {
        mutate_seed(data, WORKTREE_RECORD);
    }
});
