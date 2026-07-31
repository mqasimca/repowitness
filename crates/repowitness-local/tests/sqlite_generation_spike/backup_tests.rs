#[test]
fn online_backup_restores_committed_wal_state_and_checkpoint_truncates_it() -> TestResult {
    let directory = TempDirectory::new()?;
    let database_path = directory.join("source.sqlite3");
    let backup_path = directory.join("backup.sqlite3");
    let mut source = open_file_database(&database_path)?;
    bootstrap_workspace(&source)?;
    stage_ready_generation(&mut source, 1, 1, &fact_fixture(128, 256), 17)?;
    activate_generation(&mut source, 1, 1)?;

    let source_wal_path = wal_path(&database_path);
    assert!(
        fs::metadata(&source_wal_path)?.len() > 0,
        "committed data must remain in WAL before the explicit checkpoint"
    );
    backup_database(&source, &backup_path)?;

    let restored = open_read_database(&backup_path)?;
    assert_eq!(
        restored.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))?,
        "ok"
    );
    assert_eq!(active_generation_id(&restored)?, Some(1));
    assert_eq!(generation_facts(&restored, 1)?, fact_fixture(128, 256));
    drop(restored);

    let (busy, log_frames, checkpointed_frames) = truncate_checkpoint(&source)?;
    assert_eq!(busy, 0);
    assert_eq!(log_frames, checkpointed_frames);
    assert_eq!(fs::metadata(source_wal_path)?.len(), 0);
    Ok(())
}

#[test]
fn online_backup_interleaves_with_writes_and_restores_recoverable_state() -> TestResult {
    let directory = TempDirectory::new()?;
    let database_path = directory.join("source.sqlite3");
    let backup_path = directory.join("backup.sqlite3");
    let generation_one_facts = fact_fixture(1_024, 256);
    let generation_two_facts = fact_fixture(768, 192);
    let incomplete_facts = fact_fixture(64, 128);
    let mut writer = open_file_database(&database_path)?;
    bootstrap_workspace(&writer)?;
    stage_ready_generation(&mut writer, 1, 1, &generation_one_facts, 64)?;
    activate_generation(&mut writer, 1, 1)?;

    let (step_sender, step_receiver) = mpsc::sync_channel(0);
    let (resume_sender, resume_receiver) = mpsc::sync_channel(0);
    let backup_source_path = database_path.clone();
    let backup_destination_path = backup_path.clone();
    let backup_thread = thread::spawn(move || -> Result<(), String> {
        let source = open_read_database(&backup_source_path).map_err(|error| error.to_string())?;
        let mut destination =
            Connection::open(&backup_destination_path).map_err(|error| error.to_string())?;
        let backup = Backup::new(&source, &mut destination).map_err(|error| error.to_string())?;

        let first_step = backup.step(1).map_err(|error| error.to_string())?;
        if first_step != rusqlite::backup::StepResult::More {
            return Err(format!(
                "expected an incomplete first backup step, got {first_step:?}"
            ));
        }
        step_sender.send(()).map_err(|error| error.to_string())?;
        resume_receiver.recv().map_err(|error| error.to_string())?;

        let second_step = backup.step(1).map_err(|error| error.to_string())?;
        if second_step != rusqlite::backup::StepResult::More {
            return Err(format!(
                "expected an incomplete second backup step, got {second_step:?}"
            ));
        }
        step_sender.send(()).map_err(|error| error.to_string())?;
        resume_receiver.recv().map_err(|error| error.to_string())?;

        backup
            .run_to_completion(1, Duration::from_millis(1), None)
            .map_err(|error| error.to_string())
    });

    step_receiver.recv()?;
    stage_ready_generation(&mut writer, 2, 2, &generation_two_facts, 64)?;
    resume_sender.send(())?;

    step_receiver.recv()?;
    activate_generation(&mut writer, 2, 2)?;
    begin_generation(&writer, 3, 3)?;
    advance_to(&writer, 3, "extracting")?;
    write_facts_in_bounded_batches(&mut writer, 3, &incomplete_facts, 16)?;
    resume_sender.send(())?;

    backup_thread
        .join()
        .map_err(|_| io::Error::other("online backup thread panicked"))?
        .map_err(io::Error::other)?;

    let mut restored = open_file_database(&backup_path)?;
    assert_eq!(
        restored.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))?,
        "ok"
    );
    assert_eq!(active_generation_id(&restored)?, Some(2));
    assert_eq!(generation_facts(&restored, 2)?, generation_two_facts);
    assert_eq!(
        generation_state(&restored, 3)?.as_deref(),
        Some("extracting")
    );
    assert_eq!(generation_facts(&restored, 3)?, incomplete_facts);

    assert_eq!(recover_incomplete_generations(&mut restored)?, 1);
    assert_eq!(active_generation_id(&restored)?, Some(2));
    assert_eq!(generation_state(&restored, 3)?.as_deref(), Some("failed"));
    assert!(generation_facts(&restored, 3)?.is_empty());
    Ok(())
}

#[test]
fn sustained_writes_bound_wal_and_cancellable_backup_never_publishes_partial_state() -> TestResult {
    let directory = TempDirectory::new()?;
    let database_path = directory.join("backup-cancellation-source.sqlite3");
    let partial_backup_path = directory.join("backup-cancellation.partial");
    let verified_backup_path = directory.join("backup-cancellation.sqlite3");
    let source_wal_path = wal_path(&database_path);
    let mut setup = open_file_database(&database_path)?;
    bootstrap_workspace(&setup)?;
    stage_ready_generation(&mut setup, 1, 1, &fact_fixture(2_048, 512), 64)?;
    activate_generation(&mut setup, 1, 1)?;
    assert_eq!(truncate_checkpoint(&setup)?, (0, 0, 0));
    drop(setup);

    let (command_sender, command_receiver) = mpsc::sync_channel(1);
    let writer_path = database_path.clone();
    let writer_worker =
        thread::spawn(move || run_owned_writer_worker(&writer_path, &command_receiver));
    let writer = OwnedWriterClient {
        commands: command_sender,
    };

    let cancelled = Arc::new(AtomicBool::new(false));
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (exit_sender, exit_receiver) = mpsc::sync_channel(1);
    let backup_source_path = database_path.clone();
    let backup_destination_path = partial_backup_path.clone();
    let backup_cancelled = Arc::clone(&cancelled);
    let backup_worker = thread::spawn(move || {
        let result = run_cancellable_backup_worker(
            &backup_source_path,
            &backup_destination_path,
            backup_cancelled.as_ref(),
            &ready_sender,
        );
        send_owned_reply(&exit_sender, result)
    });
    receive_owned_worker_result(&ready_receiver)?;

    let (cancellation_started_sender, cancellation_started_receiver) = mpsc::sync_channel(1);
    let cancellation_trigger = Arc::clone(&cancelled);
    let cancellation_worker = thread::spawn(move || {
        thread::sleep(BACKUP_CANCELLATION_TRIGGER_DELAY);
        let cancellation_started_at = Instant::now();
        cancellation_trigger.store(true, Ordering::Release);
        cancellation_started_sender
            .send(cancellation_started_at)
            .map_err(|_| "backup cancellation trigger receiver disconnected".to_owned())
    });

    let mut max_wal_bytes = 0_u64;
    let mut final_facts = Vec::new();
    for generation_id in 2_i64..=5 {
        final_facts = fact_fixture(512, 384);
        writer.publish(generation_id, generation_id, final_facts.clone(), 64)?;
        let wal_bytes = fs::metadata(&source_wal_path)?.len();
        max_wal_bytes = max_wal_bytes.max(wal_bytes);
        assert!(wal_bytes > 0);
        assert!(wal_bytes <= BACKUP_MAX_WAL_BYTES);
    }
    assert_eq!(writer.active_generation()?, Some(5));

    let cancellation_started_at = cancellation_started_receiver.recv_timeout(OWNED_REPLY_TIMEOUT)?;
    require_owned_worker_success(
        cancellation_worker
            .join()
            .map_err(|_| io::Error::other("backup cancellation trigger panicked"))?,
    )?;
    let cancellation = receive_owned_worker_result(&exit_receiver)?;
    let cancellation_acknowledgement = cancellation
        .finished_at
        .saturating_duration_since(cancellation_started_at);
    assert!(cancellation.completed_steps > 0);
    assert!(cancellation.completed_steps <= BACKUP_MAX_STEPS);
    assert!(cancellation.elapsed <= BACKUP_WORKER_DEADLINE);
    assert!(cancellation_acknowledgement <= BACKUP_CANCELLATION_DEADLINE);
    join_owned_worker(backup_worker, "cancellable backup worker")?;

    let checkpoint = writer.checkpoint()?;
    assert_eq!(checkpoint.busy, 0);
    assert_eq!(checkpoint.log_frames, checkpoint.checkpointed_frames);
    assert_eq!(checkpoint.wal_bytes, 0);
    writer.shutdown()?;
    join_owned_worker(writer_worker, "owned writer worker")?;

    let source = open_read_database(&database_path)?;
    backup_database(&source, &verified_backup_path)?;
    let restored = open_read_database(&verified_backup_path)?;
    assert_eq!(
        restored.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))?,
        "ok"
    );
    assert_eq!(active_generation_id(&restored)?, Some(5));
    assert_eq!(generation_facts(&restored, 5)?, final_facts);
    eprintln!(
        "sustained-write backup cancellation: steps={}, backup_ms={}, \
         cancellation_ms={}, max_wal_bytes={max_wal_bytes}",
        cancellation.completed_steps,
        cancellation.elapsed.as_millis(),
        cancellation_acknowledgement.as_millis()
    );
    Ok(())
}
