//! Item-bounded, proof-carrying material results.

use core::fmt;

use crate::{CoverageSummary, EvidenceRecord, ResolutionStatus};

/// The semantic version of the material-result domain contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaterialResultVersion(u16);

impl MaterialResultVersion {
    /// The initial material-result contract.
    pub const V1: Self = Self(1);

    /// Returns the fixed-width version number.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A fixed-width number of items carried by a material result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResultItemCount(u64);

impl ResultItemCount {
    /// No result items.
    pub const ZERO: Self = Self(0);

    /// Returns the fixed-width item count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A fixed-width upper bound on one material-result collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResultItemLimit(u64);

impl ResultItemLimit {
    /// Creates an inclusive upper bound.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the fixed-width upper bound.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Failure to construct a bounded result collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultItemsError {
    /// The platform collection length cannot be represented as a `u64`.
    CountNotRepresentable,
    /// The collection contains more items than its declared bound.
    LimitExceeded {
        /// The collection's actual item count.
        actual: ResultItemCount,
        /// The inclusive item bound.
        limit: ResultItemLimit,
    },
}

impl fmt::Display for ResultItemsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CountNotRepresentable => {
                formatter.write_str("result item count cannot be represented as a u64")
            }
            Self::LimitExceeded { actual, limit } => write!(
                formatter,
                "result item count {} exceeds limit {}",
                actual.get(),
                limit.get()
            ),
        }
    }
}

impl std::error::Error for ResultItemsError {}

/// Failure to construct a semantically valid material result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaterialResultError {
    /// A resolved claim carries explicit contradictory evidence.
    ResolvedWithContradictoryEvidence {
        /// The invalid resolved status.
        resolution: ResolutionStatus,
    },
    /// A resolved claim has no supporting evidence.
    ResolvedWithoutSupportingEvidence {
        /// The invalid resolved status.
        resolution: ResolutionStatus,
    },
    /// An ambiguous result has no evidence describing the ambiguity.
    AmbiguousWithoutEvidence,
    /// An unresolved result does not report unresolved coverage.
    UnresolvedWithoutCoverage,
    /// An indeterminate result does not expose why no determination was made.
    IndeterminateWithoutExplanation,
}

impl fmt::Display for MaterialResultError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResolvedWithContradictoryEvidence { resolution } => write!(
                formatter,
                "{resolution:?} result cannot carry contradictory evidence"
            ),
            Self::ResolvedWithoutSupportingEvidence { resolution } => write!(
                formatter,
                "{resolution:?} result requires supporting evidence"
            ),
            Self::AmbiguousWithoutEvidence => {
                formatter.write_str("ambiguous result requires attributed evidence")
            }
            Self::UnresolvedWithoutCoverage => {
                formatter.write_str("unresolved result requires non-zero unresolved coverage")
            }
            Self::IndeterminateWithoutExplanation => formatter.write_str(
                "indeterminate result requires evidence, a notice, or incomplete coverage",
            ),
        }
    }
}

impl std::error::Error for MaterialResultError {}

/// An owned result collection that records and enforces its inclusive bound.
///
/// The collection stores a boxed slice so unused `Vec` capacity is not retained.
/// It preserves insertion order, so callers must supply deterministic ordering
/// when the source does not already define one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedResultItems<T> {
    items: Box<[T]>,
    count: ResultItemCount,
    limit: ResultItemLimit,
}

impl<T> BoundedResultItems<T> {
    /// Validates an owned collection against an inclusive item bound.
    ///
    /// Successful construction may shrink excess `Vec` capacity while
    /// converting the collection to a boxed slice.
    ///
    /// # Errors
    ///
    /// Returns [`ResultItemsError::CountNotRepresentable`] when the platform
    /// collection length cannot fit in the fixed-width count, or
    /// [`ResultItemsError::LimitExceeded`] when `items` exceeds `limit`.
    pub fn try_from_vec(items: Vec<T>, limit: ResultItemLimit) -> Result<Self, ResultItemsError> {
        let count = u64::try_from(items.len())
            .map(ResultItemCount)
            .map_err(|_| ResultItemsError::CountNotRepresentable)?;

        if count.get() > limit.get() {
            return Err(ResultItemsError::LimitExceeded {
                actual: count,
                limit,
            });
        }

        Ok(Self {
            items: items.into_boxed_slice(),
            count,
            limit,
        })
    }

    /// Returns the items in their deterministic result order.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.items
    }

    /// Returns the fixed-width item count.
    #[must_use]
    pub const fn count(&self) -> ResultItemCount {
        self.count
    }

    /// Returns the inclusive bound enforced during construction.
    #[must_use]
    pub const fn limit(&self) -> ResultItemLimit {
        self.limit
    }

    /// Returns whether this collection contains no items.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count.get() == 0
    }

    /// Consumes the bounded collection and returns its items.
    #[must_use]
    pub fn into_vec(self) -> Vec<T> {
        self.items.into_vec()
    }
}

/// The category of a material-result notice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultNoticeKind {
    /// A condition the consumer should account for.
    Warning,
    /// A known boundary on the meaning or applicability of the result.
    Limitation,
}

/// A structured warning or limitation attached to a material result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResultNotice<N> {
    kind: ResultNoticeKind,
    detail: N,
}

impl<N> ResultNotice<N> {
    /// Creates a structured result notice.
    #[must_use]
    pub const fn new(kind: ResultNoticeKind, detail: N) -> Self {
        Self { kind, detail }
    }

    /// Returns the notice category.
    #[must_use]
    pub const fn kind(&self) -> ResultNoticeKind {
        self.kind
    }

    /// Returns the validated notice detail.
    #[must_use]
    pub const fn detail(&self) -> &N {
        &self.detail
    }
}

/// An item-bounded, evidence-bearing result pinned to one snapshot and generation.
///
/// `C` is a validated claim, `I` an evidence identity, `P` a producer
/// identity, `S` a concrete revision or worktree snapshot, `G` an active index
/// generation, and `N` a validated notice detail. Boundary DTOs map their
/// versioned encodings into these domain values. Concrete component types
/// enforce their own size limits; boundary mapping additionally enforces the
/// total encoded-output byte limit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterialResult<C, I, P, S, G, N> {
    claim: C,
    evidence: BoundedResultItems<EvidenceRecord<I, P>>,
    resolution: ResolutionStatus,
    snapshot: S,
    generation: G,
    notices: BoundedResultItems<ResultNotice<N>>,
    coverage: CoverageSummary,
}

impl<C, I, P, S, G, N> MaterialResult<C, I, P, S, G, N> {
    /// The semantic version implemented by this envelope.
    pub const VERSION: MaterialResultVersion = MaterialResultVersion::V1;

    /// Returns the semantic version implemented by this envelope.
    #[must_use]
    pub const fn version(&self) -> MaterialResultVersion {
        Self::VERSION
    }

    /// Validates and creates a material result from already-bounded components.
    ///
    /// Validation takes time linear in the bounded evidence collection and
    /// performs no I/O or allocation.
    ///
    /// # Errors
    ///
    /// Returns [`MaterialResultError`] when a `confirmed` or `inferred` result
    /// lacks supporting evidence or carries contradictory evidence, or when an
    /// `ambiguous` result has no attributed evidence, or when an `unresolved`
    /// result omits unresolved coverage, or when an `indeterminate` result does
    /// not expose evidence, a notice, or incomplete coverage.
    pub fn try_new(
        claim: C,
        evidence: BoundedResultItems<EvidenceRecord<I, P>>,
        resolution: ResolutionStatus,
        snapshot: S,
        generation: G,
        notices: BoundedResultItems<ResultNotice<N>>,
        coverage: CoverageSummary,
    ) -> Result<Self, MaterialResultError> {
        let has_supporting_evidence = evidence
            .as_slice()
            .iter()
            .any(|record| record.relation() == crate::EvidenceRelation::Supports);
        let has_contradictory_evidence = evidence
            .as_slice()
            .iter()
            .any(|record| record.relation() == crate::EvidenceRelation::Contradicts);

        if matches!(
            resolution,
            ResolutionStatus::Confirmed | ResolutionStatus::Inferred
        ) {
            if has_contradictory_evidence {
                return Err(MaterialResultError::ResolvedWithContradictoryEvidence { resolution });
            }
            if !has_supporting_evidence {
                return Err(MaterialResultError::ResolvedWithoutSupportingEvidence { resolution });
            }
        } else if resolution == ResolutionStatus::Ambiguous && evidence.is_empty() {
            return Err(MaterialResultError::AmbiguousWithoutEvidence);
        } else if resolution == ResolutionStatus::Unresolved
            && coverage.unresolved() == crate::CoverageItemCount::ZERO
        {
            return Err(MaterialResultError::UnresolvedWithoutCoverage);
        } else if resolution == ResolutionStatus::Indeterminate
            && evidence.is_empty()
            && notices.is_empty()
            && coverage.completeness() == crate::CoverageCompleteness::Complete
        {
            return Err(MaterialResultError::IndeterminateWithoutExplanation);
        }

        Ok(Self {
            claim,
            evidence,
            resolution,
            snapshot,
            generation,
            notices,
            coverage,
        })
    }

    /// Returns the validated claim.
    #[must_use]
    pub const fn claim(&self) -> &C {
        &self.claim
    }

    /// Returns the bounded, attributed evidence records.
    #[must_use]
    pub const fn evidence(&self) -> &BoundedResultItems<EvidenceRecord<I, P>> {
        &self.evidence
    }

    /// Returns the categorical resolution status.
    #[must_use]
    pub const fn resolution(&self) -> ResolutionStatus {
        self.resolution
    }

    /// Returns the concrete revision or worktree snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> &S {
        &self.snapshot
    }

    /// Returns the active generation pinned by the request.
    #[must_use]
    pub const fn generation(&self) -> &G {
        &self.generation
    }

    /// Returns the bounded warnings and limitations.
    #[must_use]
    pub const fn notices(&self) -> &BoundedResultItems<ResultNotice<N>> {
        &self.notices
    }

    /// Returns the coverage reported for the request scope.
    #[must_use]
    pub const fn coverage(&self) -> CoverageSummary {
        self.coverage
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        ByteOffset, ByteSpan, CoverageCompleteness, CoverageItemCount, EvidenceIdentity,
        EvidenceLocation, EvidenceRelation, EvidenceTier, ProducerIdentity, ResolutionStatus,
    };

    use super::{
        BoundedResultItems, MaterialResult, MaterialResultError, MaterialResultVersion,
        ResultItemCount, ResultItemLimit, ResultItemsError, ResultNotice, ResultNoticeKind,
    };

    #[test]
    fn bounded_items_reject_a_collection_over_its_limit() {
        let error =
            BoundedResultItems::try_from_vec(vec!["first", "second"], ResultItemLimit::new(1))
                .expect_err("two items must exceed a one-item limit");

        assert_eq!(
            error,
            ResultItemsError::LimitExceeded {
                actual: ResultItemCount(2),
                limit: ResultItemLimit::new(1),
            }
        );
    }

    #[test]
    fn bounded_item_errors_have_stable_diagnostics() {
        assert_eq!(
            ResultItemsError::CountNotRepresentable.to_string(),
            "result item count cannot be represented as a u64"
        );
        assert_eq!(
            ResultItemsError::LimitExceeded {
                actual: ResultItemCount(2),
                limit: ResultItemLimit::new(1),
            }
            .to_string(),
            "result item count 2 exceeds limit 1"
        );
    }

    #[test]
    fn material_result_errors_have_stable_diagnostics() {
        assert_eq!(
            MaterialResultError::ResolvedWithContradictoryEvidence {
                resolution: ResolutionStatus::Confirmed,
            }
            .to_string(),
            "Confirmed result cannot carry contradictory evidence"
        );
        assert_eq!(
            MaterialResultError::ResolvedWithoutSupportingEvidence {
                resolution: ResolutionStatus::Inferred,
            }
            .to_string(),
            "Inferred result requires supporting evidence"
        );
        assert_eq!(
            MaterialResultError::AmbiguousWithoutEvidence.to_string(),
            "ambiguous result requires attributed evidence"
        );
        assert_eq!(
            MaterialResultError::UnresolvedWithoutCoverage.to_string(),
            "unresolved result requires non-zero unresolved coverage"
        );
        assert_eq!(
            MaterialResultError::IndeterminateWithoutExplanation.to_string(),
            "indeterminate result requires evidence, a notice, or incomplete coverage"
        );
    }

    #[test]
    fn zero_limit_accepts_only_an_empty_collection() {
        let empty = BoundedResultItems::<()>::try_from_vec(Vec::new(), ResultItemLimit::new(0))
            .expect("an empty collection fits a zero-item limit");

        assert!(empty.is_empty());
        assert_eq!(empty.count(), ResultItemCount::ZERO);
        assert_eq!(empty.limit().get(), 0);

        let error = BoundedResultItems::try_from_vec(vec![()], ResultItemLimit::new(0))
            .expect_err("a non-empty collection must not fit a zero-item limit");

        assert!(matches!(error, ResultItemsError::LimitExceeded { .. }));
    }

    #[test]
    fn material_result_preserves_proof_and_scope_metadata() {
        let span = ByteSpan::try_new(ByteOffset::new(10), ByteOffset::new(20))
            .expect("ordered evidence endpoints form a valid span");
        let identity = EvidenceIdentity::new(
            "repository:example",
            "snapshot:example",
            "src/lib.rs",
            "digest:example",
            EvidenceLocation::<&str>::ByteSpan(span),
        );
        let producer = ProducerIdentity::new("rust-syntax", "1");
        let evidence = BoundedResultItems::try_from_vec(
            vec![crate::EvidenceRecord::new(
                identity,
                producer,
                EvidenceTier::Syntax,
                EvidenceRelation::Supports,
            )],
            ResultItemLimit::new(4),
        )
        .expect("one evidence record fits the result limit");
        let notices = BoundedResultItems::try_from_vec(
            vec![ResultNotice::new(
                ResultNoticeKind::Limitation,
                "macro expansion was not available",
            )],
            ResultItemLimit::new(2),
        )
        .expect("one notice fits the result limit");
        let coverage = crate::CoverageSummary::new(
            CoverageItemCount::new(8),
            CoverageItemCount::ZERO,
            CoverageItemCount::new(1),
            CoverageItemCount::ZERO,
        );

        let result = MaterialResult::try_new(
            "caller invokes callee",
            evidence,
            ResolutionStatus::Inferred,
            "snapshot:example",
            "generation:7",
            notices,
            coverage,
        )
        .expect("an inferred result with supporting evidence is valid");

        assert_eq!(
            MaterialResult::<&str, &str, &str, &str, &str, &str>::VERSION,
            MaterialResultVersion::V1
        );
        assert_eq!(result.version(), MaterialResultVersion::V1);
        assert_eq!(MaterialResultVersion::V1.get(), 1);
        assert_eq!(*result.claim(), "caller invokes callee");
        assert_eq!(result.resolution(), ResolutionStatus::Inferred);
        assert_eq!(*result.snapshot(), "snapshot:example");
        assert_eq!(*result.generation(), "generation:7");
        assert_eq!(result.evidence().count().get(), 1);
        assert_eq!(result.notices().count().get(), 1);
        assert_eq!(
            result.coverage().completeness(),
            CoverageCompleteness::Partial
        );

        let record = &result.evidence().as_slice()[0];
        assert_eq!(*record.identity().repository(), "repository:example");
        assert_eq!(*record.identity().snapshot(), "snapshot:example");
        assert_eq!(*record.identity().path(), "src/lib.rs");
        assert_eq!(*record.identity().content_digest(), "digest:example");
        assert_eq!(
            record.identity().location(),
            &EvidenceLocation::ByteSpan(span)
        );
        assert_eq!(*record.producer().id(), "rust-syntax");
        assert_eq!(*record.producer().version(), "1");
        assert_eq!(record.tier(), EvidenceTier::Syntax);
        assert_eq!(record.relation(), EvidenceRelation::Supports);

        let notice = &result.notices().as_slice()[0];
        assert_eq!(notice.kind(), ResultNoticeKind::Limitation);
        assert_eq!(*notice.detail(), "macro expansion was not available");
    }

    #[test]
    fn contradictory_evidence_remains_explicit() {
        let record = crate::EvidenceRecord::new(
            "evidence:1",
            "producer:1",
            EvidenceTier::HumanAssertion,
            EvidenceRelation::Contradicts,
        );

        assert_eq!(record.relation(), EvidenceRelation::Contradicts);
    }

    #[test]
    fn confirmed_result_requires_supporting_evidence() {
        let evidence = BoundedResultItems::try_from_vec(Vec::new(), ResultItemLimit::new(1))
            .expect("an empty collection fits the declared bound");
        let notices = BoundedResultItems::try_from_vec(
            Vec::<ResultNotice<()>>::new(),
            ResultItemLimit::new(1),
        )
        .expect("an empty collection fits the declared bound");

        let error = MaterialResult::<_, &str, &str, _, _, ()>::try_new(
            "claim",
            evidence,
            ResolutionStatus::Confirmed,
            "snapshot",
            "generation",
            notices,
            crate::CoverageSummary::default(),
        )
        .expect_err("confirmed results must have supporting evidence");

        assert_eq!(
            error,
            MaterialResultError::ResolvedWithoutSupportingEvidence {
                resolution: ResolutionStatus::Confirmed,
            }
        );
    }

    #[test]
    fn resolved_result_rejects_contradictory_evidence() {
        let evidence = BoundedResultItems::try_from_vec(
            vec![
                crate::EvidenceRecord::new(
                    "support",
                    "producer",
                    EvidenceTier::Syntax,
                    EvidenceRelation::Supports,
                ),
                crate::EvidenceRecord::new(
                    "contradiction",
                    "producer",
                    EvidenceTier::Syntax,
                    EvidenceRelation::Contradicts,
                ),
            ],
            ResultItemLimit::new(2),
        )
        .expect("the evidence collection fits the declared bound");
        let notices = BoundedResultItems::try_from_vec(
            Vec::<ResultNotice<()>>::new(),
            ResultItemLimit::new(1),
        )
        .expect("an empty collection fits the declared bound");

        let error = MaterialResult::try_new(
            "claim",
            evidence,
            ResolutionStatus::Inferred,
            "snapshot",
            "generation",
            notices,
            crate::CoverageSummary::default(),
        )
        .expect_err("resolved results must not hide contradictory evidence");

        assert_eq!(
            error,
            MaterialResultError::ResolvedWithContradictoryEvidence {
                resolution: ResolutionStatus::Inferred,
            }
        );
    }

    #[test]
    fn ambiguous_result_requires_attributed_evidence() {
        let evidence = BoundedResultItems::try_from_vec(Vec::new(), ResultItemLimit::new(1))
            .expect("an empty collection fits the declared bound");
        let notices = BoundedResultItems::try_from_vec(
            Vec::<ResultNotice<()>>::new(),
            ResultItemLimit::new(1),
        )
        .expect("an empty collection fits the declared bound");

        let error = MaterialResult::<_, &str, &str, _, _, ()>::try_new(
            "claim",
            evidence,
            ResolutionStatus::Ambiguous,
            "snapshot",
            "generation",
            notices,
            crate::CoverageSummary::default(),
        )
        .expect_err("ambiguity must be explained by attributed evidence");

        assert_eq!(error, MaterialResultError::AmbiguousWithoutEvidence);
    }

    #[test]
    fn unresolved_result_may_report_no_evidence_with_explicit_coverage() {
        let evidence = BoundedResultItems::try_from_vec(Vec::new(), ResultItemLimit::new(0))
            .expect("an empty collection fits a zero-item limit");
        let notices = BoundedResultItems::try_from_vec(
            Vec::<ResultNotice<()>>::new(),
            ResultItemLimit::new(0),
        )
        .expect("an empty collection fits a zero-item limit");
        let coverage = crate::CoverageSummary::new(
            CoverageItemCount::new(1),
            CoverageItemCount::ZERO,
            CoverageItemCount::new(1),
            CoverageItemCount::ZERO,
        );

        let result = MaterialResult::<_, &str, &str, _, _, ()>::try_new(
            "claim",
            evidence,
            ResolutionStatus::Unresolved,
            "snapshot",
            "generation",
            notices,
            coverage,
        )
        .expect("unresolved results may have no available evidence");

        assert!(result.evidence().is_empty());
    }

    #[test]
    fn unresolved_result_requires_unresolved_coverage() {
        let evidence = BoundedResultItems::try_from_vec(Vec::new(), ResultItemLimit::new(0))
            .expect("an empty collection fits a zero-item limit");
        let notices = BoundedResultItems::try_from_vec(
            Vec::<ResultNotice<()>>::new(),
            ResultItemLimit::new(0),
        )
        .expect("an empty collection fits a zero-item limit");

        let error = MaterialResult::<_, &str, &str, _, _, ()>::try_new(
            "claim",
            evidence,
            ResolutionStatus::Unresolved,
            "snapshot",
            "generation",
            notices,
            crate::CoverageSummary::default(),
        )
        .expect_err("unresolved work must remain visible in coverage");

        assert_eq!(error, MaterialResultError::UnresolvedWithoutCoverage);
    }

    #[test]
    fn indeterminate_result_requires_an_explanation() {
        let evidence = BoundedResultItems::try_from_vec(Vec::new(), ResultItemLimit::new(0))
            .expect("an empty collection fits a zero-item limit");
        let notices = BoundedResultItems::try_from_vec(
            Vec::<ResultNotice<()>>::new(),
            ResultItemLimit::new(0),
        )
        .expect("an empty collection fits a zero-item limit");

        let error = MaterialResult::<_, &str, &str, _, _, ()>::try_new(
            "claim",
            evidence,
            ResolutionStatus::Indeterminate,
            "snapshot",
            "generation",
            notices,
            crate::CoverageSummary::default(),
        )
        .expect_err("indeterminate results must explain missing information");

        assert_eq!(error, MaterialResultError::IndeterminateWithoutExplanation);
    }

    #[test]
    fn bounded_items_preserve_supplied_order() {
        let items = BoundedResultItems::try_from_vec(
            vec!["first", "second", "third"],
            ResultItemLimit::new(3),
        )
        .expect("the collection exactly matches its limit");

        assert_eq!(items.as_slice(), ["first", "second", "third"]);
        assert_eq!(items.into_vec(), vec!["first", "second", "third"]);
    }
}
