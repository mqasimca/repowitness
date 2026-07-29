fn workspace_snapshot_identity(
    repository: RepositoryIdentityDigest,
    salt: u8,
) -> RustSourceSnapshotIdentity {
    RustSourceSnapshotIdentity::new(
        repository,
        GitStateDigest::new([salt; 32]),
        WorktreeStateDigest::new([salt.wrapping_add(1); 32]),
        ConfigurationDigest::new([4; 32]),
        ProducerManifestDigest::new([5; 32]),
        AnalysisSchemaDigest::new([6; 32]),
        7,
    )
}

fn stage_workspace_generation(
    store: &OwnedSqliteIndex,
    repository: RepositoryIdentityDigest,
    salt: u8,
    suffix: &str,
) -> GenerationId {
    store
        .stage(
            0,
            workspace_snapshot_identity(repository, salt),
            prepared(suffix),
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("workspace generation should stage")
}

fn bounded_slot_id(prefix: u8, ordinal: usize) -> SourceSlotId {
    let mut bytes = [0; 32];
    bytes[0] = prefix;
    bytes[24..].copy_from_slice(
        &u64::try_from(ordinal)
            .expect("bounded slot ordinal should fit")
            .to_be_bytes(),
    );
    SourceSlotId::new(bytes)
}

fn workspace_control() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn complete_slot(
    store: &OwnedSqliteIndex,
    connected: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    generation: GenerationId,
) {
    complete_slot_at(
        store,
        connected,
        source_slot,
        SourceSlotEpoch::INITIAL,
        generation,
    );
}

fn complete_slot_at(
    store: &OwnedSqliteIndex,
    connected: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    source_epoch: SourceSlotEpoch,
    generation: GenerationId,
) {
    store
        .complete_source_slot_epoch(
            connected,
            source_slot,
            source_epoch,
            generation,
            workspace_control(),
            deadline(),
        )
        .expect("complete reconciliation should bind the initial slot epoch");
}

struct TwoRepositoryViewFixture {
    store: OwnedSqliteIndex,
    connected: ConnectedWorkspaceId,
    first_repository: RepositoryIdentityDigest,
    second_repository: RepositoryIdentityDigest,
    first_slot: SourceSlotId,
    second_slot: SourceSlotId,
    first_generation: GenerationId,
    second_generation: GenerationId,
}

impl TwoRepositoryViewFixture {
    fn new(directory: &TempDirectory) -> Self {
        let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
            .expect("owned store should start");
        let first_repository = RepositoryIdentityDigest::new([0x61; 32]);
        let second_repository = RepositoryIdentityDigest::new([0x62; 32]);
        for repository in [first_repository, second_repository] {
            store
                .register_workspace(repository, 0, deadline())
                .expect("repository should register");
        }
        let first_generation =
            stage_workspace_generation(&store, first_repository, 7, "rollback-first");
        let second_generation =
            stage_workspace_generation(&store, second_repository, 8, "rollback-second");
        store
            .activate(first_generation, 0, deadline())
            .expect("first generation should activate");
        store
            .activate(second_generation, 0, deadline())
            .expect("second generation should activate");
        let connected = ConnectedWorkspaceId::new([0xA2; 32]);
        let first_slot = SourceSlotId::new([0x71; 32]);
        let second_slot = SourceSlotId::new([0x72; 32]);
        store
            .connect_workspace(
                connected,
                vec![
                    WorkspaceSourceSlot::new(first_slot, first_repository),
                    WorkspaceSourceSlot::new(second_slot, second_repository),
                ],
                workspace_control(),
                deadline(),
            )
            .expect("connected workspace should register");
        complete_slot(&store, connected, first_slot, first_generation);
        complete_slot(&store, connected, second_slot, second_generation);
        Self {
            store,
            connected,
            first_repository,
            second_repository,
            first_slot,
            second_slot,
            first_generation,
            second_generation,
        }
    }

    fn members(
        &self,
        first_generation: GenerationId,
        second_generation: GenerationId,
    ) -> Vec<WorkspaceViewMember> {
        vec![
            WorkspaceViewMember::new(self.first_slot, first_generation),
            WorkspaceViewMember::new(self.second_slot, second_generation),
        ]
    }
}

#[test]
fn single_repository_api_publishes_default_workspace_view() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let repository = RepositoryIdentityDigest::new([1; 32]);
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");
    let generation = stage_workspace_generation(&store, repository, 2, "default");

    assert_eq!(
        store
            .active_workspace_view(
                ConnectedWorkspaceId::for_single_repository(repository),
                workspace_control(),
                deadline(),
            )
            .expect("default view query should succeed"),
        None
    );
    store
        .activate(generation, 0, deadline())
        .expect("generation should activate");
    let view = store
        .active_workspace_view(
            ConnectedWorkspaceId::for_single_repository(repository),
            workspace_control(),
            deadline(),
        )
        .expect("default view query should succeed")
        .expect("default active view should exist");

    assert_eq!(view.members().len(), 1);
    assert_eq!(
        view.members()[0].source_slot(),
        SourceSlotId::for_repository(repository)
    );
    assert_eq!(view.members()[0].repository(), repository);
    assert_eq!(view.members()[0].generation(), generation);
}

#[test]
fn connected_workspace_pins_two_logical_repositories_in_canonical_order() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let first_repository = RepositoryIdentityDigest::new([0x11; 32]);
    let second_repository = RepositoryIdentityDigest::new([0x22; 32]);
    for repository in [first_repository, second_repository] {
        store
            .register_workspace(repository, 0, deadline())
            .expect("repository should register");
    }
    let first_generation =
        stage_workspace_generation(&store, first_repository, 3, "first-repository");
    let second_generation =
        stage_workspace_generation(&store, second_repository, 4, "second-repository");
    store
        .activate(first_generation, 0, deadline())
        .expect("first generation should activate");
    store
        .activate(second_generation, 0, deadline())
        .expect("second generation should activate");

    let connected = ConnectedWorkspaceId::new([0xA0; 32]);
    let first_slot = SourceSlotId::new([0x31; 32]);
    let second_slot = SourceSlotId::new([0x32; 32]);
    store
        .connect_workspace(
            connected,
            vec![
                WorkspaceSourceSlot::new(second_slot, second_repository),
                WorkspaceSourceSlot::new(first_slot, first_repository),
            ],
            workspace_control(),
            deadline(),
        )
        .expect("connected workspace should register");
    complete_slot(&store, connected, first_slot, first_generation);
    complete_slot(&store, connected, second_slot, second_generation);
    let published = store
        .publish_workspace_view(
            connected,
            vec![
                WorkspaceViewMember::new(second_slot, second_generation),
                WorkspaceViewMember::new(first_slot, first_generation),
            ],
            workspace_control(),
            deadline(),
        )
        .expect("complete workspace view should publish");
    let view = store
        .active_workspace_view(connected, workspace_control(), deadline())
        .expect("workspace view should load")
        .expect("active workspace view should exist");

    assert_eq!(view.view(), published);
    assert_eq!(
        view.members()
            .iter()
            .map(|member| (
                member.ordinal(),
                member.source_slot(),
                member.repository(),
                member.generation(),
            ))
            .collect::<Vec<_>>(),
        vec![
            (0, first_slot, first_repository, first_generation),
            (1, second_slot, second_repository, second_generation),
        ]
    );
}

#[test]
fn two_slots_for_one_logical_repository_survive_reopen() {
    let directory = TempDirectory::new();
    let connected = ConnectedWorkspaceId::new([0xA1; 32]);
    let first_slot = SourceSlotId::new([0x41; 32]);
    let second_slot = SourceSlotId::new([0x42; 32]);
    let repository = RepositoryIdentityDigest::new([0x51; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");
    let first_generation = stage_workspace_generation(&store, repository, 5, "same-repo-first");
    store
        .activate(first_generation, 0, deadline())
        .expect("first generation should activate");
    let second_generation = stage_workspace_generation(&store, repository, 6, "same-repo-second");
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
        .expect("two source slots should register");
    complete_slot(&store, connected, first_slot, first_generation);
    complete_slot(&store, connected, second_slot, second_generation);
    store
        .publish_workspace_view(
            connected,
            vec![
                WorkspaceViewMember::new(first_slot, first_generation),
                WorkspaceViewMember::new(second_slot, second_generation),
            ],
            workspace_control(),
            deadline(),
        )
        .expect("active and ready generations should publish together");
    store
        .shutdown(deadline())
        .expect("owned store should stop cleanly");

    let (reopened, startup) = OwnedSqliteIndex::start(&directory.database(), 456, deadline())
        .expect("owned store should reopen");
    let view = reopened
        .active_workspace_view(connected, workspace_control(), deadline())
        .expect("workspace view should load after reopen")
        .expect("active workspace view should remain");

    assert_eq!(startup.recovered_generations(), 0);
    assert_eq!(
        view.members()
            .iter()
            .map(|member| member.generation())
            .collect::<Vec<_>>(),
        vec![first_generation, second_generation]
    );
}

fn assert_workspace_view_storage(
    database: &Path,
    connected: ConnectedWorkspaceId,
    expected_active_view: i64,
) {
    let connection = Connection::open(database).expect("database should reopen for inspection");
    let counts: (i64, i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT count(*) FROM workspace_views
                 WHERE connected_workspace_id = ?1),
                (SELECT count(*) FROM workspace_views
                 WHERE connected_workspace_id = ?1
                   AND lifecycle_state = 'staging'),
                (SELECT workspace_view_id FROM active_workspace_views
                 WHERE connected_workspace_id = ?1)",
            [connected.as_bytes().as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("workspace view rollback should be inspectable");
    assert_eq!(counts, (2, 0, expected_active_view));
}

fn publish_successor_view(fixture: &TwoRepositoryViewFixture) -> WorkspaceViewId {
    let next_first =
        stage_workspace_generation(&fixture.store, fixture.first_repository, 10, "switch-first");
    let next_second = stage_workspace_generation(
        &fixture.store,
        fixture.second_repository,
        11,
        "switch-second",
    );
    let next_first_epoch = fixture
        .store
        .reserve_source_slot_epoch(
            fixture.connected,
            fixture.first_slot,
            SourceSlotEpoch::INITIAL,
            workspace_control(),
            deadline(),
        )
        .expect("first slot should reserve its successor epoch");
    let next_second_epoch = fixture
        .store
        .reserve_source_slot_epoch(
            fixture.connected,
            fixture.second_slot,
            SourceSlotEpoch::INITIAL,
            workspace_control(),
            deadline(),
        )
        .expect("second slot should reserve its successor epoch");
    complete_slot_at(
        &fixture.store,
        fixture.connected,
        fixture.first_slot,
        next_first_epoch,
        next_first,
    );
    complete_slot_at(
        &fixture.store,
        fixture.connected,
        fixture.second_slot,
        next_second_epoch,
        next_second,
    );
    fixture
        .store
        .publish_workspace_view(
            fixture.connected,
            vec![
                WorkspaceViewMember::at_epoch(
                    fixture.first_slot,
                    next_first_epoch,
                    next_first,
                ),
                WorkspaceViewMember::at_epoch(
                    fixture.second_slot,
                    next_second_epoch,
                    next_second,
                ),
            ],
            workspace_control(),
            deadline(),
        )
        .expect("second complete view should atomically replace the first")
}

#[test]
fn incomplete_or_mismatched_view_rolls_back_without_switching() {
    let directory = TempDirectory::new();
    let fixture = TwoRepositoryViewFixture::new(&directory);

    assert_eq!(
        fixture.store.publish_workspace_view(
            fixture.connected,
            vec![WorkspaceViewMember::new(
                fixture.first_slot,
                fixture.first_generation,
            )],
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::InvalidWorkspaceView)
    );
    let first_view = fixture
        .store
        .publish_workspace_view(
            fixture.connected,
            fixture.members(fixture.first_generation, fixture.second_generation),
            workspace_control(),
            deadline(),
        )
        .expect("initial complete view should publish");
    let pinned_first = fixture
        .store
        .active_workspace_view(fixture.connected, workspace_control(), deadline())
        .expect("initial active view should load")
        .expect("initial active view should exist");
    assert_eq!(
        fixture.store.publish_workspace_view(
            fixture.connected,
            fixture.members(fixture.second_generation, fixture.first_generation),
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::InvalidWorkspaceView)
    );
    assert_eq!(
        fixture
            .store
            .active_workspace_view(fixture.connected, workspace_control(), deadline())
            .expect("active view should remain")
            .expect("initial view should remain active")
            .view(),
        first_view
    );
    let second_view = publish_successor_view(&fixture);
    assert_ne!(second_view, first_view);
    assert_eq!(pinned_first.view(), first_view);
    assert_eq!(
        pinned_first
            .members()
            .iter()
            .map(|member| member.generation())
            .collect::<Vec<_>>(),
        vec![fixture.first_generation, fixture.second_generation]
    );
    assert_eq!(
        fixture
            .store
            .active_workspace_view(fixture.connected, workspace_control(), deadline())
            .expect("new active view should load")
            .expect("new active view should exist")
            .view(),
        second_view
    );
    fixture
        .store
        .shutdown(deadline())
        .expect("owned store should stop cleanly");
    assert_workspace_view_storage(&directory.database(), fixture.connected, second_view.get());
}

#[test]
fn source_slot_bound_is_inclusive_and_enforced_before_writes() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let repository = RepositoryIdentityDigest::new([0xA5; 32]);
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");
    let generation = stage_workspace_generation(&store, repository, 12, "slot-bound");
    store
        .activate(generation, 0, deadline())
        .expect("generation should activate");

    let connected = ConnectedWorkspaceId::new([0xA6; 32]);
    let exact_slots = (0..MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS)
        .map(|ordinal| WorkspaceSourceSlot::new(bounded_slot_id(0xB0, ordinal), repository))
        .collect::<Vec<_>>();
    store
        .connect_workspace(
            connected,
            exact_slots.clone(),
            workspace_control(),
            deadline(),
        )
        .expect("the exact source-slot bound should register");
    for slot in &exact_slots {
        complete_slot(&store, connected, slot.source_slot(), generation);
    }
    let members = exact_slots
        .iter()
        .map(|slot| WorkspaceViewMember::new(slot.source_slot(), generation))
        .collect::<Vec<_>>();
    store
        .publish_workspace_view(connected, members, workspace_control(), deadline())
        .expect("the exact source-slot bound should publish");
    assert_eq!(
        store
            .active_workspace_view(connected, workspace_control(), deadline())
            .expect("bounded view should load")
            .expect("bounded view should exist")
            .members()
            .len(),
        MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS
    );

    let over_limit = (0..=MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS)
        .map(|ordinal| WorkspaceSourceSlot::new(bounded_slot_id(0xB1, ordinal), repository))
        .collect::<Vec<_>>();
    assert_eq!(
        store.connect_workspace(
            ConnectedWorkspaceId::new([0xA7; 32]),
            over_limit,
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::WorkspaceSourceSlotLimitExceeded)
    );
    assert_eq!(
        store.connect_workspace(
            ConnectedWorkspaceId::new([0xA8; 32]),
            vec![WorkspaceSourceSlot::new(
                SourceSlotId::new([0xA9; 32]),
                repository,
            )],
            workspace_control(),
            Instant::now(),
        ),
        Err(SqliteStoreError::DeadlineExceeded)
    );
}

#[test]
fn source_slot_identity_is_global_and_membership_freezes_after_publication() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let repository = RepositoryIdentityDigest::new([0x81; 32]);
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");
    let generation = stage_workspace_generation(&store, repository, 9, "global-slot");
    store
        .activate(generation, 0, deadline())
        .expect("generation should activate");
    let first_workspace = ConnectedWorkspaceId::new([0xA3; 32]);
    let second_workspace = ConnectedWorkspaceId::new([0xA4; 32]);
    let global_slot = SourceSlotId::new([0x91; 32]);
    let first_mapping = vec![WorkspaceSourceSlot::new(global_slot, repository)];
    store
        .connect_workspace(
            first_workspace,
            first_mapping.clone(),
            workspace_control(),
            deadline(),
        )
        .expect("first workspace should claim source slot");
    complete_slot(&store, first_workspace, global_slot, generation);
    store
        .publish_workspace_view(
            first_workspace,
            vec![WorkspaceViewMember::new(global_slot, generation)],
            workspace_control(),
            deadline(),
        )
        .expect("first view should publish");

    store
        .connect_workspace(
            first_workspace,
            first_mapping,
            workspace_control(),
            deadline(),
        )
        .expect("exact repeated membership should be idempotent");
    assert_eq!(
        store.connect_workspace(
            second_workspace,
            vec![WorkspaceSourceSlot::new(global_slot, repository)],
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::InvalidWorkspaceMembership)
    );
    let isolated_slot = SourceSlotId::new([0x93; 32]);
    store
        .connect_workspace(
            second_workspace,
            vec![WorkspaceSourceSlot::new(isolated_slot, repository)],
            workspace_control(),
            deadline(),
        )
        .expect("second workspace should register an independent source slot");
    complete_slot(&store, second_workspace, isolated_slot, generation);
    let second_view = store
        .publish_workspace_view(
            second_workspace,
            vec![WorkspaceViewMember::new(isolated_slot, generation)],
            workspace_control(),
            deadline(),
        )
        .expect("second workspace should publish independently");
    assert_eq!(
        store.publish_workspace_view(
            second_workspace,
            vec![WorkspaceViewMember::new(global_slot, generation)],
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::InvalidWorkspaceView)
    );
    let first_active = store
        .active_workspace_view(first_workspace, workspace_control(), deadline())
        .expect("first workspace read should succeed")
        .expect("first workspace should remain active");
    let second_active = store
        .active_workspace_view(second_workspace, workspace_control(), deadline())
        .expect("second workspace read should succeed")
        .expect("second workspace should remain active");
    assert_eq!(first_active.members()[0].source_slot(), global_slot);
    assert_eq!(second_active.view(), second_view);
    assert_eq!(second_active.members()[0].source_slot(), isolated_slot);
    assert_eq!(
        store.connect_workspace(
            first_workspace,
            vec![
                WorkspaceSourceSlot::new(global_slot, repository),
                WorkspaceSourceSlot::new(SourceSlotId::new([0x92; 32]), repository),
            ],
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::InvalidWorkspaceMembership)
    );
}
