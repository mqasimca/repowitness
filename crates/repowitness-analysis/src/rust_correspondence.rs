use std::{error::Error, fmt};

use repowitness_domain::{
    AnalysisArtifactDigest, CorrespondenceFingerprintDigest, DeclarationDigest,
    MAX_MEMORY_INTEROPERABLE_INTEGER, RepositoryPath,
};
use sha2::{Digest, Sha256};

use crate::{RustSymbolFact, RustSymbolKind};

const NAME_ELIDED_DOMAIN: &[u8] = b"RepoWitness\0rust-correspondence\0name-elided\0v1\0";
const NAME_MARKER: &[u8] = b"<repowitness-symbol-name>";
const MAX_CORRESPONDENCE_NAME_BYTES: usize = 256;
const MAX_CORRESPONDENCE_QUALIFIED_NAME_BYTES: usize = 1_024;

/// Stable identifier for the first precision-first Rust occurrence profile.
pub const RUST_CORRESPONDENCE_PROFILE_ID: &str = "rust-name-elided";
/// Version of the first precision-first Rust occurrence profile.
pub const RUST_CORRESPONDENCE_PROFILE_VERSION: u32 = 1;
/// Maximum complete candidate set admitted for one evidence result.
pub const MAX_RUST_CORRESPONDENCE_CANDIDATES: usize = 16;

/// Returns exact first-party correspondence source bytes for producer
/// fingerprinting.
#[must_use]
pub fn rust_correspondence_implementation_fingerprint_input() -> &'static [u8] {
    include_bytes!("rust_correspondence.rs")
}

/// Exact and name-elided identities derived from one validated declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RustOccurrenceFingerprint {
    declaration: DeclarationDigest,
    name_elided: CorrespondenceFingerprintDigest,
}

impl RustOccurrenceFingerprint {
    /// Reconstructs one fingerprint from its exact persisted digest values.
    #[must_use]
    pub const fn new(
        declaration: DeclarationDigest,
        name_elided: CorrespondenceFingerprintDigest,
    ) -> Self {
        Self {
            declaration,
            name_elided,
        }
    }

    /// Returns the standard SHA-256 identity of exact declaration bytes.
    #[must_use]
    pub const fn declaration(self) -> DeclarationDigest {
        self.declaration
    }

    /// Returns the domain-separated name-elided identity.
    #[must_use]
    pub const fn name_elided(self) -> CorrespondenceFingerprintDigest {
        self.name_elided
    }
}

/// Trusted path-continuity evidence for one current candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustPathContinuity {
    /// No complete trusted path-continuity result supports this candidate.
    None,
    /// Sanitized Git proved one exact old-path to new-path move.
    GitExactMove,
}

/// Automatic Phase 0 relationship categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustAutomaticCorrespondence {
    /// The declared symbol name changed in the same path and container.
    Renamed,
    /// Exact declaration bytes moved along a Git-proven path mapping.
    Moved,
}

/// Why correspondence could not produce a complete categorical result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustCorrespondenceIndeterminateReason {
    /// Candidate enumeration exceeded its declared complete bound.
    CandidateOverflow,
    /// Historical source was unavailable for the subject fingerprint.
    MissingSubjectFingerprint,
}

/// Stable, content-redacted correspondence failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustCorrespondenceError {
    /// Source bytes, spans, names, or containers disagreed.
    InvalidOccurrence,
    /// A supplied candidate collection exceeded the hard input bound.
    CandidateLimitExceeded,
    /// Candidate count metadata was inconsistent.
    InvalidCandidateCount,
    /// The same exact target occurrence appeared more than once.
    DuplicateCandidate,
}

impl fmt::Display for RustCorrespondenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidOccurrence => "Rust correspondence occurrence is invalid",
            Self::CandidateLimitExceeded => "Rust correspondence candidate limit exceeded",
            Self::InvalidCandidateCount => "Rust correspondence candidate count is invalid",
            Self::DuplicateCandidate => "Rust correspondence candidate identity is duplicated",
        })
    }
}

impl Error for RustCorrespondenceError {}

/// Historical evidence reduced to the fields used by the Phase 0 profile.
#[derive(Clone, Eq, PartialEq)]
pub struct RustCorrespondenceSubject {
    path: RepositoryPath,
    kind: RustSymbolKind,
    name: Box<str>,
    qualified_name: Box<str>,
    container: Box<str>,
    declaration: DeclarationDigest,
    name_elided: Option<CorrespondenceFingerprintDigest>,
}

impl RustCorrespondenceSubject {
    /// Validates one exact historical subject and its optional derived
    /// fingerprint.
    #[allow(
        clippy::too_many_arguments,
        reason = "every exact occurrence identity input is semantic"
    )]
    pub fn try_new(
        path: RepositoryPath,
        kind: RustSymbolKind,
        name: String,
        qualified_name: String,
        declaration: DeclarationDigest,
        name_elided: Option<CorrespondenceFingerprintDigest>,
    ) -> Result<Self, RustCorrespondenceError> {
        let container = validate_names(&name, &qualified_name)?.to_owned();
        Ok(Self {
            path,
            kind,
            name: name.into_boxed_str(),
            qualified_name: qualified_name.into_boxed_str(),
            container: container.into_boxed_str(),
            declaration,
            name_elided,
        })
    }

    /// Returns the exact historical path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }
}

impl fmt::Debug for RustCorrespondenceSubject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustCorrespondenceSubject")
            .field("path", &self.path)
            .field("kind", &self.kind)
            .field("name", &"<redacted-symbol>")
            .field("qualified_name", &"<redacted-symbol>")
            .field("has_name_elided", &self.name_elided.is_some())
            .finish_non_exhaustive()
    }
}

/// One exact current occurrence considered by the Phase 0 profile.
#[derive(Clone, Eq, PartialEq)]
pub struct RustCorrespondenceCandidate {
    path: RepositoryPath,
    artifact: AnalysisArtifactDigest,
    fact_ordinal: u64,
    kind: RustSymbolKind,
    name: Box<str>,
    qualified_name: Box<str>,
    container: Box<str>,
    fingerprint: RustOccurrenceFingerprint,
    path_continuity: RustPathContinuity,
}

impl RustCorrespondenceCandidate {
    /// Constructs a candidate from one already validated syntax fact.
    pub fn try_from_fact(
        path: RepositoryPath,
        artifact: AnalysisArtifactDigest,
        fact_ordinal: u64,
        fact: &RustSymbolFact,
        path_continuity: RustPathContinuity,
    ) -> Result<Self, RustCorrespondenceError> {
        if fact_ordinal > MAX_MEMORY_INTEROPERABLE_INTEGER {
            return Err(RustCorrespondenceError::InvalidOccurrence);
        }
        let fingerprint = fact
            .correspondence()
            .ok_or(RustCorrespondenceError::InvalidOccurrence)?;
        let container = validate_names(fact.name(), fact.qualified_name())?;
        Ok(Self {
            path,
            artifact,
            fact_ordinal,
            kind: fact.kind(),
            name: fact.name().into(),
            qualified_name: fact.qualified_name().into(),
            container: container.into(),
            fingerprint,
            path_continuity,
        })
    }

    /// Returns the exact target path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the exact target artifact.
    #[must_use]
    pub const fn artifact(&self) -> AnalysisArtifactDigest {
        self.artifact
    }

    /// Returns the exact target fact ordinal.
    #[must_use]
    pub const fn fact_ordinal(&self) -> u64 {
        self.fact_ordinal
    }

    /// Returns the target syntax kind.
    #[must_use]
    pub const fn kind(&self) -> RustSymbolKind {
        self.kind
    }

    /// Returns the target name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the target qualified name.
    #[must_use]
    pub fn qualified_name(&self) -> &str {
        &self.qualified_name
    }

    /// Returns the derived target fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> RustOccurrenceFingerprint {
        self.fingerprint
    }

    /// Replaces path-continuity evidence after a bounded trusted history query.
    #[must_use]
    pub fn with_path_continuity(mut self, path_continuity: RustPathContinuity) -> Self {
        self.path_continuity = path_continuity;
        self
    }
}

impl fmt::Debug for RustCorrespondenceCandidate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustCorrespondenceCandidate")
            .field("path", &self.path)
            .field("artifact", &self.artifact)
            .field("fact_ordinal", &self.fact_ordinal)
            .field("kind", &self.kind)
            .field("name", &"<redacted-symbol>")
            .field("qualified_name", &"<redacted-symbol>")
            .field("path_continuity", &self.path_continuity)
            .finish_non_exhaustive()
    }
}

/// Categorical precision-first correspondence result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RustCorrespondenceResolution {
    /// The cited occurrence is unchanged in the target generation.
    Exact {
        /// Exact resolved target.
        target: RustCorrespondenceCandidate,
    },
    /// One configured high-assurance rule established correspondence.
    Automatic {
        /// Categorical relationship.
        relationship: RustAutomaticCorrespondence,
        /// Exact resolved target.
        target: RustCorrespondenceCandidate,
    },
    /// The same exact descriptor remains, but declaration semantics changed.
    Changed {
        /// Exact changed target.
        target: RustCorrespondenceCandidate,
    },
    /// Plausible targets require trusted review.
    NeedsReview {
        /// Deterministically ordered complete candidate set.
        candidates: Vec<RustCorrespondenceCandidate>,
    },
    /// Complete coverage found no plausible target.
    Missing,
    /// Missing source/history or overflow prevents a conclusion.
    Indeterminate {
        /// Stable reason category.
        reason: RustCorrespondenceIndeterminateReason,
    },
}

/// Derives the exact and name-elided identities for one source occurrence.
pub fn fingerprint_rust_occurrence(
    source: &[u8],
    fact: &RustSymbolFact,
) -> Result<RustOccurrenceFingerprint, RustCorrespondenceError> {
    let declaration_start = usize::try_from(fact.declaration_span().start().get())
        .map_err(|_| RustCorrespondenceError::InvalidOccurrence)?;
    let declaration_end = usize::try_from(fact.declaration_span().end().get())
        .map_err(|_| RustCorrespondenceError::InvalidOccurrence)?;
    let name_start = usize::try_from(fact.name_span().start().get())
        .map_err(|_| RustCorrespondenceError::InvalidOccurrence)?;
    let name_end = usize::try_from(fact.name_span().end().get())
        .map_err(|_| RustCorrespondenceError::InvalidOccurrence)?;
    let declaration = source
        .get(declaration_start..declaration_end)
        .ok_or(RustCorrespondenceError::InvalidOccurrence)?;
    if name_start < declaration_start
        || name_end > declaration_end
        || source.get(name_start..name_end) != Some(fact.name().as_bytes())
    {
        return Err(RustCorrespondenceError::InvalidOccurrence);
    }
    let container = validate_names(fact.name(), fact.qualified_name())?;

    let declaration_digest = DeclarationDigest::new(Sha256::digest(declaration).into());
    let mut hasher = Sha256::new();
    hasher.update(NAME_ELIDED_DOMAIN);
    update_length_prefixed(&mut hasher, fact.kind().as_str().as_bytes());
    update_length_prefixed(&mut hasher, container.as_bytes());
    update_length_prefixed(
        &mut hasher,
        source
            .get(declaration_start..name_start)
            .ok_or(RustCorrespondenceError::InvalidOccurrence)?,
    );
    update_length_prefixed(&mut hasher, NAME_MARKER);
    update_length_prefixed(
        &mut hasher,
        source
            .get(name_end..declaration_end)
            .ok_or(RustCorrespondenceError::InvalidOccurrence)?,
    );

    Ok(RustOccurrenceFingerprint {
        declaration: declaration_digest,
        name_elided: CorrespondenceFingerprintDigest::new(hasher.finalize().into()),
    })
}

/// Resolves one subject against a complete bounded target candidate set.
pub fn resolve_rust_correspondence(
    subject: &RustCorrespondenceSubject,
    candidates: &[RustCorrespondenceCandidate],
    candidate_count_before_limit: u64,
) -> Result<RustCorrespondenceResolution, RustCorrespondenceError> {
    if candidates.len() > MAX_RUST_CORRESPONDENCE_CANDIDATES {
        return Err(RustCorrespondenceError::CandidateLimitExceeded);
    }
    let supplied_count = u64::try_from(candidates.len())
        .map_err(|_| RustCorrespondenceError::InvalidCandidateCount)?;
    if candidate_count_before_limit < supplied_count {
        return Err(RustCorrespondenceError::InvalidCandidateCount);
    }

    let mut ordered = candidates.to_vec();
    ordered.sort_by(candidate_order);
    if ordered
        .windows(2)
        .any(|pair| same_candidate_identity(&pair[0], &pair[1]))
    {
        return Err(RustCorrespondenceError::DuplicateCandidate);
    }
    if candidate_count_before_limit > supplied_count {
        return Ok(RustCorrespondenceResolution::Indeterminate {
            reason: RustCorrespondenceIndeterminateReason::CandidateOverflow,
        });
    }

    let exact = matches(&ordered, |candidate| exact_match(subject, candidate));
    if let Some(resolution) = unique_or_review(exact, |target| {
        RustCorrespondenceResolution::Exact { target }
    }) {
        return Ok(resolution);
    }

    let renames = matches(&ordered, |candidate| rename_match(subject, candidate));
    let moves = matches(&ordered, |candidate| move_match(subject, candidate));
    let mut automatic = renames
        .into_iter()
        .map(|candidate| (RustAutomaticCorrespondence::Renamed, candidate))
        .chain(
            moves
                .into_iter()
                .map(|candidate| (RustAutomaticCorrespondence::Moved, candidate)),
        )
        .collect::<Vec<_>>();
    automatic.sort_by(|left, right| candidate_order(&left.1, &right.1));
    automatic.dedup_by(|left, right| same_candidate_identity(&left.1, &right.1));
    if automatic.len() == 1 {
        let (relationship, target) = automatic.pop().expect("one automatic target");
        return Ok(RustCorrespondenceResolution::Automatic {
            relationship,
            target,
        });
    }
    if automatic.len() > 1 {
        return Ok(RustCorrespondenceResolution::NeedsReview {
            candidates: automatic
                .into_iter()
                .map(|(_, candidate)| candidate)
                .collect(),
        });
    }

    let changed = matches(&ordered, |candidate| changed_match(subject, candidate));
    if let Some(resolution) = unique_or_review(changed, |target| {
        RustCorrespondenceResolution::Changed { target }
    }) {
        return Ok(resolution);
    }

    let plausible = matches(&ordered, |candidate| plausible_match(subject, candidate));
    if !plausible.is_empty() {
        return Ok(RustCorrespondenceResolution::NeedsReview {
            candidates: plausible,
        });
    }
    if subject.name_elided.is_none() {
        return Ok(RustCorrespondenceResolution::Indeterminate {
            reason: RustCorrespondenceIndeterminateReason::MissingSubjectFingerprint,
        });
    }
    Ok(RustCorrespondenceResolution::Missing)
}

fn matches(
    candidates: &[RustCorrespondenceCandidate],
    predicate: impl Fn(&RustCorrespondenceCandidate) -> bool,
) -> Vec<RustCorrespondenceCandidate> {
    candidates
        .iter()
        .filter(|candidate| predicate(candidate))
        .cloned()
        .collect()
}

fn unique_or_review(
    mut candidates: Vec<RustCorrespondenceCandidate>,
    unique: impl FnOnce(RustCorrespondenceCandidate) -> RustCorrespondenceResolution,
) -> Option<RustCorrespondenceResolution> {
    match candidates.len() {
        0 => None,
        1 => Some(unique(candidates.pop().expect("one candidate"))),
        _ => Some(RustCorrespondenceResolution::NeedsReview { candidates }),
    }
}

fn exact_match(
    subject: &RustCorrespondenceSubject,
    candidate: &RustCorrespondenceCandidate,
) -> bool {
    candidate.path == subject.path
        && candidate.kind == subject.kind
        && candidate.name == subject.name
        && candidate.qualified_name == subject.qualified_name
        && candidate.fingerprint.declaration == subject.declaration
}

fn rename_match(
    subject: &RustCorrespondenceSubject,
    candidate: &RustCorrespondenceCandidate,
) -> bool {
    candidate.path == subject.path
        && candidate.kind == subject.kind
        && candidate.container == subject.container
        && candidate.name != subject.name
        && subject
            .name_elided
            .is_some_and(|fingerprint| candidate.fingerprint.name_elided == fingerprint)
}

fn move_match(
    subject: &RustCorrespondenceSubject,
    candidate: &RustCorrespondenceCandidate,
) -> bool {
    candidate.path != subject.path
        && candidate.path_continuity == RustPathContinuity::GitExactMove
        && candidate.kind == subject.kind
        && candidate.name == subject.name
        && candidate.qualified_name == subject.qualified_name
        && candidate.fingerprint.declaration == subject.declaration
}

fn changed_match(
    subject: &RustCorrespondenceSubject,
    candidate: &RustCorrespondenceCandidate,
) -> bool {
    candidate.path == subject.path
        && candidate.kind == subject.kind
        && candidate.name == subject.name
        && candidate.qualified_name == subject.qualified_name
        && candidate.fingerprint.declaration != subject.declaration
}

fn plausible_match(
    subject: &RustCorrespondenceSubject,
    candidate: &RustCorrespondenceCandidate,
) -> bool {
    candidate.kind == subject.kind
        && (candidate.path == subject.path
            || candidate.container == subject.container
            || candidate.fingerprint.declaration == subject.declaration
            || subject
                .name_elided
                .is_some_and(|fingerprint| candidate.fingerprint.name_elided == fingerprint)
            || candidate.path_continuity == RustPathContinuity::GitExactMove)
}

fn validate_names<'a>(
    name: &str,
    qualified_name: &'a str,
) -> Result<&'a str, RustCorrespondenceError> {
    if name.is_empty()
        || name.len() > MAX_CORRESPONDENCE_NAME_BYTES
        || qualified_name.is_empty()
        || qualified_name.len() > MAX_CORRESPONDENCE_QUALIFIED_NAME_BYTES
        || name.chars().any(char::is_control)
        || qualified_name.chars().any(char::is_control)
    {
        return Err(RustCorrespondenceError::InvalidOccurrence);
    }
    if qualified_name == name {
        return Ok("");
    }
    qualified_name
        .strip_suffix(name)
        .and_then(|prefix| prefix.strip_suffix("::"))
        .filter(|container| !container.is_empty())
        .ok_or(RustCorrespondenceError::InvalidOccurrence)
}

fn update_length_prefixed(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(value);
}

fn candidate_order(
    left: &RustCorrespondenceCandidate,
    right: &RustCorrespondenceCandidate,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then_with(|| left.artifact.cmp(&right.artifact))
        .then_with(|| left.fact_ordinal.cmp(&right.fact_ordinal))
}

fn same_candidate_identity(
    left: &RustCorrespondenceCandidate,
    right: &RustCorrespondenceCandidate,
) -> bool {
    left.path == right.path
        && left.artifact == right.artifact
        && left.fact_ordinal == right.fact_ordinal
}

#[cfg(test)]
mod tests;
