impl WriterState {
    pub(super) fn connect_workspace(
        &mut self,
        connected_workspace: ConnectedWorkspaceId,
        source_slots: &[WorkspaceSourceSlot],
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        check_control(control)?;
        let source_slots = canonical_source_slots(source_slots)?;
        let transaction = self.transaction()?;
        let resolved = resolve_source_slot_mappings(&transaction, &source_slots, control)?;

        transaction
            .execute(
                "INSERT INTO connected_workspaces(connected_workspace_id)
                 VALUES (?1)
                 ON CONFLICT(connected_workspace_id) DO NOTHING",
                [connected_workspace.as_bytes().as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;

        let persisted = load_source_slot_mappings(&transaction, connected_workspace)?;
        if persisted != resolved {
            if !persisted.is_empty() {
                clear_unpublished_workspace_membership(&transaction, connected_workspace, control)?;
            }
            insert_source_slot_mappings(&transaction, connected_workspace, &resolved, control)?;
        }

        check_control(control)?;
        commit_mutation(transaction)
    }
}

fn resolve_source_slot_mappings(
    transaction: &Transaction<'_>,
    source_slots: &[WorkspaceSourceSlot],
    control: WriteControl<'_>,
) -> Result<Vec<(WorkspaceSourceSlot, i64)>, SqliteStoreError> {
    let mut resolved = Vec::with_capacity(source_slots.len());
    for source_slot in source_slots {
        check_control(control)?;
        let generation_workspace_id = transaction
            .query_row(
                "SELECT workspace_id FROM workspaces
                 WHERE repository_identity = ?1",
                [source_slot.repository().as_bytes().as_slice()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?
            .ok_or(SqliteStoreError::WorkspaceUnavailable)?;
        resolved.push((*source_slot, generation_workspace_id));
    }
    Ok(resolved)
}

fn clear_unpublished_workspace_membership(
    transaction: &Transaction<'_>,
    connected_workspace: ConnectedWorkspaceId,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    let published_or_active = transaction
        .query_row(
            "SELECT
                EXISTS(
                    SELECT 1 FROM workspace_views
                    WHERE connected_workspace_id = ?1
                      AND lifecycle_state = 'published'
                )
                OR EXISTS(
                    SELECT 1 FROM active_workspace_views
                    WHERE connected_workspace_id = ?1
                )",
            [connected_workspace.as_bytes().as_slice()],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    if published_or_active {
        return Err(SqliteStoreError::InvalidWorkspaceMembership);
    }

    for statement in [
        "DELETE FROM workspace_view_members
         WHERE connected_workspace_id = ?1",
        "DELETE FROM workspace_views
         WHERE connected_workspace_id = ?1",
        "DELETE FROM source_slot_generation_receipts
         WHERE connected_workspace_id = ?1",
        "DELETE FROM workspace_source_slots
         WHERE connected_workspace_id = ?1",
    ] {
        check_control(control)?;
        transaction
            .execute(statement, [connected_workspace.as_bytes().as_slice()])
            .map_err(|_| SqliteStoreError::InvalidWorkspaceMembership)?;
    }
    Ok(())
}

fn insert_source_slot_mappings(
    transaction: &Transaction<'_>,
    connected_workspace: ConnectedWorkspaceId,
    resolved: &[(WorkspaceSourceSlot, i64)],
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    for (source_slot, generation_workspace_id) in resolved {
        check_control(control)?;
        transaction
            .execute(
                "INSERT INTO workspace_source_slots(
                    connected_workspace_id, source_slot_id,
                    repository_identity, generation_workspace_id, source_epoch
                 ) VALUES (?1, ?2, ?3, ?4, 0)",
                params![
                    connected_workspace.as_bytes().as_slice(),
                    source_slot.source_slot().as_bytes().as_slice(),
                    source_slot.repository().as_bytes().as_slice(),
                    generation_workspace_id,
                ],
            )
            .map_err(|_| SqliteStoreError::InvalidWorkspaceMembership)?;
    }
    Ok(())
}
