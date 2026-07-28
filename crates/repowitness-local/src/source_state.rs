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
const SUPPORTED_LANGUAGES_WORKTREE_STATE_DOMAIN: &[u8] =
    b"RepoWitness\0supported-languages-worktree-state\0";

/// Version of the canonical Git-state encoding.
pub const GIT_STATE_VERSION: u32 = 1;
/// Version of the canonical Rust worktree-state encoding.
pub const RUST_WORKTREE_STATE_VERSION: u32 = 1;
/// Version of the canonical supported-language worktree-state encoding.
pub const SUPPORTED_LANGUAGES_WORKTREE_STATE_VERSION: u32 = 3;
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

    /// Hashes this status capture with a supported-language source manifest.
    #[must_use]
    pub fn source_worktree_state(&self, manifest: SourceManifestDigest) -> WorktreeStateDigest {
        hash_worktree_state_with_profile(
            &self.status_records,
            manifest,
            SUPPORTED_LANGUAGES_WORKTREE_STATE_DOMAIN,
            SUPPORTED_LANGUAGES_WORKTREE_STATE_VERSION,
        )
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

include!("source_state/parsing.rs");

#[cfg(test)]
mod tests;
