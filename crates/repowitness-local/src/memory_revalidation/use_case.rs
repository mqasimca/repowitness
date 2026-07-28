/// Rebuilds and atomically activates one complete local memory projection.
pub fn revalidate_local_memory(
    request: LocalMemoryRevalidationRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalMemoryRevalidationReport, LocalMemoryRevalidationError> {
    let (load_limits, result_limits) = validated_limits(request.limits)?;
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(|source| LocalMemoryRevalidationError::RepositoryIdentity { source })?;
    let deadline = Instant::now()
        .checked_add(request.limits.deadline())
        .ok_or(LocalMemoryRevalidationError::DeadlineNotRepresentable)?;
    check_control(cancelled.as_ref(), deadline)?;
    let worktree = discovered_worktree_root(request.repository_root)
        .map_err(|source| LocalMemoryRevalidationError::Discovery { source })?;
    let database = validated_database_outside_worktree(&worktree, request.database)
        .map_err(map_database_path_error)?;
    let mutation_lease = SqliteMutationLease::acquire(&database, deadline)
        .map_err(|source| LocalMemoryRevalidationError::StoreStartup { source })?;
    let database_identity = database_alias_identity(&database).map_err(map_database_path_error)?;
    let (writer, startup) = OwnedSqliteIndex::start_with_lease(
        mutation_lease,
        database_identity,
        request.migration_applied_at_unix_ms,
        Arc::clone(&cancelled),
        deadline,
    )
    .map_err(map_store_startup_error)?;

    let operation = rebuild_projection(
        &writer,
        repository,
        &worktree,
        request.limits,
        load_limits,
        result_limits,
        &cancelled,
        deadline,
    );
    let (publication, source, git_queries, head_available) = match operation {
        Ok(result) => result,
        Err(error) => {
            let _ = writer.shutdown(deadline);
            return Err(error);
        }
    };
    if let Err(source) = writer.checkpoint(deadline) {
        let _ = writer.shutdown(deadline);
        return Err(LocalMemoryRevalidationError::Checkpoint { source });
    }
    writer
        .shutdown(deadline)
        .map_err(|source| LocalMemoryRevalidationError::Shutdown { source })?;

    Ok(LocalMemoryRevalidationReport {
        projection_id: publication.projection_id(),
        generation: source.generation(),
        source_epoch: source.source_epoch(),
        recovered_generations: startup.recovered_generations(),
        projected_records: publication.projected_records(),
        skipped_records: publication.skipped_records(),
        unresolved_records: publication.unresolved_records(),
        git_queries,
        head_available,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "the owned store, exact source, limits, and controls are independent trust inputs"
)]
fn rebuild_projection(
    writer: &OwnedSqliteIndex,
    repository: repowitness_domain::RepositoryIdentityDigest,
    worktree: &Path,
    limits: LocalMemoryRevalidationLimits,
    load_limits: MemoryProjectionLoadLimits,
    result_limits: MemoryProjectionResultLimits,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> Result<
    (
        MemoryProjectionPublication,
        MemoryProjectionSource,
        u32,
        bool,
    ),
    LocalMemoryRevalidationError,
> {
    let journal = writer
        .load_memory_journal(repository, load_limits, Arc::clone(cancelled), deadline)
        .map_err(|source| LocalMemoryRevalidationError::JournalLoad { source })?;
    let source = journal.source();
    let before =
        capture_bounded_source_state(worktree, limits.source_state(), cancelled, deadline)?;
    let mut query_budget = GitQueryBudget::new(limits.max_git_queries());
    let mut queries = None;
    let mut head = None;
    if before.git_state() == source.git_state() {
        query_budget.reserve()?;
        let opened = GitMemoryQueries::open(worktree, limits.git(), cancelled.as_ref(), deadline)
            .map_err(|source| LocalMemoryRevalidationError::GitQuery { source })?;
        query_budget.reserve()?;
        head = opened
            .head_commit(cancelled.as_ref(), deadline)
            .map_err(|source| LocalMemoryRevalidationError::GitQuery { source })?;
        queries = Some(opened);
    }
    let target = MemoryRevalidationTarget::worktree(source.snapshot(), head);
    let (records, skipped_count) = prepare_records(
        writer,
        &journal,
        target,
        queries.as_ref(),
        head,
        &mut query_budget,
        cancelled,
        deadline,
    )?;
    let after = capture_bounded_source_state(worktree, limits.source_state(), cancelled, deadline)?;
    if after != before {
        return Err(LocalMemoryRevalidationError::ConcurrentSourceChange);
    }
    let prepared =
        PreparedMemoryProjection::try_new(source, target, records, skipped_count, 0, result_limits)
            .map_err(|source| LocalMemoryRevalidationError::ProjectionPreparation { source })?;
    let publication = writer
        .publish_memory_projection(prepared, Arc::clone(cancelled), deadline)
        .map_err(|source| LocalMemoryRevalidationError::Publication { source })?;
    Ok((publication, source, query_budget.used(), head.is_some()))
}

#[allow(
    clippy::too_many_arguments,
    reason = "projection evaluation keeps the exact store, source, target, Git adapter, and controls explicit"
)]
fn prepare_records(
    writer: &OwnedSqliteIndex,
    journal: &LoadedMemoryJournal,
    target: MemoryRevalidationTarget,
    queries: Option<&GitMemoryQueries>,
    head: Option<MemoryCommitId>,
    query_budget: &mut GitQueryBudget,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> Result<(Vec<PreparedProjectionRecord>, u32), LocalMemoryRevalidationError> {
    let versions = journal.versions();
    let mut records = Vec::new();
    let mut skipped_count = 0_u32;
    let mut start = 0_usize;
    while start < versions.len() {
        check_control(cancelled.as_ref(), deadline)?;
        let record_id = versions[start].record().header().record_id();
        let mut end = start + 1;
        while end < versions.len() && versions[end].record().header().record_id() == record_id {
            end += 1;
        }
        match prepare_record(
            writer,
            journal.source(),
            &versions[start..end],
            target,
            queries,
            head,
            query_budget,
            cancelled,
            deadline,
        )? {
            Some(record) => records.push(record),
            None => {
                skipped_count = skipped_count
                    .checked_add(1)
                    .ok_or(LocalMemoryRevalidationError::CountNotRepresentable)?;
            }
        }
        start = end;
    }
    Ok((records, skipped_count))
}

#[allow(
    clippy::too_many_arguments,
    reason = "one record evaluation retains every exact source and control input"
)]
fn prepare_record(
    writer: &OwnedSqliteIndex,
    source: MemoryProjectionSource,
    versions: &[LoadedMemoryVersion],
    target: MemoryRevalidationTarget,
    queries: Option<&GitMemoryQueries>,
    head: Option<MemoryCommitId>,
    query_budget: &mut GitQueryBudget,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> Result<Option<PreparedProjectionRecord>, LocalMemoryRevalidationError> {
    let record_id = versions
        .first()
        .ok_or(LocalMemoryRevalidationError::JournalIntegrity)?
        .record()
        .header()
        .record_id();
    let inputs = versions
        .iter()
        .map(|version| {
            MemoryVersionHeadInput::new(
                version.revision(),
                version.record(),
                version.locally_approved(),
            )
        })
        .collect::<Vec<_>>();
    let selection = select_memory_head(&inputs)
        .map_err(|source| LocalMemoryRevalidationError::HeadSelection { source })?;
    let kind = match selection.state() {
        MemoryHeadState::NoApprovedVersion => return Ok(None),
        MemoryHeadState::Conflicted => PreparedProjectionRecordKind::Conflicted {
            head_count: selection.head_count(),
        },
        MemoryHeadState::Indeterminate => {
            let selected = selection
                .selected_revision()
                .map(|revision| {
                    versions
                        .iter()
                        .find(|version| version.revision() == revision)
                        .ok_or(LocalMemoryRevalidationError::JournalIntegrity)
                })
                .transpose()?;
            let evidence_count = selected
                .map(|version| u32::try_from(version.record().evidence().len()))
                .transpose()
                .map_err(|_| LocalMemoryRevalidationError::CountNotRepresentable)?
                .unwrap_or(0);
            PreparedProjectionRecordKind::IndeterminateHead {
                revision: selection.selected_revision(),
                evidence_count,
                head_count: selection.head_count(),
                missing_parent_count: selection.missing_parent_count(),
                reason: if selection.missing_parent_count() > 0 {
                    ProjectionHeadReason::MissingParent
                } else {
                    ProjectionHeadReason::InvalidHeadGraph
                },
            }
        }
        MemoryHeadState::Selected => {
            let revision = selection
                .selected_revision()
                .ok_or(LocalMemoryRevalidationError::JournalIntegrity)?;
            let version = versions
                .iter()
                .find(|version| version.revision() == revision)
                .ok_or(LocalMemoryRevalidationError::JournalIntegrity)?;
            evaluated_record(
                writer,
                source,
                version,
                target,
                queries,
                head,
                query_budget,
                cancelled,
                deadline,
            )?
        }
    };
    Ok(Some(PreparedProjectionRecord { record_id, kind }))
}

#[allow(
    clippy::too_many_arguments,
    reason = "selected-version evaluation keeps exact source, target, Git, and controls explicit"
)]
fn evaluated_record(
    writer: &OwnedSqliteIndex,
    source: MemoryProjectionSource,
    version: &LoadedMemoryVersion,
    target: MemoryRevalidationTarget,
    queries: Option<&GitMemoryQueries>,
    head: Option<MemoryCommitId>,
    query_budget: &mut GitQueryBudget,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> Result<PreparedProjectionRecordKind, LocalMemoryRevalidationError> {
    let record = version.record();
    if record.lifecycle() != MemoryLifecycle::Active {
        let decision = evaluate_memory_projection(record, None, &[])
            .map_err(|source| LocalMemoryRevalidationError::ProjectionPolicy { source })?;
        return Ok(PreparedProjectionRecordKind::Evaluated {
            revision: version.revision(),
            decision,
            evidence: Vec::new(),
        });
    }
    let validity = project_validity(
        record,
        target,
        queries,
        head,
        query_budget,
        cancelled,
        deadline,
    )?;
    if validity != MemoryProjectValidity::Valid {
        let decision = evaluate_memory_projection(record, Some(validity), &[])
            .map_err(|source| LocalMemoryRevalidationError::ProjectionPolicy { source })?;
        return Ok(PreparedProjectionRecordKind::Evaluated {
            revision: version.revision(),
            decision,
            evidence: Vec::new(),
        });
    }

    let mut outcomes = Vec::with_capacity(record.evidence().len());
    let mut evidence_results = Vec::with_capacity(record.evidence().len());
    if source.has_complete_index_coverage() {
        for (evidence_ordinal, evidence) in record.evidence().iter().enumerate() {
            check_control(cancelled.as_ref(), deadline)?;
            let MemoryEvidence::RustSymbol(evidence) = evidence;
            let evidence_ordinal = u8::try_from(evidence_ordinal)
                .map_err(|_| LocalMemoryRevalidationError::CountNotRepresentable)?;
            let (outcome, prepared) = evaluate_rust_evidence(
                writer,
                source,
                record.header().record_id(),
                version.revision(),
                evidence_ordinal,
                version.approval_git_source(),
                evidence,
                queries,
                head,
                query_budget,
                cancelled,
                deadline,
            )?;
            outcomes.push(outcome);
            evidence_results.push(prepared);
        }
    } else {
        for _ in record.evidence() {
            outcomes.push(MemoryEvidenceOutcome::Indeterminate);
            evidence_results.push(PreparedProjectionEvidence::indeterminate(0));
        }
    }
    reject_automatic_many_to_one(&mut outcomes, &mut evidence_results)?;
    let decision = evaluate_memory_projection(record, Some(validity), &outcomes)
        .map_err(|source| LocalMemoryRevalidationError::ProjectionPolicy { source })?;
    Ok(PreparedProjectionRecordKind::Evaluated {
        revision: version.revision(),
        decision,
        evidence: evidence_results,
    })
}

fn reject_automatic_many_to_one(
    outcomes: &mut [MemoryEvidenceOutcome],
    evidence: &mut [PreparedProjectionEvidence],
) -> Result<(), LocalMemoryRevalidationError> {
    if outcomes.len() != evidence.len() {
        return Err(LocalMemoryRevalidationError::ProjectionPreparation {
            source: SqliteStoreError::InvalidMemoryProjection,
        });
    }
    let mut merged = Vec::new();
    for (ordinal, result) in evidence.iter().enumerate() {
        if result.assurance != ProjectionEvidenceAssurance::Automatic {
            continue;
        }
        let Some(target) = result.target.as_ref() else {
            continue;
        };
        if evidence.iter().enumerate().any(|(other_ordinal, other)| {
            other_ordinal != ordinal && other.target.as_ref() == Some(target)
        }) {
            merged.push((ordinal, target.clone()));
        }
    }
    for (ordinal, target) in merged {
        outcomes[ordinal] = MemoryEvidenceOutcome::NeedsReview;
        evidence[ordinal] =
            PreparedProjectionEvidence::ambiguous(vec![PreparedProjectionCandidate {
                occurrence: target,
                relation: ProjectionCandidateRelation::Merged,
            }])
            .map_err(|source| LocalMemoryRevalidationError::ProjectionPreparation { source })?;
    }
    Ok(())
}

fn project_validity(
    record: &MemoryRecord,
    target: MemoryRevalidationTarget,
    queries: Option<&GitMemoryQueries>,
    head: Option<MemoryCommitId>,
    query_budget: &mut GitQueryBudget,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> Result<MemoryProjectValidity, LocalMemoryRevalidationError> {
    let mut checks = Vec::new();
    if let (
        MemoryValidity::Commits {
            introduced_by,
            invalidated_by,
        },
        Some(descendant),
    ) = (record.validity(), head)
    {
        let queries = queries.ok_or(LocalMemoryRevalidationError::JournalIntegrity)?;
        checks.reserve(introduced_by.len().saturating_add(invalidated_by.len()));
        for ancestor in introduced_by.iter().chain(invalidated_by) {
            query_budget.reserve()?;
            let outcome = queries
                .is_ancestor(*ancestor, descendant, cancelled.as_ref(), deadline)
                .map_err(|source| LocalMemoryRevalidationError::GitQuery { source })?;
            checks.push(MemoryAncestryCheck::new(*ancestor, descendant, outcome));
        }
    }
    evaluate_memory_project_validity(record.validity(), target, &checks)
        .map_err(|source| LocalMemoryRevalidationError::ValidityEvaluation { source })
}

#[allow(
    clippy::too_many_arguments,
    reason = "one evidence result retains exact source, history attribution, and controls"
)]
fn capture_bounded_source_state(
    worktree: &Path,
    limits: GitPathDiscoveryLimits,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> Result<CapturedSourceState, LocalMemoryRevalidationError> {
    check_control(cancelled.as_ref(), deadline)?;
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or(LocalMemoryRevalidationError::DeadlineExceeded)?;
    let bounded = GitPathDiscoveryLimits::new(
        limits.deadline().min(remaining),
        limits.output_bytes(),
        limits.paths(),
        limits.repository_path(),
    );
    capture_source_state_with_cancel(worktree, bounded, || cancelled.load(Ordering::Acquire))
        .map_err(|source| LocalMemoryRevalidationError::SourceState { source })
}

fn validated_limits(
    limits: LocalMemoryRevalidationLimits,
) -> Result<(MemoryProjectionLoadLimits, MemoryProjectionResultLimits), LocalMemoryRevalidationError>
{
    if limits.deadline().is_zero()
        || limits.source_state().deadline().is_zero()
        || !(2..=MAX_LOCAL_MEMORY_GIT_QUERIES).contains(&limits.max_git_queries())
    {
        return Err(LocalMemoryRevalidationError::InvalidLimits);
    }
    let load =
        MemoryProjectionLoadLimits::try_new(limits.max_versions(), limits.max_canonical_bytes())
            .map_err(|_| LocalMemoryRevalidationError::InvalidLimits)?;
    let result = MemoryProjectionResultLimits::try_new(limits.max_result_candidates())
        .map_err(|_| LocalMemoryRevalidationError::InvalidLimits)?;
    Ok((load, result))
}

fn check_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalMemoryRevalidationError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalMemoryRevalidationError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(LocalMemoryRevalidationError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

fn map_database_path_error(error: LocalIndexError) -> LocalMemoryRevalidationError {
    match error {
        LocalIndexError::DatabaseInsideWorktree => {
            LocalMemoryRevalidationError::DatabaseInsideWorktree
        }
        LocalIndexError::DatabaseHasMultipleLinks => {
            LocalMemoryRevalidationError::DatabaseHasMultipleLinks
        }
        LocalIndexError::DatabaseChangedDuringIndexing => {
            LocalMemoryRevalidationError::DatabaseChangedDuringRevalidation
        }
        _ => LocalMemoryRevalidationError::DatabasePathUnavailable,
    }
}

fn map_store_startup_error(source: SqliteStoreError) -> LocalMemoryRevalidationError {
    if source == SqliteStoreError::DatabaseIdentityChanged {
        LocalMemoryRevalidationError::DatabaseChangedDuringRevalidation
    } else {
        LocalMemoryRevalidationError::StoreStartup { source }
    }
}

struct GitQueryBudget {
    used: u32,
    limit: u32,
}

impl GitQueryBudget {
    const fn new(limit: u32) -> Self {
        Self { used: 0, limit }
    }

    fn reserve(&mut self) -> Result<(), LocalMemoryRevalidationError> {
        self.used = self
            .used
            .checked_add(1)
            .filter(|used| *used <= self.limit)
            .ok_or(LocalMemoryRevalidationError::GitQueryLimitExceeded)?;
        Ok(())
    }

    const fn used(&self) -> u32 {
        self.used
    }
}
