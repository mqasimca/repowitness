use std::{
    cell::RefCell,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use repowitness_analysis::RustSymbolKind;
use repowitness_domain::{
    AnalysisArtifactDigest, ByteOffset, ByteSpan, CanonicalMemoryDigest,
    CorrespondenceFingerprintDigest, CorrespondenceProfileDigest, CoverageItemCount,
    DeclarationDigest, MemoryActorId, MemoryActorKind, MemoryAssurance, MemoryBody, MemoryClaim,
    MemoryDisplayRevision, MemoryEvidence, MemoryEvidenceIndex, MemoryFactOrdinal, MemoryKind,
    MemoryLifecycle, MemoryProducerId, MemoryProducerVersion, MemoryProvenance,
    MemoryProvenanceOrigin, MemoryQualifiedName, MemoryRecord, MemoryRecordHeader, MemoryRecordId,
    MemoryRevalidationTarget, MemoryScope, MemorySymbolName, MemoryTitle, MemoryValidity,
    ProducerIdentity, RepositoryPath, RepositoryPathLimits, RustMemorySymbolKind,
    RustSymbolMemoryEvidence, SourceContentDigest,
};

use crate::{
    MemoryProjectionValidityState, MemoryRecallEvidence, MemoryRecallEvidenceAssurance,
    MemoryRecallEvidenceOutcome, MemoryRecallEvidenceState, MemoryRecallLimits,
    MemoryRecallOccurrence, MemoryRecallPort, MemoryRecallPortResult, MemoryRecallQuery,
    MemoryRecallReason, MemoryRecallRequest, SourceArtifactEvidence, SourceLanguage, memory_recall,
};

use super::*;

const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(128, 8);

fn repository() -> RepositoryIdentityDigest {
    RepositoryIdentityDigest::new([0x10; 32])
}

fn snapshot() -> SourceSnapshotDigest {
    SourceSnapshotDigest::new([0x22; 32])
}

fn source_candidate(rank: u16, ordinal: u64, name: &str) -> ContextSourceCandidate {
    let path = RepositoryPath::try_from_bytes(b"src/lib.rs", PATH_LIMITS).expect("path");
    let content = SourceContentDigest::new([0x30; 32]);
    let artifact = AnalysisArtifactDigest::new([0x40; 32]);
    let end = u64::try_from(name.len()).expect("fixture length");
    let span = ByteSpan::try_new(ByteOffset::new(0), ByteOffset::new(end)).expect("span");
    let occurrence = RustSymbolOccurrence::try_new(
        ordinal,
        SourceArtifactEvidence::new(
            artifact,
            repowitness_domain::ProducerManifestDigest::new([0x50; 32]),
        ),
        RustSymbolKind::Function,
        name.to_owned(),
        format!("crate::{name}"),
        span,
        span,
    )
    .expect("occurrence")
    .with_language(SourceLanguage::Rust);
    ContextSourceCandidate::try_new(
        rank,
        SymbolGetSelector::new(path, content, artifact, ordinal),
        occurrence,
        name.as_bytes().to_vec().into_boxed_slice(),
    )
    .expect("source candidate")
}

fn source_input(
    generation: u64,
    total: u64,
    returned: u64,
    candidates: Vec<ContextSourceCandidate>,
) -> ContextSourceInput<u64> {
    ContextSourceInput::try_new(
        repository(),
        crate::CodeSearchQuery::try_new("atomic publish")
            .expect("query")
            .digest(),
        snapshot(),
        generation,
        CoverageSummary::new(
            CoverageItemCount::new(1),
            CoverageItemCount::new(0),
            CoverageItemCount::new(0),
            CoverageItemCount::new(total.saturating_sub(returned)),
        ),
        total,
        returned,
        candidates,
    )
    .expect("source input")
}

fn semantic_record(record_byte: u8, lifecycle: MemoryLifecycle) -> MemoryRecord {
    let evidence = RustSymbolMemoryEvidence::try_new(
        snapshot(),
        RepositoryPath::try_from_bytes(b"src/lib.rs", PATH_LIMITS).expect("path"),
        SourceContentDigest::new([0x33; 32]),
        AnalysisArtifactDigest::new([0x44; 32]),
        MemoryFactOrdinal::try_new(0).expect("ordinal"),
        RustMemorySymbolKind::Function,
        MemorySymbolName::try_new("publish".to_owned()).expect("name"),
        MemoryQualifiedName::try_new("crate::publish".to_owned()).expect("qualified"),
        ByteSpan::try_new(ByteOffset::new(3), ByteOffset::new(10)).expect("name span"),
        ByteSpan::try_new(ByteOffset::new(0), ByteOffset::new(20)).expect("declaration span"),
        DeclarationDigest::new([0x55; 32]),
        ProducerIdentity::new(
            MemoryProducerId::try_new("repowitness.rust.syntax".to_owned()).expect("producer"),
            MemoryProducerVersion::try_new("phase0-rust-syntax-v1".to_owned()).expect("version"),
        ),
    )
    .expect("memory evidence");
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
        lifecycle,
        MemoryValidity::worktree(snapshot()),
        vec![MemoryEvidence::RustSymbol(evidence)],
        Vec::new(),
        false,
    )
    .expect("memory record")
}

fn projected_occurrence() -> MemoryRecallOccurrence {
    MemoryRecallOccurrence::new(
        RepositoryPath::try_from_bytes(b"src/lib.rs", PATH_LIMITS).expect("path"),
        SourceContentDigest::new([0x66; 32]),
        AnalysisArtifactDigest::new([0x77; 32]),
        0,
        DeclarationDigest::new([0x88; 32]),
        CorrespondenceFingerprintDigest::new([0x99; 32]),
    )
}

fn current_record(record_byte: u8) -> MemoryRecallRecord {
    let record = semantic_record(record_byte, MemoryLifecycle::Active);
    let evidence = MemoryRecallEvidence::try_new(
        MemoryRecallEvidenceOutcome::Exact,
        MemoryRecallEvidenceAssurance::Automatic,
        Some(projected_occurrence()),
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
    .expect("current record")
}

fn stale_record(record_byte: u8) -> MemoryRecallRecord {
    let record = semantic_record(record_byte, MemoryLifecycle::Stale);
    MemoryRecallRecord::try_new(
        record.header().record_id(),
        Some(CanonicalMemoryDigest::new([record_byte; 32])),
        Some(record),
        MemoryEffectiveState::Stale,
        MemoryProjectionValidityState::NotEvaluated,
        MemoryRecallEvidenceState::NotEvaluated,
        MemoryRecallReason::AuthoredStale,
        0,
        0,
        0,
        0,
        1,
        0,
        Vec::new(),
    )
    .expect("stale record")
}

struct FakeMemoryPort {
    result: RefCell<Option<MemoryRecallPortResult<u64, u64>>>,
}

impl MemoryRecallPort for FakeMemoryPort {
    type Generation = u64;
    type Projection = u64;
    type Error = std::convert::Infallible;

    fn recall(
        &self,
        _repository: RepositoryIdentityDigest,
        _query: &MemoryRecallQuery,
        _limits: MemoryRecallLimits,
        _cancelled: Arc<AtomicBool>,
        _deadline: Instant,
    ) -> Result<MemoryRecallPortResult<Self::Generation, Self::Projection>, Self::Error> {
        Ok(self.result.borrow_mut().take().expect("one memory recall"))
    }
}

fn recalled_memory() -> MemoryRecallResult<u64, u64> {
    let records = vec![current_record(1), stale_record(2)];
    let output_bytes = records
        .iter()
        .map(MemoryRecallRecord::encoded_output_bytes)
        .try_fold(0_u64, |sum, value| sum.checked_add(value.ok()?))
        .expect("output bytes");
    let port = FakeMemoryPort {
        result: RefCell::new(Some(MemoryRecallPortResult::new(
            snapshot(),
            7,
            9,
            3,
            MemoryRevalidationTarget::worktree(snapshot(), None),
            MemoryRecallProducer::try_new(
                "repowitness.rust.correspondence".to_owned(),
                1,
                CorrespondenceProfileDigest::new([0xaa; 32]),
            )
            .expect("producer"),
            MemoryRecallProjectionCoverage::new(2, 0, 0, 0, 2, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0),
            records,
            2,
            output_bytes,
            1024,
        ))),
    };
    memory_recall(
        &port,
        MemoryRecallRequest::new(
            repository(),
            MemoryRecallQuery::all(),
            MemoryRecallLimits::default(),
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        ),
    )
    .expect("memory recall")
}

#[test]
fn budget_boundaries_are_explicit() {
    assert_eq!(
        ContextBuildBudget::try_new(0),
        Err(ContextBuildError::InvalidBudget)
    );
    assert_eq!(
        ContextBuildBudget::try_new(MAX_CONTEXT_BUILD_BUDGET_UNITS + 1),
        Err(ContextBuildError::InvalidBudget)
    );
    assert_eq!(
        ContextBuildBudget::try_new(MAX_CONTEXT_BUILD_BUDGET_UNITS)
            .expect("inclusive ceiling")
            .units(),
        MAX_CONTEXT_BUILD_BUDGET_UNITS
    );
    assert_eq!(
        ContextBudgetEstimator::Utf8BytesUpperBoundV1.label(),
        "utf8_bytes_upper_bound_v1"
    );
}

#[test]
fn source_only_pack_skips_complete_oversize_items_and_reports_every_gap() {
    let source = source_input(
        7,
        3,
        2,
        vec![
            source_candidate(1, 0, "oversize"),
            source_candidate(2, 1, "x"),
        ],
    );
    let result = compile_context(
        source,
        None::<&MemoryRecallResult<u64, u64>>,
        ContextBuildBudget::try_new(1).expect("budget"),
        &AtomicBool::new(false),
        Instant::now() + Duration::from_secs(1),
    )
    .expect("context");

    assert_eq!(result.used_units(), 1);
    assert_eq!(result.items().len(), 1);
    assert!(matches!(result.items()[0], ContextItem::Source(_)));
    assert_eq!(result.items()[0].rank().provider_rank(), 2);
    assert_eq!(result.items()[0].rank().fused_rank(), 2);
    assert_eq!(
        result.items()[0].rank().reciprocal_rank_denominator(),
        CONTEXT_BUILD_RRF_K + 2
    );
    assert_eq!(result.coverage().source_included(), 1);
    assert_eq!(result.coverage().source_budget_omitted(), 1);
    assert!(
        result
            .omissions()
            .contains(&ContextOmission::SourceSearchLimit(1))
    );
    assert!(
        result
            .omissions()
            .contains(&ContextOmission::MemoryProjectionUnavailable)
    );
    assert!(result.omissions().contains(&ContextOmission::Budget {
        provider: ContextProvider::Source,
        count: 1,
    }));
}

#[test]
fn current_memory_wins_equal_rank_and_non_current_memory_is_explicitly_omitted() {
    let memory = recalled_memory();
    let result = compile_context(
        source_input(7, 1, 1, vec![source_candidate(1, 0, "publish")]),
        Some(&memory),
        ContextBuildBudget::default(),
        &AtomicBool::new(false),
        Instant::now() + Duration::from_secs(1),
    )
    .expect("context");

    assert!(matches!(result.items()[0], ContextItem::Memory(_)));
    assert!(matches!(result.items()[1], ContextItem::Source(_)));
    assert_eq!(result.items()[0].rank().provider_rank(), 1);
    assert_eq!(result.items()[1].rank().provider_rank(), 1);
    assert_eq!(result.coverage().memory_included(), 1);
    assert_eq!(result.coverage().memory_non_current_omitted(), 1);
    assert_eq!(result.memory().expect("projection").projection(), &9);
    assert_eq!(result.memory().expect("projection").source_epoch(), 3);
    assert!(
        result
            .omissions()
            .contains(&ContextOmission::MemoryNotCurrent(1))
    );
}

#[test]
fn source_and_memory_generation_mismatch_fails_closed() {
    let memory = recalled_memory();
    let error = compile_context(
        source_input(8, 0, 0, Vec::new()),
        Some(&memory),
        ContextBuildBudget::default(),
        &AtomicBool::new(false),
        Instant::now() + Duration::from_secs(1),
    )
    .expect_err("generation mismatch");
    assert_eq!(error, ContextBuildError::ContextMismatch);
}

#[test]
fn invalid_ranks_candidates_cancellation_and_deadlines_fail_closed() {
    let duplicate_ranks = ContextSourceInput::try_new(
        repository(),
        crate::CodeSearchQuery::try_new("x")
            .expect("query")
            .digest(),
        snapshot(),
        7,
        CoverageSummary::new(
            CoverageItemCount::new(1),
            CoverageItemCount::new(0),
            CoverageItemCount::new(0),
            CoverageItemCount::new(0),
        ),
        2,
        2,
        vec![source_candidate(1, 0, "a"), source_candidate(1, 1, "b")],
    );
    assert!(matches!(
        duplicate_ranks,
        Err(ContextBuildError::InvalidSourceInput)
    ));

    let mut selector = source_candidate(1, 0, "valid").selector().clone();
    selector = SymbolGetSelector::new(
        selector.path().clone(),
        selector.content_digest(),
        AnalysisArtifactDigest::new([0xff; 32]),
        selector.fact_ordinal(),
    );
    let valid = source_candidate(1, 0, "valid");
    assert!(matches!(
        ContextSourceCandidate::try_new(
            1,
            selector,
            valid.occurrence().clone(),
            valid.declaration().to_vec().into_boxed_slice(),
        ),
        Err(ContextBuildError::InvalidSourceCandidate)
    ));

    let cancelled = AtomicBool::new(true);
    assert_eq!(
        compile_context(
            source_input(7, 0, 0, Vec::new()),
            None::<&MemoryRecallResult<u64, u64>>,
            ContextBuildBudget::default(),
            &cancelled,
            Instant::now() + Duration::from_secs(1),
        )
        .expect_err("cancelled"),
        ContextBuildError::Cancelled
    );
    assert_eq!(
        compile_context(
            source_input(7, 0, 0, Vec::new()),
            None::<&MemoryRecallResult<u64, u64>>,
            ContextBuildBudget::default(),
            &AtomicBool::new(false),
            Instant::now(),
        )
        .expect_err("deadline"),
        ContextBuildError::DeadlineExceeded
    );
}
