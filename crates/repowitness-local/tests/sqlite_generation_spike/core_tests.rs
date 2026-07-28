#[test]
fn bundled_sqlite_meets_runtime_and_compile_requirements() -> TestResult {
    eprintln!("bundled SQLite runtime: {}", rusqlite::version());
    assert!(
        rusqlite::version_number() >= FIXED_WAL_SQLITE_VERSION,
        "bundled SQLite {} is older than the WAL-reset-fixed floor",
        rusqlite::version()
    );

    let directory = TempDirectory::new()?;
    let connection = open_file_database(&directory.join("runtime.sqlite3"))?;
    let mut compile_options = Vec::new();
    connection.pragma_query(None, "compile_options", |row| {
        compile_options.push(row.get::<_, String>(0)?);
        Ok(())
    })?;

    assert!(
        compile_options.iter().any(|option| option == "ENABLE_FTS5"),
        "the bundled SQLite must include FTS5"
    );
    assert!(
        compile_options
            .iter()
            .any(|option| option == "THREADSAFE=1"),
        "the bundled SQLite must use serialized thread safety"
    );
    assert_eq!(
        connection.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))?,
        "wal"
    );
    assert_eq!(
        connection.pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0))?,
        1
    );
    assert_eq!(
        connection.pragma_query_value(None, "trusted_schema", |row| row.get::<_, i64>(0))?,
        0
    );
    assert_eq!(
        connection.pragma_query_value(None, "wal_autocheckpoint", |row| row.get::<_, i64>(0))?,
        0
    );
    assert_eq!(
        connection.pragma_query_value(None, "busy_timeout", |row| row.get::<_, i64>(0))?,
        250
    );
    assert_eq!(
        connection.pragma_query_value(None, "synchronous", |row| row.get::<_, i64>(0))?,
        2
    );
    Ok(())
}

#[test]
fn activation_is_atomic_and_readers_pin_one_generation() -> TestResult {
    let directory = TempDirectory::new()?;
    let database_path = directory.join("activation.sqlite3");
    let mut writer = open_file_database(&database_path)?;
    bootstrap_workspace(&writer)?;
    stage_ready_generation(&mut writer, 1, 1, &[b"old".to_vec()], 1)?;
    activate_generation(&mut writer, 1, 1)?;

    let mut reader = open_read_database(&database_path)?;
    let pinned_reader = reader.transaction()?;
    assert_eq!(active_generation_id(&pinned_reader)?, Some(1));
    assert_eq!(generation_facts(&pinned_reader, 1)?, [b"old".to_vec()]);

    stage_ready_generation(&mut writer, 2, 2, &[b"new".to_vec()], 1)?;
    activate_generation(&mut writer, 2, 2)?;

    assert_eq!(active_generation_id(&pinned_reader)?, Some(1));
    assert_eq!(generation_facts(&pinned_reader, 1)?, [b"old".to_vec()]);
    pinned_reader.commit()?;

    assert_eq!(active_generation_id(&reader)?, Some(2));
    assert_eq!(generation_facts(&reader, 2)?, [b"new".to_vec()]);
    assert_eq!(generation_state(&reader, 1)?.as_deref(), Some("retained"));
    assert_eq!(generation_state(&reader, 2)?.as_deref(), Some("active"));
    Ok(())
}

#[test]
fn full_and_normal_preserve_activation_pinning_and_stale_epoch_safety() -> TestResult {
    for synchronous in ["FULL", "NORMAL"] {
        let directory = TempDirectory::new()?;
        let database_path = directory.join(&format!("durability-invariants-{synchronous}.sqlite3"));
        let mut writer = open_file_database_with_synchronous(&database_path, synchronous)?;
        bootstrap_workspace(&writer)?;
        stage_ready_generation(&mut writer, 1, 1, &[b"old".to_vec()], 1)?;
        activate_generation(&mut writer, 1, 1)?;

        let mut reader = open_read_database(&database_path)?;
        let pinned_reader = reader.transaction()?;
        assert_eq!(active_generation_id(&pinned_reader)?, Some(1));

        stage_ready_generation(&mut writer, 2, 2, &[b"new".to_vec()], 1)?;
        activate_generation(&mut writer, 2, 2)?;
        stage_ready_generation(&mut writer, 3, 3, &[b"stale".to_vec()], 1)?;
        assert!(activate_generation(&mut writer, 3, 4).is_err());

        assert_eq!(active_generation_id(&pinned_reader)?, Some(1));
        assert_eq!(generation_facts(&pinned_reader, 1)?, [b"old".to_vec()]);
        pinned_reader.commit()?;
        assert_eq!(active_generation_id(&reader)?, Some(2));
        assert_eq!(generation_facts(&reader, 2)?, [b"new".to_vec()]);
        assert_eq!(generation_state(&reader, 1)?.as_deref(), Some("retained"));
        assert_eq!(generation_state(&reader, 2)?.as_deref(), Some("active"));
        assert_eq!(generation_state(&reader, 3)?.as_deref(), Some("ready"));
    }
    Ok(())
}

#[test]
fn pinned_reader_blocks_truncation_without_blocking_new_generations() -> TestResult {
    let directory = TempDirectory::new()?;
    let database_path = directory.join("checkpoint-starvation.sqlite3");
    let source_wal_path = wal_path(&database_path);
    let mut writer = open_file_database(&database_path)?;
    bootstrap_workspace(&writer)?;
    stage_ready_generation(&mut writer, 1, 1, &fact_fixture(128, 128), 32)?;
    activate_generation(&mut writer, 1, 1)?;

    let (busy, log_frames, checkpointed_frames) = truncate_checkpoint(&writer)?;
    assert_eq!((busy, log_frames, checkpointed_frames), (0, 0, 0));
    assert_eq!(fs::metadata(&source_wal_path)?.len(), 0);

    let mut reader = open_read_database(&database_path)?;
    let pinned_reader = reader.transaction()?;
    assert_eq!(active_generation_id(&pinned_reader)?, Some(1));

    let generation_two_facts = fact_fixture(512, 256);
    stage_ready_generation(&mut writer, 2, 2, &generation_two_facts, 64)?;
    activate_generation(&mut writer, 2, 2)?;
    let wal_bytes_before_checkpoint = fs::metadata(&source_wal_path)?.len();
    assert!(wal_bytes_before_checkpoint > 0);

    let (busy, log_frames, checkpointed_frames) = truncate_checkpoint(&writer)?;
    assert_eq!(busy, 1);
    assert!(log_frames > 0);
    assert!(checkpointed_frames <= log_frames);
    assert!(fs::metadata(&source_wal_path)?.len() > 0);
    assert_eq!(active_generation_id(&pinned_reader)?, Some(1));

    let generation_three_facts = fact_fixture(256, 192);
    stage_ready_generation(&mut writer, 3, 3, &generation_three_facts, 32)?;
    activate_generation(&mut writer, 3, 3)?;
    assert!(fs::metadata(&source_wal_path)?.len() > wal_bytes_before_checkpoint);
    assert_eq!(active_generation_id(&pinned_reader)?, Some(1));
    assert_eq!(active_generation_id(&writer)?, Some(3));

    pinned_reader.commit()?;
    assert_eq!(active_generation_id(&reader)?, Some(3));
    assert_eq!(generation_facts(&reader, 3)?, generation_three_facts);

    let (busy, log_frames, checkpointed_frames) = truncate_checkpoint(&writer)?;
    assert_eq!(busy, 0);
    assert_eq!(log_frames, checkpointed_frames);
    assert_eq!(fs::metadata(source_wal_path)?.len(), 0);
    Ok(())
}

#[test]
fn owned_connections_bound_checkpoint_contention_and_cancel_reader() -> TestResult {
    let directory = TempDirectory::new()?;
    let database_path = directory.join("owned-checkpoint.sqlite3");
    let mut setup = open_file_database(&database_path)?;
    bootstrap_workspace(&setup)?;
    stage_ready_generation(&mut setup, 1, 1, &fact_fixture(128, 128), 32)?;
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
    let reader_path = database_path.clone();
    let reader_cancelled = Arc::clone(&cancelled);
    let reader_worker = thread::spawn(move || {
        run_owned_reader_worker(
            &reader_path,
            reader_cancelled.as_ref(),
            OWNED_READER_DEADLINE,
            &ready_sender,
            &exit_sender,
        )
    });
    assert_eq!(receive_owned_worker_result(&ready_receiver)?, Some(1));

    let over_limit = vec![Vec::new(); OWNED_MAX_FACTS_PER_GENERATION + 1];
    assert_eq!(
        writer.publish_result(2, 2, over_limit, 64)?,
        Err("owned writer fact limit exceeded".to_owned())
    );
    assert_eq!(
        writer.publish_result(2, 2, fact_fixture(1, 1), 0)?,
        Err("owned writer batch limit invalid".to_owned())
    );
    assert_eq!(writer.active_generation()?, Some(1));

    writer.publish(2, 2, fact_fixture(512, 256), 64)?;
    let first_busy_checkpoint = writer.checkpoint()?;
    assert_eq!(first_busy_checkpoint.busy, 1);
    assert!(first_busy_checkpoint.log_frames > 0);
    assert!(first_busy_checkpoint.checkpointed_frames <= first_busy_checkpoint.log_frames);
    assert!(first_busy_checkpoint.elapsed <= OWNED_CHECKPOINT_DEADLINE);
    assert!(first_busy_checkpoint.wal_bytes > 0);
    assert!(first_busy_checkpoint.wal_bytes <= OWNED_MAX_WAL_BYTES);

    let generation_three_facts = fact_fixture(256, 192);
    writer.publish(3, 3, generation_three_facts.clone(), 32)?;
    let second_busy_checkpoint = writer.checkpoint()?;
    assert_eq!(second_busy_checkpoint.busy, 1);
    assert!(second_busy_checkpoint.elapsed <= OWNED_CHECKPOINT_DEADLINE);
    assert!(second_busy_checkpoint.wal_bytes >= first_busy_checkpoint.wal_bytes);
    assert!(second_busy_checkpoint.wal_bytes <= OWNED_MAX_WAL_BYTES);
    assert_eq!(writer.active_generation()?, Some(3));

    let cancellation_started_at = Instant::now();
    cancelled.store(true, Ordering::Release);
    assert_eq!(
        receive_owned_worker_result(&exit_receiver)?,
        OwnedReaderExit::Cancelled
    );
    let cancellation_elapsed = cancellation_started_at.elapsed();
    assert!(cancellation_elapsed <= OWNED_CHECKPOINT_DEADLINE);
    join_owned_worker(reader_worker, "owned reader worker")?;

    let final_checkpoint = writer.checkpoint()?;
    assert_eq!(final_checkpoint.busy, 0);
    assert_eq!(
        final_checkpoint.log_frames,
        final_checkpoint.checkpointed_frames
    );
    assert_eq!(final_checkpoint.wal_bytes, 0);
    assert!(final_checkpoint.elapsed <= OWNED_CHECKPOINT_DEADLINE);
    eprintln!(
        "owned SQLite topology: busy_checkpoint_ms=[{}, {}], max_wal_bytes={}, \
         cancellation_ms={}, final_checkpoint_ms={}",
        first_busy_checkpoint.elapsed.as_millis(),
        second_busy_checkpoint.elapsed.as_millis(),
        second_busy_checkpoint.wal_bytes,
        cancellation_elapsed.as_millis(),
        final_checkpoint.elapsed.as_millis()
    );
    writer.shutdown()?;
    join_owned_worker(writer_worker, "owned writer worker")?;

    let restored_reader = open_read_database(&database_path)?;
    assert_eq!(active_generation_id(&restored_reader)?, Some(3));
    assert_eq!(
        generation_facts(&restored_reader, 3)?,
        generation_three_facts
    );
    Ok(())
}

#[test]
fn owned_reader_deadline_releases_pinned_generation() -> TestResult {
    let directory = TempDirectory::new()?;
    let database_path = directory.join("owned-reader-deadline.sqlite3");
    let mut setup = open_file_database(&database_path)?;
    bootstrap_workspace(&setup)?;
    stage_ready_generation(&mut setup, 1, 1, &[b"active".to_vec()], 1)?;
    activate_generation(&mut setup, 1, 1)?;
    drop(setup);

    let cancelled = AtomicBool::new(false);
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (exit_sender, exit_receiver) = mpsc::sync_channel(1);
    let reader_path = database_path.clone();
    let reader_worker = thread::spawn(move || {
        run_owned_reader_worker(
            &reader_path,
            &cancelled,
            Duration::from_millis(25),
            &ready_sender,
            &exit_sender,
        )
    });

    assert_eq!(receive_owned_worker_result(&ready_receiver)?, Some(1));
    assert_eq!(
        receive_owned_worker_result(&exit_receiver)?,
        OwnedReaderExit::DeadlineExceeded
    );
    join_owned_worker(reader_worker, "deadline reader worker")?;

    let reader = open_read_database(&database_path)?;
    assert_eq!(active_generation_id(&reader)?, Some(1));
    Ok(())
}
