#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalReconciliationOutcome {
    Published(LocalIndexReport),
    Resumed(LocalIndexReport),
    Unchanged(LocalIndexReport),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalIndexPhase {
    MutationLeaseAcquired,
    WriterStarted,
    GraphProjectionPreparing,
    GraphStaged,
    PublicationCommitted,
}

fn index_local_repository_with_mode(
    request: LocalIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
    skip_unchanged: bool,
    after_lease: impl FnOnce(),
    after_graph_staging: impl FnOnce(),
) -> Result<LocalReconciliationOutcome, LocalIndexError> {
    let mut after_lease = Some(after_lease);
    let mut after_graph_staging = Some(after_graph_staging);
    index_local_repository_with_mode_and_control(
        request,
        cancelled,
        skip_unchanged,
        move |phase| match phase {
            LocalIndexPhase::MutationLeaseAcquired => {
                if let Some(hook) = after_lease.take() {
                    hook();
                }
            }
            LocalIndexPhase::GraphStaged => {
                if let Some(hook) = after_graph_staging.take() {
                    hook();
                }
            }
            LocalIndexPhase::WriterStarted
            | LocalIndexPhase::GraphProjectionPreparing
            | LocalIndexPhase::PublicationCommitted => {}
        },
        |_, deadline| deadline,
    )
}

#[allow(
    clippy::too_many_lines,
    reason = "the explicit indexing lifecycle keeps cleanup after every writer-owned outcome visible"
)]
fn index_local_repository_with_mode_and_control(
    request: LocalIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
    skip_unchanged: bool,
    mut after_phase: impl FnMut(LocalIndexPhase),
    maintenance_deadline: impl FnMut(post_commit::PostCommitMaintenancePhase, Instant) -> Instant,
) -> Result<LocalReconciliationOutcome, LocalIndexError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(|source| LocalIndexError::RepositoryIdentity { source })?;
    let configuration = resolved_index_configuration(request.configuration)?;
    let (configured_limits, languages) =
        configured_index_inputs(request.limits, configuration.as_ref())?;
    let deadline = Instant::now()
        .checked_add(configured_limits.deadline())
        .ok_or(LocalIndexError::DeadlineNotRepresentable)?;
    if cancelled.load(Ordering::Relaxed) {
        return Err(LocalIndexError::Preparation {
            source: LocalRustIndexError::Cancelled,
        });
    }
    let worktree = discovered_worktree_root(request.repository_root).map_err(|source| {
        LocalIndexError::Preparation {
            source: LocalRustIndexError::Discovery { source },
        }
    })?;
    let database = validated_database_outside_worktree(&worktree, request.database)?;
    let mutation_lease = SqliteMutationLease::acquire(&database, deadline)
        .map_err(|source| LocalIndexError::StoreStartup { source })?;
    let database_identity = database_alias_identity(&database)?;
    after_phase(LocalIndexPhase::MutationLeaseAcquired);
    let identity_after_lease = database_alias_identity(&database)?;
    if identity_after_lease != database_identity {
        return Err(LocalIndexError::DatabaseChangedDuringIndexing);
    }
    drop(identity_after_lease);

    let source = prepare_local_index_source(LocalIndexPublicationPreparationContext {
            worktree: &worktree,
            database: &database,
            database_identity: database_identity.as_ref(),
            repository,
            configuration_digest: configuration.digest(),
            languages,
            limits: configured_limits,
            build_graph: request.build_graph(),
            cancelled: &cancelled,
            deadline,
        })?;

    let (writer, startup) = OwnedSqliteIndex::start_with_lease(
        mutation_lease,
        database_identity,
        request.migration_applied_at_unix_ms,
        Arc::clone(&cancelled),
        deadline,
    )
    .map_err(map_store_startup_error)?;
    let result = (|| {
        after_phase(LocalIndexPhase::WriterStarted);
        let publication_database_identity = database_alias_identity(&database)?;
        if publication_database_identity.as_ref() != Some(writer.opened_database_identity()) {
            return Err(LocalIndexError::DatabaseChangedDuringIndexing);
        }
        let final_fence = LocalSourceSlotFinalFence::new(
            &worktree,
            &database,
            Some(writer.opened_database_identity()),
            source.identity,
            languages,
            configured_limits,
        );
        let publication = if skip_unchanged {
            match reconcile_prepared_local_source(
                &writer,
                repository,
                &source,
                &final_fence,
                &cancelled,
                deadline,
                startup.recovered_generations(),
            )? {
                ReconciliationDecision::Complete(outcome) => outcome,
                ReconciliationDecision::PublishAt(source_epoch) => {
                    after_phase(LocalIndexPhase::GraphProjectionPreparing);
                    let report_input = source.report_input;
                    let publication = prepare_local_index_publication(
                        source,
                        repository,
                        cancelled.as_ref(),
                        deadline,
                    )?;
                    publish_prepared_local_index_at_epoch(
                        &writer,
                        repository,
                        source_epoch,
                        publication,
                        &final_fence,
                        || after_phase(LocalIndexPhase::GraphStaged),
                        &cancelled,
                        deadline,
                    )
                    .map(|(generation, source_epoch)| {
                        LocalReconciliationOutcome::Published(activated_report(
                            generation,
                            source_epoch.get(),
                            startup.recovered_generations(),
                            report_input,
                        ))
                    })?
                }
            }
        } else {
            after_phase(LocalIndexPhase::GraphProjectionPreparing);
            let report_input = source.report_input;
            let publication = prepare_local_index_publication(
                source,
                repository,
                cancelled.as_ref(),
                deadline,
            )?;
            publish_prepared_local_index(
                &writer,
                repository,
                publication,
                &final_fence,
                || after_phase(LocalIndexPhase::GraphStaged),
                &cancelled,
                deadline,
            )
            .map(|(generation, source_epoch)| {
                LocalReconciliationOutcome::Published(activated_report(
                    generation,
                    source_epoch.get(),
                    startup.recovered_generations(),
                    report_input,
                ))
            })?
        };
        if !matches!(publication, LocalReconciliationOutcome::Unchanged(_)) {
            after_phase(LocalIndexPhase::PublicationCommitted);
        }
        Ok(publication)
    })();
    let committed = result
        .as_ref()
        .is_ok_and(|publication| !matches!(publication, LocalReconciliationOutcome::Unchanged(_)));
    let _maintenance =
        post_commit::finish_index_writer(writer, committed, deadline, maintenance_deadline);

    result
}

#[allow(
    clippy::too_many_arguments,
    reason = "the provisional watched seam keeps publication, fencing, control, and reports explicit"
)]
enum ReconciliationDecision {
    Complete(LocalReconciliationOutcome),
    PublishAt(SourceSlotEpoch),
}

fn reconcile_prepared_local_source(
    writer: &OwnedSqliteIndex,
    repository: repowitness_domain::RepositoryIdentityDigest,
    source: &PreparedLocalIndexSource,
    final_fence: &LocalSourceSlotFinalFence<'_>,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
    recovered_generations: u64,
) -> Result<ReconciliationDecision, LocalIndexError> {
    let persisted_epoch = writer
        .ensure_workspace(repository, INITIAL_SOURCE_EPOCH, deadline)
        .map_err(|source| {
            map_index_mutation_error(
                LocalIndexMutation::WorkspaceRegistration,
                source,
                |source| LocalIndexError::WorkspaceRegistration { source },
            )
        })?;
    let connected_workspace =
        repowitness_domain::ConnectedWorkspaceId::for_single_repository(repository);
    let source_slot = repowitness_domain::SourceSlotId::for_repository(repository);
    let state = writer
        .source_slot_state(
            connected_workspace,
            source_slot,
            Arc::clone(cancelled),
            deadline,
        )
        .map_err(|source| LocalIndexError::WorkspaceRegistration { source })?;
    let candidate = hash_source_snapshot(
        source.identity,
        source.preparation.prepared().manifest_digest(),
    );

    if let Some(active) = state
        .active()
        .filter(|active| active.snapshot() == candidate)
    {
        final_fence
            .confirm_source_snapshot(candidate, Arc::clone(cancelled), deadline)
            .map_err(|source| LocalIndexError::FinalSourceFence { source })?;
        return Ok(ReconciliationDecision::Complete(
            LocalReconciliationOutcome::Unchanged(activated_report(
            active.generation(),
            active.source_epoch().get(),
            recovered_generations,
            source.report_input,
        )),
        ));
    }

    if let Some(completion) = state
        .current_completion()
        .filter(|completion| completion.snapshot() == candidate)
    {
        final_fence
            .confirm_source_snapshot(candidate, Arc::clone(cancelled), deadline)
            .map_err(|source| LocalIndexError::FinalSourceFence { source })?;
        writer
            .activate(
                completion.generation(),
                completion.source_epoch().get(),
                deadline,
            )
            .map_err(|source| {
                map_index_mutation_error(
                    LocalIndexMutation::GenerationPublication,
                    source,
                    |source| LocalIndexError::PublicationActivation { source },
                )
            })?;
        return Ok(ReconciliationDecision::Complete(
            LocalReconciliationOutcome::Resumed(activated_report(
            completion.generation(),
            completion.source_epoch().get(),
            recovered_generations,
            source.report_input,
        )),
        ));
    }

    Ok(ReconciliationDecision::PublishAt(persisted_epoch))
}
