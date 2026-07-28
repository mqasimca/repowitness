use std::{error::Error, fmt};

use sha2::{Digest, Sha256};

/// Version of the deterministic Phase 0 memory-recall profile.
pub const MEMORY_RECALL_PROFILE_VERSION: u16 = 1;
/// Default maximum number of returned projected records.
pub const DEFAULT_MEMORY_RECALL_RESULTS: u16 = 20;
/// Default conservative encoded-output allowance.
pub const DEFAULT_MEMORY_RECALL_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;
/// Default aggregate canonical-record read allowance.
pub const DEFAULT_MEMORY_RECALL_SCAN_BYTES: u64 = 8 * 1024 * 1024;
/// Hard result-count ceiling.
pub const MAX_MEMORY_RECALL_RESULTS: u16 = 100;
/// Hard conservative encoded-output ceiling.
pub const MAX_MEMORY_RECALL_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
/// Hard aggregate canonical-record read ceiling.
pub const MAX_MEMORY_RECALL_SCAN_BYTES: u64 = 32 * 1024 * 1024;

const MAX_QUERY_BYTES: usize = 256;
const MAX_QUERY_TERMS: usize = 8;
const MAX_TERM_BYTES: usize = 64;
const QUERY_HASH_DOMAIN: &[u8] = b"repowitness.memory-recall-query.v1\0";

/// Stable failure to admit an untrusted memory query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRecallQueryError {
    /// The caller supplied an empty query instead of selecting the explicit all-records mode.
    Empty,
    /// The complete input exceeds the byte ceiling.
    QueryTooLong,
    /// The input contains too many literal terms.
    TooManyTerms,
    /// At least one literal term exceeds its byte ceiling.
    TermTooLong,
    /// At least one literal term contains a control character.
    InvalidTerm,
}

impl fmt::Display for MemoryRecallQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "memory-recall query must contain at least one term",
            Self::QueryTooLong => "memory-recall query exceeds the byte limit",
            Self::TooManyTerms => "memory-recall query exceeds the term-count limit",
            Self::TermTooLong => "memory-recall query term exceeds the byte limit",
            Self::InvalidTerm => "memory-recall query term contains an invalid character",
        })
    }
}

impl Error for MemoryRecallQueryError {}

/// SHA-256 identity for one canonical admitted query.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MemoryRecallQueryDigest([u8; 32]);

impl MemoryRecallQueryDigest {
    /// Returns the exact digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for MemoryRecallQueryDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryRecallQueryDigest")
            .field("algorithm", &"SHA-256")
            .finish_non_exhaustive()
    }
}

/// Explicit all-records mode or validated canonical literal terms.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryRecallQuery {
    canonical: Option<String>,
    digest: Option<MemoryRecallQueryDigest>,
    term_count: u8,
}

impl MemoryRecallQuery {
    /// Selects all projected records, subject to result and output bounds.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            canonical: None,
            digest: None,
            term_count: 0,
        }
    }

    /// Validates, ASCII-folds, and canonicalizes literal query terms.
    pub fn try_new(input: &str) -> Result<Self, MemoryRecallQueryError> {
        if input.len() > MAX_QUERY_BYTES {
            return Err(MemoryRecallQueryError::QueryTooLong);
        }
        let terms = input.split_whitespace().collect::<Vec<_>>();
        if terms.is_empty() {
            return Err(MemoryRecallQueryError::Empty);
        }
        if terms.len() > MAX_QUERY_TERMS {
            return Err(MemoryRecallQueryError::TooManyTerms);
        }
        if terms.iter().any(|term| term.len() > MAX_TERM_BYTES) {
            return Err(MemoryRecallQueryError::TermTooLong);
        }
        if terms.iter().any(|term| term.chars().any(char::is_control)) {
            return Err(MemoryRecallQueryError::InvalidTerm);
        }

        let canonical = terms
            .iter()
            .map(|term| term.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        let mut hasher = Sha256::new();
        hasher.update(QUERY_HASH_DOMAIN);
        for term in canonical.split(' ') {
            let byte_count =
                u16::try_from(term.len()).expect("validated memory query term fits in u16");
            hasher.update(byte_count.to_be_bytes());
            hasher.update(term.as_bytes());
        }
        Ok(Self {
            canonical: Some(canonical),
            digest: Some(MemoryRecallQueryDigest(hasher.finalize().into())),
            term_count: u8::try_from(terms.len())
                .expect("validated memory query term count fits in u8"),
        })
    }

    /// Returns canonical literal terms, or `None` for the explicit all-records mode.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        self.canonical.as_deref()
    }

    /// Returns the canonical query identity, or `None` for all-records mode.
    #[must_use]
    pub const fn digest(&self) -> Option<MemoryRecallQueryDigest> {
        self.digest
    }

    /// Returns the admitted term count.
    #[must_use]
    pub const fn term_count(&self) -> u8 {
        self.term_count
    }
}

impl fmt::Debug for MemoryRecallQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryRecallQuery")
            .field("all_records", &self.canonical.is_none())
            .field("term_count", &self.term_count)
            .field("digest", &self.digest)
            .field("text", &"<redacted-query>")
            .finish()
    }
}

/// Stable failure to construct recall bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryRecallLimitError;

impl fmt::Display for MemoryRecallLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("memory-recall limits are zero or exceed Phase 0 ceilings")
    }
}

impl Error for MemoryRecallLimitError {}

/// Inclusive result, conservative output, and canonical-input bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryRecallLimits {
    max_results: u16,
    max_output_bytes: u64,
    max_scan_bytes: u64,
}

impl MemoryRecallLimits {
    /// Validates all independent recall resource limits.
    pub const fn try_new(
        max_results: u16,
        max_output_bytes: u64,
        max_scan_bytes: u64,
    ) -> Result<Self, MemoryRecallLimitError> {
        if max_results == 0
            || max_results > MAX_MEMORY_RECALL_RESULTS
            || max_output_bytes == 0
            || max_output_bytes > MAX_MEMORY_RECALL_OUTPUT_BYTES
            || max_scan_bytes == 0
            || max_scan_bytes > MAX_MEMORY_RECALL_SCAN_BYTES
        {
            return Err(MemoryRecallLimitError);
        }
        Ok(Self {
            max_results,
            max_output_bytes,
            max_scan_bytes,
        })
    }

    /// Returns the inclusive projected-record limit.
    #[must_use]
    pub const fn max_results(self) -> u16 {
        self.max_results
    }

    /// Returns the inclusive conservative encoded-output limit.
    #[must_use]
    pub const fn max_output_bytes(self) -> u64 {
        self.max_output_bytes
    }

    /// Returns the inclusive canonical-record read limit.
    #[must_use]
    pub const fn max_scan_bytes(self) -> u64 {
        self.max_scan_bytes
    }
}

impl Default for MemoryRecallLimits {
    fn default() -> Self {
        Self {
            max_results: DEFAULT_MEMORY_RECALL_RESULTS,
            max_output_bytes: DEFAULT_MEMORY_RECALL_OUTPUT_BYTES,
            max_scan_bytes: DEFAULT_MEMORY_RECALL_SCAN_BYTES,
        }
    }
}
