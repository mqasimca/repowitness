#[allow(
    clippy::too_many_lines,
    reason = "the single-owner loop is one exhaustive bounded command dispatcher"
)]
fn run_writer(state: &mut WriterState, receiver: Receiver<WriterCommand>) {
    while let Ok(command) = receiver.recv() {
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
                send_reply(reply, result);
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
                send_reply(reply, result);
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
                send_reply(reply, result);
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
                send_reply(reply, result);
            }
            WriterCommand::AppendMemoryCorrespondenceReview(command) => {
                let AppendMemoryCorrespondenceReviewCommand {
                    prepared,
                    cancelled,
                    deadline,
                    reply,
                } = *command;
                let result = state.append_memory_correspondence_review(
                    &prepared,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_reply(reply, result);
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
                let result = state.publish_memory_projection(
                    &prepared,
                    WriteControl {
                        cancelled: &cancelled,
                        deadline,
                    },
                );
                send_reply(reply, result);
            }
            WriterCommand::Activate {
                generation,
                expected_source_epoch,
                deadline,
                reply,
            } => {
                let result = check_deadline(deadline)
                    .and_then(|()| state.activate(generation, expected_source_epoch));
                send_reply(reply, result);
            }
            WriterCommand::ActiveGeneration {
                repository,
                deadline,
                reply,
            } => {
                let result =
                    check_deadline(deadline).and_then(|()| state.active_generation(repository));
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
                send_reply(reply, result);
            }
            WriterCommand::Checkpoint { deadline, reply } => {
                let result = check_deadline(deadline).and_then(|()| state.checkpoint());
                send_reply(reply, result);
            }
            WriterCommand::Shutdown { reply } => {
                send_reply(reply, Ok(()));
                break;
            }
        }
    }
}

fn send_reply<T>(reply: Reply<T>, result: Result<T, SqliteStoreError>) {
    let _ = reply.try_send(result);
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
