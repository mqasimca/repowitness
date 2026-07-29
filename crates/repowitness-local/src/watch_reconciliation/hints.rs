use std::{collections::BTreeSet, error::Error, fmt};

use repowitness_domain::RepositoryPath;

/// Compiled hard ceiling for distinct retained dirty paths.
pub const MAX_WATCHER_HINT_PATHS: u32 = 65_536;
/// Compiled hard ceiling for aggregate retained repository-path bytes.
pub const MAX_WATCHER_HINT_PATH_BYTES: u64 = 64 * 1024 * 1024;
/// Conservative default distinct dirty-path limit.
pub const DEFAULT_WATCHER_HINT_PATHS: u32 = 4_096;
/// Conservative default aggregate dirty-path byte limit.
pub const DEFAULT_WATCHER_HINT_PATH_BYTES: u64 = 4 * 1024 * 1024;

const PATH_COUNT_OVERFLOW: u8 = 1;
const PATH_BYTE_OVERFLOW: u8 = 1 << 1;
const UNSUPPORTED_EVENT: u8 = 1 << 2;
const COUNTER_OVERFLOW: u8 = 1 << 3;

/// A fixed-width number of distinct retained repository paths.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WatcherPathCount(u32);

impl WatcherPathCount {
    /// Returns the fixed-width count.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A fixed-width number of retained repository-path bytes.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WatcherPathByteCount(u64);

impl WatcherPathByteCount {
    /// Returns the fixed-width count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Strict bounds for one in-memory dirty-path accumulator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WatcherHintLimits {
    max_paths: WatcherPathCount,
    max_path_bytes: WatcherPathByteCount,
}

impl WatcherHintLimits {
    /// Conservative default limits.
    pub const DEFAULT: Self = Self {
        max_paths: WatcherPathCount(DEFAULT_WATCHER_HINT_PATHS),
        max_path_bytes: WatcherPathByteCount(DEFAULT_WATCHER_HINT_PATH_BYTES),
    };

    /// Creates positive limits no larger than compiled hard ceilings.
    pub fn try_new(max_paths: u32, max_path_bytes: u64) -> Result<Self, WatcherHintLimitError> {
        if max_paths == 0 {
            return Err(WatcherHintLimitError::ZeroPathLimit);
        }
        if max_paths > MAX_WATCHER_HINT_PATHS {
            return Err(WatcherHintLimitError::PathLimitTooLarge);
        }
        if max_path_bytes == 0 {
            return Err(WatcherHintLimitError::ZeroPathByteLimit);
        }
        if max_path_bytes > MAX_WATCHER_HINT_PATH_BYTES {
            return Err(WatcherHintLimitError::PathByteLimitTooLarge);
        }
        Ok(Self {
            max_paths: WatcherPathCount(max_paths),
            max_path_bytes: WatcherPathByteCount(max_path_bytes),
        })
    }

    /// Returns the inclusive distinct-path limit.
    #[must_use]
    pub const fn max_paths(self) -> WatcherPathCount {
        self.max_paths
    }

    /// Returns the inclusive aggregate path-byte limit.
    #[must_use]
    pub const fn max_path_bytes(self) -> WatcherPathByteCount {
        self.max_path_bytes
    }
}

impl Default for WatcherHintLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Redacted limit-construction failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatcherHintLimitError {
    /// The distinct-path limit is zero.
    ZeroPathLimit,
    /// The distinct-path limit exceeds its hard ceiling.
    PathLimitTooLarge,
    /// The aggregate path-byte limit is zero.
    ZeroPathByteLimit,
    /// The aggregate path-byte limit exceeds its hard ceiling.
    PathByteLimitTooLarge,
}

impl fmt::Display for WatcherHintLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ZeroPathLimit => "watcher distinct-path limit must be positive",
            Self::PathLimitTooLarge => "watcher distinct-path limit exceeds its hard ceiling",
            Self::ZeroPathByteLimit => "watcher path-byte limit must be positive",
            Self::PathByteLimitTooLarge => "watcher path-byte limit exceeds its hard ceiling",
        })
    }
}

impl Error for WatcherHintLimitError {}

/// Deterministic reasons that make a complete reconciliation mandatory.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub struct WatcherFullReconciliationCauses(u8);

impl WatcherFullReconciliationCauses {
    /// Reports a distinct-path capacity overflow.
    #[must_use]
    pub const fn path_count_overflow(self) -> bool {
        self.0 & PATH_COUNT_OVERFLOW != 0
    }

    /// Reports an aggregate path-byte overflow.
    #[must_use]
    pub const fn path_byte_overflow(self) -> bool {
        self.0 & PATH_BYTE_OVERFLOW != 0
    }

    /// Reports an event that cannot be represented as validated path hints.
    #[must_use]
    pub const fn unsupported_event(self) -> bool {
        self.0 & UNSUPPORTED_EVENT != 0
    }

    /// Reports saturation of a fixed-width diagnostic counter.
    #[must_use]
    pub const fn counter_overflow(self) -> bool {
        self.0 & COUNTER_OVERFLOW != 0
    }

    /// Reports whether any cause is present.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    fn insert(&mut self, cause: u8) {
        self.0 |= cause;
    }
}

impl fmt::Debug for WatcherFullReconciliationCauses {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WatcherFullReconciliationCauses")
            .field("path_count_overflow", &self.path_count_overflow())
            .field("path_byte_overflow", &self.path_byte_overflow())
            .field("unsupported_event", &self.unsupported_event())
            .field("counter_overflow", &self.counter_overflow())
            .finish()
    }
}

/// Cumulative non-sensitive accumulator counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WatcherHintCounters {
    observed_events: u64,
    duplicate_hints: u64,
    coalesced_hints: u64,
    overflow_events: u64,
    unsupported_events: u64,
}

impl WatcherHintCounters {
    /// Returns all observed path-hint and unsupported events.
    #[must_use]
    pub const fn observed_events(self) -> u64 {
        self.observed_events
    }

    /// Returns exact duplicate path hints.
    #[must_use]
    pub const fn duplicate_hints(self) -> u64 {
        self.duplicate_hints
    }

    /// Returns hints ignored after full reconciliation became mandatory.
    #[must_use]
    pub const fn coalesced_hints(self) -> u64 {
        self.coalesced_hints
    }

    /// Returns distinct-path or aggregate-byte overflow events.
    #[must_use]
    pub const fn overflow_events(self) -> u64 {
        self.overflow_events
    }

    /// Returns unsupported backend events.
    #[must_use]
    pub const fn unsupported_events(self) -> u64 {
        self.unsupported_events
    }
}

/// Outcome of admitting one watcher event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatcherHintAdmission {
    /// A new validated path was retained.
    Retained,
    /// An exact duplicate was coalesced.
    Duplicate,
    /// A distinct-path limit overflow discarded the bounded dirty set.
    PathCountOverflow,
    /// An aggregate path-byte overflow discarded the bounded dirty set.
    PathByteOverflow,
    /// An unsupported backend event requires complete reconciliation.
    UnsupportedEvent,
    /// A diagnostic counter saturated and requires complete reconciliation.
    CounterOverflow,
    /// Full reconciliation was already mandatory, so the hint was coalesced.
    FullReconciliationAlreadyRequired,
}

/// One drained canonical dirty set or a complete-reconciliation marker.
#[derive(Eq, PartialEq)]
pub struct WatcherHintBatch {
    paths: Box<[RepositoryPath]>,
    full_reconciliation_required: bool,
    causes: WatcherFullReconciliationCauses,
}

impl WatcherHintBatch {
    /// Returns exact paths in canonical byte order.
    #[must_use]
    pub fn paths(&self) -> &[RepositoryPath] {
        &self.paths
    }

    /// Reports whether callers must ignore hints and reconcile completely.
    #[must_use]
    pub const fn full_reconciliation_required(&self) -> bool {
        self.full_reconciliation_required
    }

    /// Returns deterministic full-reconciliation causes.
    #[must_use]
    pub const fn causes(&self) -> WatcherFullReconciliationCauses {
        self.causes
    }
}

impl fmt::Debug for WatcherHintBatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WatcherHintBatch")
            .field("path_count", &self.paths.len())
            .field(
                "full_reconciliation_required",
                &self.full_reconciliation_required,
            )
            .field("causes", &self.causes)
            .finish()
    }
}

/// Bounded canonical dirty-path accumulation for one source slot.
pub struct WatcherHintAccumulator {
    limits: WatcherHintLimits,
    paths: BTreeSet<RepositoryPath>,
    retained_path_count: u32,
    retained_path_bytes: u64,
    causes: WatcherFullReconciliationCauses,
    counters: WatcherHintCounters,
}

impl WatcherHintAccumulator {
    /// Creates an empty accumulator from already validated limits.
    #[must_use]
    pub fn new(limits: WatcherHintLimits) -> Self {
        Self {
            limits,
            paths: BTreeSet::new(),
            retained_path_count: 0,
            retained_path_bytes: 0,
            causes: WatcherFullReconciliationCauses::default(),
            counters: WatcherHintCounters::default(),
        }
    }

    /// Admits one already validated repository-relative path hint.
    pub fn record_hint(&mut self, path: RepositoryPath) -> WatcherHintAdmission {
        if !increment(&mut self.counters.observed_events) {
            return self.counter_overflow();
        }
        if !self.causes.is_empty() {
            if !increment(&mut self.counters.coalesced_hints) {
                return self.counter_overflow();
            }
            return WatcherHintAdmission::FullReconciliationAlreadyRequired;
        }
        if self.paths.contains(&path) {
            if !increment(&mut self.counters.duplicate_hints) {
                return self.counter_overflow();
            }
            return WatcherHintAdmission::Duplicate;
        }
        self.retain_new_path(path)
    }

    /// Records a backend event that cannot be represented as path hints.
    pub fn record_unsupported_event(&mut self) -> WatcherHintAdmission {
        if !increment(&mut self.counters.observed_events)
            || !increment(&mut self.counters.unsupported_events)
        {
            return self.counter_overflow();
        }
        self.require_full(UNSUPPORTED_EVENT);
        WatcherHintAdmission::UnsupportedEvent
    }

    /// Drains one canonical batch and resets only pending hints and causes.
    pub fn drain(&mut self) -> WatcherHintBatch {
        let causes = self.causes;
        let full_reconciliation_required = !causes.is_empty();
        let paths = if full_reconciliation_required {
            self.paths.clear();
            Box::default()
        } else {
            std::mem::take(&mut self.paths)
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice()
        };
        self.retained_path_count = 0;
        self.retained_path_bytes = 0;
        self.causes = WatcherFullReconciliationCauses::default();
        WatcherHintBatch {
            paths,
            full_reconciliation_required,
            causes,
        }
    }

    /// Returns the pending distinct-path count.
    #[must_use]
    pub const fn pending_path_count(&self) -> WatcherPathCount {
        WatcherPathCount(self.retained_path_count)
    }

    /// Returns the pending aggregate path-byte count.
    #[must_use]
    pub const fn pending_path_bytes(&self) -> WatcherPathByteCount {
        WatcherPathByteCount(self.retained_path_bytes)
    }

    /// Reports whether complete reconciliation is currently mandatory.
    #[must_use]
    pub const fn full_reconciliation_required(&self) -> bool {
        !self.causes.is_empty()
    }

    /// Returns cumulative non-sensitive diagnostic counters.
    #[must_use]
    pub const fn counters(&self) -> WatcherHintCounters {
        self.counters
    }

    fn retain_new_path(&mut self, path: RepositoryPath) -> WatcherHintAdmission {
        let Some(next_count) = self.retained_path_count.checked_add(1) else {
            return self.overflow(PATH_COUNT_OVERFLOW, WatcherHintAdmission::PathCountOverflow);
        };
        if next_count > self.limits.max_paths.get() {
            return self.overflow(PATH_COUNT_OVERFLOW, WatcherHintAdmission::PathCountOverflow);
        }
        let Some(next_bytes) = self
            .retained_path_bytes
            .checked_add(path.byte_count().get())
        else {
            return self.overflow(PATH_BYTE_OVERFLOW, WatcherHintAdmission::PathByteOverflow);
        };
        if next_bytes > self.limits.max_path_bytes.get() {
            return self.overflow(PATH_BYTE_OVERFLOW, WatcherHintAdmission::PathByteOverflow);
        }
        self.retained_path_count = next_count;
        self.retained_path_bytes = next_bytes;
        let inserted = self.paths.insert(path);
        debug_assert!(inserted, "duplicate was checked before insertion");
        WatcherHintAdmission::Retained
    }

    fn overflow(&mut self, cause: u8, admission: WatcherHintAdmission) -> WatcherHintAdmission {
        self.require_full(cause);
        if !increment(&mut self.counters.overflow_events) {
            self.require_full(COUNTER_OVERFLOW);
            WatcherHintAdmission::CounterOverflow
        } else {
            admission
        }
    }

    fn counter_overflow(&mut self) -> WatcherHintAdmission {
        self.require_full(COUNTER_OVERFLOW);
        WatcherHintAdmission::CounterOverflow
    }

    fn require_full(&mut self, cause: u8) {
        self.causes.insert(cause);
        self.paths.clear();
        self.retained_path_count = 0;
        self.retained_path_bytes = 0;
    }

    #[cfg(test)]
    pub(super) fn set_observed_events_for_test(&mut self, value: u64) {
        self.counters.observed_events = value;
    }

    #[cfg(test)]
    pub(super) fn set_duplicate_hints_for_test(&mut self, value: u64) {
        self.counters.duplicate_hints = value;
    }

    #[cfg(test)]
    pub(super) fn set_coalesced_hints_for_test(&mut self, value: u64) {
        self.counters.coalesced_hints = value;
    }

    #[cfg(test)]
    pub(super) fn set_overflow_events_for_test(&mut self, value: u64) {
        self.counters.overflow_events = value;
    }

    #[cfg(test)]
    pub(super) fn set_unsupported_events_for_test(&mut self, value: u64) {
        self.counters.unsupported_events = value;
    }
}

impl fmt::Debug for WatcherHintAccumulator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WatcherHintAccumulator")
            .field("limits", &self.limits)
            .field("pending_path_count", &self.pending_path_count())
            .field("pending_path_bytes", &self.pending_path_bytes())
            .field("causes", &self.causes)
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
