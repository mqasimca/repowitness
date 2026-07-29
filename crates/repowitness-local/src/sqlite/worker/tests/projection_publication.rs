struct MemoryProjectionFailureCase {
    name: &'static str,
    publish_baseline: bool,
    trigger: &'static str,
}

const MEMORY_PROJECTION_FAILURE_CASES: [MemoryProjectionFailureCase; 8] = [
    MemoryProjectionFailureCase {
        name: "generation staging",
        publish_baseline: true,
        trigger: "CREATE TRIGGER fail_fixture_projection_generation
            BEFORE INSERT ON memory_projection_generations BEGIN
                SELECT RAISE(ABORT, 'fixture generation failure');
            END;",
    },
    MemoryProjectionFailureCase {
        name: "record insertion",
        publish_baseline: true,
        trigger: "CREATE TRIGGER fail_fixture_projection_record
            BEFORE INSERT ON memory_projection_records BEGIN
                SELECT RAISE(ABORT, 'fixture record failure');
            END;",
    },
    MemoryProjectionFailureCase {
        name: "evidence insertion",
        publish_baseline: true,
        trigger: "CREATE TRIGGER fail_fixture_projection_evidence
            BEFORE INSERT ON memory_projection_evidence BEGIN
                SELECT RAISE(ABORT, 'fixture evidence failure');
            END;",
    },
    MemoryProjectionFailureCase {
        name: "candidate insertion",
        publish_baseline: true,
        trigger: "CREATE TRIGGER fail_fixture_projection_candidate
            BEFORE INSERT ON memory_projection_candidates BEGIN
                SELECT RAISE(ABORT, 'fixture candidate failure');
            END;",
    },
    MemoryProjectionFailureCase {
        name: "generation completion",
        publish_baseline: true,
        trigger: "CREATE TRIGGER fail_fixture_projection_completion
            BEFORE UPDATE OF lifecycle_state ON memory_projection_generations
            WHEN OLD.lifecycle_state = 'staging'
             AND NEW.lifecycle_state = 'complete' BEGIN
                SELECT RAISE(ABORT, 'fixture completion failure');
            END;",
    },
    MemoryProjectionFailureCase {
        name: "first activation",
        publish_baseline: false,
        trigger: "CREATE TRIGGER fail_fixture_projection_activation_insert
            BEFORE INSERT ON active_memory_projections BEGIN
                SELECT RAISE(ABORT, 'fixture activation insert failure');
            END;",
    },
    MemoryProjectionFailureCase {
        name: "replacement activation",
        publish_baseline: true,
        trigger: "CREATE TRIGGER fail_fixture_projection_activation_update
            BEFORE UPDATE OF projection_id ON active_memory_projections BEGIN
                SELECT RAISE(ABORT, 'fixture activation update failure');
            END;",
    },
    MemoryProjectionFailureCase {
        name: "transaction commit",
        publish_baseline: true,
        trigger: "CREATE TABLE fixture_projection_commit_failure(
                marker INTEGER PRIMARY KEY,
                workspace_id INTEGER NOT NULL,
                FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
                    DEFERRABLE INITIALLY DEFERRED
            );
            CREATE TRIGGER fail_fixture_projection_commit
            AFTER UPDATE OF projection_id ON active_memory_projections BEGIN
                INSERT INTO fixture_projection_commit_failure(marker, workspace_id)
                VALUES (1, -1);
            END;",
    },
];

#[test]
fn memory_projection_publication_is_complete_atomic_and_stale_safe() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (store, prepared, repository) = prepared_memory_projection_fixture(&database);
    let first = store
        .publish_memory_projection(
            prepared.clone(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("projection should publish");
    assert!(first.projection_id() > 0);
    assert_eq!(first.projected_records(), 1);
    assert_eq!(first.skipped_records(), 0);
    assert_eq!(first.unresolved_records(), 1);

    store
        .advance_source_epoch(repository, 0, 1, deadline())
        .expect("fixture should advance source epoch");
    assert_eq!(
        store.publish_memory_projection(prepared, Arc::new(AtomicBool::new(false)), deadline(),),
        Err(SqliteStoreError::StaleSourceEpoch)
    );
    store.shutdown(deadline()).expect("writer should stop");

    let connection = Connection::open(&database).expect("database should reopen");
    let state: (i64, i64, i64, i64, String) = connection
        .query_row(
            "SELECT active.projection_id,
                    (SELECT count(*) FROM memory_projection_generations),
                    (SELECT count(*) FROM memory_projection_records),
                    (SELECT count(*) FROM memory_projection_candidates),
                    evidence.outcome
             FROM active_memory_projections AS active
             JOIN memory_projection_evidence AS evidence
               ON evidence.projection_id = active.projection_id",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("projection should be complete");
    assert_eq!(state.0, first.projection_id());
    assert_eq!((state.1, state.2, state.3), (1, 1, 1));
    assert_eq!(state.4, "ambiguous");
}

#[test]
fn reserved_epoch_keeps_active_source_and_memory_readable_but_rejects_stale_publication() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (store, prepared, repository) = prepared_memory_projection_fixture(&database);
    let projection = store
        .publish_memory_projection(
            prepared.clone(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("baseline projection should publish");
    store
        .advance_source_epoch(repository, 0, 1, deadline())
        .expect("a successor source epoch should reserve");

    let reader =
        OwnedSqliteReader::start(&database, deadline()).expect("reader should start concurrently");
    let source = reader
        .search(
            repository,
            "stable_v1",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("the prior active source should remain readable");
    assert_eq!(source.hits().len(), 1);
    let memory = repowitness_application::memory_recall(
        &reader,
        repowitness_application::MemoryRecallRequest::new(
            repository,
            repowitness_application::MemoryRecallQuery::all(),
            repowitness_application::MemoryRecallLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
    )
    .expect("the prior active memory projection should remain readable");
    assert_eq!(*memory.generation(), source.generation());
    assert_eq!(*memory.projection(), projection.projection_id());
    assert_eq!(memory.source_epoch(), 0);
    assert_eq!(memory.records().len(), 1);

    assert_eq!(
        store.publish_memory_projection(prepared, Arc::new(AtomicBool::new(false)), deadline(),),
        Err(SqliteStoreError::StaleSourceEpoch)
    );
    reader.shutdown(deadline()).expect("reader should stop");
    store.shutdown(deadline()).expect("writer should stop");
}

#[test]
fn every_memory_projection_stage_failure_preserves_the_previous_active_projection() {
    for case in MEMORY_PROJECTION_FAILURE_CASES {
        let directory = TempDirectory::new();
        let database = directory.database();
        let (store, prepared, _) = prepared_memory_projection_fixture(&database);
        if case.publish_baseline {
            store
                .publish_memory_projection(
                    prepared.clone(),
                    Arc::new(AtomicBool::new(false)),
                    deadline(),
                )
                .expect("baseline projection should publish");
        }
        store.shutdown(deadline()).expect("writer should stop");
        let baseline = memory_projection_database_state(&database);

        let raw = Connection::open(&database).expect("fixture database should open");
        raw.execute_batch(case.trigger)
            .expect("fixture failure trigger should install");
        drop(raw);

        let (store, _) =
            OwnedSqliteIndex::start(&database, 456, deadline()).expect("store should reopen");
        let error = store
            .publish_memory_projection(prepared, Arc::new(AtomicBool::new(false)), deadline())
            .expect_err("injected stage failure should fail publication");
        assert_eq!(
            error,
            SqliteStoreError::DatabaseOperationFailed,
            "{} stage returned an unexpected error",
            case.name
        );
        store.shutdown(deadline()).expect("writer should stop");

        assert_eq!(
            memory_projection_database_state(&database),
            baseline,
            "{} stage changed the published projection state",
            case.name
        );
    }
}

fn prepared_memory_projection_fixture(
    database: &Path,
) -> (
    OwnedSqliteIndex,
    PreparedMemoryProjection,
    RepositoryIdentityDigest,
) {
    let (record, revision, presentation) = memory_input(COMMIT_MEMORY_YAML);
    let repository = record.scope().repository();
    let identity = RustSourceSnapshotIdentity::new(
        repository,
        GitStateDigest::new([2; 32]),
        WorktreeStateDigest::new([3; 32]),
        ConfigurationDigest::new([4; 32]),
        ProducerManifestDigest::new([5; 32]),
        AnalysisSchemaDigest::new([6; 32]),
        7,
    );
    let (store, _) =
        OwnedSqliteIndex::start(database, 123, deadline()).expect("store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let generation = store
        .stage(
            0,
            identity,
            prepared("v1"),
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("generation should stage");
    store
        .activate(generation, 0, deadline())
        .expect("generation should activate");
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
        .expect("memory should import");
    let journal = store
        .load_memory_journal(
            repository,
            MemoryProjectionLoadLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("journal should load");
    let MemoryEvidence::RustSymbol(evidence) = &record.evidence()[0];
    let candidates = store
        .load_rust_memory_candidates(
            journal.source(),
            evidence.clone(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("candidate set should load")
        .into_candidates();
    assert_eq!(candidates.len(), 1);
    let evidence_result =
        PreparedProjectionEvidence::ambiguous(vec![PreparedProjectionCandidate {
            occurrence: ProjectionOccurrence::from_candidate(&candidates[0]),
            relation: ProjectionCandidateRelation::Renamed,
        }])
        .expect("ambiguous evidence");
    let decision = evaluate_memory_projection(
        &record,
        Some(MemoryProjectValidity::Valid),
        &[MemoryEvidenceOutcome::NeedsReview],
    )
    .expect("projection decision");
    let prepared = PreparedMemoryProjection::try_new(
        journal.source(),
        MemoryRevalidationTarget::worktree(
            journal.source().snapshot(),
            Some(MemoryCommitId::Sha1([0x11; 20])),
        ),
        vec![PreparedProjectionRecord {
            record_id: record.header().record_id(),
            kind: PreparedProjectionRecordKind::Evaluated {
                revision,
                decision,
                evidence: vec![evidence_result],
            },
        }],
        0,
        0,
        MemoryProjectionResultLimits::default(),
    )
    .expect("prepared projection");
    (store, prepared, repository)
}

#[derive(Debug, Eq, PartialEq)]
struct MemoryProjectionDatabaseState {
    active_projection: Option<i64>,
    generations: i64,
    records: i64,
    evidence: i64,
    candidates: i64,
}

fn memory_projection_database_state(database: &Path) -> MemoryProjectionDatabaseState {
    let connection = Connection::open(database).expect("database should reopen");
    connection
        .query_row(
            "SELECT
                (SELECT projection_id FROM active_memory_projections),
                (SELECT count(*) FROM memory_projection_generations),
                (SELECT count(*) FROM memory_projection_records),
                (SELECT count(*) FROM memory_projection_evidence),
                (SELECT count(*) FROM memory_projection_candidates)",
            [],
            |row| {
                Ok(MemoryProjectionDatabaseState {
                    active_projection: row.get(0)?,
                    generations: row.get(1)?,
                    records: row.get(2)?,
                    evidence: row.get(3)?,
                    candidates: row.get(4)?,
                })
            },
        )
        .expect("projection state should be readable")
}
