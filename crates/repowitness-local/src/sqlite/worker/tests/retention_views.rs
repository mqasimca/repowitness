fn advance_and_stage_retention_view_generation(
    store: &OwnedSqliteIndex,
    repository: RepositoryIdentityDigest,
    expected_epoch: u64,
    next_epoch: u64,
    salt: u8,
    suffix: &str,
) -> GenerationId {
    store
        .advance_source_epoch(repository, expected_epoch, next_epoch, deadline())
        .expect("default source epoch should advance");
    store
        .stage(
            next_epoch,
            workspace_snapshot_identity(repository, salt),
            prepared(suffix),
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("retention view generation should stage")
}

fn publish_successor_mixed_view(
    fixture: &TwoRepositoryViewFixture,
    successor_generations: [GenerationId; 2],
) {
    let first_connected_epoch = fixture
        .store
        .reserve_source_slot_epoch(
            fixture.connected,
            fixture.first_slot,
            SourceSlotEpoch::INITIAL,
            workspace_control(),
            deadline(),
        )
        .expect("first connected slot should advance");
    let second_connected_epoch = fixture
        .store
        .reserve_source_slot_epoch(
            fixture.connected,
            fixture.second_slot,
            SourceSlotEpoch::INITIAL,
            workspace_control(),
            deadline(),
        )
        .expect("second connected slot should advance");
    complete_slot_at(
        &fixture.store,
        fixture.connected,
        fixture.first_slot,
        first_connected_epoch,
        successor_generations[0],
    );
    complete_slot_at(
        &fixture.store,
        fixture.connected,
        fixture.second_slot,
        second_connected_epoch,
        successor_generations[1],
    );
    fixture
        .store
        .activate(successor_generations[0], 1, deadline())
        .expect("first successor should activate");
    fixture
        .store
        .activate(successor_generations[1], 1, deadline())
        .expect("second successor should activate");
    fixture
        .store
        .publish_workspace_view(
            fixture.connected,
            vec![
                WorkspaceViewMember::at_epoch(
                    fixture.first_slot,
                    first_connected_epoch,
                    successor_generations[0],
                ),
                WorkspaceViewMember::at_epoch(
                    fixture.second_slot,
                    second_connected_epoch,
                    successor_generations[1],
                ),
            ],
            workspace_control(),
            deadline(),
        )
        .expect("successor connected view should publish");
}

fn retained_mixed_view_fixture(directory: &TempDirectory) -> TwoRepositoryViewFixture {
    let fixture = TwoRepositoryViewFixture::new(directory);
    fixture
        .store
        .publish_workspace_view(
            fixture.connected,
            fixture.members(fixture.first_generation, fixture.second_generation),
            workspace_control(),
            deadline(),
        )
        .expect("initial connected view should publish");
    let successors = [
        advance_and_stage_retention_view_generation(
            &fixture.store,
            fixture.first_repository,
            0,
            1,
            0xA1,
            "retention-view-next-first",
        ),
        advance_and_stage_retention_view_generation(
            &fixture.store,
            fixture.second_repository,
            0,
            1,
            0xA2,
            "retention-view-next-second",
        ),
    ];
    publish_successor_mixed_view(&fixture, successors);

    for (repository, salt, suffix) in [
        (fixture.first_repository, 0xB1, "retention-view-first"),
        (fixture.second_repository, 0xB2, "retention-view-second"),
    ] {
        let generation = advance_and_stage_retention_view_generation(
            &fixture.store,
            repository,
            1,
            2,
            salt,
            suffix,
        );
        fixture
            .store
            .activate(generation, 2, deadline())
            .expect("newest generation should activate");
    }
    fixture
}

fn retained_mixed_view_policy(fixture: &TwoRepositoryViewFixture) -> GenerationRetentionPolicy {
    let pins = RetentionPins::try_new(vec![fixture.second_generation], Vec::new(), Vec::new())
        .expect("mixed-view retained member should pin");
    retention_policy(1, RetentionLimits::default(), pins)
}

#[test]
fn retention_estimate_covers_every_member_of_a_collected_mixed_view() {
    let directory = TempDirectory::new();
    let fixture = retained_mixed_view_fixture(&directory);
    let policy = retained_mixed_view_policy(&fixture);
    let plan = plan_retention(&fixture.store, policy.clone());
    assert_eq!(
        plan.candidate_generations(),
        &[fixture.first_generation],
        "only the unpinned old member should be eligible"
    );

    let outcome = fixture
        .store
        .apply_generation_retention(RetentionApplyRequest::new(
            policy,
            plan.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("mixed historical view should collect");
    assert!(
        outcome.deleted_rows() <= plan.estimated_rows(),
        "the hard row budget must include non-candidate members deleted with a view"
    );
    assert_eq!(outcome.generation_count(), 1);
    assert!(
        fixture
            .store
            .active_generation(fixture.second_repository, deadline())
            .expect("second workspace should remain readable")
            .is_some()
    );
    fixture
        .store
        .shutdown(deadline())
        .expect("retention view fixture should stop");
}

fn inject_foreign_plan_marks(directory: &TempDirectory, fixture: &TwoRepositoryViewFixture) {
    let connection =
        Connection::open(directory.database()).expect("fault fixture database should open");
    connection
        .execute_batch("PRAGMA foreign_keys = ON; BEGIN IMMEDIATE;")
        .expect("fault transaction should begin");
    let stale_digest = [0xAC_u8; 32];
    connection
        .execute(
            "INSERT INTO retention_generation_garbage(
                generation_id, plan_digest, lifecycle_state
             ) VALUES (?1, ?2, 'garbage')",
            params![fixture.second_generation.get(), stale_digest.as_slice()],
        )
        .expect("stale generation mark should be injectable");
    connection
        .execute(
            "INSERT INTO retention_workspace_view_garbage(
                workspace_view_id, plan_digest, lifecycle_state
             )
             SELECT DISTINCT view.workspace_view_id, ?2, 'garbage'
             FROM workspace_views AS view
             JOIN workspace_view_members AS member
               ON member.workspace_view_id = view.workspace_view_id
             WHERE member.generation_id = ?1
               AND view.connected_workspace_id != ?3
               AND NOT EXISTS (
                   SELECT 1 FROM active_workspace_views AS active
                   WHERE active.workspace_view_id = view.workspace_view_id
               )",
            params![
                fixture.second_generation.get(),
                stale_digest.as_slice(),
                fixture.connected.as_bytes().as_slice()
            ],
        )
        .expect("stale single-source view mark should be injectable");
    connection
        .execute(
            "INSERT INTO retention_source_slot_receipt_garbage(
                source_slot_id, source_epoch, plan_digest, lifecycle_state
             )
             SELECT receipt.source_slot_id, receipt.source_epoch, ?2, 'garbage'
             FROM source_slot_generation_receipts AS receipt
             WHERE receipt.generation_id = ?1
               AND NOT EXISTS (
                   SELECT 1 FROM workspace_source_slots AS slot
                   WHERE slot.connected_workspace_id = receipt.connected_workspace_id
                     AND slot.source_slot_id = receipt.source_slot_id
                     AND slot.source_epoch = receipt.source_epoch
               )",
            params![fixture.second_generation.get(), stale_digest.as_slice()],
        )
        .expect("stale receipt marks should be injectable");
    connection
        .execute_batch("COMMIT")
        .expect("coherent stale marks should persist");
}

fn failed_mixed_view_collection_state(
    directory: &TempDirectory,
    fixture: &TwoRepositoryViewFixture,
) -> (i64, i64, i64, i64) {
    let connection =
        Connection::open(directory.database()).expect("failed-closed database should open");
    connection
        .query_row(
            "SELECT
                (SELECT count(*) FROM index_generations
                 WHERE generation_id = ?1),
                (SELECT count(*) FROM index_generations
                 WHERE generation_id = ?2),
                (SELECT count(*) FROM retention_generation_garbage),
                (SELECT count(*) FROM retention_collection_audit)",
            params![
                fixture.first_generation.get(),
                fixture.second_generation.get()
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("failed-closed mixed-view state should be readable")
}

#[test]
fn stale_marks_cannot_delete_a_kept_member_of_a_collected_multi_source_view() {
    let directory = TempDirectory::new();
    let fixture = retained_mixed_view_fixture(&directory);
    let policy = retained_mixed_view_policy(&fixture);
    let plan = plan_retention(&fixture.store, policy.clone());
    assert_eq!(
        plan.candidate_generations(),
        &[fixture.first_generation],
        "the other historical view member must remain explicitly pinned"
    );

    inject_foreign_plan_marks(&directory, &fixture);
    assert_eq!(
        fixture
            .store
            .apply_generation_retention(RetentionApplyRequest::new(
                policy,
                plan.plan_digest(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )),
        Err(SqliteStoreError::IntegrityCheckFailed),
        "foreign-plan marks must block the sweep before any view member is removed"
    );
    assert_eq!(
        failed_mixed_view_collection_state(&directory, &fixture),
        (1, 1, 1, 0)
    );
    fixture
        .store
        .shutdown(deadline())
        .expect("stale-mark fixture should stop");
}
