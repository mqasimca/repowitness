pub(super) fn load_pinned_view_members(
    transaction: &Transaction<'_>,
    view: WorkspaceViewId,
) -> Result<Vec<PinnedWorkspaceViewMember>, SqliteStoreError> {
    let limit = i64::try_from(MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS + 1)
        .map_err(|_| SqliteStoreError::CountNotRepresentable)?;
    let mut statement = transaction
        .prepare(
            "SELECT member.ordinal, member.source_slot_id, member.source_epoch,
                    slot.repository_identity, member.generation_id,
                    generation.lifecycle_state
             FROM workspace_view_members AS member
             JOIN workspace_source_slots AS slot
               ON slot.connected_workspace_id = member.connected_workspace_id
              AND slot.source_slot_id = member.source_slot_id
             JOIN index_generations AS generation
               ON generation.workspace_id = member.generation_workspace_id
              AND generation.generation_id = member.generation_id
             WHERE member.workspace_view_id = ?1
             ORDER BY member.ordinal
             LIMIT ?2",
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let rows = statement
        .query_map(params![view.get(), limit], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    rows.map(|row| {
        let (ordinal, source_slot, source_epoch, repository, generation, lifecycle_state) =
            row.map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let ordinal = u16::try_from(ordinal).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let source_slot = SourceSlotId::try_from_slice(&source_slot)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let source_epoch = u64::try_from(source_epoch)
            .ok()
            .and_then(|value| SourceSlotEpoch::try_new(value).ok())
            .ok_or(SqliteStoreError::IntegrityCheckFailed)?;
        let repository = RepositoryIdentityDigest::try_from_slice(&repository)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        if generation <= 0 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        if !matches!(lifecycle_state.as_str(), "ready" | "active" | "retained") {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        Ok(PinnedWorkspaceViewMember::new(
            ordinal,
            source_slot,
            source_epoch,
            repository,
            GenerationId::from_database(generation),
        ))
    })
    .collect()
}

pub(super) fn validate_pinned_view_members(
    members: &[PinnedWorkspaceViewMember],
    expected_members: usize,
) -> Result<(), SqliteStoreError> {
    if members.is_empty()
        || members.len() != expected_members
        || members.len() > MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS
    {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    for (expected, member) in members.iter().enumerate() {
        if usize::from(member.ordinal()) != expected {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        if expected > 0 && members[expected - 1].source_slot() >= member.source_slot() {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
    }
    Ok(())
}

pub(super) fn expected_view_member_count(
    transaction: &Transaction<'_>,
    connected_workspace: ConnectedWorkspaceId,
    view: WorkspaceViewId,
) -> Result<usize, SqliteStoreError> {
    let (source_slots, view_members): (i64, i64) = transaction
        .query_row(
            "SELECT
                (SELECT count(*) FROM workspace_source_slots
                 WHERE connected_workspace_id = ?1),
                (SELECT count(*) FROM workspace_view_members
                 WHERE connected_workspace_id = ?1
                   AND workspace_view_id = ?2)",
            params![connected_workspace.as_bytes().as_slice(), view.get()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let source_slots =
        usize::try_from(source_slots).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let view_members =
        usize::try_from(view_members).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    if source_slots == 0
        || source_slots != view_members
        || source_slots > MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS
    {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    Ok(source_slots)
}

fn check_workspace_deadline(deadline: Instant) -> Result<(), SqliteStoreError> {
    if Instant::now() >= deadline {
        Err(SqliteStoreError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

fn ensure_default_workspace_membership(
    transaction: &Transaction<'_>,
    repository: RepositoryIdentityDigest,
    generation_workspace_id: i64,
    source_epoch: SourceSlotEpoch,
) -> Result<(), SqliteStoreError> {
    let connected_workspace = ConnectedWorkspaceId::for_single_repository(repository);
    let source_slot = SourceSlotId::for_repository(repository);
    transaction
        .execute(
            "INSERT INTO connected_workspaces(connected_workspace_id)
             VALUES (?1)
             ON CONFLICT(connected_workspace_id) DO NOTHING",
            [connected_workspace.as_bytes().as_slice()],
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let persisted = transaction
        .query_row(
            "SELECT repository_identity, generation_workspace_id, source_epoch
             FROM workspace_source_slots
             WHERE connected_workspace_id = ?1 AND source_slot_id = ?2",
            params![
                connected_workspace.as_bytes().as_slice(),
                source_slot.as_bytes().as_slice(),
            ],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    if let Some((persisted_repository, persisted_workspace, persisted_epoch)) = persisted {
        if persisted_repository.as_slice() != repository.as_bytes()
            || persisted_workspace != generation_workspace_id
            || decode_source_slot_epoch(persisted_epoch)? != source_epoch
        {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        return Ok(());
    }
    transaction
        .execute(
            "INSERT INTO workspace_source_slots(
                connected_workspace_id, source_slot_id, repository_identity,
                generation_workspace_id, source_epoch
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                connected_workspace.as_bytes().as_slice(),
                source_slot.as_bytes().as_slice(),
                repository.as_bytes().as_slice(),
                generation_workspace_id,
                fixed_integer(source_epoch.get())?,
            ],
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    Ok(())
}

fn publish_default_workspace_view(
    transaction: &Transaction<'_>,
    repository: RepositoryIdentityDigest,
    generation_workspace_id: i64,
    source_epoch: SourceSlotEpoch,
    generation: GenerationId,
) -> Result<(), SqliteStoreError> {
    let connected_workspace = ConnectedWorkspaceId::for_single_repository(repository);
    let source_slot = SourceSlotId::for_repository(repository);
    transaction
        .execute(
            "INSERT INTO workspace_views(
                connected_workspace_id, lifecycle_state
             ) VALUES (?1, 'staging')",
            [connected_workspace.as_bytes().as_slice()],
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let view = WorkspaceViewId::from_database(transaction.last_insert_rowid());
    if view.get() <= 0 {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    insert_source_slot_receipt(
        transaction,
        connected_workspace,
        source_slot,
        source_epoch,
        generation_workspace_id,
        generation,
    )?;
    transaction
        .execute(
            "INSERT INTO workspace_view_members(
                workspace_view_id, connected_workspace_id, source_slot_id,
                source_epoch, ordinal, generation_workspace_id, generation_id
             ) VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)",
            params![
                view.get(),
                connected_workspace.as_bytes().as_slice(),
                source_slot.as_bytes().as_slice(),
                fixed_integer(source_epoch.get())?,
                generation_workspace_id,
                generation.get(),
            ],
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    transaction
        .execute(
            "UPDATE workspace_views SET lifecycle_state = 'published'
             WHERE workspace_view_id = ?1 AND lifecycle_state = 'staging'",
            [view.get()],
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    transaction
        .execute(
            "INSERT INTO active_workspace_views(
                connected_workspace_id, workspace_view_id
             ) VALUES (?1, ?2)
             ON CONFLICT(connected_workspace_id)
             DO UPDATE SET workspace_view_id = excluded.workspace_view_id",
            params![connected_workspace.as_bytes().as_slice(), view.get()],
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    Ok(())
}
