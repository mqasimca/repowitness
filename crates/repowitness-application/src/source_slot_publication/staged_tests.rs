use std::{
    cell::{Cell, RefCell},
    sync::{Arc, atomic::AtomicBool, atomic::Ordering},
    time::{Duration, Instant},
};

use repowitness_domain::{
    AnalysisSchemaDigest, ConfigurationDigest, ConnectedWorkspaceId, GitStateDigest,
    ProducerManifestDigest, RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits,
    SourceSlotId, SourceSnapshotDigest, WorktreeStateDigest,
};

use crate::{
    ImmutableRustSource, PreparedRustIndex, RustArtifactIdentity, RustIndexCoverage,
    RustIndexLimits, RustSourceSnapshotIdentity, hash_source_snapshot, prepare_rust_index,
};

use super::{
    CompleteStagedSourceSlotIndexError, PublishSourceSlotIndexError, PublishSourceSlotIndexRequest,
    SourceSlotEpoch, SourceSlotFinalFence, SourceSlotPublicationPort, StageSourceSlotIndexRequest,
    complete_staged_source_slot_index, publish_source_slot_index, stage_source_slot_index,
};

const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(128, 16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TestError {
    Stage,
    Fence,
    Complete,
    Cancelled,
    Deadline,
}

struct FakePort<'a> {
    calls: &'a RefCell<Vec<&'static str>>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    fail_stage: Cell<Option<TestError>>,
    fail_complete: Cell<Option<TestError>>,
}

impl SourceSlotPublicationPort for FakePort<'_> {
    type Error = TestError;
    type Generation = u64;

    fn stage_source_slot(
        &self,
        request: StageSourceSlotIndexRequest,
    ) -> Result<Self::Generation, Self::Error> {
        self.calls.borrow_mut().push("stage");
        assert_eq!(request.connected_workspace(), connected_workspace());
        assert_eq!(request.source_slot(), source_slot());
        assert_eq!(request.reserved_epoch(), source_epoch());
        assert!(Arc::ptr_eq(&request.cancelled(), &self.cancelled));
        assert_eq!(request.deadline(), self.deadline);
        if let Some(error) = self.fail_stage.get() {
            Err(error)
        } else {
            Ok(7)
        }
    }

    fn complete_source_slot(
        &self,
        workspace: ConnectedWorkspaceId,
        slot: SourceSlotId,
        reserved_epoch: SourceSlotEpoch,
        generation: Self::Generation,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<(), Self::Error> {
        self.calls.borrow_mut().push("complete");
        assert_eq!(workspace, connected_workspace());
        assert_eq!(slot, source_slot());
        assert_eq!(reserved_epoch, source_epoch());
        assert_eq!(generation, 7);
        assert!(Arc::ptr_eq(&cancelled, &self.cancelled));
        assert_eq!(deadline, self.deadline);
        if let Some(error) = self.fail_complete.get() {
            Err(error)
        } else {
            Ok(())
        }
    }
}

struct FakeFence<'a> {
    calls: &'a RefCell<Vec<&'static str>>,
    expected_snapshot: SourceSnapshotDigest,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    fail: Cell<Option<TestError>>,
}

impl SourceSlotFinalFence for FakeFence<'_> {
    type Error = TestError;

    fn confirm_source_snapshot(
        &self,
        expected: SourceSnapshotDigest,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<(), Self::Error> {
        self.calls.borrow_mut().push("fence");
        assert_eq!(expected, self.expected_snapshot);
        assert!(Arc::ptr_eq(&cancelled, &self.cancelled));
        assert_eq!(deadline, self.deadline);
        if let Some(error) = self.fail.get() {
            return Err(error);
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(TestError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(TestError::Deadline);
        }
        Ok(())
    }
}

fn connected_workspace() -> ConnectedWorkspaceId {
    ConnectedWorkspaceId::new([8; 32])
}

fn source_slot() -> SourceSlotId {
    SourceSlotId::new([9; 32])
}

fn source_epoch() -> SourceSlotEpoch {
    SourceSlotEpoch::try_new(1).expect("source epoch should validate")
}

fn candidate() -> (
    RustSourceSnapshotIdentity,
    PreparedRustIndex,
    SourceSnapshotDigest,
) {
    let identity = RustSourceSnapshotIdentity::new(
        RepositoryIdentityDigest::new([1; 32]),
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
    let expected = hash_source_snapshot(identity, prepared.manifest_digest());
    (identity, prepared, expected)
}

fn request(
    identity: RustSourceSnapshotIdentity,
    prepared: PreparedRustIndex,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> PublishSourceSlotIndexRequest {
    PublishSourceSlotIndexRequest::new(
        connected_workspace(),
        source_slot(),
        source_epoch(),
        identity,
        prepared,
        RustIndexCoverage::new(1, 0, 0, 0),
        cancelled,
        deadline,
    )
}

fn fake_port<'a>(
    calls: &'a RefCell<Vec<&'static str>>,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> FakePort<'a> {
    FakePort {
        calls,
        cancelled: Arc::clone(cancelled),
        deadline,
        fail_stage: Cell::new(None),
        fail_complete: Cell::new(None),
    }
}

fn fake_fence<'a>(
    calls: &'a RefCell<Vec<&'static str>>,
    expected_snapshot: SourceSnapshotDigest,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> FakeFence<'a> {
    FakeFence {
        calls,
        expected_snapshot,
        cancelled: Arc::clone(cancelled),
        deadline,
        fail: Cell::new(None),
    }
}

#[test]
fn explicit_stage_allows_graph_work_before_fence_and_completion() {
    let (identity, prepared, expected_snapshot) = candidate();
    let calls = RefCell::new(Vec::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let deadline = Instant::now() + Duration::from_secs(2);
    let port = fake_port(&calls, &cancelled, deadline);
    let fence = fake_fence(&calls, expected_snapshot, &cancelled, deadline);

    let staged = stage_source_slot_index(
        &port,
        request(identity, prepared, Arc::clone(&cancelled), deadline),
    )
    .expect("source staging should succeed");
    assert_eq!(staged.generation(), 7);
    calls.borrow_mut().push("graph");
    let completed = complete_staged_source_slot_index(&port, &fence, staged)
        .expect("fenced completion should succeed");

    assert_eq!(completed.generation(), 7);
    assert_eq!(completed.source_epoch(), source_epoch());
    assert_eq!(*calls.borrow(), ["stage", "graph", "fence", "complete"]);
}

#[test]
fn discarding_after_graph_failure_never_fences_or_completes() {
    let (identity, prepared, _) = candidate();
    let calls = RefCell::new(Vec::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let deadline = Instant::now() + Duration::from_secs(2);
    let port = fake_port(&calls, &cancelled, deadline);

    let staged = stage_source_slot_index(
        &port,
        request(identity, prepared, Arc::clone(&cancelled), deadline),
    )
    .expect("source staging should succeed");
    calls.borrow_mut().push("graph_failed");
    drop(staged);

    assert_eq!(*calls.borrow(), ["stage", "graph_failed"]);
}

#[test]
fn explicit_stage_fence_and_completion_failures_stop_later_work() {
    let (identity, prepared, _) = candidate();
    let calls = RefCell::new(Vec::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let deadline = Instant::now() + Duration::from_secs(2);
    let port = fake_port(&calls, &cancelled, deadline);
    port.fail_stage.set(Some(TestError::Stage));
    assert!(matches!(
        stage_source_slot_index(
            &port,
            request(identity, prepared, Arc::clone(&cancelled), deadline)
        ),
        Err(TestError::Stage)
    ));
    assert_eq!(*calls.borrow(), ["stage"]);

    let (identity, prepared, expected_snapshot) = candidate();
    calls.borrow_mut().clear();
    port.fail_stage.set(None);
    let fence = fake_fence(&calls, expected_snapshot, &cancelled, deadline);
    fence.fail.set(Some(TestError::Fence));
    let staged = stage_source_slot_index(
        &port,
        request(identity, prepared, Arc::clone(&cancelled), deadline),
    )
    .expect("source staging should succeed");
    assert!(matches!(
        complete_staged_source_slot_index(&port, &fence, staged),
        Err(CompleteStagedSourceSlotIndexError::FinalFence(
            TestError::Fence
        ))
    ));
    assert_eq!(*calls.borrow(), ["stage", "fence"]);

    let (identity, prepared, expected_snapshot) = candidate();
    calls.borrow_mut().clear();
    port.fail_complete.set(Some(TestError::Complete));
    let fence = fake_fence(&calls, expected_snapshot, &cancelled, deadline);
    let staged = stage_source_slot_index(
        &port,
        request(identity, prepared, Arc::clone(&cancelled), deadline),
    )
    .expect("source staging should succeed");
    assert!(matches!(
        complete_staged_source_slot_index(&port, &fence, staged),
        Err(CompleteStagedSourceSlotIndexError::Complete(
            TestError::Complete
        ))
    ));
    assert_eq!(*calls.borrow(), ["stage", "fence", "complete"]);
}

#[test]
fn composition_preserves_stage_fence_and_completion_error_phases() {
    let calls = RefCell::new(Vec::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let deadline = Instant::now() + Duration::from_secs(2);
    let port = fake_port(&calls, &cancelled, deadline);

    let (identity, prepared, expected_snapshot) = candidate();
    port.fail_stage.set(Some(TestError::Stage));
    let fence = fake_fence(&calls, expected_snapshot, &cancelled, deadline);
    assert!(matches!(
        publish_source_slot_index(
            &port,
            &fence,
            request(identity, prepared, Arc::clone(&cancelled), deadline)
        ),
        Err(PublishSourceSlotIndexError::Stage(TestError::Stage))
    ));

    let (identity, prepared, expected_snapshot) = candidate();
    calls.borrow_mut().clear();
    port.fail_stage.set(None);
    let fence = fake_fence(&calls, expected_snapshot, &cancelled, deadline);
    fence.fail.set(Some(TestError::Fence));
    assert!(matches!(
        publish_source_slot_index(
            &port,
            &fence,
            request(identity, prepared, Arc::clone(&cancelled), deadline)
        ),
        Err(PublishSourceSlotIndexError::FinalFence(TestError::Fence))
    ));

    let (identity, prepared, expected_snapshot) = candidate();
    calls.borrow_mut().clear();
    port.fail_complete.set(Some(TestError::Complete));
    let fence = fake_fence(&calls, expected_snapshot, &cancelled, deadline);
    assert!(matches!(
        publish_source_slot_index(
            &port,
            &fence,
            request(identity, prepared, Arc::clone(&cancelled), deadline)
        ),
        Err(PublishSourceSlotIndexError::Complete(TestError::Complete))
    ));
}

#[test]
fn cancellation_and_deadline_are_owned_until_the_final_fence() {
    let (identity, prepared, expected_snapshot) = candidate();
    let calls = RefCell::new(Vec::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let deadline = Instant::now() + Duration::from_secs(2);
    let port = fake_port(&calls, &cancelled, deadline);
    let fence = fake_fence(&calls, expected_snapshot, &cancelled, deadline);
    let staged = stage_source_slot_index(
        &port,
        request(identity, prepared, Arc::clone(&cancelled), deadline),
    )
    .expect("source staging should succeed");
    cancelled.store(true, Ordering::Release);
    assert!(matches!(
        complete_staged_source_slot_index(&port, &fence, staged),
        Err(CompleteStagedSourceSlotIndexError::FinalFence(
            TestError::Cancelled
        ))
    ));
    assert_eq!(*calls.borrow(), ["stage", "fence"]);

    let (identity, prepared, expected_snapshot) = candidate();
    calls.borrow_mut().clear();
    cancelled.store(false, Ordering::Release);
    let expired = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("monotonic fixture should subtract");
    let port = fake_port(&calls, &cancelled, expired);
    let fence = fake_fence(&calls, expected_snapshot, &cancelled, expired);
    let staged = stage_source_slot_index(
        &port,
        request(identity, prepared, Arc::clone(&cancelled), expired),
    )
    .expect("fixture port permits staging before the fence");
    assert!(matches!(
        complete_staged_source_slot_index(&port, &fence, staged),
        Err(CompleteStagedSourceSlotIndexError::FinalFence(
            TestError::Deadline
        ))
    ));
    assert_eq!(*calls.borrow(), ["stage", "fence"]);
}

#[test]
fn staged_token_debug_redacts_every_owned_identity_and_capability() {
    let (identity, prepared, _) = candidate();
    let calls = RefCell::new(Vec::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let deadline = Instant::now() + Duration::from_secs(2);
    let port = fake_port(&calls, &cancelled, deadline);
    let staged = stage_source_slot_index(
        &port,
        request(identity, prepared, Arc::clone(&cancelled), deadline),
    )
    .expect("source staging should succeed");

    assert_eq!(
        format!("{staged:?}"),
        concat!(
            "StagedSourceSlotIndex { expected_snapshot: \"<redacted-digest>\", ",
            "connected_workspace: \"<redacted-identity>\", ",
            "source_slot: \"<redacted-identity>\", ",
            "reserved_epoch: SourceSlotEpoch(1), generation: \"<opaque-generation>\", ",
            "cancelled: false, deadline: \"<monotonic>\" }"
        )
    );
}
