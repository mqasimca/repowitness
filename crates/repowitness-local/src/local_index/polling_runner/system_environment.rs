use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    LocalIndexError, LocalIndexRequest, LocalRustIndexError, LocalRustIndexLimits,
    LocalSourceSnapshotFenceError, SqliteStoreError, WatcherMonotonicTimestamp,
    WatcherReconciliationReason, local_index::LocalReconciliationOutcome,
};

use super::{
    PollingAttempt, PollingRunnerControlError, PollingRunnerEnvironment, RunnerObservation,
};

const CANCELLATION_CHECK_INTERVAL: Duration = Duration::from_millis(10);

pub(super) struct SystemPollingEnvironment<'a> {
    origin: Instant,
    index: LocalIndexRequest<'a>,
}

impl<'a> SystemPollingEnvironment<'a> {
    pub(super) fn new(index: LocalIndexRequest<'a>) -> Self {
        Self {
            origin: Instant::now(),
            index,
        }
    }
}

pub(super) enum LocalPollingAttemptError {
    Control(PollingRunnerControlError),
    Reconciliation(LocalIndexError),
}

impl PollingRunnerEnvironment for SystemPollingEnvironment<'_> {
    type Success = LocalReconciliationOutcome;
    type Failure = LocalPollingAttemptError;
    type ControlError = PollingRunnerControlError;

    fn now(&mut self) -> Result<WatcherMonotonicTimestamp, Self::ControlError> {
        duration_millis(self.origin.elapsed()).map(WatcherMonotonicTimestamp::from_millis)
    }

    fn wait_until(
        &mut self,
        at: WatcherMonotonicTimestamp,
        cancelled: &AtomicBool,
    ) -> Result<(), Self::ControlError> {
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Ok(());
            }
            let now = self.now()?;
            if now >= at {
                return Ok(());
            }
            let remaining = Duration::from_millis(at.as_millis() - now.as_millis());
            thread::sleep(remaining.min(CANCELLATION_CHECK_INTERVAL));
        }
    }

    fn next_observation(&mut self) -> Option<RunnerObservation> {
        None
    }

    fn reconcile(
        &mut self,
        _reason: WatcherReconciliationReason,
        cancelled: Arc<AtomicBool>,
        remaining: Option<Duration>,
    ) -> PollingAttempt<Self::Success, Self::Failure> {
        let index = remaining.map_or(self.index, |remaining| {
            with_deadline_cap(self.index, remaining)
        });
        match reconcile_with_cancellation_bridge(index, cancelled) {
            Ok(outcome) => PollingAttempt::Succeeded(outcome),
            Err(LocalPollingAttemptError::Control(source)) => {
                PollingAttempt::Fatal(LocalPollingAttemptError::Control(source))
            }
            Err(LocalPollingAttemptError::Reconciliation(source))
                if retryable_local_index_error(&source) =>
            {
                PollingAttempt::Retryable(LocalPollingAttemptError::Reconciliation(source))
            }
            Err(error) => PollingAttempt::Fatal(error),
        }
    }
}

fn reconcile_with_cancellation_bridge(
    request: LocalIndexRequest<'_>,
    service_cancelled: Arc<AtomicBool>,
) -> Result<LocalReconciliationOutcome, LocalPollingAttemptError> {
    if service_cancelled.load(Ordering::Acquire) {
        return Err(LocalPollingAttemptError::Reconciliation(
            LocalIndexError::Preparation {
                source: LocalRustIndexError::Cancelled,
            },
        ));
    }
    let attempt_cancelled = Arc::new(AtomicBool::new(false));
    let monitor_done = Arc::new(AtomicBool::new(false));
    let outcome = thread::scope(|scope| {
        let service = Arc::clone(&service_cancelled);
        let attempt = Arc::clone(&attempt_cancelled);
        let done = Arc::clone(&monitor_done);
        let monitor = thread::Builder::new()
            .name("repowitness-poll-cancel".to_owned())
            .spawn_scoped(scope, move || {
                while !done.load(Ordering::Acquire) {
                    if service.load(Ordering::Acquire) {
                        attempt.store(true, Ordering::Release);
                        return;
                    }
                    thread::sleep(CANCELLATION_CHECK_INTERVAL);
                }
            })
            .map_err(|_| {
                LocalPollingAttemptError::Control(
                    PollingRunnerControlError::CancellationBridgeUnavailable,
                )
            })?;
        let result = reconcile_local_repository(request, Arc::clone(&attempt_cancelled));
        monitor_done.store(true, Ordering::Release);
        monitor.join().map_err(|_| {
            LocalPollingAttemptError::Control(PollingRunnerControlError::CancellationBridgePanicked)
        })?;
        result.map_err(LocalPollingAttemptError::Reconciliation)
    })?;
    Ok(outcome)
}

pub(in crate::local_index) fn reconcile_local_repository(
    request: LocalIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalReconciliationOutcome, LocalIndexError> {
    super::super::index_local_repository_with_mode(request, cancelled, true, || {}, || {})
}

fn with_deadline_cap(
    mut request: LocalIndexRequest<'_>,
    deadline: Duration,
) -> LocalIndexRequest<'_> {
    request.limits = LocalRustIndexLimits::new(
        request.limits.deadline().min(deadline),
        request.limits.discovery(),
        request.limits.source_read(),
        request.limits.preparation(),
    );
    request
}

pub(super) fn duration_millis(duration: Duration) -> Result<u64, PollingRunnerControlError> {
    let millis = u64::try_from(duration.as_millis())
        .map_err(|_| PollingRunnerControlError::TimestampOverflow)?;
    Ok(if duration.is_zero() { 0 } else { millis.max(1) })
}

fn retryable_local_index_error(error: &LocalIndexError) -> bool {
    match error {
        LocalIndexError::Preparation { source } => retryable_preparation_error(source),
        LocalIndexError::StoreStartup { source }
        | LocalIndexError::ArtifactReuse { source }
        | LocalIndexError::WorkspaceRegistration { source }
        | LocalIndexError::PublicationStaging { source }
        | LocalIndexError::GraphPublicationStaging { source }
        | LocalIndexError::PublicationActivation { source }
        | LocalIndexError::Checkpoint { source }
        | LocalIndexError::Shutdown { source } => retryable_store_error(*source),
        LocalIndexError::FinalSourceFence { source } => matches!(
            source,
            LocalSourceSnapshotFenceError::Cancelled
                | LocalSourceSnapshotFenceError::DeadlineExceeded
                | LocalSourceSnapshotFenceError::CaptureFailed
                | LocalSourceSnapshotFenceError::SourceChanged
        ),
        LocalIndexError::DatabaseChangedDuringIndexing => true,
        LocalIndexError::RepositoryIdentity { .. }
        | LocalIndexError::ConfigurationResolution { .. }
        | LocalIndexError::InvalidEffectiveConfiguration
        | LocalIndexError::DeadlineNotRepresentable
        | LocalIndexError::DatabasePathUnavailable
        | LocalIndexError::DatabaseInsideWorktree
        | LocalIndexError::DatabaseHasMultipleLinks
        | LocalIndexError::GraphPreparation { .. }
        | LocalIndexError::MutationOutcomeUnknown { .. } => false,
    }
}

fn retryable_preparation_error(error: &LocalRustIndexError) -> bool {
    matches!(
        error,
        LocalRustIndexError::Cancelled
            | LocalRustIndexError::DeadlineExceeded
            | LocalRustIndexError::Discovery { .. }
            | LocalRustIndexError::SourceState { .. }
            | LocalRustIndexError::RootOpen { .. }
            | LocalRustIndexError::SourceRead { .. }
            | LocalRustIndexError::ArtifactReuse { .. }
            | LocalRustIndexError::StalePathSet
            | LocalRustIndexError::StaleSourceContent { .. }
            | LocalRustIndexError::RevalidationRead { .. }
    )
}

const fn retryable_store_error(error: SqliteStoreError) -> bool {
    matches!(
        error,
        SqliteStoreError::OpenFailed
            | SqliteStoreError::MutationLeaseUnavailable
            | SqliteStoreError::DatabaseIdentityChanged
            | SqliteStoreError::DatabaseOperationFailed
            | SqliteStoreError::StaleSourceEpoch
            | SqliteStoreError::GenerationUnavailable
            | SqliteStoreError::Cancelled
            | SqliteStoreError::DeadlineExceeded
            | SqliteStoreError::QueueFull
            | SqliteStoreError::WorkerUnavailable
            | SqliteStoreError::WorkerPanicked
            | SqliteStoreError::ReplyTimeout
    )
}
