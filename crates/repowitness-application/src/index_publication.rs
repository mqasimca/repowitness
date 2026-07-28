use std::{
    fmt,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

use crate::{PreparedRustIndex, RustSourceSnapshotIdentity};

/// Exact independent coverage counts persisted before generation activation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RustIndexCoverage {
    searched: u64,
    skipped: u64,
    unresolved: u64,
    truncated: u64,
}

impl RustIndexCoverage {
    /// Constructs independent coverage-category counts.
    #[must_use]
    pub const fn new(searched: u64, skipped: u64, unresolved: u64, truncated: u64) -> Self {
        Self {
            searched,
            skipped,
            unresolved,
            truncated,
        }
    }

    /// Returns the exact searched-item count.
    #[must_use]
    pub const fn searched(self) -> u64 {
        self.searched
    }

    /// Returns the exact skipped-item count.
    #[must_use]
    pub const fn skipped(self) -> u64 {
        self.skipped
    }

    /// Returns the exact unresolved-item count.
    #[must_use]
    pub const fn unresolved(self) -> u64 {
        self.unresolved
    }

    /// Returns the exact truncated-item count.
    #[must_use]
    pub const fn truncated(self) -> u64 {
        self.truncated
    }
}

/// Complete input to the shared stage-then-activate publication use case.
pub struct PublishRustIndexRequest {
    source_epoch: u64,
    identity: RustSourceSnapshotIdentity,
    prepared: PreparedRustIndex,
    coverage: RustIndexCoverage,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl PublishRustIndexRequest {
    /// Constructs one publication request from already validated preparation.
    #[must_use]
    pub const fn new(
        source_epoch: u64,
        identity: RustSourceSnapshotIdentity,
        prepared: PreparedRustIndex,
        coverage: RustIndexCoverage,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            source_epoch,
            identity,
            prepared,
            coverage,
            cancelled,
            deadline,
        }
    }
}

impl fmt::Debug for PublishRustIndexRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PublishRustIndexRequest")
            .field("source_epoch", &self.source_epoch)
            .field("identity", &self.identity)
            .field("prepared", &self.prepared)
            .field("coverage", &self.coverage)
            .field(
                "cancelled",
                &self.cancelled.load(std::sync::atomic::Ordering::Acquire),
            )
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Narrow persistence boundary required by the shared publication use case.
pub trait RustIndexPublicationPort {
    /// Opaque immutable generation identity owned by the adapter.
    type Generation: Copy + Eq;
    /// Stable adapter failure mapped at its own boundary.
    type Error;

    /// Stages and validates one complete candidate without changing active state.
    fn stage(
        &self,
        source_epoch: u64,
        identity: RustSourceSnapshotIdentity,
        prepared: PreparedRustIndex,
        coverage: RustIndexCoverage,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Self::Generation, Self::Error>;

    /// Atomically activates one ready generation at the expected source epoch.
    fn activate(
        &self,
        generation: Self::Generation,
        expected_source_epoch: u64,
        deadline: Instant,
    ) -> Result<(), Self::Error>;
}

/// Successful publication of one immutable generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublishedRustIndex<Generation> {
    generation: Generation,
    source_epoch: u64,
}

impl<Generation: Copy> PublishedRustIndex<Generation> {
    /// Returns the newly active generation.
    #[must_use]
    pub const fn generation(self) -> Generation {
        self.generation
    }

    /// Returns the source epoch compared during activation.
    #[must_use]
    pub const fn source_epoch(self) -> u64 {
        self.source_epoch
    }
}

/// Failure phase from stage-then-activate publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublishRustIndexError<PortError> {
    /// Candidate staging or validation failed; active state was not requested.
    Stage(PortError),
    /// Atomic activation failed after a complete ready candidate was created.
    Activate(PortError),
}

impl<PortError> fmt::Display for PublishRustIndexError<PortError> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Stage(_) => "source index staging failed",
            Self::Activate(_) => "source index activation failed",
        })
    }
}

/// Stages, validates, and atomically activates one prepared source generation.
pub fn publish_rust_index<Port>(
    port: &Port,
    request: PublishRustIndexRequest,
) -> Result<PublishedRustIndex<Port::Generation>, PublishRustIndexError<Port::Error>>
where
    Port: RustIndexPublicationPort,
{
    let source_epoch = request.source_epoch;
    let generation = port
        .stage(
            source_epoch,
            request.identity,
            request.prepared,
            request.coverage,
            request.cancelled,
            request.deadline,
        )
        .map_err(PublishRustIndexError::Stage)?;
    port.activate(generation, source_epoch, request.deadline)
        .map_err(PublishRustIndexError::Activate)?;
    Ok(PublishedRustIndex {
        generation,
        source_epoch,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        sync::{Arc, atomic::AtomicBool},
        time::{Duration, Instant},
    };

    use repowitness_domain::{
        AnalysisSchemaDigest, ConfigurationDigest, GitStateDigest, ProducerManifestDigest,
        RepositoryIdentityDigest, WorktreeStateDigest,
    };

    use crate::{
        ImmutableRustSource, RustArtifactIdentity, RustIndexLimits, RustSourceSnapshotIdentity,
        prepare_rust_index,
    };

    use super::{
        PublishRustIndexError, PublishRustIndexRequest, RustIndexCoverage,
        RustIndexPublicationPort, publish_rust_index,
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum FakeError {
        Stage,
        Activate,
    }

    struct FakePort {
        calls: RefCell<Vec<&'static str>>,
        stage_error: Cell<bool>,
        activate_error: Cell<bool>,
    }

    impl RustIndexPublicationPort for FakePort {
        type Generation = u64;
        type Error = FakeError;

        fn stage(
            &self,
            _source_epoch: u64,
            _identity: RustSourceSnapshotIdentity,
            _prepared: crate::PreparedRustIndex,
            _coverage: RustIndexCoverage,
            _cancelled: Arc<AtomicBool>,
            _deadline: Instant,
        ) -> Result<Self::Generation, Self::Error> {
            self.calls.borrow_mut().push("stage");
            if self.stage_error.get() {
                Err(FakeError::Stage)
            } else {
                Ok(41)
            }
        }

        fn activate(
            &self,
            generation: Self::Generation,
            expected_source_epoch: u64,
            _deadline: Instant,
        ) -> Result<(), Self::Error> {
            assert_eq!(generation, 41);
            assert_eq!(expected_source_epoch, 7);
            self.calls.borrow_mut().push("activate");
            if self.activate_error.get() {
                Err(FakeError::Activate)
            } else {
                Ok(())
            }
        }
    }

    fn identity() -> RustSourceSnapshotIdentity {
        RustSourceSnapshotIdentity::new(
            RepositoryIdentityDigest::new([1; 32]),
            GitStateDigest::new([2; 32]),
            WorktreeStateDigest::new([3; 32]),
            ConfigurationDigest::new([4; 32]),
            ProducerManifestDigest::new([5; 32]),
            AnalysisSchemaDigest::new([6; 32]),
            1,
        )
    }

    fn request() -> PublishRustIndexRequest {
        let identity = identity();
        let artifact = RustArtifactIdentity::new(
            identity.producer_manifest(),
            identity.configuration(),
            identity.analysis_schema(),
            identity.canonicalization_version(),
        );
        let cancelled = Arc::new(AtomicBool::new(false));
        let prepared = prepare_rust_index(
            vec![ImmutableRustSource::new(
                repowitness_domain::RepositoryPath::try_from_bytes(
                    b"src/lib.rs",
                    repowitness_domain::RepositoryPathLimits::new(4096, 256),
                )
                .expect("fixture path should be valid"),
                b"pub fn indexed() {}\n".to_vec().into_boxed_slice(),
            )],
            artifact,
            RustIndexLimits::default(),
            &cancelled,
            Instant::now() + Duration::from_secs(5),
        )
        .expect("fixture index should prepare");
        PublishRustIndexRequest::new(
            7,
            identity,
            prepared,
            RustIndexCoverage::new(1, 0, 0, 0),
            cancelled,
            Instant::now() + Duration::from_secs(5),
        )
    }

    fn port() -> FakePort {
        FakePort {
            calls: RefCell::new(Vec::new()),
            stage_error: Cell::new(false),
            activate_error: Cell::new(false),
        }
    }

    #[test]
    fn publication_stages_before_activation_and_preserves_epoch() {
        let port = port();
        let published = publish_rust_index(&port, request()).expect("publication should succeed");

        assert_eq!(port.calls.borrow().as_slice(), ["stage", "activate"]);
        assert_eq!(published.generation(), 41);
        assert_eq!(published.source_epoch(), 7);
    }

    #[test]
    fn staging_failure_never_requests_activation() {
        let port = port();
        port.stage_error.set(true);

        assert_eq!(
            publish_rust_index(&port, request()),
            Err(PublishRustIndexError::Stage(FakeError::Stage))
        );
        assert_eq!(port.calls.borrow().as_slice(), ["stage"]);
    }

    #[test]
    fn activation_failure_remains_distinct() {
        let port = port();
        port.activate_error.set(true);

        assert_eq!(
            publish_rust_index(&port, request()),
            Err(PublishRustIndexError::Activate(FakeError::Activate))
        );
        assert_eq!(port.calls.borrow().as_slice(), ["stage", "activate"]);
    }
}
