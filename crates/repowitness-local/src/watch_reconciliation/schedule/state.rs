use std::fmt;

use super::{
    WatcherCompletion, WatcherCompletionOutcome, WatcherMonotonicTimestamp,
    WatcherObservationOutcome, WatcherPollDecision, WatcherReconciliationReason,
    WatcherRetryAttempt, WatcherScheduleLimits, WatcherStateCounters, WatcherStateError,
};

/// Pure single-slot polling and reconciliation admission state.
pub struct WatcherPollingState {
    limits: WatcherScheduleLimits,
    last_timestamp: WatcherMonotonicTimestamp,
    next_periodic: WatcherMonotonicTimestamp,
    dirty_deadline: Option<WatcherMonotonicTimestamp>,
    retry_deadline: Option<(WatcherMonotonicTimestamp, WatcherRetryAttempt)>,
    pending: Option<WatcherReconciliationReason>,
    active: bool,
    full_reconciliation_required: bool,
    retry_attempts: u16,
    retry_exhausted: bool,
    cancelled: bool,
    counters: WatcherStateCounters,
}

impl WatcherPollingState {
    /// Creates a state machine with exactly one startup reconciliation pending.
    pub fn new(
        started_at: WatcherMonotonicTimestamp,
        limits: WatcherScheduleLimits,
    ) -> Result<Self, WatcherStateError> {
        let next_periodic = started_at
            .checked_add(limits.periodic)
            .ok_or(WatcherStateError::TimestampOverflow)?;
        Ok(Self {
            limits,
            last_timestamp: started_at,
            next_periodic,
            dirty_deadline: None,
            retry_deadline: None,
            pending: Some(WatcherReconciliationReason::Startup),
            active: false,
            full_reconciliation_required: false,
            retry_attempts: 0,
            retry_exhausted: false,
            cancelled: false,
            counters: WatcherStateCounters::default(),
        })
    }

    /// Evaluates one injected monotonic timestamp without starting work.
    #[must_use]
    pub fn poll(&mut self, now: WatcherMonotonicTimestamp) -> WatcherPollDecision {
        if self.cancelled {
            return WatcherPollDecision::Cancelled;
        }
        if !self.admit_clock(now) {
            return self.pending_or_backpressured();
        }
        if let Some(reason) = self.pending {
            return WatcherPollDecision::Pending(reason);
        }
        if self.active {
            return WatcherPollDecision::Backpressured;
        }
        if self.retry_exhausted {
            return self.poll_retry_exhausted(now);
        }
        if let Some((at, attempt)) = self.retry_deadline {
            if now < at {
                return WatcherPollDecision::WaitingRetry { at, attempt };
            }
            self.pending = Some(WatcherReconciliationReason::Retry(attempt));
            return WatcherPollDecision::Pending(WatcherReconciliationReason::Retry(attempt));
        }
        if self.full_reconciliation_required {
            return self.make_pending(WatcherReconciliationReason::FullReconciliationRequired);
        }
        if now >= self.next_periodic {
            return self.make_pending(WatcherReconciliationReason::Periodic);
        }
        if let Some(until) = self.dirty_deadline {
            if now >= until {
                return self.make_pending(WatcherReconciliationReason::DirtyAfterDebounce);
            }
            return WatcherPollDecision::Debouncing { until };
        }
        WatcherPollDecision::WaitingPeriodic {
            at: self.next_periodic,
        }
    }

    /// Marks ordinary dirty input and refreshes the bounded debounce deadline.
    pub fn observe_dirty(&mut self, now: WatcherMonotonicTimestamp) -> WatcherObservationOutcome {
        self.observe(now, false)
    }

    /// Marks overflow or unsupported input that requires immediate full work.
    pub fn observe_full_reconciliation_required(
        &mut self,
        now: WatcherMonotonicTimestamp,
    ) -> WatcherObservationOutcome {
        self.observe(now, true)
    }

    /// Consumes the one pending decision and marks one reconciliation active.
    pub fn begin_pending(&mut self) -> Result<WatcherReconciliationReason, WatcherStateError> {
        if self.cancelled {
            return Err(WatcherStateError::Cancelled);
        }
        if self.active {
            return Err(WatcherStateError::ReconciliationAlreadyActive);
        }
        let reason = self
            .pending
            .ok_or(WatcherStateError::NoPendingReconciliation)?;
        if !increment(&mut self.counters.reconciliations_started) {
            self.force_full(WatcherReconciliationReason::CounterOverflow);
            return Err(WatcherStateError::CounterOverflow);
        }
        self.pending = None;
        self.active = true;
        self.dirty_deadline = None;
        self.retry_deadline = None;
        self.full_reconciliation_required = false;
        if matches!(reason, WatcherReconciliationReason::Periodic) {
            self.retry_exhausted = false;
            self.retry_attempts = 0;
        }
        Ok(reason)
    }

    /// Completes the one active reconciliation.
    pub fn complete(
        &mut self,
        now: WatcherMonotonicTimestamp,
        completion: WatcherCompletion,
    ) -> Result<WatcherCompletionOutcome, WatcherStateError> {
        if !self.active {
            return Err(WatcherStateError::NoActiveReconciliation);
        }
        if now < self.last_timestamp {
            self.note_clock_regression();
            return Err(WatcherStateError::ClockRegression);
        }
        self.last_timestamp = now;
        match completion {
            WatcherCompletion::Succeeded => self.complete_success(now),
            WatcherCompletion::RetryableFailure => self.complete_retryable_failure(now),
            WatcherCompletion::Cancelled => {
                self.active = false;
                self.cancel();
                Ok(WatcherCompletionOutcome::Cancelled)
            }
        }
    }

    /// Permanently stops new polling and reconciliation admission.
    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.pending = None;
        self.dirty_deadline = None;
        self.retry_deadline = None;
        self.full_reconciliation_required = false;
    }

    /// Returns the one pending reason without consuming it.
    #[must_use]
    pub const fn pending_reason(&self) -> Option<WatcherReconciliationReason> {
        self.pending
    }

    /// Reports whether one reconciliation is active.
    #[must_use]
    pub const fn reconciliation_active(&self) -> bool {
        self.active
    }

    /// Reports whether cancellation stopped the state machine.
    #[must_use]
    pub const fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Returns cumulative non-sensitive counters.
    #[must_use]
    pub const fn counters(&self) -> WatcherStateCounters {
        self.counters
    }

    fn observe(
        &mut self,
        now: WatcherMonotonicTimestamp,
        require_full: bool,
    ) -> WatcherObservationOutcome {
        if self.cancelled {
            return WatcherObservationOutcome::Cancelled;
        }
        if !self.admit_clock(now) {
            return WatcherObservationOutcome::ClockRegression;
        }
        if self.retry_exhausted {
            self.retry_exhausted = false;
            self.retry_attempts = 0;
        }
        if require_full {
            self.full_reconciliation_required = true;
            self.retry_deadline = None;
        } else {
            let Some(deadline) = now.checked_add(self.limits.debounce) else {
                self.force_full(WatcherReconciliationReason::TimestampOverflow);
                return WatcherObservationOutcome::TimestampOverflow;
            };
            self.dirty_deadline = Some(deadline);
        }
        self.observation_admission(require_full || self.full_reconciliation_required)
    }

    fn observation_admission(&mut self, require_full: bool) -> WatcherObservationOutcome {
        if self.active {
            if !increment(&mut self.counters.backpressure_observations) {
                self.force_full(WatcherReconciliationReason::CounterOverflow);
                return WatcherObservationOutcome::CounterOverflow;
            }
            return WatcherObservationOutcome::Backpressured;
        }
        if self.pending.is_some() {
            if !increment(&mut self.counters.coalesced_observations) {
                self.force_full(WatcherReconciliationReason::CounterOverflow);
                return WatcherObservationOutcome::CounterOverflow;
            }
            return WatcherObservationOutcome::Coalesced;
        }
        if require_full {
            self.pending = Some(WatcherReconciliationReason::FullReconciliationRequired);
            WatcherObservationOutcome::FullReconciliationPending
        } else {
            WatcherObservationOutcome::DirtyDebouncing
        }
    }

    fn complete_success(
        &mut self,
        now: WatcherMonotonicTimestamp,
    ) -> Result<WatcherCompletionOutcome, WatcherStateError> {
        let Some(next_periodic) = now.checked_add(self.limits.periodic) else {
            self.active = false;
            self.force_full(WatcherReconciliationReason::TimestampOverflow);
            return Err(WatcherStateError::TimestampOverflow);
        };
        if !increment(&mut self.counters.reconciliations_completed) {
            self.active = false;
            self.force_full(WatcherReconciliationReason::CounterOverflow);
            return Err(WatcherStateError::CounterOverflow);
        }
        self.active = false;
        self.next_periodic = next_periodic;
        self.retry_deadline = None;
        self.retry_attempts = 0;
        self.retry_exhausted = false;
        Ok(WatcherCompletionOutcome::Completed)
    }

    fn complete_retryable_failure(
        &mut self,
        now: WatcherMonotonicTimestamp,
    ) -> Result<WatcherCompletionOutcome, WatcherStateError> {
        self.active = false;
        if !increment(&mut self.counters.retryable_failures) {
            self.force_full(WatcherReconciliationReason::CounterOverflow);
            return Err(WatcherStateError::CounterOverflow);
        }
        let Some(next_attempt) = self.retry_attempts.checked_add(1) else {
            self.force_full(WatcherReconciliationReason::CounterOverflow);
            return Err(WatcherStateError::CounterOverflow);
        };
        self.retry_attempts = next_attempt;
        if next_attempt > self.limits.max_retries {
            self.retry_deadline = None;
            self.retry_exhausted = true;
            self.full_reconciliation_required = true;
            return Ok(WatcherCompletionOutcome::RetryExhausted);
        }
        let Some(at) = now.checked_add(self.limits.retry_delay) else {
            self.force_full(WatcherReconciliationReason::TimestampOverflow);
            return Err(WatcherStateError::TimestampOverflow);
        };
        let attempt = WatcherRetryAttempt(next_attempt);
        self.retry_deadline = Some((at, attempt));
        Ok(WatcherCompletionOutcome::RetryScheduled { attempt, at })
    }

    fn poll_retry_exhausted(&mut self, now: WatcherMonotonicTimestamp) -> WatcherPollDecision {
        if now >= self.next_periodic {
            self.retry_exhausted = false;
            self.retry_attempts = 0;
            return self.make_pending(WatcherReconciliationReason::Periodic);
        }
        WatcherPollDecision::RetryExhausted
    }

    fn make_pending(&mut self, reason: WatcherReconciliationReason) -> WatcherPollDecision {
        self.pending = Some(reason);
        WatcherPollDecision::Pending(reason)
    }

    fn admit_clock(&mut self, now: WatcherMonotonicTimestamp) -> bool {
        if now < self.last_timestamp {
            self.note_clock_regression();
            false
        } else {
            self.last_timestamp = now;
            true
        }
    }

    fn note_clock_regression(&mut self) {
        if !increment(&mut self.counters.clock_regressions) {
            self.force_full(WatcherReconciliationReason::CounterOverflow);
        } else {
            self.force_full(WatcherReconciliationReason::ClockRegression);
        }
    }

    fn force_full(&mut self, reason: WatcherReconciliationReason) {
        self.full_reconciliation_required = true;
        self.retry_deadline = None;
        if !self.active && self.pending.is_none() && !self.cancelled {
            self.pending = Some(reason);
        }
    }

    fn pending_or_backpressured(&mut self) -> WatcherPollDecision {
        if let Some(reason) = self.pending {
            WatcherPollDecision::Pending(reason)
        } else if self.active {
            WatcherPollDecision::Backpressured
        } else {
            self.make_pending(WatcherReconciliationReason::ClockRegression)
        }
    }

    #[cfg(test)]
    pub(in crate::watch_reconciliation) fn set_reconciliations_started_for_test(
        &mut self,
        value: u64,
    ) {
        self.counters.reconciliations_started = value;
    }

    #[cfg(test)]
    pub(in crate::watch_reconciliation) fn set_reconciliations_completed_for_test(
        &mut self,
        value: u64,
    ) {
        self.counters.reconciliations_completed = value;
    }

    #[cfg(test)]
    pub(in crate::watch_reconciliation) fn set_retryable_failures_for_test(&mut self, value: u64) {
        self.counters.retryable_failures = value;
    }

    #[cfg(test)]
    pub(in crate::watch_reconciliation) fn set_coalesced_observations_for_test(
        &mut self,
        value: u64,
    ) {
        self.counters.coalesced_observations = value;
    }

    #[cfg(test)]
    pub(in crate::watch_reconciliation) fn set_backpressure_observations_for_test(
        &mut self,
        value: u64,
    ) {
        self.counters.backpressure_observations = value;
    }

    #[cfg(test)]
    pub(in crate::watch_reconciliation) fn set_clock_regressions_for_test(&mut self, value: u64) {
        self.counters.clock_regressions = value;
    }
}

impl fmt::Debug for WatcherPollingState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WatcherPollingState")
            .field("limits", &self.limits)
            .field("last_timestamp", &self.last_timestamp)
            .field("next_periodic", &self.next_periodic)
            .field("dirty_pending", &self.dirty_deadline.is_some())
            .field("retry_pending", &self.retry_deadline.is_some())
            .field("pending", &self.pending)
            .field("active", &self.active)
            .field(
                "full_reconciliation_required",
                &self.full_reconciliation_required,
            )
            .field("retry_attempts", &self.retry_attempts)
            .field("retry_exhausted", &self.retry_exhausted)
            .field("cancelled", &self.cancelled)
            .field("counters", &self.counters)
            .finish()
    }
}

fn increment(counter: &mut u64) -> bool {
    let next = counter.saturating_add(1);
    let advanced = next != *counter;
    *counter = next;
    advanced
}
