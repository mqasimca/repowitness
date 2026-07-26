//! Canonical, bounded Git and worktree source-state capture.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::process::ExitStatus;
use std::time::Instant;

use repowitness_domain::{
    GitStateDigest, RepositoryPath, RepositoryPathError, SourceManifestDigest, WorktreeStateDigest,
};
use sha2::{Digest, Sha256};

use crate::git_paths::{
    GitPathDiscoveryError, GitPathDiscoveryLimits, capture_git_output_from_command,
    capture_git_output_with_status_from_command, discovered_worktree_root,
    sanitized_git_base_command,
};

const GIT_STATE_DOMAIN: &[u8] = b"RepoWitness\0git-state\0";
const RUST_WORKTREE_STATE_DOMAIN: &[u8] = b"RepoWitness\0rust-worktree-state\0";

/// Version of the canonical Git-state encoding.
pub const GIT_STATE_VERSION: u32 = 1;
/// Version of the canonical Rust worktree-state encoding.
pub const RUST_WORKTREE_STATE_VERSION: u32 = 1;
/// Version of the sanitized porcelain-v2 status profile.
pub const GIT_STATUS_PROFILE_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GitObjectFormat {
    Sha1,
    Sha256,
}

impl GitObjectFormat {
    const fn tag(self) -> u8 {
        match self {
            Self::Sha1 => 1,
            Self::Sha256 => 2,
        }
    }

    const fn object_id_bytes(self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
struct CanonicalStatusRecord {
    path: RepositoryPath,
    tag: u8,
    fields: Box<[u8]>,
}

impl fmt::Debug for CanonicalStatusRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CanonicalStatusRecord")
            .field("tag", &self.tag)
            .field("field_bytes", &self.fields.len())
            .field("path", &self.path)
            .finish()
    }
}

/// An opaque, exact source-state capture from which receipts can be derived.
#[derive(Clone, Eq, PartialEq)]
pub struct CapturedSourceState {
    git_state: GitStateDigest,
    status_records: Box<[CanonicalStatusRecord]>,
}

impl CapturedSourceState {
    /// Returns the canonical Git-state receipt.
    #[must_use]
    pub const fn git_state(&self) -> GitStateDigest {
        self.git_state
    }

    /// Returns the number of validated status records.
    #[must_use]
    pub fn status_record_count(&self) -> u64 {
        u64::try_from(self.status_records.len()).unwrap_or(u64::MAX)
    }

    /// Hashes this status capture with an exact source-manifest receipt.
    #[must_use]
    pub fn worktree_state(&self, manifest: SourceManifestDigest) -> WorktreeStateDigest {
        hash_worktree_state(&self.status_records, manifest)
    }
}

impl fmt::Debug for CapturedSourceState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CapturedSourceState")
            .field("git_state", &self.git_state)
            .field("status_record_count", &self.status_record_count())
            .finish_non_exhaustive()
    }
}

/// A bounded source-state capture failure.
#[derive(Debug)]
pub enum SourceStateError {
    /// The configured deadline cannot be represented by the monotonic clock.
    DeadlineNotRepresentable,
    /// A sanitized Git subprocess failed.
    Git {
        /// The redacted subprocess failure.
        source: GitPathDiscoveryError,
    },
    /// Git reported an unsupported object format.
    UnsupportedObjectFormat,
    /// Git returned malformed object-format output.
    InvalidObjectFormat,
    /// Git returned malformed `HEAD` output.
    InvalidHead,
    /// Git returned malformed shallow-state output.
    InvalidShallowState,
    /// Git returned a malformed cached-index record.
    InvalidIndexRecord,
    /// Sparse-worktree state is outside the Phase 0 contract.
    SparseWorktreeUnsupported,
    /// A gitlink or submodule is outside the Phase 0 contract.
    SubmoduleUnsupported,
    /// Git returned malformed porcelain-v2 status output.
    InvalidStatusRecord,
    /// A Git record contained a path outside the repository-path contract.
    InvalidRepositoryPath {
        /// The one-based record ordinal without path content.
        ordinal: u64,
        /// The redacted domain validation error.
        source: RepositoryPathError,
    },
    /// Status contained more paths than the configured bound.
    StatusPathLimitExceeded {
        /// The inclusive configured path bound.
        limit: u64,
    },
    /// Status returned more than one record for a repository-path identity.
    DuplicateStatusPath,
    /// A fixed-width record count could not represent the captured result.
    RecordCountNotRepresentable,
    /// Source state changed across a stability fence.
    ConcurrentSourceChange,
}

impl fmt::Display for SourceStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeadlineNotRepresentable => {
                formatter.write_str("source-state deadline cannot be represented")
            }
            Self::Git { .. } => formatter.write_str("sanitized Git source-state capture failed"),
            Self::UnsupportedObjectFormat => {
                formatter.write_str("Git object format is not supported")
            }
            Self::InvalidObjectFormat => {
                formatter.write_str("Git returned an invalid object format")
            }
            Self::InvalidHead => formatter.write_str("Git returned an invalid HEAD state"),
            Self::InvalidShallowState => {
                formatter.write_str("Git returned an invalid shallow state")
            }
            Self::InvalidIndexRecord => {
                formatter.write_str("Git returned an invalid cached-index record")
            }
            Self::SparseWorktreeUnsupported => {
                formatter.write_str("sparse worktrees are not supported in Phase 0")
            }
            Self::SubmoduleUnsupported => {
                formatter.write_str("submodules are not supported in Phase 0")
            }
            Self::InvalidStatusRecord => {
                formatter.write_str("Git returned an invalid porcelain-v2 status record")
            }
            Self::InvalidRepositoryPath { ordinal, source } => {
                write!(
                    formatter,
                    "source-state path {ordinal} failed validation: {source}"
                )
            }
            Self::StatusPathLimitExceeded { limit } => {
                write!(
                    formatter,
                    "source-state status exceeded its {limit} path bound"
                )
            }
            Self::DuplicateStatusPath => {
                formatter.write_str("Git returned a duplicate source-state path")
            }
            Self::RecordCountNotRepresentable => {
                formatter.write_str("source-state record count cannot be represented")
            }
            Self::ConcurrentSourceChange => {
                formatter.write_str("repository source state changed during indexing")
            }
        }
    }
}

impl Error for SourceStateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Git { source } => Some(source),
            Self::InvalidRepositoryPath { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<GitPathDiscoveryError> for SourceStateError {
    fn from(source: GitPathDiscoveryError) -> Self {
        Self::Git { source }
    }
}

/// Captures canonical Git and porcelain-v2 source state.
///
/// The returned value contains no host path or remote identity and cannot
/// expose raw Git output. Sparse worktrees and gitlinks fail closed.
///
/// # Errors
///
/// Returns a typed error for process, cancellation, deadline, bounds,
/// unsupported scope, or malformed hostile input.
pub fn capture_source_state(
    root: &Path,
    limits: GitPathDiscoveryLimits,
) -> Result<CapturedSourceState, SourceStateError> {
    capture_source_state_with_cancel(root, limits, || false)
}

/// Captures source state while polling a caller-provided cancellation signal.
///
/// # Errors
///
/// Returns the same failures as [`capture_source_state`], including a wrapped
/// cancellation error when the callback requests cancellation.
pub fn capture_source_state_with_cancel(
    root: &Path,
    limits: GitPathDiscoveryLimits,
    mut is_cancelled: impl FnMut() -> bool,
) -> Result<CapturedSourceState, SourceStateError> {
    if is_cancelled() {
        return Err(GitPathDiscoveryError::Cancelled.into());
    }
    if limits.deadline().is_zero() {
        return Err(GitPathDiscoveryError::DeadlineExceeded {
            deadline: limits.deadline(),
        }
        .into());
    }
    let deadline = Instant::now()
        .checked_add(limits.deadline())
        .ok_or(SourceStateError::DeadlineNotRepresentable)?;
    let worktree_root = discovered_worktree_root(root)?;
    check_capture_control(deadline, limits.deadline(), &mut is_cancelled)?;

    let object_format = capture_object_format(&worktree_root, limits, deadline, &mut is_cancelled)?;
    check_capture_control(deadline, limits.deadline(), &mut is_cancelled)?;
    let head = capture_head(
        &worktree_root,
        object_format,
        limits,
        deadline,
        &mut is_cancelled,
    )?;
    check_capture_control(deadline, limits.deadline(), &mut is_cancelled)?;
    let shallow = capture_shallow_state(&worktree_root, limits, deadline, &mut is_cancelled)?;
    check_capture_control(deadline, limits.deadline(), &mut is_cancelled)?;
    inspect_index_scope(
        &worktree_root,
        object_format,
        limits,
        deadline,
        &mut is_cancelled,
    )?;
    check_capture_control(deadline, limits.deadline(), &mut is_cancelled)?;
    let status_output = capture_status(&worktree_root, limits, deadline, &mut is_cancelled)?;
    let status_records = parse_status_records(&status_output, object_format, limits)?;

    Ok(CapturedSourceState {
        git_state: hash_git_state(object_format, head.as_deref(), shallow),
        status_records,
    })
}

fn check_capture_control(
    deadline: Instant,
    configured_deadline: std::time::Duration,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(), SourceStateError> {
    if is_cancelled() {
        return Err(GitPathDiscoveryError::Cancelled.into());
    }
    if Instant::now() >= deadline {
        return Err(GitPathDiscoveryError::DeadlineExceeded {
            deadline: configured_deadline,
        }
        .into());
    }
    Ok(())
}

fn capture_object_format(
    root: &Path,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<GitObjectFormat, SourceStateError> {
    let mut command = sanitized_git_base_command(root);
    command.arg("rev-parse").arg("--show-object-format");
    let output = capture_git_output_from_command(command, limits, deadline, is_cancelled)?;
    parse_object_format(&output)
}

fn capture_head(
    root: &Path,
    object_format: GitObjectFormat,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<Box<[u8]>>, SourceStateError> {
    let mut command = sanitized_git_base_command(root);
    command
        .arg("rev-parse")
        .arg("--verify")
        .arg("--quiet")
        .arg("--end-of-options")
        .arg("HEAD^{commit}");
    let (status, output) =
        capture_git_output_with_status_from_command(command, limits, deadline, is_cancelled)?;
    if status.success() {
        let line = single_lf_line(&output).ok_or(SourceStateError::InvalidHead)?;
        let oid = decode_object_id(line, object_format).ok_or(SourceStateError::InvalidHead)?;
        if oid.iter().all(|byte| *byte == 0) {
            return Err(SourceStateError::InvalidHead);
        }
        return Ok(Some(oid));
    }
    if is_exit_code(&status, 1) && output.is_empty() {
        return Ok(None);
    }
    Err(GitPathDiscoveryError::GitUnsuccessful {
        code: status.code(),
    }
    .into())
}

fn capture_shallow_state(
    root: &Path,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<bool, SourceStateError> {
    let mut command = sanitized_git_base_command(root);
    command.arg("rev-parse").arg("--is-shallow-repository");
    let output = capture_git_output_from_command(command, limits, deadline, is_cancelled)?;
    parse_shallow_state(&output)
}

fn inspect_index_scope(
    root: &Path,
    object_format: GitObjectFormat,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(), SourceStateError> {
    let mut command = sanitized_git_base_command(root);
    command
        .arg("ls-files")
        .arg("--stage")
        .arg("-v")
        .arg("-z")
        .arg("--full-name")
        .arg("--cached");
    let output = capture_git_output_from_command(command, limits, deadline, is_cancelled)?;
    parse_index_scope(&output, object_format, limits)
}

fn capture_status(
    root: &Path,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<u8>, SourceStateError> {
    let mut command = sanitized_git_base_command(root);
    command
        .arg("status")
        .arg("--porcelain=v2")
        .arg("-z")
        .arg("--untracked-files=all")
        .arg("--ignore-submodules=none")
        .arg("--no-renames");
    capture_git_output_from_command(command, limits, deadline, is_cancelled)
        .map_err(SourceStateError::from)
}

fn parse_object_format(output: &[u8]) -> Result<GitObjectFormat, SourceStateError> {
    match single_lf_line(output) {
        Some(b"sha1") => Ok(GitObjectFormat::Sha1),
        Some(b"sha256") => Ok(GitObjectFormat::Sha256),
        Some(line)
            if line
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()) =>
        {
            Err(SourceStateError::UnsupportedObjectFormat)
        }
        _ => Err(SourceStateError::InvalidObjectFormat),
    }
}

fn parse_shallow_state(output: &[u8]) -> Result<bool, SourceStateError> {
    match single_lf_line(output) {
        Some(b"true") => Ok(true),
        Some(b"false") => Ok(false),
        _ => Err(SourceStateError::InvalidShallowState),
    }
}

fn single_lf_line(output: &[u8]) -> Option<&[u8]> {
    let line = output.strip_suffix(b"\n")?;
    if line.is_empty() || line.contains(&b'\n') || line.contains(&b'\r') {
        return None;
    }
    Some(line)
}

fn is_exit_code(status: &ExitStatus, expected: i32) -> bool {
    status.code() == Some(expected)
}

fn parse_index_scope(
    output: &[u8],
    object_format: GitObjectFormat,
    limits: GitPathDiscoveryLimits,
) -> Result<(), SourceStateError> {
    let records = nul_records(output).ok_or(SourceStateError::InvalidIndexRecord)?;
    let mut paths = BTreeMap::<RepositoryPath, u8>::new();
    for record in records {
        let (metadata, path) = split_once(record, b'\t')
            .filter(|(_, path)| !path.is_empty())
            .ok_or(SourceStateError::InvalidIndexRecord)?;
        let fields = metadata.split(|byte| *byte == b' ').collect::<Vec<_>>();
        if fields.len() != 4
            || !valid_index_tag(fields[0])
            || parse_mode(fields[1]).is_none()
            || decode_object_id(fields[2], object_format).is_none()
            || !matches!(fields[3], b"0" | b"1" | b"2" | b"3")
        {
            return Err(SourceStateError::InvalidIndexRecord);
        }
        if matches!(fields[0], b"S" | b"s") {
            return Err(SourceStateError::SparseWorktreeUnsupported);
        }
        if fields[1] == b"040000" {
            return Err(SourceStateError::SparseWorktreeUnsupported);
        }
        if fields[1] == b"160000" {
            return Err(SourceStateError::SubmoduleUnsupported);
        }
        let stage = fields[3][0] - b'0';
        let ordinal = u64::try_from(paths.len())
            .ok()
            .and_then(|count| count.checked_add(1))
            .ok_or(SourceStateError::RecordCountNotRepresentable)?;
        let path = validate_path(path, ordinal, limits)?;
        if !paths.contains_key(&path)
            && u64::try_from(paths.len()).unwrap_or(u64::MAX) >= limits.paths()
        {
            return Err(SourceStateError::StatusPathLimitExceeded {
                limit: limits.paths(),
            });
        }
        match paths.entry(path) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(1_u8 << stage);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let existing = *entry.get();
                let stage_mask = 1_u8 << stage;
                if existing & stage_mask != 0 || stage == 0 || existing & 1 != 0 {
                    return Err(SourceStateError::InvalidIndexRecord);
                }
                *entry.get_mut() = existing | stage_mask;
            }
        }
    }
    Ok(())
}

fn parse_status_records(
    output: &[u8],
    object_format: GitObjectFormat,
    limits: GitPathDiscoveryLimits,
) -> Result<Box<[CanonicalStatusRecord]>, SourceStateError> {
    let records = nul_records(output).ok_or(SourceStateError::InvalidStatusRecord)?;
    let mut parsed = Vec::new();
    let mut count = 0_u64;
    for record in records {
        count = checked_record_count(count, limits)?;
        let parsed_record = match record.first() {
            Some(b'1') => parse_ordinary_record(record, object_format, count, limits)?,
            Some(b'u') => parse_unmerged_record(record, object_format, count, limits)?,
            Some(b'?') => parse_untracked_record(record, count, limits)?,
            Some(b'2') => return Err(SourceStateError::InvalidStatusRecord),
            Some(b'#' | b'!' | b'S') => return Err(SourceStateError::InvalidStatusRecord),
            _ => return Err(SourceStateError::InvalidStatusRecord),
        };
        parsed.push(parsed_record);
    }
    parsed.sort_unstable_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.tag.cmp(&right.tag))
    });
    if parsed
        .windows(2)
        .any(|records| records[0].path == records[1].path)
    {
        return Err(SourceStateError::DuplicateStatusPath);
    }
    Ok(parsed.into_boxed_slice())
}

fn parse_ordinary_record(
    record: &[u8],
    object_format: GitObjectFormat,
    ordinal: u64,
    limits: GitPathDiscoveryLimits,
) -> Result<CanonicalStatusRecord, SourceStateError> {
    let fields = record.splitn(9, |byte| *byte == b' ').collect::<Vec<_>>();
    if fields.len() != 9
        || fields[0] != b"1"
        || !valid_ordinary_status(fields[1])
        || !valid_non_submodule_field(fields[2])?
    {
        return Err(SourceStateError::InvalidStatusRecord);
    }
    let mut canonical_fields = Vec::with_capacity(96);
    canonical_fields.extend_from_slice(fields[1]);
    canonical_fields.extend_from_slice(fields[2]);
    for mode in &fields[3..6] {
        append_mode(&mut canonical_fields, mode)?;
    }
    for object_id in &fields[6..8] {
        append_object_id(&mut canonical_fields, object_id, object_format)?;
    }
    let path = validate_path(fields[8], ordinal, limits)?;
    Ok(CanonicalStatusRecord {
        path,
        tag: 1,
        fields: canonical_fields.into_boxed_slice(),
    })
}

fn parse_unmerged_record(
    record: &[u8],
    object_format: GitObjectFormat,
    ordinal: u64,
    limits: GitPathDiscoveryLimits,
) -> Result<CanonicalStatusRecord, SourceStateError> {
    let fields = record.splitn(11, |byte| *byte == b' ').collect::<Vec<_>>();
    if fields.len() != 11
        || fields[0] != b"u"
        || !valid_unmerged_status(fields[1])
        || !valid_non_submodule_field(fields[2])?
    {
        return Err(SourceStateError::InvalidStatusRecord);
    }
    let mut canonical_fields = Vec::with_capacity(132);
    canonical_fields.extend_from_slice(fields[1]);
    canonical_fields.extend_from_slice(fields[2]);
    for mode in &fields[3..7] {
        append_mode(&mut canonical_fields, mode)?;
    }
    for object_id in &fields[7..10] {
        append_object_id(&mut canonical_fields, object_id, object_format)?;
    }
    let path = validate_path(fields[10], ordinal, limits)?;
    Ok(CanonicalStatusRecord {
        path,
        tag: 2,
        fields: canonical_fields.into_boxed_slice(),
    })
}

fn parse_untracked_record(
    record: &[u8],
    ordinal: u64,
    limits: GitPathDiscoveryLimits,
) -> Result<CanonicalStatusRecord, SourceStateError> {
    let path = record
        .strip_prefix(b"? ")
        .filter(|path| !path.is_empty())
        .ok_or(SourceStateError::InvalidStatusRecord)?;
    Ok(CanonicalStatusRecord {
        path: validate_path(path, ordinal, limits)?,
        tag: 3,
        fields: Box::new([]),
    })
}

fn valid_ordinary_status(status: &[u8]) -> bool {
    status.len() == 2
        && matches!(status[0], b'.' | b'M' | b'T' | b'A' | b'D' | b'R' | b'C')
        && matches!(status[1], b'.' | b'M' | b'T' | b'D' | b'R' | b'C')
        && status != b".."
}

fn valid_unmerged_status(status: &[u8]) -> bool {
    matches!(
        status,
        b"DD" | b"AU" | b"UD" | b"UA" | b"DU" | b"AA" | b"UU"
    )
}

fn valid_non_submodule_field(field: &[u8]) -> Result<bool, SourceStateError> {
    if field.len() != 4 {
        return Ok(false);
    }
    if field[0] == b'S' {
        return Err(SourceStateError::SubmoduleUnsupported);
    }
    Ok(field == b"N...")
}

fn append_mode(target: &mut Vec<u8>, field: &[u8]) -> Result<(), SourceStateError> {
    let mode = parse_mode(field).ok_or(SourceStateError::InvalidStatusRecord)?;
    if field == b"040000" {
        return Err(SourceStateError::SparseWorktreeUnsupported);
    }
    if field == b"160000" {
        return Err(SourceStateError::SubmoduleUnsupported);
    }
    target.extend_from_slice(&mode.to_be_bytes());
    Ok(())
}

fn parse_mode(field: &[u8]) -> Option<u32> {
    if !matches!(
        field,
        b"000000" | b"040000" | b"100644" | b"100755" | b"120000" | b"160000"
    ) {
        return None;
    }
    field.iter().try_fold(0_u32, |mode, byte| {
        mode.checked_mul(8)?.checked_add(u32::from(*byte - b'0'))
    })
}

fn valid_index_tag(field: &[u8]) -> bool {
    matches!(
        field,
        b"H" | b"h" | b"S" | b"s" | b"M" | b"m" | b"R" | b"r" | b"C" | b"c" | b"K" | b"k"
    )
}

fn append_object_id(
    target: &mut Vec<u8>,
    field: &[u8],
    object_format: GitObjectFormat,
) -> Result<(), SourceStateError> {
    let object_id =
        decode_object_id(field, object_format).ok_or(SourceStateError::InvalidStatusRecord)?;
    target.push(object_format.tag());
    target.push(u8::try_from(object_id.len()).map_err(|_| SourceStateError::InvalidStatusRecord)?);
    target.extend_from_slice(&object_id);
    Ok(())
}

fn decode_object_id(field: &[u8], object_format: GitObjectFormat) -> Option<Box<[u8]>> {
    if field.len() != object_format.object_id_bytes().checked_mul(2)?
        || !field
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return None;
    }
    let decoded = field
        .chunks_exact(2)
        .map(|pair| decode_nibble(pair[0]).zip(decode_nibble(pair[1])))
        .map(|pair| pair.map(|(high, low)| (high << 4) | low))
        .collect::<Option<Vec<_>>>()?;
    Some(decoded.into_boxed_slice())
}

const fn decode_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn validate_path(
    path: &[u8],
    ordinal: u64,
    limits: GitPathDiscoveryLimits,
) -> Result<RepositoryPath, SourceStateError> {
    RepositoryPath::try_from_bytes(path, limits.repository_path())
        .map_err(|source| SourceStateError::InvalidRepositoryPath { ordinal, source })
}

fn checked_record_count(
    count: u64,
    limits: GitPathDiscoveryLimits,
) -> Result<u64, SourceStateError> {
    let next = count
        .checked_add(1)
        .ok_or(SourceStateError::RecordCountNotRepresentable)?;
    if next > limits.paths() {
        return Err(SourceStateError::StatusPathLimitExceeded {
            limit: limits.paths(),
        });
    }
    Ok(next)
}

struct NulRecords<'a> {
    remaining: Option<&'a [u8]>,
}

impl<'a> Iterator for NulRecords<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        let remaining = self.remaining.take()?;
        if remaining.is_empty() {
            return None;
        }
        match remaining.iter().position(|byte| *byte == 0) {
            Some(position) => {
                self.remaining = Some(&remaining[position + 1..]);
                Some(&remaining[..position])
            }
            None => Some(remaining),
        }
    }
}

fn nul_records(output: &[u8]) -> Option<NulRecords<'_>> {
    if output.is_empty() {
        return Some(NulRecords { remaining: None });
    }
    let records = output.strip_suffix(b"\0")?;
    if records.split(|byte| *byte == 0).any(<[u8]>::is_empty) {
        return None;
    }
    Some(NulRecords {
        remaining: Some(records),
    })
}

fn split_once(bytes: &[u8], delimiter: u8) -> Option<(&[u8], &[u8])> {
    let position = bytes.iter().position(|byte| *byte == delimiter)?;
    Some((&bytes[..position], &bytes[position + 1..]))
}

fn hash_git_state(
    object_format: GitObjectFormat,
    head: Option<&[u8]>,
    shallow: bool,
) -> GitStateDigest {
    let mut hasher = Sha256::new();
    hasher.update(GIT_STATE_DOMAIN);
    hasher.update(GIT_STATE_VERSION.to_be_bytes());
    hasher.update([object_format.tag()]);
    match head {
        Some(object_id) => {
            hasher.update([1]);
            hasher.update([u8::try_from(object_id.len()).unwrap_or(u8::MAX)]);
            hasher.update(object_id);
        }
        None => hasher.update([0, 0]),
    }
    hasher.update([u8::from(shallow)]);
    GitStateDigest::new(hasher.finalize().into())
}

fn hash_worktree_state(
    records: &[CanonicalStatusRecord],
    manifest: SourceManifestDigest,
) -> WorktreeStateDigest {
    let mut hasher = Sha256::new();
    hasher.update(RUST_WORKTREE_STATE_DOMAIN);
    hasher.update(RUST_WORKTREE_STATE_VERSION.to_be_bytes());
    hasher.update(GIT_STATUS_PROFILE_VERSION.to_be_bytes());
    hasher.update(
        u64::try_from(records.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for record in records {
        hasher.update([record.tag]);
        hasher.update(
            u64::try_from(record.fields.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hasher.update(&record.fields);
        hasher.update(
            u64::try_from(record.path.as_bytes().len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hasher.update(record.path.as_bytes());
    }
    hasher.update(manifest.as_bytes());
    WorktreeStateDigest::new(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use repowitness_domain::{RepositoryPathLimits, SourceManifestDigest};

    use super::{
        GIT_STATE_VERSION, GIT_STATUS_PROFILE_VERSION, GitObjectFormat,
        RUST_WORKTREE_STATE_VERSION, SourceStateError, capture_source_state,
        capture_source_state_with_cancel, hash_git_state, parse_index_scope, parse_object_format,
        parse_shallow_state, parse_status_records,
    };
    use crate::GitPathDiscoveryLimits;

    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    const SHA1_ZERO: &str = "0000000000000000000000000000000000000000";
    const SHA1_ONE: &str = "1111111111111111111111111111111111111111";
    const SHA1_TWO: &str = "2222222222222222222222222222222222222222";
    const SHA1_THREE: &str = "3333333333333333333333333333333333333333";

    struct TempDirectory {
        root: PathBuf,
    }

    impl TempDirectory {
        fn new(label: &str) -> Self {
            let ordinal = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "repowitness-source-state-{label}-{}-{ordinal}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("fixture directory should be created");
            Self { root }
        }

        fn path(&self) -> &Path {
            &self.root
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    struct TempRepository {
        directory: TempDirectory,
    }

    impl TempRepository {
        fn new(object_format: &str) -> Self {
            let directory = TempDirectory::new("repository");
            let repository = Self { directory };
            let format_argument = format!("--object-format={object_format}");
            repository.git(&["init", "--quiet", "--initial-branch=main", &format_argument]);
            repository
        }

        fn path(&self) -> &Path {
            self.directory.path()
        }

        fn write(&self, relative: &str, content: &[u8]) {
            let path = self.path().join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("fixture parent should be created");
            }
            fs::write(path, content).expect("fixture file should be written");
        }

        fn git(&self, arguments: &[&str]) {
            let status = self
                .git_command(arguments)
                .status()
                .expect("Git should start");
            assert!(status.success(), "fixture Git failed: {status}");
        }

        fn git_text(&self, arguments: &[&str]) -> String {
            let output = self
                .git_command(arguments)
                .output()
                .expect("Git should start");
            assert!(output.status.success(), "fixture Git failed");
            String::from_utf8(output.stdout)
                .expect("fixture Git output should be UTF-8")
                .trim()
                .to_owned()
        }

        fn git_command(&self, arguments: &[&str]) -> Command {
            let mut command = Command::new("git");
            command
                .arg("--no-pager")
                .arg("-C")
                .arg(self.path())
                .args(arguments)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", null_device())
                .env("GIT_CONFIG_SYSTEM", null_device())
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GCM_INTERACTIVE", "never")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            command
        }

        fn commit_all(&self, message: &str) {
            self.git(&["add", "--all"]);
            self.git(&[
                "-c",
                "user.name=RepoWitness Test",
                "-c",
                "user.email=repowitness@example.invalid",
                "commit",
                "--quiet",
                "-m",
                message,
            ]);
        }
    }

    fn null_device() -> OsString {
        if cfg!(windows) {
            OsString::from("NUL")
        } else {
            OsString::from("/dev/null")
        }
    }

    fn limits(paths: u64) -> GitPathDiscoveryLimits {
        GitPathDiscoveryLimits::new(
            Duration::from_secs(1),
            4096,
            paths,
            RepositoryPathLimits::new(1024, 32),
        )
    }

    fn ordinary(path: &[u8]) -> Vec<u8> {
        let mut record =
            format!("1 M. N... 100644 100644 100644 {SHA1_ONE} {SHA1_TWO} ").into_bytes();
        record.extend_from_slice(path);
        record
    }

    #[test]
    fn metadata_parsers_are_strict_and_support_both_object_formats() {
        assert_eq!(
            parse_object_format(b"sha1\n").unwrap(),
            GitObjectFormat::Sha1
        );
        assert_eq!(
            parse_object_format(b"sha256\n").unwrap(),
            GitObjectFormat::Sha256
        );
        assert!(matches!(
            parse_object_format(b"blake3\n"),
            Err(SourceStateError::UnsupportedObjectFormat)
        ));
        for malformed in [b"sha1".as_slice(), b"SHA1\n", b"sha1\r\n", b"sha1\nextra\n"] {
            assert!(matches!(
                parse_object_format(malformed),
                Err(SourceStateError::InvalidObjectFormat)
            ));
        }
        assert!(parse_shallow_state(b"true\n").unwrap());
        assert!(!parse_shallow_state(b"false\n").unwrap());
        assert!(matches!(
            parse_shallow_state(b"TRUE\n"),
            Err(SourceStateError::InvalidShallowState)
        ));
    }

    #[test]
    fn git_state_hash_has_stable_vectors_and_every_field_participates() {
        let oid = [0x11; 20];
        let baseline = hash_git_state(GitObjectFormat::Sha1, Some(&oid), false);
        assert_eq!(
            baseline.into_bytes(),
            [
                0x88, 0xE7, 0xF0, 0x98, 0xAA, 0x81, 0xB7, 0xAE, 0xE4, 0xB4, 0xFB, 0x91, 0xE6, 0x97,
                0x86, 0xEA, 0xB9, 0x66, 0x86, 0x60, 0xD3, 0xEB, 0xBD, 0xC3, 0x0A, 0x21, 0xDD, 0x32,
                0x04, 0xEC, 0x2E, 0x0E,
            ]
        );
        assert_ne!(
            baseline,
            hash_git_state(GitObjectFormat::Sha256, Some(&[0x11; 32]), false)
        );
        assert_ne!(baseline, hash_git_state(GitObjectFormat::Sha1, None, false));
        assert_ne!(
            baseline,
            hash_git_state(GitObjectFormat::Sha1, Some(&[0x22; 20]), false)
        );
        assert_ne!(
            baseline,
            hash_git_state(GitObjectFormat::Sha1, Some(&oid), true)
        );
        assert_eq!(GIT_STATE_VERSION, 1);
    }

    #[test]
    fn index_scope_rejects_sparse_gitlinks_and_malformed_records() {
        let ordinary = format!("H 100644 {SHA1_ONE} 0\tlib.rs\0");
        parse_index_scope(ordinary.as_bytes(), GitObjectFormat::Sha1, limits(1)).unwrap();

        let conflict = format!(
            "M 100644 {SHA1_ONE} 1\tlib.rs\0M 100644 {SHA1_TWO} 2\tlib.rs\0M 100644 {SHA1_THREE} 3\tlib.rs\0"
        );
        parse_index_scope(conflict.as_bytes(), GitObjectFormat::Sha1, limits(1))
            .expect("three conflict stages are one bounded repository path");
        let duplicate_stage =
            format!("M 100644 {SHA1_ONE} 1\tlib.rs\0M 100644 {SHA1_TWO} 1\tlib.rs\0");
        assert!(matches!(
            parse_index_scope(duplicate_stage.as_bytes(), GitObjectFormat::Sha1, limits(1)),
            Err(SourceStateError::InvalidIndexRecord)
        ));

        let sparse = format!("S 040000 {SHA1_ZERO} 0\tsrc\0");
        assert!(matches!(
            parse_index_scope(sparse.as_bytes(), GitObjectFormat::Sha1, limits(1)),
            Err(SourceStateError::SparseWorktreeUnsupported)
        ));
        let gitlink = format!("H 160000 {SHA1_ONE} 0\tdependency\0");
        assert!(matches!(
            parse_index_scope(gitlink.as_bytes(), GitObjectFormat::Sha1, limits(1)),
            Err(SourceStateError::SubmoduleUnsupported)
        ));
        for malformed in [
            format!("H 100644 {SHA1_ONE} 0\tlib.rs"),
            format!("H 100644 {SHA1_ONE}\tlib.rs\0"),
            format!("H 100644 {SHA1_ONE} 4\tlib.rs\0"),
            format!("X 100644 {SHA1_ONE} 0\tlib.rs\0"),
            format!("H 777777 {SHA1_ONE} 0\tlib.rs\0"),
            format!("H 100644 {SHA1_ONE} 0\t/lib.rs\0"),
        ] {
            assert!(
                parse_index_scope(malformed.as_bytes(), GitObjectFormat::Sha1, limits(1)).is_err()
            );
        }
    }

    #[test]
    fn status_parser_canonicalizes_order_and_preserves_categories() {
        let mut output = Vec::new();
        output.extend_from_slice(b"? z.rs\0");
        output.extend_from_slice(
            format!(
                "u UU N... 100644 100644 100644 100644 {SHA1_ONE} {SHA1_TWO} {SHA1_THREE} conflict.rs\0"
            )
            .as_bytes(),
        );
        output.extend_from_slice(&ordinary(b"a path.rs"));
        output.push(0);

        let parsed = parse_status_records(&output, GitObjectFormat::Sha1, limits(3)).unwrap();
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].path.as_bytes(), b"a path.rs");
        assert_eq!(parsed[0].tag, 1);
        assert_eq!(parsed[1].path.as_bytes(), b"conflict.rs");
        assert_eq!(parsed[1].tag, 2);
        assert_eq!(parsed[2].path.as_bytes(), b"z.rs");
        assert_eq!(parsed[2].tag, 3);
    }

    #[test]
    fn worktree_hash_is_stable_and_every_component_participates() {
        let mut record = ordinary(b"src/lib.rs");
        record.push(0);
        let parsed = parse_status_records(&record, GitObjectFormat::Sha1, limits(1)).unwrap();
        let baseline = super::hash_worktree_state(&parsed, SourceManifestDigest::new([0x44; 32]));
        assert_eq!(
            baseline.into_bytes(),
            [
                0x8B, 0x82, 0x11, 0x5D, 0x8C, 0xA1, 0x32, 0xA8, 0xBD, 0x7E, 0x72, 0x60, 0x09, 0xC4,
                0x55, 0xFF, 0x76, 0x37, 0x94, 0x91, 0x58, 0x7F, 0xE5, 0xB7, 0xB4, 0xAE, 0xE0, 0xA7,
                0xC7, 0x89, 0x0E, 0xED,
            ]
        );
        assert_ne!(
            baseline,
            super::hash_worktree_state(&parsed, SourceManifestDigest::new([0x55; 32]))
        );

        let mut changed_status = ordinary(b"src/lib.rs");
        changed_status[2] = b'A';
        changed_status.push(0);
        let changed =
            parse_status_records(&changed_status, GitObjectFormat::Sha1, limits(1)).unwrap();
        assert_ne!(
            baseline,
            super::hash_worktree_state(&changed, SourceManifestDigest::new([0x44; 32]))
        );
        assert_eq!(RUST_WORKTREE_STATE_VERSION, 1);
        assert_eq!(GIT_STATUS_PROFILE_VERSION, 1);
    }

    #[test]
    fn status_parser_rejects_unknown_malformed_duplicate_and_over_limit_records() {
        let mut duplicate = ordinary(b"same.rs");
        duplicate.push(0);
        duplicate.extend_from_slice(b"? same.rs\0");
        assert!(matches!(
            parse_status_records(&duplicate, GitObjectFormat::Sha1, limits(2)),
            Err(SourceStateError::DuplicateStatusPath)
        ));

        for malformed in [
            b"2 R. N... malformed\0".as_slice(),
            b"# branch.head main\0",
            b"! ignored.rs\0",
            b"? /absolute.rs\0",
            b"? unterminated",
            b"? one.rs\0\0",
        ] {
            assert!(parse_status_records(malformed, GitObjectFormat::Sha1, limits(2)).is_err());
        }
        assert!(matches!(
            parse_status_records(b"? one.rs\0? two.rs\0", GitObjectFormat::Sha1, limits(1)),
            Err(SourceStateError::StatusPathLimitExceeded { limit: 1 })
        ));
    }

    #[test]
    fn status_parser_preserves_non_utf8_and_rejects_submodule_status() {
        let output = [b'?', b' ', b'n', b'o', b'n', b'-', 0xFF, 0];
        let parsed = parse_status_records(&output, GitObjectFormat::Sha1, limits(1)).unwrap();
        assert_eq!(parsed[0].path.as_bytes(), &output[2..7]);

        let submodule =
            format!("1 M. S.M. 160000 160000 160000 {SHA1_ONE} {SHA1_TWO} dependency\0");
        assert!(matches!(
            parse_status_records(submodule.as_bytes(), GitObjectFormat::Sha1, limits(1)),
            Err(SourceStateError::SubmoduleUnsupported)
        ));
        let hidden_gitlink =
            format!("1 M. N... 160000 160000 160000 {SHA1_ONE} {SHA1_TWO} dependency\0");
        assert!(matches!(
            parse_status_records(hidden_gitlink.as_bytes(), GitObjectFormat::Sha1, limits(1)),
            Err(SourceStateError::SubmoduleUnsupported)
        ));
        let invalid_mode =
            format!("1 M. N... 777777 100644 100644 {SHA1_ONE} {SHA1_TWO} source.rs\0");
        assert!(matches!(
            parse_status_records(invalid_mode.as_bytes(), GitObjectFormat::Sha1, limits(1)),
            Err(SourceStateError::InvalidStatusRecord)
        ));
    }

    #[test]
    fn diagnostics_and_debug_are_redacted() {
        let error = parse_status_records(
            b"? /secret/repository/path.rs\0",
            GitObjectFormat::Sha1,
            limits(1),
        )
        .unwrap_err();
        let display = error.to_string();
        let debug = format!("{error:?}");
        assert!(!display.contains("secret"));
        assert!(!debug.contains("secret"));

        let parsed = parse_status_records(b"? private.rs\0", GitObjectFormat::Sha1, limits(1))
            .expect("valid status");
        let capture = super::CapturedSourceState {
            git_state: hash_git_state(GitObjectFormat::Sha1, None, false),
            status_records: parsed,
        };
        let debug = format!("{capture:?}");
        assert!(!debug.contains("private.rs"));
        assert_eq!(capture.status_record_count(), 1);
    }

    #[test]
    fn real_capture_supports_unborn_symbolic_detached_and_linked_states() {
        let repository = TempRepository::new("sha1");
        repository.write("src/lib.rs", b"pub struct Visible;\n");

        let unborn =
            capture_source_state(repository.path(), limits(10)).expect("unborn state should work");
        assert_eq!(unborn.status_record_count(), 1);

        repository.commit_all("initial");
        let symbolic =
            capture_source_state(repository.path(), limits(10)).expect("symbolic HEAD should work");
        assert_ne!(symbolic.git_state(), unborn.git_state());
        assert_eq!(symbolic.status_record_count(), 0);

        repository.git(&["checkout", "--quiet", "--detach", "HEAD"]);
        let detached =
            capture_source_state(repository.path(), limits(10)).expect("detached HEAD should work");
        assert_eq!(detached, symbolic);

        let linked = TempDirectory::new("linked-worktree");
        repository.git(&[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            linked
                .path()
                .to_str()
                .expect("fixture path should be UTF-8"),
            "HEAD",
        ]);
        let linked_state =
            capture_source_state(linked.path(), limits(10)).expect("linked worktree should work");
        assert_eq!(linked_state, detached);
    }

    #[test]
    fn real_capture_supports_sha256_and_fails_closed_on_sparse_and_gitlinks() {
        let sha256 = TempRepository::new("sha256");
        sha256.write("lib.rs", b"pub fn sha256() {}\n");
        sha256.commit_all("sha256");
        capture_source_state(sha256.path(), limits(10)).expect("SHA-256 repository should work");

        let sparse = TempRepository::new("sha1");
        sparse.write("lib.rs", b"pub fn sparse() {}\n");
        sparse.commit_all("sparse");
        sparse.git(&["update-index", "--skip-worktree", "lib.rs"]);
        assert!(matches!(
            capture_source_state(sparse.path(), limits(10)),
            Err(SourceStateError::SparseWorktreeUnsupported)
        ));

        let gitlink = TempRepository::new("sha1");
        gitlink.write("lib.rs", b"pub fn gitlink() {}\n");
        gitlink.commit_all("gitlink");
        let head = gitlink.git_text(&["rev-parse", "HEAD"]);
        let cache_info = format!("160000,{head},dependency");
        gitlink.git(&["update-index", "--add", "--cacheinfo", &cache_info]);
        assert!(matches!(
            capture_source_state(gitlink.path(), limits(10)),
            Err(SourceStateError::SubmoduleUnsupported)
        ));
    }

    #[test]
    fn real_capture_distinguishes_shallow_history_at_the_same_head() {
        let origin = TempRepository::new("sha1");
        origin.write("lib.rs", b"pub fn first() {}\n");
        origin.commit_all("first");
        origin.write("lib.rs", b"pub fn second() {}\n");
        origin.commit_all("second");

        let shallow = TempDirectory::new("shallow-clone");
        let origin_url = format!(
            "file://{}",
            origin
                .path()
                .to_str()
                .expect("fixture path should be UTF-8")
        );
        let status = Command::new("git")
            .arg("--no-pager")
            .arg("clone")
            .arg("--quiet")
            .arg("--depth=1")
            .arg(origin_url)
            .arg(shallow.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", null_device())
            .env("GIT_CONFIG_SYSTEM", null_device())
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("fixture clone should start");
        assert!(status.success(), "fixture clone failed: {status}");

        let origin_state =
            capture_source_state(origin.path(), limits(10)).expect("origin should capture");
        let shallow_state =
            capture_source_state(shallow.path(), limits(10)).expect("shallow clone should capture");
        assert_ne!(origin_state.git_state(), shallow_state.git_state());
        assert_eq!(origin_state.status_record_count(), 0);
        assert_eq!(shallow_state.status_record_count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn real_capture_preserves_non_utf8_untracked_path_identity() {
        use std::os::unix::ffi::OsStringExt;

        let repository = TempRepository::new("sha1");
        let path = repository
            .path()
            .join(OsString::from_vec(b"non-utf8-\xFF.rs".to_vec()));
        fs::write(path, b"pub fn non_utf8() {}\n").expect("fixture file should be written");

        let capture = capture_source_state(repository.path(), limits(10))
            .expect("non-UTF-8 status path should capture");
        assert_eq!(capture.status_record_count(), 1);
    }

    #[test]
    fn real_capture_enforces_cancellation_output_and_path_bounds() {
        let repository = TempRepository::new("sha1");
        repository.write("one.rs", b"fn one() {}\n");
        repository.write("two.rs", b"fn two() {}\n");

        assert!(matches!(
            capture_source_state_with_cancel(repository.path(), limits(10), || true),
            Err(SourceStateError::Git {
                source: crate::GitPathDiscoveryError::Cancelled
            })
        ));

        let output_limited = GitPathDiscoveryLimits::new(
            Duration::from_secs(1),
            1,
            10,
            RepositoryPathLimits::new(1024, 32),
        );
        assert!(matches!(
            capture_source_state(repository.path(), output_limited),
            Err(SourceStateError::Git {
                source: crate::GitPathDiscoveryError::OutputByteLimitExceeded { limit: 1 }
            })
        ));
        assert!(matches!(
            capture_source_state(repository.path(), limits(1)),
            Err(SourceStateError::StatusPathLimitExceeded { limit: 1 })
        ));
    }
}
