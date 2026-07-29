use super::super::{
    MAX_WATCHER_HINT_PATH_BYTES, MAX_WATCHER_HINT_PATHS, WatcherHintAccumulator,
    WatcherHintAdmission, WatcherHintLimitError, WatcherHintLimits,
};
use super::path;

fn batch_bytes(accumulator: &mut WatcherHintAccumulator) -> Vec<Vec<u8>> {
    accumulator
        .drain()
        .paths()
        .iter()
        .map(|path| path.as_bytes().to_vec())
        .collect()
}

#[test]
fn hint_limits_reject_zero_and_over_ceiling_without_state() {
    assert_eq!(
        WatcherHintLimits::try_new(0, 1),
        Err(WatcherHintLimitError::ZeroPathLimit)
    );
    assert_eq!(
        WatcherHintLimits::try_new(MAX_WATCHER_HINT_PATHS + 1, 1),
        Err(WatcherHintLimitError::PathLimitTooLarge)
    );
    assert_eq!(
        WatcherHintLimits::try_new(1, 0),
        Err(WatcherHintLimitError::ZeroPathByteLimit)
    );
    assert_eq!(
        WatcherHintLimits::try_new(1, MAX_WATCHER_HINT_PATH_BYTES + 1),
        Err(WatcherHintLimitError::PathByteLimitTooLarge)
    );
}

#[test]
fn distinct_path_limit_is_inclusive_and_one_over_discards_partial_hints() {
    let mut accumulator =
        WatcherHintAccumulator::new(WatcherHintLimits::try_new(2, 100).expect("valid limits"));
    assert_eq!(
        accumulator.record_hint(path(b"a.rs")),
        WatcherHintAdmission::Retained
    );
    assert_eq!(
        accumulator.record_hint(path(b"b.rs")),
        WatcherHintAdmission::Retained
    );
    assert_eq!(accumulator.pending_path_count().get(), 2);
    assert_eq!(
        accumulator.record_hint(path(b"c.rs")),
        WatcherHintAdmission::PathCountOverflow
    );
    assert!(accumulator.full_reconciliation_required());
    assert_eq!(accumulator.pending_path_count().get(), 0);

    let batch = accumulator.drain();
    assert!(batch.full_reconciliation_required());
    assert!(batch.causes().path_count_overflow());
    assert!(batch.paths().is_empty());
}

#[test]
fn aggregate_path_bytes_are_inclusive_and_checked_before_retention() {
    let mut exact =
        WatcherHintAccumulator::new(WatcherHintLimits::try_new(4, 9).expect("valid limits"));
    assert_eq!(
        exact.record_hint(path(b"aa.rs")),
        WatcherHintAdmission::Retained
    );
    assert_eq!(
        exact.record_hint(path(b"b.rs")),
        WatcherHintAdmission::Retained
    );
    assert_eq!(exact.pending_path_bytes().get(), 9);
    assert_eq!(
        batch_bytes(&mut exact),
        [b"aa.rs".to_vec(), b"b.rs".to_vec()]
    );

    let mut over =
        WatcherHintAccumulator::new(WatcherHintLimits::try_new(4, 8).expect("valid limits"));
    assert_eq!(
        over.record_hint(path(b"aa.rs")),
        WatcherHintAdmission::Retained
    );
    assert_eq!(
        over.record_hint(path(b"b.rs")),
        WatcherHintAdmission::PathByteOverflow
    );
    let batch = over.drain();
    assert!(batch.full_reconciliation_required());
    assert!(batch.causes().path_byte_overflow());
    assert!(batch.paths().is_empty());
}

#[test]
fn duplicates_and_reordering_produce_the_same_canonical_set() {
    let limits = WatcherHintLimits::try_new(8, 128).expect("valid limits");
    let mut first = WatcherHintAccumulator::new(limits);
    let mut second = WatcherHintAccumulator::new(limits);
    for value in [b"z.rs".as_slice(), b"a.rs", b"m.rs", b"a.rs", b"z.rs"] {
        first.record_hint(path(value));
    }
    for value in [b"m.rs".as_slice(), b"z.rs", b"a.rs", b"m.rs", b"a.rs"] {
        second.record_hint(path(value));
    }

    assert_eq!(batch_bytes(&mut first), batch_bytes(&mut second));
    assert_eq!(
        batch_bytes(&mut WatcherHintAccumulator::new(limits)),
        Vec::<Vec<u8>>::new()
    );
    assert_eq!(first.counters().duplicate_hints(), 2);
    assert_eq!(second.counters().duplicate_hints(), 2);
}

#[test]
fn rename_like_pairs_keep_both_exact_paths_once() {
    let mut accumulator =
        WatcherHintAccumulator::new(WatcherHintLimits::try_new(4, 128).expect("valid limits"));
    for value in [b"new/name.rs".as_slice(), b"old/name.rs", b"new/name.rs"] {
        accumulator.record_hint(path(value));
    }

    assert_eq!(
        batch_bytes(&mut accumulator),
        [b"new/name.rs".to_vec(), b"old/name.rs".to_vec()]
    );
    assert_eq!(accumulator.counters().duplicate_hints(), 1);
}

#[test]
fn overflow_storms_are_bounded_and_order_independent() {
    let limits = WatcherHintLimits::try_new(3, 1_024).expect("valid limits");
    let mut forward = WatcherHintAccumulator::new(limits);
    let mut reverse = WatcherHintAccumulator::new(limits);
    let hints = (0_u16..100)
        .map(|index| format!("storm/{index:03}.rs").into_bytes())
        .collect::<Vec<_>>();
    for value in &hints {
        forward.record_hint(path(value));
    }
    for value in hints.iter().rev() {
        reverse.record_hint(path(value));
    }

    let forward_batch = forward.drain();
    let reverse_batch = reverse.drain();
    assert_eq!(forward_batch, reverse_batch);
    assert!(forward_batch.full_reconciliation_required());
    assert!(forward_batch.paths().is_empty());
    assert_eq!(forward.pending_path_count().get(), 0);
    assert_eq!(reverse.pending_path_bytes().get(), 0);
}

#[test]
fn unsupported_event_is_sticky_until_drain_and_clears_retained_paths() {
    let mut accumulator =
        WatcherHintAccumulator::new(WatcherHintLimits::try_new(4, 128).expect("valid limits"));
    accumulator.record_hint(path(b"secret-before.rs"));
    assert_eq!(
        accumulator.record_unsupported_event(),
        WatcherHintAdmission::UnsupportedEvent
    );
    assert_eq!(
        accumulator.record_hint(path(b"after.rs")),
        WatcherHintAdmission::FullReconciliationAlreadyRequired
    );

    let batch = accumulator.drain();
    assert!(batch.full_reconciliation_required());
    assert!(batch.causes().unsupported_event());
    assert!(batch.paths().is_empty());
    assert!(!accumulator.full_reconciliation_required());
    assert_eq!(
        accumulator.record_hint(path(b"fresh.rs")),
        WatcherHintAdmission::Retained
    );
}

#[test]
fn counter_saturation_is_categorical_and_discards_partial_hints() {
    let mut accumulator =
        WatcherHintAccumulator::new(WatcherHintLimits::try_new(4, 128).expect("valid limits"));
    accumulator.record_hint(path(b"before.rs"));
    accumulator.set_observed_events_for_test(u64::MAX);
    assert_eq!(
        accumulator.record_hint(path(b"after.rs")),
        WatcherHintAdmission::CounterOverflow
    );

    let batch = accumulator.drain();
    assert!(batch.full_reconciliation_required());
    assert!(batch.causes().counter_overflow());
    assert!(batch.paths().is_empty());
    assert_eq!(accumulator.counters().observed_events(), u64::MAX);
}

#[test]
fn every_remaining_hint_counter_saturation_is_categorical() {
    let limits = WatcherHintLimits::try_new(1, 128).expect("valid limits");

    let mut duplicate = WatcherHintAccumulator::new(limits);
    duplicate.record_hint(path(b"same.rs"));
    duplicate.set_duplicate_hints_for_test(u64::MAX);
    assert_eq!(
        duplicate.record_hint(path(b"same.rs")),
        WatcherHintAdmission::CounterOverflow
    );
    assert!(duplicate.drain().causes().counter_overflow());
    assert_eq!(duplicate.counters().duplicate_hints(), u64::MAX);

    let mut coalesced = WatcherHintAccumulator::new(limits);
    coalesced.record_unsupported_event();
    coalesced.set_coalesced_hints_for_test(u64::MAX);
    assert_eq!(
        coalesced.record_hint(path(b"ignored.rs")),
        WatcherHintAdmission::CounterOverflow
    );
    let coalesced_batch = coalesced.drain();
    assert!(coalesced_batch.causes().unsupported_event());
    assert!(coalesced_batch.causes().counter_overflow());
    assert_eq!(coalesced.counters().coalesced_hints(), u64::MAX);

    let mut overflow = WatcherHintAccumulator::new(limits);
    overflow.record_hint(path(b"first.rs"));
    overflow.set_overflow_events_for_test(u64::MAX);
    assert_eq!(
        overflow.record_hint(path(b"second.rs")),
        WatcherHintAdmission::CounterOverflow
    );
    let overflow_batch = overflow.drain();
    assert!(overflow_batch.causes().path_count_overflow());
    assert!(overflow_batch.causes().counter_overflow());
    assert_eq!(overflow.counters().overflow_events(), u64::MAX);

    let mut unsupported = WatcherHintAccumulator::new(limits);
    unsupported.set_unsupported_events_for_test(u64::MAX);
    assert_eq!(
        unsupported.record_unsupported_event(),
        WatcherHintAdmission::CounterOverflow
    );
    assert!(unsupported.drain().causes().counter_overflow());
    assert_eq!(unsupported.counters().unsupported_events(), u64::MAX);
}

#[test]
fn debug_and_errors_never_expose_repository_path_bytes() {
    let limits = WatcherHintLimits::try_new(4, 128).expect("valid limits");
    let mut first = WatcherHintAccumulator::new(limits);
    let mut second = WatcherHintAccumulator::new(limits);
    first.record_hint(path(b"private/customer-name.rs"));
    first.record_hint(path(b"private/other-name.rs"));
    second.record_hint(path(b"private/other-name.rs"));
    second.record_hint(path(b"private/customer-name.rs"));

    let first_debug = format!("{first:?}");
    let second_debug = format!("{second:?}");
    let first_batch_debug = format!("{:?}", first.drain());
    let second_batch_debug = format!("{:?}", second.drain());
    let error = WatcherHintLimits::try_new(0, 1).expect_err("zero must fail");

    assert_eq!(first_debug, second_debug);
    assert_eq!(first_batch_debug, second_batch_debug);
    assert!(!first_debug.contains("customer-name"));
    assert!(!first_debug.contains("other-name"));
    assert!(!first_batch_debug.contains("customer-name"));
    assert!(!first_batch_debug.contains("other-name"));
    assert!(!error.to_string().contains("customer-name"));
    assert!(!format!("{error:?}").contains("customer-name"));
}
