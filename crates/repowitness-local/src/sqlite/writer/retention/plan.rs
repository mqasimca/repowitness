const RETENTION_PLAN_DIGEST_DOMAIN: &[u8] = b"RepoWitness\0phase1-generation-retention-plan\0";
const ESTIMATED_ROW_BYTES: u64 = 256;

#[derive(Clone)]
struct EligibleRetentionGeneration {
    generation: GenerationId,
    sort_source_slot: [u8; 32],
    source_epoch: i64,
}

struct RetentionCandidateSelection {
    candidates: Vec<GenerationId>,
    estimated_rows: u64,
    estimated_bytes: u64,
    more_work: bool,
}

pub(crate) fn build_retention_plan(
    transaction: &Transaction<'_>,
    policy: &GenerationRetentionPolicy,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<RetentionPlan, SqliteStoreError> {
    build_retention_plan_with_budget(transaction, policy, cancelled, deadline)
        .map(|(plan, _budget)| plan)
}

pub(crate) fn build_retention_plan_with_budget(
    transaction: &Transaction<'_>,
    policy: &GenerationRetentionPolicy,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(RetentionPlan, RetentionWorkBudget), SqliteStoreError> {
    check_retention_control(cancelled, deadline)?;
    let mut budget = RetentionWorkBudget::new(policy.limits().max_rows());
    let mut root_count = 0_u64;
    let pinned_generations = validate_retention_pins(
        transaction,
        policy,
        &mut budget,
        &mut root_count,
        cancelled,
        deadline,
    )?;
    let mut hasher = Sha256::new();
    hasher.update(RETENTION_PLAN_DIGEST_DOMAIN);
    hasher.update(policy.digest().as_bytes());
    hash_retention_roots(
        transaction,
        policy,
        &mut hasher,
        &mut budget,
        &mut root_count,
        cancelled,
        deadline,
    )?;
    let (eligible, eligible_truncated) = eligible_retention_generations(
        transaction,
        policy,
        &pinned_generations,
        &mut budget,
        cancelled,
        deadline,
    )?;
    hasher.update(
        u64::try_from(eligible.len())
            .map_err(|_| SqliteStoreError::CountNotRepresentable)?
            .to_be_bytes(),
    );
    for candidate in &eligible {
        hasher.update(candidate.generation.get().to_be_bytes());
        hasher.update(candidate.sort_source_slot);
        hasher.update(candidate.source_epoch.to_be_bytes());
    }
    hasher.update([u8::from(eligible_truncated)]);

    let selection = select_retention_candidates(
        transaction,
        policy,
        &eligible,
        eligible_truncated,
        &mut budget,
        cancelled,
        deadline,
    )?;
    budget.reserve(1)?;
    let unresolved_count = u64::try_from(eligible.len().saturating_sub(selection.candidates.len()))
        .map_err(|_| SqliteStoreError::CountNotRepresentable)?;
    let logical_work_rows = budget.logical_rows()?;
    hasher.update(
        u64::try_from(selection.candidates.len())
            .map_err(|_| SqliteStoreError::CountNotRepresentable)?
            .to_be_bytes(),
    );
    for generation in &selection.candidates {
        hasher.update(generation.get().to_be_bytes());
    }
    hasher.update(selection.estimated_rows.to_be_bytes());
    hasher.update(selection.estimated_bytes.to_be_bytes());
    hasher.update(root_count.to_be_bytes());
    hasher.update(unresolved_count.to_be_bytes());
    hasher.update([u8::from(eligible_truncated)]);
    hasher.update(logical_work_rows.to_be_bytes());
    hasher.update([u8::from(selection.more_work)]);
    check_retention_control(cancelled, deadline)?;
    Ok((
        RetentionPlan::new(
            policy.digest(),
            RetentionPlanDigest::new(hasher.finalize().into()),
            selection.candidates,
            selection.estimated_rows,
            selection.estimated_bytes,
            root_count,
            unresolved_count,
            eligible_truncated,
            logical_work_rows,
            selection.more_work,
        ),
        budget,
    ))
}

fn select_retention_candidates(
    transaction: &Transaction<'_>,
    policy: &GenerationRetentionPolicy,
    eligible: &[EligibleRetentionGeneration],
    eligible_truncated: bool,
    budget: &mut RetentionWorkBudget,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<RetentionCandidateSelection, SqliteStoreError> {
    let limits = policy.limits();
    let mut selection = RetentionCandidateSelection {
        candidates: Vec::new(),
        estimated_rows: 0,
        estimated_bytes: 0,
        more_work: eligible_truncated,
    };
    for candidate in eligible {
        check_retention_control(cancelled, deadline)?;
        if u64::try_from(selection.candidates.len())
            .map_err(|_| SqliteStoreError::CountNotRepresentable)?
            >= limits.max_generation_candidates()
        {
            selection.more_work = true;
            break;
        }
        if !add_retention_candidate(
            transaction,
            candidate.generation,
            limits,
            budget,
            cancelled,
            deadline,
            &mut selection,
        )? {
            break;
        }
    }
    if selection.candidates.len() < eligible.len() {
        selection.more_work = true;
    }
    Ok(selection)
}

#[allow(
    clippy::too_many_arguments,
    reason = "the candidate, limits, budget, cancellation, deadline, and mutable selection are explicit transaction controls"
)]
fn add_retention_candidate(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    limits: crate::RetentionLimits,
    budget: &mut RetentionWorkBudget,
    cancelled: &AtomicBool,
    deadline: Instant,
    selection: &mut RetentionCandidateSelection,
) -> Result<bool, SqliteStoreError> {
    let (candidate_rows, candidate_bytes) =
        estimate_retention_generation(transaction, generation, cancelled, deadline)?;
    let next_rows = selection
        .estimated_rows
        .checked_add(candidate_rows)
        .ok_or(SqliteStoreError::RetentionLimitExceeded)?;
    let next_bytes = selection
        .estimated_bytes
        .checked_add(candidate_bytes)
        .ok_or(SqliteStoreError::RetentionLimitExceeded)?;
    let candidate_apply_rows = candidate_rows
        .checked_mul(2)
        .ok_or(SqliteStoreError::RetentionLimitExceeded)?;
    let candidate_and_audit_rows = candidate_apply_rows
        .checked_add(1)
        .ok_or(SqliteStoreError::RetentionLimitExceeded)?;
    if next_bytes > limits.max_estimated_bytes() || !budget.can_reserve(candidate_and_audit_rows) {
        if selection.candidates.is_empty() {
            return Err(SqliteStoreError::RetentionLimitExceeded);
        }
        selection.more_work = true;
        return Ok(false);
    }
    budget.reserve(candidate_apply_rows)?;
    selection.candidates.push(generation);
    selection.estimated_rows = next_rows;
    selection.estimated_bytes = next_bytes;
    Ok(true)
}

fn validate_retention_pins(
    transaction: &Transaction<'_>,
    policy: &GenerationRetentionPolicy,
    budget: &mut RetentionWorkBudget,
    root_count: &mut u64,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<BTreeSet<i64>, SqliteStoreError> {
    let mut pinned = BTreeSet::new();
    for generation in policy.pins().explicit_generations() {
        check_retention_control(cancelled, deadline)?;
        validate_generation_pin(transaction, *generation, false, cancelled, deadline)?;
        record_retention_root(budget, root_count)?;
        pinned.insert(generation.get());
    }
    for generation in policy.pins().supervised_generations() {
        check_retention_control(cancelled, deadline)?;
        validate_generation_pin(transaction, *generation, true, cancelled, deadline)?;
        record_retention_root(budget, root_count)?;
        pinned.insert(generation.get());
    }
    for workspace_view in policy.pins().workspace_views() {
        check_retention_control(cancelled, deadline)?;
        let published = transaction
            .query_row(
                "SELECT lifecycle_state = 'published'
                 FROM workspace_views WHERE workspace_view_id = ?1",
                [workspace_view.get()],
                |row| row.get::<_, bool>(0),
            )
            .optional()
            .map_err(|error| retention_database_error(error, cancelled, deadline))?
            .ok_or(SqliteStoreError::RetentionPinUnavailable)?;
        if !published {
            return Err(SqliteStoreError::RetentionPinUnavailable);
        }
        record_retention_root(budget, root_count)?;
        let mut statement = transaction
            .prepare(
                "SELECT generation_id FROM workspace_view_members
                 WHERE workspace_view_id = ?1 ORDER BY ordinal",
            )
            .map_err(|error| retention_database_error(error, cancelled, deadline))?;
        let rows = statement
            .query_map([workspace_view.get()], |row| row.get::<_, i64>(0))
            .map_err(|error| retention_database_error(error, cancelled, deadline))?;
        let mut member_count = 0_usize;
        for row in rows {
            let generation =
                row.map_err(|error| retention_database_error(error, cancelled, deadline))?;
            if generation <= 0 {
                return Err(SqliteStoreError::IntegrityCheckFailed);
            }
            record_retention_root(budget, root_count)?;
            pinned.insert(generation);
            member_count = member_count
                .checked_add(1)
                .ok_or(SqliteStoreError::CountNotRepresentable)?;
            if pinned.len() > MAX_RETENTION_GENERATION_PINS {
                return Err(SqliteStoreError::RetentionPinUnavailable);
            }
        }
        if member_count == 0 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
    }
    Ok(pinned)
}

fn validate_generation_pin(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    supervised: bool,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SqliteStoreError> {
    let state = transaction
        .query_row(
            "SELECT lifecycle_state FROM index_generations WHERE generation_id = ?1",
            [generation.get()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| retention_database_error(error, cancelled, deadline))?
        .ok_or(SqliteStoreError::RetentionPinUnavailable)?;
    let valid = if supervised {
        !matches!(state.as_str(), "failed" | "cancelled")
    } else {
        matches!(state.as_str(), "active" | "retained")
    };
    if valid {
        Ok(())
    } else {
        Err(SqliteStoreError::RetentionPinUnavailable)
    }
}

fn hash_retention_roots(
    transaction: &Transaction<'_>,
    policy: &GenerationRetentionPolicy,
    hasher: &mut Sha256,
    budget: &mut RetentionWorkBudget,
    root_count: &mut u64,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SqliteStoreError> {
    hash_workspace_roots(transaction, hasher, budget, root_count, cancelled, deadline)?;
    hash_source_slot_roots(transaction, hasher, budget, root_count, cancelled, deadline)?;
    hash_active_view_roots(transaction, hasher, budget, root_count, cancelled, deadline)?;
    hash_current_receipt_roots(transaction, hasher, budget, root_count, cancelled, deadline)?;
    hash_enforced_retention_root_relations(
        transaction,
        policy,
        hasher,
        budget,
        root_count,
        cancelled,
        deadline,
    )
}

fn hash_workspace_roots(
    transaction: &Transaction<'_>,
    hasher: &mut Sha256,
    budget: &mut RetentionWorkBudget,
    root_count: &mut u64,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SqliteStoreError> {
    let mut statement = transaction
        .prepare(
            "SELECT workspace_id, source_epoch, COALESCE(active_generation_id, 0)
             FROM workspaces ORDER BY workspace_id",
        )
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    for row in rows {
        check_retention_control(cancelled, deadline)?;
        let (workspace, epoch, active) =
            row.map_err(|error| retention_database_error(error, cancelled, deadline))?;
        record_retention_root(budget, root_count)?;
        hasher.update(b"workspace");
        hasher.update(workspace.to_be_bytes());
        hasher.update(epoch.to_be_bytes());
        hasher.update(active.to_be_bytes());
    }
    Ok(())
}

fn hash_source_slot_roots(
    transaction: &Transaction<'_>,
    hasher: &mut Sha256,
    budget: &mut RetentionWorkBudget,
    root_count: &mut u64,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SqliteStoreError> {
    let mut statement = transaction
        .prepare(
            "SELECT connected_workspace_id, source_slot_id,
                    generation_workspace_id, source_epoch
             FROM workspace_source_slots
             ORDER BY connected_workspace_id, source_slot_id",
        )
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    for row in rows {
        check_retention_control(cancelled, deadline)?;
        let (workspace, slot, generation_workspace, epoch) =
            row.map_err(|error| retention_database_error(error, cancelled, deadline))?;
        if workspace.len() != 32 || slot.len() != 32 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        record_retention_root(budget, root_count)?;
        hasher.update(b"slot");
        hasher.update(workspace);
        hasher.update(slot);
        hasher.update(generation_workspace.to_be_bytes());
        hasher.update(epoch.to_be_bytes());
    }
    Ok(())
}

fn hash_active_view_roots(
    transaction: &Transaction<'_>,
    hasher: &mut Sha256,
    budget: &mut RetentionWorkBudget,
    root_count: &mut u64,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SqliteStoreError> {
    let mut statement = transaction
        .prepare(
            "SELECT active.connected_workspace_id, active.workspace_view_id
             FROM active_workspace_views AS active
             ORDER BY active.connected_workspace_id",
        )
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    for row in rows {
        check_retention_control(cancelled, deadline)?;
        let (workspace, view) =
            row.map_err(|error| retention_database_error(error, cancelled, deadline))?;
        if workspace.len() != 32 || view <= 0 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        record_retention_root(budget, root_count)?;
        hasher.update(b"active-view");
        hasher.update(workspace);
        hasher.update(view.to_be_bytes());
    }
    Ok(())
}

fn hash_current_receipt_roots(
    transaction: &Transaction<'_>,
    hasher: &mut Sha256,
    budget: &mut RetentionWorkBudget,
    root_count: &mut u64,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SqliteStoreError> {
    let mut statement = transaction
        .prepare(
            "SELECT receipt.source_slot_id, receipt.source_epoch,
                    receipt.generation_id
             FROM source_slot_generation_receipts AS receipt
             JOIN workspace_source_slots AS slot
               ON slot.connected_workspace_id = receipt.connected_workspace_id
              AND slot.source_slot_id = receipt.source_slot_id
              AND slot.source_epoch = receipt.source_epoch
             ORDER BY receipt.source_slot_id, receipt.source_epoch",
        )
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    for row in rows {
        check_retention_control(cancelled, deadline)?;
        let (slot, epoch, generation) =
            row.map_err(|error| retention_database_error(error, cancelled, deadline))?;
        if slot.len() != 32 || epoch < 0 || generation <= 0 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        record_retention_root(budget, root_count)?;
        hasher.update(b"current-receipt");
        hasher.update(slot);
        hasher.update(epoch.to_be_bytes());
        hasher.update(generation.to_be_bytes());
    }
    Ok(())
}

fn record_retention_root(
    budget: &mut RetentionWorkBudget,
    root_count: &mut u64,
) -> Result<(), SqliteStoreError> {
    budget.consume(1)?;
    *root_count = root_count
        .checked_add(1)
        .ok_or(SqliteStoreError::RetentionLimitExceeded)?;
    Ok(())
}

fn eligible_retention_generations(
    transaction: &Transaction<'_>,
    policy: &GenerationRetentionPolicy,
    pinned_generations: &BTreeSet<i64>,
    budget: &mut RetentionWorkBudget,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(Vec<EligibleRetentionGeneration>, bool), SqliteStoreError> {
    let mut sql = String::from(include_str!("eligible_generations.sql"));
    let mut parameters = vec![rusqlite::types::Value::Integer(i64::from(
        policy.retained_generations_per_source_slot(),
    ))];
    if !pinned_generations.is_empty() {
        sql.push_str(" AND eligible.generation_id NOT IN (");
        for (ordinal, generation) in pinned_generations.iter().enumerate() {
            if ordinal != 0 {
                sql.push(',');
            }
            let parameter = parameters
                .len()
                .checked_add(1)
                .ok_or(SqliteStoreError::CountNotRepresentable)?;
            sql.push('?');
            sql.push_str(&parameter.to_string());
            parameters.push(rusqlite::types::Value::Integer(*generation));
        }
        sql.push(')');
    }
    let limit_parameter = parameters
        .len()
        .checked_add(1)
        .ok_or(SqliteStoreError::CountNotRepresentable)?;
    sql.push_str(
        " ORDER BY eligible.sort_source_slot,
                   eligible.sort_source_epoch, eligible.generation_id
          LIMIT ?",
    );
    sql.push_str(&limit_parameter.to_string());
    let query_limit = MAX_RETENTION_GENERATION_CANDIDATES
        .checked_add(1)
        .map(|value| value.min(budget.available()))
        .and_then(|value| i64::try_from(value).ok())
        .ok_or(SqliteStoreError::CountNotRepresentable)?;
    if query_limit == 0 {
        return Ok((Vec::new(), true));
    }
    parameters.push(rusqlite::types::Value::Integer(query_limit));

    check_retention_control(cancelled, deadline)?;
    let mut statement = transaction
        .prepare(&sql)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    let mut eligible = Vec::new();
    for row in rows {
        check_retention_control(cancelled, deadline)?;
        let (generation, source_slot, source_epoch) =
            row.map_err(|error| retention_database_error(error, cancelled, deadline))?;
        let sort_source_slot: [u8; 32] = source_slot
            .try_into()
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        if generation <= 0 || source_epoch < 0 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        budget.consume(1)?;
        eligible.push(EligibleRetentionGeneration {
            generation: GenerationId::from_database(generation),
            sort_source_slot,
            source_epoch,
        });
    }
    let truncated = i64::try_from(eligible.len())
        .map_err(|_| SqliteStoreError::CountNotRepresentable)?
        == query_limit;
    Ok((eligible, truncated))
}

#[allow(
    clippy::too_many_lines,
    reason = "the bounded SQL projection must keep every generation-owned row type auditable together"
)]
fn estimate_retention_generation(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(u64, u64), SqliteStoreError> {
    let rows: i64 = transaction
        .query_row(
            "WITH candidate_artifacts(artifact_digest) AS (
                 SELECT artifact_digest FROM generation_files
                 WHERE generation_id = ?1
                 UNION
                 SELECT graph_artifact_digest FROM generation_graph_artifacts
                 WHERE generation_id = ?1
                 UNION
                 SELECT artifact_digest FROM generation_graph_definitions
                 WHERE generation_id = ?1
                 UNION
                 SELECT site_artifact_digest FROM generation_graph_resolutions
                 WHERE generation_id = ?1
                 UNION
                 SELECT syntax_site_artifact_digest FROM generation_syntax_site_artifacts
                 WHERE generation_id = ?1
             )
             SELECT
                 1
                 + (SELECT count(*) FROM generation_files
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_facts
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_search
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_search_rebuild
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM workspace_view_members
                    WHERE workspace_view_id IN (
                        SELECT workspace_view_id FROM workspace_view_members
                        WHERE generation_id = ?1
                    ))
                 + (SELECT count(DISTINCT workspace_view_id)
                    FROM workspace_view_members WHERE generation_id = ?1)
                 + (SELECT count(*) FROM source_slot_generation_receipts
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_graph_edges
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_graph_candidates
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_graph_resolutions
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_graph_definitions
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_graph_artifacts
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_graph_sources
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_graph_publications
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_graph_requirements
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_syntax_site_artifacts
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_syntax_site_publications
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_syntax_site_requirements
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_repository_topology_entries
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_repository_topology_publications
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM generation_repository_topology_requirements
                    WHERE generation_id = ?1)
                 + (SELECT count(*) FROM source_snapshots
                    WHERE snapshot_digest = (
                        SELECT snapshot_digest FROM index_generations
                        WHERE generation_id = ?1
                    ))
                 + (SELECT count(*) FROM source_manifest_entries
                    WHERE snapshot_digest = (
                        SELECT snapshot_digest FROM index_generations
                        WHERE generation_id = ?1
                    ))
                 + (SELECT count(*) FROM candidate_artifacts)
                 + (SELECT count(*) FROM artifact_facts
                    WHERE artifact_digest IN candidate_artifacts)
                 + (SELECT count(*) FROM artifact_fact_correspondence
                    WHERE artifact_digest IN candidate_artifacts)
                 + (SELECT count(*) FROM rust_graph_artifacts
                    WHERE artifact_digest IN candidate_artifacts)
                 + (SELECT count(*) FROM rust_graph_sites
                    WHERE artifact_digest IN candidate_artifacts)
                 + (SELECT count(*) FROM syntax_site_artifacts
                    WHERE artifact_digest IN candidate_artifacts)
                 + (SELECT count(*) FROM syntax_sites
                    WHERE artifact_digest IN candidate_artifacts)",
            [generation.get()],
            |row| row.get(0),
        )
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    let source_bytes: i64 = transaction
        .query_row(
            "SELECT snapshot.total_source_bytes
             FROM index_generations AS generation
             JOIN source_snapshots AS snapshot
               ON snapshot.snapshot_digest = generation.snapshot_digest
             WHERE generation.generation_id = ?1",
            [generation.get()],
            |row| row.get(0),
        )
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    let rows = u64::try_from(rows).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let source_bytes =
        u64::try_from(source_bytes).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let bytes = rows
        .checked_mul(ESTIMATED_ROW_BYTES)
        .and_then(|overhead| overhead.checked_add(source_bytes))
        .ok_or(SqliteStoreError::RetentionLimitExceeded)?;
    Ok((rows, bytes))
}
