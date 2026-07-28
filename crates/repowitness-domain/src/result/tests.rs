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
    let error = BoundedResultItems::try_from_vec(vec!["first", "second"], ResultItemLimit::new(1))
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
    let notices =
        BoundedResultItems::try_from_vec(Vec::<ResultNotice<()>>::new(), ResultItemLimit::new(1))
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
    let notices =
        BoundedResultItems::try_from_vec(Vec::<ResultNotice<()>>::new(), ResultItemLimit::new(1))
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
    let notices =
        BoundedResultItems::try_from_vec(Vec::<ResultNotice<()>>::new(), ResultItemLimit::new(1))
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
    let notices =
        BoundedResultItems::try_from_vec(Vec::<ResultNotice<()>>::new(), ResultItemLimit::new(0))
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
    let notices =
        BoundedResultItems::try_from_vec(Vec::<ResultNotice<()>>::new(), ResultItemLimit::new(0))
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
    let notices =
        BoundedResultItems::try_from_vec(Vec::<ResultNotice<()>>::new(), ResultItemLimit::new(0))
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
    let items =
        BoundedResultItems::try_from_vec(vec!["first", "second", "third"], ResultItemLimit::new(3))
            .expect("the collection exactly matches its limit");

    assert_eq!(items.as_slice(), ["first", "second", "third"]);
    assert_eq!(items.into_vec(), vec!["first", "second", "third"]);
}
