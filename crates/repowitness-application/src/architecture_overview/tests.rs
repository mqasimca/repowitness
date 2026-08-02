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
    AnalysisArtifactDigest, ByteOffset, ByteSpan, ProducerManifestDigest, RepositoryPath,
    RepositoryPathLimits, SourceContentDigest, SourceSnapshotDigest,
};

use super::{
    ARCHITECTURE_OVERVIEW_PROFILE_VERSION, ArchitectureOverviewEntryPointCandidate,
    ArchitectureOverviewError, ArchitectureOverviewKindSummary, ArchitectureOverviewLimits,
    ArchitectureOverviewPort, ArchitectureOverviewPortOutputError, ArchitectureOverviewPortResult,
    ArchitectureOverviewRequest, ArchitectureOverviewSourceRoot,
    ArchitectureOverviewSourceRootSummary, DEFAULT_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES,
    DEFAULT_ARCHITECTURE_OVERVIEW_FILES, DEFAULT_ARCHITECTURE_OVERVIEW_ROOTS,
    MAX_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES, MAX_ARCHITECTURE_OVERVIEW_FILES,
    MAX_ARCHITECTURE_OVERVIEW_ROOTS, architecture_overview,
};
use crate::{
    ArchitectureMapFile, ArchitectureMapLanguageSummary, RustIndexCoverage, RustSymbolOccurrence,
    SourceArtifactEvidence, SourceLanguage,
};

const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(128, 8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FakeError {
    Failed,
}

impl std::fmt::Display for FakeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("fake architecture-overview read failed")
    }
}

impl std::error::Error for FakeError {}

struct FakePort {
    calls: Cell<u64>,
    result: Cell<Option<Result<ArchitectureOverviewPortResult<u64>, FakeError>>>,
}

impl FakePort {
    fn with(result: Result<ArchitectureOverviewPortResult<u64>, FakeError>) -> Self {
        Self {
            calls: Cell::new(0),
            result: Cell::new(Some(result)),
        }
    }
}

impl ArchitectureOverviewPort for FakePort {
    type Generation = u64;
    type Error = FakeError;

    fn architecture_overview(
        &self,
        _repository: repowitness_domain::RepositoryIdentityDigest,
        _limits: ArchitectureOverviewLimits,
        _cancelled: Arc<AtomicBool>,
        _deadline: Instant,
    ) -> Result<ArchitectureOverviewPortResult<Self::Generation>, Self::Error> {
        self.calls.set(self.calls.get() + 1);
        self.result
            .take()
            .expect("fake port should be called at most once")
    }
}

fn path(value: &str) -> RepositoryPath {
    RepositoryPath::try_from_bytes(value.as_bytes(), PATH_LIMITS)
        .expect("fixture path should be valid")
}

fn file(path: &str, language: SourceLanguage, declarations: u64) -> ArchitectureMapFile {
    ArchitectureMapFile::new(
        self::path(path),
        language,
        SourceContentDigest::new([3; 32]),
        AnalysisArtifactDigest::new([4; 32]),
        ProducerManifestDigest::new([5; 32]),
        declarations,
    )
}

fn candidate(
    path: &str,
    language: SourceLanguage,
    ordinal: u64,
    name: &str,
    kind: RustSymbolKind,
) -> ArchitectureOverviewEntryPointCandidate {
    let name_end = u64::try_from(name.len()).expect("fixture name length fits");
    let occurrence = RustSymbolOccurrence::try_new(
        ordinal,
        SourceArtifactEvidence::new(
            AnalysisArtifactDigest::new([4; 32]),
            ProducerManifestDigest::new([5; 32]),
        ),
        kind,
        name.to_owned(),
        format!("fixture::{name}"),
        ByteSpan::try_new(ByteOffset::new(0), ByteOffset::new(name_end))
            .expect("fixture name span is valid"),
        ByteSpan::try_new(ByteOffset::new(0), ByteOffset::new(name_end + 4))
            .expect("fixture declaration span is valid"),
    )
    .expect("fixture occurrence is valid")
    .with_language(language);
    ArchitectureOverviewEntryPointCandidate::new(
        self::path(path),
        SourceContentDigest::new([3; 32]),
        occurrence,
    )
}

fn valid_result(
    roots: Vec<ArchitectureOverviewSourceRootSummary>,
    candidates: Vec<ArchitectureOverviewEntryPointCandidate>,
    files: Vec<ArchitectureMapFile>,
    total_roots: u64,
    total_candidates: u64,
) -> ArchitectureOverviewPortResult<u64> {
    ArchitectureOverviewPortResult::new(
        SourceSnapshotDigest::new([2; 32]),
        7,
        ProducerManifestDigest::new([9; 32]),
        RustIndexCoverage::new(8, 2, 1, 0),
        vec![
            ArchitectureMapLanguageSummary::new(SourceLanguage::Go, 1, 1),
            ArchitectureMapLanguageSummary::new(SourceLanguage::Rust, 2, 2),
        ],
        vec![
            ArchitectureOverviewKindSummary::new(SourceLanguage::Go, RustSymbolKind::Function, 1),
            ArchitectureOverviewKindSummary::new(SourceLanguage::Rust, RustSymbolKind::Function, 2),
        ],
        roots,
        candidates,
        files,
        3,
        3,
        total_roots,
        total_candidates,
        2_048,
    )
}

fn complete_result() -> ArchitectureOverviewPortResult<u64> {
    valid_result(
        complete_source_roots(),
        complete_candidates(),
        complete_files(),
        3,
        2,
    )
}

fn complete_source_roots() -> Vec<ArchitectureOverviewSourceRootSummary> {
    vec![
        ArchitectureOverviewSourceRootSummary::new(
            ArchitectureOverviewSourceRoot::repository_root(),
            1,
            1,
        ),
        ArchitectureOverviewSourceRootSummary::new(
            ArchitectureOverviewSourceRoot::top_level_directory(path("go")),
            1,
            1,
        ),
        ArchitectureOverviewSourceRootSummary::new(
            ArchitectureOverviewSourceRoot::top_level_directory(path("src")),
            1,
            1,
        ),
    ]
}

fn complete_candidates() -> Vec<ArchitectureOverviewEntryPointCandidate> {
    vec![
        candidate(
            "go/main.go",
            SourceLanguage::Go,
            0,
            "main",
            RustSymbolKind::Function,
        ),
        candidate(
            "main.rs",
            SourceLanguage::Rust,
            0,
            "main",
            RustSymbolKind::Function,
        ),
    ]
}

fn complete_files() -> Vec<ArchitectureMapFile> {
    vec![
        file("go/main.go", SourceLanguage::Go, 1),
        file("main.rs", SourceLanguage::Rust, 1),
        file("src/lib.rs", SourceLanguage::Rust, 1),
    ]
}

fn request(
    cancelled: Arc<AtomicBool>,
    limits: ArchitectureOverviewLimits,
) -> ArchitectureOverviewRequest {
    ArchitectureOverviewRequest::new(
        repowitness_domain::RepositoryIdentityDigest::new([1; 32]),
        limits,
        cancelled,
        Instant::now() + Duration::from_secs(1),
    )
}

#[test]
fn profile_has_independent_bounded_receipt_limits() {
    let limits = ArchitectureOverviewLimits::default();
    assert_eq!(ARCHITECTURE_OVERVIEW_PROFILE_VERSION, 1);
    assert_eq!(limits.max_roots(), DEFAULT_ARCHITECTURE_OVERVIEW_ROOTS);
    assert_eq!(
        limits.max_entry_point_candidates(),
        DEFAULT_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES
    );
    assert_eq!(limits.max_files(), DEFAULT_ARCHITECTURE_OVERVIEW_FILES);
    assert!(
        ArchitectureOverviewLimits::try_new(
            MAX_ARCHITECTURE_OVERVIEW_ROOTS,
            MAX_ARCHITECTURE_OVERVIEW_ENTRY_POINT_CANDIDATES,
            MAX_ARCHITECTURE_OVERVIEW_FILES,
            1,
        )
        .is_ok()
    );
    assert!(ArchitectureOverviewLimits::try_new(0, 1, 1, 1).is_err());
    assert!(ArchitectureOverviewLimits::try_new(1, 0, 1, 1).is_err());
    assert!(ArchitectureOverviewLimits::try_new(1, 1, 0, 1).is_err());
}

#[test]
fn complete_multilanguage_source_fact_overview_is_preserved() {
    let port = FakePort::with(Ok(complete_result()));
    let overview = architecture_overview(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            ArchitectureOverviewLimits::default(),
        ),
    )
    .expect("complete source-only overview should validate");

    assert_eq!(port.calls.get(), 1);
    assert_eq!(overview.snapshot(), SourceSnapshotDigest::new([2; 32]));
    assert_eq!(overview.generation(), &7);
    assert_eq!(
        overview.source_producer_manifest(),
        ProducerManifestDigest::new([9; 32])
    );
    assert_eq!(
        overview.index_coverage(),
        RustIndexCoverage::new(8, 2, 1, 0)
    );
    assert_eq!(overview.total_files(), 3);
    assert_eq!(overview.total_declarations(), 3);
    assert_eq!(overview.total_source_roots(), 3);
    assert_eq!(overview.total_entry_point_candidates(), 2);
    assert!(!overview.source_roots_truncated());
    assert!(!overview.entry_point_candidates_truncated());
    assert!(!overview.files_truncated());
    assert_eq!(overview.language_summaries().len(), 2);
    assert_eq!(overview.kind_summaries().len(), 2);
    assert_eq!(overview.source_roots().len(), 3);
    assert_eq!(overview.files().len(), 3);
    assert_eq!(overview.entry_point_candidates().len(), 2);
    assert!(matches!(
        overview.source_roots()[0].root(),
        ArchitectureOverviewSourceRoot::RepositoryRoot
    ));
    assert_eq!(
        overview.entry_point_candidates()[0].occurrence().name(),
        "main"
    );
    assert_eq!(
        overview.entry_point_candidates()[0].occurrence().kind(),
        RustSymbolKind::Function
    );
}

#[test]
fn independently_truncated_receipts_remain_explicit_without_changing_totals() {
    let complete = complete_result();
    let ArchitectureOverviewPortResult {
        source_roots,
        entry_point_candidates,
        files,
        ..
    } = complete;
    let port = FakePort::with(Ok(valid_result(
        vec![source_roots[0].clone()],
        vec![entry_point_candidates[0].clone()],
        vec![files[0].clone()],
        3,
        2,
    )));
    let limits = ArchitectureOverviewLimits::try_new(1, 1, 1, 8_192)
        .expect("fixture limits should be valid");
    let overview = architecture_overview(&port, request(Arc::new(AtomicBool::new(false)), limits))
        .expect("independently bounded receipts should validate");
    assert!(overview.source_roots_truncated());
    assert!(overview.entry_point_candidates_truncated());
    assert!(overview.files_truncated());
    assert_eq!(overview.total_files(), 3);
    assert_eq!(overview.total_declarations(), 3);
}

#[test]
fn invalid_entry_point_candidates_fail_closed() {
    let invalid_candidate = valid_result(
        complete_source_roots(),
        vec![candidate(
            "go/main.go",
            SourceLanguage::Go,
            0,
            "run",
            RustSymbolKind::Function,
        )],
        complete_files(),
        3,
        1,
    );
    let port = FakePort::with(Ok(invalid_candidate));
    assert!(matches!(
        architecture_overview(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                ArchitectureOverviewLimits::default()
            )
        ),
        Err(ArchitectureOverviewError::InvalidPortOutput(
            ArchitectureOverviewPortOutputError::InvalidEntryPointCandidate
        ))
    ));
}

#[test]
fn unordered_file_receipts_fail_closed() {
    let unordered_files = valid_result(
        complete_source_roots(),
        complete_candidates(),
        vec![
            file("main.rs", SourceLanguage::Rust, 1),
            file("go/main.go", SourceLanguage::Go, 1),
            file("src/lib.rs", SourceLanguage::Rust, 1),
        ],
        3,
        2,
    );
    let port = FakePort::with(Ok(unordered_files));
    assert!(matches!(
        architecture_overview(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                ArchitectureOverviewLimits::default()
            )
        ),
        Err(ArchitectureOverviewError::InvalidPortOutput(
            ArchitectureOverviewPortOutputError::InvalidFiles
        ))
    ));
}

#[test]
fn invalid_aggregate_totals_and_output_ceiling_fail_closed() {
    let mut invalid_kinds = complete_result();
    invalid_kinds.kind_summaries[1] =
        ArchitectureOverviewKindSummary::new(SourceLanguage::Rust, RustSymbolKind::Function, 1);
    let port = FakePort::with(Ok(invalid_kinds));
    assert!(matches!(
        architecture_overview(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                ArchitectureOverviewLimits::default()
            )
        ),
        Err(ArchitectureOverviewError::InvalidPortOutput(
            ArchitectureOverviewPortOutputError::InvalidKindSummaries
        ))
    ));

    let mut impossible_roots = complete_result();
    impossible_roots.total_source_roots = impossible_roots.total_files + 1;
    let port = FakePort::with(Ok(impossible_roots));
    assert!(matches!(
        architecture_overview(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                ArchitectureOverviewLimits::default()
            )
        ),
        Err(ArchitectureOverviewError::InvalidPortOutput(
            ArchitectureOverviewPortOutputError::InvalidTotals
        ))
    ));

    let mut impossible_candidates = complete_result();
    impossible_candidates.total_entry_point_candidates =
        impossible_candidates.total_declarations + 1;
    let port = FakePort::with(Ok(impossible_candidates));
    assert!(matches!(
        architecture_overview(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                ArchitectureOverviewLimits::default()
            )
        ),
        Err(ArchitectureOverviewError::InvalidPortOutput(
            ArchitectureOverviewPortOutputError::InvalidTotals
        ))
    ));

    let mut oversized = complete_result();
    oversized.output_bytes = ArchitectureOverviewLimits::default().max_output_bytes() + 1;
    let port = FakePort::with(Ok(oversized));
    assert!(matches!(
        architecture_overview(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                ArchitectureOverviewLimits::default()
            )
        ),
        Err(ArchitectureOverviewError::InvalidPortOutput(
            ArchitectureOverviewPortOutputError::OutputByteLimitExceeded
        ))
    ));
}

#[test]
fn nested_source_root_receipt_fails_closed() {
    let mut roots = complete_source_roots();
    roots[1] = ArchitectureOverviewSourceRootSummary::new(
        ArchitectureOverviewSourceRoot::top_level_directory(path("go/nested")),
        1,
        1,
    );
    let port = FakePort::with(Ok(valid_result(
        roots,
        complete_candidates(),
        complete_files(),
        3,
        2,
    )));
    assert!(matches!(
        architecture_overview(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                ArchitectureOverviewLimits::default()
            )
        ),
        Err(ArchitectureOverviewError::InvalidPortOutput(
            ArchitectureOverviewPortOutputError::InvalidSourceRoots
        ))
    ));
}

#[test]
fn cancellation_prevents_port_access_and_post_port_cancellation_fails_closed() {
    let cancelled = Arc::new(AtomicBool::new(true));
    let port = FakePort::with(Ok(complete_result()));
    assert!(matches!(
        architecture_overview(
            &port,
            request(cancelled, ArchitectureOverviewLimits::default())
        ),
        Err(ArchitectureOverviewError::Cancelled)
    ));
    assert_eq!(port.calls.get(), 0);

    struct CancellingPort;
    impl ArchitectureOverviewPort for CancellingPort {
        type Generation = u64;
        type Error = FakeError;

        fn architecture_overview(
            &self,
            _repository: repowitness_domain::RepositoryIdentityDigest,
            _limits: ArchitectureOverviewLimits,
            cancelled: Arc<AtomicBool>,
            _deadline: Instant,
        ) -> Result<ArchitectureOverviewPortResult<Self::Generation>, Self::Error> {
            cancelled.store(true, Ordering::Release);
            Ok(complete_result())
        }
    }
    assert!(matches!(
        architecture_overview(
            &CancellingPort,
            request(
                Arc::new(AtomicBool::new(false)),
                ArchitectureOverviewLimits::default()
            )
        ),
        Err(ArchitectureOverviewError::Cancelled)
    ));

    let port = FakePort::with(Err(FakeError::Failed));
    assert!(matches!(
        architecture_overview(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                ArchitectureOverviewLimits::default()
            )
        ),
        Err(ArchitectureOverviewError::Port(FakeError::Failed))
    ));
}
