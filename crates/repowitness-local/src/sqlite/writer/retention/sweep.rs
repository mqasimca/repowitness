fn load_applied_retention(
    transaction: &Transaction<'_>,
    policy_digest: RetentionPolicyDigest,
    plan_digest: RetentionPlanDigest,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Option<RetentionApplyOutcome>, SqliteStoreError> {
    let stored = transaction
        .query_row(
            "SELECT collection_id, generation_count, workspace_view_count,
                    source_slot_receipt_count, snapshot_count, artifact_count,
                    deleted_row_count, estimated_deleted_bytes, more_work
             FROM retention_collection_audit
             WHERE policy_digest = ?1 AND plan_digest = ?2",
            params![
                policy_digest.as_bytes().as_slice(),
                plan_digest.as_bytes().as_slice()
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, bool>(8)?,
                ))
            },
        )
        .optional()
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    stored
        .map(
            |(
                collection,
                generations,
                views,
                receipts,
                snapshots,
                artifacts,
                rows,
                bytes,
                more_work,
            )| {
                Ok(RetentionApplyOutcome::new(
                    positive_database_count(collection)?,
                    positive_database_count(generations)?,
                    positive_database_count(views)?,
                    positive_database_count(receipts)?,
                    positive_database_count(snapshots)?,
                    positive_database_count(artifacts)?,
                    positive_database_count(rows)?,
                    positive_database_count(bytes)?,
                    more_work,
                ))
            },
        )
        .transpose()
}

fn sweep_retention_plan(
    transaction: &Transaction<'_>,
    policy: &GenerationRetentionPolicy,
    plan: &RetentionPlan,
    budget: &mut RetentionWorkBudget,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<RetentionApplyOutcome, SqliteStoreError> {
    check_retention_control(cancelled, deadline)?;
    ensure_retention_marks_empty(transaction, cancelled, deadline)?;
    let plan_digest = plan.plan_digest();
    for generation in plan.candidate_generations() {
        check_retention_control(cancelled, deadline)?;
        transaction
            .execute(
                "INSERT INTO retention_generation_garbage(
                    generation_id, plan_digest, lifecycle_state
                 ) VALUES (?1, ?2, 'garbage')",
                params![generation.get(), plan_digest.as_bytes().as_slice(),],
            )
            .map_err(|error| retention_sweep_error(error, cancelled, deadline))?;
        budget.consume(1)?;
    }
    mark_retention_dependents(transaction, plan_digest, budget, cancelled, deadline)?;
    let generation_count =
        count_retention_marks(transaction, "retention_generation_garbage", plan_digest)?;
    let workspace_view_count =
        count_retention_marks(transaction, "retention_workspace_view_garbage", plan_digest)?;
    let source_slot_receipt_count = count_retention_marks(
        transaction,
        "retention_source_slot_receipt_garbage",
        plan_digest,
    )?;
    let snapshot_count =
        count_retention_marks(transaction, "retention_snapshot_garbage", plan_digest)?;
    let artifact_count =
        count_retention_marks(transaction, "retention_artifact_garbage", plan_digest)?;
    if generation_count
        != u64::try_from(plan.candidate_generations().len())
            .map_err(|_| SqliteStoreError::CountNotRepresentable)?
    {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }

    let mut deleted_rows = 0_u64;
    if generation_count != 0 {
        for sql in RETENTION_DELETE_ORDER {
            check_retention_control(cancelled, deadline)?;
            let deleted = transaction
                .execute(sql, [plan_digest.as_bytes().as_slice()])
                .map_err(|error| retention_sweep_error(error, cancelled, deadline))?;
            deleted_rows = deleted_rows
                .checked_add(
                    u64::try_from(deleted).map_err(|_| SqliteStoreError::CountNotRepresentable)?,
                )
                .ok_or(SqliteStoreError::CountNotRepresentable)?;
            budget.consume(
                u64::try_from(deleted).map_err(|_| SqliteStoreError::CountNotRepresentable)?,
            )?;
        }
        verify_retention_marks_consumed(transaction, plan_digest)?;
    }
    check_retention_control(cancelled, deadline)?;
    let collection_id = next_retention_collection_id(transaction)?;
    let outcome = if generation_count == 0 {
        "no_op"
    } else {
        "applied"
    };
    transaction
        .execute(
            "INSERT INTO retention_collection_audit(
                collection_id, policy_digest, plan_digest, generation_count,
                workspace_view_count, source_slot_receipt_count,
                snapshot_count, artifact_count, deleted_row_count,
                estimated_deleted_bytes, more_work, outcome
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12
             )",
            params![
                fixed_integer(collection_id)?,
                policy.digest().as_bytes().as_slice(),
                plan_digest.as_bytes().as_slice(),
                fixed_integer(generation_count)?,
                fixed_integer(workspace_view_count)?,
                fixed_integer(source_slot_receipt_count)?,
                fixed_integer(snapshot_count)?,
                fixed_integer(artifact_count)?,
                fixed_integer(deleted_rows)?,
                fixed_integer(plan.estimated_bytes())?,
                i64::from(plan.more_work()),
                outcome,
            ],
        )
        .map_err(|error| retention_sweep_error(error, cancelled, deadline))?;
    budget.consume(1)?;
    Ok(RetentionApplyOutcome::new(
        collection_id,
        generation_count,
        workspace_view_count,
        source_slot_receipt_count,
        snapshot_count,
        artifact_count,
        deleted_rows,
        plan.estimated_bytes(),
        plan.more_work(),
    ))
}

fn ensure_retention_marks_empty(
    transaction: &Transaction<'_>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SqliteStoreError> {
    let marks_exist = transaction
        .query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM retention_generation_garbage
                 UNION ALL
                 SELECT 1 FROM retention_snapshot_garbage
                 UNION ALL
                 SELECT 1 FROM retention_artifact_garbage
                 UNION ALL
                 SELECT 1 FROM retention_workspace_view_garbage
                 UNION ALL
                 SELECT 1 FROM retention_source_slot_receipt_garbage
                 UNION ALL
                 SELECT 1 FROM retention_scip_overlay_garbage
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    if marks_exist {
        Err(SqliteStoreError::IntegrityCheckFailed)
    } else {
        Ok(())
    }
}

fn mark_retention_dependents(
    transaction: &Transaction<'_>,
    plan_digest: RetentionPlanDigest,
    budget: &mut RetentionWorkBudget,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SqliteStoreError> {
    for sql in RETENTION_MARK_DEPENDENTS {
        check_retention_control(cancelled, deadline)?;
        let inserted = transaction
            .execute(sql, [plan_digest.as_bytes().as_slice()])
            .map_err(|error| retention_sweep_error(error, cancelled, deadline))?;
        budget.consume(
            u64::try_from(inserted).map_err(|_| SqliteStoreError::CountNotRepresentable)?,
        )?;
    }
    Ok(())
}

fn count_retention_marks(
    transaction: &Transaction<'_>,
    relation: &str,
    plan_digest: RetentionPlanDigest,
) -> Result<u64, SqliteStoreError> {
    let sql = format!("SELECT count(*) FROM {relation} WHERE plan_digest = ?1");
    let count = transaction
        .query_row(&sql, [plan_digest.as_bytes().as_slice()], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    positive_database_count(count)
}

fn verify_retention_marks_consumed(
    transaction: &Transaction<'_>,
    plan_digest: RetentionPlanDigest,
) -> Result<(), SqliteStoreError> {
    for relation in [
        "retention_generation_garbage",
        "retention_snapshot_garbage",
        "retention_artifact_garbage",
        "retention_workspace_view_garbage",
        "retention_source_slot_receipt_garbage",
        "retention_scip_overlay_garbage",
    ] {
        if count_retention_marks(transaction, relation, plan_digest)? != 0 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
    }
    Ok(())
}

fn next_retention_collection_id(transaction: &Transaction<'_>) -> Result<u64, SqliteStoreError> {
    let current = transaction
        .query_row(
            "SELECT COALESCE(max(collection_id), 0)
             FROM retention_collection_audit",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    positive_database_count(current)?
        .checked_add(1)
        .ok_or(SqliteStoreError::CountNotRepresentable)
}

fn retention_sweep_error(
    error: rusqlite::Error,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> SqliteStoreError {
    if matches!(
        error,
        rusqlite::Error::SqliteFailure(code, _) if code.code == ErrorCode::ConstraintViolation
    ) {
        SqliteStoreError::IntegrityCheckFailed
    } else {
        retention_database_error(error, cancelled, deadline)
    }
}

const RETENTION_MARK_DEPENDENTS: &[&str] = &[
    "INSERT INTO retention_scip_overlay_garbage(
         overlay_digest, plan_digest, lifecycle_state
     )
     SELECT receipt.overlay_digest, ?1, 'garbage'
     FROM scip_overlay_receipts AS receipt
     JOIN retention_generation_garbage AS garbage
       ON garbage.generation_id = receipt.generation_id
      AND garbage.plan_digest = ?1
     WHERE receipt.lifecycle_state = 'complete'",
    "INSERT INTO retention_workspace_view_garbage(
         workspace_view_id, plan_digest, lifecycle_state
     )
     SELECT DISTINCT view.workspace_view_id, ?1, 'garbage'
     FROM workspace_views AS view
     JOIN workspace_view_members AS member
       ON member.workspace_view_id = view.workspace_view_id
     JOIN retention_generation_garbage AS garbage
       ON garbage.generation_id = member.generation_id
      AND garbage.plan_digest = ?1
     WHERE view.lifecycle_state = 'published'
       AND NOT EXISTS (
           SELECT 1 FROM active_workspace_views AS active
           WHERE active.workspace_view_id = view.workspace_view_id
       )",
    "INSERT INTO retention_source_slot_receipt_garbage(
         source_slot_id, source_epoch, plan_digest, lifecycle_state
     )
     SELECT receipt.source_slot_id, receipt.source_epoch, ?1, 'garbage'
     FROM source_slot_generation_receipts AS receipt
     JOIN retention_generation_garbage AS garbage
       ON garbage.generation_id = receipt.generation_id
      AND garbage.plan_digest = ?1
     WHERE NOT EXISTS (
         SELECT 1 FROM workspace_source_slots AS slot
         WHERE slot.connected_workspace_id = receipt.connected_workspace_id
           AND slot.source_slot_id = receipt.source_slot_id
           AND slot.source_epoch = receipt.source_epoch
     )",
    "INSERT INTO retention_snapshot_garbage(
         snapshot_digest, plan_digest, lifecycle_state
     )
     SELECT DISTINCT generation.snapshot_digest, ?1, 'garbage'
     FROM index_generations AS generation
     JOIN retention_generation_garbage AS garbage
       ON garbage.generation_id = generation.generation_id
      AND garbage.plan_digest = ?1
     WHERE NOT EXISTS (
         SELECT 1 FROM index_generations AS remaining
         WHERE remaining.snapshot_digest = generation.snapshot_digest
           AND NOT EXISTS (
               SELECT 1 FROM retention_generation_garbage AS marked
               WHERE marked.generation_id = remaining.generation_id
                 AND marked.plan_digest = ?1
           )
     )
       AND NOT EXISTS (
           SELECT 1 FROM memory_projection_generations
           WHERE snapshot_digest = generation.snapshot_digest
       )
       AND NOT EXISTS (
           SELECT 1 FROM memory_versions_all
           WHERE validity_source_snapshot = generation.snapshot_digest
       )
       AND NOT EXISTS (
           SELECT 1 FROM memory_evidence
           WHERE source_snapshot_digest = generation.snapshot_digest
       )
       AND NOT EXISTS (
           SELECT 1 FROM memory_audit_all
           WHERE source_format = 'source_snapshot'
             AND source_revision = generation.snapshot_digest
       )
       AND NOT EXISTS (
           SELECT 1 FROM memory_correspondence_audit
           WHERE source_snapshot_digest = generation.snapshot_digest
              OR target_snapshot_digest = generation.snapshot_digest
       )
       AND NOT EXISTS (
           SELECT 1 FROM memory_projection_evidence
           WHERE target_snapshot_digest = generation.snapshot_digest
       )
       AND NOT EXISTS (
           SELECT 1 FROM memory_projection_candidates
           WHERE target_snapshot_digest = generation.snapshot_digest
       )",
    "INSERT INTO retention_artifact_garbage(
         artifact_digest, plan_digest, lifecycle_state
     )
     WITH candidate_artifacts(artifact_digest) AS (
         SELECT file.artifact_digest
         FROM generation_files AS file
         JOIN retention_generation_garbage AS garbage
           ON garbage.generation_id = file.generation_id
          AND garbage.plan_digest = ?1
         UNION
         SELECT artifact.graph_artifact_digest
         FROM generation_graph_artifacts AS artifact
         JOIN retention_generation_garbage AS garbage
           ON garbage.generation_id = artifact.generation_id
          AND garbage.plan_digest = ?1
         UNION
         SELECT definition.artifact_digest
         FROM generation_graph_definitions AS definition
         JOIN retention_generation_garbage AS garbage
           ON garbage.generation_id = definition.generation_id
          AND garbage.plan_digest = ?1
         UNION
         SELECT resolution.site_artifact_digest
         FROM generation_graph_resolutions AS resolution
         JOIN retention_generation_garbage AS garbage
           ON garbage.generation_id = resolution.generation_id
          AND garbage.plan_digest = ?1
         UNION
         SELECT syntax_site_artifact.syntax_site_artifact_digest
         FROM generation_syntax_site_artifacts AS syntax_site_artifact
         JOIN retention_generation_garbage AS garbage
           ON garbage.generation_id = syntax_site_artifact.generation_id
          AND garbage.plan_digest = ?1
     )
     SELECT candidate.artifact_digest, ?1, 'garbage'
     FROM candidate_artifacts AS candidate
     JOIN analysis_artifacts AS artifact
       ON artifact.artifact_digest = candidate.artifact_digest
     WHERE artifact.lifecycle_state = 'complete'
       AND NOT EXISTS (
           SELECT 1 FROM generation_files AS file
           WHERE file.artifact_digest = candidate.artifact_digest
             AND NOT EXISTS (
                 SELECT 1 FROM retention_generation_garbage AS marked
                 WHERE marked.generation_id = file.generation_id
                   AND marked.plan_digest = ?1
             )
       )
       AND NOT EXISTS (
           SELECT 1 FROM generation_graph_artifacts AS graph_artifact
           WHERE graph_artifact.graph_artifact_digest = candidate.artifact_digest
             AND NOT EXISTS (
                 SELECT 1 FROM retention_generation_garbage AS marked
                 WHERE marked.generation_id = graph_artifact.generation_id
                   AND marked.plan_digest = ?1
             )
       )
       AND NOT EXISTS (
           SELECT 1 FROM generation_graph_definitions AS definition
           WHERE definition.artifact_digest = candidate.artifact_digest
             AND NOT EXISTS (
                 SELECT 1 FROM retention_generation_garbage AS marked
                 WHERE marked.generation_id = definition.generation_id
                   AND marked.plan_digest = ?1
             )
       )
       AND NOT EXISTS (
           SELECT 1 FROM generation_graph_resolutions AS resolution
           WHERE resolution.site_artifact_digest = candidate.artifact_digest
             AND NOT EXISTS (
                 SELECT 1 FROM retention_generation_garbage AS marked
                 WHERE marked.generation_id = resolution.generation_id
                 AND marked.plan_digest = ?1
             )
       )
       AND NOT EXISTS (
           SELECT 1 FROM generation_syntax_site_artifacts AS syntax_site_artifact
           WHERE syntax_site_artifact.syntax_site_artifact_digest = candidate.artifact_digest
             AND NOT EXISTS (
                 SELECT 1 FROM retention_generation_garbage AS marked
                 WHERE marked.generation_id = syntax_site_artifact.generation_id
                   AND marked.plan_digest = ?1
             )
       )
       AND NOT EXISTS (
           SELECT 1 FROM memory_evidence
           WHERE artifact_digest = candidate.artifact_digest
       )
       AND NOT EXISTS (
           SELECT 1 FROM memory_correspondence_audit
           WHERE source_artifact_digest = candidate.artifact_digest
              OR target_artifact_digest = candidate.artifact_digest
       )
       AND NOT EXISTS (
           SELECT 1 FROM memory_projection_evidence
           WHERE target_artifact_digest = candidate.artifact_digest
       )
       AND NOT EXISTS (
           SELECT 1 FROM memory_projection_candidates
           WHERE target_artifact_digest = candidate.artifact_digest
       )",
];

const RETENTION_DELETE_ORDER: &[&str] = &[
    "DELETE FROM active_scip_overlays
     WHERE overlay_digest IN (
         SELECT overlay_digest FROM retention_scip_overlay_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM scip_enclosed_reference_edges
     WHERE overlay_digest IN (
         SELECT overlay_digest FROM retention_scip_overlay_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM scip_overlay_occurrences
     WHERE overlay_digest IN (
         SELECT overlay_digest FROM retention_scip_overlay_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM scip_overlay_relationships
     WHERE overlay_digest IN (
         SELECT overlay_digest FROM retention_scip_overlay_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM scip_overlay_documents
     WHERE overlay_digest IN (
         SELECT overlay_digest FROM retention_scip_overlay_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM scip_overlay_receipts
     WHERE overlay_digest IN (
         SELECT overlay_digest FROM retention_scip_overlay_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM workspace_view_members
     WHERE workspace_view_id IN (
         SELECT workspace_view_id FROM retention_workspace_view_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM workspace_views
     WHERE workspace_view_id IN (
         SELECT workspace_view_id FROM retention_workspace_view_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM source_slot_generation_receipts
     WHERE (source_slot_id, source_epoch) IN (
         SELECT source_slot_id, source_epoch
         FROM retention_source_slot_receipt_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_graph_edges
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_graph_candidates
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_graph_resolutions
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_graph_definitions
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_graph_artifacts
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_graph_sources
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_graph_publications
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_graph_requirements
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_syntax_site_artifacts
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_syntax_site_publications
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_syntax_site_requirements
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_repository_topology_entries
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_repository_topology_publications
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_repository_topology_requirements
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_search
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_search_rebuild
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_facts
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM generation_files
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM index_generations
     WHERE generation_id IN (
         SELECT generation_id FROM retention_generation_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM rust_graph_sites
     WHERE artifact_digest IN (
         SELECT artifact_digest FROM retention_artifact_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM rust_graph_artifacts
     WHERE artifact_digest IN (
         SELECT artifact_digest FROM retention_artifact_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM syntax_sites
     WHERE artifact_digest IN (
         SELECT artifact_digest FROM retention_artifact_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM syntax_site_artifacts
     WHERE artifact_digest IN (
         SELECT artifact_digest FROM retention_artifact_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM artifact_fact_correspondence
     WHERE artifact_digest IN (
         SELECT artifact_digest FROM retention_artifact_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM artifact_facts
     WHERE artifact_digest IN (
         SELECT artifact_digest FROM retention_artifact_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM analysis_artifacts
     WHERE artifact_digest IN (
         SELECT artifact_digest FROM retention_artifact_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM source_manifest_entries
     WHERE snapshot_digest IN (
         SELECT snapshot_digest FROM retention_snapshot_garbage
         WHERE plan_digest = ?1
     )",
    "DELETE FROM source_snapshots
     WHERE snapshot_digest IN (
         SELECT snapshot_digest FROM retention_snapshot_garbage
         WHERE plan_digest = ?1
     )",
];
