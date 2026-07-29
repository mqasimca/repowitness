mod hints;
mod schedule;
mod supervisor;

use repowitness_domain::{RepositoryPath, RepositoryPathLimits};

use super::{
    WatcherCompletion, WatcherHintAccumulator, WatcherHintAdmission, WatcherHintLimits,
    WatcherMonotonicTimestamp, WatcherObservationOutcome, WatcherPollDecision, WatcherPollingState,
    WatcherReconciliationReason, WatcherScheduleLimits,
};

fn path(bytes: &[u8]) -> RepositoryPath {
    RepositoryPath::try_from_bytes(bytes, RepositoryPathLimits::new(4_096, 64))
        .expect("fixture repository path must be valid")
}

fn timestamp(value: u64) -> WatcherMonotonicTimestamp {
    WatcherMonotonicTimestamp::from_millis(value)
}

fn schedule() -> WatcherScheduleLimits {
    WatcherScheduleLimits::try_new(5, 100, 10, 2).expect("fixture schedule must be valid")
}

fn complete_startup(state: &mut WatcherPollingState, at: u64) {
    assert_eq!(
        state.begin_pending().expect("startup must be pending"),
        WatcherReconciliationReason::Startup
    );
    state
        .complete(timestamp(at), WatcherCompletion::Succeeded)
        .expect("startup completion must succeed");
}

#[test]
fn overflowing_hint_batch_drives_one_immediate_full_decision() {
    let mut accumulator =
        WatcherHintAccumulator::new(WatcherHintLimits::try_new(1, 64).expect("valid limits"));
    assert_eq!(
        accumulator.record_hint(path(b"old.rs")),
        WatcherHintAdmission::Retained
    );
    assert_eq!(
        accumulator.record_hint(path(b"new.rs")),
        WatcherHintAdmission::PathCountOverflow
    );
    let batch = accumulator.drain();
    assert!(batch.full_reconciliation_required());
    assert!(batch.paths().is_empty());

    let mut state =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    complete_startup(&mut state, 0);
    assert_eq!(
        state.observe_full_reconciliation_required(timestamp(1)),
        WatcherObservationOutcome::FullReconciliationPending
    );
    assert_eq!(
        state.poll(timestamp(1)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::FullReconciliationRequired)
    );
    assert_eq!(
        state.poll(timestamp(2)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::FullReconciliationRequired)
    );
}
