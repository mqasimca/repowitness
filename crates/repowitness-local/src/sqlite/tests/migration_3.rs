#[test]
fn populated_version_two_database_upgrades_with_default_active_view() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let mut connection =
        Connection::open(&database).expect("version-two fixture database should open");
    apply_migration(&mut connection, 1, MIGRATION_1_NAME, MIGRATION_1, 111)
        .expect("accepted version-one baseline should apply");
    apply_migration(&mut connection, 2, MIGRATION_2_NAME, MIGRATION_2, 222)
        .expect("accepted migration two should apply");
    insert_workspace(&connection);
    insert_active_generation_fixture(&connection);
    drop(connection);

    let connection =
        open_index_writer(&database, 333).expect("populated version two should upgrade");
    let user_version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema version should be readable");
    let migration: (String, Vec<u8>, i64) = connection
        .query_row(
            "SELECT name, checksum, applied_at_unix_ms
             FROM schema_migrations WHERE version = 3",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("migration-three ledger row should be readable");
    let backfill: (i64, i64, i64, i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT count(*) FROM connected_workspaces),
                (SELECT count(*) FROM workspace_source_slots),
                (SELECT count(*) FROM workspace_views
                 WHERE lifecycle_state = 'published'),
                (SELECT count(*) FROM workspace_view_members
                 WHERE generation_id = 1 AND ordinal = 0),
                (SELECT count(*) FROM active_workspace_views)",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("workspace migration backfill should be readable");
    let identity_match: i64 = connection
        .query_row(
            "SELECT count(*)
             FROM workspace_source_slots AS slot
             JOIN workspaces AS workspace
               ON workspace.workspace_id = slot.generation_workspace_id
             WHERE slot.connected_workspace_id = workspace.repository_identity
               AND slot.source_slot_id = workspace.repository_identity
               AND slot.repository_identity = workspace.repository_identity",
            [],
            |row| row.get(0),
        )
        .expect("default source-slot identity should be readable");
    let foreign_key_failures: i64 = connection
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .expect("foreign-key check should run");

    assert_eq!(user_version, 3);
    assert_eq!(
        migration,
        (
            MIGRATION_3_NAME.to_owned(),
            migration_checksum(MIGRATION_3).to_vec(),
            333,
        )
    );
    assert_eq!(backfill, (1, 1, 1, 1, 1));
    assert_eq!(identity_match, 1);
    assert_eq!(foreign_key_failures, 0);
}

#[test]
fn version_two_without_active_generation_backfills_membership_only() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let mut connection =
        Connection::open(&database).expect("version-two fixture database should open");
    apply_migration(&mut connection, 1, MIGRATION_1_NAME, MIGRATION_1, 111)
        .expect("accepted version-one baseline should apply");
    apply_migration(&mut connection, 2, MIGRATION_2_NAME, MIGRATION_2, 222)
        .expect("accepted migration two should apply");
    insert_workspace(&connection);
    drop(connection);

    let connection = open_index_writer(&database, 333).expect("version two should upgrade");
    let counts: (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT count(*) FROM connected_workspaces),
                (SELECT count(*) FROM workspace_source_slots),
                (SELECT count(*) FROM workspace_views),
                (SELECT count(*) FROM active_workspace_views)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("workspace migration backfill should be readable");

    assert_eq!(counts, (1, 1, 0, 0));
}

#[test]
fn workspace_schema_contains_no_host_root_or_selector_columns() {
    let directory = TempDirectory::new();
    let connection =
        open_index_writer(&directory.database(), 123).expect("migration should succeed");
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
    let lowercase = schema.to_ascii_lowercase();

    for forbidden in [
        "root_path",
        "filesystem_path",
        "branch_name",
        "ref_name",
        "selector_text",
    ] {
        assert!(!lowercase.contains(forbidden));
    }
}

#[test]
fn source_slot_membership_freezes_at_publication_not_staging() {
    let directory = TempDirectory::new();
    let connection = open_populated_version_three_workspace(&directory);
    let connected = [0x30_u8; 32];
    let first_slot = [0x31_u8; 32];
    let second_slot = [0x32_u8; 32];
    let repository = [0x10_u8; 32];
    connection
        .execute(
            "INSERT INTO connected_workspaces(connected_workspace_id) VALUES (?1)",
            [&connected.as_slice()],
        )
        .expect("connected workspace should insert");
    connection
        .execute(
            "INSERT INTO workspace_source_slots(
                connected_workspace_id, source_slot_id, repository_identity,
                generation_workspace_id
             ) VALUES (?1, ?2, ?3, 1)",
            params![
                &connected.as_slice(),
                &first_slot.as_slice(),
                &repository.as_slice(),
            ],
        )
        .expect("first source slot should insert");
    connection
        .execute(
            "INSERT INTO workspace_views(
                connected_workspace_id, lifecycle_state
             ) VALUES (?1, 'staging')",
            [&connected.as_slice()],
        )
        .expect("staging view should insert");
    connection
        .execute(
            "INSERT INTO workspace_source_slots(
                connected_workspace_id, source_slot_id, repository_identity,
                generation_workspace_id
             ) VALUES (?1, ?2, ?3, 1)",
            params![
                &connected.as_slice(),
                &second_slot.as_slice(),
                &repository.as_slice(),
            ],
        )
        .expect("staging alone must not freeze membership");
}

fn open_populated_version_three_workspace(directory: &TempDirectory) -> Connection {
    let database = directory.database();
    let mut connection =
        Connection::open(&database).expect("version-two fixture database should open");
    apply_migration(&mut connection, 1, MIGRATION_1_NAME, MIGRATION_1, 111)
        .expect("accepted version-one baseline should apply");
    apply_migration(&mut connection, 2, MIGRATION_2_NAME, MIGRATION_2, 222)
        .expect("accepted migration two should apply");
    insert_workspace(&connection);
    insert_active_generation_fixture(&connection);
    drop(connection);

    open_index_writer(&database, 333).expect("version two should upgrade")
}

fn assert_unpublished_pointer_switch_rolls_back(connection: &mut Connection, connected: &[u8]) {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("fixture transaction should start");
    transaction
        .execute(
            "INSERT INTO workspace_views(
                connected_workspace_id, lifecycle_state
             ) VALUES (?1, 'staging')",
            [connected],
        )
        .expect("staging view should insert");
    let staging_view = transaction.last_insert_rowid();
    assert!(
        transaction
            .execute(
                "UPDATE active_workspace_views SET workspace_view_id = ?1
                 WHERE connected_workspace_id = ?2",
                params![staging_view, connected],
            )
            .is_err(),
        "an active pointer must reject an unpublished view"
    );
    transaction
        .rollback()
        .expect("failed pointer switch should roll back");
}

fn assert_incomplete_publication_rolls_back(connection: &mut Connection, connected: &[u8]) {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("fixture transaction should start");
    transaction
        .execute(
            "INSERT INTO workspace_views(
                connected_workspace_id, lifecycle_state
             ) VALUES (?1, 'staging')",
            [connected],
        )
        .expect("staging view should insert");
    let staging_view = transaction.last_insert_rowid();
    assert!(
        transaction
            .execute(
                "UPDATE workspace_views SET lifecycle_state = 'published'
                 WHERE workspace_view_id = ?1",
                [staging_view],
            )
            .is_err(),
        "an incomplete view must not become published"
    );
    transaction
        .rollback()
        .expect("failed publication should roll back");
}

fn assert_published_view_is_immutable(
    connection: &Connection,
    connected: &[u8],
    original_view: i64,
) {
    assert!(
        connection
            .execute(
                "UPDATE workspace_views SET lifecycle_state = 'staging'
                 WHERE workspace_view_id = ?1",
                [original_view],
            )
            .is_err(),
        "published lifecycle must be immutable"
    );
    assert!(
        connection
            .execute(
                "DELETE FROM workspace_views WHERE workspace_view_id = ?1",
                [original_view],
            )
            .is_err(),
        "published views must be immutable"
    );
    assert!(
        connection
            .execute(
                "INSERT INTO workspace_views(
                    connected_workspace_id, lifecycle_state
                 ) VALUES (?1, 'retained')",
                [connected],
            )
            .is_err(),
        "active and retained are pointer-derived, not competing lifecycle states"
    );
}

fn assert_active_pointer_identity_is_immutable(connection: &Connection, connected: &[u8]) {
    let alternate_connected = [0x20_u8; 32];
    let alternate_slot = [0x21_u8; 32];
    connection
        .execute(
            "INSERT INTO connected_workspaces(connected_workspace_id) VALUES (?1)",
            [&alternate_connected.as_slice()],
        )
        .expect("alternate connected workspace should insert");
    connection
        .execute(
            "INSERT INTO workspace_source_slots(
                connected_workspace_id, source_slot_id, repository_identity,
                generation_workspace_id
             ) VALUES (?1, ?2, ?3, 1)",
            params![
                &alternate_connected.as_slice(),
                &alternate_slot.as_slice(),
                connected,
            ],
        )
        .expect("alternate source slot should insert");
    connection
        .execute(
            "INSERT INTO source_slot_generation_receipts(
                connected_workspace_id, source_slot_id, source_epoch,
                generation_workspace_id, generation_id
             ) VALUES (?1, ?2, 0, 1, 1)",
            params![
                &alternate_connected.as_slice(),
                &alternate_slot.as_slice(),
            ],
        )
        .expect("alternate source-slot receipt should insert");
    connection
        .execute(
            "INSERT INTO workspace_views(
                connected_workspace_id, lifecycle_state
             ) VALUES (?1, 'staging')",
            [&alternate_connected.as_slice()],
        )
        .expect("alternate staging view should insert");
    let alternate_view = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO workspace_view_members(
                workspace_view_id, connected_workspace_id, source_slot_id,
                source_epoch, ordinal, generation_workspace_id, generation_id
             ) VALUES (?1, ?2, ?3, 0, 0, 1, 1)",
            params![
                alternate_view,
                &alternate_connected.as_slice(),
                &alternate_slot.as_slice(),
            ],
        )
        .expect("alternate member should insert");
    connection
        .execute(
            "UPDATE workspace_views SET lifecycle_state = 'published'
             WHERE workspace_view_id = ?1",
            [alternate_view],
        )
        .expect("alternate view should publish");

    assert!(
        connection
            .execute(
                "UPDATE active_workspace_views
                 SET connected_workspace_id = ?1, workspace_view_id = ?2
                 WHERE connected_workspace_id = ?3",
                params![&alternate_connected.as_slice(), alternate_view, connected,],
            )
            .is_err(),
        "an active pointer must not move to a different connected workspace"
    );
}

#[test]
fn active_pointer_can_reference_only_an_immutable_published_view() {
    let directory = TempDirectory::new();
    let mut connection = open_populated_version_three_workspace(&directory);
    let connected = [0x10; 32];
    let original_view: i64 = connection
        .query_row(
            "SELECT workspace_view_id FROM active_workspace_views
             WHERE connected_workspace_id = ?1",
            [&connected.as_slice()],
            |row| row.get(0),
        )
        .expect("backfilled active pointer should exist");

    assert_unpublished_pointer_switch_rolls_back(&mut connection, &connected);
    assert_incomplete_publication_rolls_back(&mut connection, &connected);
    assert_published_view_is_immutable(&connection, &connected, original_view);
    assert_active_pointer_identity_is_immutable(&connection, &connected);

    let consistency: (i64, i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT workspace_view_id FROM active_workspace_views
                 WHERE connected_workspace_id = ?1),
                (SELECT count(*) FROM workspace_views
                 WHERE lifecycle_state = 'staging'),
                (SELECT count(*)
                 FROM active_workspace_views AS active
                 JOIN workspace_views AS view
                   ON view.connected_workspace_id =
                      active.connected_workspace_id
                  AND view.workspace_view_id = active.workspace_view_id
                 WHERE view.lifecycle_state != 'published')",
            [&connected.as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("pointer consistency should be readable");
    assert_eq!(consistency, (original_view, 0, 0));
}

#[test]
fn interrupted_complete_view_switch_preserves_previous_pointer() {
    let directory = TempDirectory::new();
    let mut connection = open_populated_version_three_workspace(&directory);
    let connected = [0x10; 32];
    let original_view: i64 = connection
        .query_row(
            "SELECT workspace_view_id FROM active_workspace_views
             WHERE connected_workspace_id = ?1",
            [&connected.as_slice()],
            |row| row.get(0),
        )
        .expect("backfilled active pointer should exist");

    {
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .expect("publication transaction should start");
        transaction
            .execute(
                "INSERT INTO workspace_views(
                    connected_workspace_id, lifecycle_state
                 ) VALUES (?1, 'staging')",
                [&connected.as_slice()],
            )
            .expect("staging view should insert");
        let next_view = transaction.last_insert_rowid();
        transaction
            .execute(
                "INSERT INTO workspace_view_members(
                    workspace_view_id, connected_workspace_id, source_slot_id,
                    source_epoch, ordinal, generation_workspace_id, generation_id
                 ) VALUES (?1, ?2, ?2, 0, 0, 1, 1)",
                params![next_view, &connected.as_slice()],
            )
            .expect("complete member should insert");
        transaction
            .execute(
                "UPDATE workspace_views SET lifecycle_state = 'published'
                 WHERE workspace_view_id = ?1",
                [next_view],
            )
            .expect("complete view should publish inside the transaction");
        transaction
            .execute(
                "UPDATE active_workspace_views SET workspace_view_id = ?1
                 WHERE connected_workspace_id = ?2",
                params![next_view, &connected.as_slice()],
            )
            .expect("pointer should switch inside the transaction");
        drop(transaction);
    }
    drop(connection);

    let connection = open_index_writer(&directory.database(), 444).expect("database should reopen");
    let state: (i64, i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT workspace_view_id FROM active_workspace_views
                 WHERE connected_workspace_id = ?1),
                (SELECT count(*) FROM workspace_views
                 WHERE connected_workspace_id = ?1),
                (SELECT count(*) FROM workspace_views
                 WHERE lifecycle_state = 'staging')",
            [&connected.as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("reopened view state should be readable");
    assert_eq!(state, (original_view, 1, 0));
}
