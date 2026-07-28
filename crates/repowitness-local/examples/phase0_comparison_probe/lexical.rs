use std::{
    path::Path,
    time::{Duration, Instant},
};

use repowitness_domain::RepositoryPathLimits;
use repowitness_local::{
    ContainedSourceRoot, GitPathDiscoveryLimits, SourceReadLimits,
    discover_repository_paths_with_cancel,
};

use crate::ProbeResult;

const OLD_BEHAVIOR_LITERAL: &[u8] = b"skip(src, 4)";
const FIX_EVIDENCE_LITERAL: &[u8] = b"check_rejects_invalid_negative_bulk_length";
const TOTAL_DEADLINE_MS: u64 = 10_000;
const TOTAL_DEADLINE: Duration = Duration::from_millis(TOTAL_DEADLINE_MS);
const PER_FILE_DEADLINE: Duration = Duration::from_secs(2);
const MAX_DISCOVERY_OUTPUT_BYTES: u64 = 1024 * 1024;
const MAX_REPOSITORY_PATHS: u64 = 4_096;
const MAX_PATH_BYTES: u64 = 4_096;
const MAX_PATH_COMPONENTS: u64 = 256;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_TOTAL_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
const READ_CHUNK_BYTES: u64 = 64 * 1024;

#[cfg(test)]
pub const fn base_support_literal() -> &'static [u8] {
    OLD_BEHAVIOR_LITERAL
}

#[cfg(test)]
pub const fn changed_contradiction_literal() -> &'static [u8] {
    FIX_EVIDENCE_LITERAL
}

#[cfg(test)]
pub const fn max_paths() -> u64 {
    MAX_REPOSITORY_PATHS
}

#[cfg(test)]
pub const fn max_file_bytes() -> u64 {
    MAX_FILE_BYTES
}

#[cfg(test)]
pub const fn max_total_source_bytes() -> u64 {
    MAX_TOTAL_SOURCE_BYTES
}

#[cfg(test)]
pub const fn deadline_ms() -> u64 {
    TOTAL_DEADLINE_MS
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceRelation {
    Supports,
    Contradicts,
}

impl EvidenceRelation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supports => "supports",
            Self::Contradicts => "contradicts",
        }
    }
}

pub struct LexicalMetrics {
    relation: EvidenceRelation,
    old_behavior_matches: u64,
    fix_evidence_matches: u64,
    scanned_rust_files: u64,
    scanned_source_bytes: u64,
}

impl LexicalMetrics {
    pub const fn relation(&self) -> EvidenceRelation {
        self.relation
    }

    pub const fn old_behavior_matches(&self) -> u64 {
        self.old_behavior_matches
    }

    pub const fn fix_evidence_matches(&self) -> u64 {
        self.fix_evidence_matches
    }

    pub const fn scanned_rust_files(&self) -> u64 {
        self.scanned_rust_files
    }

    pub const fn scanned_source_bytes(&self) -> u64 {
        self.scanned_source_bytes
    }
}

pub fn observe(repository: &Path) -> ProbeResult<LexicalMetrics> {
    let deadline = Instant::now()
        .checked_add(TOTAL_DEADLINE)
        .ok_or("lexical baseline deadline is not representable")?;
    let discovery = discover_repository_paths_with_cancel(
        repository,
        GitPathDiscoveryLimits::new(
            TOTAL_DEADLINE,
            MAX_DISCOVERY_OUTPUT_BYTES,
            MAX_REPOSITORY_PATHS,
            RepositoryPathLimits::new(MAX_PATH_BYTES, MAX_PATH_COMPONENTS),
        ),
        || Instant::now() >= deadline,
    )?;
    let source = ContainedSourceRoot::open(repository)?;
    let mut old_behavior_matches = 0_u64;
    let mut fix_evidence_matches = 0_u64;
    let mut scanned_rust_files = 0_u64;
    let mut scanned_source_bytes = 0_u64;

    for path in discovery
        .paths()
        .iter()
        .filter(|path| path.as_bytes().ends_with(b".rs"))
    {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("lexical baseline deadline elapsed".into());
        }
        let bytes = source.read_unique_exact_with_cancel(
            path,
            SourceReadLimits::try_new(
                remaining.min(PER_FILE_DEADLINE),
                MAX_FILE_BYTES,
                READ_CHUNK_BYTES,
            )?,
            || Instant::now() >= deadline,
        )?;
        scanned_rust_files = scanned_rust_files
            .checked_add(1)
            .ok_or("lexical source-file count overflowed")?;
        scanned_source_bytes = scanned_source_bytes
            .checked_add(u64::try_from(bytes.len())?)
            .ok_or("lexical source-byte count overflowed")?;
        if scanned_source_bytes > MAX_TOTAL_SOURCE_BYTES {
            return Err("lexical baseline source-byte budget was exceeded".into());
        }
        old_behavior_matches = old_behavior_matches
            .checked_add(count_occurrences(&bytes, OLD_BEHAVIOR_LITERAL)?)
            .ok_or("lexical match count overflowed")?;
        fix_evidence_matches = fix_evidence_matches
            .checked_add(count_occurrences(&bytes, FIX_EVIDENCE_LITERAL)?)
            .ok_or("lexical match count overflowed")?;
    }

    let relation = classify(old_behavior_matches, fix_evidence_matches)?;
    Ok(LexicalMetrics {
        relation,
        old_behavior_matches,
        fix_evidence_matches,
        scanned_rust_files,
        scanned_source_bytes,
    })
}

fn classify(old_behavior_matches: u64, fix_evidence_matches: u64) -> ProbeResult<EvidenceRelation> {
    match (old_behavior_matches, fix_evidence_matches) {
        (1.., 0) => Ok(EvidenceRelation::Supports),
        (0, 1..) => Ok(EvidenceRelation::Contradicts),
        _ => Err("lexical evidence did not produce one categorical relation".into()),
    }
}

fn count_occurrences(haystack: &[u8], needle: &[u8]) -> ProbeResult<u64> {
    if needle.is_empty() {
        return Err("lexical query must not be empty".into());
    }
    let mut count = 0_u64;
    let mut remaining = haystack;
    while let Some(position) = remaining
        .windows(needle.len())
        .position(|window| window == needle)
    {
        count = count
            .checked_add(1)
            .ok_or("lexical match count overflowed")?;
        remaining = &remaining[position + needle.len()..];
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::{EvidenceRelation, classify, count_occurrences};

    #[test]
    fn evidence_signals_classify_before_and_after_without_guessing() {
        assert_eq!(
            classify(1, 0).expect("base relation"),
            EvidenceRelation::Supports
        );
        assert_eq!(
            classify(0, 1).expect("changed relation"),
            EvidenceRelation::Contradicts
        );
        assert!(classify(0, 0).is_err());
        assert!(classify(1, 1).is_err());
    }

    #[test]
    fn lexical_occurrences_are_non_overlapping_and_bounded() {
        assert_eq!(count_occurrences(b"abc abc abc", b"abc").expect("count"), 3);
        assert_eq!(count_occurrences(b"aaaa", b"aa").expect("count"), 2);
        assert!(count_occurrences(b"abc", b"").is_err());
    }
}
