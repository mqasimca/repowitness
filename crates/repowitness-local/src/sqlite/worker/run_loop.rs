#[allow(
    clippy::too_many_lines,
    reason = "the single-owner loop is one exhaustive bounded command dispatcher"
)]
fn run_writer(
    state: &mut WriterState,
    receiver: Receiver<WriterCommand>,
    hooks: &mut WriterHooks,
    unresolved_mutation: &AtomicBool,
) {
    while let Ok(command) = receiver.recv() {
        if command.is_mutating() && unresolved_mutation.load(Ordering::Acquire) {
            command.reject_unresolved_mutation();
            continue;
        }
        match command {
            WriterCommand::Register {
                repository,
                initial_source_epoch,
                deadline,
                reply,
            } => {
                let result = check_deadline(deadline).and_then(|()| {
                    state
                        .register_workspace(repository, initial_source_epoch)
                        .map(|_| ())
                });
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::EnsureWorkspace {
                repository,
                initial_source_epoch,
                deadline,
                reply,
            } => {
                let result = check_deadline(deadline).and_then(|()| {
                    state
                        .ensure_workspace(repository, initial_source_epoch)
                        .map(|(_, source_epoch)| source_epoch)
                });
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::AdvanceEpoch {
                repository,
                expected,
                next,
                deadline,
                reply,
            } => {
                let result = check_deadline(deadline)
                    .and_then(|()| state.advance_source_epoch(repository, expected, next));
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::Stage(command) => {
                let StageCommand {
                    source_epoch,
                    identity,
                    prepared,
                    coverage,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.stage(
                    source_epoch,
                    identity,
                    &prepared,
                    coverage,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::StageGraph(command) => {
                let StageGraphCommand {
                    generation,
                    prepared,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.stage_graph(
                    generation,
                    &prepared,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::StageScipOverlay(command) => {
                let StageScipOverlayCommand {
                    connected_workspace,
                    workspace_view,
                    source_slot,
                    require_active_view,
                    prepared,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.stage_scip_overlay(
                    connected_workspace,
                    workspace_view,
                    source_slot,
                    &prepared,
                    require_active_view,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::StageSourceSlot(command) => {
                let StageSourceSlotCommand {
                    connected_workspace,
                    source_slot,
                    reserved_epoch,
                    identity,
                    prepared,
                    coverage,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.stage_source_slot(
                    SourceSlotReservation::new(connected_workspace, source_slot, reserved_epoch),
                    identity,
                    &prepared,
                    coverage,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::ImportMemory(command) => {
                let MemoryImportCommand {
                    prepared,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.import_memory_version(
                    &prepared,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::SyncTeamMemory(command) => {
                let MemoryImportCommand {
                    prepared,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.sync_team_memory(
                    &prepared,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::ImportObservedMemoryHistory(command) => {
                let ObservedMemoryHistoryCommand {
                    repository,
                    prepared,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.import_observed_memory_history(
                    repository,
                    &prepared,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::AppendMemoryCorrespondenceReview(command) => {
                let AppendMemoryCorrespondenceReviewCommand {
                    prepared,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let force_progress_handler_clear_failure =
                    hooks.take_mutation_progress_handler_clear_failure();
                let outcome = state.append_memory_correspondence_review(
                    &prepared,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                    force_progress_handler_clear_failure,
                );
                if !send_progress_managed_mutation_reply(
                    reply,
                    outcome,
                    hooks,
                    unresolved_mutation,
                ) {
                    break;
                }
            }
            WriterCommand::AppendTaskCheckpoint(command) => {
                let TaskCheckpointCommand {
                    checkpoint,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.append_task_checkpoint(
                    &checkpoint,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::AppendPersonalMemory(command) => {
                let PersonalMemoryCommand {
                    record,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.append_personal_memory(
                    &record,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::TaskStatus(command) => {
                let TaskStatusCommand {
                    repository,
                    task_id,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.task_status(
                    repository,
                    task_id,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                let _ = reply.try_send(result);
            }
            WriterCommand::AppendTaskVerification(command) => {
                let TaskVerificationCommand {
                    verification,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.append_task_verification(
                    &verification,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::LoadMemorySource {
                repository,
                cancelled,
                deadline,
                reply,
            } => {
                let result = state.load_memory_source(
                    repository,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_reply(reply, result);
            }
            WriterCommand::LoadMemoryJournal(command) => {
                let LoadMemoryJournalCommand {
                    repository,
                    limits,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.load_memory_journal(
                    repository,
                    limits,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_reply(reply, result);
            }
            WriterCommand::LoadRustMemoryCandidates(command) => {
                let LoadRustMemoryCandidatesCommand {
                    source,
                    evidence,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.load_rust_memory_candidates(
                    source,
                    &evidence,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_reply(reply, result);
            }
            WriterCommand::LoadMemoryCorrespondenceReviews(command) => {
                let LoadMemoryCorrespondenceReviewsCommand {
                    source,
                    record_id,
                    revision,
                    evidence_ordinal,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.load_memory_correspondence_reviews(
                    source,
                    record_id,
                    revision,
                    evidence_ordinal,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_reply(reply, result);
            }
            WriterCommand::PublishMemoryProjection(command) => {
                let PublishMemoryProjectionCommand {
                    prepared,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let force_progress_handler_clear_failure =
                    hooks.take_mutation_progress_handler_clear_failure();
                let outcome = state.publish_memory_projection(
                    &prepared,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                    force_progress_handler_clear_failure,
                );
                if !send_progress_managed_mutation_reply(
                    reply,
                    outcome,
                    hooks,
                    unresolved_mutation,
                ) {
                    break;
                }
            }
            WriterCommand::Activate {
                generation,
                expected_source_epoch,
                deadline,
                reply,
            } => {
                let result = check_deadline(deadline)
                    .and_then(|()| state.activate(generation, expected_source_epoch, deadline));
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::ActiveGeneration {
                repository,
                deadline,
                reply,
            } => {
                let result =
                    check_deadline(deadline).and_then(|()| state.active_generation(repository));
                hooks.before_read_reply();
                send_reply(reply, result);
            }
            WriterCommand::ConnectWorkspace(command) => {
                let ConnectWorkspaceCommand {
                    connected_workspace,
                    source_slots,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.connect_workspace(
                    connected_workspace,
                    &source_slots,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::SourceSlotState(command) => {
                let SourceSlotStateCommand {
                    connected_workspace,
                    source_slot,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.source_slot_state(
                    connected_workspace,
                    source_slot,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_reply(reply, result);
            }
            WriterCommand::ReserveSourceSlotEpoch(command) => {
                let ReserveSourceSlotEpochCommand {
                    connected_workspace,
                    source_slot,
                    expected,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.reserve_source_slot_epoch(
                    connected_workspace,
                    source_slot,
                    expected,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::CompleteSourceSlotEpoch(command) => {
                let CompleteSourceSlotEpochCommand {
                    connected_workspace,
                    source_slot,
                    source_epoch,
                    generation,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.complete_source_slot_epoch(
                    connected_workspace,
                    source_slot,
                    source_epoch,
                    generation,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::PublishWorkspaceView(command) => {
                let PublishWorkspaceViewCommand {
                    connected_workspace,
                    members,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.publish_workspace_view(
                    connected_workspace,
                    &members,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::ActiveWorkspaceView {
                connected_workspace,
                cancelled,
                deadline,
                reply,
            } => {
                let result = state.active_workspace_view(
                    connected_workspace,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_reply(reply, result);
            }
            WriterCommand::RebuildProjection(command) => {
                let RebuildProjectionCommand {
                    limits,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.rebuild_search_projection(
                    limits,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::PlanRetention(command) => {
                let PlanRetentionCommand {
                    policy,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.plan_generation_retention(&policy, cancelled, deadline);
                send_reply(reply, result);
            }
            WriterCommand::ApplyRetention(command) => {
                let ApplyRetentionCommand {
                    policy,
                    expected_plan,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result =
                    state.apply_generation_retention(&policy, expected_plan, cancelled, deadline);
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::Checkpoint { deadline, reply } => {
                let result = check_deadline(deadline).and_then(|()| state.checkpoint());
                send_mutation_reply(reply, result, hooks, unresolved_mutation);
            }
            WriterCommand::Shutdown { reply } => {
                send_reply(reply, Ok(()));
                hooks.after_shutdown_reply();
                break;
            }
        }
    }
}

fn send_reply<T>(reply: Reply<T>, result: Result<T, SqliteStoreError>) {
    let _ = reply.try_send(result);
}

fn send_mutation_reply<T>(
    reply: Reply<T>,
    result: Result<T, SqliteStoreError>,
    hooks: &mut WriterHooks,
    unresolved_mutation: &AtomicBool,
) {
    hooks.after_commit_before_reply(&result);
    if reply.try_send(result).is_err() {
        unresolved_mutation.store(true, Ordering::Release);
    }
}

fn send_progress_managed_mutation_reply<T>(
    reply: Reply<T>,
    outcome: WriterMutationResult<T>,
    hooks: &mut WriterHooks,
    unresolved_mutation: &AtomicBool,
) -> bool {
    let (result, connection_usable) = outcome.into_parts();
    send_mutation_reply(reply, result, hooks, unresolved_mutation);
    connection_usable
}

fn receive_reply<T>(
    receiver: &Receiver<Result<T, SqliteStoreError>>,
    deadline: Instant,
) -> Result<T, SqliteStoreError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(SqliteStoreError::DeadlineExceeded);
    }
    receiver
        .recv_timeout(remaining)
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => SqliteStoreError::ReplyTimeout,
            mpsc::RecvTimeoutError::Disconnected => SqliteStoreError::WorkerUnavailable,
        })?
}

/// Bounded resolution for a command that may have crossed a durable commit point.
///
/// A command accepted by the owner queue can finish after the caller's ordinary
/// deadline. Once that deadline passes, request cancellation where supported and
/// allow one fixed grace interval for its receipt. A missing receipt is not a
/// rollback or ordinary timeout: its durable outcome is unknown and must not be
/// retried implicitly.
fn receive_mutation_reply<T>(
    receiver: &Receiver<Result<T, SqliteStoreError>>,
    cancelled: Option<&AtomicBool>,
    deadline: Instant,
    unresolved_mutation: Option<&AtomicBool>,
) -> Result<T, SqliteStoreError> {
    const OUTCOME_RESOLUTION_GRACE: Duration = Duration::from_millis(250);

    let remaining = deadline.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        match receiver.recv_timeout(remaining) {
            Ok(reply) => return record_mutation_outcome(reply, unresolved_mutation),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return record_mutation_outcome(
                    Err(SqliteStoreError::MutationOutcomeUnknown),
                    unresolved_mutation,
                );
            }
        }
    }

    if let Some(cancelled) = cancelled {
        cancelled.store(true, Ordering::Release);
    }
    let outcome = receiver
        .recv_timeout(OUTCOME_RESOLUTION_GRACE)
        .unwrap_or(Err(SqliteStoreError::MutationOutcomeUnknown));
    record_mutation_outcome(outcome, unresolved_mutation)
}

fn record_mutation_outcome<T>(
    outcome: Result<T, SqliteStoreError>,
    unresolved_mutation: Option<&AtomicBool>,
) -> Result<T, SqliteStoreError> {
    if matches!(&outcome, Err(SqliteStoreError::MutationOutcomeUnknown))
        && let Some(unresolved_mutation) = unresolved_mutation
    {
        unresolved_mutation.store(true, Ordering::Release);
    }
    outcome
}

fn check_deadline(deadline: Instant) -> Result<(), SqliteStoreError> {
    if Instant::now() >= deadline {
        Err(SqliteStoreError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "each semantic and audit identity remains explicit before queueing"
)]
fn prepare_memory_import(
    repository: RepositoryIdentityDigest,
    record: MemoryRecord,
    presentation: MemoryPresentationDigest,
    source: MemoryObservationSource,
    audit_actor: MemoryAuditActorId,
    recorded_at: MemoryRecordedAtUnixMillis,
    approval: MemoryImportApproval,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<PreparedMemoryImport, SqliteStoreError> {
    check_memory_control(cancelled, deadline)?;
    if record.scope().repository() != repository {
        return Err(SqliteStoreError::InvalidMemoryImport);
    }
    let canonical_json =
        canonical_memory_json(&record, MemoryFormatControl::new(cancelled, deadline))
            .map_err(map_memory_format_error)?;
    check_memory_control(cancelled, deadline)?;
    let revision = digest_canonical_bytes(&canonical_json).map_err(map_memory_format_error)?;
    check_memory_control(cancelled, deadline)?;
    Ok(PreparedMemoryImport::new(
        repository,
        record,
        canonical_json,
        revision,
        presentation,
        source,
        audit_actor,
        recorded_at,
        approval,
    ))
}

fn map_memory_format_error(error: MemoryFormatError) -> SqliteStoreError {
    match error {
        MemoryFormatError::Cancelled => SqliteStoreError::Cancelled,
        MemoryFormatError::DeadlineExceeded => SqliteStoreError::DeadlineExceeded,
        MemoryFormatError::InputTooLarge
        | MemoryFormatError::InvalidYaml
        | MemoryFormatError::InvalidRecord(_)
        | MemoryFormatError::InvalidCanonicalRecord
        | MemoryFormatError::CanonicalizationFailed
        | MemoryFormatError::GenerationFailed => SqliteStoreError::InvalidMemoryImport,
    }
}

fn check_memory_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), SqliteStoreError> {
    if cancelled.load(Ordering::Acquire) {
        Err(SqliteStoreError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(SqliteStoreError::DeadlineExceeded)
    } else {
        Ok(())
    }
}
