use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use repowitness_domain::{
    AnalysisArtifactDigest, ProducerManifestDigest, RepositoryPath, RepositoryPathLimits,
    SourceContentDigest, SourceSnapshotDigest,
};

use super::{
    ARCHITECTURE_MAP_PROFILE_VERSION, ArchitectureMapError, ArchitectureMapFile,
    ArchitectureMapLanguageSummary, ArchitectureMapLimits, ArchitectureMapPort,
    ArchitectureMapPortOutputError, ArchitectureMapPortResult, ArchitectureMapRequest,
    DEFAULT_ARCHITECTURE_MAP_FILES, MAX_ARCHITECTURE_MAP_FILES, architecture_map,
};
use crate::{RustIndexCoverage, SourceLanguage};

const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(128, 8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FakeError {
    Failed,
}

impl std::fmt::Display for FakeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("fake architecture-map read failed")
    }
}

impl std::error::Error for FakeError {}

struct FakePort {
    calls: Cell<u64>,
    result: Cell<Option<Result<ArchitectureMapPortResult<u64>, FakeError>>>,
}

impl FakePort {
    fn with(result: Result<ArchitectureMapPortResult<u64>, FakeError>) -> Self {
        Self {
            calls: Cell::new(0),
            result: Cell::new(Some(result)),
        }
    }
}

impl ArchitectureMapPort for FakePort {
    type Generation = u64;
    type Error = FakeError;

    fn architecture_map(
        &self,
        _repository: repowitness_domain::RepositoryIdentityDigest,
        _limits: ArchitectureMapLimits,
        _cancelled: Arc<AtomicBool>,
        _deadline: Instant,
    ) -> Result<ArchitectureMapPortResult<Self::Generation>, Self::Error> {
        self.calls.set(self.calls.get() + 1);
        self.result
            .take()
            .expect("fake port should be called at most once")
    }
}

fn file(path: &str, language: SourceLanguage, declarations: u64) -> ArchitectureMapFile {
    ArchitectureMapFile::new(
        RepositoryPath::try_from_bytes(path.as_bytes(), PATH_LIMITS)
            .expect("fixture path should be valid"),
        language,
        SourceContentDigest::new([3; 32]),
        AnalysisArtifactDigest::new([4; 32]),
        ProducerManifestDigest::new([5; 32]),
        declarations,
    )
}

fn result(
    files: Vec<ArchitectureMapFile>,
    summaries: Vec<ArchitectureMapLanguageSummary>,
    total_files: u64,
    total_declarations: u64,
) -> ArchitectureMapPortResult<u64> {
    ArchitectureMapPortResult::new(
        SourceSnapshotDigest::new([2; 32]),
        7,
        RustIndexCoverage::new(8, 2, 1, 0),
        files,
        summaries,
        total_files,
        total_declarations,
        512,
    )
}

fn request(cancelled: Arc<AtomicBool>, deadline: Instant) -> ArchitectureMapRequest {
    ArchitectureMapRequest::new(
        repowitness_domain::RepositoryIdentityDigest::new([1; 32]),
        ArchitectureMapLimits::default(),
        cancelled,
        deadline,
    )
}

#[test]
fn profile_limits_are_bounded() {
    assert_eq!(ARCHITECTURE_MAP_PROFILE_VERSION, 1);
    assert_eq!(
        ArchitectureMapLimits::default().max_files(),
        DEFAULT_ARCHITECTURE_MAP_FILES
    );
    assert!(ArchitectureMapLimits::try_new(MAX_ARCHITECTURE_MAP_FILES, 1).is_ok());
    assert!(ArchitectureMapLimits::try_new(0, 1).is_err());
    assert!(ArchitectureMapLimits::try_new(MAX_ARCHITECTURE_MAP_FILES + 1, 1).is_err());
    assert!(ArchitectureMapLimits::try_new(1, 0).is_err());
}

#[test]
fn exact_files_and_complete_language_totals_are_preserved() {
    let port = FakePort::with(Ok(result(
        vec![
            file("src/lib.rs", SourceLanguage::Rust, 3),
            file("src/main.go", SourceLanguage::Go, 2),
        ],
        vec![
            ArchitectureMapLanguageSummary::new(SourceLanguage::Go, 1, 2),
            ArchitectureMapLanguageSummary::new(SourceLanguage::Rust, 1, 3),
        ],
        2,
        5,
    )));
    let map = architecture_map(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        ),
    )
    .expect("map should be valid");

    assert_eq!(port.calls.get(), 1);
    assert_eq!(map.snapshot(), SourceSnapshotDigest::new([2; 32]));
    assert_eq!(map.generation(), &7);
    assert_eq!(map.index_coverage(), RustIndexCoverage::new(8, 2, 1, 0));
    assert_eq!(map.total_files(), 2);
    assert_eq!(map.total_declarations(), 5);
    assert!(!map.truncated());
    assert_eq!(map.files().len(), 2);
    assert_eq!(map.files()[0].path().as_bytes(), b"src/lib.rs");
    assert_eq!(map.files()[0].language(), SourceLanguage::Rust);
    assert_eq!(map.files()[1].path().as_bytes(), b"src/main.go");
    assert_eq!(map.files()[1].declaration_count(), 2);
    assert_eq!(map.language_summaries().len(), 2);
    assert_eq!(map.language_summaries()[0].language(), SourceLanguage::Go);
}

#[test]
fn invalid_order_language_totals_and_cancellation_fail_closed() {
    let unordered = FakePort::with(Ok(result(
        vec![
            file("src/z.rs", SourceLanguage::Rust, 1),
            file("src/a.go", SourceLanguage::Go, 1),
        ],
        vec![
            ArchitectureMapLanguageSummary::new(SourceLanguage::Go, 1, 1),
            ArchitectureMapLanguageSummary::new(SourceLanguage::Rust, 1, 1),
        ],
        2,
        2,
    )));
    assert!(matches!(
        architecture_map(
            &unordered,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1)
            )
        ),
        Err(ArchitectureMapError::InvalidPortOutput(
            ArchitectureMapPortOutputError::InvalidFileOrder
        ))
    ));

    let invalid_totals = FakePort::with(Ok(result(
        vec![file("src/a.rs", SourceLanguage::Rust, 1)],
        vec![ArchitectureMapLanguageSummary::new(
            SourceLanguage::Rust,
            1,
            2,
        )],
        1,
        1,
    )));
    assert!(matches!(
        architecture_map(
            &invalid_totals,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1)
            )
        ),
        Err(ArchitectureMapError::InvalidPortOutput(
            ArchitectureMapPortOutputError::InvalidLanguageSummaries
        ))
    ));

    let cancelled = Arc::new(AtomicBool::new(true));
    let cancelled_port = FakePort::with(Err(FakeError::Failed));
    assert!(matches!(
        architecture_map(
            &cancelled_port,
            request(cancelled, Instant::now() + Duration::from_secs(1))
        ),
        Err(ArchitectureMapError::Cancelled)
    ));
    assert_eq!(cancelled_port.calls.get(), 0);

    struct CancellingPort;
    impl ArchitectureMapPort for CancellingPort {
        type Generation = u64;
        type Error = FakeError;

        fn architecture_map(
            &self,
            _repository: repowitness_domain::RepositoryIdentityDigest,
            _limits: ArchitectureMapLimits,
            cancelled: Arc<AtomicBool>,
            _deadline: Instant,
        ) -> Result<ArchitectureMapPortResult<Self::Generation>, Self::Error> {
            cancelled.store(true, Ordering::Release);
            Ok(result(Vec::new(), Vec::new(), 0, 0))
        }
    }
    assert!(matches!(
        architecture_map(
            &CancellingPort,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1)
            )
        ),
        Err(ArchitectureMapError::Cancelled)
    ));
}
