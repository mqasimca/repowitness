use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
use std::{fs, mem::MaybeUninit, path::PathBuf};

use crate::{
    LocalIndexError, LocalIndexRequest, LocalRustIndexError, LocalRustIndexLimits,
    LocalSourceSnapshotFenceError, SqliteStoreError, WatcherMonotonicTimestamp,
    WatcherReconciliationReason, local_index::LocalReconciliationOutcome,
};

use super::{
    NativeEventHintsError, PollingAttempt, PollingRunnerControlError, PollingRunnerEnvironment,
    RunnerObservation,
};

const CANCELLATION_CHECK_INTERVAL: Duration = Duration::from_millis(10);

pub(super) struct SystemPollingEnvironment<'a> {
    origin: Instant,
    index: LocalIndexRequest<'a>,
    native_event_hints: Option<NativeEventHints>,
}

impl<'a> SystemPollingEnvironment<'a> {
    pub(super) fn new(index: LocalIndexRequest<'a>) -> Self {
        Self {
            origin: Instant::now(),
            index,
            native_event_hints: None,
        }
    }

    pub(super) fn with_native_event_hints(
        index: LocalIndexRequest<'a>,
    ) -> Result<Self, NativeEventHintsError> {
        let native_event_hints = NativeEventHints::new(index.repository_root)?;
        Ok(Self {
            origin: Instant::now(),
            index,
            native_event_hints: Some(native_event_hints),
        })
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
        self.native_event_hints
            .as_mut()
            .and_then(NativeEventHints::next_observation)
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

const MAX_NATIVE_HINT_DIRECTORIES: usize = 8_192;

#[cfg(target_os = "linux")]
struct NativeEventHints {
    root: PathBuf,
    descriptor: rustix::fd::OwnedFd,
}

#[cfg(not(target_os = "linux"))]
struct NativeEventHints;

impl NativeEventHints {
    #[cfg(target_os = "linux")]
    fn new(root: &Path) -> Result<Self, NativeEventHintsError> {
        use rustix::fs::inotify;

        let descriptor =
            inotify::init(inotify::CreateFlags::CLOEXEC | inotify::CreateFlags::NONBLOCK)
                .map_err(|_| NativeEventHintsError::Initialization)?;
        let mut directories = vec![root.to_owned()];
        let mut cursor = 0_usize;
        while cursor < directories.len() && directories.len() < MAX_NATIVE_HINT_DIRECTORIES {
            let directory = &directories[cursor];
            if inotify::add_watch(&descriptor, directory, inotify::WatchFlags::ALL_EVENTS).is_err()
                && cursor == 0
            {
                return Err(NativeEventHintsError::Initialization);
            }
            if let Ok(entries) = fs::read_dir(directory) {
                for entry in entries.flatten() {
                    if entry
                        .file_type()
                        .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
                        && directories.len() < MAX_NATIVE_HINT_DIRECTORIES
                    {
                        directories.push(entry.path());
                    }
                }
            }
            cursor += 1;
        }
        Ok(Self {
            root: root.to_owned(),
            descriptor,
        })
    }

    #[cfg(not(target_os = "linux"))]
    fn new(_root: &Path) -> Result<Self, NativeEventHintsError> {
        Err(NativeEventHintsError::Unavailable)
    }

    #[cfg(target_os = "linux")]
    fn next_observation(&mut self) -> Option<RunnerObservation> {
        use rustix::{fs::inotify, io::Errno};

        let mut buffer = [MaybeUninit::uninit(); 4_096];
        let mut reader = inotify::Reader::new(&self.descriptor, &mut buffer);
        match reader.next() {
            Ok(_event) => {
                // Rebuild after each observed event so newly created
                // directories can contribute future hints. The bounded
                // watcher is still only an optimization; periodic complete
                // reconciliation remains the correctness fallback.
                if let Ok(replacement) = Self::new(&self.root) {
                    *self = replacement;
                }
                Some(RunnerObservation::Unsupported)
            }
            Err(Errno::AGAIN) => None,
            Err(_) => Some(RunnerObservation::Unsupported),
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn next_observation(&mut self) -> Option<RunnerObservation> {
        None
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
        | LocalIndexError::RawSyntaxPublicationStaging { source }
        | LocalIndexError::RepositoryTopologyPublicationStaging { source }
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
        | LocalIndexError::RawSyntaxPreparation { .. }
        | LocalIndexError::RepositoryTopologyPreparation { .. }
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
