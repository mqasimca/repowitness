use std::{error::Error, fmt};

/// Maximum admitted debounce interval in monotonic milliseconds.
pub const MAX_WATCHER_DEBOUNCE_MS: u64 = 60_000;
/// Maximum admitted periodic reconciliation interval in monotonic milliseconds.
pub const MAX_WATCHER_PERIODIC_MS: u64 = 86_400_000;
/// Maximum admitted retry delay in monotonic milliseconds.
pub const MAX_WATCHER_RETRY_DELAY_MS: u64 = 3_600_000;
/// Maximum retry attempts after one initial reconciliation attempt.
pub const MAX_WATCHER_RETRIES: u16 = 64;
/// Conservative default debounce interval.
pub const DEFAULT_WATCHER_DEBOUNCE_MS: u64 = 250;
/// Conservative default maximum interval between complete reconciliations.
pub const DEFAULT_WATCHER_PERIODIC_MS: u64 = 30_000;
/// Conservative default fixed retry delay for this pure first slice.
pub const DEFAULT_WATCHER_RETRY_DELAY_MS: u64 = 1_000;
/// Conservative default retry-attempt limit.
pub const DEFAULT_WATCHER_MAX_RETRIES: u16 = 5;

/// An injected fixed-width monotonic timestamp in milliseconds.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WatcherMonotonicTimestamp(u64);

impl WatcherMonotonicTimestamp {
    /// Creates an injected monotonic timestamp.
    #[must_use]
    pub const fn from_millis(value: u64) -> Self {
        Self(value)
    }

    /// Returns the fixed-width monotonic millisecond value.
    #[must_use]
    pub const fn as_millis(self) -> u64 {
        self.0
    }

    fn checked_add(self, duration: WatcherDurationMillis) -> Option<Self> {
        self.0.checked_add(duration.0).map(Self)
    }
}

/// A fixed-width monotonic duration in milliseconds.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WatcherDurationMillis(u64);

impl WatcherDurationMillis {
    /// Returns the fixed-width duration.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// One-based retry-attempt number.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WatcherRetryAttempt(u16);

impl WatcherRetryAttempt {
    /// Returns the fixed-width one-based retry number.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Validated timing and retry policy for one polling state machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WatcherScheduleLimits {
    debounce: WatcherDurationMillis,
    periodic: WatcherDurationMillis,
    retry_delay: WatcherDurationMillis,
    max_retries: u16,
}

impl WatcherScheduleLimits {
    /// Conservative default polling policy.
    pub const DEFAULT: Self = Self {
        debounce: WatcherDurationMillis(DEFAULT_WATCHER_DEBOUNCE_MS),
        periodic: WatcherDurationMillis(DEFAULT_WATCHER_PERIODIC_MS),
        retry_delay: WatcherDurationMillis(DEFAULT_WATCHER_RETRY_DELAY_MS),
        max_retries: DEFAULT_WATCHER_MAX_RETRIES,
    };

    /// Creates positive limits no larger than compiled hard ceilings.
    pub fn try_new(
        debounce_ms: u64,
        periodic_ms: u64,
        retry_delay_ms: u64,
        max_retries: u16,
    ) -> Result<Self, WatcherScheduleLimitError> {
        validate_schedule_limits(debounce_ms, periodic_ms, retry_delay_ms, max_retries)?;
        Ok(Self {
            debounce: WatcherDurationMillis(debounce_ms),
            periodic: WatcherDurationMillis(periodic_ms),
            retry_delay: WatcherDurationMillis(retry_delay_ms),
            max_retries,
        })
    }

    /// Returns the debounce interval.
    #[must_use]
    pub const fn debounce(self) -> WatcherDurationMillis {
        self.debounce
    }

    /// Returns the mandatory complete-reconciliation interval.
    #[must_use]
    pub const fn periodic(self) -> WatcherDurationMillis {
        self.periodic
    }

    /// Returns the fixed retry delay.
    #[must_use]
    pub const fn retry_delay(self) -> WatcherDurationMillis {
        self.retry_delay
    }

    /// Returns the maximum number of retries after the initial attempt.
    #[must_use]
    pub const fn max_retries(self) -> u16 {
        self.max_retries
    }
}

impl Default for WatcherScheduleLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

fn validate_schedule_limits(
    debounce_ms: u64,
    periodic_ms: u64,
    retry_delay_ms: u64,
    max_retries: u16,
) -> Result<(), WatcherScheduleLimitError> {
    if debounce_ms == 0 {
        return Err(WatcherScheduleLimitError::ZeroDebounce);
    }
    if debounce_ms > MAX_WATCHER_DEBOUNCE_MS {
        return Err(WatcherScheduleLimitError::DebounceTooLarge);
    }
    if periodic_ms == 0 {
        return Err(WatcherScheduleLimitError::ZeroPeriodic);
    }
    if periodic_ms > MAX_WATCHER_PERIODIC_MS {
        return Err(WatcherScheduleLimitError::PeriodicTooLarge);
    }
    if retry_delay_ms == 0 {
        return Err(WatcherScheduleLimitError::ZeroRetryDelay);
    }
    if retry_delay_ms > MAX_WATCHER_RETRY_DELAY_MS {
        return Err(WatcherScheduleLimitError::RetryDelayTooLarge);
    }
    if max_retries == 0 {
        return Err(WatcherScheduleLimitError::ZeroRetries);
    }
    if max_retries > MAX_WATCHER_RETRIES {
        return Err(WatcherScheduleLimitError::RetriesTooLarge);
    }
    Ok(())
}

/// Redacted schedule-limit failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatcherScheduleLimitError {
    /// The debounce interval is zero.
    ZeroDebounce,
    /// The debounce interval exceeds its hard ceiling.
    DebounceTooLarge,
    /// The periodic interval is zero.
    ZeroPeriodic,
    /// The periodic interval exceeds its hard ceiling.
    PeriodicTooLarge,
    /// The retry delay is zero.
    ZeroRetryDelay,
    /// The retry delay exceeds its hard ceiling.
    RetryDelayTooLarge,
    /// The retry-attempt limit is zero.
    ZeroRetries,
    /// The retry-attempt limit exceeds its hard ceiling.
    RetriesTooLarge,
}

impl fmt::Display for WatcherScheduleLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ZeroDebounce => "watcher debounce must be positive",
            Self::DebounceTooLarge => "watcher debounce exceeds its hard ceiling",
            Self::ZeroPeriodic => "watcher periodic interval must be positive",
            Self::PeriodicTooLarge => "watcher periodic interval exceeds its hard ceiling",
            Self::ZeroRetryDelay => "watcher retry delay must be positive",
            Self::RetryDelayTooLarge => "watcher retry delay exceeds its hard ceiling",
            Self::ZeroRetries => "watcher retry limit must be positive",
            Self::RetriesTooLarge => "watcher retry limit exceeds its hard ceiling",
        })
    }
}

impl Error for WatcherScheduleLimitError {}

/// Why one complete reconciliation is pending.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatcherReconciliationReason {
    /// Mandatory complete startup reconciliation.
    Startup,
    /// Dirty hints reached the quiet-period deadline.
    DirtyAfterDebounce,
    /// The maximum complete-reconciliation interval elapsed.
    Periodic,
    /// Overflow or unsupported input requires a complete reconciliation.
    FullReconciliationRequired,
    /// A prior retryable failure reached its retry deadline.
    Retry(WatcherRetryAttempt),
    /// An injected timestamp regressed.
    ClockRegression,
    /// A timestamp deadline could not be represented.
    TimestampOverflow,
    /// A fixed-width state counter saturated.
    CounterOverflow,
}

/// One deterministic polling decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatcherPollDecision {
    /// Exactly one reconciliation reason is pending admission.
    Pending(WatcherReconciliationReason),
    /// A dirty set is still inside its debounce interval.
    Debouncing {
        /// Inclusive timestamp at which dirty work becomes ready.
        until: WatcherMonotonicTimestamp,
    },
    /// No dirty or retry work exists before mandatory periodic reconciliation.
    WaitingPeriodic {
        /// Inclusive timestamp of the next mandatory complete reconciliation.
        at: WatcherMonotonicTimestamp,
    },
    /// A retry exists but its delay has not elapsed.
    WaitingRetry {
        /// Inclusive retry timestamp.
        at: WatcherMonotonicTimestamp,
        /// One-based retry attempt.
        attempt: WatcherRetryAttempt,
    },
    /// One reconciliation is active; no second decision is queued.
    Backpressured,
    /// The bounded retry budget is exhausted until a new event or periodic run.
    RetryExhausted,
    /// Cancellation permanently stopped new admission.
    Cancelled,
}

/// Outcome of observing dirty or mandatory-full state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatcherObservationOutcome {
    /// Dirty work is waiting for its debounce deadline.
    DirtyDebouncing,
    /// Complete reconciliation became immediately pending.
    FullReconciliationPending,
    /// One pending reconciliation coalesced this observation.
    Coalesced,
    /// One active reconciliation backpressured this observation.
    Backpressured,
    /// Clock regression forced complete reconciliation.
    ClockRegression,
    /// Timestamp arithmetic overflow forced complete reconciliation.
    TimestampOverflow,
    /// Counter saturation forced complete reconciliation.
    CounterOverflow,
    /// Cancellation ignored the observation.
    Cancelled,
}

/// Result supplied when one active reconciliation ends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatcherCompletion {
    /// Complete reconciliation and its final fence succeeded.
    Succeeded,
    /// The attempt failed in a way eligible for bounded retry.
    RetryableFailure,
    /// Cooperative cancellation ended the active attempt.
    Cancelled,
}

/// State transition after one active reconciliation ends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatcherCompletionOutcome {
    /// Success updated the periodic deadline.
    Completed,
    /// One bounded retry was scheduled.
    RetryScheduled {
        /// One-based retry attempt.
        attempt: WatcherRetryAttempt,
        /// Inclusive retry timestamp.
        at: WatcherMonotonicTimestamp,
    },
    /// The retry budget was exhausted.
    RetryExhausted,
    /// Cancellation stopped the state machine.
    Cancelled,
}

/// Cumulative non-sensitive polling-state counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WatcherStateCounters {
    reconciliations_started: u64,
    reconciliations_completed: u64,
    retryable_failures: u64,
    coalesced_observations: u64,
    backpressure_observations: u64,
    clock_regressions: u64,
}

impl WatcherStateCounters {
    /// Returns reconciliation tasks admitted.
    #[must_use]
    pub const fn reconciliations_started(self) -> u64 {
        self.reconciliations_started
    }

    /// Returns successful reconciliations.
    #[must_use]
    pub const fn reconciliations_completed(self) -> u64 {
        self.reconciliations_completed
    }

    /// Returns retryable failures.
    #[must_use]
    pub const fn retryable_failures(self) -> u64 {
        self.retryable_failures
    }

    /// Returns observations coalesced behind pending work.
    #[must_use]
    pub const fn coalesced_observations(self) -> u64 {
        self.coalesced_observations
    }

    /// Returns observations backpressured by active work.
    #[must_use]
    pub const fn backpressure_observations(self) -> u64 {
        self.backpressure_observations
    }

    /// Returns detected injected-clock regressions.
    #[must_use]
    pub const fn clock_regressions(self) -> u64 {
        self.clock_regressions
    }
}

/// Redacted invalid lifecycle or exhausted fixed-width state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatcherStateError {
    /// A deadline timestamp could not be represented.
    TimestampOverflow,
    /// A fixed-width diagnostic counter saturated.
    CounterOverflow,
    /// No reconciliation decision is pending.
    NoPendingReconciliation,
    /// A reconciliation is already active.
    ReconciliationAlreadyActive,
    /// No reconciliation is active.
    NoActiveReconciliation,
    /// A completion timestamp regressed.
    ClockRegression,
    /// Cancellation stopped the state machine.
    Cancelled,
}

impl fmt::Display for WatcherStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TimestampOverflow => "watcher timestamp overflow",
            Self::CounterOverflow => "watcher counter overflow",
            Self::NoPendingReconciliation => "no watcher reconciliation is pending",
            Self::ReconciliationAlreadyActive => "a watcher reconciliation is already active",
            Self::NoActiveReconciliation => "no watcher reconciliation is active",
            Self::ClockRegression => "watcher monotonic clock regressed",
            Self::Cancelled => "watcher reconciliation is cancelled",
        })
    }
}

impl Error for WatcherStateError {}

mod state;

pub use state::WatcherPollingState;
