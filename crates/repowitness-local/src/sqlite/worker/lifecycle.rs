const MUTATION_LEASE_SUFFIX: &str = ".repowitness-mutation.lock";
const MUTATION_LEASE_RETRY_DELAY: Duration = Duration::from_millis(10);
const WORKER_EXIT_POLL_DELAY: Duration = Duration::from_millis(1);

#[derive(Default)]
struct WriterHooks {
    #[cfg(test)]
    after_commit_before_reply: Option<Box<dyn FnMut() + Send>>,
    #[cfg(test)]
    fail_next_mutation_progress_handler_clear: bool,
    #[cfg(test)]
    fail_next_commit: Option<Arc<AtomicBool>>,
    #[cfg(test)]
    before_read_reply: Option<Box<dyn FnMut() + Send>>,
    #[cfg(test)]
    after_shutdown_reply: Option<Box<dyn FnMut() + Send>>,
}

impl WriterHooks {
    #[cfg(test)]
    fn with_post_commit_pause(hook: impl FnMut() + Send + 'static) -> Self {
        Self {
            after_commit_before_reply: Some(Box::new(hook)),
            ..Self::default()
        }
    }

    #[cfg(test)]
    fn with_progress_handler_clear_failure() -> Self {
        Self {
            fail_next_mutation_progress_handler_clear: true,
            ..Self::default()
        }
    }

    #[cfg(test)]
    fn with_commit_failure_control(fail_next_commit: Arc<AtomicBool>) -> Self {
        Self {
            fail_next_commit: Some(fail_next_commit),
            ..Self::default()
        }
    }

    #[cfg(test)]
    fn with_read_reply_pause(hook: impl FnMut() + Send + 'static) -> Self {
        Self {
            before_read_reply: Some(Box::new(hook)),
            ..Self::default()
        }
    }

    #[cfg(test)]
    fn with_shutdown_exit_pause(hook: impl FnMut() + Send + 'static) -> Self {
        Self {
            after_shutdown_reply: Some(Box::new(hook)),
            ..Self::default()
        }
    }

    fn install_on(&self, state: &WriterState) -> Result<(), SqliteStoreError> {
        #[cfg(not(test))]
        let _ = state;
        #[cfg(test)]
        if let Some(fail_next_commit) = &self.fail_next_commit {
            state.install_commit_failure_control(Arc::clone(fail_next_commit))?;
        }
        Ok(())
    }

    fn after_commit_before_reply<T>(&mut self, result: &Result<T, SqliteStoreError>) {
        #[cfg(not(test))]
        let _ = result;
        #[cfg(test)]
        if result.is_ok()
            && let Some(hook) = self.after_commit_before_reply.as_mut()
        {
            hook();
        }
    }

    fn before_read_reply(&mut self) {
        #[cfg(test)]
        if let Some(mut hook) = self.before_read_reply.take() {
            hook();
        }
    }

    fn after_shutdown_reply(&mut self) {
        #[cfg(test)]
        if let Some(mut hook) = self.after_shutdown_reply.take() {
            hook();
        }
    }

    fn take_mutation_progress_handler_clear_failure(&mut self) -> bool {
        #[cfg(test)]
        {
            std::mem::take(&mut self.fail_next_mutation_progress_handler_clear)
        }
        #[cfg(not(test))]
        {
            false
        }
    }
}

pub(crate) struct SqliteMutationLease {
    database_path: PathBuf,
    _file: File,
}

impl SqliteMutationLease {
    pub(crate) fn acquire(
        database_path: &Path,
        deadline: Instant,
    ) -> Result<Self, SqliteStoreError> {
        Self::acquire_with_cancel(database_path, None, deadline)
    }

    pub(crate) fn acquire_with_cancel(
        database_path: &Path,
        cancelled: Option<&AtomicBool>,
        deadline: Instant,
    ) -> Result<Self, SqliteStoreError> {
        if cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Acquire)) {
            return Err(SqliteStoreError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(SqliteStoreError::DeadlineExceeded);
        }
        let database_path = canonical_database_path(database_path)?;
        let file = acquire_mutation_lease(&database_path, cancelled, deadline)?;
        Ok(Self {
            database_path,
            _file: file,
        })
    }
}

impl OwnedSqliteIndex {
    /// Stops and joins the owned writer thread within the caller's deadline.
    pub fn shutdown(mut self, deadline: Instant) -> Result<(), SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(WriterCommand::Shutdown { reply }, deadline)?;
        receive_reply(&receiver, deadline)?;
        self.join_worker_until(deadline)
    }

    fn send(&self, command: WriterCommand, deadline: Instant) -> Result<(), SqliteStoreError> {
        if Instant::now() >= deadline {
            return Err(SqliteStoreError::DeadlineExceeded);
        }
        if command.is_mutating() && self.unresolved_mutation.load(Ordering::Acquire) {
            return Err(SqliteStoreError::MutationOutcomeUnknown);
        }
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(_) => SqliteStoreError::QueueFull,
                TrySendError::Disconnected(_) => SqliteStoreError::WorkerUnavailable,
            })
    }

    fn join_worker(&mut self) -> Result<(), SqliteStoreError> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker.join().map_err(|_| SqliteStoreError::WorkerPanicked)
    }

    fn join_worker_until(&mut self, deadline: Instant) -> Result<(), SqliteStoreError> {
        loop {
            if self.worker.as_ref().is_none_or(JoinHandle::is_finished) {
                return self.join_worker();
            }
            let now = Instant::now();
            if now >= deadline {
                let _ = self.worker.take();
                return Err(SqliteStoreError::DeadlineExceeded);
            }
            thread::sleep(WORKER_EXIT_POLL_DELAY.min(deadline.duration_since(now)));
        }
    }
}

fn acquire_mutation_lease(
    database_path: &Path,
    cancelled: Option<&AtomicBool>,
    deadline: Instant,
) -> Result<File, SqliteStoreError> {
    let lease_path = mutation_lease_path(database_path);
    let lease = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lease_path)
        .map_err(|_| SqliteStoreError::MutationLeaseUnavailable)?;

    loop {
        if cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Acquire)) {
            return Err(SqliteStoreError::Cancelled);
        }
        match lease.try_lock() {
            Ok(()) => return Ok(lease),
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(_)) => {
                return Err(SqliteStoreError::MutationLeaseUnavailable);
            }
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(SqliteStoreError::DeadlineExceeded);
        }
        thread::sleep(MUTATION_LEASE_RETRY_DELAY.min(deadline.duration_since(now)));
    }
}

fn mutation_lease_path(database_path: &Path) -> PathBuf {
    let mut lease_name = OsString::from(database_path.as_os_str());
    lease_name.push(MUTATION_LEASE_SUFFIX);
    PathBuf::from(lease_name)
}

impl Drop for OwnedSqliteIndex {
    fn drop(&mut self) {
        if self.worker.is_none() {
            return;
        }
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            let _ = self.join_worker();
            return;
        }
        let (reply, _receiver) = mpsc::sync_channel(1);
        let _ = self.commands.try_send(WriterCommand::Shutdown { reply });
        // Queue admission is not proof that the owner reached shutdown. A
        // detached owner retains the connection and mutation lease until
        // in-flight or queued work completes, then exits through shutdown or
        // sender disconnection.
        let _ = self.worker.take();
    }
}
