use std::collections::VecDeque;
use std::path::Path;

use repowitness_application::{
    ConfigurationLayer, ConfigurationLayerKind, ConfigurationPolicyOverrides,
    ConfigurationPreferenceOverrides, resolve_configuration,
};
use repowitness_domain::{RepositoryPath, RepositoryPathLimits};

use super::*;

struct ScriptedEnvironment {
    now: u64,
    now_calls: usize,
    fail_now_call: Option<usize>,
    cancel_on_wait: bool,
    cancel_during_attempt: bool,
    waits: Vec<u64>,
    reasons: Vec<WatcherReconciliationReason>,
    remaining: Vec<Option<Duration>>,
    attempts: VecDeque<PollingAttempt<u64, &'static str>>,
    observations: VecDeque<RunnerObservation>,
    during_attempt: VecDeque<Vec<RunnerObservation>>,
    finish_at: VecDeque<u64>,
}

impl ScriptedEnvironment {
    fn new(attempts: impl IntoIterator<Item = PollingAttempt<u64, &'static str>>) -> Self {
        Self {
            now: 0,
            now_calls: 0,
            fail_now_call: None,
            cancel_on_wait: false,
            cancel_during_attempt: false,
            waits: Vec::new(),
            reasons: Vec::new(),
            remaining: Vec::new(),
            attempts: attempts.into_iter().collect(),
            observations: VecDeque::new(),
            during_attempt: VecDeque::new(),
            finish_at: VecDeque::new(),
        }
    }
}

impl PollingRunnerEnvironment for ScriptedEnvironment {
    type Success = u64;
    type Failure = &'static str;
    type ControlError = &'static str;

    fn now(&mut self) -> Result<WatcherMonotonicTimestamp, Self::ControlError> {
        self.now_calls += 1;
        if self.fail_now_call == Some(self.now_calls) {
            Err("clock failed")
        } else {
            Ok(timestamp(self.now))
        }
    }

    fn wait_until(
        &mut self,
        at: WatcherMonotonicTimestamp,
        cancelled: &AtomicBool,
    ) -> Result<(), Self::ControlError> {
        self.waits.push(at.as_millis());
        if self.cancel_on_wait {
            cancelled.store(true, Ordering::Release);
        } else {
            self.now = at.as_millis();
        }
        Ok(())
    }

    fn next_observation(&mut self) -> Option<RunnerObservation> {
        self.observations.pop_front()
    }

    fn reconcile(
        &mut self,
        reason: WatcherReconciliationReason,
        cancelled: Arc<AtomicBool>,
        remaining: Option<Duration>,
    ) -> PollingAttempt<Self::Success, Self::Failure> {
        self.reasons.push(reason);
        self.remaining.push(remaining);
        if let Some(observations) = self.during_attempt.pop_front() {
            self.observations.extend(observations);
        }
        if let Some(finished) = self.finish_at.pop_front() {
            self.now = finished;
        }
        if self.cancel_during_attempt {
            cancelled.store(true, Ordering::Release);
        }
        self.attempts
            .pop_front()
            .unwrap_or(PollingAttempt::Fatal("missing scripted attempt"))
    }
}

fn timestamp(value: u64) -> WatcherMonotonicTimestamp {
    WatcherMonotonicTimestamp::from_millis(value)
}

fn schedule(debounce: u64, periodic: u64, retry_delay: u64, retries: u16) -> WatcherScheduleLimits {
    WatcherScheduleLimits::try_new(debounce, periodic, retry_delay, retries)
        .expect("scripted schedule should validate")
}

fn supervisor(
    limits: WatcherScheduleLimits,
    configuration: Option<&repowitness_application::ResolvedConfiguration>,
) -> PollingReconciliationSupervisor {
    PollingReconciliationSupervisor::start(
        PollingReconciliationRequest::new(timestamp(0), configuration).with_schedule_limits(limits),
    )
    .expect("scripted supervisor should start")
}

fn path(value: &[u8]) -> RepositoryPath {
    RepositoryPath::try_from_bytes(value, RepositoryPathLimits::new(4_096, 64))
        .expect("scripted path should validate")
}

#[test]
fn startup_and_periodic_reconciliations_run_until_the_overall_deadline() {
    let mut environment = ScriptedEnvironment::new([
        PollingAttempt::Succeeded(1),
        PollingAttempt::Succeeded(2),
        PollingAttempt::Succeeded(3),
    ]);
    let report = run_with_environment(
        supervisor(schedule(5, 100, 10, 2), None),
        Arc::new(AtomicBool::new(false)),
        Some(timestamp(201)),
        &mut environment,
    )
    .unwrap_or_else(|_| panic!("scripted runner should complete at its deadline"));

    assert_eq!(report.exit(), PollingRunnerExit::DeadlineExceeded);
    assert_eq!(report.last_success(), Some(&3));
    assert_eq!(
        environment.reasons,
        [
            WatcherReconciliationReason::Startup,
            WatcherReconciliationReason::Periodic,
            WatcherReconciliationReason::Periodic,
        ]
    );
    assert_eq!(environment.waits, [100, 200, 201]);
    assert_eq!(
        environment.remaining,
        [
            Some(Duration::from_millis(201)),
            Some(Duration::from_millis(101)),
            Some(Duration::from_millis(1)),
        ]
    );
    assert_eq!(report.state_counters().reconciliations_completed(), 3);
}

#[test]
fn source_poll_interval_triggers_complete_reconciliation_before_periodic() {
    let preferences =
        ConfigurationPreferenceOverrides::try_new(None, None, None, None, Some(250), None)
            .expect("polling preference should validate");
    let layer = ConfigurationLayer::try_new(
        ConfigurationLayerKind::Repository,
        None,
        preferences,
        ConfigurationPolicyOverrides::default(),
    )
    .expect("configuration layer should validate");
    let configuration = resolve_configuration(&[layer]).expect("configuration should resolve");
    let mut environment =
        ScriptedEnvironment::new([PollingAttempt::Succeeded(1), PollingAttempt::Succeeded(2)]);

    let report = run_with_environment(
        supervisor(schedule(5, 1_000, 10, 2), Some(&configuration)),
        Arc::new(AtomicBool::new(false)),
        Some(timestamp(251)),
        &mut environment,
    )
    .unwrap_or_else(|_| panic!("polling runner should reach its deadline"));

    assert_eq!(report.exit(), PollingRunnerExit::DeadlineExceeded);
    assert_eq!(
        environment.reasons,
        [
            WatcherReconciliationReason::Startup,
            WatcherReconciliationReason::FullReconciliationRequired,
        ]
    );
    assert_eq!(environment.waits, [250, 251]);
}

#[test]
fn retryable_failures_use_bounded_retry_schedule_then_succeed() {
    let mut environment = ScriptedEnvironment::new([
        PollingAttempt::Retryable("first"),
        PollingAttempt::Retryable("second"),
        PollingAttempt::Succeeded(3),
    ]);

    let report = run_with_environment(
        supervisor(schedule(5, 100, 10, 2), None),
        Arc::new(AtomicBool::new(false)),
        Some(timestamp(21)),
        &mut environment,
    )
    .unwrap_or_else(|_| panic!("retry script should reach its deadline"));

    assert_eq!(environment.reasons.len(), 3);
    assert_eq!(environment.reasons[0], WatcherReconciliationReason::Startup);
    assert!(matches!(
        environment.reasons[1],
        WatcherReconciliationReason::Retry(attempt) if attempt.get() == 1
    ));
    assert!(matches!(
        environment.reasons[2],
        WatcherReconciliationReason::Retry(attempt) if attempt.get() == 2
    ));
    assert_eq!(environment.waits, [10, 20, 21]);
    assert_eq!(report.state_counters().retryable_failures(), 2);
    assert_eq!(report.state_counters().reconciliations_completed(), 1);
    assert_eq!(report.last_success(), Some(&3));
}

#[test]
fn observations_coalesce_before_work_and_backpressure_during_work() {
    let mut environment =
        ScriptedEnvironment::new([PollingAttempt::Succeeded(1), PollingAttempt::Succeeded(2)]);
    environment.observations.extend([
        RunnerObservation::Path(path(b"a.rs")),
        RunnerObservation::Path(path(b"b.rs")),
    ]);
    environment
        .during_attempt
        .push_back(vec![RunnerObservation::Path(path(b"late.rs"))]);

    let report = run_with_environment(
        supervisor(schedule(5, 100, 10, 2), None),
        Arc::new(AtomicBool::new(false)),
        Some(timestamp(6)),
        &mut environment,
    )
    .unwrap_or_else(|_| panic!("coalescing script should reach its deadline"));

    assert_eq!(
        environment.reasons,
        [
            WatcherReconciliationReason::Startup,
            WatcherReconciliationReason::DirtyAfterDebounce,
        ]
    );
    assert_eq!(report.state_counters().coalesced_observations(), 2);
    assert_eq!(report.state_counters().backpressure_observations(), 1);
    assert_eq!(report.hint_counters().observed_events(), 3);
}

#[test]
fn cancellation_from_an_attempt_or_wait_stops_new_admission() {
    let mut attempt_cancelled = ScriptedEnvironment::new([PollingAttempt::Cancelled]);
    let report = run_with_environment(
        supervisor(schedule(5, 100, 10, 2), None),
        Arc::new(AtomicBool::new(false)),
        None,
        &mut attempt_cancelled,
    )
    .unwrap_or_else(|_| panic!("cancelled attempt should exit cleanly"));
    assert_eq!(report.exit(), PollingRunnerExit::Cancelled);
    assert_eq!(report.state_counters().reconciliations_started(), 1);
    assert_eq!(report.state_counters().reconciliations_completed(), 0);

    let cancelled = Arc::new(AtomicBool::new(false));
    let mut wait_cancelled = ScriptedEnvironment::new([PollingAttempt::Succeeded(1)]);
    wait_cancelled.cancel_on_wait = true;
    let report = run_with_environment(
        supervisor(schedule(5, 100, 10, 2), None),
        Arc::clone(&cancelled),
        None,
        &mut wait_cancelled,
    )
    .unwrap_or_else(|_| panic!("wait cancellation should exit cleanly"));
    assert!(cancelled.load(Ordering::Acquire));
    assert_eq!(report.exit(), PollingRunnerExit::Cancelled);
    assert_eq!(
        wait_cancelled.reasons,
        [WatcherReconciliationReason::Startup]
    );
}

#[test]
fn cancellation_after_a_successful_attempt_preserves_the_completion_receipt() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut environment = ScriptedEnvironment::new([PollingAttempt::Succeeded(7)]);
    environment.cancel_during_attempt = true;

    let report = run_with_environment(
        supervisor(schedule(5, 100, 10, 2), None),
        Arc::clone(&cancelled),
        None,
        &mut environment,
    )
    .unwrap_or_else(|_| panic!("successful completion before cancellation should be reported"));

    assert!(cancelled.load(Ordering::Acquire));
    assert_eq!(report.exit(), PollingRunnerExit::Cancelled);
    assert_eq!(report.state_counters().reconciliations_started(), 1);
    assert_eq!(report.state_counters().reconciliations_completed(), 1);
    assert_eq!(report.last_success(), Some(&7));
}

#[test]
fn fatal_reconciliation_and_clock_errors_are_explicit() {
    let mut fatal = ScriptedEnvironment::new([PollingAttempt::Fatal("fatal")]);
    let result = run_with_environment(
        supervisor(schedule(5, 100, 10, 2), None),
        Arc::new(AtomicBool::new(false)),
        None,
        &mut fatal,
    );
    assert!(matches!(result, Err(PollingRunError::Fatal("fatal"))));

    let mut failed_clock = ScriptedEnvironment::new([PollingAttempt::Succeeded(1)]);
    failed_clock.fail_now_call = Some(2);
    let result = run_with_environment(
        supervisor(schedule(5, 100, 10, 2), None),
        Arc::new(AtomicBool::new(false)),
        None,
        &mut failed_clock,
    );
    assert!(matches!(
        result,
        Err(PollingRunError::Control("clock failed"))
    ));

    let mut regressed = ScriptedEnvironment::new([PollingAttempt::Succeeded(1)]);
    regressed.now = 10;
    regressed.finish_at.push_back(5);
    let result = run_with_environment(
        supervisor(schedule(5, 100, 10, 2), None),
        Arc::new(AtomicBool::new(false)),
        None,
        &mut regressed,
    );
    assert!(matches!(
        result,
        Err(PollingRunError::State(WatcherStateError::ClockRegression))
    ));
}

#[test]
fn unsupported_observation_is_immediate_and_path_free() {
    let mut environment =
        ScriptedEnvironment::new([PollingAttempt::Succeeded(1), PollingAttempt::Succeeded(2)]);
    environment
        .during_attempt
        .push_back(vec![RunnerObservation::Unsupported]);

    let report = run_with_environment(
        supervisor(schedule(5, 100, 10, 2), None),
        Arc::new(AtomicBool::new(false)),
        Some(timestamp(1)),
        &mut environment,
    )
    .unwrap_or_else(|_| panic!("unsupported observation should coalesce safely"));

    assert_eq!(report.exit(), PollingRunnerExit::DeadlineExceeded);
    assert_eq!(report.hint_counters().unsupported_events(), 1);
    assert_eq!(
        environment.reasons,
        [
            WatcherReconciliationReason::Startup,
            WatcherReconciliationReason::FullReconciliationRequired,
        ]
    );
}

#[test]
fn concrete_runner_honors_preexisting_cancellation_without_filesystem_access() {
    let index = LocalIndexRequest::new(
        Path::new("private-repository"),
        Path::new("private-database"),
        "private-identity",
        0,
    );
    let request = LocalPollingRunnerRequest::new(index)
        .with_schedule(schedule(5, 100, 10, 2))
        .with_hint_limits(WatcherHintLimits::try_new(8, 512).expect("hint limits should validate"))
        .with_max_runtime(Duration::from_millis(100));
    let debug = format!("{request:?}");
    assert!(!debug.contains("private-repository"));
    assert!(!debug.contains("private-database"));
    assert!(!debug.contains("private-identity"));

    let report = run_local_polling_reconciliation(request, Arc::new(AtomicBool::new(true)))
        .expect("preexisting cancellation should exit before I/O");
    assert_eq!(report.exit(), PollingRunnerExit::Cancelled);
    assert_eq!(report.state_counters().reconciliations_started(), 0);
}
