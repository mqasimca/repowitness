use super::super::{
    MAX_WATCHER_DEBOUNCE_MS, MAX_WATCHER_PERIODIC_MS, MAX_WATCHER_RETRIES,
    MAX_WATCHER_RETRY_DELAY_MS, WatcherCompletion, WatcherCompletionOutcome,
    WatcherMonotonicTimestamp, WatcherObservationOutcome, WatcherPollDecision, WatcherPollingState,
    WatcherReconciliationReason, WatcherScheduleLimitError, WatcherScheduleLimits,
    WatcherStateError,
};
use super::{complete_startup, schedule, timestamp};

fn exhaust_retry_budget(state: &mut WatcherPollingState) {
    state.begin_pending().expect("startup must begin");
    for (failed_at, retry_at) in [(0, 10), (10, 20)] {
        state
            .complete(timestamp(failed_at), WatcherCompletion::RetryableFailure)
            .expect("bounded retry must schedule");
        assert!(matches!(
            state.poll(timestamp(retry_at)),
            WatcherPollDecision::Pending(WatcherReconciliationReason::Retry(_))
        ));
        state.begin_pending().expect("scheduled retry must begin");
    }
    assert_eq!(
        state
            .complete(timestamp(20), WatcherCompletion::RetryableFailure)
            .expect("retry exhaustion is categorical"),
        WatcherCompletionOutcome::RetryExhausted
    );
}

#[test]
fn schedule_limits_reject_zero_and_over_ceiling_without_state() {
    let invalid = [
        (
            WatcherScheduleLimits::try_new(0, 1, 1, 1),
            WatcherScheduleLimitError::ZeroDebounce,
        ),
        (
            WatcherScheduleLimits::try_new(MAX_WATCHER_DEBOUNCE_MS + 1, 1, 1, 1),
            WatcherScheduleLimitError::DebounceTooLarge,
        ),
        (
            WatcherScheduleLimits::try_new(1, 0, 1, 1),
            WatcherScheduleLimitError::ZeroPeriodic,
        ),
        (
            WatcherScheduleLimits::try_new(1, MAX_WATCHER_PERIODIC_MS + 1, 1, 1),
            WatcherScheduleLimitError::PeriodicTooLarge,
        ),
        (
            WatcherScheduleLimits::try_new(1, 1, 0, 1),
            WatcherScheduleLimitError::ZeroRetryDelay,
        ),
        (
            WatcherScheduleLimits::try_new(1, 1, MAX_WATCHER_RETRY_DELAY_MS + 1, 1),
            WatcherScheduleLimitError::RetryDelayTooLarge,
        ),
        (
            WatcherScheduleLimits::try_new(1, 1, 1, 0),
            WatcherScheduleLimitError::ZeroRetries,
        ),
        (
            WatcherScheduleLimits::try_new(1, 1, 1, MAX_WATCHER_RETRIES + 1),
            WatcherScheduleLimitError::RetriesTooLarge,
        ),
    ];
    for (result, expected) in invalid {
        assert_eq!(result, Err(expected));
    }
}

#[test]
fn startup_is_exactly_one_pending_decision_until_begun() {
    let mut state =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    let startup = WatcherPollDecision::Pending(WatcherReconciliationReason::Startup);
    assert_eq!(state.poll(timestamp(0)), startup);
    assert_eq!(state.poll(timestamp(1)), startup);
    assert_eq!(
        state.pending_reason(),
        Some(WatcherReconciliationReason::Startup)
    );
    assert_eq!(
        state.begin_pending().expect("startup must begin"),
        WatcherReconciliationReason::Startup
    );
    assert!(state.reconciliation_active());
    assert_eq!(state.poll(timestamp(1)), WatcherPollDecision::Backpressured);
    assert_eq!(
        state.begin_pending(),
        Err(WatcherStateError::ReconciliationAlreadyActive)
    );
}

#[test]
fn dirty_hints_refresh_debounce_and_become_one_pending_decision() {
    let mut state =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    complete_startup(&mut state, 0);
    assert_eq!(
        state.observe_dirty(timestamp(10)),
        WatcherObservationOutcome::DirtyDebouncing
    );
    assert_eq!(
        state.observe_dirty(timestamp(12)),
        WatcherObservationOutcome::DirtyDebouncing
    );
    assert_eq!(
        state.poll(timestamp(16)),
        WatcherPollDecision::Debouncing {
            until: timestamp(17)
        }
    );
    let pending = WatcherPollDecision::Pending(WatcherReconciliationReason::DirtyAfterDebounce);
    assert_eq!(state.poll(timestamp(17)), pending);
    assert_eq!(state.poll(timestamp(18)), pending);
}

#[test]
fn quiet_state_reconciles_at_the_exact_periodic_boundary() {
    let mut state =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    complete_startup(&mut state, 10);
    assert_eq!(
        state.poll(timestamp(109)),
        WatcherPollDecision::WaitingPeriodic { at: timestamp(110) }
    );
    assert_eq!(
        state.poll(timestamp(110)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::Periodic)
    );
}

#[test]
fn retry_delay_is_inclusive_and_retry_budget_is_bounded() {
    let mut state =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    state.begin_pending().expect("startup must begin");
    let first_retry = state
        .complete(timestamp(0), WatcherCompletion::RetryableFailure)
        .expect("first failure must schedule retry");
    let WatcherCompletionOutcome::RetryScheduled { attempt, at } = first_retry else {
        panic!("first retry must be scheduled");
    };
    assert_eq!(attempt.get(), 1);
    assert_eq!(at, timestamp(10));

    let WatcherPollDecision::WaitingRetry { at, attempt } = state.poll(timestamp(9)) else {
        panic!("retry must wait before its boundary");
    };
    assert_eq!(attempt.get(), 1);
    assert_eq!(at, timestamp(10));
    let WatcherPollDecision::Pending(WatcherReconciliationReason::Retry(attempt)) =
        state.poll(timestamp(10))
    else {
        panic!("retry must become pending at its boundary");
    };
    assert_eq!(attempt.get(), 1);
    state.begin_pending().expect("retry one must begin");
    state
        .complete(timestamp(10), WatcherCompletion::RetryableFailure)
        .expect("second retry must schedule");
    let WatcherPollDecision::Pending(WatcherReconciliationReason::Retry(attempt)) =
        state.poll(timestamp(20))
    else {
        panic!("second retry must become pending");
    };
    assert_eq!(attempt.get(), 2);
    state.begin_pending().expect("retry two must begin");
    assert_eq!(
        state
            .complete(timestamp(20), WatcherCompletion::RetryableFailure)
            .expect("retry exhaustion is categorical"),
        WatcherCompletionOutcome::RetryExhausted
    );
    assert_eq!(
        state.poll(timestamp(99)),
        WatcherPollDecision::RetryExhausted
    );
    assert_eq!(
        state.poll(timestamp(100)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::Periodic)
    );
}

#[test]
fn active_work_backpressures_hints_and_runs_dirty_work_after_completion() {
    let mut state =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    state.begin_pending().expect("startup must begin");
    assert_eq!(
        state.observe_dirty(timestamp(2)),
        WatcherObservationOutcome::Backpressured
    );
    assert_eq!(state.poll(timestamp(7)), WatcherPollDecision::Backpressured);
    state
        .complete(timestamp(7), WatcherCompletion::Succeeded)
        .expect("active startup must complete");
    assert_eq!(
        state.poll(timestamp(7)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::DirtyAfterDebounce)
    );
    assert_eq!(state.counters().backpressure_observations(), 1);
}

#[test]
fn pending_work_coalesces_observations_without_a_second_decision() {
    let mut state =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    assert_eq!(
        state.observe_dirty(timestamp(1)),
        WatcherObservationOutcome::Coalesced
    );
    assert_eq!(
        state.observe_full_reconciliation_required(timestamp(2)),
        WatcherObservationOutcome::Coalesced
    );
    assert_eq!(
        state.poll(timestamp(2)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::Startup)
    );
    assert_eq!(state.counters().coalesced_observations(), 2);
}

#[test]
fn observations_before_begin_are_covered_by_the_pending_complete_reconciliation() {
    let mut state =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    assert_eq!(
        state.observe_dirty(timestamp(1)),
        WatcherObservationOutcome::Coalesced
    );
    assert_eq!(
        state.observe_full_reconciliation_required(timestamp(2)),
        WatcherObservationOutcome::Coalesced
    );

    assert_eq!(
        state.begin_pending().expect("startup must begin"),
        WatcherReconciliationReason::Startup
    );
    state
        .complete(timestamp(2), WatcherCompletion::Succeeded)
        .expect("startup must cover pre-begin observations");
    assert_eq!(
        state.poll(timestamp(2)),
        WatcherPollDecision::WaitingPeriodic { at: timestamp(102) }
    );
}

#[test]
fn full_reconciliation_is_immediate_when_idle() {
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
}

#[test]
fn cancellation_clears_pending_retry_and_stops_new_admission() {
    let mut state =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    state.begin_pending().expect("startup must begin");
    state
        .complete(timestamp(0), WatcherCompletion::RetryableFailure)
        .expect("retry must schedule");
    state.cancel();

    assert_eq!(state.poll(timestamp(10)), WatcherPollDecision::Cancelled);
    assert_eq!(
        state.observe_dirty(timestamp(10)),
        WatcherObservationOutcome::Cancelled
    );
    assert_eq!(state.begin_pending(), Err(WatcherStateError::Cancelled));
    assert!(state.is_cancelled());
}

#[test]
fn cancellation_is_idempotent_and_preserves_counters() {
    let mut state =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    let counters = state.counters();
    state.cancel();
    state.cancel();

    assert_eq!(state.poll(timestamp(0)), WatcherPollDecision::Cancelled);
    assert_eq!(
        state.observe_full_reconciliation_required(timestamp(0)),
        WatcherObservationOutcome::Cancelled
    );
    assert_eq!(state.counters(), counters);
    assert_eq!(state.pending_reason(), None);
    assert!(!state.reconciliation_active());
}

#[test]
fn active_cancellation_completion_is_explicit() {
    let mut state =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    state.begin_pending().expect("startup must begin");
    assert_eq!(
        state
            .complete(timestamp(1), WatcherCompletion::Cancelled)
            .expect("cancelled completion must transition"),
        WatcherCompletionOutcome::Cancelled
    );
    assert_eq!(state.poll(timestamp(2)), WatcherPollDecision::Cancelled);
    assert!(!state.reconciliation_active());
}

#[test]
fn clock_regression_forces_one_complete_reconciliation() {
    let mut state =
        WatcherPollingState::new(timestamp(100), schedule()).expect("state must construct");
    complete_startup(&mut state, 100);
    assert_eq!(
        state.poll(timestamp(99)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::ClockRegression)
    );
    assert_eq!(
        state.poll(timestamp(100)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::ClockRegression)
    );
    assert_eq!(state.counters().clock_regressions(), 1);
}

#[test]
fn completion_timestamp_regression_keeps_work_active_until_explicit_recovery() {
    let mut state =
        WatcherPollingState::new(timestamp(100), schedule()).expect("state must construct");
    state.begin_pending().expect("startup must begin");
    assert_eq!(
        state.complete(timestamp(99), WatcherCompletion::Succeeded),
        Err(WatcherStateError::ClockRegression)
    );
    assert!(state.reconciliation_active());
    assert_eq!(
        state.poll(timestamp(100)),
        WatcherPollDecision::Backpressured
    );

    assert_eq!(
        state
            .complete(timestamp(100), WatcherCompletion::Succeeded)
            .expect("caller can recover with a valid timestamp"),
        WatcherCompletionOutcome::Completed
    );
    assert_eq!(
        state.poll(timestamp(100)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::FullReconciliationRequired)
    );
}

#[test]
fn active_retry_coalesces_observations_into_the_next_complete_retry() {
    let mut state =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    state.begin_pending().expect("startup must begin");
    state
        .complete(timestamp(0), WatcherCompletion::RetryableFailure)
        .expect("first retry must schedule");
    assert!(matches!(
        state.poll(timestamp(10)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::Retry(_))
    ));
    state.begin_pending().expect("first retry must begin");

    assert_eq!(
        state.observe_dirty(timestamp(11)),
        WatcherObservationOutcome::Backpressured
    );
    assert_eq!(
        state.observe_full_reconciliation_required(timestamp(12)),
        WatcherObservationOutcome::Backpressured
    );
    assert!(matches!(
        state
            .complete(timestamp(12), WatcherCompletion::RetryableFailure)
            .expect("second retry must schedule"),
        WatcherCompletionOutcome::RetryScheduled { .. }
    ));
    assert!(matches!(
        state.poll(timestamp(22)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::Retry(_))
    ));
    state.begin_pending().expect("second retry must begin");
    state
        .complete(timestamp(22), WatcherCompletion::Succeeded)
        .expect("complete retry covers active observations");
    assert_eq!(
        state.poll(timestamp(22)),
        WatcherPollDecision::WaitingPeriodic { at: timestamp(122) }
    );
    assert_eq!(state.counters().backpressure_observations(), 2);
}

#[test]
fn new_dirty_input_after_retry_exhaustion_immediately_releases_full_work() {
    let mut state =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    exhaust_retry_budget(&mut state);
    assert_eq!(
        state.poll(timestamp(21)),
        WatcherPollDecision::RetryExhausted
    );

    assert_eq!(
        state.observe_dirty(timestamp(21)),
        WatcherObservationOutcome::FullReconciliationPending
    );
    assert_eq!(
        state.poll(timestamp(21)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::FullReconciliationRequired)
    );
}

#[test]
fn exact_u64_timestamp_boundaries_are_admitted() {
    let limits = schedule();
    let start = u64::MAX - limits.periodic().get();

    let mut periodic =
        WatcherPollingState::new(timestamp(start), limits).expect("exact periodic bound is valid");
    periodic.begin_pending().expect("startup must begin");
    periodic
        .complete(timestamp(start), WatcherCompletion::Succeeded)
        .expect("exact periodic deadline is representable");
    assert_eq!(
        periodic.poll(timestamp(u64::MAX - 1)),
        WatcherPollDecision::WaitingPeriodic {
            at: timestamp(u64::MAX)
        }
    );
    assert_eq!(
        periodic.poll(timestamp(u64::MAX)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::Periodic)
    );

    let mut dirty =
        WatcherPollingState::new(timestamp(start), limits).expect("exact debounce bound is valid");
    complete_startup(&mut dirty, start);
    assert_eq!(
        dirty.observe_dirty(timestamp(u64::MAX - limits.debounce().get())),
        WatcherObservationOutcome::DirtyDebouncing
    );
    assert_eq!(
        dirty.poll(timestamp(u64::MAX - 1)),
        WatcherPollDecision::Debouncing {
            until: timestamp(u64::MAX)
        }
    );

    let mut retry =
        WatcherPollingState::new(timestamp(start), limits).expect("exact retry bound is valid");
    retry.begin_pending().expect("startup must begin");
    assert!(matches!(
        retry
            .complete(
                timestamp(u64::MAX - limits.retry_delay().get()),
                WatcherCompletion::RetryableFailure,
            )
            .expect("exact retry deadline is representable"),
        WatcherCompletionOutcome::RetryScheduled { at, .. } if at == timestamp(u64::MAX)
    ));
}

#[test]
fn timestamp_overflow_fails_construction_or_forces_full_work() {
    let limits = schedule();
    assert_eq!(
        WatcherPollingState::new(timestamp(u64::MAX - limits.periodic().get() + 1), limits,)
            .expect_err("one past the initial periodic boundary cannot construct"),
        WatcherStateError::TimestampOverflow
    );

    let mut state =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    complete_startup(&mut state, 0);
    assert_eq!(
        state.observe_dirty(WatcherMonotonicTimestamp::from_millis(u64::MAX - 1)),
        WatcherObservationOutcome::TimestampOverflow
    );
    assert_eq!(
        state.pending_reason(),
        Some(WatcherReconciliationReason::TimestampOverflow)
    );

    let mut completed =
        WatcherPollingState::new(timestamp(0), limits).expect("state must construct");
    completed.begin_pending().expect("startup must begin");
    assert_eq!(
        completed.complete(timestamp(u64::MAX), WatcherCompletion::Succeeded),
        Err(WatcherStateError::TimestampOverflow)
    );
    assert_eq!(
        completed.pending_reason(),
        Some(WatcherReconciliationReason::TimestampOverflow)
    );

    let mut retry = WatcherPollingState::new(timestamp(0), limits).expect("state must construct");
    retry.begin_pending().expect("startup must begin");
    assert_eq!(
        retry.complete(
            timestamp(u64::MAX - limits.retry_delay().get() + 1),
            WatcherCompletion::RetryableFailure,
        ),
        Err(WatcherStateError::TimestampOverflow)
    );
    assert_eq!(
        retry.pending_reason(),
        Some(WatcherReconciliationReason::TimestampOverflow)
    );
}

#[test]
fn state_counter_saturation_never_partially_begins_work() {
    let mut state =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    state.set_reconciliations_started_for_test(u64::MAX);
    assert_eq!(
        state.begin_pending(),
        Err(WatcherStateError::CounterOverflow)
    );
    assert!(!state.reconciliation_active());
    assert_eq!(
        state.pending_reason(),
        Some(WatcherReconciliationReason::Startup)
    );
}

#[test]
fn every_remaining_state_counter_saturation_is_categorical() {
    let mut completed =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    completed.begin_pending().expect("startup must begin");
    completed.set_reconciliations_completed_for_test(u64::MAX);
    assert_eq!(
        completed.complete(timestamp(0), WatcherCompletion::Succeeded),
        Err(WatcherStateError::CounterOverflow)
    );
    assert_eq!(
        completed.pending_reason(),
        Some(WatcherReconciliationReason::CounterOverflow)
    );
    assert_eq!(completed.counters().reconciliations_completed(), u64::MAX);

    let mut failed =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    failed.begin_pending().expect("startup must begin");
    failed.set_retryable_failures_for_test(u64::MAX);
    assert_eq!(
        failed.complete(timestamp(0), WatcherCompletion::RetryableFailure),
        Err(WatcherStateError::CounterOverflow)
    );
    assert_eq!(
        failed.pending_reason(),
        Some(WatcherReconciliationReason::CounterOverflow)
    );
    assert_eq!(failed.counters().retryable_failures(), u64::MAX);

    let mut coalesced =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    coalesced.set_coalesced_observations_for_test(u64::MAX);
    assert_eq!(
        coalesced.observe_dirty(timestamp(1)),
        WatcherObservationOutcome::CounterOverflow
    );
    assert_eq!(coalesced.counters().coalesced_observations(), u64::MAX);

    let mut backpressured =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    backpressured.begin_pending().expect("startup must begin");
    backpressured.set_backpressure_observations_for_test(u64::MAX);
    assert_eq!(
        backpressured.observe_dirty(timestamp(1)),
        WatcherObservationOutcome::CounterOverflow
    );
    backpressured
        .complete(timestamp(1), WatcherCompletion::Succeeded)
        .expect("active work can complete after diagnostic overflow");
    assert_eq!(
        backpressured.poll(timestamp(1)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::FullReconciliationRequired)
    );
    assert_eq!(
        backpressured.counters().backpressure_observations(),
        u64::MAX
    );

    let mut regressed =
        WatcherPollingState::new(timestamp(100), schedule()).expect("state must construct");
    complete_startup(&mut regressed, 100);
    regressed.set_clock_regressions_for_test(u64::MAX);
    assert_eq!(
        regressed.poll(timestamp(99)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::CounterOverflow)
    );
    assert_eq!(regressed.counters().clock_regressions(), u64::MAX);
}

#[test]
fn debug_and_errors_are_path_free_and_redacted() {
    let mut first =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    let mut second =
        WatcherPollingState::new(timestamp(0), schedule()).expect("state must construct");
    first.observe_dirty(timestamp(1));
    second.observe_dirty(timestamp(1));
    let first_debug = format!("{first:?}");
    let second_debug = format!("{second:?}");
    let error = WatcherStateError::ClockRegression;
    let limit_error =
        WatcherScheduleLimits::try_new(0, 1, 1, 1).expect_err("invalid schedule must fail");

    assert_eq!(first_debug, second_debug);
    assert!(!first_debug.contains("repository"));
    assert!(!error.to_string().contains("repository"));
    assert!(!format!("{error:?}").contains("repository"));
    assert!(!limit_error.to_string().contains("repository"));
}
