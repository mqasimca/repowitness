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
    AnalysisArtifactDigest, ByteOffset, ByteSpan, EvidenceLocation, RepositoryPath,
    RepositoryPathLimits, ResolutionStatus, SourceContentDigest, SourceSnapshotDigest,
};

use super::{
    CodeSearchCandidate, CodeSearchError, CodeSearchLimits, CodeSearchPort,
    CodeSearchPortOutputError, CodeSearchPortResult, CodeSearchProducer, CodeSearchQuery,
    CodeSearchQueryError, CodeSearchRequest, MAX_CODE_SEARCH_OUTPUT_BYTES, MAX_CODE_SEARCH_RESULTS,
    RustIndexCoverage, RustSymbolOccurrence, SourceArtifactEvidence, code_search,
};
use crate::{RelevantPathsError, RelevantPathsLimits, SourceLanguage, locate_relevant_paths};

const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(128, 8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FakeError {
    Failed,
}

impl std::fmt::Display for FakeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("fake retrieval failed")
    }
}

impl std::error::Error for FakeError {}

struct FakePort {
    calls: Cell<u64>,
    result: Cell<Option<Result<CodeSearchPortResult<u64>, FakeError>>>,
}

impl FakePort {
    fn with(result: Result<CodeSearchPortResult<u64>, FakeError>) -> Self {
        Self {
            calls: Cell::new(0),
            result: Cell::new(Some(result)),
        }
    }
}

impl CodeSearchPort for FakePort {
    type Generation = u64;
    type Error = FakeError;

    fn search(
        &self,
        _repository: repowitness_domain::RepositoryIdentityDigest,
        _query: &CodeSearchQuery,
        _limits: CodeSearchLimits,
        _cancelled: Arc<AtomicBool>,
        _deadline: Instant,
    ) -> Result<CodeSearchPortResult<Self::Generation>, Self::Error> {
        self.calls.set(self.calls.get() + 1);
        self.result
            .take()
            .expect("fake port should be called at most once")
    }
}

fn candidate(ordinal: u64, name: &str) -> CodeSearchCandidate {
    candidate_for_language(ordinal, name, SourceLanguage::Rust)
}

fn candidate_for_language(
    ordinal: u64,
    name: &str,
    language: SourceLanguage,
) -> CodeSearchCandidate {
    let extension = match language {
        SourceLanguage::Rust => "rs",
        SourceLanguage::Go => "go",
        SourceLanguage::TypeScript => "ts",
        SourceLanguage::Tsx => "tsx",
        SourceLanguage::Python => "py",
    };
    candidate_for_path(
        ordinal,
        name,
        language,
        &format!("src/{name}.{extension}"),
        SourceContentDigest::new([3; 32]),
    )
}

fn candidate_for_path(
    ordinal: u64,
    name: &str,
    language: SourceLanguage,
    path: &str,
    content_digest: SourceContentDigest,
) -> CodeSearchCandidate {
    let name_start = 10 + ordinal;
    let name_end = name_start + u64::try_from(name.len()).expect("fixture length fits");
    let producer_manifest = match language {
        SourceLanguage::Rust => repowitness_domain::ProducerManifestDigest::new([5; 32]),
        SourceLanguage::Go => repowitness_domain::ProducerManifestDigest::new([6; 32]),
        SourceLanguage::TypeScript => repowitness_domain::ProducerManifestDigest::new([7; 32]),
        SourceLanguage::Tsx => repowitness_domain::ProducerManifestDigest::new([8; 32]),
        SourceLanguage::Python => repowitness_domain::ProducerManifestDigest::new([9; 32]),
    };
    let occurrence = RustSymbolOccurrence::try_new(
        ordinal,
        SourceArtifactEvidence::new(AnalysisArtifactDigest::new([4; 32]), producer_manifest),
        RustSymbolKind::Function,
        name.to_owned(),
        format!("fixture::{name}"),
        ByteSpan::try_new(ByteOffset::new(name_start), ByteOffset::new(name_end))
            .expect("fixture span is valid"),
        ByteSpan::try_new(ByteOffset::new(0), ByteOffset::new(name_end + 2))
            .expect("fixture declaration is valid"),
    )
    .expect("fixture occurrence is valid")
    .with_language(language);
    CodeSearchCandidate::new(
        RepositoryPath::try_from_bytes(path.as_bytes(), PATH_LIMITS)
            .expect("fixture path is valid"),
        content_digest,
        occurrence,
    )
}

fn result(candidates: Vec<CodeSearchCandidate>, total_matches: u64) -> CodeSearchPortResult<u64> {
    CodeSearchPortResult::new(
        SourceSnapshotDigest::new([2; 32]),
        7,
        RustIndexCoverage::new(8, 2, 1, 0),
        candidates,
        total_matches,
        512,
    )
}

fn request(cancelled: Arc<AtomicBool>, deadline: Instant) -> CodeSearchRequest {
    CodeSearchRequest::new(
        repowitness_domain::RepositoryIdentityDigest::new([1; 32]),
        CodeSearchQuery::try_new("  Widget\t run ").expect("query is valid"),
        CodeSearchLimits::default(),
        cancelled,
        deadline,
    )
}

#[test]
fn query_admission_is_canonical_bounded_and_redacted() {
    let first = CodeSearchQuery::try_new("  Widget\t run ").expect("query is valid");
    let second = CodeSearchQuery::try_new("Widget run").expect("query is valid");
    assert_eq!(first, second);
    assert_eq!(first.as_str(), "Widget run");
    assert_eq!(first.term_count(), 2);
    assert_eq!(first.digest(), second.digest());
    let debug = format!("{first:?}");
    assert!(debug.contains("<redacted-query>"));
    assert!(!debug.contains("Widget"));

    assert_eq!(
        CodeSearchQuery::try_new(""),
        Err(CodeSearchQueryError::Empty)
    );
    assert_eq!(
        CodeSearchQuery::try_new(&"x".repeat(257)),
        Err(CodeSearchQueryError::QueryTooLong)
    );
    assert_eq!(
        CodeSearchQuery::try_new("1 2 3 4 5 6 7 8 9"),
        Err(CodeSearchQueryError::TooManyTerms)
    );
    assert_eq!(
        CodeSearchQuery::try_new(&"x".repeat(65)),
        Err(CodeSearchQueryError::TermTooLong)
    );
    assert_eq!(
        CodeSearchQuery::try_new("private\0term"),
        Err(CodeSearchQueryError::InvalidTerm)
    );
}

#[test]
fn limits_enforce_inclusive_phase0_ceilings() {
    assert!(
        CodeSearchLimits::try_new(MAX_CODE_SEARCH_RESULTS, MAX_CODE_SEARCH_OUTPUT_BYTES).is_ok()
    );
    assert!(CodeSearchLimits::try_new(0, 1).is_err());
    assert!(CodeSearchLimits::try_new(MAX_CODE_SEARCH_RESULTS + 1, 1).is_err());
    assert!(CodeSearchLimits::try_new(1, 0).is_err());
    assert!(CodeSearchLimits::try_new(1, MAX_CODE_SEARCH_OUTPUT_BYTES + 1).is_err());
}

#[test]
fn candidates_become_ordered_attributed_evidence_with_exact_coverage() {
    let port = FakePort::with(Ok(result(
        vec![
            candidate(0, "Widget"),
            candidate_for_language(1, "run", SourceLanguage::Go),
            candidate_for_language(2, "load", SourceLanguage::TypeScript),
            candidate_for_language(3, "View", SourceLanguage::Tsx),
        ],
        5,
    )));
    let material = code_search(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        ),
    )
    .expect("search should succeed");

    assert_eq!(port.calls.get(), 1);
    assert_eq!(material.resolution(), ResolutionStatus::Confirmed);
    assert_eq!(material.claim().returned_matches(), 4);
    assert_eq!(material.claim().total_matches(), 5);
    assert_eq!(material.generation(), &7);
    assert_eq!(material.snapshot(), &SourceSnapshotDigest::new([2; 32]));
    assert_eq!(material.coverage().searched().get(), 8);
    assert_eq!(material.coverage().skipped().get(), 2);
    assert_eq!(material.coverage().unresolved().get(), 1);
    assert_eq!(material.coverage().truncated().get(), 1);
    let evidence = material.evidence().as_slice();
    assert_eq!(evidence.len(), 4);
    assert_eq!(evidence[0].producer().id(), &CodeSearchProducer::RustSyntax);
    assert_eq!(
        evidence[0].producer().version(),
        &repowitness_domain::ProducerManifestDigest::new([5; 32])
    );
    assert_eq!(evidence[0].tier(), repowitness_domain::EvidenceTier::Syntax);
    assert_eq!(
        evidence[0].relation(),
        repowitness_domain::EvidenceRelation::Supports
    );
    let EvidenceLocation::SymbolOccurrence(first) = evidence[0].identity().location() else {
        panic!("candidate evidence should identify a symbol occurrence");
    };
    assert_eq!(first.name(), "Widget");
    let EvidenceLocation::SymbolOccurrence(second) = evidence[1].identity().location() else {
        panic!("candidate evidence should identify a symbol occurrence");
    };
    assert_eq!(second.name(), "run");
    assert_eq!(second.language(), SourceLanguage::Go);
    assert_eq!(evidence[1].producer().id(), &CodeSearchProducer::GoSyntax);
    assert_eq!(
        evidence[1].producer().version(),
        &repowitness_domain::ProducerManifestDigest::new([6; 32])
    );
    assert_eq!(
        evidence[2].producer().id(),
        &CodeSearchProducer::TypeScriptSyntax
    );
    assert_eq!(
        evidence[2].producer().version(),
        &repowitness_domain::ProducerManifestDigest::new([7; 32])
    );
    assert_eq!(evidence[3].producer().id(), &CodeSearchProducer::TsxSyntax);
    assert_eq!(
        evidence[3].producer().version(),
        &repowitness_domain::ProducerManifestDigest::new([8; 32])
    );
}

#[test]
fn lexical_path_navigation_qualifies_candidate_truncation_from_path_presentation() {
    let port = FakePort::with(Ok(result(
        vec![
            candidate_for_path(
                4,
                "first",
                SourceLanguage::Rust,
                "src/shared.rs",
                SourceContentDigest::new([3; 32]),
            ),
            candidate_for_path(
                2,
                "second",
                SourceLanguage::Rust,
                "src/shared.rs",
                SourceContentDigest::new([3; 32]),
            ),
            candidate_for_path(
                1,
                "third",
                SourceLanguage::Rust,
                "src/other.rs",
                SourceContentDigest::new([6; 32]),
            ),
        ],
        4,
    )));
    let search = code_search(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        ),
    )
    .expect("search should succeed");
    let result = locate_relevant_paths(
        search,
        RelevantPathsLimits::try_new(2).expect("limit is valid"),
    )
    .expect("projection should succeed");

    assert_eq!(result.search().claim().returned_matches(), 3);
    assert_eq!(result.search().claim().total_matches(), 4);
    assert_eq!(result.search().coverage().truncated().get(), 1);
    // Two is exact only for the three returned candidates, never a claim that
    // the omitted fourth match has no distinct path.
    assert_eq!(result.returned_match_paths_total(), 2);
    assert!(!result.returned_match_paths_truncated());
    let paths = result.paths().as_slice();
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0].matching_declarations(), 2);
    assert_eq!(paths[0].first_fact_ordinal(), 2);
    assert_eq!(
        paths[0].path(),
        &RepositoryPath::try_from_bytes(b"src/shared.rs", PATH_LIMITS)
            .expect("fixture path is valid")
    );
    assert_eq!(paths[1].matching_declarations(), 1);
    assert_eq!(
        paths[1].path(),
        &RepositoryPath::try_from_bytes(b"src/other.rs", PATH_LIMITS)
            .expect("fixture path is valid")
    );
}

#[test]
fn lexical_path_navigation_preserves_an_unresolved_empty_search() {
    let port = FakePort::with(Ok(result(Vec::new(), 0)));
    let search = code_search(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        ),
    )
    .expect("empty search should remain a valid material result");
    let paths = locate_relevant_paths(search, RelevantPathsLimits::default())
        .expect("empty projection should succeed");

    assert!(paths.paths().as_slice().is_empty());
    assert_eq!(paths.search().resolution(), ResolutionStatus::Unresolved);
    assert_eq!(paths.search().coverage().unresolved().get(), 2);
}

#[test]
fn lexical_path_navigation_reports_path_presentation_truncation_separately() {
    let port = FakePort::with(Ok(result(
        vec![
            candidate_for_path(
                0,
                "first",
                SourceLanguage::Rust,
                "src/first.rs",
                SourceContentDigest::new([3; 32]),
            ),
            candidate_for_path(
                1,
                "second",
                SourceLanguage::Rust,
                "src/second.rs",
                SourceContentDigest::new([4; 32]),
            ),
        ],
        2,
    )));
    let search = code_search(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        ),
    )
    .expect("search should succeed");
    let result = locate_relevant_paths(
        search,
        RelevantPathsLimits::try_new(1).expect("limit is valid"),
    )
    .expect("projection should succeed");

    assert_eq!(result.search().claim().returned_matches(), 2);
    assert_eq!(result.returned_match_paths_total(), 2);
    assert!(result.returned_match_paths_truncated());
    assert_eq!(result.paths().as_slice().len(), 1);
}

#[test]
fn lexical_path_navigation_rejects_conflicting_content_for_one_path() {
    let port = FakePort::with(Ok(result(
        vec![
            candidate_for_path(
                0,
                "first",
                SourceLanguage::Rust,
                "src/shared.rs",
                SourceContentDigest::new([3; 32]),
            ),
            candidate_for_path(
                1,
                "second",
                SourceLanguage::Rust,
                "src/shared.rs",
                SourceContentDigest::new([4; 32]),
            ),
        ],
        2,
    )));
    let search = code_search(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        ),
    )
    .expect("individual candidates satisfy the code-search contract");

    assert!(matches!(
        locate_relevant_paths(search, RelevantPathsLimits::default()),
        Err(RelevantPathsError::InconsistentPathContent)
    ));
}

#[test]
fn an_empty_candidate_set_abstains_and_reports_unresolved_scope() {
    let port = FakePort::with(Ok(result(Vec::new(), 0)));
    let material = code_search(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        ),
    )
    .expect("empty search should be a valid unresolved result");

    assert_eq!(material.resolution(), ResolutionStatus::Unresolved);
    assert!(material.evidence().is_empty());
    assert_eq!(material.coverage().unresolved().get(), 2);
}

#[test]
fn cancellation_deadline_and_port_failures_remain_distinct() {
    let cancelled = Arc::new(AtomicBool::new(true));
    let cancelled_port = FakePort::with(Err(FakeError::Failed));
    let cancelled_error = code_search(
        &cancelled_port,
        request(cancelled, Instant::now() + Duration::from_secs(1)),
    )
    .expect_err("pre-cancelled work should fail");
    assert!(matches!(cancelled_error, CodeSearchError::Cancelled));
    assert_eq!(cancelled_port.calls.get(), 0);

    let deadline_port = FakePort::with(Err(FakeError::Failed));
    let deadline_error = code_search(
        &deadline_port,
        request(Arc::new(AtomicBool::new(false)), Instant::now()),
    )
    .expect_err("elapsed deadline should fail");
    assert!(matches!(deadline_error, CodeSearchError::DeadlineExceeded));
    assert_eq!(deadline_port.calls.get(), 0);

    let failure_port = FakePort::with(Err(FakeError::Failed));
    let failure = code_search(
        &failure_port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        ),
    )
    .expect_err("adapter failure should remain distinct");
    assert!(matches!(failure, CodeSearchError::Port(FakeError::Failed)));
}

#[test]
fn invalid_adapter_counts_and_bytes_fail_closed() {
    let too_many = (0..21)
        .map(|ordinal| candidate(ordinal, &format!("item{ordinal}")))
        .collect();
    let port = FakePort::with(Ok(result(too_many, 21)));
    assert!(matches!(
        code_search(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1)
            )
        ),
        Err(CodeSearchError::InvalidPortOutput(
            CodeSearchPortOutputError::CandidateLimitExceeded
        ))
    ));

    let port = FakePort::with(Ok(result(vec![candidate(0, "item")], 0)));
    assert!(matches!(
        code_search(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1)
            )
        ),
        Err(CodeSearchError::InvalidPortOutput(
            CodeSearchPortOutputError::InvalidTotalMatches
        ))
    ));

    let oversized = CodeSearchPortResult::new(
        SourceSnapshotDigest::new([2; 32]),
        7,
        RustIndexCoverage::new(1, 0, 0, 0),
        vec![candidate(0, "item")],
        1,
        256 * 1024 + 1,
    );
    let port = FakePort::with(Ok(oversized));
    assert!(matches!(
        code_search(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1)
            )
        ),
        Err(CodeSearchError::InvalidPortOutput(
            CodeSearchPortOutputError::OutputByteLimitExceeded
        ))
    ));

    let mut mismatched = candidate_for_language(0, "item", SourceLanguage::Go);
    mismatched.path =
        RepositoryPath::try_from_bytes(b"src/item.rs", PATH_LIMITS).expect("fixture path is valid");
    let port = FakePort::with(Ok(result(vec![mismatched], 1)));
    assert!(matches!(
        code_search(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1)
            )
        ),
        Err(CodeSearchError::InvalidPortOutput(
            CodeSearchPortOutputError::InvalidCandidate
        ))
    ));
}

#[test]
fn request_debug_and_post_port_cancellation_do_not_expose_query_text() {
    struct CancellingPort;

    impl CodeSearchPort for CancellingPort {
        type Generation = u64;
        type Error = FakeError;

        fn search(
            &self,
            _repository: repowitness_domain::RepositoryIdentityDigest,
            _query: &CodeSearchQuery,
            _limits: CodeSearchLimits,
            cancelled: Arc<AtomicBool>,
            _deadline: Instant,
        ) -> Result<CodeSearchPortResult<Self::Generation>, Self::Error> {
            cancelled.store(true, Ordering::Release);
            Ok(result(vec![candidate(0, "private_symbol")], 1))
        }
    }

    let request = request(
        Arc::new(AtomicBool::new(false)),
        Instant::now() + Duration::from_secs(1),
    );
    let debug = format!("{request:?}");
    assert!(!debug.contains("Widget"));
    assert!(!debug.contains("run"));
    assert!(matches!(
        code_search(&CancellingPort, request),
        Err(CodeSearchError::Cancelled)
    ));
}
