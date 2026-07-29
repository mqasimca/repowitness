#[allow(
    clippy::too_many_lines,
    reason = "one immediate transaction stages, validates, completes, and switches one immutable projection"
)]
fn publish_memory_projection_inner(
    connection: &mut Connection,
    prepared: &PreparedMemoryProjection,
    control: WriteControl<'_>,
) -> Result<MemoryProjectionPublication, SqliteStoreError> {
    check_control(control)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| control_database_error(control))?;
    let current =
        load_active_source(&transaction, prepared.source.repository).map_err(|error| {
            if error == SqliteStoreError::GenerationUnavailable {
                SqliteStoreError::StaleSourceEpoch
            } else {
                error
            }
        })?;
    if current != prepared.source {
        return Err(SqliteStoreError::StaleSourceEpoch);
    }
    require_current_write_source(&transaction, current, control)?;
    let target = projection_target(prepared.target);
    let profile = phase0_rust_correspondence_profile_digest();
    let record_count = u32::try_from(prepared.records.len())
        .map_err(|_| SqliteStoreError::CountNotRepresentable)?;
    transaction
        .execute(
            "INSERT INTO memory_projection_generations(
                workspace_id, index_generation_id, source_epoch, snapshot_digest,
                target_kind, target_format, target_revision, head_format, head_revision,
                correspondence_profile_id, correspondence_profile_version,
                correspondence_profile_digest, lifecycle_state,
                searched_count, skipped_count, unresolved_count, truncated_count,
                total_count, current_count, not_applicable_count, stale_count,
                needs_review_count, indeterminate_count, conflicted_count,
                contradicted_count, superseded_count, quarantined_count, tombstoned_count
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'staging',
                ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24,
                ?25, ?26, ?27
             )",
            params![
                prepared.source.workspace_id,
                prepared.source.generation.get(),
                fixed_integer(prepared.source.source_epoch)?,
                prepared.source.snapshot.as_bytes().as_slice(),
                target.kind,
                target.format,
                target.revision,
                target.head_format,
                target.head_revision,
                RUST_CORRESPONDENCE_PROFILE_ID,
                i64::from(RUST_CORRESPONDENCE_PROFILE_VERSION),
                profile.as_bytes().as_slice(),
                i64::from(record_count),
                i64::from(prepared.skipped_count),
                i64::from(prepared.unresolved_count),
                i64::from(prepared.truncated_count),
                i64::from(record_count),
                i64::from(prepared.state_counts.current),
                i64::from(prepared.state_counts.not_applicable),
                i64::from(prepared.state_counts.stale),
                i64::from(prepared.state_counts.needs_review),
                i64::from(prepared.state_counts.indeterminate),
                i64::from(prepared.state_counts.conflicted),
                i64::from(prepared.state_counts.contradicted),
                i64::from(prepared.state_counts.superseded),
                i64::from(prepared.state_counts.quarantined),
                i64::from(prepared.state_counts.tombstoned),
            ],
        )
        .map_err(|_| control_database_error(control))?;
    let projection_id = transaction.last_insert_rowid();
    if projection_id <= 0 {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }

    for (ordinal, record) in prepared.records.iter().enumerate() {
        check_control(control)?;
        let ordinal =
            i64::try_from(ordinal).map_err(|_| SqliteStoreError::CountNotRepresentable)?;
        insert_projection_record(
            &transaction,
            projection_id,
            prepared.source,
            ordinal,
            record,
            control,
        )?;
    }
    check_control(control)?;
    let completed = transaction
        .execute(
            "UPDATE memory_projection_generations
             SET lifecycle_state = 'complete'
             WHERE projection_id = ?1 AND lifecycle_state = 'staging'",
            [projection_id],
        )
        .map_err(|_| control_database_error(control))?;
    if completed != 1 {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    transaction
        .execute(
            "INSERT INTO active_memory_projections(workspace_id, projection_id)
             VALUES (?1, ?2)
             ON CONFLICT(workspace_id) DO UPDATE
             SET projection_id = excluded.projection_id",
            params![prepared.source.workspace_id, projection_id],
        )
        .map_err(|_| control_database_error(control))?;
    check_control(control)?;
    transaction
        .commit()
        .map_err(|_| control_database_error(control))?;
    Ok(MemoryProjectionPublication {
        projection_id,
        projected_records: record_count,
        skipped_records: prepared.skipped_count,
        unresolved_records: prepared.unresolved_count,
    })
}

fn insert_projection_record(
    transaction: &rusqlite::Transaction<'_>,
    projection_id: i64,
    source: MemoryProjectionSource,
    ordinal: i64,
    record: &PreparedProjectionRecord,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    let persisted = persisted_record(record);
    transaction
        .execute(
            "INSERT INTO memory_projection_records(
                projection_id, workspace_id, ordinal, record_id, revision_digest,
                effective_state, validity_state, evidence_state, reason,
                evidence_count, resolved_count, review_count, indeterminate_count,
                head_count, missing_parent_count, has_trusted_approval
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, ?15, 1
             )",
            params![
                projection_id,
                source.workspace_id,
                ordinal,
                record.record_id.as_bytes().as_slice(),
                persisted.revision,
                persisted.effective,
                persisted.validity,
                persisted.evidence,
                persisted.reason,
                i64::from(persisted.evidence_count),
                i64::from(persisted.resolved_count),
                i64::from(persisted.review_count),
                i64::from(persisted.indeterminate_count),
                i64::from(persisted.head_count),
                i64::from(persisted.missing_parent_count),
            ],
        )
        .map_err(|_| control_database_error(control))?;
    if let PreparedProjectionRecordKind::Evaluated {
        revision, evidence, ..
    } = &record.kind
    {
        for (evidence_ordinal, result) in evidence.iter().enumerate() {
            check_control(control)?;
            insert_projection_evidence(
                transaction,
                projection_id,
                source,
                ordinal,
                record.record_id,
                *revision,
                i64::try_from(evidence_ordinal)
                    .map_err(|_| SqliteStoreError::CountNotRepresentable)?,
                result,
                control,
            )?;
        }
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "the immutable projection evidence key remains explicit"
)]
fn insert_projection_evidence(
    transaction: &rusqlite::Transaction<'_>,
    projection_id: i64,
    source: MemoryProjectionSource,
    record_ordinal: i64,
    record_id: MemoryRecordId,
    revision: CanonicalMemoryDigest,
    evidence_ordinal: i64,
    result: &PreparedProjectionEvidence,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    let target_path = result.target.as_ref().map(|target| target.path.as_bytes());
    let target_artifact = result
        .target
        .as_ref()
        .map(|target| target.artifact.as_bytes().as_slice());
    let target_fact = result
        .target
        .as_ref()
        .map(|target| fixed_integer(target.fact_ordinal))
        .transpose()?;
    let target_declaration = result
        .target
        .as_ref()
        .map(|target| target.declaration.as_bytes().as_slice());
    let target_name_elided = result
        .target
        .as_ref()
        .map(|target| target.name_elided.as_bytes().as_slice());
    let target_snapshot = result
        .target
        .as_ref()
        .map(|_| source.snapshot.as_bytes().as_slice());
    let (method_id, method_version) = projection_evidence_method(result.outcome);
    transaction
        .execute(
            "INSERT INTO memory_projection_evidence(
                projection_id, workspace_id, record_ordinal, record_id,
                revision_digest, evidence_ordinal, outcome, method_id,
                method_version, assurance, target_snapshot_digest,
                target_repository_path, target_artifact_digest, target_fact_ordinal,
                target_declaration_digest, target_name_elided_digest,
                candidate_coverage, candidate_count_before_limit
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, ?15, ?16, ?17, ?18
             )",
            params![
                projection_id,
                source.workspace_id,
                record_ordinal,
                record_id.as_bytes().as_slice(),
                revision.as_bytes().as_slice(),
                evidence_ordinal,
                projection_evidence_outcome(result.outcome),
                method_id,
                i64::from(method_version),
                projection_assurance(result.assurance),
                target_snapshot,
                target_path,
                target_artifact,
                target_fact,
                target_declaration,
                target_name_elided,
                if result.candidate_coverage_complete {
                    "complete"
                } else {
                    "partial"
                },
                fixed_integer(result.candidate_count_before_limit)?,
            ],
        )
        .map_err(|_| control_database_error(control))?;
    for (candidate_ordinal, candidate) in result.candidates.iter().enumerate() {
        check_control(control)?;
        transaction
            .execute(
                "INSERT INTO memory_projection_candidates(
                    projection_id, workspace_id, record_ordinal, evidence_ordinal,
                    ordinal, record_id, revision_digest, target_snapshot_digest,
                    target_repository_path, target_artifact_digest, target_fact_ordinal,
                    target_declaration_digest, target_name_elided_digest,
                    proposed_relation, method_id, method_version, assurance
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                    ?13, ?14, ?15, ?16, 'review_required'
                 )",
                params![
                    projection_id,
                    source.workspace_id,
                    record_ordinal,
                    evidence_ordinal,
                    i64::try_from(candidate_ordinal)
                        .map_err(|_| SqliteStoreError::CountNotRepresentable)?,
                    record_id.as_bytes().as_slice(),
                    revision.as_bytes().as_slice(),
                    source.snapshot.as_bytes().as_slice(),
                    candidate.occurrence.path.as_bytes(),
                    candidate.occurrence.artifact.as_bytes().as_slice(),
                    fixed_integer(candidate.occurrence.fact_ordinal)?,
                    candidate.occurrence.declaration.as_bytes().as_slice(),
                    candidate.occurrence.name_elided.as_bytes().as_slice(),
                    projection_candidate_relation(candidate.relation),
                    RUST_CORRESPONDENCE_PROFILE_ID,
                    i64::from(RUST_CORRESPONDENCE_PROFILE_VERSION),
                ],
            )
            .map_err(|_| control_database_error(control))?;
    }
    Ok(())
}

fn validate_projection_record(record: &PreparedProjectionRecord) -> Result<(), SqliteStoreError> {
    match &record.kind {
        PreparedProjectionRecordKind::Evaluated {
            decision, evidence, ..
        } => {
            if usize::try_from(decision.evidence_count())
                .map_err(|_| SqliteStoreError::CountNotRepresentable)?
                > MAX_MEMORY_EVIDENCE
            {
                return Err(SqliteStoreError::InvalidMemoryProjection);
            }
            let should_have_results =
                decision.evidence_state() != MemoryProjectionEvidenceState::NotEvaluated;
            if (should_have_results
                && evidence.len()
                    != usize::try_from(decision.evidence_count())
                        .map_err(|_| SqliteStoreError::CountNotRepresentable)?)
                || (!should_have_results && !evidence.is_empty())
            {
                return Err(SqliteStoreError::InvalidMemoryProjection);
            }
            let mut resolved = 0_u32;
            let mut review = 0_u32;
            let mut indeterminate = 0_u32;
            for result in evidence {
                validate_projection_evidence(result)?;
                match result.outcome {
                    ProjectionEvidenceOutcome::Exact
                    | ProjectionEvidenceOutcome::SamePathRename
                    | ProjectionEvidenceOutcome::GitExactMove
                    | ProjectionEvidenceOutcome::ReviewedLink => {
                        resolved = resolved
                            .checked_add(1)
                            .ok_or(SqliteStoreError::CountNotRepresentable)?;
                    }
                    ProjectionEvidenceOutcome::Ambiguous => {
                        review = review
                            .checked_add(1)
                            .ok_or(SqliteStoreError::CountNotRepresentable)?;
                    }
                    ProjectionEvidenceOutcome::Indeterminate => {
                        indeterminate = indeterminate
                            .checked_add(1)
                            .ok_or(SqliteStoreError::CountNotRepresentable)?;
                    }
                    ProjectionEvidenceOutcome::Changed | ProjectionEvidenceOutcome::Missing => {}
                }
            }
            if resolved != decision.resolved_count()
                || review != decision.review_count()
                || indeterminate != decision.indeterminate_count()
            {
                return Err(SqliteStoreError::InvalidMemoryProjection);
            }
        }
        PreparedProjectionRecordKind::Conflicted { head_count } if *head_count >= 2 => {}
        PreparedProjectionRecordKind::IndeterminateHead {
            revision,
            evidence_count,
            head_count,
            missing_parent_count,
            reason,
        } => {
            if usize::try_from(*evidence_count)
                .map_err(|_| SqliteStoreError::CountNotRepresentable)?
                > MAX_MEMORY_EVIDENCE
                || (revision.is_none() && *evidence_count != 0)
                || (*reason == ProjectionHeadReason::MissingParent && *missing_parent_count == 0)
                || (*reason == ProjectionHeadReason::InvalidHeadGraph
                    && (*head_count != 0 || *missing_parent_count != 0))
            {
                return Err(SqliteStoreError::InvalidMemoryProjection);
            }
        }
        PreparedProjectionRecordKind::Conflicted { .. } => {
            return Err(SqliteStoreError::InvalidMemoryProjection);
        }
    }
    Ok(())
}

fn validate_projection_evidence(
    result: &PreparedProjectionEvidence,
) -> Result<(), SqliteStoreError> {
    if result.candidate_count_before_limit > MAX_MEMORY_INTEROPERABLE_INTEGER {
        return Err(SqliteStoreError::InvalidMemoryProjection);
    }
    match result.outcome {
        ProjectionEvidenceOutcome::Exact
        | ProjectionEvidenceOutcome::SamePathRename
        | ProjectionEvidenceOutcome::GitExactMove => {
            if result.target.is_none()
                || !result.candidates.is_empty()
                || !result.candidate_coverage_complete
                || !matches!(
                    result.assurance,
                    ProjectionEvidenceAssurance::Automatic | ProjectionEvidenceAssurance::Reviewed
                )
            {
                return Err(SqliteStoreError::InvalidMemoryProjection);
            }
        }
        ProjectionEvidenceOutcome::ReviewedLink => {
            if result.target.is_none()
                || !result.candidates.is_empty()
                || result.assurance != ProjectionEvidenceAssurance::Reviewed
            {
                return Err(SqliteStoreError::InvalidMemoryProjection);
            }
        }
        ProjectionEvidenceOutcome::Changed => {
            if result.target.is_none()
                || !result.candidates.is_empty()
                || !result.candidate_coverage_complete
                || result.assurance != ProjectionEvidenceAssurance::None
            {
                return Err(SqliteStoreError::InvalidMemoryProjection);
            }
        }
        ProjectionEvidenceOutcome::Ambiguous => {
            if result.target.is_some()
                || result.assurance != ProjectionEvidenceAssurance::None
                || !result.candidate_coverage_complete
                || result.candidates.is_empty()
                || result.candidates.len() > MAX_RUST_CORRESPONDENCE_CANDIDATES
                || result.candidate_count_before_limit
                    != u64::try_from(result.candidates.len())
                        .map_err(|_| SqliteStoreError::CountNotRepresentable)?
            {
                return Err(SqliteStoreError::InvalidMemoryProjection);
            }
        }
        ProjectionEvidenceOutcome::Missing => {
            if result.target.is_some()
                || !result.candidates.is_empty()
                || !result.candidate_coverage_complete
                || result.assurance != ProjectionEvidenceAssurance::None
            {
                return Err(SqliteStoreError::InvalidMemoryProjection);
            }
        }
        ProjectionEvidenceOutcome::Indeterminate => {
            if result.target.is_some()
                || !result.candidates.is_empty()
                || result.assurance != ProjectionEvidenceAssurance::None
            {
                return Err(SqliteStoreError::InvalidMemoryProjection);
            }
        }
    }
    Ok(())
}

fn record_effective_state(record: &PreparedProjectionRecord) -> MemoryEffectiveState {
    match &record.kind {
        PreparedProjectionRecordKind::Evaluated { decision, .. } => decision.effective_state(),
        PreparedProjectionRecordKind::Conflicted { .. } => MemoryEffectiveState::Conflicted,
        PreparedProjectionRecordKind::IndeterminateHead { .. } => {
            MemoryEffectiveState::Indeterminate
        }
    }
}
