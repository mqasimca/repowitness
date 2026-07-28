use std::{
    cell::{Cell, RefCell},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use repowitness_domain::{
    AnalysisArtifactDigest, ByteOffset, ByteSpan, CanonicalMemoryDigest,
    CorrespondenceFingerprintDigest, CorrespondenceProfileDigest, DeclarationDigest, MemoryActorId,
    MemoryActorKind, MemoryAssurance, MemoryBody, MemoryClaim, MemoryDisplayRevision,
    MemoryEvidence, MemoryEvidenceIndex, MemoryFactOrdinal, MemoryKind, MemoryLifecycle,
    MemoryProducerId, MemoryProducerVersion, MemoryProvenance, MemoryProvenanceOrigin,
    MemoryQualifiedName, MemoryRecord, MemoryRecordHeader, MemoryRecordId, MemoryScope,
    MemorySymbolName, MemoryTitle, MemoryValidity, ProducerIdentity, RepositoryIdentityDigest,
    RepositoryPath, RepositoryPathLimits, RustMemorySymbolKind, RustSymbolMemoryEvidence,
    SourceContentDigest, SourceSnapshotDigest,
};

use super::{
    MAX_MEMORY_RECALL_OUTPUT_BYTES, MAX_MEMORY_RECALL_RESULTS, MAX_MEMORY_RECALL_SCAN_BYTES,
    MemoryRecallCandidate, MemoryRecallCandidateRelation, MemoryRecallError, MemoryRecallEvidence,
    MemoryRecallEvidenceAssurance, MemoryRecallEvidenceOutcome, MemoryRecallEvidenceState,
    MemoryRecallLimits, MemoryRecallOccurrence, MemoryRecallPort, MemoryRecallPortOutputError,
    MemoryRecallPortResult, MemoryRecallProducer, MemoryRecallProjectionCoverage,
    MemoryRecallQuery, MemoryRecallQueryError, MemoryRecallReason, MemoryRecallRecord,
    MemoryRecallRequest, memory_recall,
};
use crate::{MemoryEffectiveState, MemoryProjectionValidityState};

const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(128, 8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FakeError {
    Failed,
}

impl std::fmt::Display for FakeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("fake recall failure")
    }
}

impl std::error::Error for FakeError {}

struct FakePort {
    calls: Cell<u64>,
    result: RefCell<Option<Result<MemoryRecallPortResult<u64, u64>, FakeError>>>,
}

impl FakePort {
    fn with(result: Result<MemoryRecallPortResult<u64, u64>, FakeError>) -> Self {
        Self {
            calls: Cell::new(0),
            result: RefCell::new(Some(result)),
        }
    }
}

impl MemoryRecallPort for FakePort {
    type Generation = u64;
    type Projection = u64;
    type Error = FakeError;

    fn recall(
        &self,
        _repository: RepositoryIdentityDigest,
        _query: &MemoryRecallQuery,
        _limits: MemoryRecallLimits,
        _cancelled: Arc<AtomicBool>,
        _deadline: Instant,
    ) -> Result<MemoryRecallPortResult<Self::Generation, Self::Projection>, Self::Error> {
        self.calls.set(self.calls.get() + 1);
        self.result
            .borrow_mut()
            .take()
            .expect("fake port should be called at most once")
    }
}

fn repository() -> RepositoryIdentityDigest {
    RepositoryIdentityDigest::new([0x10; 32])
}

fn semantic_record(record_byte: u8) -> MemoryRecord {
    let snapshot = SourceSnapshotDigest::new([0x22; 32]);
    let evidence = RustSymbolMemoryEvidence::try_new(
        snapshot,
        RepositoryPath::try_from_bytes(b"src/lib.rs", PATH_LIMITS).expect("path"),
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
            MemoryDisplayRevision::try_new(1).expect("display revision"),
            Vec::new(),
        )
        .expect("header"),
        MemoryClaim::new(
            MemoryKind::Decision,
            MemoryTitle::try_new("Keep publication atomic".to_owned()).expect("title"),
            MemoryBody::try_new("Readers see complete generations.".to_owned()).expect("body"),
        ),
        MemoryScope::new(
            repository(),
            MemoryEvidenceIndex::try_new(0).expect("subject evidence"),
        ),
        MemoryProvenance::new(
            MemoryProvenanceOrigin::Human,
            MemoryActorKind::LocalAsserted,
            MemoryActorId::try_new("maintainer".to_owned()).expect("actor"),
        ),
        MemoryAssurance::LocallyApproved,
        MemoryLifecycle::Active,
        MemoryValidity::worktree(snapshot),
        vec![MemoryEvidence::RustSymbol(evidence)],
        Vec::new(),
        false,
    )
    .expect("record")
}

fn occurrence(path: &[u8], ordinal: u64) -> MemoryRecallOccurrence {
    MemoryRecallOccurrence::new(
        RepositoryPath::try_from_bytes(path, PATH_LIMITS).expect("path"),
        SourceContentDigest::new([0x66; 32]),
        AnalysisArtifactDigest::new([0x77; 32]),
        ordinal,
        DeclarationDigest::new([0x88; 32]),
        CorrespondenceFingerprintDigest::new([0x99; 32]),
    )
}

fn current_record(record_byte: u8) -> MemoryRecallRecord {
    let record = semantic_record(record_byte);
    let evidence = MemoryRecallEvidence::try_new(
        MemoryRecallEvidenceOutcome::Exact,
        MemoryRecallEvidenceAssurance::Automatic,
        Some(occurrence(b"src/lib.rs", 0)),
        true,
        1,
        Vec::new(),
    )
    .expect("projection evidence");
    MemoryRecallRecord::try_new(
        record.header().record_id(),
        Some(CanonicalMemoryDigest::new([record_byte; 32])),
        Some(record),
        MemoryEffectiveState::Current,
        MemoryProjectionValidityState::Valid,
        MemoryRecallEvidenceState::Exact,
        MemoryRecallReason::EvidenceExact,
        1,
        1,
        0,
        0,
        1,
        0,
        vec![evidence],
    )
    .expect("projection record")
}

fn coverage(total: u64) -> MemoryRecallProjectionCoverage {
    MemoryRecallProjectionCoverage::new(total, 0, 0, 0, total, total, 0, 0, 0, 0, 0, 0, 0, 0, 0)
}

fn port_result(records: Vec<MemoryRecallRecord>) -> MemoryRecallPortResult<u64, u64> {
    port_result_with_coverage(records, None)
}

fn port_result_with_coverage(
    records: Vec<MemoryRecallRecord>,
    explicit_coverage: Option<MemoryRecallProjectionCoverage>,
) -> MemoryRecallPortResult<u64, u64> {
    let total = u64::try_from(records.len()).expect("fixture count");
    let output_bytes = records
        .iter()
        .map(MemoryRecallRecord::encoded_output_bytes)
        .try_fold(0_u64, |sum, value| sum.checked_add(value.ok()?))
        .expect("fixture output count");
    MemoryRecallPortResult::new(
        SourceSnapshotDigest::new([0x22; 32]),
        7,
        9,
        3,
        repowitness_domain::MemoryRevalidationTarget::worktree(
            SourceSnapshotDigest::new([0x22; 32]),
            None,
        ),
        MemoryRecallProducer::try_new(
            "repowitness.rust.correspondence".to_owned(),
            1,
            CorrespondenceProfileDigest::new([0xaa; 32]),
        )
        .expect("producer"),
        explicit_coverage.unwrap_or_else(|| coverage(total)),
        records,
        total,
        output_bytes,
        1024,
    )
}

fn request(cancelled: Arc<AtomicBool>, deadline: Instant) -> MemoryRecallRequest {
    MemoryRecallRequest::new(
        repository(),
        MemoryRecallQuery::try_new("  PUBLICATION\tAtomic ").expect("query"),
        MemoryRecallLimits::default(),
        cancelled,
        deadline,
    )
}

#[test]
fn query_modes_are_canonical_bounded_and_redacted() {
    let first = MemoryRecallQuery::try_new("  PUBLICATION\tAtomic ").expect("query");
    let second = MemoryRecallQuery::try_new("publication atomic").expect("query");
    assert_eq!(first, second);
    assert_eq!(first.as_str(), Some("publication atomic"));
    assert_eq!(first.term_count(), 2);
    assert_eq!(first.digest(), second.digest());
    assert_eq!(MemoryRecallQuery::all().as_str(), None);
    let debug = format!("{first:?}");
    assert!(debug.contains("<redacted-query>"));
    assert!(!debug.contains("publication"));

    assert_eq!(
        MemoryRecallQuery::try_new(""),
        Err(MemoryRecallQueryError::Empty)
    );
    assert_eq!(
        MemoryRecallQuery::try_new(&"x".repeat(257)),
        Err(MemoryRecallQueryError::QueryTooLong)
    );
    assert_eq!(
        MemoryRecallQuery::try_new("1 2 3 4 5 6 7 8 9"),
        Err(MemoryRecallQueryError::TooManyTerms)
    );
    assert_eq!(
        MemoryRecallQuery::try_new(&"x".repeat(65)),
        Err(MemoryRecallQueryError::TermTooLong)
    );
    assert_eq!(
        MemoryRecallQuery::try_new("secret\0term"),
        Err(MemoryRecallQueryError::InvalidTerm)
    );
}

#[test]
fn limits_enforce_inclusive_phase0_ceilings() {
    assert!(
        MemoryRecallLimits::try_new(
            MAX_MEMORY_RECALL_RESULTS,
            MAX_MEMORY_RECALL_OUTPUT_BYTES,
            MAX_MEMORY_RECALL_SCAN_BYTES,
        )
        .is_ok()
    );
    assert!(MemoryRecallLimits::try_new(0, 1, 1).is_err());
    assert!(MemoryRecallLimits::try_new(MAX_MEMORY_RECALL_RESULTS + 1, 1, 1).is_err());
    assert!(MemoryRecallLimits::try_new(1, 0, 1).is_err());
    assert!(MemoryRecallLimits::try_new(1, MAX_MEMORY_RECALL_OUTPUT_BYTES + 1, 1).is_err());
    assert!(MemoryRecallLimits::try_new(1, 1, 0).is_err());
    assert!(MemoryRecallLimits::try_new(1, 1, MAX_MEMORY_RECALL_SCAN_BYTES + 1).is_err());
}

#[test]
fn current_claim_keeps_exact_projection_and_coverage_attribution() {
    let port = FakePort::with(Ok(port_result(vec![current_record(1)])));
    let result = memory_recall(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        ),
    )
    .expect("recall");

    assert_eq!(port.calls.get(), 1);
    assert_eq!(result.generation(), &7);
    assert_eq!(result.projection(), &9);
    assert_eq!(result.source_epoch(), 3);
    assert_eq!(result.total_matches(), 1);
    assert_eq!(result.omitted_matches(), 0);
    assert_eq!(
        result
            .projection_coverage()
            .state_count(MemoryEffectiveState::Current),
        1
    );
    assert_eq!(
        result.records()[0].effective_state(),
        MemoryEffectiveState::Current
    );
    assert_eq!(
        result.records()[0]
            .record()
            .expect("selected claim")
            .claim()
            .title()
            .as_str(),
        "Keep publication atomic"
    );
    assert_eq!(result.records()[0].evidence().len(), 1);
}

#[test]
fn invalid_evidence_records_ordering_and_coverage_fail_closed() {
    let duplicate_candidates = vec![
        MemoryRecallCandidate::new(
            occurrence(b"src/a.rs", 0),
            MemoryRecallCandidateRelation::Moved,
        ),
        MemoryRecallCandidate::new(
            occurrence(b"src/a.rs", 0),
            MemoryRecallCandidateRelation::Moved,
        ),
    ];
    assert_eq!(
        MemoryRecallEvidence::try_new(
            MemoryRecallEvidenceOutcome::Ambiguous,
            MemoryRecallEvidenceAssurance::None,
            None,
            true,
            2,
            duplicate_candidates,
        ),
        Err(MemoryRecallPortOutputError::InvalidEvidence)
    );

    let inconsistent_coverage =
        MemoryRecallProjectionCoverage::new(1, 0, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    let port = FakePort::with(Ok(port_result_with_coverage(
        vec![current_record(1)],
        Some(inconsistent_coverage),
    )));
    assert!(matches!(
        memory_recall(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
            ),
        ),
        Err(MemoryRecallError::InvalidPortOutput(
            MemoryRecallPortOutputError::InvalidCoverage
        ))
    ));

    let port = FakePort::with(Ok(port_result(vec![current_record(2), current_record(1)])));
    assert!(matches!(
        memory_recall(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
            ),
        ),
        Err(MemoryRecallError::InvalidPortOutput(
            MemoryRecallPortOutputError::InvalidOrdering
        ))
    ));
}

#[test]
fn cancellation_deadline_port_errors_and_debug_output_are_safe() {
    let cancelled = Arc::new(AtomicBool::new(true));
    let port = FakePort::with(Err(FakeError::Failed));
    assert!(matches!(
        memory_recall(
            &port,
            request(cancelled, Instant::now() + Duration::from_secs(1)),
        ),
        Err(MemoryRecallError::Cancelled)
    ));
    assert_eq!(port.calls.get(), 0);

    let port = FakePort::with(Err(FakeError::Failed));
    assert!(matches!(
        memory_recall(
            &port,
            request(Arc::new(AtomicBool::new(false)), Instant::now()),
        ),
        Err(MemoryRecallError::DeadlineExceeded)
    ));
    assert_eq!(port.calls.get(), 0);

    let port = FakePort::with(Err(FakeError::Failed));
    assert!(matches!(
        memory_recall(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
            ),
        ),
        Err(MemoryRecallError::Port(FakeError::Failed))
    ));
    assert_eq!(port.calls.get(), 1);

    let debug = format!(
        "{:?}",
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        )
    );
    assert!(debug.contains("<redacted-query>"));
    assert!(!debug.contains("publication"));
}
