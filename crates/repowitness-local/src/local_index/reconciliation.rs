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
            LocalIndexPhase::WriterStarted | LocalIndexPhase::PublicationCommitted => {}
        },
        |_, deadline| deadline,
    )
}

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

    let (publication, report_input) =
        prepare_local_index_publication(LocalIndexPublicationPreparationContext {
            worktree: &worktree,
            database: &database,
            database_identity: database_identity.as_ref(),
            repository,
            configuration_digest: configuration.digest(),
            languages,
            limits: configured_limits,
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
    after_phase(LocalIndexPhase::WriterStarted);
    let publication_database_identity = database_alias_identity(&database)?;
    if publication_database_identity.as_ref() != Some(writer.opened_database_identity()) {
        return Err(LocalIndexError::DatabaseChangedDuringIndexing);
    }
    let final_fence = LocalSourceSlotFinalFence::new(
        &worktree,
        &database,
        Some(writer.opened_database_identity()),
        publication.identity,
        languages,
        configured_limits,
    );
    let publication = if skip_unchanged {
        reconcile_prepared_local_index(
            &writer,
            repository,
            publication,
            &final_fence,
            || after_phase(LocalIndexPhase::GraphStaged),
            &cancelled,
            deadline,
            startup.recovered_generations(),
            report_input,
        )?
    } else {
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
    let committed = !matches!(publication, LocalReconciliationOutcome::Unchanged(_));
    if committed {
        after_phase(LocalIndexPhase::PublicationCommitted);
    }
    let _maintenance =
        post_commit::finish_index_writer(writer, committed, deadline, maintenance_deadline);

    Ok(publication)
}

#[allow(
    clippy::too_many_arguments,
    reason = "the provisional watched seam keeps publication, fencing, control, and reports explicit"
)]
fn reconcile_prepared_local_index(
    writer: &OwnedSqliteIndex,
    repository: repowitness_domain::RepositoryIdentityDigest,
    publication: PreparedLocalIndexPublication,
    final_fence: &LocalSourceSlotFinalFence<'_>,
    after_graph_staging: impl FnOnce(),
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
    recovered_generations: u64,
    report_input: ReportInput,
) -> Result<LocalReconciliationOutcome, LocalIndexError> {
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
    let candidate =
        hash_source_snapshot(publication.identity, publication.prepared.manifest_digest());

    if let Some(active) = state
        .active()
        .filter(|active| active.snapshot() == candidate)
    {
        final_fence
            .confirm_source_snapshot(candidate, Arc::clone(cancelled), deadline)
            .map_err(|source| LocalIndexError::FinalSourceFence { source })?;
        return Ok(LocalReconciliationOutcome::Unchanged(activated_report(
            active.generation(),
            active.source_epoch().get(),
            recovered_generations,
            report_input,
        )));
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
        return Ok(LocalReconciliationOutcome::Resumed(activated_report(
            completion.generation(),
            completion.source_epoch().get(),
            recovered_generations,
            report_input,
        )));
    }

    let (generation, source_epoch) = publish_prepared_local_index_at_epoch(
        writer,
        repository,
        persisted_epoch,
        publication,
        final_fence,
        after_graph_staging,
        cancelled,
        deadline,
    )?;
    Ok(LocalReconciliationOutcome::Published(activated_report(
        generation,
        source_epoch.get(),
        recovered_generations,
        report_input,
    )))
}
