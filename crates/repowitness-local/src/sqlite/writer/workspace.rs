impl WriterState {
    pub(super) fn source_slot_state(
        &mut self,
        connected_workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        control: WriteControl<'_>,
    ) -> Result<SourceSlotState, SqliteStoreError> {
        check_control(control)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let current_epoch = transaction
            .query_row(
                "SELECT source_epoch FROM workspace_source_slots
                 WHERE connected_workspace_id = ?1 AND source_slot_id = ?2",
                params![
                    connected_workspace.as_bytes().as_slice(),
                    source_slot.as_bytes().as_slice(),
                ],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?
            .ok_or(SqliteStoreError::ConnectedWorkspaceUnavailable)?;
        let current_epoch = decode_source_slot_epoch(current_epoch)?;
        let current_completion = load_source_slot_generation(
            &transaction,
            connected_workspace,
            source_slot,
            current_epoch,
            false,
        )?;
        check_control(control)?;
        let active = load_source_slot_generation(
            &transaction,
            connected_workspace,
            source_slot,
            current_epoch,
            true,
        )?;
        check_control(control)?;
        transaction
            .commit()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        Ok(SourceSlotState::new(
            current_epoch,
            current_completion,
            active,
        ))
    }

    pub(super) fn reserve_source_slot_epoch(
        &mut self,
        connected_workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        expected: SourceSlotEpoch,
        control: WriteControl<'_>,
    ) -> Result<SourceSlotEpoch, SqliteStoreError> {
        check_control(control)?;
        let next = expected
            .checked_next()
            .map_err(|_| SqliteStoreError::SourceEpochExhausted)?;
        let expected_database = fixed_integer(expected.get())?;
        let next_database = fixed_integer(next.get())?;
        let transaction = self.transaction()?;
        let (repository, generation_workspace_id, persisted_epoch) = transaction
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
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?
            .ok_or(SqliteStoreError::ConnectedWorkspaceUnavailable)?;
        if persisted_epoch != expected_database {
            return Err(SqliteStoreError::StaleSourceEpoch);
        }
        let repository = RepositoryIdentityDigest::try_from_slice(&repository)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let changed = transaction
            .execute(
                "UPDATE workspace_source_slots SET source_epoch = ?1
                 WHERE connected_workspace_id = ?2
                   AND source_slot_id = ?3
                   AND source_epoch = ?4",
                params![
                    next_database,
                    connected_workspace.as_bytes().as_slice(),
                    source_slot.as_bytes().as_slice(),
                    expected_database,
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::StaleSourceEpoch);
        }
        if is_default_source_slot(connected_workspace, source_slot, repository) {
            let changed = transaction
                .execute(
                    "UPDATE workspaces SET source_epoch = ?1
                     WHERE workspace_id = ?2 AND source_epoch = ?3",
                    params![next_database, generation_workspace_id, expected_database],
                )
                .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            if changed != 1 {
                return Err(SqliteStoreError::StaleSourceEpoch);
            }
        }
        check_control(control)?;
        commit_mutation(transaction)?;
        Ok(next)
    }

    pub(super) fn complete_source_slot_epoch(
        &mut self,
        connected_workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        source_epoch: SourceSlotEpoch,
        generation: GenerationId,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        check_control(control)?;
        let transaction = self.transaction()?;
        let generation_workspace_id = source_slot_generation_workspace(
            &transaction,
            connected_workspace,
            source_slot,
            source_epoch,
            generation,
        )?;
        insert_source_slot_receipt(
            &transaction,
            connected_workspace,
            source_slot,
            source_epoch,
            generation_workspace_id,
            generation,
        )?;
        check_control(control)?;
        commit_mutation(transaction)
    }

    pub(super) fn publish_workspace_view(
        &mut self,
        connected_workspace: ConnectedWorkspaceId,
        members: &[WorkspaceViewMember],
        control: WriteControl<'_>,
    ) -> Result<WorkspaceViewId, SqliteStoreError> {
        check_control(control)?;
        let members = canonical_view_members(members)?;
        let transaction = self.transaction()?;
        let source_slot_count: i64 = transaction
            .query_row(
                "SELECT count(*) FROM workspace_source_slots
                 WHERE connected_workspace_id = ?1",
                [connected_workspace.as_bytes().as_slice()],
                |row| row.get(0),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if source_slot_count == 0 {
            return Err(SqliteStoreError::ConnectedWorkspaceUnavailable);
        }
        if usize::try_from(source_slot_count).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?
            != members.len()
        {
            return Err(SqliteStoreError::InvalidWorkspaceView);
        }

        let mut resolved = Vec::with_capacity(members.len());
        for member in members {
            check_control(control)?;
            let generation_workspace_id =
                eligible_generation_workspace(&transaction, connected_workspace, member)?;
            resolved.push((member, generation_workspace_id));
        }

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
        for (ordinal, (member, generation_workspace_id)) in resolved.iter().enumerate() {
            check_control(control)?;
            let ordinal =
                i64::try_from(ordinal).map_err(|_| SqliteStoreError::CountNotRepresentable)?;
            transaction
                .execute(
                    "INSERT INTO workspace_view_members(
                        workspace_view_id, connected_workspace_id, source_slot_id,
                        source_epoch, ordinal, generation_workspace_id, generation_id
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        view.get(),
                        connected_workspace.as_bytes().as_slice(),
                        member.source_slot().as_bytes().as_slice(),
                        fixed_integer(member.source_epoch().get())?,
                        ordinal,
                        generation_workspace_id,
                        member.generation().get(),
                    ],
                )
                .map_err(|_| SqliteStoreError::InvalidWorkspaceView)?;
        }
        let published = transaction
            .execute(
                "UPDATE workspace_views SET lifecycle_state = 'published'
                 WHERE workspace_view_id = ?1 AND lifecycle_state = 'staging'",
                [view.get()],
            )
            .map_err(|_| SqliteStoreError::InvalidWorkspaceView)?;
        if published != 1 {
            return Err(SqliteStoreError::InvalidWorkspaceView);
        }
        transaction
            .execute(
                "INSERT INTO active_workspace_views(
                    connected_workspace_id, workspace_view_id
                 ) VALUES (?1, ?2)
                 ON CONFLICT(connected_workspace_id)
                 DO UPDATE SET workspace_view_id = excluded.workspace_view_id",
                params![connected_workspace.as_bytes().as_slice(), view.get()],
            )
            .map_err(|_| SqliteStoreError::InvalidWorkspaceView)?;
        check_control(control)?;
        commit_mutation(transaction)?;
        Ok(view)
    }

    pub(super) fn active_workspace_view(
        &mut self,
        connected_workspace: ConnectedWorkspaceId,
        control: WriteControl<'_>,
    ) -> Result<Option<PinnedWorkspaceView>, SqliteStoreError> {
        check_control(control)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let view = transaction
            .query_row(
                "SELECT active.workspace_view_id
                 FROM active_workspace_views AS active
                 JOIN workspace_views AS view
                   ON view.connected_workspace_id = active.connected_workspace_id
                  AND view.workspace_view_id = active.workspace_view_id
                 WHERE active.connected_workspace_id = ?1
                   AND view.lifecycle_state = 'published'",
                [connected_workspace.as_bytes().as_slice()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?
            .map(WorkspaceViewId::from_database);
        let Some(view) = view else {
            check_control(control)?;
            transaction
                .commit()
                .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            return Ok(None);
        };

        let expected_members = expected_view_member_count(&transaction, connected_workspace, view)?;
        check_control(control)?;
        let members = load_pinned_view_members(&transaction, view)?;
        validate_pinned_view_members(&members, expected_members)?;
        check_control(control)?;
        transaction
            .commit()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        Ok(Some(PinnedWorkspaceView::new(
            connected_workspace,
            view,
            members,
        )))
    }
}

fn load_source_slot_mappings(
    transaction: &Transaction<'_>,
    connected_workspace: ConnectedWorkspaceId,
) -> Result<Vec<(WorkspaceSourceSlot, i64)>, SqliteStoreError> {
    let mut statement = transaction
        .prepare(
            "SELECT source_slot_id, repository_identity, generation_workspace_id
             FROM workspace_source_slots
             WHERE connected_workspace_id = ?1
             ORDER BY source_slot_id",
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let rows = statement
        .query_map([connected_workspace.as_bytes().as_slice()], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    rows.map(|row| {
        let (source_slot, repository, generation_workspace_id) =
            row.map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let source_slot = SourceSlotId::try_from_slice(&source_slot)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let repository = RepositoryIdentityDigest::try_from_slice(&repository)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        Ok((
            WorkspaceSourceSlot::new(source_slot, repository),
            generation_workspace_id,
        ))
    })
    .collect()
}

fn eligible_generation_workspace(
    transaction: &Transaction<'_>,
    connected_workspace: ConnectedWorkspaceId,
    member: WorkspaceViewMember,
) -> Result<i64, SqliteStoreError> {
    let (generation_workspace_id, current_epoch) = transaction
        .query_row(
            "SELECT slot.generation_workspace_id, slot.source_epoch
             FROM workspace_source_slots AS slot
             WHERE slot.connected_workspace_id = ?1
               AND slot.source_slot_id = ?2",
            params![
                connected_workspace.as_bytes().as_slice(),
                member.source_slot().as_bytes().as_slice(),
            ],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?
        .ok_or(SqliteStoreError::InvalidWorkspaceView)?;
    if decode_source_slot_epoch(current_epoch)? != member.source_epoch() {
        return Err(SqliteStoreError::StaleSourceEpoch);
    }
    let eligible = transaction
        .query_row(
            "SELECT 1
             FROM source_slot_generation_receipts AS receipt
             JOIN index_generations AS generation
               ON generation.workspace_id = receipt.generation_workspace_id
              AND generation.generation_id = receipt.generation_id
             WHERE receipt.connected_workspace_id = ?1
               AND receipt.source_slot_id = ?2
               AND receipt.source_epoch = ?3
               AND receipt.generation_workspace_id = ?4
               AND receipt.generation_id = ?5
               AND generation.lifecycle_state IN ('ready', 'active', 'retained')",
            params![
                connected_workspace.as_bytes().as_slice(),
                member.source_slot().as_bytes().as_slice(),
                fixed_integer(member.source_epoch().get())?,
                generation_workspace_id,
                member.generation().get(),
            ],
            |_| Ok(()),
        )
        .optional()
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    eligible
        .map(|()| generation_workspace_id)
        .ok_or(SqliteStoreError::InvalidWorkspaceView)
}

fn source_slot_generation_workspace(
    transaction: &Transaction<'_>,
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    source_epoch: SourceSlotEpoch,
    generation: GenerationId,
) -> Result<i64, SqliteStoreError> {
    let (generation_workspace_id, current_epoch) = transaction
        .query_row(
            "SELECT generation_workspace_id, source_epoch
             FROM workspace_source_slots
             WHERE connected_workspace_id = ?1 AND source_slot_id = ?2",
            params![
                connected_workspace.as_bytes().as_slice(),
                source_slot.as_bytes().as_slice(),
            ],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?
        .ok_or(SqliteStoreError::ConnectedWorkspaceUnavailable)?;
    if decode_source_slot_epoch(current_epoch)? != source_epoch {
        return Err(SqliteStoreError::StaleSourceEpoch);
    }
    let eligible = transaction
        .query_row(
            "SELECT 1 FROM index_generations
             WHERE workspace_id = ?1
               AND generation_id = ?2
               AND lifecycle_state IN ('ready', 'active', 'retained')",
            params![generation_workspace_id, generation.get()],
            |_| Ok(()),
        )
        .optional()
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    eligible
        .map(|()| generation_workspace_id)
        .ok_or(SqliteStoreError::InvalidWorkspaceView)
}

fn insert_source_slot_receipt(
    transaction: &Transaction<'_>,
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    source_epoch: SourceSlotEpoch,
    generation_workspace_id: i64,
    generation: GenerationId,
) -> Result<(), SqliteStoreError> {
    transaction
        .execute(
            "INSERT INTO source_slot_generation_receipts(
                connected_workspace_id, source_slot_id, source_epoch,
                generation_workspace_id, generation_id
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(source_slot_id, source_epoch) DO NOTHING",
            params![
                connected_workspace.as_bytes().as_slice(),
                source_slot.as_bytes().as_slice(),
                fixed_integer(source_epoch.get())?,
                generation_workspace_id,
                generation.get(),
            ],
        )
        .map_err(|_| SqliteStoreError::InvalidWorkspaceView)?;
    let exact = transaction
        .query_row(
            "SELECT 1 FROM source_slot_generation_receipts
             WHERE connected_workspace_id = ?1
               AND source_slot_id = ?2
               AND source_epoch = ?3
               AND generation_workspace_id = ?4
               AND generation_id = ?5",
            params![
                connected_workspace.as_bytes().as_slice(),
                source_slot.as_bytes().as_slice(),
                fixed_integer(source_epoch.get())?,
                generation_workspace_id,
                generation.get(),
            ],
            |_| Ok(()),
        )
        .optional()
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    exact.ok_or(SqliteStoreError::StaleSourceEpoch)
}

fn load_source_slot_generation(
    transaction: &Transaction<'_>,
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    current_epoch: SourceSlotEpoch,
    active: bool,
) -> Result<Option<SourceSlotGeneration>, SqliteStoreError> {
    let sql = if active {
        "SELECT member.source_epoch, member.generation_id, generation.snapshot_digest
         FROM active_workspace_views AS active
         JOIN workspace_view_members AS member
           ON member.connected_workspace_id = active.connected_workspace_id
          AND member.workspace_view_id = active.workspace_view_id
         JOIN index_generations AS generation
           ON generation.workspace_id = member.generation_workspace_id
          AND generation.generation_id = member.generation_id
         WHERE active.connected_workspace_id = ?1
           AND member.source_slot_id = ?2"
    } else {
        "SELECT receipt.source_epoch, receipt.generation_id, generation.snapshot_digest
         FROM source_slot_generation_receipts AS receipt
         JOIN index_generations AS generation
           ON generation.workspace_id = receipt.generation_workspace_id
          AND generation.generation_id = receipt.generation_id
         WHERE receipt.connected_workspace_id = ?1
           AND receipt.source_slot_id = ?2
           AND receipt.source_epoch = ?3"
    };
    let row = if active {
        transaction
            .query_row(
                sql,
                params![
                    connected_workspace.as_bytes().as_slice(),
                    source_slot.as_bytes().as_slice(),
                ],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .optional()
    } else {
        transaction
            .query_row(
                sql,
                params![
                    connected_workspace.as_bytes().as_slice(),
                    source_slot.as_bytes().as_slice(),
                    fixed_integer(current_epoch.get())?,
                ],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .optional()
    }
    .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    row.map(|(source_epoch, generation, snapshot)| {
        if generation <= 0 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        let snapshot = SourceSnapshotDigest::try_from_slice(&snapshot)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        Ok(SourceSlotGeneration::new(
            decode_source_slot_epoch(source_epoch)?,
            GenerationId::from_database(generation),
            snapshot,
        ))
    })
    .transpose()
}

fn decode_source_slot_epoch(value: i64) -> Result<SourceSlotEpoch, SqliteStoreError> {
    u64::try_from(value)
        .ok()
        .and_then(|value| SourceSlotEpoch::try_new(value).ok())
        .ok_or(SqliteStoreError::IntegrityCheckFailed)
}

fn is_default_source_slot(
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    repository: RepositoryIdentityDigest,
) -> bool {
    connected_workspace == ConnectedWorkspaceId::for_single_repository(repository)
        && source_slot == SourceSlotId::for_repository(repository)
}

include!("workspace/membership.rs");
include!("workspace/view_storage.rs");
