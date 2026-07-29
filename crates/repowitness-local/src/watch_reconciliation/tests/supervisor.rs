use repowitness_application::{
    ConfigurationLayer, ConfigurationLayerKind, ConfigurationPolicyOverrides,
    ConfigurationPreferenceOverrides, resolve_configuration,
};

use super::*;
use crate::{PollingReconciliationRequest, PollingReconciliationSupervisor};

fn supervisor(
    schedule: WatcherScheduleLimits,
    hints: WatcherHintLimits,
) -> PollingReconciliationSupervisor {
    PollingReconciliationSupervisor::start(
        PollingReconciliationRequest::new(timestamp(0), None)
            .with_schedule_limits(schedule)
            .with_hint_limits(hints),
    )
    .expect("fixture supervisor should start")
}

fn complete_supervisor_startup(supervisor: &mut PollingReconciliationSupervisor) {
    assert_eq!(
        supervisor
            .begin_pending()
            .expect("startup work should be pending")
            .reason(),
        WatcherReconciliationReason::Startup
    );
    assert_eq!(
        supervisor
            .complete(timestamp(0), WatcherCompletion::Succeeded)
            .expect("startup should complete"),
        crate::WatcherCompletionOutcome::Completed
    );
}

fn configured_poll_interval(value: u64) -> repowitness_application::ResolvedConfiguration {
    let preferences =
        ConfigurationPreferenceOverrides::try_new(None, None, None, None, Some(value), None)
            .expect("poll interval should validate");
    let layer = ConfigurationLayer::try_new(
        ConfigurationLayerKind::Repository,
        None,
        preferences,
        ConfigurationPolicyOverrides::default(),
    )
    .expect("repository layer should validate");
    resolve_configuration(&[layer]).expect("configuration should resolve")
}

#[test]
fn resolved_poll_interval_cannot_weaken_mandatory_periodic_or_retry_bounds() {
    let default_configuration = resolve_configuration(&[]).expect("defaults should resolve");
    let default_request =
        PollingReconciliationRequest::new(timestamp(0), Some(&default_configuration));
    assert_eq!(default_request.effective_poll_interval().get(), 2_000);
    assert_eq!(
        default_request.configuration_digest(),
        Some(default_configuration.digest())
    );

    let slow_configuration = configured_poll_interval(86_400_000);
    let supervisor = PollingReconciliationSupervisor::start(PollingReconciliationRequest::new(
        timestamp(0),
        Some(&slow_configuration),
    ))
    .expect("supervisor should start");

    assert_eq!(supervisor.effective_poll_interval().get(), 30_000);
    assert_eq!(
        supervisor.schedule_limits().retry_delay().get(),
        crate::DEFAULT_WATCHER_RETRY_DELAY_MS
    );
    assert_eq!(
        supervisor.schedule_limits().max_retries(),
        crate::DEFAULT_WATCHER_MAX_RETRIES
    );
}

#[test]
fn different_hint_order_and_duplicates_admit_identical_path_free_work() {
    let schedule = WatcherScheduleLimits::try_new(5, 100, 10, 2).expect("valid schedule");
    let hints = WatcherHintLimits::try_new(8, 512).expect("valid hint limits");
    let mut first = supervisor(schedule, hints);
    let mut second = supervisor(schedule, hints);
    complete_supervisor_startup(&mut first);
    complete_supervisor_startup(&mut second);

    first.observe_hint(path(b"src/a.rs"), timestamp(1));
    first.observe_hint(path(b"src/a.rs"), timestamp(2));
    first.observe_hint(path(b"src/b.rs"), timestamp(3));
    second.observe_hint(path(b"src/b.rs"), timestamp(1));
    second.observe_hint(path(b"src/a.rs"), timestamp(3));
    assert_eq!(
        first.poll(timestamp(8)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::DirtyAfterDebounce)
    );
    assert_eq!(
        second.poll(timestamp(8)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::DirtyAfterDebounce)
    );

    let first_work = first.begin_pending().expect("first work should admit");
    let second_work = second.begin_pending().expect("second work should admit");
    assert_eq!(
        first_work.reason(),
        WatcherReconciliationReason::DirtyAfterDebounce
    );
    assert_eq!(first_work, second_work);
    assert_eq!(first.pending_hint_paths().get(), 0);
    assert_eq!(second.pending_hint_paths().get(), 0);
}

#[test]
fn overflow_and_unsupported_events_force_complete_work_without_path_payloads() {
    let schedule = WatcherScheduleLimits::try_new(5, 100, 10, 2).expect("valid schedule");
    let hints = WatcherHintLimits::try_new(1, 64).expect("valid hint limits");
    let mut overflow = supervisor(schedule, hints);
    complete_supervisor_startup(&mut overflow);
    overflow.observe_hint(path(b"old.rs"), timestamp(1));
    let observed = overflow.observe_hint(path(b"new.rs"), timestamp(2));
    assert_eq!(
        observed.admission(),
        WatcherHintAdmission::PathCountOverflow
    );
    assert_eq!(
        observed.scheduling(),
        WatcherObservationOutcome::FullReconciliationPending
    );
    assert_eq!(
        overflow
            .begin_pending()
            .expect("overflow work should admit")
            .reason(),
        WatcherReconciliationReason::FullReconciliationRequired
    );
    assert_eq!(overflow.pending_hint_paths().get(), 0);

    let mut unsupported = supervisor(schedule, hints);
    complete_supervisor_startup(&mut unsupported);
    let observed = unsupported.observe_unsupported_event(timestamp(1));
    assert_eq!(observed.admission(), WatcherHintAdmission::UnsupportedEvent);
    assert_eq!(
        unsupported
            .begin_pending()
            .expect("unsupported event should admit")
            .reason(),
        WatcherReconciliationReason::FullReconciliationRequired
    );
}

#[test]
fn hints_during_active_work_are_backpressured_then_reconciled() {
    let schedule = WatcherScheduleLimits::try_new(5, 100, 10, 2).expect("valid schedule");
    let hints = WatcherHintLimits::try_new(8, 512).expect("valid hint limits");
    let mut supervisor = supervisor(schedule, hints);
    let startup = supervisor
        .begin_pending()
        .expect("startup should become active");
    assert_eq!(startup.reason(), WatcherReconciliationReason::Startup);

    let observed = supervisor.observe_hint(path(b"src/late.rs"), timestamp(1));
    assert_eq!(
        observed.scheduling(),
        WatcherObservationOutcome::Backpressured
    );
    supervisor
        .complete(timestamp(2), WatcherCompletion::Succeeded)
        .expect("startup should complete");
    assert_eq!(
        supervisor.poll(timestamp(6)),
        WatcherPollDecision::Pending(WatcherReconciliationReason::DirtyAfterDebounce)
    );
}

#[test]
fn cancellation_stops_admission_and_redacted_debug_omits_hint_paths() {
    let mut supervisor = supervisor(
        WatcherScheduleLimits::try_new(5, 100, 10, 2).expect("valid schedule"),
        WatcherHintLimits::try_new(8, 512).expect("valid hint limits"),
    );
    complete_supervisor_startup(&mut supervisor);
    supervisor.observe_hint(path(b"private/customer-name.rs"), timestamp(1));
    let debug = format!("{supervisor:?}");
    assert!(!debug.contains("customer-name"));
    assert!(!debug.contains("private"));

    supervisor.cancel();
    assert!(supervisor.is_cancelled());
    assert_eq!(supervisor.pending_hint_paths().get(), 0);
    assert_eq!(
        supervisor.poll(timestamp(2)),
        WatcherPollDecision::Cancelled
    );
    assert_eq!(
        supervisor.begin_pending(),
        Err(crate::WatcherStateError::Cancelled)
    );
}
