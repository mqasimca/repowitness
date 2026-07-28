use repowitness_domain::{
    AnalysisArtifactDigest, ByteOffset, ByteSpan, CanonicalMemoryDigest, DeclarationDigest,
    MemoryActorId, MemoryActorKind, MemoryAssurance, MemoryBody, MemoryClaim,
    MemoryDisplayRevision, MemoryEvidence, MemoryEvidenceIndex, MemoryFactOrdinal, MemoryKind,
    MemoryLifecycle, MemoryProducerId, MemoryProducerVersion, MemoryProjectValidity,
    MemoryProvenance, MemoryProvenanceOrigin, MemoryQualifiedName, MemoryRecord,
    MemoryRecordHeader, MemoryRecordId, MemoryScope, MemorySymbolName, MemoryTitle, MemoryValidity,
    ProducerIdentity, RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits,
    RustMemorySymbolKind, RustSymbolMemoryEvidence, SourceContentDigest, SourceSnapshotDigest,
};

use super::{
    MemoryEffectiveState, MemoryEvidenceOutcome, MemoryHeadState, MemoryProjectionEvidenceState,
    MemoryProjectionReason, MemoryProjectionValidityState, MemoryVersionHeadInput,
    evaluate_memory_projection, select_memory_head,
};

fn digest(byte: u8) -> CanonicalMemoryDigest {
    CanonicalMemoryDigest::new([byte; 32])
}

fn record(
    record_byte: u8,
    display_revision: u32,
    parents: Vec<CanonicalMemoryDigest>,
    lifecycle: MemoryLifecycle,
) -> MemoryRecord {
    let snapshot = SourceSnapshotDigest::new([0x22; 32]);
    let evidence = RustSymbolMemoryEvidence::try_new(
        snapshot,
        RepositoryPath::try_from_bytes(b"src/lib.rs", RepositoryPathLimits::new(128, 8))
            .expect("path"),
        SourceContentDigest::new([0x33; 32]),
        AnalysisArtifactDigest::new([0x44; 32]),
        MemoryFactOrdinal::try_new(0).expect("ordinal"),
        RustMemorySymbolKind::Function,
        MemorySymbolName::try_new("publish".to_owned()).expect("name"),
        MemoryQualifiedName::try_new("crate::publish".to_owned()).expect("qualified name"),
        ByteSpan::try_new(ByteOffset::new(3), ByteOffset::new(10)).expect("name span"),
        ByteSpan::try_new(ByteOffset::new(0), ByteOffset::new(20)).expect("declaration span"),
        DeclarationDigest::new([0x55; 32]),
        ProducerIdentity::new(
            MemoryProducerId::try_new("repowitness.rust.syntax".to_owned()).expect("producer ID"),
            MemoryProducerVersion::try_new("phase0-rust-syntax-v1".to_owned())
                .expect("producer version"),
        ),
    )
    .expect("evidence");
    MemoryRecord::try_new(
        MemoryRecordHeader::try_new(
            MemoryRecordId::new([record_byte; 16]),
            MemoryDisplayRevision::try_new(display_revision).expect("display revision"),
            parents,
        )
        .expect("header"),
        MemoryClaim::new(
            MemoryKind::Decision,
            MemoryTitle::try_new("Keep publication atomic".to_owned()).expect("title"),
            MemoryBody::try_new("Readers see complete generations.".to_owned()).expect("body"),
        ),
        MemoryScope::new(
            RepositoryIdentityDigest::new([0x10; 32]),
            MemoryEvidenceIndex::try_new(0).expect("subject evidence"),
        ),
        MemoryProvenance::new(
            MemoryProvenanceOrigin::Human,
            MemoryActorKind::LocalAsserted,
            MemoryActorId::try_new("maintainer".to_owned()).expect("actor"),
        ),
        MemoryAssurance::LocallyApproved,
        lifecycle,
        MemoryValidity::worktree(snapshot),
        vec![MemoryEvidence::RustSymbol(evidence)],
        Vec::new(),
        lifecycle == MemoryLifecycle::Tombstoned,
    )
    .expect("record")
}

#[test]
fn approved_heads_ignore_display_order_and_unapproved_children() {
    let first = record(1, 99, Vec::new(), MemoryLifecycle::Active);
    let second = record(1, 1, vec![digest(0x11)], MemoryLifecycle::Active);
    let unapproved = record(1, 500, vec![digest(0x22)], MemoryLifecycle::Active);
    let inputs = [
        MemoryVersionHeadInput::new(digest(0x11), &first, true),
        MemoryVersionHeadInput::new(digest(0x22), &second, true),
        MemoryVersionHeadInput::new(digest(0x33), &unapproved, false),
    ];

    let selection = select_memory_head(&inputs).expect("selection");
    assert_eq!(selection.state(), MemoryHeadState::Selected);
    assert_eq!(selection.selected_revision(), Some(digest(0x22)));
    assert_eq!(selection.approved_version_count(), 2);
    assert_eq!(selection.head_count(), 1);
    assert_eq!(selection.missing_parent_count(), 0);
}

#[test]
fn divergent_heads_conflict_and_missing_parents_are_indeterminate() {
    let first = record(1, 1, Vec::new(), MemoryLifecycle::Active);
    let second = record(1, 2, Vec::new(), MemoryLifecycle::Active);
    let conflict = [
        MemoryVersionHeadInput::new(digest(0x11), &first, true),
        MemoryVersionHeadInput::new(digest(0x22), &second, true),
    ];
    let selection = select_memory_head(&conflict).expect("conflict selection");
    assert_eq!(selection.state(), MemoryHeadState::Conflicted);
    assert_eq!(selection.selected_revision(), None);
    assert_eq!(selection.head_count(), 2);

    let missing = record(1, 3, vec![digest(0x99)], MemoryLifecycle::Active);
    let selection =
        select_memory_head(&[MemoryVersionHeadInput::new(digest(0x33), &missing, true)])
            .expect("missing-parent selection");
    assert_eq!(selection.state(), MemoryHeadState::Indeterminate);
    assert_eq!(selection.selected_revision(), Some(digest(0x33)));
    assert_eq!(selection.missing_parent_count(), 1);
}

#[test]
fn active_effective_state_is_precision_first() {
    let record = record(1, 1, Vec::new(), MemoryLifecycle::Active);
    for (outcomes, effective, aggregate, reason) in [
        (
            vec![MemoryEvidenceOutcome::Exact],
            MemoryEffectiveState::Current,
            MemoryProjectionEvidenceState::Exact,
            MemoryProjectionReason::EvidenceExact,
        ),
        (
            vec![MemoryEvidenceOutcome::Corresponded],
            MemoryEffectiveState::Current,
            MemoryProjectionEvidenceState::Corresponded,
            MemoryProjectionReason::EvidenceCorresponded,
        ),
        (
            vec![MemoryEvidenceOutcome::Changed],
            MemoryEffectiveState::Stale,
            MemoryProjectionEvidenceState::Changed,
            MemoryProjectionReason::EvidenceChanged,
        ),
        (
            vec![MemoryEvidenceOutcome::NeedsReview],
            MemoryEffectiveState::NeedsReview,
            MemoryProjectionEvidenceState::Ambiguous,
            MemoryProjectionReason::EvidenceAmbiguous,
        ),
        (
            vec![MemoryEvidenceOutcome::Missing],
            MemoryEffectiveState::Stale,
            MemoryProjectionEvidenceState::Missing,
            MemoryProjectionReason::EvidenceMissing,
        ),
        (
            vec![MemoryEvidenceOutcome::Indeterminate],
            MemoryEffectiveState::Indeterminate,
            MemoryProjectionEvidenceState::Indeterminate,
            MemoryProjectionReason::EvidenceIndeterminate,
        ),
    ] {
        let decision =
            evaluate_memory_projection(&record, Some(MemoryProjectValidity::Valid), &outcomes)
                .expect("decision");
        assert_eq!(decision.effective_state(), effective);
        assert_eq!(
            decision.validity_state(),
            MemoryProjectionValidityState::Valid
        );
        assert_eq!(decision.evidence_state(), aggregate);
        assert_eq!(decision.reason(), reason);
    }
}

#[test]
fn validity_and_authored_lifecycle_short_circuit_evidence() {
    let active = record(1, 1, Vec::new(), MemoryLifecycle::Active);
    let decision =
        evaluate_memory_projection(&active, Some(MemoryProjectValidity::NotApplicable), &[])
            .expect("not-applicable decision");
    assert_eq!(
        decision.effective_state(),
        MemoryEffectiveState::NotApplicable
    );
    assert_eq!(
        decision.validity_state(),
        MemoryProjectionValidityState::Invalid
    );
    assert_eq!(
        decision.evidence_state(),
        MemoryProjectionEvidenceState::NotEvaluated
    );

    let tombstone = record(1, 2, vec![digest(0x10)], MemoryLifecycle::Tombstoned);
    let decision = evaluate_memory_projection(&tombstone, None, &[]).expect("tombstone decision");
    assert_eq!(decision.effective_state(), MemoryEffectiveState::Tombstoned);
    assert_eq!(
        decision.validity_state(),
        MemoryProjectionValidityState::NotEvaluated
    );
    assert_eq!(
        decision.reason(),
        MemoryProjectionReason::AuthoredTombstoned
    );
    assert_eq!(decision.evidence_count(), tombstone.evidence().len() as u32);
}

#[test]
fn mixed_records_duplicate_revisions_and_mismatched_evidence_fail_closed() {
    let first = record(1, 1, Vec::new(), MemoryLifecycle::Active);
    let other = record(2, 1, Vec::new(), MemoryLifecycle::Active);
    assert!(
        select_memory_head(&[
            MemoryVersionHeadInput::new(digest(0x11), &first, true),
            MemoryVersionHeadInput::new(digest(0x22), &other, true),
        ])
        .is_err()
    );
    assert!(
        select_memory_head(&[
            MemoryVersionHeadInput::new(digest(0x11), &first, true),
            MemoryVersionHeadInput::new(digest(0x11), &first, true),
        ])
        .is_err()
    );
    assert!(evaluate_memory_projection(&first, Some(MemoryProjectValidity::Valid), &[]).is_err());
}
