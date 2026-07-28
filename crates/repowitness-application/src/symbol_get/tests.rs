use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use repowitness_analysis::RustSymbolKind;
use repowitness_domain::{
    AnalysisArtifactDigest, ByteOffset, ByteSpan, EvidenceLocation, ProducerManifestDigest,
    RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits, ResolutionStatus,
    SourceContentDigest, SourceSnapshotDigest,
};

use super::{
    MAX_SYMBOL_GET_DECLARATION_BYTES, MAX_SYMBOL_GET_OUTPUT_BYTES, SYMBOL_GET_PROFILE_VERSION,
    SymbolGetCandidate, SymbolGetError, SymbolGetLimits, SymbolGetPort, SymbolGetPortOutputError,
    SymbolGetPortRequest, SymbolGetPortResult, SymbolGetProducer, SymbolGetRequest,
    SymbolGetSelector, symbol_get,
};
use crate::{RustIndexCoverage, RustSymbolOccurrence, SourceArtifactEvidence, SourceLanguage};

const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(128, 8);
const DECLARATION: &[u8] = b"fn Widget() {}";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FakeError {
    Failed,
}

impl std::fmt::Display for FakeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("fake symbol retrieval failed")
    }
}

impl std::error::Error for FakeError {}

struct FakePort {
    calls: Cell<u64>,
    result: Cell<Option<Result<SymbolGetPortResult<u64>, FakeError>>>,
}

impl FakePort {
    fn with(result: Result<SymbolGetPortResult<u64>, FakeError>) -> Self {
        Self {
            calls: Cell::new(0),
            result: Cell::new(Some(result)),
        }
    }
}

impl SymbolGetPort for FakePort {
    type Generation = u64;
    type Error = FakeError;

    fn get(
        &self,
        _request: SymbolGetPortRequest<Self::Generation>,
    ) -> Result<SymbolGetPortResult<Self::Generation>, Self::Error> {
        self.calls.set(self.calls.get() + 1);
        self.result
            .take()
            .expect("fake port should be called at most once")
    }
}

fn path_for_language(language: SourceLanguage) -> RepositoryPath {
    let bytes = match language {
        SourceLanguage::Rust => b"src/lib.rs".as_slice(),
        SourceLanguage::Go => b"src/lib.go".as_slice(),
        SourceLanguage::TypeScript => b"src/lib.ts".as_slice(),
        SourceLanguage::Tsx => b"src/lib.tsx".as_slice(),
        SourceLanguage::Python => b"src/lib.py".as_slice(),
    };
    RepositoryPath::try_from_bytes(bytes, PATH_LIMITS).expect("fixture path is valid")
}

fn path() -> RepositoryPath {
    path_for_language(SourceLanguage::Rust)
}

fn selector_for_language(language: SourceLanguage) -> SymbolGetSelector {
    SymbolGetSelector::new(
        path_for_language(language),
        SourceContentDigest::new([3; 32]),
        AnalysisArtifactDigest::new([4; 32]),
        5,
    )
}

fn occurrence() -> RustSymbolOccurrence {
    occurrence_for_language(SourceLanguage::Rust)
}

fn occurrence_for_language(language: SourceLanguage) -> RustSymbolOccurrence {
    let producer_manifest = match language {
        SourceLanguage::Rust => ProducerManifestDigest::new([6; 32]),
        SourceLanguage::Go => ProducerManifestDigest::new([7; 32]),
        SourceLanguage::TypeScript => ProducerManifestDigest::new([8; 32]),
        SourceLanguage::Tsx => ProducerManifestDigest::new([9; 32]),
        SourceLanguage::Python => ProducerManifestDigest::new([10; 32]),
    };
    RustSymbolOccurrence::try_new(
        5,
        SourceArtifactEvidence::new(AnalysisArtifactDigest::new([4; 32]), producer_manifest),
        RustSymbolKind::Function,
        "Widget".to_owned(),
        "fixture::Widget".to_owned(),
        ByteSpan::try_new(ByteOffset::new(3), ByteOffset::new(9))
            .expect("fixture name span is valid"),
        ByteSpan::try_new(ByteOffset::new(0), ByteOffset::new(14))
            .expect("fixture declaration span is valid"),
    )
    .expect("fixture occurrence is valid")
    .with_language(language)
}

fn candidate() -> SymbolGetCandidate {
    candidate_for_language(SourceLanguage::Rust)
}

fn candidate_for_language(language: SourceLanguage) -> SymbolGetCandidate {
    SymbolGetCandidate::new(
        path_for_language(language),
        SourceContentDigest::new([3; 32]),
        occurrence_for_language(language),
        Box::from(DECLARATION),
    )
}

fn result(candidate: Option<SymbolGetCandidate>) -> SymbolGetPortResult<u64> {
    SymbolGetPortResult::new(
        SourceSnapshotDigest::new([2; 32]),
        7,
        RustIndexCoverage::new(8, 2, 1, 3),
        candidate,
    )
}

fn request(
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    limits: SymbolGetLimits,
) -> SymbolGetRequest<u64> {
    request_for_language(SourceLanguage::Rust, cancelled, deadline, limits)
}

fn request_for_language(
    language: SourceLanguage,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    limits: SymbolGetLimits,
) -> SymbolGetRequest<u64> {
    SymbolGetRequest::new(
        RepositoryIdentityDigest::new([1; 32]),
        SourceSnapshotDigest::new([2; 32]),
        7,
        selector_for_language(language),
        limits,
        cancelled,
        deadline,
    )
}

#[test]
fn limits_enforce_inclusive_phase0_ceilings() {
    assert!(
        SymbolGetLimits::try_new(
            MAX_SYMBOL_GET_DECLARATION_BYTES,
            MAX_SYMBOL_GET_OUTPUT_BYTES
        )
        .is_ok()
    );
    assert!(SymbolGetLimits::try_new(0, 1).is_err());
    assert!(SymbolGetLimits::try_new(MAX_SYMBOL_GET_DECLARATION_BYTES + 1, 1).is_err());
    assert!(SymbolGetLimits::try_new(1, 0).is_err());
    assert!(SymbolGetLimits::try_new(1, MAX_SYMBOL_GET_OUTPUT_BYTES + 1).is_err());
}

#[test]
fn exact_candidate_becomes_verified_source_and_attributed_evidence() {
    let port = FakePort::with(Ok(result(Some(candidate()))));
    let material = symbol_get(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
            SymbolGetLimits::default(),
        ),
    )
    .expect("exact symbol lookup should succeed");

    assert_eq!(port.calls.get(), 1);
    assert_eq!(material.resolution(), ResolutionStatus::Confirmed);
    assert_eq!(material.generation(), &7);
    assert_eq!(material.snapshot(), &SourceSnapshotDigest::new([2; 32]));
    assert_eq!(
        material.claim().profile_version(),
        SYMBOL_GET_PROFILE_VERSION
    );
    assert_eq!(
        material
            .claim()
            .symbol()
            .expect("symbol should resolve")
            .declaration(),
        DECLARATION
    );
    assert_eq!(material.coverage().searched().get(), 8);
    assert_eq!(material.coverage().skipped().get(), 2);
    assert_eq!(material.coverage().unresolved().get(), 1);
    assert_eq!(material.coverage().truncated().get(), 3);
    let evidence = material.evidence().as_slice();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].producer().id(), &SymbolGetProducer::RustSyntax);
    assert_eq!(
        evidence[0].producer().version(),
        &ProducerManifestDigest::new([6; 32])
    );
    assert_eq!(evidence[0].tier(), repowitness_domain::EvidenceTier::Syntax);
    let EvidenceLocation::SymbolOccurrence(occurrence) = evidence[0].identity().location() else {
        panic!("symbol evidence should identify one occurrence");
    };
    assert_eq!(occurrence.name(), "Widget");
}

#[test]
fn non_rust_candidates_use_their_exact_producer_classes() {
    for (language, expected) in [
        (SourceLanguage::Go, SymbolGetProducer::GoSyntax),
        (
            SourceLanguage::TypeScript,
            SymbolGetProducer::TypeScriptSyntax,
        ),
        (SourceLanguage::Tsx, SymbolGetProducer::TsxSyntax),
    ] {
        let port = FakePort::with(Ok(result(Some(candidate_for_language(language)))));
        let material = symbol_get(
            &port,
            request_for_language(
                language,
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
                SymbolGetLimits::default(),
            ),
        )
        .expect("supported-language symbol lookup should succeed");

        let evidence = material.evidence().as_slice();
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].producer().id(), &expected);
        assert_eq!(
            evidence[0].producer().version(),
            &occurrence_for_language(language).producer_manifest()
        );
        let EvidenceLocation::SymbolOccurrence(occurrence) = evidence[0].identity().location()
        else {
            panic!("symbol evidence should identify one occurrence");
        };
        assert_eq!(occurrence.language(), language);
    }
}

#[test]
fn language_and_repository_path_must_agree() {
    let mismatched = SymbolGetCandidate::new(
        path(),
        SourceContentDigest::new([3; 32]),
        occurrence_for_language(SourceLanguage::Go),
        Box::from(DECLARATION),
    );
    let port = FakePort::with(Ok(result(Some(mismatched))));

    assert!(matches!(
        symbol_get(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
                SymbolGetLimits::default(),
            )
        ),
        Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::LanguagePathMismatch
        ))
    ));
}

#[test]
fn a_missing_exact_occurrence_abstains_with_unresolved_coverage() {
    let port = FakePort::with(Ok(result(None)));
    let material = symbol_get(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
            SymbolGetLimits::default(),
        ),
    )
    .expect("missing symbol should be an unresolved result");

    assert_eq!(material.resolution(), ResolutionStatus::Unresolved);
    assert!(material.claim().symbol().is_none());
    assert!(material.evidence().is_empty());
    assert_eq!(material.coverage().unresolved().get(), 2);
}

#[test]
fn context_selector_and_declaration_mismatches_fail_closed() {
    let wrong_context = SymbolGetPortResult::new(
        SourceSnapshotDigest::new([9; 32]),
        7,
        RustIndexCoverage::new(1, 0, 0, 0),
        Some(candidate()),
    );
    let port = FakePort::with(Ok(wrong_context));
    assert!(matches!(
        symbol_get(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
                SymbolGetLimits::default(),
            )
        ),
        Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::ContextMismatch
        ))
    ));

    let wrong_selector = SymbolGetCandidate::new(
        path(),
        SourceContentDigest::new([8; 32]),
        occurrence(),
        Box::from(DECLARATION),
    );
    let port = FakePort::with(Ok(result(Some(wrong_selector))));
    assert!(matches!(
        symbol_get(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
                SymbolGetLimits::default(),
            )
        ),
        Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::SelectorMismatch
        ))
    ));

    let wrong_source = SymbolGetCandidate::new(
        path(),
        SourceContentDigest::new([3; 32]),
        occurrence(),
        Box::from(&b"fn Gadget() {}"[..]),
    );
    let port = FakePort::with(Ok(result(Some(wrong_source))));
    assert!(matches!(
        symbol_get(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
                SymbolGetLimits::default(),
            )
        ),
        Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::InvalidDeclaration
        ))
    ));
}

#[test]
fn declaration_and_aggregate_output_bounds_are_rechecked_by_the_use_case() {
    let declaration_limit =
        SymbolGetLimits::try_new(13, MAX_SYMBOL_GET_OUTPUT_BYTES).expect("limits are valid");
    let port = FakePort::with(Ok(result(Some(candidate()))));
    assert!(matches!(
        symbol_get(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
                declaration_limit,
            )
        ),
        Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::DeclarationLimitExceeded
        ))
    ));

    let output_limit =
        SymbolGetLimits::try_new(MAX_SYMBOL_GET_DECLARATION_BYTES, 200).expect("limits valid");
    let port = FakePort::with(Ok(result(Some(candidate()))));
    assert!(matches!(
        symbol_get(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
                output_limit,
            )
        ),
        Err(SymbolGetError::InvalidPortOutput(
            SymbolGetPortOutputError::OutputByteLimitExceeded
        ))
    ));
}

#[test]
fn cancellation_deadline_port_errors_and_debug_output_remain_safe() {
    let cancelled_port = FakePort::with(Err(FakeError::Failed));
    let cancelled = symbol_get(
        &cancelled_port,
        request(
            Arc::new(AtomicBool::new(true)),
            Instant::now() + Duration::from_secs(1),
            SymbolGetLimits::default(),
        ),
    )
    .expect_err("pre-cancelled work should fail");
    assert!(matches!(cancelled, SymbolGetError::Cancelled));
    assert_eq!(cancelled_port.calls.get(), 0);

    let deadline_port = FakePort::with(Err(FakeError::Failed));
    let elapsed = symbol_get(
        &deadline_port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now(),
            SymbolGetLimits::default(),
        ),
    )
    .expect_err("elapsed deadline should fail");
    assert!(matches!(elapsed, SymbolGetError::DeadlineExceeded));
    assert_eq!(deadline_port.calls.get(), 0);

    let failure_port = FakePort::with(Err(FakeError::Failed));
    let failure = symbol_get(
        &failure_port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
            SymbolGetLimits::default(),
        ),
    )
    .expect_err("port failure should remain distinct");
    assert!(matches!(failure, SymbolGetError::Port(FakeError::Failed)));

    struct CancellingPort;
    impl SymbolGetPort for CancellingPort {
        type Generation = u64;
        type Error = FakeError;

        fn get(
            &self,
            request: SymbolGetPortRequest<Self::Generation>,
        ) -> Result<SymbolGetPortResult<Self::Generation>, Self::Error> {
            request.cancelled().store(true, Ordering::Release);
            Ok(result(Some(candidate())))
        }
    }
    let request = request(
        Arc::new(AtomicBool::new(false)),
        Instant::now() + Duration::from_secs(1),
        SymbolGetLimits::default(),
    );
    let debug = format!("{request:?}");
    assert!(debug.contains("SymbolGetSelector"));
    assert!(!debug.contains("src/lib.rs"));
    assert!(matches!(
        symbol_get(&CancellingPort, request),
        Err(SymbolGetError::Cancelled)
    ));
}
