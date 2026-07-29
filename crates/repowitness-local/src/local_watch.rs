use std::{
    error::Error,
    fmt,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use crate::{
    LocalIndexReport, LocalIndexRequest, WatcherHintCounters, WatcherStateCounters,
    local_index::{
        LocalReconciliationOutcome,
        polling_runner::{
            LocalPollingRunnerError, LocalPollingRunnerRequest, PollingRunnerExit,
            run_local_polling_reconciliation,
        },
    },
};

/// Stable profile version for the foreground polling watch facade.
pub const LOCAL_WATCH_PROFILE_VERSION: u16 = 1;
/// Largest explicitly requested foreground watch runtime.
pub const MAX_LOCAL_WATCH_RUNTIME: Duration = Duration::from_millis(86_400_000);

/// Complete validated local index request plus optional foreground lifetime.
pub struct LocalWatchRequest<'a> {
    index: LocalIndexRequest<'a>,
    max_runtime: Option<Duration>,
}

impl<'a> LocalWatchRequest<'a> {
    /// Constructs an unbounded-lifetime foreground watch request.
    #[must_use]
    pub const fn new(index: LocalIndexRequest<'a>) -> Self {
        Self {
            index,
            max_runtime: None,
        }
    }

    /// Applies a positive bounded foreground lifetime.
    pub fn with_max_runtime(
        mut self,
        max_runtime: Duration,
    ) -> Result<Self, LocalWatchRequestError> {
        if max_runtime.is_zero() || max_runtime > MAX_LOCAL_WATCH_RUNTIME {
            return Err(LocalWatchRequestError::MaxRuntime);
        }
        self.max_runtime = Some(max_runtime);
        Ok(self)
    }
}

impl fmt::Debug for LocalWatchRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalWatchRequest")
            .field("index", &self.index)
            .field("max_runtime", &self.max_runtime)
            .finish()
    }
}

/// Stable invalid local watch request category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalWatchRequestError {
    /// The requested overall runtime is zero or exceeds one day.
    MaxRuntime,
}

impl fmt::Display for LocalWatchRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("local watch runtime is outside the supported range")
    }
}

impl Error for LocalWatchRequestError {}

/// Categorical reason the foreground watch loop returned normally.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalWatchExit {
    /// Cooperative cancellation was observed.
    Cancelled,
    /// The explicitly requested overall runtime elapsed.
    DeadlineExceeded,
}

/// Categorical result of the last complete reconciliation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalWatchReconciliation {
    /// A new immutable generation was published and activated.
    Published,
    /// A complete staged generation was activated.
    Resumed,
    /// The already-active generation exactly matched the current source.
    Unchanged,
}

/// Path-free aggregate receipt from one foreground watch session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalWatchReport {
    exit: LocalWatchExit,
    state_counters: WatcherStateCounters,
    hint_counters: WatcherHintCounters,
    last_reconciliation: Option<LocalWatchReconciliation>,
    last_index: Option<LocalIndexReport>,
}

impl LocalWatchReport {
    /// Returns the normal foreground-loop exit category.
    #[must_use]
    pub const fn exit(self) -> LocalWatchExit {
        self.exit
    }

    /// Returns aggregate polling lifecycle counters.
    #[must_use]
    pub const fn state_counters(self) -> WatcherStateCounters {
        self.state_counters
    }

    /// Returns aggregate event-hint counters.
    #[must_use]
    pub const fn hint_counters(self) -> WatcherHintCounters {
        self.hint_counters
    }

    /// Returns the last complete reconciliation category, if any.
    #[must_use]
    pub const fn last_reconciliation(self) -> Option<LocalWatchReconciliation> {
        self.last_reconciliation
    }

    /// Returns the last complete aggregate index receipt, if any.
    #[must_use]
    pub const fn last_index(self) -> Option<LocalIndexReport> {
        self.last_index
    }
}

/// Opaque path-free foreground watch failure.
#[derive(Debug)]
pub struct LocalWatchError {
    source: LocalPollingRunnerError,
}

impl fmt::Display for LocalWatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("local watch failed")
    }
}

impl Error for LocalWatchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// Runs complete reconciliation in the foreground until cancellation or an
/// explicit overall deadline. Each successful change atomically activates one
/// immutable generation; cancellation and failure preserve the prior active
/// generation.
pub fn watch_local_repository(
    request: LocalWatchRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalWatchReport, LocalWatchError> {
    let mut runner = LocalPollingRunnerRequest::new(request.index);
    if let Some(max_runtime) = request.max_runtime {
        runner = runner.with_max_runtime(max_runtime);
    }
    let report = run_local_polling_reconciliation(runner, cancelled)
        .map_err(|source| LocalWatchError { source })?;
    let exit = match report.exit() {
        PollingRunnerExit::Cancelled => LocalWatchExit::Cancelled,
        PollingRunnerExit::DeadlineExceeded => LocalWatchExit::DeadlineExceeded,
    };
    let (last_reconciliation, last_index) = report.last_success().map_or((None, None), |outcome| {
        let (kind, report) = match outcome {
            LocalReconciliationOutcome::Published(report) => {
                (LocalWatchReconciliation::Published, *report)
            }
            LocalReconciliationOutcome::Resumed(report) => {
                (LocalWatchReconciliation::Resumed, *report)
            }
            LocalReconciliationOutcome::Unchanged(report) => {
                (LocalWatchReconciliation::Unchanged, *report)
            }
        };
        (Some(kind), Some(report))
    });
    Ok(LocalWatchReport {
        exit,
        state_counters: report.state_counters(),
        hint_counters: report.hint_counters(),
        last_reconciliation,
        last_index,
    })
}

#[cfg(test)]
mod tests;
