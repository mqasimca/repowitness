fn complete_workspace_source(
    store: &OwnedSqliteIndex,
    connected: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    repository: RepositoryIdentityDigest,
    source_epoch: SourceSlotEpoch,
    salt: u8,
    suffix: &str,
) -> CompletedWorkspaceSource {
    let identity = workspace_snapshot_identity(repository, salt);
    let prepared = prepared(suffix);
    let expected = hash_source_snapshot(identity, prepared.manifest_digest());
    let completed = publish_source_slot_index(
        store,
        &ExactSnapshotFence {
            expected,
            fail: false,
        },
        PublishSourceSlotIndexRequest::new(
            connected,
            source_slot,
            source_epoch,
            identity,
            prepared,
            GenerationCoverage::new(2, 0, 0, 0),
            workspace_control(),
            deadline(),
        ),
    )
    .expect("source-slot candidate should complete");
    CompletedWorkspaceSource::new(source_slot, completed)
}

fn register_and_connect_sources(
    store: &OwnedSqliteIndex,
    connected: ConnectedWorkspaceId,
    sources: &[(SourceSlotId, RepositoryIdentityDigest)],
) {
    for (_, repository) in sources {
        store
            .register_workspace(*repository, 0, deadline())
            .expect("repository should register");
    }
    store
        .connect_workspace(
            connected,
            sources
                .iter()
                .rev()
                .map(|(source_slot, repository)| {
                    WorkspaceSourceSlot::new(*source_slot, *repository)
                })
                .collect(),
            workspace_control(),
            deadline(),
        )
        .expect("workspace sources should connect");
}

fn assert_workspace_schema_has_no_host_selection(database: &Path) {
    let connection = Connection::open(database).expect("workspace database should open");
    let schema: String = connection
        .query_row(
            "SELECT group_concat(sql, ' ')
             FROM sqlite_schema
             WHERE tbl_name IN (
                'connected_workspaces', 'workspace_source_slots',
                'workspace_views', 'workspace_view_members',
                'active_workspace_views'
             )",
            [],
            |row| row.get(0),
        )
        .expect("workspace schema should be readable");
    let schema = schema.to_ascii_lowercase();
    for forbidden in [
        "root_path",
        "filesystem_path",
        "branch_name",
        "ref_name",
        "selector_text",
    ] {
        assert!(!schema.contains(forbidden));
    }
}

fn assert_one_published_workspace_view(database: &Path, connected: ConnectedWorkspaceId) {
    let connection = Connection::open(database).expect("database should reopen");
    let counts: (i64, i64) = connection
        .query_row(
            "SELECT
                count(*),
                sum(CASE WHEN lifecycle_state = 'staging' THEN 1 ELSE 0 END)
             FROM workspace_views
             WHERE connected_workspace_id = ?1",
            [connected.as_bytes().as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("workspace views should be inspectable");
    assert_eq!(counts, (1, 0));
}

#[test]
fn completed_multi_repository_view_is_canonical_and_reopen_pinned() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let connected = ConnectedWorkspaceId::new([0x81; 32]);
    let first_slot = SourceSlotId::new([0x11; 32]);
    let second_slot = SourceSlotId::new([0x22; 32]);
    let first_repository = RepositoryIdentityDigest::new([0x31; 32]);
    let second_repository = RepositoryIdentityDigest::new([0x42; 32]);
    let (store, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("store should start");
    register_and_connect_sources(
        &store,
        connected,
        &[
            (first_slot, first_repository),
            (second_slot, second_repository),
        ],
    );
    let second = complete_workspace_source(
        &store,
        connected,
        second_slot,
        second_repository,
        SourceSlotEpoch::INITIAL,
        0x52,
        "multi-second",
    );
    let first = complete_workspace_source(
        &store,
        connected,
        first_slot,
        first_repository,
        SourceSlotEpoch::INITIAL,
        0x51,
        "multi-first",
    );

    let published = store
        .publish_completed_workspace_view(
            connected,
            vec![second, first],
            workspace_control(),
            deadline(),
        )
        .expect("completed multi-repository view should publish");
    let active = store
        .active_workspace_view(connected, workspace_control(), deadline())
        .expect("active view should load")
        .expect("active view should exist");
    assert_eq!(active.view(), published);
    assert_eq!(
        active
            .members()
            .iter()
            .map(|member| (member.source_slot(), member.repository()))
            .collect::<Vec<_>>(),
        vec![
            (first_slot, first_repository),
            (second_slot, second_repository),
        ]
    );
    store.shutdown(deadline()).expect("store should stop");

    let reader = OwnedSqliteReader::start(&database, deadline()).expect("reader should start");
    let reopened = reader
        .pin_workspace_view(
            connected,
            Some(published.get()),
            workspace_control(),
            deadline(),
        )
        .expect("exact immutable view should load after reopen")
        .expect("published view should remain available");
    assert_eq!(reopened, active);
    reader.shutdown(deadline()).expect("reader should stop");
    assert_workspace_schema_has_no_host_selection(&database);
}

#[test]
fn completed_view_keeps_two_slots_for_one_logical_repository_distinct() {
    let directory = TempDirectory::new();
    let connected = ConnectedWorkspaceId::new([0x82; 32]);
    let first_slot = SourceSlotId::new([0x13; 32]);
    let second_slot = SourceSlotId::new([0x24; 32]);
    let repository = RepositoryIdentityDigest::new([0x35; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("store should start");
    register_and_connect_sources(
        &store,
        connected,
        &[(first_slot, repository), (second_slot, repository)],
    );
    let first = complete_workspace_source(
        &store,
        connected,
        first_slot,
        repository,
        SourceSlotEpoch::INITIAL,
        0x61,
        "same-repository-first",
    );
    let second = complete_workspace_source(
        &store,
        connected,
        second_slot,
        repository,
        SourceSlotEpoch::INITIAL,
        0x62,
        "same-repository-second",
    );

    store
        .publish_completed_workspace_view(
            connected,
            vec![second, first],
            workspace_control(),
            deadline(),
        )
        .expect("same-repository slots should publish independently");
    let active = store
        .active_workspace_view(connected, workspace_control(), deadline())
        .expect("active view should load")
        .expect("active view should exist");
    assert_eq!(active.members().len(), 2);
    assert_eq!(active.members()[0].source_slot(), first_slot);
    assert_eq!(active.members()[1].source_slot(), second_slot);
    assert!(
        active
            .members()
            .iter()
            .all(|member| member.repository() == repository)
    );
    assert_ne!(
        active.members()[0].generation(),
        active.members()[1].generation()
    );
}

#[test]
fn completed_view_revalidates_stale_receipts_and_control_before_publication() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let connected = ConnectedWorkspaceId::new([0x83; 32]);
    let source_slot = SourceSlotId::new([0x15; 32]);
    let repository = RepositoryIdentityDigest::new([0x37; 32]);
    let (store, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("store should start");
    register_and_connect_sources(&store, connected, &[(source_slot, repository)]);
    let initial = complete_workspace_source(
        &store,
        connected,
        source_slot,
        repository,
        SourceSlotEpoch::INITIAL,
        0x71,
        "initial-completed",
    );
    let initial_view = store
        .publish_completed_workspace_view(connected, vec![initial], workspace_control(), deadline())
        .expect("initial view should publish");
    assert_eq!(
        store.publish_completed_workspace_view(
            connected,
            vec![initial, initial],
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::InvalidWorkspaceView)
    );

    let next_epoch = store
        .reserve_source_slot_epoch(
            connected,
            source_slot,
            SourceSlotEpoch::INITIAL,
            workspace_control(),
            deadline(),
        )
        .expect("next epoch should reserve");
    let stale = complete_workspace_source(
        &store,
        connected,
        source_slot,
        repository,
        next_epoch,
        0x72,
        "stale-completed",
    );
    store
        .reserve_source_slot_epoch(
            connected,
            source_slot,
            next_epoch,
            workspace_control(),
            deadline(),
        )
        .expect("completed receipt should become stale");
    assert_eq!(
        store.publish_completed_workspace_view(
            connected,
            vec![stale],
            workspace_control(),
            deadline(),
        ),
        Err(SqliteStoreError::StaleSourceEpoch)
    );
    assert_eq!(
        store.publish_completed_workspace_view(
            connected,
            vec![initial],
            Arc::new(AtomicBool::new(true)),
            deadline(),
        ),
        Err(SqliteStoreError::Cancelled)
    );
    assert_eq!(
        store
            .active_workspace_view(connected, workspace_control(), deadline())
            .expect("prior view should remain readable")
            .expect("prior view should remain active")
            .view(),
        initial_view
    );

    store.shutdown(deadline()).expect("store should stop");
    assert_one_published_workspace_view(&database, connected);
}
