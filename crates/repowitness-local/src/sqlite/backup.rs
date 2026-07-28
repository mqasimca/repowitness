use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
};

use crate::contained_source::FileIdentity;

use super::{SqliteStoreError, open_index_reader, validate_database_file};

#[cfg(test)]
mod tests;

const MAX_BACKUP_PAGES_PER_STEP: u32 = 4_096;
const MAX_BACKUP_STEPS: u32 = 1_000_000;
const MAX_BACKUP_RETRY_DELAY: Duration = Duration::from_millis(250);

/// Page, step, and contention limits for one online backup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupLimits {
    pages_per_step: u32,
    max_steps: u32,
    retry_delay: Duration,
}

impl BackupLimits {
    /// Constructs limits no larger than the Phase 0 backup ceilings.
    pub fn try_new(
        pages_per_step: u32,
        max_steps: u32,
        retry_delay: Duration,
    ) -> Result<Self, SqliteStoreError> {
        if pages_per_step == 0
            || pages_per_step > MAX_BACKUP_PAGES_PER_STEP
            || max_steps == 0
            || max_steps > MAX_BACKUP_STEPS
            || retry_delay > MAX_BACKUP_RETRY_DELAY
        {
            return Err(SqliteStoreError::InvalidBackupLimits);
        }
        Ok(Self {
            pages_per_step,
            max_steps,
            retry_delay,
        })
    }
}

impl Default for BackupLimits {
    fn default() -> Self {
        Self {
            pages_per_step: 128,
            max_steps: 100_000,
            retry_delay: Duration::from_millis(10),
        }
    }
}

/// Bounded facts about one completed and published online backup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupOutcome {
    steps: u32,
    source_pages: u32,
}

impl BackupOutcome {
    /// Returns the number of bounded backup steps.
    #[must_use]
    pub const fn steps(self) -> u32 {
        self.steps
    }

    /// Returns SQLite's final observed source page count.
    #[must_use]
    pub const fn source_pages(self) -> u32 {
        self.source_pages
    }
}

/// Creates a validated no-clobber backup on a separately owned thread.
pub fn create_online_backup(
    source: &Path,
    destination: &Path,
    limits: BackupLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<BackupOutcome, SqliteStoreError> {
    if Instant::now() >= deadline {
        return Err(SqliteStoreError::DeadlineExceeded);
    }
    if source == destination || destination.exists() {
        return Err(SqliteStoreError::BackupDestinationUnavailable);
    }
    let source = PathBuf::from(source);
    let destination = PathBuf::from(destination);
    let worker_cancelled = Arc::clone(&cancelled);
    let (reply, receiver) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("repowitness-sqlite-backup".to_owned())
        .spawn(move || {
            let result = run_backup(&source, &destination, limits, &worker_cancelled, deadline);
            let _ = reply.try_send(result);
        })
        .map_err(|_| SqliteStoreError::WorkerUnavailable)?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    let result = if remaining.is_zero() {
        Err(SqliteStoreError::DeadlineExceeded)
    } else {
        match receiver.recv_timeout(remaining) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(SqliteStoreError::ReplyTimeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(SqliteStoreError::WorkerUnavailable),
        }
    };
    if result.is_err() {
        cancelled.store(true, Ordering::Release);
    }
    worker
        .join()
        .map_err(|_| SqliteStoreError::WorkerPanicked)?;
    result
}

fn run_backup(
    source_path: &Path,
    destination_path: &Path,
    limits: BackupLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<BackupOutcome, SqliteStoreError> {
    check_control(cancelled, deadline)?;
    let temporary_path = temporary_backup_path(destination_path)?;
    let mut temporary = TemporaryBackup::reserve(temporary_path)?;
    let source = open_index_reader(source_path)?;
    let mut destination = Connection::open_with_flags(
        temporary.path(),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| SqliteStoreError::BackupFailed)?;
    let journal_mode: String = destination
        .pragma_update_and_check(None, "journal_mode", "MEMORY", |row| row.get(0))
        .map_err(|_| SqliteStoreError::BackupFailed)?;
    if journal_mode != "memory" {
        return Err(SqliteStoreError::BackupFailed);
    }
    let backup =
        Backup::new(&source, &mut destination).map_err(|_| SqliteStoreError::BackupFailed)?;
    let pages_per_step =
        i32::try_from(limits.pages_per_step).map_err(|_| SqliteStoreError::InvalidBackupLimits)?;
    let mut steps = 0_u32;
    loop {
        check_control(cancelled, deadline)?;
        if steps >= limits.max_steps {
            return Err(SqliteStoreError::BackupStepLimitExceeded);
        }
        let step = backup
            .step(pages_per_step)
            .map_err(|_| SqliteStoreError::BackupFailed)?;
        steps += 1;
        match step {
            StepResult::Done => break,
            StepResult::More => {}
            StepResult::Busy | StepResult::Locked => {
                if !limits.retry_delay.is_zero() {
                    thread::sleep(limits.retry_delay);
                }
            }
            _ => return Err(SqliteStoreError::BackupFailed),
        }
    }
    let progress = backup.progress();
    drop(backup);
    let journal_mode: String = destination
        .pragma_update_and_check(None, "journal_mode", "DELETE", |row| row.get(0))
        .map_err(|_| SqliteStoreError::BackupFailed)?;
    if journal_mode != "delete" {
        return Err(SqliteStoreError::BackupFailed);
    }
    drop(destination);
    drop(source);
    check_control(cancelled, deadline)?;
    temporary.ensure_sidecars_absent()?;
    temporary.verify_exclusive_path()?;
    validate_backup(temporary.path())?;
    check_control(cancelled, deadline)?;
    temporary.ensure_sidecars_absent()?;
    temporary.verify_exclusive_path()?;
    fs::hard_link(temporary.path(), destination_path)
        .map_err(|_| SqliteStoreError::BackupDestinationUnavailable)?;
    temporary
        .remove()
        .map_err(|_| SqliteStoreError::BackupCleanupFailed)?;
    Ok(BackupOutcome {
        steps,
        source_pages: u32::try_from(progress.pagecount)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
    })
}

fn validate_backup(path: &Path) -> Result<(), SqliteStoreError> {
    let connection = open_index_reader(path)?;
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|_| SqliteStoreError::BackupFailed)?;
    let foreign_key_violations: i64 = connection
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .map_err(|_| SqliteStoreError::BackupFailed)?;
    let pointer_mismatches: i64 = connection
        .query_row(
            "SELECT count(*) FROM workspaces AS workspace
             LEFT JOIN index_generations AS generation
               ON generation.generation_id = workspace.active_generation_id
             WHERE workspace.active_generation_id IS NOT NULL
               AND (
                    generation.generation_id IS NULL
                    OR generation.workspace_id != workspace.workspace_id
                    OR generation.lifecycle_state != 'active'
               )",
            [],
            |row| row.get(0),
        )
        .map_err(|_| SqliteStoreError::BackupFailed)?;
    if integrity != "ok" || foreign_key_violations != 0 || pointer_mismatches != 0 {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    Ok(())
}

fn temporary_backup_path(destination: &Path) -> Result<PathBuf, SqliteStoreError> {
    let file_name = destination
        .file_name()
        .ok_or(SqliteStoreError::BackupDestinationUnavailable)?;
    let mut temporary_name = OsString::from(file_name);
    temporary_name.push(format!(".repowitness-partial-{}", std::process::id()));
    Ok(destination.with_file_name(temporary_name))
}

struct TemporaryBackup {
    path: PathBuf,
    file: Option<File>,
    identity: FileIdentity,
    armed: bool,
}

impl TemporaryBackup {
    fn reserve(path: PathBuf) -> Result<Self, SqliteStoreError> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| SqliteStoreError::BackupDestinationUnavailable)?;
        validate_database_file(&file)
            .map_err(|_| SqliteStoreError::BackupDestinationUnavailable)?;
        let identity = FileIdentity::from_file(
            file.try_clone()
                .map_err(|_| SqliteStoreError::BackupDestinationUnavailable)?,
        )
        .map_err(|_| SqliteStoreError::BackupDestinationUnavailable)?;
        let temporary = Self {
            path,
            file: Some(file),
            identity,
            armed: true,
        };
        temporary.ensure_sidecars_absent()?;
        Ok(temporary)
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn ensure_sidecars_absent(&self) -> Result<(), SqliteStoreError> {
        for suffix in ["-journal", "-wal", "-shm"] {
            match fs::symlink_metadata(path_with_suffix(&self.path, suffix)) {
                Ok(_) => return Err(SqliteStoreError::BackupDestinationUnavailable),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(SqliteStoreError::BackupDestinationUnavailable),
            }
        }
        Ok(())
    }

    fn verify_exclusive_path(&self) -> Result<(), SqliteStoreError> {
        let file = self.file.as_ref().ok_or(SqliteStoreError::BackupFailed)?;
        validate_database_file(file).map_err(|_| SqliteStoreError::BackupFailed)?;
        self.verify_path()
    }

    fn verify_path(&self) -> Result<(), SqliteStoreError> {
        let current = FileIdentity::from_path(&self.path)
            .map_err(|_| SqliteStoreError::BackupCleanupFailed)?;
        if current != self.identity {
            return Err(SqliteStoreError::BackupCleanupFailed);
        }
        Ok(())
    }

    fn remove(&mut self) -> Result<(), SqliteStoreError> {
        self.verify_path()?;
        drop(self.file.take());
        fs::remove_file(&self.path).map_err(|_| SqliteStoreError::BackupCleanupFailed)?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for TemporaryBackup {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.remove();
        }
    }
}

fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), SqliteStoreError> {
    if cancelled.load(Ordering::Acquire) {
        Err(SqliteStoreError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(SqliteStoreError::DeadlineExceeded)
    } else {
        Ok(())
    }
}
