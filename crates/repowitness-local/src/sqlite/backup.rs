use std::{
    ffi::{OsStr, OsString},
    fs,
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

mod authority;
mod outcome;
#[cfg(test)]
mod tests;

use self::authority::{BackupDestinationAuthority, BackupSourceAuthority, file_identity};
pub use self::outcome::{BackupIdentityStatus, BackupMaintenanceStatus, BackupPublicationStatus};
use super::{SqliteStoreError, open_index_reader};

const MAX_BACKUP_PAGES_PER_STEP: u32 = 4_096;
const MAX_BACKUP_STEPS: u32 = 1_000_000;
const MAX_BACKUP_RETRY_DELAY: Duration = Duration::from_millis(250);
const BACKUP_OUTCOME_RESOLUTION_GRACE: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BackupStage {
    BeforePublish,
    RemoveTemporary,
    VerifySourceIdentity,
    VerifyDestinationIdentity,
    SyncDirectory,
    DeliverReceipt,
}

trait BackupFaultInjector: Send + 'static {
    fn check(&mut self, stage: BackupStage) -> Result<(), SqliteStoreError>;
}

struct NoBackupFaults;

impl BackupFaultInjector for NoBackupFaults {
    fn check(&mut self, _stage: BackupStage) -> Result<(), SqliteStoreError> {
        Ok(())
    }
}

impl<F> BackupFaultInjector for F
where
    F: FnMut(BackupStage) -> Result<(), SqliteStoreError> + Send + 'static,
{
    fn check(&mut self, stage: BackupStage) -> Result<(), SqliteStoreError> {
        self(stage)
    }
}

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
    publication_status: BackupPublicationStatus,
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

    /// Returns categorical identity and maintenance truth after publication.
    #[must_use]
    pub const fn publication_status(self) -> BackupPublicationStatus {
        self.publication_status
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
    create_online_backup_with_faults(
        source,
        destination,
        limits,
        cancelled,
        deadline,
        BACKUP_OUTCOME_RESOLUTION_GRACE,
        NoBackupFaults,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "the private seam makes post-commit and reply races deterministic in tests"
)]
fn create_online_backup_with_faults(
    source: &Path,
    destination: &Path,
    limits: BackupLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    outcome_grace: Duration,
    mut faults: impl BackupFaultInjector,
) -> Result<BackupOutcome, SqliteStoreError> {
    if Instant::now() >= deadline {
        return Err(SqliteStoreError::DeadlineExceeded);
    }
    if outcome_grace.is_zero() || outcome_grace > BACKUP_OUTCOME_RESOLUTION_GRACE {
        return Err(SqliteStoreError::InvalidBackupLimits);
    }
    if source == destination || destination.exists() {
        return Err(SqliteStoreError::BackupDestinationUnavailable);
    }
    let source = PathBuf::from(source);
    let destination = PathBuf::from(destination);
    let worker_cancelled = Arc::clone(&cancelled);
    let (reply, receiver) = mpsc::sync_channel(1);
    let _worker = thread::Builder::new()
        .name("repowitness-sqlite-backup".to_owned())
        .spawn(move || {
            let result = run_backup_with_faults(
                &source,
                &destination,
                limits,
                &worker_cancelled,
                deadline,
                &mut faults,
            );
            if faults.check(BackupStage::DeliverReceipt).is_ok() {
                let _ = reply.try_send(result);
            }
        })
        .map_err(|_| SqliteStoreError::WorkerUnavailable)?;
    receive_backup_outcome(&receiver, cancelled.as_ref(), deadline, outcome_grace)
}

fn receive_backup_outcome(
    receiver: &mpsc::Receiver<Result<BackupOutcome, SqliteStoreError>>,
    cancelled: &AtomicBool,
    deadline: Instant,
    outcome_grace: Duration,
) -> Result<BackupOutcome, SqliteStoreError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        match receiver.recv_timeout(remaining) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(SqliteStoreError::MutationOutcomeUnknown);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    cancelled.store(true, Ordering::Release);
    match receiver.recv_timeout(outcome_grace) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) | Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(SqliteStoreError::MutationOutcomeUnknown)
        }
    }
}

fn run_backup_with_faults(
    source_path: &Path,
    destination_path: &Path,
    limits: BackupLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
    faults: &mut impl BackupFaultInjector,
) -> Result<BackupOutcome, SqliteStoreError> {
    check_control(cancelled, deadline)?;
    let source_authority = BackupSourceAuthority::open(source_path)?;
    let destination_authority = BackupDestinationAuthority::open(destination_path)?;
    let destination_path = destination_authority.destination_path();
    ensure_destination_absent(&destination_path)?;
    if source_authority.path() == destination_path.as_path() {
        return Err(SqliteStoreError::BackupDestinationUnavailable);
    }
    let temporary_name = destination_authority.temporary_name();
    let mut temporary = TemporaryBackup::reserve(&destination_authority, temporary_name)?;
    let source = open_index_reader(source_authority.path())?;
    source_authority.verify_current_path()?;
    let mut destination = Connection::open_with_flags(
        temporary.path(),
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
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
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(SqliteStoreError::DeadlineExceeded);
                    }
                    thread::sleep(limits.retry_delay.min(remaining));
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
    source_authority.verify_current_path()?;
    temporary.ensure_sidecars_absent()?;
    temporary.verify_exclusive_path()?;
    validate_backup(temporary.path())?;
    check_control(cancelled, deadline)?;
    temporary.ensure_sidecars_absent()?;
    temporary.verify_exclusive_path()?;
    let source_pages =
        u32::try_from(progress.pagecount).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    faults.check(BackupStage::BeforePublish)?;
    source_authority.verify_current_path()?;
    destination_authority.publish(temporary.name())?;

    let temporary_cleanup =
        post_commit_maintenance(faults, BackupStage::RemoveTemporary, || temporary.remove());
    let source_identity = post_commit_identity(faults, BackupStage::VerifySourceIdentity, || {
        source_authority.verify_current_path()
    });
    let destination_identity =
        post_commit_identity(faults, BackupStage::VerifyDestinationIdentity, || {
            destination_authority.verify_destination(temporary.identity())
        });
    let directory_sync = post_commit_maintenance(faults, BackupStage::SyncDirectory, || {
        destination_authority.sync()
    });

    Ok(BackupOutcome {
        steps,
        source_pages,
        publication_status: BackupPublicationStatus::new(
            source_identity,
            destination_identity,
            temporary_cleanup,
            directory_sync,
        ),
    })
}

fn post_commit_identity(
    faults: &mut impl BackupFaultInjector,
    stage: BackupStage,
    operation: impl FnOnce() -> Result<(), SqliteStoreError>,
) -> BackupIdentityStatus {
    if faults.check(stage).is_ok() && operation().is_ok() {
        BackupIdentityStatus::ConfirmedAtFinalFence
    } else {
        BackupIdentityStatus::ChangedAfterCommit
    }
}

fn post_commit_maintenance(
    faults: &mut impl BackupFaultInjector,
    stage: BackupStage,
    operation: impl FnOnce() -> Result<(), SqliteStoreError>,
) -> BackupMaintenanceStatus {
    if faults.check(stage).is_ok() && operation().is_ok() {
        BackupMaintenanceStatus::Complete
    } else {
        BackupMaintenanceStatus::Deferred
    }
}

fn ensure_destination_absent(path: &Path) -> Result<(), SqliteStoreError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) | Err(_) => Err(SqliteStoreError::BackupDestinationUnavailable),
    }
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

#[cfg(test)]
fn temporary_backup_path(destination: &Path) -> Result<PathBuf, SqliteStoreError> {
    let file_name = destination
        .file_name()
        .ok_or(SqliteStoreError::BackupDestinationUnavailable)?;
    let mut temporary_name = OsString::from(file_name);
    temporary_name.push(format!(".repowitness-partial-{}", std::process::id()));
    Ok(destination.with_file_name(temporary_name))
}

struct TemporaryBackup<'authority> {
    authority: &'authority BackupDestinationAuthority,
    name: OsString,
    path: PathBuf,
    file: Option<cap_std::fs::File>,
    identity: FileIdentity,
    armed: bool,
}

impl<'authority> TemporaryBackup<'authority> {
    fn reserve(
        authority: &'authority BackupDestinationAuthority,
        name: OsString,
    ) -> Result<Self, SqliteStoreError> {
        let file = authority.create_temporary(&name)?;
        let identity =
            file_identity(&file).map_err(|_| SqliteStoreError::BackupDestinationUnavailable)?;
        let path = authority.temporary_path(&name);
        let temporary = Self {
            authority,
            name,
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

    fn name(&self) -> &OsStr {
        &self.name
    }

    fn identity(&self) -> &FileIdentity {
        &self.identity
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
        self.authority
            .verify_open_file(file, &self.identity)
            .map_err(|_| SqliteStoreError::BackupFailed)?;
        self.authority
            .verify_named_file(&self.name, &self.identity)
            .map_err(|_| SqliteStoreError::BackupFailed)
    }

    fn verify_path(&self) -> Result<(), SqliteStoreError> {
        self.authority
            .verify_named_identity(&self.name, &self.identity)
            .map_err(|_| SqliteStoreError::BackupCleanupFailed)
    }

    fn remove(&mut self) -> Result<(), SqliteStoreError> {
        self.verify_path()?;
        drop(self.file.take());
        self.authority.remove(&self.name)?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for TemporaryBackup<'_> {
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
