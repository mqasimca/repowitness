#[test]
fn failed_cancelled_stale_and_rolled_back_staging_never_replace_active() -> TestResult {
    let directory = TempDirectory::new()?;
    let mut connection = open_file_database(&directory.join("failure.sqlite3"))?;
    bootstrap_workspace(&connection)?;
    stage_ready_generation(&mut connection, 1, 1, &[b"active".to_vec()], 1)?;
    activate_generation(&mut connection, 1, 1)?;

    begin_generation(&connection, 2, 2)?;
    advance_generation(&connection, 2, "discovered", "cancelled")?;
    assert_eq!(active_generation_id(&connection)?, Some(1));

    stage_ready_generation(&mut connection, 3, 3, &[b"stale".to_vec()], 1)?;
    assert!(activate_generation(&mut connection, 3, 4).is_err());
    assert_eq!(active_generation_id(&connection)?, Some(1));
    assert_eq!(generation_state(&connection, 1)?.as_deref(), Some("active"));
    assert_eq!(generation_state(&connection, 3)?.as_deref(), Some("ready"));

    {
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO generations(
                generation_id,
                workspace_id,
                source_epoch,
                lifecycle_state
             ) VALUES (4, 1, 4, 'discovered')",
            [],
        )?;
    }
    assert_eq!(generation_state(&connection, 4)?, None);
    assert_eq!(generation_facts(&connection, 1)?, [b"active".to_vec()]);
    Ok(())
}

#[test]
fn crash_writer_child() -> TestResult {
    let Some(database_path) = env::var_os(CRASH_CHILD_DATABASE) else {
        return Ok(());
    };
    let sentinel_path = env::var_os(CRASH_CHILD_SENTINEL)
        .ok_or_else(|| io::Error::other("crash child sentinel is missing"))?;
    let target_state = env::var(CRASH_CHILD_STATE)?;

    let mut connection = open_file_database(Path::new(&database_path))?;
    begin_generation(&connection, 2, 2)?;
    advance_to(&connection, 2, &target_state)?;
    write_facts_in_bounded_batches(
        &mut connection,
        2,
        &[b"partial-a".to_vec(), b"partial-b".to_vec()],
        1,
    )?;
    fs::write(sentinel_path, b"ready")?;
    thread::sleep(Duration::from_secs(60));
    Ok(())
}

#[test]
fn process_termination_in_every_staging_state_recovers_without_replacing_active() -> TestResult {
    for target_state in INCOMPLETE_STATES {
        let directory = TempDirectory::new()?;
        let database_path = directory.join("crash.sqlite3");
        let sentinel_path = directory.join("child-ready");
        let mut connection = open_file_database(&database_path)?;
        bootstrap_workspace(&connection)?;
        stage_ready_generation(&mut connection, 1, 1, &[b"active".to_vec()], 1)?;
        activate_generation(&mut connection, 1, 1)?;
        drop(connection);

        let mut child = Command::new(env::current_exe()?)
            .args(["--exact", "crash_writer_child", "--nocapture"])
            .env(CRASH_CHILD_DATABASE, &database_path)
            .env(CRASH_CHILD_SENTINEL, &sentinel_path)
            .env(CRASH_CHILD_STATE, target_state)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        wait_for_sentinel(&mut child, &sentinel_path)?;
        child.kill()?;
        let status = child.wait()?;
        assert!(!status.success(), "the child must be terminated");

        let mut recovered = open_file_database(&database_path)?;
        assert_eq!(recover_incomplete_generations(&mut recovered)?, 1);
        assert_eq!(active_generation_id(&recovered)?, Some(1));
        assert_eq!(generation_facts(&recovered, 1)?, [b"active".to_vec()]);
        assert!(generation_facts(&recovered, 2)?.is_empty());
        assert_eq!(generation_state(&recovered, 2)?.as_deref(), Some("failed"));
    }
    Ok(())
}
