use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use repowitness_domain::RepositoryPath;

use crate::{
    LocalIndexError, LocalIndexRequest, PollingReconciliationRequest,
    PollingReconciliationSupervisor, WatcherCompletion, WatcherHintCounters, WatcherHintLimits,
    WatcherMonotonicTimestamp, WatcherPollDecision, WatcherReconciliationReason,
    WatcherScheduleLimits, WatcherStateCounters, WatcherStateError,
    local_index::LocalReconciliationOutcome,
};

mod system_environment;

#[cfg(test)]
pub(super) use system_environment::reconcile_local_repository;
use system_environment::{LocalPollingAttemptError, SystemPollingEnvironment, duration_millis};

const MAX_OBSERVATIONS_PER_TURN: usize = 1_024;

pub(crate) struct LocalPollingRunnerRequest<'a> {
    index: LocalIndexRequest<'a>,
    schedule: WatcherScheduleLimits,
    hints: WatcherHintLimits,
    max_runtime: Option<Duration>,
    native_event_hints: bool,
}

impl<'a> LocalPollingRunnerRequest<'a> {
    pub(crate) const fn new(index: LocalIndexRequest<'a>) -> Self {
        Self {
            index,
            schedule: WatcherScheduleLimits::DEFAULT,
            hints: WatcherHintLimits::DEFAULT,
            max_runtime: None,
            native_event_hints: false,
        }
    }

    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "custom schedule injection remains an internal deterministic-test seam"
        )
    )]
    pub(crate) const fn with_schedule(mut self, schedule: WatcherScheduleLimits) -> Self {
        self.schedule = schedule;
        self
    }

    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "custom hint-limit injection remains an internal deterministic-test seam"
        )
    )]
    pub(crate) const fn with_hint_limits(mut self, hints: WatcherHintLimits) -> Self {
        self.hints = hints;
        self
    }

    pub(crate) const fn with_max_runtime(mut self, max_runtime: Duration) -> Self {
        self.max_runtime = Some(max_runtime);
        self
    }

    pub(crate) const fn with_native_event_hints(mut self) -> Self {
        self.native_event_hints = true;
        self
    }
}

impl fmt::Debug for LocalPollingRunnerRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalPollingRunnerRequest")
            .field("index", &self.index)
            .field("schedule", &self.schedule)
            .field("hints", &self.hints)
            .field("max_runtime", &self.max_runtime)
            .field("native_event_hints", &self.native_event_hints)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PollingRunnerExit {
    Cancelled,
    DeadlineExceeded,
}

#[derive(Debug)]
pub(crate) struct PollingRunnerReport<Success> {
    exit: PollingRunnerExit,
    state_counters: WatcherStateCounters,
    hint_counters: WatcherHintCounters,
    last_success: Option<Success>,
}

impl<Success> PollingRunnerReport<Success> {
    pub(crate) const fn exit(&self) -> PollingRunnerExit {
        self.exit
    }

    pub(crate) const fn state_counters(&self) -> WatcherStateCounters {
        self.state_counters
    }

    pub(crate) const fn hint_counters(&self) -> WatcherHintCounters {
        self.hint_counters
    }

    pub(crate) const fn last_success(&self) -> Option<&Success> {
        self.last_success.as_ref()
    }
}

pub(crate) type LocalPollingRunnerReport = PollingRunnerReport<LocalReconciliationOutcome>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PollingRunnerControlError {
    TimestampOverflow,
    CancellationBridgeUnavailable,
    CancellationBridgePanicked,
}

impl fmt::Display for PollingRunnerControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TimestampOverflow => "polling runner timestamp is not representable",
            Self::CancellationBridgeUnavailable => {
                "polling runner cancellation bridge is unavailable"
            }
            Self::CancellationBridgePanicked => "polling runner cancellation bridge panicked",
        })
    }
}

impl Error for PollingRunnerControlError {}

#[derive(Debug)]
pub(crate) enum LocalPollingRunnerError {
    State(WatcherStateError),
    Control(PollingRunnerControlError),
    NativeEventHints(NativeEventHintsError),
    Reconciliation(LocalIndexError),
}

impl fmt::Display for LocalPollingRunnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::State(_) => "polling reconciliation state failed",
            Self::Control(_) => "polling reconciliation control failed",
            Self::NativeEventHints(_) => "native event hints failed",
            Self::Reconciliation(_) => "polling reconciliation failed",
        })
    }
}

impl Error for LocalPollingRunnerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::State(source) => Some(source),
            Self::Control(source) => Some(source),
            Self::NativeEventHints(source) => Some(source),
            Self::Reconciliation(source) => Some(source),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeEventHintsError {
    #[cfg(not(target_os = "linux"))]
    Unavailable,
    Initialization,
}

impl fmt::Display for NativeEventHintsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            #[cfg(not(target_os = "linux"))]
            Self::Unavailable => "native event hints are unavailable on this host",
            Self::Initialization => "native event hints could not be initialized",
        })
    }
}

impl Error for NativeEventHintsError {}

pub(crate) fn run_local_polling_reconciliation(
    request: LocalPollingRunnerRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalPollingRunnerReport, LocalPollingRunnerError> {
    let started_at = WatcherMonotonicTimestamp::from_millis(0);
    let reconciliation = PollingReconciliationRequest::new(started_at, request.index.configuration)
        .with_schedule_limits(request.schedule)
        .with_hint_limits(request.hints);
    let reconciliation = if request.native_event_hints {
        reconciliation.with_native_event_hints()
    } else {
        reconciliation
    };
    let supervisor = PollingReconciliationSupervisor::start(reconciliation)
        .map_err(LocalPollingRunnerError::State)?;
    let deadline = request
        .max_runtime
        .map(duration_millis)
        .transpose()
        .map_err(LocalPollingRunnerError::Control)?
        .map(WatcherMonotonicTimestamp::from_millis);
    if cancelled.load(Ordering::Acquire) {
        return Ok(runner_report(
            PollingRunnerExit::Cancelled,
            &supervisor,
            None,
        ));
    }
    let mut environment = if request.native_event_hints {
        SystemPollingEnvironment::with_native_event_hints(request.index)
            .map_err(LocalPollingRunnerError::NativeEventHints)?
    } else {
        SystemPollingEnvironment::new(request.index)
    };
    run_with_environment(supervisor, cancelled, deadline, &mut environment).map_err(|error| {
        match error {
            PollingRunError::State(source) => LocalPollingRunnerError::State(source),
            PollingRunError::Control(source) => LocalPollingRunnerError::Control(source),
            PollingRunError::Fatal(LocalPollingAttemptError::Control(source)) => {
                LocalPollingRunnerError::Control(source)
            }
            PollingRunError::Fatal(LocalPollingAttemptError::Reconciliation(source)) => {
                LocalPollingRunnerError::Reconciliation(source)
            }
        }
    })
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "event observations are retained for a future bounded backend; production currently polls complete state"
    )
)]
enum RunnerObservation {
    Path(RepositoryPath),
    Unsupported,
}

enum PollingAttempt<Success, Failure> {
    Succeeded(Success),
    Retryable(Failure),
    Fatal(Failure),
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the explicit environment cancellation outcome remains a deterministic-test seam"
        )
    )]
    Cancelled,
}

trait PollingRunnerEnvironment {
    type Success;
    type Failure;
    type ControlError;

    fn now(&mut self) -> Result<WatcherMonotonicTimestamp, Self::ControlError>;

    fn wait_until(
        &mut self,
        at: WatcherMonotonicTimestamp,
        cancelled: &AtomicBool,
    ) -> Result<(), Self::ControlError>;

    fn next_observation(&mut self) -> Option<RunnerObservation>;

    fn reconcile(
        &mut self,
        reason: WatcherReconciliationReason,
        cancelled: Arc<AtomicBool>,
        remaining: Option<Duration>,
    ) -> PollingAttempt<Self::Success, Self::Failure>;
}

enum PollingRunError<Failure, ControlError> {
    State(WatcherStateError),
    Control(ControlError),
    Fatal(Failure),
}

type EnvironmentRunResult<Environment> = Result<
    PollingRunnerReport<<Environment as PollingRunnerEnvironment>::Success>,
    PollingRunError<
        <Environment as PollingRunnerEnvironment>::Failure,
        <Environment as PollingRunnerEnvironment>::ControlError,
    >,
>;

type AttemptRunResult<Environment> = Result<
    Option<PollingRunnerExit>,
    PollingRunError<
        <Environment as PollingRunnerEnvironment>::Failure,
        <Environment as PollingRunnerEnvironment>::ControlError,
    >,
>;

struct PollingLoopState<Success> {
    next_source_poll: WatcherMonotonicTimestamp,
    last_success: Option<Success>,
}

fn run_with_environment<Environment>(
    mut supervisor: PollingReconciliationSupervisor,
    cancelled: Arc<AtomicBool>,
    deadline: Option<WatcherMonotonicTimestamp>,
    environment: &mut Environment,
) -> EnvironmentRunResult<Environment>
where
    Environment: PollingRunnerEnvironment,
{
    let mut state = PollingLoopState {
        next_source_poll: add_millis(
            environment.now().map_err(PollingRunError::Control)?,
            supervisor.effective_poll_interval().get(),
        )
        .map_err(|_| PollingRunError::State(WatcherStateError::TimestampOverflow))?,
        last_success: None,
    };

    loop {
        let now = environment.now().map_err(PollingRunError::Control)?;
        if cancelled.load(Ordering::Acquire) {
            supervisor.cancel();
            return Ok(runner_report(
                PollingRunnerExit::Cancelled,
                &supervisor,
                state.last_success,
            ));
        }
        if deadline.is_some_and(|deadline| now >= deadline) {
            supervisor.cancel();
            return Ok(runner_report(
                PollingRunnerExit::DeadlineExceeded,
                &supervisor,
                state.last_success,
            ));
        }
        drain_observations(environment, &mut supervisor, now);

        let mut decision = supervisor.poll(now);
        if source_poll_is_admissible(decision) && now >= state.next_source_poll {
            let _observation = supervisor.observe_unsupported_event(now);
            state.next_source_poll = add_millis(now, supervisor.effective_poll_interval().get())
                .map_err(|_| PollingRunError::State(WatcherStateError::TimestampOverflow))?;
            decision = supervisor.poll(now);
        }

        match decision {
            WatcherPollDecision::Pending(_) => {
                if let Some(exit) = run_pending_attempt(
                    &mut supervisor,
                    environment,
                    &cancelled,
                    deadline,
                    now,
                    &mut state,
                )? {
                    return Ok(runner_report(exit, &supervisor, state.last_success));
                }
            }
            WatcherPollDecision::Debouncing { until }
            | WatcherPollDecision::WaitingPeriodic { at: until } => {
                wait_until(
                    environment,
                    earliest_deadline(until, state.next_source_poll, deadline),
                    cancelled.as_ref(),
                )?;
            }
            WatcherPollDecision::WaitingRetry { at, .. } => {
                wait_until(
                    environment,
                    deadline.map_or(at, |deadline| at.min(deadline)),
                    cancelled.as_ref(),
                )?;
            }
            WatcherPollDecision::RetryExhausted => {
                wait_until(
                    environment,
                    deadline.map_or(state.next_source_poll, |deadline| {
                        state.next_source_poll.min(deadline)
                    }),
                    cancelled.as_ref(),
                )?;
            }
            WatcherPollDecision::Cancelled => {
                return Ok(runner_report(
                    PollingRunnerExit::Cancelled,
                    &supervisor,
                    state.last_success,
                ));
            }
            WatcherPollDecision::Backpressured => {
                return Err(PollingRunError::State(
                    WatcherStateError::ReconciliationAlreadyActive,
                ));
            }
        }
    }
}

fn run_pending_attempt<Environment>(
    supervisor: &mut PollingReconciliationSupervisor,
    environment: &mut Environment,
    cancelled: &Arc<AtomicBool>,
    deadline: Option<WatcherMonotonicTimestamp>,
    started: WatcherMonotonicTimestamp,
    state: &mut PollingLoopState<Environment::Success>,
) -> AttemptRunResult<Environment>
where
    Environment: PollingRunnerEnvironment,
{
    let work = supervisor.begin_pending().map_err(PollingRunError::State)?;
    let remaining = deadline.map(|deadline| {
        Duration::from_millis(deadline.as_millis().saturating_sub(started.as_millis()))
    });
    let attempt = environment.reconcile(work.reason(), Arc::clone(cancelled), remaining);
    let finished = environment.now().map_err(PollingRunError::Control)?;
    drain_observations(environment, supervisor, finished);
    state.next_source_poll = add_millis(finished, supervisor.effective_poll_interval().get())
        .map_err(|_| PollingRunError::State(WatcherStateError::TimestampOverflow))?;
    match attempt {
        PollingAttempt::Succeeded(success) => {
            supervisor
                .complete(finished, WatcherCompletion::Succeeded)
                .map_err(PollingRunError::State)?;
            state.last_success = Some(success);
            if cancelled.load(Ordering::Acquire) {
                supervisor.cancel();
                Ok(Some(PollingRunnerExit::Cancelled))
            } else {
                Ok(None)
            }
        }
        PollingAttempt::Retryable(_failure) => {
            if cancelled.load(Ordering::Acquire) {
                supervisor
                    .complete(finished, WatcherCompletion::Cancelled)
                    .map_err(PollingRunError::State)?;
                return Ok(Some(PollingRunnerExit::Cancelled));
            }
            supervisor
                .complete(finished, WatcherCompletion::RetryableFailure)
                .map_err(PollingRunError::State)?;
            Ok(None)
        }
        PollingAttempt::Fatal(failure) => {
            supervisor.cancel();
            Err(PollingRunError::Fatal(failure))
        }
        PollingAttempt::Cancelled => {
            supervisor
                .complete(finished, WatcherCompletion::Cancelled)
                .map_err(PollingRunError::State)?;
            Ok(Some(PollingRunnerExit::Cancelled))
        }
    }
}

fn wait_until<Environment>(
    environment: &mut Environment,
    at: WatcherMonotonicTimestamp,
    cancelled: &AtomicBool,
) -> Result<(), PollingRunError<Environment::Failure, Environment::ControlError>>
where
    Environment: PollingRunnerEnvironment,
{
    environment
        .wait_until(at, cancelled)
        .map_err(PollingRunError::Control)
}

fn drain_observations<Environment>(
    environment: &mut Environment,
    supervisor: &mut PollingReconciliationSupervisor,
    now: WatcherMonotonicTimestamp,
) where
    Environment: PollingRunnerEnvironment,
{
    for _ in 0..MAX_OBSERVATIONS_PER_TURN {
        let Some(observation) = environment.next_observation() else {
            return;
        };
        match observation {
            RunnerObservation::Path(path) => {
                let _outcome = supervisor.observe_hint(path, now);
            }
            RunnerObservation::Unsupported => {
                let _outcome = supervisor.observe_unsupported_event(now);
            }
        }
    }
    let _overflow = supervisor.observe_unsupported_event(now);
}

const fn source_poll_is_admissible(decision: WatcherPollDecision) -> bool {
    matches!(
        decision,
        WatcherPollDecision::Debouncing { .. }
            | WatcherPollDecision::WaitingPeriodic { .. }
            | WatcherPollDecision::RetryExhausted
    )
}

fn earliest_deadline(
    scheduled: WatcherMonotonicTimestamp,
    source_poll: WatcherMonotonicTimestamp,
    overall: Option<WatcherMonotonicTimestamp>,
) -> WatcherMonotonicTimestamp {
    let earliest = scheduled.min(source_poll);
    overall.map_or(earliest, |overall| earliest.min(overall))
}

fn add_millis(
    timestamp: WatcherMonotonicTimestamp,
    duration_ms: u64,
) -> Result<WatcherMonotonicTimestamp, PollingRunnerControlError> {
    timestamp
        .as_millis()
        .checked_add(duration_ms)
        .map(WatcherMonotonicTimestamp::from_millis)
        .ok_or(PollingRunnerControlError::TimestampOverflow)
}

fn runner_report<Success>(
    exit: PollingRunnerExit,
    supervisor: &PollingReconciliationSupervisor,
    last_success: Option<Success>,
) -> PollingRunnerReport<Success> {
    PollingRunnerReport {
        exit,
        state_counters: supervisor.state_counters(),
        hint_counters: supervisor.hint_counters(),
        last_success,
    }
}

#[cfg(test)]
mod tests;
