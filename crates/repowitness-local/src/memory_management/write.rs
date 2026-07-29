use std::{
    ffi::{OsStr, OsString},
    fs::TryLockError,
    io::{self, Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, File, Metadata, OpenOptions},
};
use repowitness_application::{MemoryRecordIdTextV1, RepositoryIdentityTextV1};
use repowitness_domain::{CanonicalMemoryDigest, MemoryRecord};

use super::{
    LocalMemoryManageError, LocalMemoryWriteInput, LocalMemoryWriteRequest, check_control,
    checked_deadline, map_file_error, map_repository_identity_error, open_worktree, secret,
};
use crate::{
    MAX_MEMORY_YAML_BYTES, MemoryFormatControl, MemoryRecordFiles, contained_source::FileIdentity,
    generate_memory_yaml, parse_memory_record,
};

mod directory;
mod outcome;
use directory::RecordsDirectoryAuthority;
pub use outcome::{
    LocalMemoryFilePublicationStatus, MemoryFileIdentityStatus, MemoryFilePublicationStepStatus,
};

const MAX_INPUT_READ_CHUNK: usize = 16 * 1024;
const TEMP_ATTEMPTS: u32 = 16;
const WRITE_LEASE_NAME: &str = ".repowitness-write.lock";
const WRITE_LEASE_RETRY_DELAY: Duration = Duration::from_millis(10);
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

#[cfg(unix)]
type DirectorySyncHandle = std::os::fd::OwnedFd;
#[cfg(not(unix))]
type DirectorySyncHandle = std::fs::File;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PublicationStage {
    OpenDirectorySync,
    CreateTemporary,
    WriteTemporary,
    SyncTemporary,
    InspectTemporary,
    PublishTarget,
    RemoveTemporary,
    VerifyTarget,
    SyncDirectory,
    VerifyRecordsDirectory,
    CleanupTemporary,
}

trait PublicationFaultInjector {
    fn check(&mut self, stage: PublicationStage) -> Result<(), LocalMemoryManageError>;
}

struct NoPublicationFaults;

impl PublicationFaultInjector for NoPublicationFaults {
    fn check(&mut self, _stage: PublicationStage) -> Result<(), LocalMemoryManageError> {
        Ok(())
    }
}

impl<F> PublicationFaultInjector for F
where
    F: FnMut(PublicationStage) -> Result<(), LocalMemoryManageError>,
{
    fn check(&mut self, stage: PublicationStage) -> Result<(), LocalMemoryManageError> {
        self(stage)
    }
}

/// Receipt for one canonical Git-memory file publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalMemoryWriteReceipt {
    revision: CanonicalMemoryDigest,
    created: bool,
    canonical_bytes: u64,
    publication_status: LocalMemoryFilePublicationStatus,
}

impl LocalMemoryWriteReceipt {
    /// Returns the exact semantic revision written to the canonical file.
    #[must_use]
    pub const fn revision(self) -> CanonicalMemoryDigest {
        self.revision
    }

    /// Reports whether this publication created a previously absent record.
    #[must_use]
    pub const fn created(self) -> bool {
        self.created
    }

    /// Returns the exact deterministic YAML byte count.
    #[must_use]
    pub const fn canonical_bytes(self) -> u64 {
        self.canonical_bytes
    }

    /// Returns categorical truth about work attempted after atomic publication.
    #[must_use]
    pub const fn publication_status(self) -> LocalMemoryFilePublicationStatus {
        self.publication_status
    }
}

pub(super) fn write(
    request: LocalMemoryWriteRequest<'_>,
    cancelled: std::sync::Arc<AtomicBool>,
) -> Result<LocalMemoryWriteReceipt, LocalMemoryManageError> {
    write_with_faults(request, cancelled, &mut NoPublicationFaults)
}

fn write_with_faults(
    request: LocalMemoryWriteRequest<'_>,
    cancelled: std::sync::Arc<AtomicBool>,
    faults: &mut impl PublicationFaultInjector,
) -> Result<LocalMemoryWriteReceipt, LocalMemoryManageError> {
    let deadline = checked_deadline(request.deadline)?;
    check_control(cancelled.as_ref(), deadline)?;
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(map_repository_identity_error)?;
    let worktree = open_worktree(request.repository_root)?;
    let parsed = load_input(request.input, cancelled.as_ref(), deadline)?;
    if parsed.record().scope().repository() != repository {
        return Err(LocalMemoryManageError::ScopeMismatch);
    }
    secret::check_record(parsed.record())?;
    let canonical = generate_memory_yaml(
        parsed.record(),
        MemoryFormatControl::new(cancelled.as_ref(), deadline),
    )
    .map_err(|_| LocalMemoryManageError::InputUnavailable)?;
    let verified = parse_memory_record(
        &canonical,
        MemoryFormatControl::new(cancelled.as_ref(), deadline),
    )
    .map_err(|_| LocalMemoryManageError::InputUnavailable)?;
    if verified.digest() != parsed.digest() {
        return Err(LocalMemoryManageError::InputUnavailable);
    }
    let record_id = parsed.record().header().record_id();
    let (records, _write_lease) = open_records_directory(&worktree, cancelled.as_ref(), deadline)?;
    let current = current_record(&worktree, record_id, cancelled.as_ref(), deadline)?;
    let created = validate_update(parsed.record(), current.as_ref())?;
    let target = target_name(record_id);
    let canonical_bytes = u64::try_from(canonical.len())
        .map_err(|_| LocalMemoryManageError::CountNotRepresentable)?;
    let publication_status = publish_with_faults(
        &records,
        &worktree,
        &target,
        &canonical,
        current.as_ref(),
        cancelled.as_ref(),
        deadline,
        faults,
    )?;
    Ok(LocalMemoryWriteReceipt {
        revision: parsed.digest(),
        created,
        canonical_bytes,
        publication_status,
    })
}

struct CurrentRecord {
    revision: CanonicalMemoryDigest,
    display_revision: u32,
    presentation: repowitness_domain::MemoryPresentationDigest,
}

fn load_input(
    input: LocalMemoryWriteInput<'_>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<crate::ParsedMemoryRecord, LocalMemoryManageError> {
    match input {
        LocalMemoryWriteInput::File(input) => load_file_input(input, cancelled, deadline),
        LocalMemoryWriteInput::Bytes(input) => load_byte_input(input, cancelled, deadline),
    }
}

fn load_byte_input(
    input: &[u8],
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<crate::ParsedMemoryRecord, LocalMemoryManageError> {
    check_control(cancelled, deadline)?;
    if input.len() > MAX_MEMORY_YAML_BYTES {
        return Err(LocalMemoryManageError::InputUnavailable);
    }
    parse_memory_record(input, MemoryFormatControl::new(cancelled, deadline))
        .map_err(|_| LocalMemoryManageError::InputUnavailable)
}

fn load_file_input(
    input: &Path,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<crate::ParsedMemoryRecord, LocalMemoryManageError> {
    check_control(cancelled, deadline)?;
    let parent = input
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = input
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or(LocalMemoryManageError::InputUnavailable)?;
    let directory = Dir::open_ambient_dir(parent, ambient_authority())
        .map_err(|_| LocalMemoryManageError::InputUnavailable)?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = directory
        .open_with(name, &options)
        .map_err(|_| LocalMemoryManageError::InputUnavailable)?;
    let metadata = file
        .metadata()
        .map_err(|_| LocalMemoryManageError::InputUnavailable)?;
    if !metadata.is_file() || !has_one_link(&metadata) {
        return Err(LocalMemoryManageError::InputUnavailable);
    }
    let mut bytes = Vec::with_capacity(MAX_INPUT_READ_CHUNK);
    Read::by_ref(&mut file)
        .take(
            u64::try_from(MAX_MEMORY_YAML_BYTES)
                .map_err(|_| LocalMemoryManageError::CountNotRepresentable)?
                .saturating_add(1),
        )
        .read_to_end(&mut bytes)
        .map_err(|_| LocalMemoryManageError::InputUnavailable)?;
    if bytes.len() > MAX_MEMORY_YAML_BYTES {
        return Err(LocalMemoryManageError::InputUnavailable);
    }
    check_control(cancelled, deadline)?;
    parse_memory_record(&bytes, MemoryFormatControl::new(cancelled, deadline))
        .map_err(|_| LocalMemoryManageError::InputUnavailable)
}

fn open_records_directory(
    worktree: &Path,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(RecordsDirectoryAuthority, std::fs::File), LocalMemoryManageError> {
    let root = Dir::open_ambient_dir(worktree, ambient_authority())
        .map_err(|_| LocalMemoryManageError::RepositoryUnavailable)?;
    let memory = open_or_create_directory(&root, OsStr::new(".code-memory"))?;
    let lease = acquire_write_lease(&memory, cancelled, deadline)?;
    let records = open_or_create_directory(&memory, OsStr::new("records"))?;
    let records = RecordsDirectoryAuthority::new(records, worktree)?;
    Ok((records, lease))
}

fn open_or_create_directory(parent: &Dir, name: &OsStr) -> Result<Dir, LocalMemoryManageError> {
    match parent.create_dir(name) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(LocalMemoryManageError::FilePublicationFailed),
    }
    parent
        .open_dir_nofollow(name)
        .map_err(|_| LocalMemoryManageError::FilePublicationFailed)
}

fn acquire_write_lease(
    memory: &Dir,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<std::fs::File, LocalMemoryManageError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .follow(FollowSymlinks::No);
    let lease = memory
        .open_with(WRITE_LEASE_NAME, &options)
        .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
    let metadata = lease
        .metadata()
        .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
    if !metadata.is_file() || !has_one_link(&metadata) {
        return Err(LocalMemoryManageError::FilePublicationFailed);
    }
    let lease = lease.into_std();
    loop {
        check_control(cancelled, deadline)?;
        match lease.try_lock() {
            Ok(()) => return Ok(lease),
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(_)) => {
                return Err(LocalMemoryManageError::FilePublicationFailed);
            }
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(LocalMemoryManageError::DeadlineExceeded);
        }
        thread::sleep(WRITE_LEASE_RETRY_DELAY.min(deadline.duration_since(now)));
    }
}

fn current_record(
    worktree: &Path,
    record_id: repowitness_domain::MemoryRecordId,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Option<CurrentRecord>, LocalMemoryManageError> {
    let files = MemoryRecordFiles::open(worktree).map_err(map_file_error)?;
    match files.load(record_id, cancelled, deadline) {
        Ok(loaded) => Ok(Some(CurrentRecord {
            revision: loaded.revision(),
            display_revision: loaded.record().header().display_revision().get(),
            presentation: loaded.presentation(),
        })),
        Err(crate::MemoryFileImportError::FileUnavailable) => Ok(None),
        Err(error) => Err(map_file_error(error)),
    }
}

fn validate_update(
    proposed: &MemoryRecord,
    current: Option<&CurrentRecord>,
) -> Result<bool, LocalMemoryManageError> {
    let parents = proposed.header().parents();
    match current {
        None if parents.is_empty() && proposed.header().display_revision().get() == 1 => Ok(true),
        None => Err(LocalMemoryManageError::WriteConflict),
        Some(_) if parents.len() > 1 => Err(LocalMemoryManageError::MergeUnsupported),
        Some(current)
            if parents.len() == 1
                && parents[0] == current.revision
                && proposed.header().display_revision().get()
                    == current
                        .display_revision
                        .checked_add(1)
                        .ok_or(LocalMemoryManageError::WriteConflict)? =>
        {
            Ok(false)
        }
        Some(_) => Err(LocalMemoryManageError::WriteConflict),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the private publication seam keeps every fault stage deterministic without widening public APIs"
)]
fn publish_with_faults(
    records_authority: &RecordsDirectoryAuthority,
    worktree: &Path,
    target: &OsStr,
    canonical: &[u8],
    current: Option<&CurrentRecord>,
    cancelled: &AtomicBool,
    deadline: Instant,
    faults: &mut impl PublicationFaultInjector,
) -> Result<LocalMemoryFilePublicationStatus, LocalMemoryManageError> {
    let records = records_authority.directory();
    check_control(cancelled, deadline)?;
    faults.check(PublicationStage::OpenDirectorySync)?;
    let directory_sync = open_directory_sync_handle(records)?;
    faults.check(PublicationStage::CreateTemporary)?;
    let (temporary, mut file) = create_temporary(records)?;
    let publication = (|| {
        faults.check(PublicationStage::WriteTemporary)?;
        file.write_all(canonical)
            .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
        faults.check(PublicationStage::SyncTemporary)?;
        file.sync_all()
            .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
        faults.check(PublicationStage::InspectTemporary)?;
        let metadata = file
            .metadata()
            .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
        if !metadata.is_file() || !has_one_link(&metadata) {
            return Err(LocalMemoryManageError::FilePublicationFailed);
        }
        let identity = FileIdentity::from_file(
            file.try_clone()
                .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?
                .into_std(),
        )
        .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
        check_control(cancelled, deadline)?;
        match current {
            None => {
                faults.check(PublicationStage::PublishTarget)?;
                records_authority.verify_current_path()?;
                records
                    .hard_link(&temporary, records, target)
                    .map_err(|error| {
                        if error.kind() == io::ErrorKind::AlreadyExists {
                            LocalMemoryManageError::WriteConflict
                        } else {
                            LocalMemoryManageError::FilePublicationFailed
                        }
                    })?;
            }
            Some(expected) => {
                revalidate_current(worktree, target, expected, cancelled, deadline)?;
                faults.check(PublicationStage::PublishTarget)?;
                records_authority.verify_current_path()?;
                records
                    .rename(&temporary, records, target)
                    .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
            }
        }
        Ok(identity)
    })();
    let identity = match publication {
        Ok(identity) => identity,
        Err(error) => {
            if faults.check(PublicationStage::CleanupTemporary).is_ok() {
                let _ = records.remove_file(&temporary);
            }
            return Err(error);
        }
    };

    let temporary_cleanup = if current.is_none() {
        post_commit_step(faults, PublicationStage::RemoveTemporary, || {
            check_control(cancelled, deadline)?;
            records
                .remove_file(&temporary)
                .map_err(|_| LocalMemoryManageError::FilePublicationFailed)
        })
    } else {
        MemoryFilePublicationStepStatus::NotRequired
    };
    let target_identity = if faults.check(PublicationStage::VerifyTarget).is_ok()
        && verify_published_target(records, target, &identity, canonical, cancelled, deadline)
            .is_ok()
    {
        MemoryFileIdentityStatus::ConfirmedAtFinalFence
    } else {
        MemoryFileIdentityStatus::ChangedAfterCommit
    };
    let directory_sync = post_commit_step(faults, PublicationStage::SyncDirectory, || {
        check_control(cancelled, deadline)?;
        sync_directory(&directory_sync)
    });
    let records_directory_identity = if faults
        .check(PublicationStage::VerifyRecordsDirectory)
        .is_ok()
        && records_authority.verify_current_path().is_ok()
    {
        MemoryFileIdentityStatus::ConfirmedAtFinalFence
    } else {
        MemoryFileIdentityStatus::ChangedAfterCommit
    };

    Ok(LocalMemoryFilePublicationStatus::new(
        temporary_cleanup,
        target_identity,
        records_directory_identity,
        directory_sync,
    ))
}

fn post_commit_step(
    faults: &mut impl PublicationFaultInjector,
    stage: PublicationStage,
    operation: impl FnOnce() -> Result<(), LocalMemoryManageError>,
) -> MemoryFilePublicationStepStatus {
    if faults.check(stage).is_ok() && operation().is_ok() {
        MemoryFilePublicationStepStatus::Complete
    } else {
        MemoryFilePublicationStepStatus::Deferred
    }
}

fn create_temporary(records: &Dir) -> Result<(OsString, File), LocalMemoryManageError> {
    for _ in 0..TEMP_ATTEMPTS {
        let ordinal = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let name = OsString::from(format!(
            ".repowitness-write-{}-{ordinal}.tmp",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        match records.open_with(&name, &options) {
            Ok(file) => return Ok((name, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(LocalMemoryManageError::FilePublicationFailed),
        }
    }
    Err(LocalMemoryManageError::FilePublicationFailed)
}

fn revalidate_current(
    worktree: &Path,
    target: &OsStr,
    expected: &CurrentRecord,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalMemoryManageError> {
    let name = target
        .to_str()
        .and_then(|value| value.strip_suffix(".yaml"))
        .ok_or(LocalMemoryManageError::WriteConflict)?;
    let record_id =
        MemoryRecordIdTextV1::decode(name).map_err(|_| LocalMemoryManageError::WriteConflict)?;
    let loaded = MemoryRecordFiles::open(worktree)
        .map_err(map_file_error)?
        .load(record_id, cancelled, deadline)
        .map_err(map_file_error)?;
    if loaded.revision() != expected.revision
        || loaded.presentation() != expected.presentation
        || loaded.record().header().display_revision().get() != expected.display_revision
    {
        return Err(LocalMemoryManageError::WriteConflict);
    }
    Ok(())
}

fn target_name(record_id: repowitness_domain::MemoryRecordId) -> OsString {
    OsString::from(format!(
        "{}.yaml",
        MemoryRecordIdTextV1::encode(record_id).as_str()
    ))
}

fn verify_published_target(
    records: &Dir,
    target: &OsStr,
    expected_identity: &FileIdentity,
    expected_bytes: &[u8],
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalMemoryManageError> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = records
        .open_with(target, &options)
        .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
    let metadata = file
        .metadata()
        .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
    let expected_len = u64::try_from(expected_bytes.len())
        .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
    if !metadata.is_file() || !has_one_link(&metadata) || metadata.len() != expected_len {
        return Err(LocalMemoryManageError::FilePublicationFailed);
    }
    let identity = FileIdentity::from_file(
        file.try_clone()
            .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?
            .into_std(),
    )
    .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
    if &identity != expected_identity {
        return Err(LocalMemoryManageError::FilePublicationFailed);
    }

    let mut buffer = [0_u8; MAX_INPUT_READ_CHUNK];
    for expected in expected_bytes.chunks(MAX_INPUT_READ_CHUNK) {
        check_control(cancelled, deadline)?;
        file.read_exact(&mut buffer[..expected.len()])
            .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
        if &buffer[..expected.len()] != expected {
            return Err(LocalMemoryManageError::FilePublicationFailed);
        }
    }
    let mut trailing = [0_u8; 1];
    if file
        .read(&mut trailing)
        .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?
        != 0
    {
        return Err(LocalMemoryManageError::FilePublicationFailed);
    }
    check_control(cancelled, deadline)?;

    let final_metadata = file
        .metadata()
        .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
    if !final_metadata.is_file()
        || !has_one_link(&final_metadata)
        || final_metadata.len() != expected_len
    {
        return Err(LocalMemoryManageError::FilePublicationFailed);
    }
    let final_identity = FileIdentity::from_file(file.into_std())
        .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
    if &final_identity != expected_identity {
        return Err(LocalMemoryManageError::FilePublicationFailed);
    }
    Ok(())
}

#[cfg(unix)]
fn open_directory_sync_handle(
    directory: &Dir,
) -> Result<DirectorySyncHandle, LocalMemoryManageError> {
    rustix::fs::openat(
        directory,
        ".",
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| LocalMemoryManageError::FilePublicationFailed)
}

#[cfg(not(unix))]
fn open_directory_sync_handle(
    directory: &Dir,
) -> Result<DirectorySyncHandle, LocalMemoryManageError> {
    directory
        .try_clone()
        .map(Dir::into_std_file)
        .map_err(|_| LocalMemoryManageError::FilePublicationFailed)
}

#[cfg(unix)]
fn sync_directory(directory: &DirectorySyncHandle) -> Result<(), LocalMemoryManageError> {
    rustix::fs::fsync(directory).map_err(|_| LocalMemoryManageError::FilePublicationFailed)
}

#[cfg(not(unix))]
fn sync_directory(directory: &DirectorySyncHandle) -> Result<(), LocalMemoryManageError> {
    directory
        .sync_all()
        .map_err(|_| LocalMemoryManageError::FilePublicationFailed)
}

#[cfg(unix)]
fn has_one_link(metadata: &Metadata) -> bool {
    use cap_std::fs::MetadataExt as _;
    metadata.nlink() == 1
}

#[cfg(windows)]
fn has_one_link(metadata: &Metadata) -> bool {
    use cap_std::fs::MetadataExt as _;
    metadata.number_of_links() == Some(1)
}

#[cfg(not(any(unix, windows)))]
fn has_one_link(_metadata: &Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests;
