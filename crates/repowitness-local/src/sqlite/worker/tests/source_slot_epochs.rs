fn connected_single_slot(
    store: &OwnedSqliteIndex,
    repository: RepositoryIdentityDigest,
    connected: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
) {
    store
        .connect_workspace(
            connected,
            vec![WorkspaceSourceSlot::new(source_slot, repository)],
            workspace_control(),
            deadline(),
        )
        .expect("source slot should connect");
}

fn publish_slot_view(
    store: &OwnedSqliteIndex,
    connected: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    source_epoch: SourceSlotEpoch,
    generation: GenerationId,
) {
    store
        .publish_workspace_view(
            connected,
            vec![WorkspaceViewMember::at_epoch(
                source_slot,
                source_epoch,
                generation,
            )],
            workspace_control(),
            deadline(),
        )
        .expect("completed slot generation should publish");
}

fn retry_reservation(
    store: &OwnedSqliteIndex,
    connected: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    expected: SourceSlotEpoch,
) -> Result<SourceSlotEpoch, SqliteStoreError> {
    loop {
        match store.reserve_source_slot_epoch(
            connected,
            source_slot,
            expected,
            workspace_control(),
            deadline(),
        ) {
            Err(SqliteStoreError::QueueFull) => thread::yield_now(),
            result => return result,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FinalFenceFailure;

struct ExactSnapshotFence {
    expected: SourceSnapshotDigest,
    fail: bool,
}

impl SourceSlotFinalFence for ExactSnapshotFence {
    type Error = FinalFenceFailure;

    fn confirm_source_snapshot(
        &self,
        expected: SourceSnapshotDigest,
        _cancelled: Arc<AtomicBool>,
        _deadline: Instant,
    ) -> Result<(), Self::Error> {
        assert_eq!(expected, self.expected);
        if self.fail {
            Err(FinalFenceFailure)
        } else {
            Ok(())
        }
    }
}

#[test]
fn strict_application_publication_requires_final_fence_before_slot_completion() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let repository = RepositoryIdentityDigest::new([0x01; 32]);
    let connected = ConnectedWorkspaceId::new([0x02; 32]);
    let source_slot = SourceSlotId::new([0x03; 32]);
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");
    let prior = stage_workspace_generation(&store, repository, 0x04, "prior-fenced");
    connected_single_slot(&store, repository, connected, source_slot);
    complete_slot(&store, connected, source_slot, prior);
    publish_slot_view(
        &store,
        connected,
        source_slot,
        SourceSlotEpoch::INITIAL,
        prior,
    );
    let reserved = store
        .reserve_source_slot_epoch(
            connected,
            source_slot,
            SourceSlotEpoch::INITIAL,
            workspace_control(),
            deadline(),
        )
        .expect("successor epoch should reserve");
    let identity = workspace_snapshot_identity(repository, 0x05);
    let failed_prepared = prepared("fence-failure");
    let expected = hash_source_snapshot(identity, failed_prepared.manifest_digest());
    let failure = publish_source_slot_index(
        &store,
        &ExactSnapshotFence {
            expected,
            fail: true,
        },
        PublishSourceSlotIndexRequest::new(
            connected,
            source_slot,
            reserved,
            identity,
            failed_prepared,
            GenerationCoverage::new(2, 0, 0, 0),
            workspace_control(),
            deadline(),
        ),
    );
    assert!(matches!(
        failure,
        Err(PublishSourceSlotIndexError::FinalFence(FinalFenceFailure))
    ));
    let failed_state = store
        .source_slot_state(connected, source_slot, workspace_control(), deadline())
        .expect("failed fenced state should load");
    assert_eq!(failed_state.current_completion(), None);
    assert_eq!(
        failed_state.active().map(|active| active.generation()),
        Some(prior)
    );

    let successful_prepared = prepared("fence-success");
    let expected = hash_source_snapshot(identity, successful_prepared.manifest_digest());
    let completed = publish_source_slot_index(
        &store,
        &ExactSnapshotFence {
            expected,
            fail: false,
        },
        PublishSourceSlotIndexRequest::new(
            connected,
            source_slot,
            reserved,
            identity,
            successful_prepared,
            GenerationCoverage::new(2, 0, 0, 0),
            workspace_control(),
            deadline(),
        ),
    )
    .expect("successful final fence should permit durable completion");
    publish_slot_view(
        &store,
        connected,
        source_slot,
        completed.source_epoch(),
        completed.generation(),
    );
    let state = store
        .source_slot_state(connected, source_slot, workspace_control(), deadline())
        .expect("completed fenced state should load");
    assert_eq!(
        state.current_completion().map(|entry| entry.generation()),
        Some(completed.generation())
    );
    assert_eq!(
        state.active().map(|entry| entry.generation()),
        Some(completed.generation())
    );
}

#[test]
fn concurrent_reservations_have_one_winner_and_stale_completion_cannot_publish() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let repository = RepositoryIdentityDigest::new([0x11; 32]);
    let connected = ConnectedWorkspaceId::new([0x12; 32]);
    let source_slot = SourceSlotId::new([0x13; 32]);
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");
    connected_single_slot(&store, repository, connected, source_slot);

    let (first, second) = thread::scope(|scope| {
        let first =
            scope.spawn(|| retry_reservation(&store, connected, source_slot, SourceSlotEpoch::INITIAL));
        let second =
            scope.spawn(|| retry_reservation(&store, connected, source_slot, SourceSlotEpoch::INITIAL));
        (
            first.join().expect("first reservation thread should join"),
            second.join().expect("second reservation thread should join"),
        )
    });
    assert!(
        matches!(
            (first, second),
            (Ok(epoch), Err(SqliteStoreError::StaleSourceEpoch))
                | (Err(SqliteStoreError::StaleSourceEpoch), Ok(epoch))
                if epoch.get() == 1
        ),
        "exactly one compare-and-set reservation must win"
    );

    let stale_generation =
        stage_workspace_generation(&store, repository, 0x14, "stale-completion");
    let current = SourceSlotEpoch::try_new(1).expect("fixture epoch should validate");
    let newer = store
        .reserve_source_slot_epoch(
            connected,
            source_slot,
            current,
            workspace_control(),
            deadline(),
        )
        .expect("newer proven source should reserve a successor");
    assert_eq!(
        store.complete_source_slot_epoch(
            connected,
            source_slot,
            current,
            stale_generation,
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::StaleSourceEpoch)
    );
    assert_eq!(
        store.publish_workspace_view(
            connected,
            vec![WorkspaceViewMember::at_epoch(
                source_slot,
                current,
                stale_generation,
            )],
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::StaleSourceEpoch)
    );
    assert_eq!(newer.get(), 2);
    assert_eq!(
        store
            .source_slot_state(connected, source_slot, workspace_control(), deadline())
            .expect("slot state should load")
            .current_epoch(),
        newer
    );
}

#[test]
fn same_generation_can_complete_independent_epochs_for_two_slots() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let repository = RepositoryIdentityDigest::new([0x21; 32]);
    let connected = ConnectedWorkspaceId::new([0x22; 32]);
    let first_slot = SourceSlotId::new([0x23; 32]);
    let second_slot = SourceSlotId::new([0x24; 32]);
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");
    let generation =
        stage_workspace_generation(&store, repository, 0x25, "shared-generation");
    store
        .connect_workspace(
            connected,
            vec![
                WorkspaceSourceSlot::new(first_slot, repository),
                WorkspaceSourceSlot::new(second_slot, repository),
            ],
            workspace_control(),
            deadline(),
        )
        .expect("two slots should connect");
    complete_slot(&store, connected, first_slot, generation);
    complete_slot(&store, connected, second_slot, generation);
    store
        .publish_workspace_view(
            connected,
            vec![
                WorkspaceViewMember::new(second_slot, generation),
                WorkspaceViewMember::new(first_slot, generation),
            ],
            workspace_control(),
            deadline(),
        )
        .expect("one generation should be independently receipted by both slots");

    let pinned = store
        .active_workspace_view(connected, workspace_control(), deadline())
        .expect("view should load")
        .expect("view should be active");
    assert_eq!(pinned.members().len(), 2);
    assert!(
        pinned
            .members()
            .iter()
            .all(|member| member.generation() == generation
                && member.source_epoch() == SourceSlotEpoch::INITIAL)
    );
}

#[test]
fn restart_recovers_uncompleted_generation_and_preserves_prior_view() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x31; 32]);
    let connected = ConnectedWorkspaceId::new([0x32; 32]);
    let source_slot = SourceSlotId::new([0x33; 32]);
    let (store, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("owned store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");
    let prior = stage_workspace_generation(&store, repository, 0x34, "prior");
    store
        .activate(prior, 0, deadline())
        .expect("prior generation should activate");
    connected_single_slot(&store, repository, connected, source_slot);
    complete_slot(&store, connected, source_slot, prior);
    publish_slot_view(
        &store,
        connected,
        source_slot,
        SourceSlotEpoch::INITIAL,
        prior,
    );

    let reserved = store
        .reserve_source_slot_epoch(
            connected,
            source_slot,
            SourceSlotEpoch::INITIAL,
            workspace_control(),
            deadline(),
        )
        .expect("changed source should reserve");
    let abandoned = store
        .stage_source_slot(
            connected,
            source_slot,
            reserved,
            workspace_snapshot_identity(repository, 0x35),
            prepared("abandoned"),
            GenerationCoverage::new(2, 0, 0, 0),
            workspace_control(),
            deadline(),
        )
        .expect("candidate should stage before interruption");
    store.shutdown(deadline()).expect("store should stop");

    let (reopened, startup) =
        OwnedSqliteIndex::start(&database, 456, deadline()).expect("store should reopen");
    assert_eq!(startup.recovered_generations(), 1);
    let state = reopened
        .source_slot_state(connected, source_slot, workspace_control(), deadline())
        .expect("durable state should reload");
    assert_eq!(state.current_epoch(), reserved);
    assert_eq!(state.current_completion(), None);
    assert_eq!(
        state.active().map(|active| active.generation()),
        Some(prior)
    );
    assert_ne!(state.active().map(|active| active.generation()), Some(abandoned));
    assert_eq!(
        reopened
            .active_workspace_view(connected, workspace_control(), deadline())
            .expect("prior view should remain readable")
            .expect("prior view should remain active")
            .members()[0]
            .generation(),
        prior
    );
}

#[test]
fn restart_preserves_current_completion_until_its_view_can_publish() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x36; 32]);
    let connected = ConnectedWorkspaceId::new([0x37; 32]);
    let source_slot = SourceSlotId::new([0x38; 32]);
    let (store, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("owned store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");
    let prior = stage_workspace_generation(&store, repository, 0x39, "prior-completed");
    store
        .activate(prior, 0, deadline())
        .expect("prior generation should activate");
    connected_single_slot(&store, repository, connected, source_slot);
    complete_slot(&store, connected, source_slot, prior);
    publish_slot_view(
        &store,
        connected,
        source_slot,
        SourceSlotEpoch::INITIAL,
        prior,
    );

    let reserved = store
        .reserve_source_slot_epoch(
            connected,
            source_slot,
            SourceSlotEpoch::INITIAL,
            workspace_control(),
            deadline(),
        )
        .expect("successor epoch should reserve");
    let completed =
        stage_workspace_generation(&store, repository, 0x3A, "completed-before-restart");
    store
        .complete_source_slot_epoch(
            connected,
            source_slot,
            reserved,
            completed,
            workspace_control(),
            deadline(),
        )
        .expect("current candidate should complete before interruption");
    store.shutdown(deadline()).expect("store should stop");

    let (reopened, startup) =
        OwnedSqliteIndex::start(&database, 456, deadline()).expect("store should reopen");
    assert_eq!(startup.recovered_generations(), 0);
    let state = reopened
        .source_slot_state(connected, source_slot, workspace_control(), deadline())
        .expect("completed state should reload");
    assert_eq!(
        state.current_completion().map(|entry| entry.generation()),
        Some(completed)
    );
    assert_eq!(
        state.active().map(|entry| entry.generation()),
        Some(prior)
    );

    publish_slot_view(&reopened, connected, source_slot, reserved, completed);
    assert_eq!(
        reopened
            .source_slot_state(connected, source_slot, workspace_control(), deadline())
            .expect("published state should load")
            .active()
            .map(|entry| entry.generation()),
        Some(completed)
    );
}

#[test]
fn restart_does_not_pin_ready_generation_through_superseded_receipt() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x3B; 32]);
    let connected = ConnectedWorkspaceId::new([0x3C; 32]);
    let source_slot = SourceSlotId::new([0x3D; 32]);
    let (store, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("owned store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");
    connected_single_slot(&store, repository, connected, source_slot);
    let superseded =
        stage_workspace_generation(&store, repository, 0x3E, "superseded-completion");
    complete_slot(&store, connected, source_slot, superseded);
    let current = store
        .reserve_source_slot_epoch(
            connected,
            source_slot,
            SourceSlotEpoch::INITIAL,
            workspace_control(),
            deadline(),
        )
        .expect("successor epoch should reserve");
    store.shutdown(deadline()).expect("store should stop");

    let (reopened, startup) =
        OwnedSqliteIndex::start(&database, 456, deadline()).expect("store should reopen");
    assert_eq!(startup.recovered_generations(), 1);
    let state = reopened
        .source_slot_state(connected, source_slot, workspace_control(), deadline())
        .expect("slot state should reload");
    assert_eq!(state.current_epoch(), current);
    assert_eq!(state.current_completion(), None);
    assert_eq!(state.active(), None);
}

#[test]
fn cancellation_deadline_and_failure_leave_epoch_and_active_view_unchanged() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let repository = RepositoryIdentityDigest::new([0x41; 32]);
    let connected = ConnectedWorkspaceId::new([0x42; 32]);
    let source_slot = SourceSlotId::new([0x43; 32]);
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");
    let prior = stage_workspace_generation(&store, repository, 0x44, "prior-control");
    store
        .activate(prior, 0, deadline())
        .expect("prior generation should activate");
    connected_single_slot(&store, repository, connected, source_slot);
    complete_slot(&store, connected, source_slot, prior);
    publish_slot_view(
        &store,
        connected,
        source_slot,
        SourceSlotEpoch::INITIAL,
        prior,
    );

    assert_eq!(
        store.reserve_source_slot_epoch(
            connected,
            source_slot,
            SourceSlotEpoch::INITIAL,
            Arc::new(AtomicBool::new(true)),
            deadline(),
        ),
        Err(SqliteStoreError::Cancelled)
    );
    assert_eq!(
        store.reserve_source_slot_epoch(
            connected,
            source_slot,
            SourceSlotEpoch::INITIAL,
            workspace_control(),
            Instant::now(),
        ),
        Err(SqliteStoreError::DeadlineExceeded)
    );
    assert_eq!(
        store.complete_source_slot_epoch(
            connected,
            source_slot,
            SourceSlotEpoch::INITIAL,
            GenerationId::from_database(i64::MAX),
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::InvalidWorkspaceView)
    );
    let state = store
        .source_slot_state(connected, source_slot, workspace_control(), deadline())
        .expect("state should remain readable");
    assert_eq!(state.current_epoch(), SourceSlotEpoch::INITIAL);
    assert_eq!(
        state.active().map(|active| active.generation()),
        Some(prior)
    );
}

#[test]
fn maximum_persisted_epoch_is_exhausted_without_mutation() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x51; 32]);
    let connected = ConnectedWorkspaceId::new([0x52; 32]);
    let source_slot = SourceSlotId::new([0x53; 32]);
    let (store, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("owned store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");
    connected_single_slot(&store, repository, connected, source_slot);
    store.shutdown(deadline()).expect("store should stop");

    let connection = Connection::open(&database).expect("fixture database should open");
    connection
        .execute_batch(
            "DROP TRIGGER workspace_source_slots_epoch_monotonic;
             UPDATE workspace_source_slots
             SET source_epoch = 9223372036854775807
             WHERE source_slot_id = X'5353535353535353535353535353535353535353535353535353535353535353';
             CREATE TRIGGER workspace_source_slots_epoch_monotonic
             BEFORE UPDATE OF source_epoch ON workspace_source_slots
             WHEN OLD.source_epoch = 9223372036854775807
               OR NEW.source_epoch != OLD.source_epoch + 1
             BEGIN
                 SELECT RAISE(ABORT, 'invalid workspace source-slot epoch transition');
             END;",
        )
        .expect("fixture should install the exact maximum epoch");
    drop(connection);

    let (reopened, _) =
        OwnedSqliteIndex::start(&database, 456, deadline()).expect("store should reopen");
    let maximum = SourceSlotEpoch::try_new(i64::MAX as u64).expect("maximum should validate");
    assert_eq!(
        reopened.reserve_source_slot_epoch(
            connected,
            source_slot,
            maximum,
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::SourceEpochExhausted)
    );
    assert_eq!(
        reopened
            .source_slot_state(connected, source_slot, workspace_control(), deadline())
            .expect("maximum state should remain readable")
            .current_epoch(),
        maximum
    );
}
