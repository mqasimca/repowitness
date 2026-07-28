use super::{
    MAX_MEMORY_INTEROPERABLE_INTEGER, MemoryActorId, MemoryAuditActorId, MemoryCommitId,
    MemoryDisplayRevision, MemoryEvidenceIndex, MemoryObservationSource, MemoryRecordError,
    MemoryRecordId, MemoryRecordedAtUnixMillis, MemoryTextField, MemoryTitle, MemoryValidity,
};

#[test]
fn record_id_is_exact_and_debug_is_redacted() {
    let bytes = [0xA5; 16];
    let id = MemoryRecordId::new(bytes);

    assert_eq!(id.as_bytes(), &bytes);
    assert_eq!(id.into_bytes(), bytes);
    assert!(!format!("{id:?}").contains("A5"));
    assert!(!format!("{id:?}").contains("165"));
}

#[test]
fn scalar_boundaries_are_typed_and_redacted() {
    assert_eq!(
        MemoryDisplayRevision::try_new(0),
        Err(MemoryRecordError::InvalidDisplayRevision)
    );
    assert_eq!(
        MemoryEvidenceIndex::try_new(MAX_MEMORY_INTEROPERABLE_INTEGER + 1),
        Err(MemoryRecordError::InvalidInteger(
            super::MemoryIntegerField::EvidenceIndex
        ))
    );
    assert_eq!(
        MemoryTitle::try_new(String::new()),
        Err(MemoryRecordError::InvalidText(MemoryTextField::Title))
    );
    assert_eq!(
        MemoryActorId::try_new("line\nbreak".to_owned()),
        Err(MemoryRecordError::InvalidText(MemoryTextField::ActorId))
    );
    assert_eq!(
        MemoryAuditActorId::try_new("line\nbreak".to_owned()),
        Err(MemoryRecordError::InvalidText(
            MemoryTextField::AuditActorId
        ))
    );
    assert_eq!(
        MemoryRecordedAtUnixMillis::try_new(i64::MAX as u64 + 1),
        Err(MemoryRecordError::InvalidInteger(
            super::MemoryIntegerField::RecordedAtUnixMillis
        ))
    );
    assert_eq!(
        MemoryRecordedAtUnixMillis::try_new(i64::MAX as u64)
            .expect("signed SQLite maximum is valid")
            .get(),
        i64::MAX as u64
    );
}

#[test]
fn observation_source_debug_redacts_exact_identity_bytes() {
    let source = MemoryObservationSource::Git(MemoryCommitId::Sha1([0xA5; 20]));
    let debug = format!("{source:?}");

    assert!(debug.contains("sha1"));
    assert!(!debug.contains("A5"));
    assert!(!debug.contains("165"));
}

#[test]
fn commit_validity_is_sorted_and_conflicts_fail_closed() {
    let sha1 = MemoryCommitId::Sha1([0x11; 20]);
    let sha256 = MemoryCommitId::Sha256([0x22; 32]);
    let validity = MemoryValidity::try_commits(vec![sha256, sha1], Vec::new())
        .expect("distinct introduction commits are valid");
    let MemoryValidity::Commits { introduced_by, .. } = validity else {
        panic!("expected commit validity");
    };
    assert_eq!(introduced_by, vec![sha1, sha256]);

    assert_eq!(
        MemoryValidity::try_commits(vec![sha1], vec![sha1]),
        Err(MemoryRecordError::InvalidValidity)
    );
}
