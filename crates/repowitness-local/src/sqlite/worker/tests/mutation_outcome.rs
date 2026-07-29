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
        receive_mutation_reply(&receiver, Some(cancelled.as_ref()), Instant::now()),
        Ok(7)
    );
    assert!(cancelled.load(Ordering::Acquire));
    sender_thread.join().expect("sender thread should finish");
}

#[test]
fn missing_mutation_receipt_is_bounded_and_never_reported_as_rollback() {
    let (_sender, receiver) = mpsc::sync_channel::<Result<(), SqliteStoreError>>(1);
    let cancelled = Arc::new(AtomicBool::new(false));
    let started = Instant::now();

    assert_eq!(
        receive_mutation_reply(&receiver, Some(cancelled.as_ref()), Instant::now()),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
    assert!(cancelled.load(Ordering::Acquire));
    assert!(started.elapsed() >= Duration::from_millis(200));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn disconnected_mutation_reply_is_outcome_unknown() {
    let (sender, receiver) = mpsc::sync_channel::<Result<(), SqliteStoreError>>(1);
    drop(sender);

    assert_eq!(
        receive_mutation_reply(
            &receiver,
            Some(&AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        ),
        Err(SqliteStoreError::MutationOutcomeUnknown)
    );
}
