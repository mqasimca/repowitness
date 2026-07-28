struct MemoryImportFailureCase {
    name: &'static str,
    yaml: &'static [u8],
    approval: MemoryImportApproval,
    trigger: &'static str,
}

const MEMORY_IMPORT_FAILURE_CASES: [MemoryImportFailureCase; 8] = [
    MemoryImportFailureCase {
        name: "parent revision",
        yaml: WORKTREE_RELATIONSHIP_MEMORY_YAML,
        approval: MemoryImportApproval::LocallyApproved,
        trigger: "CREATE TRIGGER fail_fixture_memory_parent
            BEFORE INSERT ON memory_version_parents BEGIN
                SELECT RAISE(ABORT, 'fixture parent failure');
            END;",
    },
    MemoryImportFailureCase {
        name: "commit validity",
        yaml: COMMIT_MEMORY_YAML,
        approval: MemoryImportApproval::LocallyApproved,
        trigger: "CREATE TRIGGER fail_fixture_memory_validity
            BEFORE INSERT ON memory_validity_commits BEGIN
                SELECT RAISE(ABORT, 'fixture validity failure');
            END;",
    },
    MemoryImportFailureCase {
        name: "evidence",
        yaml: COMMIT_MEMORY_YAML,
        approval: MemoryImportApproval::LocallyApproved,
        trigger: "CREATE TRIGGER fail_fixture_memory_evidence
            BEFORE INSERT ON memory_evidence BEGIN
                SELECT RAISE(ABORT, 'fixture evidence failure');
            END;",
    },
    MemoryImportFailureCase {
        name: "relationship",
        yaml: WORKTREE_RELATIONSHIP_MEMORY_YAML,
        approval: MemoryImportApproval::LocallyApproved,
        trigger: "CREATE TRIGGER fail_fixture_memory_relationship
            BEFORE INSERT ON memory_relationships BEGIN
                SELECT RAISE(ABORT, 'fixture relationship failure');
            END;",
    },
    MemoryImportFailureCase {
        name: "canonical version",
        yaml: COMMIT_MEMORY_YAML,
        approval: MemoryImportApproval::LocallyApproved,
        trigger: "CREATE TRIGGER fail_fixture_memory_version
            BEFORE INSERT ON memory_versions BEGIN
                SELECT RAISE(ABORT, 'fixture version failure');
            END;",
    },
    MemoryImportFailureCase {
        name: "observation audit",
        yaml: COMMIT_MEMORY_YAML,
        approval: MemoryImportApproval::ObservedOnly,
        trigger: "CREATE TRIGGER fail_fixture_memory_observation
            BEFORE INSERT ON memory_audit
            WHEN NEW.operation = 'observed' BEGIN
                SELECT RAISE(ABORT, 'fixture observation failure');
            END;",
    },
    MemoryImportFailureCase {
        name: "approval audit",
        yaml: COMMIT_MEMORY_YAML,
        approval: MemoryImportApproval::LocallyApproved,
        trigger: "CREATE TRIGGER fail_fixture_memory_approval
            BEFORE INSERT ON memory_audit
            WHEN NEW.operation = 'locally_approved' BEGIN
                SELECT RAISE(ABORT, 'fixture approval failure');
            END;",
    },
    MemoryImportFailureCase {
        name: "transaction commit",
        yaml: COMMIT_MEMORY_YAML,
        approval: MemoryImportApproval::LocallyApproved,
        trigger: "CREATE TABLE fixture_memory_commit_failure(
                marker INTEGER PRIMARY KEY,
                workspace_id INTEGER NOT NULL,
                FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
                    DEFERRABLE INITIALLY DEFERRED
            );
            CREATE TRIGGER fail_fixture_memory_commit
            AFTER INSERT ON memory_audit
            WHEN NEW.operation = 'locally_approved' BEGIN
                INSERT INTO fixture_memory_commit_failure(marker, workspace_id)
                VALUES (1, -1);
            END;",
    },
];

#[test]
fn saturated_drop_detaches_instead_of_waiting_without_shutdown() {
    let (commands, _receiver) = mpsc::sync_channel(1);
    let (reply, _reply_receiver) = mpsc::sync_channel(1);
    commands
        .send(WriterCommand::Shutdown { reply })
        .expect("fixture queue should accept one command");
    let worker = thread::spawn(|| thread::sleep(Duration::from_millis(500)));
    let store = OwnedSqliteIndex {
        commands,
        worker: Some(worker),
    };

    let started = Instant::now();
    drop(store);

    assert!(started.elapsed() < Duration::from_millis(250));
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one end-to-end fixture verifies semantic, presentation, audit, backup, and reopen idempotency together"
)]
fn memory_import_is_append_only_idempotent_and_survives_backup_and_reopen() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (record, revision, presentation) = memory_input(COMMIT_MEMORY_YAML);
    let repository = record.scope().repository();
    let (store, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");

    let first = import_memory_record(
        &store,
        ImportMemoryRecordRequest::new(
            repository,
            record.clone(),
            presentation,
            memory_source(),
            memory_actor(),
            memory_recorded_at(),
            MemoryImportApproval::LocallyApproved,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
    )
    .expect("shared application import should succeed");
    assert_eq!(first.revision(), revision);
    assert!(first.version_inserted());
    assert!(first.observation_inserted());
    assert!(first.approval_inserted());

    let repeated = store
        .import_memory_version(
            repository,
            record,
            presentation,
            memory_source(),
            memory_actor(),
            memory_recorded_at(),
            MemoryImportApproval::LocallyApproved,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("exact re-import should succeed");
    assert_eq!(repeated.revision(), revision);
    assert!(!repeated.version_inserted());
    assert!(!repeated.observation_inserted());
    assert!(!repeated.approval_inserted());

    let display_yaml =
        String::from_utf8(COMMIT_MEMORY_YAML.to_vec()).expect("fixture should be UTF-8");
    let display_yaml = display_yaml.replacen("display_revision: 1", "display_revision: 2", 1);
    let (display_record, display_revision, display_presentation) =
        memory_input(display_yaml.as_bytes());
    assert_eq!(display_revision, revision);
    let display_receipt = store
        .import_memory_version(
            repository,
            display_record,
            display_presentation,
            memory_source(),
            memory_actor(),
            memory_recorded_at(),
            MemoryImportApproval::LocallyApproved,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("presentation-only revision should import");
    assert!(!display_receipt.version_inserted());
    assert!(display_receipt.observation_inserted());
    assert!(!display_receipt.approval_inserted());

    let semantic_yaml = display_yaml.replace("partially staged", "partly staged");
    let (semantic_record, semantic_revision, semantic_presentation) =
        memory_input(semantic_yaml.as_bytes());
    assert_ne!(semantic_revision, revision);
    let semantic_receipt = store
        .import_memory_version(
            repository,
            semantic_record,
            semantic_presentation,
            memory_source(),
            memory_actor(),
            memory_recorded_at(),
            MemoryImportApproval::LocallyApproved,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("new semantic revision should append");
    assert!(semantic_receipt.version_inserted());
    assert!(semantic_receipt.observation_inserted());
    assert!(semantic_receipt.approval_inserted());

    let backup_path = directory.0.join("memory-backup.sqlite3");
    create_online_backup(
        &database,
        &backup_path,
        BackupLimits::default(),
        Arc::new(AtomicBool::new(false)),
        deadline(),
    )
    .expect("live memory database should back up");
    store.shutdown(deadline()).expect("writer should stop");

    for path in [&database, &backup_path] {
        let connection = Connection::open(path).expect("database should be inspectable");
        let counts: (i64, i64, i64) = connection
            .query_row(
                "SELECT
                        (SELECT count(*) FROM memory_versions),
                        (SELECT count(*) FROM memory_evidence),
                        (SELECT count(*) FROM memory_audit)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("memory rows should be readable");
        assert_eq!(counts, (2, 2, 5));
    }

    let (restarted, startup) =
        OwnedSqliteIndex::start(&database, 456, deadline()).expect("store should reopen");
    assert_eq!(startup.recovered_generations(), 0);
    restarted
        .register_workspace(repository, 0, deadline())
        .expect("workspace should remain registered");
    let (record, _, presentation) = memory_input(COMMIT_MEMORY_YAML);
    let receipt = restarted
        .import_memory_version(
            repository,
            record,
            presentation,
            memory_source(),
            memory_actor(),
            memory_recorded_at(),
            MemoryImportApproval::LocallyApproved,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("reopened import should remain idempotent");
    assert!(!receipt.version_inserted());
    assert!(!receipt.observation_inserted());
    assert!(!receipt.approval_inserted());
    restarted
        .shutdown(deadline())
        .expect("restarted writer should stop");
}

#[test]
fn observed_only_import_cannot_activate_repository_authored_memory() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (record, revision, presentation) = memory_input(COMMIT_MEMORY_YAML);
    let repository = record.scope().repository();
    let (store, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");

    let receipt = store
        .import_memory_version(
            repository,
            record,
            presentation,
            memory_source(),
            memory_actor(),
            memory_recorded_at(),
            MemoryImportApproval::ObservedOnly,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("observation-only import should succeed");
    assert_eq!(receipt.revision(), revision);
    assert!(receipt.version_inserted());
    assert!(receipt.observation_inserted());
    assert!(!receipt.approval_inserted());
    store.shutdown(deadline()).expect("writer should stop");

    let connection = Connection::open(&database).expect("database should reopen");
    let counts: (i64, i64) = connection
        .query_row(
            "SELECT
                    count(*) FILTER (WHERE operation = 'observed'),
                    count(*) FILTER (WHERE operation = 'locally_approved')
             FROM memory_audit",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("audit counts should be readable");
    assert_eq!(counts, (1, 0));
}

#[test]
fn memory_import_control_failure_leaves_no_partial_rows() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (record, _, presentation) = memory_input(COMMIT_MEMORY_YAML);
    let repository = record.scope().repository();
    let (store, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    assert_eq!(
        store
            .import_memory_version(
                repository,
                record.clone(),
                presentation,
                memory_source(),
                memory_actor(),
                memory_recorded_at(),
                MemoryImportApproval::LocallyApproved,
                Arc::new(AtomicBool::new(true)),
                deadline(),
            )
            .expect_err("pre-cancelled import should fail"),
        SqliteStoreError::Cancelled
    );
    assert_eq!(
        store
            .import_memory_version(
                repository,
                record.clone(),
                presentation,
                memory_source(),
                memory_actor(),
                memory_recorded_at(),
                MemoryImportApproval::LocallyApproved,
                Arc::new(AtomicBool::new(false)),
                Instant::now(),
            )
            .expect_err("elapsed import deadline should fail"),
        SqliteStoreError::DeadlineExceeded
    );
    store.shutdown(deadline()).expect("writer should stop");

    assert_eq!(memory_row_count(&database), 0);
}

#[test]
fn every_memory_import_stage_failure_rolls_back_the_whole_journal_transaction() {
    for case in MEMORY_IMPORT_FAILURE_CASES {
        let directory = TempDirectory::new();
        let database = directory.database();
        let (record, _, presentation) = memory_input(case.yaml);
        let repository = record.scope().repository();
        let (store, _) =
            OwnedSqliteIndex::start(&database, 123, deadline()).expect("store should start");
        store
            .register_workspace(repository, 0, deadline())
            .expect("workspace should register");
        store.shutdown(deadline()).expect("writer should stop");

        let raw = Connection::open(&database).expect("fixture database should open");
        raw.execute_batch(case.trigger)
            .expect("fixture failure trigger should install");
        drop(raw);

        let (store, _) =
            OwnedSqliteIndex::start(&database, 456, deadline()).expect("store should reopen");
        let error =
        store
            .import_memory_version(
                repository,
                record,
                presentation,
                memory_source(),
                memory_actor(),
                memory_recorded_at(),
                case.approval,
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect_err("injected stage failure should fail the import");
        assert_eq!(
            error,
            SqliteStoreError::DatabaseOperationFailed,
            "{} stage returned an unexpected error",
            case.name
        );
        store.shutdown(deadline()).expect("writer should stop");

        assert_eq!(
            memory_row_count(&database),
            0,
            "{} stage left partial journal rows",
            case.name
        );
    }
}

fn memory_row_count(database: &Path) -> i64 {
    let raw = Connection::open(database).expect("database should reopen");
    raw.query_row(
            "SELECT
                    (SELECT count(*) FROM memory_versions) +
                    (SELECT count(*) FROM memory_version_parents) +
                    (SELECT count(*) FROM memory_validity_commits) +
                    (SELECT count(*) FROM memory_evidence) +
                    (SELECT count(*) FROM memory_relationships) +
                    (SELECT count(*) FROM memory_audit)",
            [],
            |row| row.get(0),
        )
        .expect("memory row count should be readable")
}

#[test]
fn memory_reimport_detects_normalized_row_corruption() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (record, _, presentation) = memory_input(COMMIT_MEMORY_YAML);
    let repository = record.scope().repository();
    let (store, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    store
        .import_memory_version(
            repository,
            record.clone(),
            presentation,
            memory_source(),
            memory_actor(),
            memory_recorded_at(),
            MemoryImportApproval::LocallyApproved,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("baseline memory should import");
    store.shutdown(deadline()).expect("writer should stop");

    let raw = Connection::open(&database).expect("fixture database should open");
    raw.execute("DROP TRIGGER memory_evidence_no_update", [])
        .expect("fixture should remove the immutable evidence guard");
    assert_eq!(
        raw.execute(
            "UPDATE memory_evidence SET producer_version = 'tampered'",
            []
        )
        .expect("fixture should alter normalized evidence"),
        1
    );
    drop(raw);

    let (store, _) =
        OwnedSqliteIndex::start(&database, 456, deadline()).expect("store should reopen");
    assert_eq!(
        store
            .import_memory_version(
                repository,
                record,
                presentation,
                memory_source(),
                memory_actor(),
                memory_recorded_at(),
                MemoryImportApproval::LocallyApproved,
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect_err("normalized corruption must fail re-import"),
        SqliteStoreError::IntegrityCheckFailed
    );
    store.shutdown(deadline()).expect("writer should stop");

    let raw = Connection::open(&database).expect("database should reopen");
    let counts: (i64, i64) = raw
        .query_row(
            "SELECT
                    (SELECT count(*) FROM memory_versions),
                    (SELECT count(*) FROM memory_audit)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("history counts should remain readable");
    assert_eq!(counts, (1, 2));
}
