fn paused_writer(database: &Path) -> (OwnedSqliteIndex, mpsc::Receiver<()>, mpsc::SyncSender<()>) {
    let (committed, committed_receiver) = mpsc::sync_channel(1);
    let (release, release_receiver) = mpsc::sync_channel(1);
    let mut pause = Some((committed, release_receiver));
    let (writer, _) =
        OwnedSqliteIndex::start_with_post_commit_pause(database, 123, deadline(), move || {
            let Some((committed, release)) = pause.take() else {
                return;
            };
            let _ = committed.try_send(());
            let _ = release.recv_timeout(Duration::from_secs(2));
        })
        .expect("writer with a post-commit pause should start");
    (writer, committed_receiver, release)
}

fn paused_read_writer(
    database: &Path,
) -> (OwnedSqliteIndex, mpsc::Receiver<()>, mpsc::SyncSender<()>) {
    let (paused, paused_receiver) = mpsc::sync_channel(1);
    let (release, release_receiver) = mpsc::sync_channel(1);
    let mut pause = Some((paused, release_receiver));
    let (writer, _) =
        OwnedSqliteIndex::start_with_read_reply_pause(database, 123, deadline(), move || {
            let Some((paused, release)) = pause.take() else {
                return;
            };
            let _ = paused.try_send(());
            let _ = release.recv_timeout(Duration::from_secs(2));
        })
        .expect("writer with a paused read reply should start");
    (writer, paused_receiver, release)
}

fn paused_shutdown_writer(
    database: &Path,
) -> (OwnedSqliteIndex, mpsc::Receiver<()>, mpsc::SyncSender<()>) {
    let (paused, paused_receiver) = mpsc::sync_channel(1);
    let (release, release_receiver) = mpsc::sync_channel(1);
    let mut pause = Some((paused, release_receiver));
    let (writer, _) =
        OwnedSqliteIndex::start_with_shutdown_exit_pause(database, 123, deadline(), move || {
            let Some((paused, release)) = pause.take() else {
                return;
            };
            let _ = paused.try_send(());
            let _ = release.recv_timeout(Duration::from_secs(2));
        })
        .expect("writer with a paused shutdown exit should start");
    (writer, paused_receiver, release)
}

fn persisted_workspace_epoch(database: &Path) -> i64 {
    Connection::open(database)
        .expect("committed database should be readable")
        .query_row("SELECT source_epoch FROM workspaces", [], |row| row.get(0))
        .expect("one committed workspace should exist")
}

fn persisted_workspace_count(database: &Path) -> i64 {
    Connection::open(database)
        .expect("committed database should be readable")
        .query_row("SELECT count(*) FROM workspaces", [], |row| row.get(0))
        .expect("workspace count should be readable")
}

#[test]
fn receipt_arriving_during_mutation_resolution_grace_is_returned() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let cancelled = Arc::new(AtomicBool::new(false));
    let sender_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(10));
        sender
            .send(Ok::<u8, SqliteStoreError>(7))
            .expect("receiver should await the bounded outcome");
    });

    assert_eq!(
        receive_mutation_reply(&receiver, Some(cancelled.as_ref()), Instant::now(), None,),
        Ok(7)
    );
    assert!(cancelled.load(Ordering::Acquire));
    sender_thread.join().expect("sender thread should finish");
}

#[test]
fn missing_mutation_receipt_is_bounded_and_never_reported_as_rollback() {
    let (_sender, receiver) = mpsc::sync_channel::<Result<(), SqliteStoreError>>(1);
    let cancelled = Arc::new(AtomicBool::new(false));
    let unresolved_mutation = AtomicBool::new(false);
    let started = Instant::now();

    assert_eq!(
        receive_mutation_reply(
            &receiver,
            Some(cancelled.as_ref()),
            Instant::now(),
            Some(&unresolved_mutation),
        ),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    assert!(cancelled.load(Ordering::Acquire));
    assert!(unresolved_mutation.load(Ordering::Acquire));
    assert!(started.elapsed() >= Duration::from_millis(200));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn disconnected_mutation_reply_is_outcome_unknown() {
    let (sender, receiver) = mpsc::sync_channel::<Result<(), SqliteStoreError>>(1);
    let unresolved_mutation = AtomicBool::new(false);
    drop(sender);

    assert_eq!(
        receive_mutation_reply(
            &receiver,
            Some(&AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
            Some(&unresolved_mutation),
        ),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    assert!(unresolved_mutation.load(Ordering::Acquire));
}

#[test]
fn committed_writer_receipt_released_inside_grace_is_returned_exactly() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x51; 32]);
    let (writer, committed, release) = paused_writer(&database);
    let operation_deadline = Instant::now() + Duration::from_millis(200);

    thread::scope(|scope| {
        let operation =
            scope.spawn(|| writer.register_workspace(repository, 7, operation_deadline));
        committed
            .recv_timeout(Duration::from_secs(1))
            .expect("the transaction should commit before reply delivery");
        let until_deadline = operation_deadline.saturating_duration_since(Instant::now());
        thread::sleep(until_deadline + Duration::from_millis(20));
        release
            .send(())
            .expect("the paused owner should still await release");
        assert_eq!(
            operation.join().expect("caller thread should not panic"),
            Ok(())
        );
    });

    assert_eq!(persisted_workspace_epoch(&database), 7);
    writer
        .shutdown(deadline())
        .expect("writer should stop after the exact receipt");
}

#[test]
fn committed_writer_without_receipt_beyond_grace_is_unknown_but_durable() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x52; 32]);
    let (writer, committed, release) = paused_writer(&database);
    let operation_deadline = Instant::now() + Duration::from_millis(200);
    let started = Instant::now();

    thread::scope(|scope| {
        let operation =
            scope.spawn(|| writer.register_workspace(repository, 11, operation_deadline));
        committed
            .recv_timeout(Duration::from_secs(1))
            .expect("the transaction should commit before reply delivery");
        assert_eq!(
            operation.join().expect("caller thread should not panic"),
            Err(SqliteStoreError::MutationOutcomeUnknown)
        );
    });

    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(350));
    assert!(elapsed < Duration::from_millis(1_500));
    assert_eq!(persisted_workspace_epoch(&database), 11);
    release
        .send(())
        .expect("the detached owner should still await release");
    writer
        .shutdown(deadline())
        .expect("writer should stop after its delayed reply is discarded");
}

#[test]
fn unresolved_mutation_fences_queued_and_later_mutations_until_reopen() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let first_repository = RepositoryIdentityDigest::new([0x54; 32]);
    let second_repository = RepositoryIdentityDigest::new([0x55; 32]);
    let (writer, committed, release) = paused_writer(&database);
    let first_deadline = Instant::now() + Duration::from_millis(200);

    thread::scope(|scope| {
        let first =
            scope.spawn(|| writer.register_workspace(first_repository, 11, first_deadline));
        committed
            .recv_timeout(Duration::from_secs(1))
            .expect("the first transaction should commit before reply delivery");

        let queued_deadline = Instant::now() + Duration::from_secs(3);
        let (queued_reply, queued_receiver) = mpsc::sync_channel(1);
        writer
            .send(
                WriterCommand::Register {
                    repository: second_repository,
                    initial_source_epoch: 22,
                    deadline: queued_deadline,
                    reply: queued_reply,
                },
                queued_deadline,
            )
            .expect("the second mutation should queue before the first outcome is unresolved");

        assert_eq!(
            first.join().expect("first caller should not panic"),
            Err(SqliteStoreError::MutationOutcomeUnknown)
        );
        assert!(writer.unresolved_mutation.load(Ordering::Acquire));
        release
            .send(())
            .expect("the delayed writer should still await release");
        assert_eq!(
            receive_mutation_reply(
                &queued_receiver,
                None,
                queued_deadline,
                Some(writer.unresolved_mutation.as_ref()),
            ),
            Err(SqliteStoreError::MutationOutcomeUnknown)
        );
    });

    assert_eq!(
        writer.register_workspace(second_repository, 22, deadline()),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    writer
        .shutdown(deadline())
        .expect("shutdown should remain available for a fenced owner");
    assert_eq!(persisted_workspace_count(&database), 1);

    let (reopened, _) =
        OwnedSqliteIndex::start(&database, 456, deadline()).expect("store should reopen");
    reopened
        .register_workspace(second_repository, 22, deadline())
        .expect("reopening after reconciliation should clear the owner-local fence");
    reopened
        .shutdown(deadline())
        .expect("reopened writer should stop");
    assert_eq!(persisted_workspace_count(&database), 2);
}

#[test]
fn checkpoint_unknown_then_expired_shutdown_drops_without_joining_the_stalled_owner() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x55; 32]);
    let (seed, _) =
        OwnedSqliteIndex::start(&database, 122, deadline()).expect("seed writer should start");
    seed.register_workspace(repository, 19, deadline())
        .expect("seed workspace should commit");
    seed.shutdown(deadline())
        .expect("seed writer should stop cleanly");

    let (writer, committed, release) = paused_writer(&database);
    let operation_deadline = Instant::now() + Duration::from_millis(200);

    let error = thread::scope(|scope| {
        let operation = scope.spawn(|| writer.checkpoint(operation_deadline));
        committed
            .recv_timeout(Duration::from_secs(1))
            .expect("checkpoint should complete before reply delivery");
        operation
            .join()
            .expect("caller thread should not panic")
            .expect_err("a receipt withheld beyond grace should be outcome-unknown")
    });
    assert_eq!(error, SqliteStoreError::MutationOutcomeUnknown);
    let diagnostic = error.to_string();
    assert_eq!(
        diagnostic,
        "SQLite mutation outcome could not be determined"
    );
    assert!(!diagnostic.contains(database.to_string_lossy().as_ref()));

    let (shutdown_result, shutdown_receiver) = mpsc::sync_channel(1);
    let dropper = thread::spawn(move || {
        let result = writer.shutdown(Instant::now());
        let _ = shutdown_result.try_send(result);
    });
    assert_eq!(
        shutdown_receiver
            .recv_timeout(Duration::from_millis(500))
            .expect("expired shutdown and drop must remain bounded"),
        Err(SqliteStoreError::DeadlineExceeded)
    );
    dropper.join().expect("bounded drop should not panic");

    release
        .send(())
        .expect("the detached owner should still await release");
    let (reopened, _) = OwnedSqliteIndex::start(&database, 124, deadline())
        .expect("the detached owner should exit and release its mutation lease");
    assert_eq!(persisted_workspace_epoch(&database), 19);
    reopened
        .shutdown(deadline())
        .expect("the reopened writer should stop cleanly");

    let integrity = Connection::open(&database)
        .expect("the committed database should reopen")
        .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
        .expect("the integrity result should be readable");
    assert_eq!(integrity, "ok");
}

#[test]
fn queue_full_before_mutation_admission_is_definitely_not_committed() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x53; 32]);
    let (writer, committed, release) = paused_writer(&database);
    let first_deadline = deadline();

    thread::scope(|scope| {
        let first = scope.spawn(|| writer.register_workspace(repository, 13, first_deadline));
        committed
            .recv_timeout(Duration::from_secs(1))
            .expect("the first transaction should reach its reply seam");

        let (queued_reply, queued_receiver) = mpsc::sync_channel(1);
        let queued_deadline = deadline();
        writer
            .send(
                WriterCommand::ActiveGeneration {
                    repository,
                    deadline: queued_deadline,
                    reply: queued_reply,
                },
                queued_deadline,
            )
            .expect("one read should occupy the bounded owner queue");

        assert_eq!(
            writer.advance_source_epoch(repository, 13, 14, deadline()),
            Err(SqliteStoreError::QueueFull)
        );
        release
            .send(())
            .expect("the first committed operation should be released");
        assert_eq!(
            first.join().expect("caller thread should not panic"),
            Ok(())
        );
        assert_eq!(
            queued_receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("the queued read should receive its result"),
            Ok(None)
        );
    });

    assert_eq!(persisted_workspace_epoch(&database), 13);
    writer
        .shutdown(deadline())
        .expect("writer should stop after proving pre-admission backpressure");
}

#[test]
fn read_only_command_behind_paused_commit_keeps_reply_timeout_semantics() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x54; 32]);
    let (writer, committed, release) = paused_writer(&database);
    let first_deadline = deadline();

    thread::scope(|scope| {
        let first = scope.spawn(|| writer.register_workspace(repository, 17, first_deadline));
        committed
            .recv_timeout(Duration::from_secs(1))
            .expect("the transaction should reach its reply seam");
        assert_eq!(
            writer.active_generation(repository, Instant::now() + Duration::from_millis(30),),
            Err(SqliteStoreError::ReplyTimeout)
        );
        release
            .send(())
            .expect("the committed mutation should still await release");
        assert_eq!(
            first.join().expect("caller thread should not panic"),
            Ok(())
        );
    });

    assert_eq!(persisted_workspace_epoch(&database), 17);
    writer
        .shutdown(deadline())
        .expect("writer should stop after the expired read is discarded");
}

#[test]
fn drop_after_a_stalled_read_reply_detaches_without_joining() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let repository = RepositoryIdentityDigest::new([0x56; 32]);
    let (writer, paused, release) = paused_read_writer(&database);

    assert_eq!(
        writer.active_generation(repository, Instant::now() + Duration::from_millis(30),),
        Err(SqliteStoreError::ReplyTimeout)
    );
    paused
        .recv_timeout(Duration::from_secs(1))
        .expect("owner should pause before delivering the read reply");

    let (dropped, dropped_receiver) = mpsc::sync_channel(1);
    let dropper = thread::spawn(move || {
        drop(writer);
        let _ = dropped.try_send(());
    });
    let bounded = dropped_receiver
        .recv_timeout(Duration::from_millis(250))
        .is_ok();
    release
        .send(())
        .expect("detached owner should still await release");
    dropper.join().expect("dropper should not panic");
    assert!(bounded, "drop must not join a stalled read owner");

    let (reopened, _) = OwnedSqliteIndex::start(&database, 124, deadline())
        .expect("detached owner should release the mutation lease");
    reopened.shutdown(deadline()).expect("writer should stop");
}

#[test]
fn shutdown_deadline_bounds_owner_exit_after_acknowledgement() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (writer, paused, release) = paused_shutdown_writer(&database);
    let shutdown_deadline = Instant::now() + Duration::from_millis(100);
    let started = Instant::now();

    let result = writer.shutdown(shutdown_deadline);
    let elapsed = started.elapsed();
    paused
        .recv_timeout(Duration::from_millis(250))
        .expect("owner should pause after acknowledging shutdown");
    release
        .send(())
        .expect("detached owner should still await release");

    assert_eq!(result, Err(SqliteStoreError::DeadlineExceeded));
    assert!(elapsed >= Duration::from_millis(75));
    assert!(
        elapsed < Duration::from_millis(500),
        "shutdown must not join past its deadline"
    );

    let (reopened, _) = OwnedSqliteIndex::start(&database, 124, deadline())
        .expect("detached owner should release the mutation lease");
    reopened.shutdown(deadline()).expect("writer should stop");
}
