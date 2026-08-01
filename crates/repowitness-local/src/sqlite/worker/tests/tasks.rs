#[test]
fn task_checkpoints_and_verifications_are_append_only_and_scope_checked() {
    let directory = TempDirectory::new();
    let (store, _startup) =
        OwnedSqliteIndex::start(&directory.database(), 1, deadline()).expect("store should open");
    let task_id = TaskId::new([0x41; 16]);
    let repository = RepositoryIdentityDigest::new([0x42; 32]);
    let checkpoint = TaskCheckpoint::try_new(
        task_id,
        repository,
        1,
        TaskState::Open,
        TaskText::try_new("verify the durable task receipt").expect("objective"),
        Some(TaskText::try_new("the task owner preserves ordering").expect("hypothesis")),
        Some(TaskText::try_new("record an exact verification").expect("next action")),
        1,
    )
    .expect("checkpoint");
    let receipt = store
        .append_task_checkpoint(checkpoint.clone(), Arc::new(AtomicBool::new(false)), deadline())
        .expect("checkpoint should persist");
    assert_eq!(receipt.task_id(), task_id);
    assert_eq!(receipt.sequence(), 1);

    let verification = TaskVerification::try_new(
        task_id,
        1,
        SourceSnapshotDigest::new([0x43; 32]),
        TaskText::try_new("cargo test -p repowitness-local").expect("check"),
        TaskText::try_new("cargo").expect("producer"),
        ConfigurationDigest::new([0x44; 32]),
        TaskVerificationOutcome::Passed,
        [0x45; 32],
        0,
        2,
    )
    .expect("verification");
    assert!(store
        .append_task_verification(verification, Arc::new(AtomicBool::new(false)), deadline())
        .expect("verification should persist")
        .verification_id()
        > 0);
    let status = store
        .task_status(
            repository,
            task_id,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("status query succeeds")
        .expect("same repository may poll task");
    assert_eq!(status.state(), TaskState::Open);
    assert_eq!(status.checkpoint_sequence(), 1);
    assert_eq!(status.verification_count(), 1);
    assert_eq!(
        store
            .task_status(
                RepositoryIdentityDigest::new([0x99; 32]),
                task_id,
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("foreign scope query is safe"),
        None
    );
    assert_eq!(
        store
            .append_task_checkpoint(checkpoint, Arc::new(AtomicBool::new(false)), deadline())
            .expect_err("a repeated sequence must not overwrite history"),
        SqliteStoreError::InvalidTask
    );
    store.shutdown(deadline()).expect("store should stop");

    let connection = Connection::open(directory.database()).expect("database should reopen");
    let counts: (i64, i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT count(*) FROM engineering_tasks),
                (SELECT count(*) FROM engineering_task_checkpoints),
                (SELECT count(*) FROM engineering_task_verifications)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("task rows should be readable");
    assert_eq!(counts, (1, 1, 1));
}

#[test]
fn task_secret_text_is_rejected_before_any_durable_mutation() {
    let directory = TempDirectory::new();
    let (store, _startup) =
        OwnedSqliteIndex::start(&directory.database(), 1, deadline()).expect("store starts");
    let checkpoint = TaskCheckpoint::try_new(
        TaskId::new([0x51; 16]),
        RepositoryIdentityDigest::new([0x52; 32]),
        1,
        TaskState::Open,
        TaskText::try_new("api_key = private-value".to_owned()).expect("text validates structurally"),
        None,
        None,
        1,
    )
    .expect("checkpoint validates structurally");
    assert_eq!(
        store
            .append_task_checkpoint(checkpoint, Arc::new(AtomicBool::new(false)), deadline())
            .expect_err("sensitive task text is rejected"),
        SqliteStoreError::InvalidTask
    );
    store.shutdown(deadline()).expect("store stops");
    let connection = Connection::open(directory.database()).expect("open database");
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM engineering_tasks", [], |row| row.get(0))
        .expect("count tasks");
    assert_eq!(count, 0);
}

#[test]
fn personal_memory_is_profile_and_repository_isolated_and_append_only() {
    let directory = TempDirectory::new();
    let (store, _startup) =
        OwnedSqliteIndex::start(&directory.database(), 1, deadline()).expect("store starts");
    let profile = PersonalMemoryProfileId::new([0x61; 16]);
    let repository = RepositoryIdentityDigest::new([0x62; 32]);
    let record = PersonalMemoryRecord::new(
        profile,
        repository,
        PersonalMemoryId::new([0x63; 16]),
        PersonalMemoryRevision::new([0x64; 32]),
        PersonalMemoryKind::Preference,
        TaskText::try_new("prefer explicit source evidence".to_owned()).expect("title"),
        TaskText::try_new("keep this only in the local profile".to_owned()).expect("body"),
        MemoryLifecycle::Active,
        1,
    );
    let first = store
        .append_personal_memory(record.clone(), Arc::new(AtomicBool::new(false)), deadline())
        .expect("personal revision persists");
    assert!(first.inserted());
    assert!(!store
        .append_personal_memory(record, Arc::new(AtomicBool::new(false)), deadline())
        .expect("same revision is idempotent")
        .inserted());
    store.shutdown(deadline()).expect("store stops");

    let reader = OwnedSqliteReader::start(&directory.database(), deadline())
        .expect("read-only personal memory reader starts");
    let exact = reader
        .read_personal_memory(
            profile,
            repository,
            10,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("exact personal scope reads its record");
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].record_id(), PersonalMemoryId::new([0x63; 16]));
    assert!(reader
        .read_personal_memory(
            PersonalMemoryProfileId::new([0x65; 16]),
            repository,
            10,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("foreign profile remains isolated")
        .is_empty());
    assert!(reader
        .read_personal_memory(
            profile,
            RepositoryIdentityDigest::new([0x66; 32]),
            10,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("foreign repository remains isolated")
        .is_empty());
    reader.shutdown(deadline()).expect("reader stops");

    let connection = Connection::open(directory.database()).expect("open database");
    assert!(connection
        .execute("UPDATE personal_memory_records SET title = 'changed'", [])
        .is_err());
    let counts: (i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM personal_memory_records),
                (SELECT COUNT(*) FROM personal_memory_audit)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("counts");
    assert_eq!(counts, (1, 1));
}
