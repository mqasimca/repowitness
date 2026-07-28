use std::fmt;

use repowitness_domain::{
    AnalysisArtifactDigest, CorrespondenceFingerprintDigest, CorrespondenceProfileDigest,
    DeclarationDigest, RepositoryPath, SourceContentDigest,
};

use super::port::MemoryRecallPortOutputError;

const MAX_CORRESPONDENCE_ID_BYTES: usize = 128;
const MAX_CANDIDATES_PER_EVIDENCE: usize = 16;

/// Stable correspondence producer attribution for a projection.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryRecallProducer {
    id: Box<str>,
    version: u32,
    digest: CorrespondenceProfileDigest,
}

impl MemoryRecallProducer {
    /// Validates producer identity before it crosses the application boundary.
    pub fn try_new(
        id: String,
        version: u32,
        digest: CorrespondenceProfileDigest,
    ) -> Result<Self, MemoryRecallPortOutputError> {
        if id.is_empty()
            || id.len() > MAX_CORRESPONDENCE_ID_BYTES
            || !id.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
            || version == 0
        {
            return Err(MemoryRecallPortOutputError::InvalidProducer);
        }
        Ok(Self {
            id: id.into_boxed_str(),
            version,
            digest,
        })
    }

    /// Returns the printable correspondence-profile identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the positive correspondence-profile version.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// Returns the complete correspondence-profile digest.
    #[must_use]
    pub const fn digest(&self) -> CorrespondenceProfileDigest {
        self.digest
    }
}

impl fmt::Debug for MemoryRecallProducer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryRecallProducer")
            .field("id", &self.id)
            .field("version", &self.version)
            .field("digest", &self.digest)
            .finish()
    }
}

/// Persisted projection evidence state, including head-selection states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRecallEvidenceState {
    /// Every citation remained exact.
    Exact,
    /// Every citation resolved and at least one used correspondence.
    Corresponded,
    /// At least one exact descriptor changed.
    Changed,
    /// At least one citation requires review.
    Ambiguous,
    /// At least one citation is absent.
    Missing,
    /// Evidence coverage was incomplete.
    Indeterminate,
    /// Approved head selection conflicted before evidence evaluation.
    Conflicted,
    /// Evidence was not evaluated.
    NotEvaluated,
}

/// Stable persisted reason for one recalled effective state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRecallReason {
    /// Every citation remained exact.
    EvidenceExact,
    /// At least one citation resolved through correspondence.
    EvidenceCorresponded,
    /// At least one exact descriptor changed.
    EvidenceChanged,
    /// At least one citation requires review.
    EvidenceAmbiguous,
    /// At least one citation is absent.
    EvidenceMissing,
    /// Evidence coverage was incomplete.
    EvidenceIndeterminate,
    /// Project validity excludes the version.
    ProjectNotApplicable,
    /// Project validity could not be established.
    ProjectIndeterminate,
    /// The authored lifecycle requires review.
    AuthoredNeedsReview,
    /// The authored lifecycle is stale.
    AuthoredStale,
    /// The authored lifecycle is contradicted.
    AuthoredContradicted,
    /// The authored lifecycle is superseded.
    AuthoredSuperseded,
    /// The authored lifecycle is quarantined.
    AuthoredQuarantined,
    /// The authored lifecycle is tombstoned.
    AuthoredTombstoned,
    /// Multiple approved immutable heads exist.
    ApprovedHeadConflict,
    /// At least one parent revision was unavailable.
    MissingParent,
    /// The approved version graph violated head-selection invariants.
    InvalidHeadGraph,
}

/// Exact occurrence identity produced by current-memory correspondence.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryRecallOccurrence {
    path: RepositoryPath,
    content_digest: SourceContentDigest,
    artifact_digest: AnalysisArtifactDigest,
    fact_ordinal: u64,
    declaration_digest: DeclarationDigest,
    name_elided_digest: CorrespondenceFingerprintDigest,
}

impl MemoryRecallOccurrence {
    /// Constructs one occurrence from validated identity components.
    #[must_use]
    pub const fn new(
        path: RepositoryPath,
        content_digest: SourceContentDigest,
        artifact_digest: AnalysisArtifactDigest,
        fact_ordinal: u64,
        declaration_digest: DeclarationDigest,
        name_elided_digest: CorrespondenceFingerprintDigest,
    ) -> Self {
        Self {
            path,
            content_digest,
            artifact_digest,
            fact_ordinal,
            declaration_digest,
            name_elided_digest,
        }
    }

    /// Returns the exact repository-relative target path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the exact target source-content digest.
    #[must_use]
    pub const fn content_digest(&self) -> SourceContentDigest {
        self.content_digest
    }

    /// Returns the exact target analysis-artifact digest.
    #[must_use]
    pub const fn artifact_digest(&self) -> AnalysisArtifactDigest {
        self.artifact_digest
    }

    /// Returns the deterministic target fact ordinal.
    #[must_use]
    pub const fn fact_ordinal(&self) -> u64 {
        self.fact_ordinal
    }

    /// Returns the target declaration digest.
    #[must_use]
    pub const fn declaration_digest(&self) -> DeclarationDigest {
        self.declaration_digest
    }

    /// Returns the target name-elided correspondence fingerprint.
    #[must_use]
    pub const fn name_elided_digest(&self) -> CorrespondenceFingerprintDigest {
        self.name_elided_digest
    }
}

impl fmt::Debug for MemoryRecallOccurrence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryRecallOccurrence")
            .field("path", &self.path)
            .field("content_digest", &self.content_digest)
            .field("artifact_digest", &self.artifact_digest)
            .field("fact_ordinal", &self.fact_ordinal)
            .finish_non_exhaustive()
    }
}

/// Proposed relation for one ambiguous correspondence candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRecallCandidateRelation {
    /// The exact occurrence remains.
    Same,
    /// The occurrence moved without a rename.
    Moved,
    /// The occurrence was renamed at the same path.
    Renamed,
    /// The occurrence moved and was renamed.
    MovedRenamed,
    /// Review may classify the source as split.
    Split,
    /// Review may classify the source as merged.
    Merged,
}

/// One exact review-required correspondence candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryRecallCandidate {
    occurrence: MemoryRecallOccurrence,
    relation: MemoryRecallCandidateRelation,
}

impl MemoryRecallCandidate {
    /// Constructs an attributed review candidate.
    #[must_use]
    pub const fn new(
        occurrence: MemoryRecallOccurrence,
        relation: MemoryRecallCandidateRelation,
    ) -> Self {
        Self {
            occurrence,
            relation,
        }
    }

    /// Returns the candidate target occurrence.
    #[must_use]
    pub const fn occurrence(&self) -> &MemoryRecallOccurrence {
        &self.occurrence
    }

    /// Returns the proposed categorical relation.
    #[must_use]
    pub const fn relation(&self) -> MemoryRecallCandidateRelation {
        self.relation
    }
}

/// Per-citation correspondence outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRecallEvidenceOutcome {
    /// The exact occurrence remains.
    Exact,
    /// A same-path rename rule resolved the citation.
    SamePathRename,
    /// Git path continuity resolved an exact move.
    GitExactMove,
    /// An explicit trusted review linked the historical occurrence to this target.
    ReviewedLink,
    /// The exact descriptor remains but declaration semantics changed.
    Changed,
    /// Complete bounded candidates require review.
    Ambiguous,
    /// Complete coverage found no occurrence.
    Missing,
    /// Incomplete evidence prevents a conclusion.
    Indeterminate,
}

/// Assurance attached to one correspondence outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRecallEvidenceAssurance {
    /// A deterministic high-assurance rule resolved the citation.
    Automatic,
    /// A trusted manual review resolved the citation.
    Reviewed,
    /// The outcome does not establish correspondence.
    None,
}

/// One projected evidence result with bounded ambiguity candidates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryRecallEvidence {
    outcome: MemoryRecallEvidenceOutcome,
    assurance: MemoryRecallEvidenceAssurance,
    target: Option<MemoryRecallOccurrence>,
    candidate_coverage_complete: bool,
    candidate_count_before_limit: u64,
    candidates: Box<[MemoryRecallCandidate]>,
}

impl MemoryRecallEvidence {
    /// Validates one storage-neutral projected evidence result.
    pub fn try_new(
        outcome: MemoryRecallEvidenceOutcome,
        assurance: MemoryRecallEvidenceAssurance,
        target: Option<MemoryRecallOccurrence>,
        candidate_coverage_complete: bool,
        candidate_count_before_limit: u64,
        candidates: Vec<MemoryRecallCandidate>,
    ) -> Result<Self, MemoryRecallPortOutputError> {
        if candidates.len() > MAX_CANDIDATES_PER_EVIDENCE {
            return Err(MemoryRecallPortOutputError::InvalidEvidence);
        }
        let resolved = matches!(
            outcome,
            MemoryRecallEvidenceOutcome::Exact
                | MemoryRecallEvidenceOutcome::SamePathRename
                | MemoryRecallEvidenceOutcome::GitExactMove
                | MemoryRecallEvidenceOutcome::ReviewedLink
                | MemoryRecallEvidenceOutcome::Changed
        );
        let valid = if resolved {
            target.is_some()
                && candidates.is_empty()
                && match outcome {
                    MemoryRecallEvidenceOutcome::ReviewedLink => {
                        assurance == MemoryRecallEvidenceAssurance::Reviewed
                    }
                    MemoryRecallEvidenceOutcome::Changed => {
                        candidate_coverage_complete
                            && assurance == MemoryRecallEvidenceAssurance::None
                    }
                    _ => {
                        candidate_coverage_complete
                            && matches!(
                                assurance,
                                MemoryRecallEvidenceAssurance::Automatic
                                    | MemoryRecallEvidenceAssurance::Reviewed
                            )
                    }
                }
        } else {
            target.is_none()
                && assurance == MemoryRecallEvidenceAssurance::None
                && match outcome {
                    MemoryRecallEvidenceOutcome::Ambiguous => {
                        candidate_coverage_complete
                            && !candidates.is_empty()
                            && candidate_count_before_limit
                                == u64::try_from(candidates.len()).unwrap_or(u64::MAX)
                    }
                    MemoryRecallEvidenceOutcome::Missing => {
                        candidate_coverage_complete && candidates.is_empty()
                    }
                    MemoryRecallEvidenceOutcome::Indeterminate => candidates.is_empty(),
                    _ => false,
                }
        };
        if !valid
            || target
                .iter()
                .chain(candidates.iter().map(|candidate| &candidate.occurrence))
                .any(|occurrence| !occurrence.path.as_bytes().ends_with(b".rs"))
            || candidates
                .windows(2)
                .any(|pair| occurrence_order(&pair[0].occurrence, &pair[1].occurrence).is_ge())
        {
            return Err(MemoryRecallPortOutputError::InvalidEvidence);
        }
        Ok(Self {
            outcome,
            assurance,
            target,
            candidate_coverage_complete,
            candidate_count_before_limit,
            candidates: candidates.into_boxed_slice(),
        })
    }

    /// Returns the categorical evidence outcome.
    #[must_use]
    pub const fn outcome(&self) -> MemoryRecallEvidenceOutcome {
        self.outcome
    }

    /// Returns the categorical correspondence assurance.
    #[must_use]
    pub const fn assurance(&self) -> MemoryRecallEvidenceAssurance {
        self.assurance
    }

    /// Returns the resolved target occurrence, when one was established.
    #[must_use]
    pub const fn target(&self) -> Option<&MemoryRecallOccurrence> {
        self.target.as_ref()
    }

    /// Reports whether candidate enumeration was complete.
    #[must_use]
    pub const fn candidate_coverage_complete(&self) -> bool {
        self.candidate_coverage_complete
    }

    /// Returns the candidate count observed before any adapter limit.
    #[must_use]
    pub const fn candidate_count_before_limit(&self) -> u64 {
        self.candidate_count_before_limit
    }

    /// Returns candidates in deterministic persisted order.
    #[must_use]
    pub const fn candidates(&self) -> &[MemoryRecallCandidate] {
        &self.candidates
    }
}

fn occurrence_order(
    left: &MemoryRecallOccurrence,
    right: &MemoryRecallOccurrence,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then_with(|| left.artifact_digest.cmp(&right.artifact_digest))
        .then_with(|| left.fact_ordinal.cmp(&right.fact_ordinal))
}
