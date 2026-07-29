use std::fmt;

use repowitness_application::ResolvedConfiguration;
use repowitness_domain::{ConfigurationDigest, RepositoryPath};

use super::{
    WATCH_RECONCILIATION_PROFILE_VERSION, WatcherCompletion, WatcherCompletionOutcome,
    WatcherHintAccumulator, WatcherHintAdmission, WatcherHintCounters, WatcherHintLimits,
    WatcherMonotonicTimestamp, WatcherObservationOutcome, WatcherPathByteCount, WatcherPathCount,
    WatcherPollDecision, WatcherPollingState, WatcherReconciliationReason, WatcherScheduleLimits,
    WatcherStateCounters, WatcherStateError,
};

/// Conservative polling interval used when no resolved configuration is supplied.
pub const DEFAULT_WATCHER_POLL_INTERVAL_MS: u64 = 2_000;

/// Effective bounded interval between source-poll observations.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WatcherPollIntervalMillis(u64);

impl WatcherPollIntervalMillis {
    /// Returns the fixed-width effective interval.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Borrowed, path-free inputs for one production polling supervisor.
#[derive(Clone, Copy)]
pub struct PollingReconciliationRequest<'a> {
    started_at: WatcherMonotonicTimestamp,
    schedule_limits: WatcherScheduleLimits,
    hint_limits: WatcherHintLimits,
    configuration: Option<&'a ResolvedConfiguration>,
}

impl<'a> PollingReconciliationRequest<'a> {
    /// Constructs a request with conservative compiled schedule and hint limits.
    #[must_use]
    pub const fn new(
        started_at: WatcherMonotonicTimestamp,
        configuration: Option<&'a ResolvedConfiguration>,
    ) -> Self {
        Self {
            started_at,
            schedule_limits: WatcherScheduleLimits::DEFAULT,
            hint_limits: WatcherHintLimits::DEFAULT,
            configuration,
        }
    }

    /// Replaces the already-validated schedule limits.
    #[must_use]
    pub const fn with_schedule_limits(mut self, limits: WatcherScheduleLimits) -> Self {
        self.schedule_limits = limits;
        self
    }

    /// Replaces the already-validated hint bounds.
    #[must_use]
    pub const fn with_hint_limits(mut self, limits: WatcherHintLimits) -> Self {
        self.hint_limits = limits;
        self
    }

    /// Returns the effective source-poll interval.
    ///
    /// A slower repository preference cannot postpone mandatory complete
    /// reconciliation beyond the compiled or caller-tightened periodic bound.
    #[must_use]
    pub fn effective_poll_interval(&self) -> WatcherPollIntervalMillis {
        let configured =
            self.configuration
                .map_or(DEFAULT_WATCHER_POLL_INTERVAL_MS, |configuration| {
                    *configuration
                        .preferences()
                        .watcher_poll_interval_ms()
                        .effective()
                });
        WatcherPollIntervalMillis(configured.min(self.schedule_limits.periodic().get()))
    }

    /// Returns the optional canonical semantic configuration identity.
    #[must_use]
    pub fn configuration_digest(&self) -> Option<ConfigurationDigest> {
        self.configuration.map(ResolvedConfiguration::digest)
    }
}

impl fmt::Debug for PollingReconciliationRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PollingReconciliationRequest")
            .field("backend", &"polling")
            .field("profile_version", &WATCH_RECONCILIATION_PROFILE_VERSION)
            .field("configuration_digest", &self.configuration_digest())
            .field("effective_poll_interval", &self.effective_poll_interval())
            .field("schedule_limits", &self.schedule_limits)
            .field("hint_limits", &self.hint_limits)
            .finish()
    }
}

/// Combined result of retaining one hint and scheduling authoritative work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PollingHintObservation {
    admission: WatcherHintAdmission,
    scheduling: WatcherObservationOutcome,
}

impl PollingHintObservation {
    /// Returns how the bounded hint accumulator handled the event.
    #[must_use]
    pub const fn admission(self) -> WatcherHintAdmission {
        self.admission
    }

    /// Returns how the one-slot scheduler handled the observation.
    #[must_use]
    pub const fn scheduling(self) -> WatcherObservationOutcome {
        self.scheduling
    }
}

/// Path-free admission token for one authoritative complete reconciliation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompleteReconciliationWork {
    reason: WatcherReconciliationReason,
}

impl CompleteReconciliationWork {
    /// Returns the categorical reason complete source work was admitted.
    #[must_use]
    pub const fn reason(self) -> WatcherReconciliationReason {
        self.reason
    }
}

/// One owned, bounded polling supervisor for a single source slot.
pub struct PollingReconciliationSupervisor {
    effective_poll_interval: WatcherPollIntervalMillis,
    configuration_digest: Option<ConfigurationDigest>,
    schedule_limits: WatcherScheduleLimits,
    hint_limits: WatcherHintLimits,
    hints: WatcherHintAccumulator,
    state: WatcherPollingState,
}

impl PollingReconciliationSupervisor {
    /// Starts with exactly one mandatory complete startup reconciliation pending.
    pub fn start(request: PollingReconciliationRequest<'_>) -> Result<Self, WatcherStateError> {
        let state = WatcherPollingState::new(request.started_at, request.schedule_limits)?;
        Ok(Self {
            effective_poll_interval: request.effective_poll_interval(),
            configuration_digest: request.configuration_digest(),
            schedule_limits: request.schedule_limits,
            hint_limits: request.hint_limits,
            hints: WatcherHintAccumulator::new(request.hint_limits),
            state,
        })
    }

    /// Records one already-validated repository path as a scheduling hint only.
    pub fn observe_hint(
        &mut self,
        path: RepositoryPath,
        now: WatcherMonotonicTimestamp,
    ) -> PollingHintObservation {
        let admission = self.hints.record_hint(path);
        let scheduling = if hint_requires_full_reconciliation(admission) {
            self.state.observe_full_reconciliation_required(now)
        } else {
            self.state.observe_dirty(now)
        };
        PollingHintObservation {
            admission,
            scheduling,
        }
    }

    /// Records a backend event that cannot be represented as a safe path hint.
    pub fn observe_unsupported_event(
        &mut self,
        now: WatcherMonotonicTimestamp,
    ) -> PollingHintObservation {
        let admission = self.hints.record_unsupported_event();
        let scheduling = self.state.observe_full_reconciliation_required(now);
        PollingHintObservation {
            admission,
            scheduling,
        }
    }

    /// Evaluates one injected monotonic timestamp without starting work.
    #[must_use]
    pub fn poll(&mut self, now: WatcherMonotonicTimestamp) -> WatcherPollDecision {
        self.state.poll(now)
    }

    /// Admits one path-free complete reconciliation and discards optimization hints.
    ///
    /// Callers must reconcile the complete canonical source state. Pending path
    /// hints are deliberately unavailable through this token.
    pub fn begin_pending(&mut self) -> Result<CompleteReconciliationWork, WatcherStateError> {
        let reason = self.state.begin_pending()?;
        let _discarded_hints = self.hints.drain();
        Ok(CompleteReconciliationWork { reason })
    }

    /// Completes the one active authoritative reconciliation.
    pub fn complete(
        &mut self,
        now: WatcherMonotonicTimestamp,
        completion: WatcherCompletion,
    ) -> Result<WatcherCompletionOutcome, WatcherStateError> {
        self.state.complete(now, completion)
    }

    /// Permanently stops new admission and discards pending optimization hints.
    pub fn cancel(&mut self) {
        self.state.cancel();
        let _discarded_hints = self.hints.drain();
    }

    /// Returns the effective source-poll interval.
    #[must_use]
    pub const fn effective_poll_interval(&self) -> WatcherPollIntervalMillis {
        self.effective_poll_interval
    }

    /// Returns the optional canonical semantic configuration identity.
    #[must_use]
    pub const fn configuration_digest(&self) -> Option<ConfigurationDigest> {
        self.configuration_digest
    }

    /// Returns the authoritative scheduling and retry bounds.
    #[must_use]
    pub const fn schedule_limits(&self) -> WatcherScheduleLimits {
        self.schedule_limits
    }

    /// Returns the bounded dirty-hint limits.
    #[must_use]
    pub const fn hint_limits(&self) -> WatcherHintLimits {
        self.hint_limits
    }

    /// Returns pending distinct hint-path count without exposing path bytes.
    #[must_use]
    pub const fn pending_hint_paths(&self) -> WatcherPathCount {
        self.hints.pending_path_count()
    }

    /// Returns pending aggregate hint-path bytes without exposing path bytes.
    #[must_use]
    pub const fn pending_hint_path_bytes(&self) -> WatcherPathByteCount {
        self.hints.pending_path_bytes()
    }

    /// Returns cumulative non-sensitive hint counters.
    #[must_use]
    pub const fn hint_counters(&self) -> WatcherHintCounters {
        self.hints.counters()
    }

    /// Returns cumulative non-sensitive scheduler counters.
    #[must_use]
    pub const fn state_counters(&self) -> WatcherStateCounters {
        self.state.counters()
    }

    /// Reports whether cancellation permanently stopped admission.
    #[must_use]
    pub const fn is_cancelled(&self) -> bool {
        self.state.is_cancelled()
    }
}

impl fmt::Debug for PollingReconciliationSupervisor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PollingReconciliationSupervisor")
            .field("backend", &"polling")
            .field("profile_version", &WATCH_RECONCILIATION_PROFILE_VERSION)
            .field("configuration_digest", &self.configuration_digest)
            .field("effective_poll_interval", &self.effective_poll_interval)
            .field("schedule_limits", &self.schedule_limits)
            .field("hint_limits", &self.hint_limits)
            .field("pending_hint_paths", &self.pending_hint_paths())
            .field("pending_hint_path_bytes", &self.pending_hint_path_bytes())
            .field("hint_counters", &self.hint_counters())
            .field("state", &self.state)
            .finish()
    }
}

const fn hint_requires_full_reconciliation(admission: WatcherHintAdmission) -> bool {
    matches!(
        admission,
        WatcherHintAdmission::PathCountOverflow
            | WatcherHintAdmission::PathByteOverflow
            | WatcherHintAdmission::UnsupportedEvent
            | WatcherHintAdmission::CounterOverflow
            | WatcherHintAdmission::FullReconciliationAlreadyRequired
    )
}
