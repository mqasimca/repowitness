//! Validated, redacted source-selector values for caller-provided worktrees.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::git_paths::GitPathDiscoveryError;

mod git;

const FULL_REF_DIGEST_DOMAIN: &[u8] = b"RepoWitness\0source-selector-full-ref\0v1\0";

/// Inclusive UTF-8 byte bound for one version-1 source selector.
pub(crate) const MAX_SOURCE_SELECTOR_BYTES: usize = 1_024;
/// Default wall-clock bound for one selector resolution.
pub(crate) const DEFAULT_SOURCE_SELECTOR_DEADLINE: Duration = Duration::from_secs(30);
/// Default stdout bound for each sanitized selector Git subprocess.
pub(crate) const DEFAULT_SOURCE_SELECTOR_OUTPUT_BYTES: u64 = 256;

/// Resource bounds for one source-selector resolution or final fence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SourceSelectorLimits {
    deadline: Duration,
    output_bytes: u64,
}

impl SourceSelectorLimits {
    /// Creates explicit selector subprocess and time bounds.
    pub(crate) const fn new(deadline: Duration, output_bytes: u64) -> Self {
        Self {
            deadline,
            output_bytes,
        }
    }

    /// Returns the inclusive wall-clock duration.
    pub(crate) const fn deadline(self) -> Duration {
        self.deadline
    }

    /// Returns the inclusive stdout byte bound for each subprocess.
    pub(crate) const fn output_bytes(self) -> u64 {
        self.output_bytes
    }
}

impl Default for SourceSelectorLimits {
    fn default() -> Self {
        Self::new(
            DEFAULT_SOURCE_SELECTOR_DEADLINE,
            DEFAULT_SOURCE_SELECTOR_OUTPUT_BYTES,
        )
    }
}

/// Stable category for one admitted version-1 selector.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum SourceSelectorCategory {
    /// The caller-provided worktree's concrete `HEAD`.
    WorktreeHead,
    /// One exact full-width Git object ID.
    ExactRevision,
    /// One moving, fully-qualified, allow-listed Git ref.
    FullRef,
}

/// One full Git commit object ID resolved by the selector adapter.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) enum SourceSelectorCommit {
    /// Raw 20-byte SHA-1 commit identity.
    Sha1([u8; 20]),
    /// Raw 32-byte SHA-256 commit identity.
    Sha256([u8; 32]),
}

impl SourceSelectorCommit {
    /// Returns the repository object format carried with the object ID.
    pub(crate) const fn object_format(self) -> SourceSelectorObjectFormat {
        match self {
            Self::Sha1(_) => SourceSelectorObjectFormat::Sha1,
            Self::Sha256(_) => SourceSelectorObjectFormat::Sha256,
        }
    }

    /// Returns the exact decoded object-ID bytes.
    pub(crate) fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Sha1(bytes) => bytes,
            Self::Sha256(bytes) => bytes,
        }
    }

    fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let bytes = self.as_bytes();
        let mut text = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            text.push(char::from(HEX[usize::from(byte >> 4)]));
            text.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        text
    }
}

impl fmt::Debug for SourceSelectorCommit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourceSelectorCommit")
            .field("object_format", &self.object_format())
            .field("identity_bytes", &self.as_bytes().len())
            .finish_non_exhaustive()
    }
}

/// Git object format typed with a resolved source-selector commit.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum SourceSelectorObjectFormat {
    /// SHA-1 object format.
    Sha1,
    /// SHA-256 object format.
    Sha256,
}

impl SourceSelectorObjectFormat {
    const fn object_id_bytes(self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
        }
    }
}

#[derive(Clone, Eq, Hash, PartialEq)]
struct FullRef(Box<str>);

impl FullRef {
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for FullRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FullRef")
            .field("text", &"<redacted-selector>")
            .field("bytes", &self.0.len())
            .finish()
    }
}

/// One admitted version-1 source selector.
#[derive(Clone, Eq, Hash, PartialEq)]
pub(crate) struct SourceSelectorV1(SourceSelectorKind);

#[derive(Clone, Eq, Hash, PartialEq)]
enum SourceSelectorKind {
    WorktreeHead,
    ExactRevision(SourceSelectorCommit),
    FullRef(FullRef),
}

impl SourceSelectorV1 {
    /// Admits one UTF-8 selector without consulting Git.
    pub(crate) fn parse(text: &str) -> Result<Self, SourceSelectorAdmissionError> {
        validate_text_boundary(text)?;
        if text == "worktree-head" {
            return Ok(Self(SourceSelectorKind::WorktreeHead));
        }
        if is_allow_listed_ref(text) {
            return Ok(Self(SourceSelectorKind::FullRef(FullRef(text.into()))));
        }
        if matches!(text.len(), 40 | 64) {
            return decode_exact_revision(text)
                .map(SourceSelectorKind::ExactRevision)
                .map(Self)
                .ok_or(SourceSelectorAdmissionError::InvalidExactRevision);
        }
        Err(SourceSelectorAdmissionError::UnsupportedCategory)
    }

    /// Returns the stable selector category without exposing selector text.
    pub(crate) const fn category(&self) -> SourceSelectorCategory {
        match &self.0 {
            SourceSelectorKind::WorktreeHead => SourceSelectorCategory::WorktreeHead,
            SourceSelectorKind::ExactRevision(_) => SourceSelectorCategory::ExactRevision,
            SourceSelectorKind::FullRef(_) => SourceSelectorCategory::FullRef,
        }
    }

    const fn kind(&self) -> &SourceSelectorKind {
        &self.0
    }

    fn moving_ref_digest(&self) -> Option<FullRefDigest> {
        let SourceSelectorKind::FullRef(reference) = &self.0 else {
            return None;
        };
        let mut hasher = Sha256::new();
        hasher.update(FULL_REF_DIGEST_DOMAIN);
        hasher.update(
            u64::try_from(reference.0.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hasher.update(reference.0.as_bytes());
        Some(FullRefDigest(hasher.finalize().into()))
    }
}

impl fmt::Debug for SourceSelectorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = match &self.0 {
            SourceSelectorKind::WorktreeHead => "worktree-head".len(),
            SourceSelectorKind::ExactRevision(commit) => commit.as_bytes().len() * 2,
            SourceSelectorKind::FullRef(reference) => reference.0.len(),
        };
        formatter
            .debug_struct("SourceSelectorV1")
            .field("category", &self.category())
            .field("bytes", &bytes)
            .field("text", &"<redacted-selector>")
            .finish()
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct FullRefDigest([u8; 32]);

impl fmt::Debug for FullRefDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("FullRefDigest(<redacted-digest>)")
    }
}

/// A resolved selector bound to one canonical caller worktree.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct ResolvedSourceSelector {
    worktree_root: PathBuf,
    selector: SourceSelectorV1,
    commit: SourceSelectorCommit,
    moving_ref_digest: Option<FullRefDigest>,
}

impl ResolvedSourceSelector {
    /// Returns the canonical worktree capability selected during resolution.
    pub(crate) fn worktree_root(&self) -> &Path {
        &self.worktree_root
    }

    /// Returns the selector category.
    #[cfg(test)]
    pub(crate) const fn category(&self) -> SourceSelectorCategory {
        self.selector.category()
    }

    /// Returns the concrete commit selected in the caller worktree.
    #[cfg(test)]
    pub(crate) const fn commit(&self) -> SourceSelectorCommit {
        self.commit
    }

    /// Returns the digest of a moving full-ref selector, when applicable.
    #[cfg(test)]
    pub(crate) fn moving_ref_digest(&self) -> Option<[u8; 32]> {
        self.moving_ref_digest.map(|digest| digest.0)
    }

    /// Confirms that the worktree `HEAD` and selector still resolve identically.
    pub(crate) fn confirm(
        &self,
        limits: SourceSelectorLimits,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<(), SourceSelectorFinalFenceError> {
        git::confirm_source_selector(self, limits, cancelled, deadline)
    }
}

impl fmt::Debug for ResolvedSourceSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedSourceSelector")
            .field("worktree_root", &"<redacted-path>")
            .field("selector", &self.selector)
            .field("commit", &self.commit)
            .field("moving_ref_digest", &self.moving_ref_digest)
            .finish_non_exhaustive()
    }
}

/// Stable, content-redacted selector admission failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourceSelectorAdmissionError {
    /// The selector is empty.
    Empty,
    /// The selector exceeds the version-1 UTF-8 byte bound.
    ByteLimitExceeded {
        /// Inclusive byte bound.
        limit: usize,
    },
    /// The selector contains NUL or a Unicode control character.
    ControlCharacter,
    /// The selector does not belong to an allowed category.
    UnsupportedCategory,
    /// A full-width exact revision contains a non-hexadecimal character.
    InvalidExactRevision,
}

impl fmt::Display for SourceSelectorAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("source selector is empty"),
            Self::ByteLimitExceeded { limit } => {
                write!(formatter, "source selector exceeds its {limit} byte bound")
            }
            Self::ControlCharacter => {
                formatter.write_str("source selector contains a control character")
            }
            Self::UnsupportedCategory => {
                formatter.write_str("source selector category is not supported")
            }
            Self::InvalidExactRevision => {
                formatter.write_str("exact source revision is not full-width hexadecimal")
            }
        }
    }
}

impl Error for SourceSelectorAdmissionError {}

/// Stable, path-, selector-, output-, and object-ID-redacted resolution failure.
#[derive(Debug)]
pub(crate) enum SourceSelectorResolutionError {
    /// The monotonic deadline cannot represent the requested duration.
    DeadlineNotRepresentable,
    /// Resolution was cancelled.
    Cancelled,
    /// Resolution exceeded its declared deadline.
    DeadlineExceeded {
        /// Configured wall-clock duration.
        deadline: Duration,
    },
    /// A sanitized Git subprocess or worktree discovery step failed.
    Git {
        /// Redacted subprocess failure.
        source: GitPathDiscoveryError,
    },
    /// Git reported an unsupported object format.
    UnsupportedObjectFormat,
    /// Git returned malformed object-format output.
    InvalidObjectFormat,
    /// The caller worktree has no concrete commit at `HEAD`.
    HeadUnavailable,
    /// Git returned malformed `HEAD` output.
    InvalidHead,
    /// An exact revision's width does not match the repository object format.
    ExactRevisionObjectFormatMismatch,
    /// Git rejected a full ref under an allow-listed namespace.
    InvalidFullRef,
    /// The selector does not resolve to a commit.
    SelectorUnavailable,
    /// Git returned malformed selector-resolution output.
    InvalidSelectorResolution,
    /// The caller worktree `HEAD` does not equal the selected commit.
    WorktreeHeadMismatch,
}

impl fmt::Display for SourceSelectorResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeadlineNotRepresentable => {
                formatter.write_str("source-selector deadline cannot be represented")
            }
            Self::Cancelled => formatter.write_str("source-selector resolution was cancelled"),
            Self::DeadlineExceeded { deadline } => write!(
                formatter,
                "source-selector resolution exceeded its {} millisecond deadline",
                deadline.as_millis()
            ),
            Self::Git { .. } => formatter.write_str("sanitized Git selector resolution failed"),
            Self::UnsupportedObjectFormat => {
                formatter.write_str("Git object format is not supported")
            }
            Self::InvalidObjectFormat => {
                formatter.write_str("Git returned an invalid object format")
            }
            Self::HeadUnavailable => {
                formatter.write_str("caller worktree has no concrete HEAD commit")
            }
            Self::InvalidHead => formatter.write_str("Git returned an invalid HEAD commit"),
            Self::ExactRevisionObjectFormatMismatch => {
                formatter.write_str("exact revision does not match the Git object format")
            }
            Self::InvalidFullRef => formatter.write_str("Git rejected the full source ref"),
            Self::SelectorUnavailable => {
                formatter.write_str("source selector does not resolve to a commit")
            }
            Self::InvalidSelectorResolution => {
                formatter.write_str("Git returned an invalid selector resolution")
            }
            Self::WorktreeHeadMismatch => {
                formatter.write_str("caller worktree HEAD does not match the source selector")
            }
        }
    }
}

impl Error for SourceSelectorResolutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Git { source } => Some(source),
            _ => None,
        }
    }
}

impl From<GitPathDiscoveryError> for SourceSelectorResolutionError {
    fn from(source: GitPathDiscoveryError) -> Self {
        match source {
            GitPathDiscoveryError::DeadlineNotRepresentable => Self::DeadlineNotRepresentable,
            GitPathDiscoveryError::Cancelled => Self::Cancelled,
            GitPathDiscoveryError::DeadlineExceeded { deadline } => {
                Self::DeadlineExceeded { deadline }
            }
            source => Self::Git { source },
        }
    }
}

/// Stable, redacted selector final-fence failure.
#[derive(Debug)]
pub(crate) enum SourceSelectorFinalFenceError {
    /// Final confirmation was cancelled.
    Cancelled,
    /// Final confirmation exceeded its declared deadline.
    DeadlineExceeded {
        /// Configured wall-clock duration.
        deadline: Duration,
    },
    /// The worktree or selector no longer matches the captured resolution.
    SourceChanged,
    /// The selector could not be inspected safely.
    Inspection {
        /// Redacted resolution failure.
        source: SourceSelectorResolutionError,
    },
}

impl fmt::Display for SourceSelectorFinalFenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("source-selector final fence was cancelled"),
            Self::DeadlineExceeded { deadline } => write!(
                formatter,
                "source-selector final fence exceeded its {} millisecond deadline",
                deadline.as_millis()
            ),
            Self::SourceChanged => {
                formatter.write_str("source selector changed during reconciliation")
            }
            Self::Inspection { .. } => {
                formatter.write_str("source-selector final inspection failed")
            }
        }
    }
}

impl Error for SourceSelectorFinalFenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Inspection { source } => Some(source),
            _ => None,
        }
    }
}

/// Resolves a selector in one caller-provided worktree.
#[cfg(test)]
pub(crate) fn resolve_source_selector(
    root: &Path,
    selector: SourceSelectorV1,
    limits: SourceSelectorLimits,
    cancelled: &AtomicBool,
) -> Result<ResolvedSourceSelector, SourceSelectorResolutionError> {
    git::resolve_source_selector(root, selector, limits, cancelled)
}

/// Resolves a selector under a caller-owned absolute operation deadline.
pub(crate) fn resolve_source_selector_until(
    root: &Path,
    selector: SourceSelectorV1,
    limits: SourceSelectorLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<ResolvedSourceSelector, SourceSelectorResolutionError> {
    git::resolve_until(root, selector, limits, cancelled, deadline)
}

fn validate_text_boundary(text: &str) -> Result<(), SourceSelectorAdmissionError> {
    if text.is_empty() {
        return Err(SourceSelectorAdmissionError::Empty);
    }
    if text.len() > MAX_SOURCE_SELECTOR_BYTES {
        return Err(SourceSelectorAdmissionError::ByteLimitExceeded {
            limit: MAX_SOURCE_SELECTOR_BYTES,
        });
    }
    if text.chars().any(char::is_control) {
        return Err(SourceSelectorAdmissionError::ControlCharacter);
    }
    Ok(())
}

fn is_allow_listed_ref(text: &str) -> bool {
    ["refs/heads/", "refs/tags/", "refs/remotes/"]
        .iter()
        .any(|prefix| {
            text.strip_prefix(prefix)
                .is_some_and(|suffix| !suffix.is_empty())
        })
}

fn decode_exact_revision(text: &str) -> Option<SourceSelectorCommit> {
    match text.len() {
        40 => {
            let mut bytes = [0_u8; 20];
            decode_hex_into(text.as_bytes(), &mut bytes)?;
            Some(SourceSelectorCommit::Sha1(bytes))
        }
        64 => {
            let mut bytes = [0_u8; 32];
            decode_hex_into(text.as_bytes(), &mut bytes)?;
            Some(SourceSelectorCommit::Sha256(bytes))
        }
        _ => None,
    }
}

fn decode_hex_into(text: &[u8], output: &mut [u8]) -> Option<()> {
    if text.len() != output.len() * 2 {
        return None;
    }
    for (pair, byte) in text.chunks_exact(2).zip(output) {
        *byte = (decode_hex_digit(pair[0])? << 4) | decode_hex_digit(pair[1])?;
    }
    Some(())
}

const fn decode_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
