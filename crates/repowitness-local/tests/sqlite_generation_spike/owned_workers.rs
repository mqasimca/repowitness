type OwnedWorkerResult<T> = Result<T, String>;

#[derive(Clone, Copy, Debug)]
struct OwnedCheckpointObservation {
    busy: i64,
    log_frames: i64,
    checkpointed_frames: i64,
    elapsed: Duration,
    wal_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OwnedReaderExit {
    Cancelled,
    DeadlineExceeded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BackupCancellationObservation {
    completed_steps: u32,
    elapsed: Duration,
    finished_at: Instant,
}

enum OwnedWriterCommand {
    Publish {
        generation_id: i64,
        source_epoch: i64,
        facts: Vec<Vec<u8>>,
        batch_limit: usize,
        reply: mpsc::SyncSender<OwnedWorkerResult<()>>,
    },
    Checkpoint {
        reply: mpsc::SyncSender<OwnedWorkerResult<OwnedCheckpointObservation>>,
    },
    ActiveGeneration {
        reply: mpsc::SyncSender<OwnedWorkerResult<Option<i64>>>,
    },
    Shutdown {
        reply: mpsc::SyncSender<OwnedWorkerResult<()>>,
    },
}

struct OwnedWriterClient {
    commands: mpsc::SyncSender<OwnedWriterCommand>,
}

impl OwnedWriterClient {
    fn command_result<T>(
        &self,
        command: OwnedWriterCommand,
        receiver: &mpsc::Receiver<OwnedWorkerResult<T>>,
    ) -> TestResult<OwnedWorkerResult<T>> {
        self.commands
            .try_send(command)
            .map_err(|_| io::Error::other("owned writer command queue unavailable"))?;
        Ok(receiver.recv_timeout(OWNED_REPLY_TIMEOUT)?)
    }

    fn publish_result(
        &self,
        generation_id: i64,
        source_epoch: i64,
        facts: Vec<Vec<u8>>,
        batch_limit: usize,
    ) -> TestResult<OwnedWorkerResult<()>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.command_result(
            OwnedWriterCommand::Publish {
                generation_id,
                source_epoch,
                facts,
                batch_limit,
                reply,
            },
            &receiver,
        )
    }

    fn publish(
        &self,
        generation_id: i64,
        source_epoch: i64,
        facts: Vec<Vec<u8>>,
        batch_limit: usize,
    ) -> TestResult {
        require_owned_worker_success(self.publish_result(
            generation_id,
            source_epoch,
            facts,
            batch_limit,
        )?)
    }

    fn checkpoint(&self) -> TestResult<OwnedCheckpointObservation> {
        let (reply, receiver) = mpsc::sync_channel(1);
        require_owned_worker_success(
            self.command_result(OwnedWriterCommand::Checkpoint { reply }, &receiver)?,
        )
    }

    fn active_generation(&self) -> TestResult<Option<i64>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        require_owned_worker_success(
            self.command_result(OwnedWriterCommand::ActiveGeneration { reply }, &receiver)?,
        )
    }

    fn shutdown(&self) -> TestResult {
        let (reply, receiver) = mpsc::sync_channel(1);
        require_owned_worker_success(
            self.command_result(OwnedWriterCommand::Shutdown { reply }, &receiver)?,
        )
    }
}

fn require_owned_worker_success<T>(result: OwnedWorkerResult<T>) -> TestResult<T> {
    result.map_err(|message| Box::new(io::Error::other(message)) as Box<dyn Error>)
}

fn send_owned_reply<T>(
    sender: &mpsc::SyncSender<OwnedWorkerResult<T>>,
    result: OwnedWorkerResult<T>,
) -> OwnedWorkerResult<()> {
    sender
        .send(result)
        .map_err(|_| "owned worker reply receiver disconnected".to_owned())
}

fn publish_owned_generation(
    connection: &mut Connection,
    generation_id: i64,
    source_epoch: i64,
    facts: &[Vec<u8>],
    batch_limit: usize,
) -> OwnedWorkerResult<()> {
    if facts.len() > OWNED_MAX_FACTS_PER_GENERATION {
        return Err("owned writer fact limit exceeded".to_owned());
    }
    if batch_limit == 0 || batch_limit > OWNED_MAX_FACTS_PER_GENERATION {
        return Err("owned writer batch limit invalid".to_owned());
    }
    stage_ready_generation(connection, generation_id, source_epoch, facts, batch_limit)
        .and_then(|()| activate_generation(connection, generation_id, source_epoch))
        .map_err(|error| error.to_string())
}

fn observe_owned_checkpoint(
    connection: &Connection,
    source_wal_path: &Path,
) -> OwnedWorkerResult<OwnedCheckpointObservation> {
    let started_at = Instant::now();
    let (busy, log_frames, checkpointed_frames) =
        truncate_checkpoint(connection).map_err(|error| error.to_string())?;
    let wal_bytes = fs::metadata(source_wal_path)
        .map_err(|error| error.to_string())?
        .len();
    Ok(OwnedCheckpointObservation {
        busy,
        log_frames,
        checkpointed_frames,
        elapsed: started_at.elapsed(),
        wal_bytes,
    })
}

fn process_owned_writer_command(
    connection: &mut Connection,
    source_wal_path: &Path,
    command: OwnedWriterCommand,
) -> OwnedWorkerResult<bool> {
    match command {
        OwnedWriterCommand::Publish {
            generation_id,
            source_epoch,
            facts,
            batch_limit,
            reply,
        } => {
            let result = publish_owned_generation(
                connection,
                generation_id,
                source_epoch,
                &facts,
                batch_limit,
            );
            send_owned_reply(&reply, result)?;
            Ok(false)
        }
        OwnedWriterCommand::Checkpoint { reply } => {
            let result = observe_owned_checkpoint(connection, source_wal_path);
            send_owned_reply(&reply, result)?;
            Ok(false)
        }
        OwnedWriterCommand::ActiveGeneration { reply } => {
            let result = active_generation_id(connection).map_err(|error| error.to_string());
            send_owned_reply(&reply, result)?;
            Ok(false)
        }
        OwnedWriterCommand::Shutdown { reply } => {
            send_owned_reply(&reply, Ok(()))?;
            Ok(true)
        }
    }
}

fn run_owned_writer_worker(
    database_path: &Path,
    commands: &mpsc::Receiver<OwnedWriterCommand>,
) -> OwnedWorkerResult<()> {
    let mut connection = open_file_database(database_path).map_err(|error| error.to_string())?;
    let source_wal_path = wal_path(database_path);

    loop {
        match commands.recv_timeout(OWNED_WORKER_POLL_INTERVAL) {
            Ok(command) => {
                if process_owned_writer_command(&mut connection, &source_wal_path, command)? {
                    return Ok(());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

fn run_owned_reader_worker(
    database_path: &Path,
    cancelled: &AtomicBool,
    reader_lifetime: Duration,
    ready: &mpsc::SyncSender<OwnedWorkerResult<Option<i64>>>,
    exited: &mpsc::SyncSender<OwnedWorkerResult<OwnedReaderExit>>,
) -> OwnedWorkerResult<()> {
    let mut connection = open_read_database(database_path).map_err(|error| error.to_string())?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let pinned_generation =
        active_generation_id(&transaction).map_err(|error| error.to_string())?;
    send_owned_reply(ready, Ok(pinned_generation))?;

    let deadline = Instant::now() + reader_lifetime;
    let outcome = loop {
        if cancelled.load(Ordering::Acquire) {
            break OwnedReaderExit::Cancelled;
        }
        if Instant::now() >= deadline {
            break OwnedReaderExit::DeadlineExceeded;
        }
        thread::sleep(OWNED_WORKER_POLL_INTERVAL);
    };
    transaction.commit().map_err(|error| error.to_string())?;
    send_owned_reply(exited, Ok(outcome))
}

fn run_cancellable_backup_worker(
    source_path: &Path,
    destination_path: &Path,
    cancelled: &AtomicBool,
    ready: &mpsc::SyncSender<OwnedWorkerResult<()>>,
) -> OwnedWorkerResult<BackupCancellationObservation> {
    let source = open_read_database(source_path).map_err(|error| error.to_string())?;
    let mut destination = Connection::open(destination_path).map_err(|error| error.to_string())?;
    let backup = Backup::new(&source, &mut destination).map_err(|error| error.to_string())?;
    let started_at = Instant::now();
    let deadline = started_at
        .checked_add(BACKUP_WORKER_DEADLINE)
        .ok_or_else(|| "backup worker deadline is not representable".to_owned())?;
    let first_step = backup.step(1).map_err(|error| error.to_string())?;
    if first_step == rusqlite::backup::StepResult::Done {
        return Err("backup fixture completed before cancellation".to_owned());
    }
    send_owned_reply(ready, Ok(()))?;

    let mut completed_steps = 1_u32;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Ok(BackupCancellationObservation {
                completed_steps,
                elapsed: started_at.elapsed(),
                finished_at: Instant::now(),
            });
        }
        if Instant::now() >= deadline {
            return Err("backup worker deadline exceeded".to_owned());
        }
        if completed_steps >= BACKUP_MAX_STEPS {
            return Err("backup worker step limit exceeded".to_owned());
        }
        thread::sleep(OWNED_WORKER_POLL_INTERVAL);
        let step = backup.step(1).map_err(|error| error.to_string())?;
        completed_steps += 1;
        if step == rusqlite::backup::StepResult::Done {
            return Err("backup fixture completed before cancellation".to_owned());
        }
    }
}

fn receive_owned_worker_result<T>(
    receiver: &mpsc::Receiver<OwnedWorkerResult<T>>,
) -> TestResult<T> {
    require_owned_worker_success(receiver.recv_timeout(OWNED_REPLY_TIMEOUT)?)
}

fn join_owned_worker(
    worker: thread::JoinHandle<OwnedWorkerResult<()>>,
    worker_name: &str,
) -> TestResult {
    let result = worker
        .join()
        .map_err(|_| io::Error::other(format!("{worker_name} panicked")))?;
    require_owned_worker_success(result)
}

fn wait_for_sentinel(child: &mut Child, sentinel: &Path) -> io::Result<()> {
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(5))
        .expect("short test deadline must be representable");
    loop {
        if sentinel.is_file() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "crash child exited before synchronization with {status}"
            )));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "crash child did not synchronize before the deadline",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(target_os = "linux")]
fn peak_resident_set_kib() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let value = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))?
        .split_ascii_whitespace()
        .next()?;
    value.parse().ok()
}

#[cfg(not(target_os = "linux"))]
fn peak_resident_set_kib() -> Option<u64> {
    None
}

fn benchmark_direct(
    directory: &TempDirectory,
    facts: &[Vec<u8>],
) -> TestResult<(Duration, PathBuf)> {
    let database_path = directory.join("direct-benchmark.sqlite3");
    let started = Instant::now();
    let mut connection = open_file_database(&database_path)?;
    bootstrap_workspace(&connection)?;
    stage_ready_generation(&mut connection, 1, 1, facts, 256)?;
    activate_generation(&mut connection, 1, 1)?;
    let elapsed = started.elapsed();
    drop(connection);
    Ok((elapsed, database_path))
}

fn benchmark_private_ram_first(
    directory: &TempDirectory,
    facts: &[Vec<u8>],
) -> TestResult<(Duration, PathBuf)> {
    let database_path = directory.join("memory-benchmark.sqlite3");
    let started = Instant::now();
    let mut connection = open_memory_database()?;
    bootstrap_workspace(&connection)?;
    stage_ready_generation(&mut connection, 1, 1, facts, 256)?;
    activate_generation(&mut connection, 1, 1)?;
    backup_database(&connection, &database_path)?;
    Ok((started.elapsed(), database_path))
}

fn benchmark_direct_durability_profile(
    directory: &TempDirectory,
    facts: &[Vec<u8>],
    synchronous: &str,
    batch_limit: usize,
    sample: usize,
) -> TestResult<(Duration, u64)> {
    let database_path = directory.join(&format!(
        "durability-{synchronous}-{batch_limit}-{sample}.sqlite3"
    ));
    let source_wal_path = wal_path(&database_path);
    let started_at = Instant::now();
    let mut connection = open_file_database_with_synchronous(&database_path, synchronous)?;
    bootstrap_workspace(&connection)?;
    stage_ready_generation(&mut connection, 1, 1, facts, batch_limit)?;
    activate_generation(&mut connection, 1, 1)?;
    let elapsed = started_at.elapsed();
    assert_eq!(active_generation_id(&connection)?, Some(1));
    assert_eq!(generation_facts(&connection, 1)?, facts);
    Ok((elapsed, fs::metadata(source_wal_path)?.len()))
}
