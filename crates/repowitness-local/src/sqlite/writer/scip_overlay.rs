use repowitness_domain::ScipOverlayDigest;

use crate::sqlite::PreparedScipOverlay;

struct OverlayScope {
    source_epoch: i64,
    generation_workspace_id: i64,
    generation_id: i64,
    source_snapshot: [u8; 32],
    source_manifest: [u8; 32],
}

impl WriterState {
    pub(super) fn stage_scip_overlay(
        &mut self,
        connected_workspace: ConnectedWorkspaceId,
        workspace_view: WorkspaceViewId,
        source_slot: SourceSlotId,
        prepared: &PreparedScipOverlay,
        require_active_view: bool,
        control: WriteControl<'_>,
    ) -> Result<ScipOverlayDigest, SqliteStoreError> {
        check_control(control)?;
        let transaction = self.transaction()?;
        if require_active_view
            && !workspace_view_is_active(&transaction, connected_workspace, workspace_view)?
        {
            return Err(SqliteStoreError::InvalidWorkspaceView);
        }
        let scope = load_overlay_scope(
            &transaction,
            connected_workspace,
            workspace_view,
            source_slot,
        )?;
        validate_overlay_identity(
            prepared,
            &scope,
            connected_workspace,
            workspace_view,
            source_slot,
        )?;
        if overlay_is_complete(&transaction, prepared.digest())? {
            activate_overlay(
                &transaction,
                connected_workspace,
                workspace_view,
                source_slot,
                prepared.digest(),
            )?;
            check_control(control)?;
            commit_mutation(transaction)?;
            return Ok(prepared.digest());
        }
        insert_overlay_receipt(
            &transaction,
            connected_workspace,
            workspace_view,
            source_slot,
            prepared,
            &scope,
        )?;
        stage_overlay_documents(&transaction, prepared, control)?;
        stage_enclosed_reference_edges(&transaction, prepared.digest(), &scope, control)?;
        check_control(control)?;
        let completed = transaction
            .execute(
                "UPDATE scip_overlay_receipts
                 SET lifecycle_state = 'complete'
                 WHERE overlay_digest = ?1 AND lifecycle_state = 'staging'",
                [prepared.digest().as_bytes().as_slice()],
            )
            .map_err(|_| SqliteStoreError::InvalidScipOverlay)?;
        if completed != 1 {
            return Err(SqliteStoreError::InvalidScipOverlay);
        }
        activate_overlay(
            &transaction,
            connected_workspace,
            workspace_view,
            source_slot,
            prepared.digest(),
        )?;
        check_control(control)?;
        commit_mutation(transaction)?;
        Ok(prepared.digest())
    }
}

/// Projects exact SCIP reference occurrences into bounded caller/callee edges.
///
/// SCIP gives us the referenced target and the definition occurrence, while
/// the source index gives us the enclosing function/method span. Keeping this
/// projection separate preserves the distinction between producer-declared
/// relationships and RepoWitness-derived call-site evidence.
fn stage_enclosed_reference_edges(
    transaction: &Transaction<'_>,
    digest: ScipOverlayDigest,
    scope: &OverlayScope,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    check_control(control)?;
    transaction
        .execute(
            "INSERT INTO scip_enclosed_reference_edges(
                overlay_digest, document_ordinal, relationship_ordinal,
                source_symbol, target_symbol, kinds
             )
             SELECT ?1, occurrence.document_ordinal,
                    ROW_NUMBER() OVER (
                        PARTITION BY occurrence.document_ordinal
                        ORDER BY definition.occurrence_ordinal, occurrence.occurrence_ordinal
                    ) - 1,
                    definition.symbol, occurrence.symbol, 1
             FROM scip_overlay_occurrences AS occurrence
             JOIN scip_overlay_documents AS document
               ON document.overlay_digest = occurrence.overlay_digest
              AND document.document_ordinal = occurrence.document_ordinal
             JOIN generation_files AS file
               ON file.generation_id = ?2
              AND file.repository_path = document.repository_path
              AND file.content_digest = document.content_digest
             JOIN artifact_facts AS fact
               ON fact.artifact_digest = file.artifact_digest
              AND fact.kind IN ('function', 'method')
             JOIN scip_overlay_occurrences AS definition
               ON definition.overlay_digest = occurrence.overlay_digest
              AND definition.document_ordinal = occurrence.document_ordinal
              AND definition.roles & 1 != 0
               AND definition.start_byte >= fact.name_start
               AND definition.end_byte <= fact.name_end
               AND NOT EXISTS (
                   SELECT 1
                   FROM artifact_facts AS nested
                   WHERE nested.artifact_digest = fact.artifact_digest
                     AND nested.kind IN ('function', 'method')
                     AND nested.declaration_start >= fact.declaration_start
                     AND nested.declaration_end <= fact.declaration_end
                     AND (nested.declaration_start > fact.declaration_start
                          OR nested.declaration_end < fact.declaration_end)
                     AND occurrence.start_byte >= nested.declaration_start
                     AND occurrence.end_byte <= nested.declaration_end
               )
             WHERE occurrence.overlay_digest = ?1
               AND occurrence.symbol IS NOT NULL
               AND occurrence.roles & 1 = 0
               AND occurrence.start_byte >= fact.declaration_start
               AND occurrence.end_byte <= fact.declaration_end
               AND definition.symbol IS NOT NULL
               AND NOT EXISTS (
                   SELECT 1
                   FROM scip_overlay_relationships AS declared
                   WHERE declared.overlay_digest = ?1
                     AND declared.document_ordinal = occurrence.document_ordinal
                     AND declared.source_symbol = definition.symbol
                     AND declared.target_symbol = occurrence.symbol
               )
             GROUP BY occurrence.document_ordinal, definition.occurrence_ordinal,
                      occurrence.occurrence_ordinal, definition.symbol, occurrence.symbol",
            rusqlite::params![digest.as_bytes().as_slice(), scope.generation_id],
        )
        .map_err(|_| SqliteStoreError::InvalidScipOverlay)?;
    Ok(())
}

fn workspace_view_is_active(
    transaction: &Transaction<'_>,
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: WorkspaceViewId,
) -> Result<bool, SqliteStoreError> {
    transaction
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM active_workspace_views
                WHERE connected_workspace_id = ?1 AND workspace_view_id = ?2
            )",
            params![
                connected_workspace.as_bytes().as_slice(),
                workspace_view.get(),
            ],
            |row| row.get::<_, i64>(0),
        )
        .map(|value| value == 1)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
}

fn load_overlay_scope(
    transaction: &Transaction<'_>,
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: WorkspaceViewId,
    source_slot: SourceSlotId,
) -> Result<OverlayScope, SqliteStoreError> {
    transaction
        .query_row(
            "SELECT member.source_epoch, member.generation_workspace_id,
                    member.generation_id, generation.snapshot_digest,
                    snapshot.manifest_digest
             FROM workspace_views AS view
             JOIN workspace_view_members AS member
               ON member.workspace_view_id = view.workspace_view_id
             JOIN index_generations AS generation
               ON generation.workspace_id = member.generation_workspace_id
              AND generation.generation_id = member.generation_id
             JOIN source_snapshots AS snapshot
               ON snapshot.snapshot_digest = generation.snapshot_digest
             WHERE view.connected_workspace_id = ?1
               AND view.workspace_view_id = ?2
               AND view.lifecycle_state = 'published'
               AND member.source_slot_id = ?3",
            params![
                connected_workspace.as_bytes().as_slice(),
                workspace_view.get(),
                source_slot.as_bytes().as_slice(),
            ],
            |row| {
                Ok(OverlayScope {
                    source_epoch: row.get(0)?,
                    generation_workspace_id: row.get(1)?,
                    generation_id: row.get(2)?,
                    source_snapshot: row.get(3)?,
                    source_manifest: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?
        .ok_or(SqliteStoreError::InvalidWorkspaceView)
}

fn validate_overlay_identity(
    prepared: &PreparedScipOverlay,
    scope: &OverlayScope,
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: WorkspaceViewId,
    source_slot: SourceSlotId,
) -> Result<(), SqliteStoreError> {
    let identity = prepared.identity();
    let overlay_scope = identity.scope();
    if overlay_scope.connected_workspace() != connected_workspace
        || overlay_scope.workspace_view() != workspace_view.get()
        || overlay_scope.source_slot() != source_slot
        || overlay_scope.source_epoch().get()
            != u64::try_from(scope.source_epoch).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?
        || overlay_scope.generation() != scope.generation_id
        || identity.source_snapshot().as_bytes() != &scope.source_snapshot
        || identity.source_manifest().as_bytes() != &scope.source_manifest
    {
        return Err(SqliteStoreError::PreparedIdentityMismatch);
    }
    Ok(())
}

fn overlay_is_complete(
    transaction: &Transaction<'_>,
    digest: ScipOverlayDigest,
) -> Result<bool, SqliteStoreError> {
    let lifecycle: Option<String> = transaction
        .query_row(
            "SELECT lifecycle_state FROM scip_overlay_receipts WHERE overlay_digest = ?1",
            [digest.as_bytes().as_slice()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    match lifecycle.as_deref() {
        None => Ok(false),
        Some("complete") => Ok(true),
        Some("staging") | Some(_) => Err(SqliteStoreError::InvalidScipOverlay),
    }
}

fn insert_overlay_receipt(
    transaction: &Transaction<'_>,
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: WorkspaceViewId,
    source_slot: SourceSlotId,
    prepared: &PreparedScipOverlay,
    scope: &OverlayScope,
) -> Result<(), SqliteStoreError> {
    let identity = prepared.identity();
    let changed = transaction
        .execute(
            "INSERT INTO scip_overlay_receipts(
                overlay_digest, connected_workspace_id, workspace_view_id,
                source_slot_id, source_epoch, generation_workspace_id, generation_id,
                source_snapshot_digest, source_manifest_digest, configuration_digest,
                producer_digest, schema_digest, importer_digest, input_digest,
                lifecycle_state, document_count, occurrence_count, relationship_count
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                'staging', ?15, ?16, ?17
             )",
            params![
                prepared.digest().as_bytes().as_slice(),
                connected_workspace.as_bytes().as_slice(),
                workspace_view.get(),
                source_slot.as_bytes().as_slice(),
                scope.source_epoch,
                scope.generation_workspace_id,
                scope.generation_id,
                identity.source_snapshot().as_bytes().as_slice(),
                identity.source_manifest().as_bytes().as_slice(),
                identity.configuration().as_bytes().as_slice(),
                identity.producer().as_bytes().as_slice(),
                identity.schema().as_bytes().as_slice(),
                identity.importer().as_bytes().as_slice(),
                identity.input().as_bytes().as_slice(),
                i64::try_from(prepared.documents().len())
                    .map_err(|_| SqliteStoreError::CountNotRepresentable)?,
                fixed_integer(prepared.occurrence_count())?,
                fixed_integer(prepared.relationship_count())?,
            ],
        )
        .map_err(|_| SqliteStoreError::InvalidScipOverlay)?;
    if changed != 1 {
        return Err(SqliteStoreError::InvalidScipOverlay);
    }
    Ok(())
}

fn stage_overlay_documents(
    transaction: &Transaction<'_>,
    prepared: &PreparedScipOverlay,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    let mut insert = transaction
        .prepare(
            "INSERT INTO scip_overlay_documents(
                overlay_digest, document_ordinal, repository_path, content_digest,
                occurrence_count, relationship_count
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .map_err(|_| SqliteStoreError::InvalidScipOverlay)?;
    for (document_ordinal, document) in prepared.documents().iter().enumerate() {
        if document_ordinal % WRITE_BATCH_ROWS == 0 {
            check_control(control)?;
        }
        let document_ordinal =
            i64::try_from(document_ordinal).map_err(|_| SqliteStoreError::CountNotRepresentable)?;
        insert
            .execute(params![
                prepared.digest().as_bytes().as_slice(),
                document_ordinal,
                document.path().as_bytes(),
                document.content().as_bytes().as_slice(),
                i64::try_from(document.occurrences().len())
                    .map_err(|_| SqliteStoreError::CountNotRepresentable)?,
                i64::try_from(document.relationships().len())
                    .map_err(|_| SqliteStoreError::CountNotRepresentable)?,
            ])
            .map_err(|_| SqliteStoreError::InvalidScipOverlay)?;
        stage_document_occurrences(transaction, prepared.digest(), document_ordinal, document, control)?;
        stage_document_relationships(transaction, prepared.digest(), document_ordinal, document, control)?;
    }
    Ok(())
}

fn stage_document_occurrences(
    transaction: &Transaction<'_>,
    digest: ScipOverlayDigest,
    document_ordinal: i64,
    document: &repowitness_analysis::ScipOverlayDocument,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    let mut insert = transaction
        .prepare(
            "INSERT INTO scip_overlay_occurrences(
                overlay_digest, document_ordinal, occurrence_ordinal,
                symbol, roles, start_byte, end_byte
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .map_err(|_| SqliteStoreError::InvalidScipOverlay)?;
    for occurrence in document.occurrences() {
        if usize::try_from(occurrence.ordinal())
            .map_err(|_| SqliteStoreError::CountNotRepresentable)?
            % WRITE_BATCH_ROWS
            == 0
        {
            check_control(control)?;
        }
        let symbol = occurrence.symbol().map(|value| value.as_str().as_bytes());
        insert
            .execute(params![
                digest.as_bytes().as_slice(),
                document_ordinal,
                i64::from(occurrence.ordinal()),
                symbol,
                i64::from(occurrence.roles().bits()),
                fixed_integer(occurrence.span().start().get())?,
                fixed_integer(occurrence.span().end().get())?,
            ])
            .map_err(|_| SqliteStoreError::InvalidScipOverlay)?;
    }
    Ok(())
}

fn stage_document_relationships(
    transaction: &Transaction<'_>,
    digest: ScipOverlayDigest,
    document_ordinal: i64,
    document: &repowitness_analysis::ScipOverlayDocument,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    let mut insert = transaction
        .prepare(
            "INSERT INTO scip_overlay_relationships(
                overlay_digest, document_ordinal, relationship_ordinal,
                source_symbol, target_symbol, kinds
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .map_err(|_| SqliteStoreError::InvalidScipOverlay)?;
    for (ordinal, relationship) in document.relationships().iter().enumerate() {
        if ordinal % WRITE_BATCH_ROWS == 0 {
            check_control(control)?;
        }
        insert
            .execute(params![
                digest.as_bytes().as_slice(),
                document_ordinal,
                i64::try_from(ordinal).map_err(|_| SqliteStoreError::CountNotRepresentable)?,
                relationship.source().as_str().as_bytes(),
                relationship.target().as_str().as_bytes(),
                i64::from(relationship_kind_bits(relationship.kinds())),
            ])
            .map_err(|_| SqliteStoreError::InvalidScipOverlay)?;
    }
    Ok(())
}

fn relationship_kind_bits(kinds: repowitness_domain::ScipRelationshipKinds) -> u8 {
    (u8::from(kinds.is_reference()))
        | (u8::from(kinds.is_implementation()) << 1)
        | (u8::from(kinds.is_type_definition()) << 2)
        | (u8::from(kinds.is_definition()) << 3)
}

fn activate_overlay(
    transaction: &Transaction<'_>,
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: WorkspaceViewId,
    source_slot: SourceSlotId,
    digest: ScipOverlayDigest,
) -> Result<(), SqliteStoreError> {
    transaction
        .execute(
            "INSERT INTO active_scip_overlays(
                connected_workspace_id, source_slot_id, workspace_view_id, overlay_digest
             ) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(connected_workspace_id, source_slot_id)
             DO UPDATE SET workspace_view_id = excluded.workspace_view_id,
                           overlay_digest = excluded.overlay_digest",
            params![
                connected_workspace.as_bytes().as_slice(),
                source_slot.as_bytes().as_slice(),
                workspace_view.get(),
                digest.as_bytes().as_slice(),
            ],
        )
        .map_err(|_| SqliteStoreError::InvalidScipOverlay)?;
    Ok(())
}
