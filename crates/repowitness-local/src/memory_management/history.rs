use std::{
    process::Command,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use repowitness_application::{
    MemoryImportApproval, MemoryRecordIdTextV1, RepositoryIdentityTextV1, ResolvedConfiguration,
};
use repowitness_domain::{
    MemoryAuditActorId, MemoryCommitId, MemoryObjectFormat, MemoryObservationSource,
    MemoryPresentationDigest, MemoryRecord, MemoryRecordedAtUnixMillis, RepositoryPathLimits,
};
use sha2::{Digest, Sha256};

use super::{
    LocalMemoryManageError, check_control, check_memory_write_policy, checked_deadline,
    map_repository_identity_error, map_store_error, open_store, open_worktree, secret,
};
use crate::{
    GitPathDiscoveryLimits, MAX_MEMORY_YAML_BYTES, MemoryFormatControl,
    git_paths::{capture_git_output_from_command, sanitized_git_base_command},
    parse_memory_record,
};

mod error_mapping;
mod git;

use error_mapping::{map_git_error, map_memory_query_error};

const MEMORY_PATH_PREFIX: &[u8] = b".code-memory/records/";
const YAML_SUFFIX: &[u8] = b".yaml";
const DEFAULT_HISTORY_COMMITS: u16 = 256;
const DEFAULT_HISTORY_RECORDS: u32 = 4_096;
const DEFAULT_HISTORY_BYTES: u64 = 64 * 1024 * 1024;
const DEFAULT_GIT_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
const DEFAULT_GIT_COMMAND_DEADLINE: Duration = Duration::from_secs(5);
const MEMORY_QUERY_OUTPUT_BYTES: u64 = 1024 * 1024;
const MAX_HISTORY_COMMITS: u16 = 4_096;
const MAX_HISTORY_RECORDS: u32 = 65_536;
const MAX_HISTORY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_GIT_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_GIT_COMMAND_DEADLINE: Duration = Duration::from_secs(30);
const GIT_PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(1_048_576, 65_535);

/// Explicit bounds for one reachable Git-memory history import.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalMemoryHistoryImportLimits {
    deadline: Duration,
    git_command_deadline: Duration,
    max_commits: u16,
    max_records: u32,
    max_total_bytes: u64,
    max_git_output_bytes: u64,
}

impl LocalMemoryHistoryImportLimits {
    /// Validates all independent history traversal and byte bounds.
    pub fn try_new(
        deadline: Duration,
        git_command_deadline: Duration,
        max_commits: u16,
        max_records: u32,
        max_total_bytes: u64,
        max_git_output_bytes: u64,
    ) -> Result<Self, LocalMemoryManageError> {
        if deadline.is_zero()
            || git_command_deadline.is_zero()
            || git_command_deadline > MAX_GIT_COMMAND_DEADLINE
            || max_commits == 0
            || max_commits > MAX_HISTORY_COMMITS
            || max_records == 0
            || max_records > MAX_HISTORY_RECORDS
            || max_total_bytes == 0
            || max_total_bytes > MAX_HISTORY_BYTES
            || max_git_output_bytes == 0
            || max_git_output_bytes > MAX_GIT_OUTPUT_BYTES
        {
            return Err(LocalMemoryManageError::InvalidLimits);
        }
        Ok(Self {
            deadline,
            git_command_deadline,
            max_commits,
            max_records,
            max_total_bytes,
            max_git_output_bytes,
        })
    }
}

impl Default for LocalMemoryHistoryImportLimits {
    fn default() -> Self {
        Self {
            deadline: super::DEFAULT_LOCAL_MEMORY_MANAGE_DEADLINE,
            git_command_deadline: DEFAULT_GIT_COMMAND_DEADLINE,
            max_commits: DEFAULT_HISTORY_COMMITS,
            max_records: DEFAULT_HISTORY_RECORDS,
            max_total_bytes: DEFAULT_HISTORY_BYTES,
            max_git_output_bytes: DEFAULT_GIT_OUTPUT_BYTES,
        }
    }
}

/// Complete input for one observation-only reachable-history import.
#[derive(Clone, Copy)]
pub struct LocalMemoryHistoryImportRequest<'a> {
    repository_root: &'a std::path::Path,
    database: &'a std::path::Path,
    repository_identity: &'a str,
    actor: &'a str,
    migration_applied_at_unix_ms: u64,
    recorded_at_unix_ms: u64,
    limits: LocalMemoryHistoryImportLimits,
    configuration: Option<&'a ResolvedConfiguration>,
}

impl<'a> LocalMemoryHistoryImportRequest<'a> {
    /// Constructs an import of the bounded history reachable from concrete HEAD.
    #[must_use]
    pub const fn new(
        repository_root: &'a std::path::Path,
        database: &'a std::path::Path,
        repository_identity: &'a str,
        actor: &'a str,
        migration_applied_at_unix_ms: u64,
        recorded_at_unix_ms: u64,
    ) -> Self {
        Self {
            repository_root,
            database,
            repository_identity,
            actor,
            migration_applied_at_unix_ms,
            recorded_at_unix_ms,
            limits: LocalMemoryHistoryImportLimits {
                deadline: super::DEFAULT_LOCAL_MEMORY_MANAGE_DEADLINE,
                git_command_deadline: DEFAULT_GIT_COMMAND_DEADLINE,
                max_commits: DEFAULT_HISTORY_COMMITS,
                max_records: DEFAULT_HISTORY_RECORDS,
                max_total_bytes: DEFAULT_HISTORY_BYTES,
                max_git_output_bytes: DEFAULT_GIT_OUTPUT_BYTES,
            },
            configuration: None,
        }
    }

    /// Replaces every history-import resource bound.
    #[must_use]
    pub const fn with_limits(mut self, limits: LocalMemoryHistoryImportLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Applies resolved memory-mutation policy to this request.
    #[must_use]
    pub const fn with_configuration(mut self, configuration: &'a ResolvedConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }

    /// Replaces only the end-to-end operation deadline.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.limits.deadline = deadline;
        self
    }
}

impl std::fmt::Debug for LocalMemoryHistoryImportRequest<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalMemoryHistoryImportRequest")
            .field("limits", &self.limits)
            .field(
                "configuration_digest",
                &self.configuration.map(ResolvedConfiguration::digest),
            )
            .finish_non_exhaustive()
    }
}

/// Redacted exact counts from one observation-only history import.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalMemoryHistoryImportReport {
    commits_inspected: u32,
    records_inspected: u32,
    imported_versions: u32,
    appended_observations: u32,
    total_record_bytes: u64,
    git_processes: u32,
    history_complete: bool,
}

impl LocalMemoryHistoryImportReport {
    /// Returns the number of admitted commits containing memory paths.
    #[must_use]
    pub const fn commits_inspected(self) -> u32 {
        self.commits_inspected
    }

    /// Returns the exact number of record blobs admitted before persistence.
    #[must_use]
    pub const fn records_inspected(self) -> u32 {
        self.records_inspected
    }

    /// Returns the number of newly inserted immutable semantic versions.
    #[must_use]
    pub const fn imported_versions(self) -> u32 {
        self.imported_versions
    }

    /// Returns the number of newly appended commit observations.
    #[must_use]
    pub const fn appended_observations(self) -> u32 {
        self.appended_observations
    }

    /// Returns the total exact YAML bytes inspected.
    #[must_use]
    pub const fn total_record_bytes(self) -> u64 {
        self.total_record_bytes
    }

    /// Returns the exact number of sanitized Git subprocesses.
    #[must_use]
    pub const fn git_processes(self) -> u32 {
        self.git_processes
    }

    /// Reports whether the reachable memory-changing commit set fit the bound.
    #[must_use]
    pub const fn history_complete(self) -> bool {
        self.history_complete
    }
}

struct HistoryObservation {
    commit: MemoryCommitId,
    record: MemoryRecord,
    presentation: MemoryPresentationDigest,
}

struct PreparedHistory {
    observations: Vec<HistoryObservation>,
    commits_inspected: u32,
    total_record_bytes: u64,
    git_processes: u32,
    history_complete: bool,
}

/// Imports bounded reachable Git-tree records as unapproved observations.
pub fn import_local_memory_history(
    request: LocalMemoryHistoryImportRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalMemoryHistoryImportReport, LocalMemoryManageError> {
    check_memory_write_policy(request.configuration)?;
    validate_limits(request.limits)?;
    let deadline = checked_deadline(request.limits.deadline)?;
    check_control(cancelled.as_ref(), deadline)?;
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(map_repository_identity_error)?;
    let actor = MemoryAuditActorId::try_new(request.actor.to_owned())
        .map_err(|_| LocalMemoryManageError::ActorInvalid)?;
    let recorded_at = MemoryRecordedAtUnixMillis::try_new(request.recorded_at_unix_ms)
        .map_err(|_| LocalMemoryManageError::InvalidLimits)?;
    let worktree = open_worktree(request.repository_root)?;
    let prepared = prepare_history(
        &worktree,
        repository,
        request.limits,
        cancelled.as_ref(),
        deadline,
    )?;
    let store = open_store(
        &worktree,
        request.database,
        request.migration_applied_at_unix_ms,
        Arc::clone(&cancelled),
        deadline,
    )?;
    store
        .register_workspace(repository, 0, deadline)
        .map_err(map_store_error)?;
    let operation = persist_history(
        &store,
        prepared,
        repository,
        actor,
        recorded_at,
        &cancelled,
        deadline,
    );
    finish_history(store, operation, deadline)
}

fn validate_limits(limits: LocalMemoryHistoryImportLimits) -> Result<(), LocalMemoryManageError> {
    LocalMemoryHistoryImportLimits::try_new(
        limits.deadline,
        limits.git_command_deadline,
        limits.max_commits,
        limits.max_records,
        limits.max_total_bytes,
        limits.max_git_output_bytes,
    )
    .map(|_| ())
}

fn prepare_history(
    worktree: &std::path::Path,
    repository: repowitness_domain::RepositoryIdentityDigest,
    limits: LocalMemoryHistoryImportLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<PreparedHistory, LocalMemoryManageError> {
    let queries = crate::GitMemoryQueries::open(
        worktree,
        crate::GitMemoryQueryLimits::try_new(
            limits.git_command_deadline,
            MEMORY_QUERY_OUTPUT_BYTES,
            u32::from(limits.max_commits).saturating_add(1),
        )
        .map_err(|_| LocalMemoryManageError::InvalidLimits)?,
        cancelled,
        deadline,
    )
    .map_err(map_memory_query_error)?;
    let head = queries
        .head_commit(cancelled, deadline)
        .map_err(map_memory_query_error)?
        .ok_or(LocalMemoryManageError::HistoryUnavailable)?;
    let mut git_processes = 2_u32;
    let shallow = git::repository_is_shallow(worktree, limits, cancelled, deadline)?;
    git_processes = git_processes
        .checked_add(1)
        .ok_or(LocalMemoryManageError::CountNotRepresentable)?;
    let (commits, commit_history_complete) =
        history_commits(worktree, head, limits, cancelled, deadline)?;
    git_processes = git_processes
        .checked_add(1)
        .ok_or(LocalMemoryManageError::CountNotRepresentable)?;
    let mut observations = Vec::new();
    let mut total_record_bytes = 0_u64;
    for commit in &commits {
        check_control(cancelled, deadline)?;
        let entries = tree_entries(worktree, *commit, limits, cancelled, deadline)?;
        git_processes = git_processes
            .checked_add(1)
            .ok_or(LocalMemoryManageError::CountNotRepresentable)?;
        for entry in entries {
            if observations.len()
                >= usize::try_from(limits.max_records)
                    .map_err(|_| LocalMemoryManageError::CountNotRepresentable)?
            {
                return Err(LocalMemoryManageError::HistoryLimitExceeded);
            }
            let bytes = read_blob(worktree, &entry.object_hex, limits, cancelled, deadline)?;
            git_processes = git_processes
                .checked_add(1)
                .ok_or(LocalMemoryManageError::CountNotRepresentable)?;
            total_record_bytes = total_record_bytes
                .checked_add(
                    u64::try_from(bytes.len())
                        .map_err(|_| LocalMemoryManageError::CountNotRepresentable)?,
                )
                .ok_or(LocalMemoryManageError::CountNotRepresentable)?;
            if total_record_bytes > limits.max_total_bytes {
                return Err(LocalMemoryManageError::HistoryLimitExceeded);
            }
            let parsed = parse_memory_record(&bytes, MemoryFormatControl::new(cancelled, deadline))
                .map_err(|_| LocalMemoryManageError::HistoryUnavailable)?;
            if parsed.record().header().record_id() != entry.record_id
                || parsed.record().scope().repository() != repository
            {
                return Err(LocalMemoryManageError::HistoryUnavailable);
            }
            secret::check_record(parsed.record())?;
            observations.push(HistoryObservation {
                commit: *commit,
                record: parsed.into_record(),
                presentation: MemoryPresentationDigest::new(Sha256::digest(&bytes).into()),
            });
        }
    }
    Ok(PreparedHistory {
        observations,
        commits_inspected: u32::try_from(commits.len())
            .map_err(|_| LocalMemoryManageError::CountNotRepresentable)?,
        total_record_bytes,
        git_processes,
        history_complete: commit_history_complete && !shallow,
    })
}

struct TreeEntry {
    object_hex: String,
    record_id: repowitness_domain::MemoryRecordId,
}

fn history_commits(
    worktree: &std::path::Path,
    head: MemoryCommitId,
    limits: LocalMemoryHistoryImportLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(Vec<MemoryCommitId>, bool), LocalMemoryManageError> {
    let requested = u32::from(limits.max_commits)
        .checked_add(1)
        .ok_or(LocalMemoryManageError::CountNotRepresentable)?;
    let mut command = sanitized_git_base_command(worktree);
    command
        .arg("rev-list")
        .arg("--topo-order")
        .arg("--reverse")
        .arg(format!("--max-count={requested}"))
        .arg(commit_hex(head))
        .arg("--")
        .arg(".code-memory/records");
    let output = capture(command, limits, cancelled, deadline)?;
    let mut commits = parse_commit_lines(head.object_format(), &output)?;
    let complete = commits.len() <= usize::from(limits.max_commits);
    if !complete {
        commits.remove(0);
    }
    Ok((commits, complete))
}

fn tree_entries(
    worktree: &std::path::Path,
    commit: MemoryCommitId,
    limits: LocalMemoryHistoryImportLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<TreeEntry>, LocalMemoryManageError> {
    let mut command = sanitized_git_base_command(worktree);
    command
        .arg("ls-tree")
        .arg("-rz")
        .arg("--full-tree")
        .arg(commit_hex(commit))
        .arg("--")
        .arg(".code-memory/records");
    let output = capture(command, limits, cancelled, deadline)?;
    parse_tree_entries(commit.object_format(), &output)
}

fn read_blob(
    worktree: &std::path::Path,
    object_hex: &str,
    limits: LocalMemoryHistoryImportLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<u8>, LocalMemoryManageError> {
    let mut command = sanitized_git_base_command(worktree);
    command.arg("cat-file").arg("blob").arg(object_hex);
    let blob_limits = GitPathDiscoveryLimits::new(
        limits.git_command_deadline,
        u64::try_from(MAX_MEMORY_YAML_BYTES)
            .map_err(|_| LocalMemoryManageError::CountNotRepresentable)?,
        1,
        GIT_PATH_LIMITS,
    );
    capture_git_output_from_command(command, blob_limits, deadline, &mut || {
        cancelled.load(std::sync::atomic::Ordering::Acquire)
    })
    .map_err(map_git_error)
}

fn capture(
    command: Command,
    limits: LocalMemoryHistoryImportLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<u8>, LocalMemoryManageError> {
    let git_limits = GitPathDiscoveryLimits::new(
        limits.git_command_deadline,
        limits.max_git_output_bytes,
        u64::from(limits.max_records),
        GIT_PATH_LIMITS,
    );
    capture_git_output_from_command(command, git_limits, deadline, &mut || {
        cancelled.load(std::sync::atomic::Ordering::Acquire)
    })
    .map_err(map_git_error)
}

fn parse_commit_lines(
    format: MemoryObjectFormat,
    output: &[u8],
) -> Result<Vec<MemoryCommitId>, LocalMemoryManageError> {
    if output.is_empty() {
        return Ok(Vec::new());
    }
    if output.last() != Some(&b'\n') {
        return Err(LocalMemoryManageError::HistoryUnavailable);
    }
    let mut commits = Vec::new();
    for line in output[..output.len() - 1].split(|byte| *byte == b'\n') {
        let commit =
            parse_object_id(format, line).ok_or(LocalMemoryManageError::HistoryUnavailable)?;
        commits.push(commit);
    }
    if commits.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(LocalMemoryManageError::HistoryUnavailable);
    }
    Ok(commits)
}

fn parse_tree_entries(
    format: MemoryObjectFormat,
    output: &[u8],
) -> Result<Vec<TreeEntry>, LocalMemoryManageError> {
    if output.is_empty() {
        return Ok(Vec::new());
    }
    let records = output
        .strip_suffix(&[0])
        .ok_or(LocalMemoryManageError::HistoryUnavailable)?;
    let mut entries = Vec::new();
    for raw in records.split(|byte| *byte == 0) {
        let (metadata, path) = split_once(raw, b'\t')?;
        let mut fields = metadata.split(|byte| *byte == b' ');
        if fields.next() != Some(b"100644".as_slice()) || fields.next() != Some(b"blob".as_slice())
        {
            return Err(LocalMemoryManageError::HistoryUnavailable);
        }
        let object = fields
            .next()
            .ok_or(LocalMemoryManageError::HistoryUnavailable)?;
        if fields.next().is_some() || parse_object_id(format, object).is_none() {
            return Err(LocalMemoryManageError::HistoryUnavailable);
        }
        let record_id = record_id_from_path(path)?;
        let object_hex = std::str::from_utf8(object)
            .map_err(|_| LocalMemoryManageError::HistoryUnavailable)?
            .to_owned();
        entries.push(TreeEntry {
            object_hex,
            record_id,
        });
    }
    entries.sort_by_key(|entry| entry.record_id);
    if entries
        .windows(2)
        .any(|pair| pair[0].record_id == pair[1].record_id)
    {
        return Err(LocalMemoryManageError::HistoryUnavailable);
    }
    Ok(entries)
}

fn split_once(bytes: &[u8], delimiter: u8) -> Result<(&[u8], &[u8]), LocalMemoryManageError> {
    let index = bytes
        .iter()
        .position(|byte| *byte == delimiter)
        .ok_or(LocalMemoryManageError::HistoryUnavailable)?;
    Ok((&bytes[..index], &bytes[index + 1..]))
}

fn record_id_from_path(
    path: &[u8],
) -> Result<repowitness_domain::MemoryRecordId, LocalMemoryManageError> {
    let name = path
        .strip_prefix(MEMORY_PATH_PREFIX)
        .and_then(|value| value.strip_suffix(YAML_SUFFIX))
        .filter(|value| !value.is_empty() && !value.contains(&b'/'))
        .ok_or(LocalMemoryManageError::HistoryUnavailable)?;
    let name = std::str::from_utf8(name).map_err(|_| LocalMemoryManageError::HistoryUnavailable)?;
    MemoryRecordIdTextV1::decode(name).map_err(|_| LocalMemoryManageError::HistoryUnavailable)
}

fn parse_object_id(format: MemoryObjectFormat, bytes: &[u8]) -> Option<MemoryCommitId> {
    let expected = match format {
        MemoryObjectFormat::Sha1 => 40,
        MemoryObjectFormat::Sha256 => 64,
    };
    if bytes.len() != expected {
        return None;
    }
    let mut decoded = [0_u8; 32];
    for (index, pair) in bytes.chunks_exact(2).enumerate() {
        decoded[index] = decode_hex(pair)?;
    }
    Some(match format {
        MemoryObjectFormat::Sha1 => {
            let mut value = [0_u8; 20];
            value.copy_from_slice(&decoded[..20]);
            MemoryCommitId::Sha1(value)
        }
        MemoryObjectFormat::Sha256 => MemoryCommitId::Sha256(decoded),
    })
}

fn decode_hex(pair: &[u8]) -> Option<u8> {
    Some(hex_nibble(*pair.first()?)? << 4 | hex_nibble(*pair.get(1)?)?)
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn commit_hex(commit: MemoryCommitId) -> String {
    let mut output = String::with_capacity(commit.as_bytes().len() * 2);
    for byte in commit.as_bytes() {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn persist_history(
    store: &crate::OwnedSqliteIndex,
    prepared: PreparedHistory,
    repository: repowitness_domain::RepositoryIdentityDigest,
    actor: MemoryAuditActorId,
    recorded_at: MemoryRecordedAtUnixMillis,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> Result<LocalMemoryHistoryImportReport, LocalMemoryManageError> {
    let records_inspected = u32::try_from(prepared.observations.len())
        .map_err(|_| LocalMemoryManageError::CountNotRepresentable)?;
    let mut imported_versions = 0_u32;
    let mut appended_observations = 0_u32;
    for observation in prepared.observations {
        check_control(cancelled.as_ref(), deadline)?;
        let receipt = store
            .import_memory_version(
                repository,
                observation.record,
                observation.presentation,
                MemoryObservationSource::Git(observation.commit),
                actor.clone(),
                recorded_at,
                MemoryImportApproval::ObservedOnly,
                Arc::clone(cancelled),
                deadline,
            )
            .map_err(map_store_error)?;
        imported_versions = imported_versions
            .checked_add(u32::from(receipt.version_inserted()))
            .ok_or(LocalMemoryManageError::CountNotRepresentable)?;
        appended_observations = appended_observations
            .checked_add(u32::from(receipt.observation_inserted()))
            .ok_or(LocalMemoryManageError::CountNotRepresentable)?;
        if receipt.approval_inserted() {
            return Err(LocalMemoryManageError::PersistenceFailed);
        }
    }
    Ok(LocalMemoryHistoryImportReport {
        commits_inspected: prepared.commits_inspected,
        records_inspected,
        imported_versions,
        appended_observations,
        total_record_bytes: prepared.total_record_bytes,
        git_processes: prepared.git_processes,
        history_complete: prepared.history_complete,
    })
}

fn finish_history(
    store: crate::OwnedSqliteIndex,
    operation: Result<LocalMemoryHistoryImportReport, LocalMemoryManageError>,
    deadline: Instant,
) -> Result<LocalMemoryHistoryImportReport, LocalMemoryManageError> {
    if operation.is_ok() {
        store.checkpoint(deadline).map_err(map_store_error)?;
    }
    let shutdown = store.shutdown(deadline).map_err(map_store_error);
    match (operation, shutdown) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(report), Ok(())) => Ok(report),
    }
}
