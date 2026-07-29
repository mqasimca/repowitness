#[test]
fn empty_duplicate_unknown_and_ineligible_inputs_leave_no_workspace_view() {
    let directory = TempDirectory::new();
    let fixture = TwoRepositoryViewFixture::new(&directory);
    let unknown_connected = ConnectedWorkspaceId::new([0xD0; 32]);
    let duplicate_slot = SourceSlotId::new([0xD1; 32]);

    assert_eq!(
        fixture.store.connect_workspace(
            unknown_connected,
            Vec::new(),
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::InvalidWorkspaceMembership)
    );
    assert_eq!(
        fixture.store.connect_workspace(
            unknown_connected,
            vec![
                WorkspaceSourceSlot::new(duplicate_slot, fixture.first_repository),
                WorkspaceSourceSlot::new(duplicate_slot, fixture.second_repository),
            ],
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::InvalidWorkspaceMembership)
    );
    assert_eq!(
        fixture.store.connect_workspace(
            unknown_connected,
            vec![WorkspaceSourceSlot::new(
                SourceSlotId::new([0xD4; 32]),
                RepositoryIdentityDigest::new([0xD5; 32]),
            )],
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::WorkspaceUnavailable)
    );
    assert_eq!(
        fixture.store.publish_workspace_view(
            unknown_connected,
            vec![WorkspaceViewMember::new(
                duplicate_slot,
                fixture.first_generation,
            )],
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::ConnectedWorkspaceUnavailable)
    );
    assert_eq!(
        fixture.store.publish_workspace_view(
            fixture.connected,
            Vec::new(),
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::InvalidWorkspaceView)
    );
    assert_eq!(
        fixture.store.publish_workspace_view(
            fixture.connected,
            vec![
                WorkspaceViewMember::new(fixture.first_slot, fixture.first_generation),
                WorkspaceViewMember::new(fixture.first_slot, fixture.first_generation),
            ],
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::InvalidWorkspaceView)
    );
    assert_eq!(
        fixture.store.publish_workspace_view(
            fixture.connected,
            fixture.members(
                GenerationId::from_database(i64::MAX),
                fixture.second_generation,
            ),
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::InvalidWorkspaceView)
    );
    assert_eq!(
        fixture
            .store
            .active_workspace_view(fixture.connected, workspace_control(), deadline(),),
        Ok(None)
    );
}

#[test]
fn terminal_generation_states_cannot_enter_a_workspace_view() {
    let directory = TempDirectory::new();
    let fixture = TwoRepositoryViewFixture::new(&directory);
    let failed_generation =
        stage_workspace_generation(&fixture.store, fixture.first_repository, 0xD2, "failed");
    let cancelled_generation =
        stage_workspace_generation(&fixture.store, fixture.first_repository, 0xD3, "cancelled");
    {
        let connection =
            Connection::open(directory.database()).expect("database should open for state fixture");
        for (generation, state) in [
            (failed_generation, "failed"),
            (cancelled_generation, "cancelled"),
        ] {
            connection
                .execute(
                    "UPDATE index_generations SET lifecycle_state = ?1
                     WHERE generation_id = ?2",
                    params![state, generation.get()],
                )
                .expect("ready fixture generation should enter terminal state");
        }
    }
    for ineligible in [failed_generation, cancelled_generation] {
        assert_eq!(
            fixture.store.publish_workspace_view(
                fixture.connected,
                fixture.members(ineligible, fixture.second_generation),
                workspace_control(),
                deadline(),
            ),
            Err(SqliteStoreError::InvalidWorkspaceView)
        );
    }
    assert_eq!(
        fixture
            .store
            .active_workspace_view(fixture.connected, workspace_control(), deadline(),),
        Ok(None)
    );
}

#[test]
fn workspace_operations_observe_cancellation_and_deadlines_before_writes() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let repository = RepositoryIdentityDigest::new([0xE0; 32]);
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");
    let generation = stage_workspace_generation(&store, repository, 0xE1, "control");
    store
        .activate(generation, 0, deadline())
        .expect("generation should activate");
    let connected = ConnectedWorkspaceId::new([0xE2; 32]);
    let source_slot = SourceSlotId::new([0xE3; 32]);
    let mapping = vec![WorkspaceSourceSlot::new(source_slot, repository)];
    let member = vec![WorkspaceViewMember::new(source_slot, generation)];

    let cancelled = Arc::new(AtomicBool::new(true));
    assert_eq!(
        store.connect_workspace(
            connected,
            mapping.clone(),
            Arc::clone(&cancelled),
            deadline(),
        ),
        Err(SqliteStoreError::Cancelled)
    );
    assert_eq!(
        store.connect_workspace(
            connected,
            mapping.clone(),
            workspace_control(),
            Instant::now(),
        ),
        Err(SqliteStoreError::DeadlineExceeded)
    );
    store
        .connect_workspace(connected, mapping, workspace_control(), deadline())
        .expect("workspace should register under live control");
    assert_eq!(
        store.publish_workspace_view(
            connected,
            member.clone(),
            Arc::new(AtomicBool::new(true)),
            deadline(),
        ),
        Err(SqliteStoreError::Cancelled)
    );
    assert_eq!(
        store.publish_workspace_view(connected, member, workspace_control(), Instant::now(),),
        Err(SqliteStoreError::DeadlineExceeded)
    );
    assert_eq!(
        store.active_workspace_view(connected, Arc::new(AtomicBool::new(true)), deadline(),),
        Err(SqliteStoreError::Cancelled)
    );
    assert_eq!(
        store.active_workspace_view(connected, workspace_control(), Instant::now()),
        Err(SqliteStoreError::DeadlineExceeded)
    );
    assert_eq!(
        store.active_workspace_view(connected, workspace_control(), deadline()),
        Ok(None)
    );
}

#[test]
fn workspace_errors_and_debug_output_do_not_expose_hostile_identity_bytes() {
    let connected = ConnectedWorkspaceId::new([0xFE; 32]);
    let source_slot = SourceSlotId::new([0xFD; 32]);
    let repository = RepositoryIdentityDigest::new([0xFC; 32]);
    let mapping = WorkspaceSourceSlot::new(source_slot, repository);
    let member = WorkspaceViewMember::new(source_slot, GenerationId::from_database(1));

    for debug in [
        format!("{connected:?}"),
        format!("{source_slot:?}"),
        format!("{mapping:?}"),
        format!("{member:?}"),
    ] {
        assert!(!debug.contains("FE"));
        assert!(!debug.contains("FD"));
        assert!(!debug.contains("FC"));
        assert!(!debug.contains('/'));
        assert!(!debug.contains('\\'));
    }
    for error in [
        SqliteStoreError::InvalidWorkspaceMembership,
        SqliteStoreError::WorkspaceSourceSlotLimitExceeded,
        SqliteStoreError::ConnectedWorkspaceUnavailable,
        SqliteStoreError::InvalidWorkspaceView,
        SqliteStoreError::Cancelled,
        SqliteStoreError::DeadlineExceeded,
    ] {
        let display = error.to_string();
        let debug = format!("{error:?}");
        for forbidden in ["FE", "FD", "FC", "cwi1:", "ssi1:", "/", "\\"] {
            assert!(!display.contains(forbidden));
            assert!(!debug.contains(forbidden));
        }
    }
}
