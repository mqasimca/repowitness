use std::{
    error::Error,
    fmt,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

use repowitness_domain::{ConnectedWorkspaceId, SourceSlotId, SourceSnapshotDigest};

use crate::{
    PreparedRustIndex, RustIndexCoverage, RustSourceSnapshotIdentity, hash_source_snapshot,
};

/// Largest source-slot epoch representable by the provisional SQLite format.
pub const MAX_SOURCE_SLOT_EPOCH: u64 = i64::MAX as u64;

/// Durable fixed-width monotonic epoch owned by one source slot.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SourceSlotEpoch(u64);

impl SourceSlotEpoch {
    /// Initial epoch for a source slot with no reserved successor.
    pub const INITIAL: Self = Self(0);

    /// Validates one epoch against the provisional persistence ceiling.
    pub const fn try_new(value: u64) -> Result<Self, SourceSlotEpochError> {
        if value > MAX_SOURCE_SLOT_EPOCH {
            Err(SourceSlotEpochError::NotRepresentable)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the fixed-width persisted value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the exact next epoch or fails closed at exhaustion.
    pub const fn checked_next(self) -> Result<Self, SourceSlotEpochError> {
        if self.0 == MAX_SOURCE_SLOT_EPOCH {
            Err(SourceSlotEpochError::Exhausted)
        } else {
            Ok(Self(self.0 + 1))
        }
    }
}

/// Failure to construct or advance a durable source-slot epoch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceSlotEpochError {
    /// The input exceeds the provisional persistence representation.
    NotRepresentable,
    /// The monotonic counter has no representable successor.
    Exhausted,
}

impl fmt::Display for SourceSlotEpochError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotRepresentable => "source-slot epoch is not representable",
            Self::Exhausted => "source-slot epoch is exhausted",
        })
    }
}

impl Error for SourceSlotEpochError {}

/// Complete candidate admitted after one authoritative source reconciliation.
pub struct PublishSourceSlotIndexRequest {
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    reserved_epoch: SourceSlotEpoch,
    identity: RustSourceSnapshotIdentity,
    prepared: PreparedRustIndex,
    coverage: RustIndexCoverage,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl PublishSourceSlotIndexRequest {
    /// Constructs one already-reserved, complete source candidate.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "slot identity, source identity, control, and coverage remain explicit"
    )]
    pub const fn new(
        connected_workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        reserved_epoch: SourceSlotEpoch,
        identity: RustSourceSnapshotIdentity,
        prepared: PreparedRustIndex,
        coverage: RustIndexCoverage,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            connected_workspace,
            source_slot,
            reserved_epoch,
            identity,
            prepared,
            coverage,
            cancelled,
            deadline,
        }
    }
}

impl fmt::Debug for PublishSourceSlotIndexRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PublishSourceSlotIndexRequest")
            .field("connected_workspace", &"<redacted-identity>")
            .field("source_slot", &"<redacted-identity>")
            .field("reserved_epoch", &self.reserved_epoch)
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

/// Complete owned input passed to a source-slot staging adapter.
pub struct StageSourceSlotIndexRequest {
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    reserved_epoch: SourceSlotEpoch,
    identity: RustSourceSnapshotIdentity,
    prepared: PreparedRustIndex,
    coverage: RustIndexCoverage,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl StageSourceSlotIndexRequest {
    /// Returns the connected workspace that owns the source slot.
    #[must_use]
    pub const fn connected_workspace(&self) -> ConnectedWorkspaceId {
        self.connected_workspace
    }

    /// Returns the stable source-slot identity.
    #[must_use]
    pub const fn source_slot(&self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the already-reserved durable source-slot epoch.
    #[must_use]
    pub const fn reserved_epoch(&self) -> SourceSlotEpoch {
        self.reserved_epoch
    }

    /// Returns the complete source snapshot identity.
    #[must_use]
    pub const fn identity(&self) -> RustSourceSnapshotIdentity {
        self.identity
    }

    /// Returns the non-sensitive categorical coverage summary.
    #[must_use]
    pub const fn coverage(&self) -> RustIndexCoverage {
        self.coverage
    }

    /// Returns the shared cancellation signal.
    #[must_use]
    pub fn cancelled(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }

    /// Returns the monotonic operation deadline.
    #[must_use]
    pub const fn deadline(&self) -> Instant {
        self.deadline
    }

    /// Consumes the request and returns the complete prepared index.
    #[must_use]
    pub fn into_prepared(self) -> PreparedRustIndex {
        self.prepared
    }
}

impl fmt::Debug for StageSourceSlotIndexRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StageSourceSlotIndexRequest")
            .field("connected_workspace", &"<redacted-identity>")
            .field("source_slot", &"<redacted-identity>")
            .field("reserved_epoch", &self.reserved_epoch)
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

/// Persistence operations required to stage and durably bind one slot candidate.
pub trait SourceSlotPublicationPort {
    /// Opaque immutable generation identity owned by the adapter.
    type Generation: Copy + Eq;
    /// Stable adapter failure mapped at its own boundary.
    type Error;

    /// Stages one complete generation under an already-reserved slot epoch.
    fn stage_source_slot(
        &self,
        request: StageSourceSlotIndexRequest,
    ) -> Result<Self::Generation, Self::Error>;

    /// Atomically records completion only while the reserved epoch is current.
    fn complete_source_slot(
        &self,
        connected_workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        reserved_epoch: SourceSlotEpoch,
        generation: Self::Generation,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<(), Self::Error>;
}

/// Authoritative full-source stability check performed after staging.
pub trait SourceSlotFinalFence {
    /// Stable adapter failure mapped at its own boundary.
    type Error;

    /// Confirms the complete current source state still has the expected digest.
    fn confirm_source_snapshot(
        &self,
        expected: SourceSnapshotDigest,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<(), Self::Error>;
}

/// Linear capability proving one source generation was staged for a slot epoch.
///
/// The token deliberately does not implement `Clone` or `Copy`. Optional
/// generation-owned work may inspect [`Self::generation`], after which the
/// token must be moved into [`complete_staged_source_slot_index`]. Consuming
/// the token makes a second completion attempt impossible through this API.
#[must_use = "a staged source-slot candidate must be completed or deliberately discarded"]
pub struct StagedSourceSlotIndex<Generation> {
    expected_snapshot: SourceSnapshotDigest,
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    reserved_epoch: SourceSlotEpoch,
    generation: Generation,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl<Generation: Copy> StagedSourceSlotIndex<Generation> {
    /// Returns the immutable generation available for generation-owned staging.
    #[must_use]
    pub const fn generation(&self) -> Generation {
        self.generation
    }
}

impl<Generation> fmt::Debug for StagedSourceSlotIndex<Generation> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StagedSourceSlotIndex")
            .field("expected_snapshot", &"<redacted-digest>")
            .field("connected_workspace", &"<redacted-identity>")
            .field("source_slot", &"<redacted-identity>")
            .field("reserved_epoch", &self.reserved_epoch)
            .field("generation", &"<opaque-generation>")
            .field(
                "cancelled",
                &self.cancelled.load(std::sync::atomic::Ordering::Acquire),
            )
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// One generation durably bound to the slot epoch that produced it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletedSourceSlotIndex<Generation> {
    generation: Generation,
    source_epoch: SourceSlotEpoch,
}

impl<Generation: Copy> CompletedSourceSlotIndex<Generation> {
    /// Returns the immutable candidate generation.
    #[must_use]
    pub const fn generation(self) -> Generation {
        self.generation
    }

    /// Returns the durable slot epoch bound to the generation.
    #[must_use]
    pub const fn source_epoch(self) -> SourceSlotEpoch {
        self.source_epoch
    }
}

/// Failure after source staging, from the final fence or immutable completion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompleteStagedSourceSlotIndexError<PortError, FenceError> {
    /// The complete source state changed or control interrupted the final fence.
    FinalFence(FenceError),
    /// The slot epoch became stale or completion persistence failed.
    Complete(PortError),
}

impl<PortError, FenceError> fmt::Display
    for CompleteStagedSourceSlotIndexError<PortError, FenceError>
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::FinalFence(_) => "source-slot final source fence failed",
            Self::Complete(_) => "source-slot completion failed",
        })
    }
}

/// Result of fencing and completing one already-staged source-slot candidate.
pub type CompleteStagedSourceSlotIndexResult<Generation, PortError, FenceError> = Result<
    CompletedSourceSlotIndex<Generation>,
    CompleteStagedSourceSlotIndexError<PortError, FenceError>,
>;

/// Failure phase from stage, final fence, and durable completion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublishSourceSlotIndexError<PortError, FenceError> {
    /// Candidate staging failed without changing the active view.
    Stage(PortError),
    /// The complete source state changed or control interrupted the final fence.
    FinalFence(FenceError),
    /// The slot epoch became stale or completion persistence failed.
    Complete(PortError),
}

impl<PortError, FenceError> fmt::Display for PublishSourceSlotIndexError<PortError, FenceError> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Stage(_) => "source-slot index staging failed",
            Self::FinalFence(_) => "source-slot final source fence failed",
            Self::Complete(_) => "source-slot completion failed",
        })
    }
}

/// Result of one strict source-slot publication attempt.
pub type PublishSourceSlotIndexResult<Generation, PortError, FenceError> = Result<
    CompletedSourceSlotIndex<Generation>,
    PublishSourceSlotIndexError<PortError, FenceError>,
>;

/// Stages source facts and returns the linear capability required for completion.
///
/// Callers may use [`StagedSourceSlotIndex::generation`] to stage immutable
/// generation-owned artifacts before moving the token into
/// [`complete_staged_source_slot_index`].
pub fn stage_source_slot_index<Port>(
    port: &Port,
    request: PublishSourceSlotIndexRequest,
) -> Result<StagedSourceSlotIndex<Port::Generation>, Port::Error>
where
    Port: SourceSlotPublicationPort,
{
    let expected_snapshot =
        hash_source_snapshot(request.identity, request.prepared.manifest_digest());
    let generation = port.stage_source_slot(StageSourceSlotIndexRequest {
        connected_workspace: request.connected_workspace,
        source_slot: request.source_slot,
        reserved_epoch: request.reserved_epoch,
        identity: request.identity,
        prepared: request.prepared,
        coverage: request.coverage,
        cancelled: Arc::clone(&request.cancelled),
        deadline: request.deadline,
    })?;
    Ok(StagedSourceSlotIndex {
        expected_snapshot,
        connected_workspace: request.connected_workspace,
        source_slot: request.source_slot,
        reserved_epoch: request.reserved_epoch,
        generation,
        cancelled: request.cancelled,
        deadline: request.deadline,
    })
}

/// Applies the authoritative final fence and immutably completes a staged slot.
///
/// The staged token is consumed on every outcome. A fence or completion
/// failure therefore requires a fresh reconciliation and cannot be retried
/// accidentally with the same capability.
pub fn complete_staged_source_slot_index<Port, Fence>(
    port: &Port,
    fence: &Fence,
    staged: StagedSourceSlotIndex<Port::Generation>,
) -> CompleteStagedSourceSlotIndexResult<Port::Generation, Port::Error, Fence::Error>
where
    Port: SourceSlotPublicationPort,
    Fence: SourceSlotFinalFence,
{
    fence
        .confirm_source_snapshot(
            staged.expected_snapshot,
            Arc::clone(&staged.cancelled),
            staged.deadline,
        )
        .map_err(CompleteStagedSourceSlotIndexError::FinalFence)?;
    port.complete_source_slot(
        staged.connected_workspace,
        staged.source_slot,
        staged.reserved_epoch,
        staged.generation,
        staged.cancelled,
        staged.deadline,
    )
    .map_err(CompleteStagedSourceSlotIndexError::Complete)?;
    Ok(CompletedSourceSlotIndex {
        generation: staged.generation,
        source_epoch: staged.reserved_epoch,
    })
}

/// Stages, revalidates, and durably binds one complete source-slot candidate.
pub fn publish_source_slot_index<Port, Fence>(
    port: &Port,
    fence: &Fence,
    request: PublishSourceSlotIndexRequest,
) -> PublishSourceSlotIndexResult<Port::Generation, Port::Error, Fence::Error>
where
    Port: SourceSlotPublicationPort,
    Fence: SourceSlotFinalFence,
{
    let staged =
        stage_source_slot_index(port, request).map_err(PublishSourceSlotIndexError::Stage)?;
    complete_staged_source_slot_index(port, fence, staged).map_err(|error| match error {
        CompleteStagedSourceSlotIndexError::FinalFence(error) => {
            PublishSourceSlotIndexError::FinalFence(error)
        }
        CompleteStagedSourceSlotIndexError::Complete(error) => {
            PublishSourceSlotIndexError::Complete(error)
        }
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
        AnalysisSchemaDigest, ConfigurationDigest, ConnectedWorkspaceId, GitStateDigest,
        ProducerManifestDigest, RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits,
        SourceSlotId, SourceSnapshotDigest, WorktreeStateDigest,
    };

    use crate::{
        ImmutableRustSource, RustArtifactIdentity, RustIndexCoverage, RustIndexLimits,
        RustSourceSnapshotIdentity, hash_source_snapshot, prepare_rust_index,
    };

    use super::{
        MAX_SOURCE_SLOT_EPOCH, PublishSourceSlotIndexRequest, SourceSlotEpoch,
        SourceSlotEpochError, SourceSlotFinalFence, SourceSlotPublicationPort,
        StageSourceSlotIndexRequest, publish_source_slot_index,
    };

    const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(128, 16);

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TestError {
        Stage,
        Fence,
        Complete,
    }

    struct FakePort {
        calls: RefCell<Vec<&'static str>>,
        fail_stage: Cell<bool>,
        fail_complete: Cell<bool>,
    }

    impl SourceSlotPublicationPort for FakePort {
        type Error = TestError;
        type Generation = u64;

        fn stage_source_slot(
            &self,
            _request: StageSourceSlotIndexRequest,
        ) -> Result<Self::Generation, Self::Error> {
            self.calls.borrow_mut().push("stage");
            if self.fail_stage.get() {
                Err(TestError::Stage)
            } else {
                Ok(7)
            }
        }

        fn complete_source_slot(
            &self,
            _connected_workspace: ConnectedWorkspaceId,
            _source_slot: SourceSlotId,
            _reserved_epoch: SourceSlotEpoch,
            _generation: Self::Generation,
            _cancelled: Arc<AtomicBool>,
            _deadline: Instant,
        ) -> Result<(), Self::Error> {
            self.calls.borrow_mut().push("complete");
            if self.fail_complete.get() {
                Err(TestError::Complete)
            } else {
                Ok(())
            }
        }
    }

    struct FakeFence<'a> {
        calls: &'a RefCell<Vec<&'static str>>,
        expected: SourceSnapshotDigest,
        fail: bool,
    }

    impl SourceSlotFinalFence for FakeFence<'_> {
        type Error = TestError;

        fn confirm_source_snapshot(
            &self,
            expected: SourceSnapshotDigest,
            _cancelled: Arc<AtomicBool>,
            _deadline: Instant,
        ) -> Result<(), Self::Error> {
            assert_eq!(expected, self.expected);
            self.calls.borrow_mut().push("fence");
            if self.fail {
                Err(TestError::Fence)
            } else {
                Ok(())
            }
        }
    }

    fn candidate() -> (
        RustSourceSnapshotIdentity,
        crate::PreparedRustIndex,
        SourceSnapshotDigest,
    ) {
        let repository = RepositoryIdentityDigest::new([1; 32]);
        let identity = RustSourceSnapshotIdentity::new(
            repository,
            GitStateDigest::new([2; 32]),
            WorktreeStateDigest::new([3; 32]),
            ConfigurationDigest::new([4; 32]),
            ProducerManifestDigest::new([5; 32]),
            AnalysisSchemaDigest::new([6; 32]),
            7,
        );
        let prepared = prepare_rust_index(
            vec![ImmutableRustSource::new(
                RepositoryPath::try_from_bytes(b"src/lib.rs", PATH_LIMITS)
                    .expect("fixture path should validate"),
                Box::from(&b"pub fn indexed() {}"[..]),
            )],
            RustArtifactIdentity::new(
                ProducerManifestDigest::new([5; 32]),
                ConfigurationDigest::new([4; 32]),
                AnalysisSchemaDigest::new([6; 32]),
                7,
            ),
            RustIndexLimits::default(),
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(1),
        )
        .expect("fixture should prepare");
        let digest = hash_source_snapshot(identity, prepared.manifest_digest());
        (identity, prepared, digest)
    }

    fn request(
        identity: RustSourceSnapshotIdentity,
        prepared: crate::PreparedRustIndex,
    ) -> PublishSourceSlotIndexRequest {
        PublishSourceSlotIndexRequest::new(
            ConnectedWorkspaceId::new([8; 32]),
            SourceSlotId::new([9; 32]),
            SourceSlotEpoch::try_new(1).expect("epoch should validate"),
            identity,
            prepared,
            RustIndexCoverage::new(1, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        )
    }

    #[test]
    fn stage_fence_and_completion_are_strictly_ordered() {
        let (identity, prepared, expected) = candidate();
        let port = FakePort {
            calls: RefCell::new(Vec::new()),
            fail_stage: Cell::new(false),
            fail_complete: Cell::new(false),
        };
        let fence = FakeFence {
            calls: &port.calls,
            expected,
            fail: false,
        };

        let completed = publish_source_slot_index(&port, &fence, request(identity, prepared))
            .expect("complete candidate should bind");

        assert_eq!(completed.generation(), 7);
        assert_eq!(completed.source_epoch().get(), 1);
        assert_eq!(*port.calls.borrow(), ["stage", "fence", "complete"]);
    }

    #[test]
    fn epoch_boundaries_are_exact_and_overflow_fails_closed() {
        assert_eq!(SourceSlotEpoch::INITIAL.get(), 0);
        let maximum =
            SourceSlotEpoch::try_new(MAX_SOURCE_SLOT_EPOCH).expect("maximum should validate");
        assert_eq!(maximum.checked_next(), Err(SourceSlotEpochError::Exhausted));
        assert_eq!(
            SourceSlotEpoch::try_new(MAX_SOURCE_SLOT_EPOCH + 1),
            Err(SourceSlotEpochError::NotRepresentable)
        );
    }

    #[test]
    fn request_debug_redacts_workspace_and_slot_identities() {
        let (identity, prepared, _) = candidate();
        let debug = format!("{:?}", request(identity, prepared));

        assert!(!debug.contains("08080808"));
        assert!(!debug.contains("09090909"));
        assert!(!debug.contains('/'));
        assert!(!debug.contains('\\'));
    }
}

#[cfg(test)]
#[path = "source_slot_publication/staged_tests.rs"]
mod staged_tests;
