//! Pure bounded hint accumulation and polling decisions for reconciliation.
//!
//! This module owns no filesystem handles, threads, timers, signals, database
//! connections, or indexing tasks. Callers inject validated repository paths
//! and fixed-width monotonic timestamps.

mod hints;
mod schedule;
mod supervisor;

pub use hints::{
    DEFAULT_WATCHER_HINT_PATH_BYTES, DEFAULT_WATCHER_HINT_PATHS, MAX_WATCHER_HINT_PATH_BYTES,
    MAX_WATCHER_HINT_PATHS, WatcherFullReconciliationCauses, WatcherHintAccumulator,
    WatcherHintAdmission, WatcherHintBatch, WatcherHintCounters, WatcherHintLimitError,
    WatcherHintLimits, WatcherPathByteCount, WatcherPathCount,
};
pub use schedule::{
    DEFAULT_WATCHER_DEBOUNCE_MS, DEFAULT_WATCHER_MAX_RETRIES, DEFAULT_WATCHER_PERIODIC_MS,
    DEFAULT_WATCHER_RETRY_DELAY_MS, MAX_WATCHER_DEBOUNCE_MS, MAX_WATCHER_PERIODIC_MS,
    MAX_WATCHER_RETRIES, MAX_WATCHER_RETRY_DELAY_MS, WatcherCompletion, WatcherCompletionOutcome,
    WatcherDurationMillis, WatcherMonotonicTimestamp, WatcherObservationOutcome,
    WatcherPollDecision, WatcherPollingState, WatcherReconciliationReason, WatcherRetryAttempt,
    WatcherScheduleLimitError, WatcherScheduleLimits, WatcherStateCounters, WatcherStateError,
};
pub use supervisor::{
    CompleteReconciliationWork, DEFAULT_WATCHER_POLL_INTERVAL_MS, PollingHintObservation,
    PollingReconciliationRequest, PollingReconciliationSupervisor, WatcherPollIntervalMillis,
};

/// Version of the pure watcher reconciliation state contract.
pub const WATCH_RECONCILIATION_PROFILE_VERSION: u32 = 1;

#[cfg(test)]
mod tests;
