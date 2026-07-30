#[test]
fn projection_clear_failure_preserves_receipt_and_poisons_the_writer() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (seed, prepared, _) = prepared_memory_projection_fixture(&database);
    seed.shutdown(deadline())
        .expect("fixture writer should stop before fault injection");

    let (store, _) =
        OwnedSqliteIndex::start_with_progress_handler_clear_failure(&database, 456, deadline())
            .expect("fault-injected writer should start");
    let publication = store
        .publish_memory_projection(prepared, Arc::new(AtomicBool::new(false)), deadline())
        .expect("the exact committed projection receipt must survive cleanup failure");
    let projection_id = publication.projection_id();
    let (publication, maintenance) =
        crate::memory_management::finish_known_memory_mutation(store, publication, deadline());

    assert_eq!(publication.projection_id(), projection_id);
    assert_eq!(
        maintenance,
        crate::LocalMemoryMaintenance::CheckpointAndShutdownDeferred
    );
    let state = memory_projection_database_state(&database);
    assert_eq!(state.active_projection, Some(projection_id));
    assert_eq!(
        (state.generations, state.records, state.evidence),
        (1, 1, 1)
    );
}

#[test]
fn review_clear_failure_preserves_receipt_and_poisons_the_writer() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (seed, review) = prepared_memory_review_fixture(&database);
    seed.shutdown(deadline())
        .expect("fixture writer should stop before fault injection");

    let (store, _) =
        OwnedSqliteIndex::start_with_progress_handler_clear_failure(&database, 456, deadline())
            .expect("fault-injected writer should start");
    let receipt = store
        .append_memory_correspondence_review(review, Arc::new(AtomicBool::new(false)), deadline())
        .expect("the exact committed review receipt must survive cleanup failure");
    let (receipt, maintenance) =
        crate::memory_management::finish_known_memory_mutation(store, receipt, deadline());

    assert!(receipt.inserted());
    assert_eq!(
        maintenance,
        crate::LocalMemoryMaintenance::CheckpointAndShutdownDeferred
    );
    let connection = Connection::open(&database).expect("database should reopen");
    let review_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM memory_correspondence_audit",
            [],
            |row| row.get(0),
        )
        .expect("committed review should remain readable");
    assert_eq!(review_count, 1);
}

#[test]
fn review_commit_failure_reports_unknown_and_rolls_back_the_audit() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (seed, review) = prepared_memory_review_fixture(&database);
    seed.shutdown(deadline())
        .expect("fixture writer should stop before fault injection");

    let connection = Connection::open(&database).expect("fixture database should open");
    connection
        .execute_batch(
            "CREATE TABLE fixture_review_commit_failure(
                marker INTEGER PRIMARY KEY,
                workspace_id INTEGER NOT NULL,
                FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
                    DEFERRABLE INITIALLY DEFERRED
            );
            CREATE TRIGGER fail_fixture_review_commit
            AFTER INSERT ON memory_correspondence_audit BEGIN
                INSERT INTO fixture_review_commit_failure(marker, workspace_id)
                VALUES (1, -1);
            END;",
        )
        .expect("commit failure fixture should install");
    drop(connection);

    let (store, _) =
        OwnedSqliteIndex::start(&database, 456, deadline()).expect("store should reopen");
    let error = store
        .append_memory_correspondence_review(
            review,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect_err("commit failure must not produce a retry-safe error");
    assert_eq!(error, SqliteStoreError::MutationOutcomeUnknown);
    store.shutdown(deadline()).expect("writer should stop");

    let connection = Connection::open(&database).expect("database should reopen");
    let review_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM memory_correspondence_audit",
            [],
            |row| row.get(0),
        )
        .expect("review count should be readable");
    assert_eq!(review_count, 0);
}

fn prepared_memory_review_fixture(
    database: &Path,
) -> (OwnedSqliteIndex, PreparedMemoryCorrespondenceReview) {
    let (base_record, _, _) = memory_input(COMMIT_MEMORY_YAML);
    let repository = base_record.scope().repository();
    let source_identity = RustSourceSnapshotIdentity::new(
        repository,
        GitStateDigest::new([2; 32]),
        WorktreeStateDigest::new([0x12; 32]),
        ConfigurationDigest::new([4; 32]),
        ProducerManifestDigest::new([5; 32]),
        AnalysisSchemaDigest::new([6; 32]),
        7,
    );
    let source_index = prepared("review_source");
    let memory_yaml = aligned_memory_yaml(source_identity, &source_index);
    let (record, revision, presentation) = memory_input(memory_yaml.as_bytes());
    let (store, _) =
        OwnedSqliteIndex::start(database, 123, deadline()).expect("store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let source_generation = store
        .stage(
            0,
            source_identity,
            source_index,
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("source generation should stage");
    store
        .activate(source_generation, 0, deadline())
        .expect("source generation should activate");
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
    store
        .advance_source_epoch(repository, 0, 1, deadline())
        .expect("target source epoch should reserve");
    let target_generation = store
        .stage(
            1,
            RustSourceSnapshotIdentity::new(
                repository,
                GitStateDigest::new([2; 32]),
                WorktreeStateDigest::new([3; 32]),
                ConfigurationDigest::new([4; 32]),
                ProducerManifestDigest::new([5; 32]),
                AnalysisSchemaDigest::new([6; 32]),
                7,
            ),
            prepared("v1"),
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("target generation should stage");
    store
        .activate(target_generation, 1, deadline())
        .expect("target generation should activate");
    let review = prepared_correspondence_review(&store, &record, revision, repository);
    (store, review)
}

fn aligned_memory_yaml(
    source_identity: RustSourceSnapshotIdentity,
    source_index: &repowitness_application::PreparedRustIndex,
) -> String {
    let source_snapshot = hash_source_snapshot(source_identity, source_index.manifest_digest());
    let source_file = source_index
        .files()
        .first()
        .expect("source fixture should contain its source file");
    let source_fact = source_file
        .analysis()
        .facts()
        .first()
        .expect("source fixture should contain its declaration");
    let source_fingerprint = source_fact
        .correspondence()
        .expect("source declaration should have a correspondence fingerprint");
    String::from_utf8(COMMIT_MEMORY_YAML.to_vec())
        .expect("memory fixture should be UTF-8")
        .replace(
            "source_snapshot_digest: \"2222222222222222222222222222222222222222222222222222222222222222\"",
            &format!(
                "source_snapshot_digest: \"{}\"",
                lower_hex(source_snapshot.as_bytes())
            ),
        )
        .replace(
            "artifact_digest: \"4444444444444444444444444444444444444444444444444444444444444444\"",
            &format!(
                "artifact_digest: \"{}\"",
                lower_hex(source_file.artifact_digest().as_bytes())
            ),
        )
        .replace(
            "content_digest: \"3333333333333333333333333333333333333333333333333333333333333333\"",
            &format!(
                "content_digest: \"{}\"",
                lower_hex(source_file.content_digest().as_bytes())
            ),
        )
        .replace(
            "declaration_digest: \"5555555555555555555555555555555555555555555555555555555555555555\"",
            &format!(
                "declaration_digest: \"{}\"",
                lower_hex(source_fingerprint.declaration().as_bytes())
            ),
        )
        .replace(
            "name: \"publish\"",
            &format!("name: \"{}\"", source_fact.name()),
        )
        .replace(
            "qualified_name: \"crate::publish\"",
            &format!("qualified_name: \"{}\"", source_fact.qualified_name()),
        )
        .replace(
            "name_start: 3\n    name_length: 7",
            &format!(
                "name_start: {}\n    name_length: {}",
                source_fact.name_span().start().get(),
                source_fact.name_span().len().get()
            ),
        )
        .replace(
            "declaration_start: 0\n    declaration_length: 20",
            &format!(
                "declaration_start: {}\n    declaration_length: {}",
                source_fact.declaration_span().start().get(),
                source_fact.declaration_span().len().get()
            ),
        )
}

fn prepared_correspondence_review(
    store: &OwnedSqliteIndex,
    record: &MemoryRecord,
    revision: CanonicalMemoryDigest,
    repository: RepositoryIdentityDigest,
) -> PreparedMemoryCorrespondenceReview {
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
    let occurrence = ProjectionOccurrence::from_candidate(
        candidates
            .first()
            .expect("fixture should expose one review target"),
    );
    PreparedMemoryCorrespondenceReview::new(
        repository,
        record.header().record_id(),
        revision,
        0,
        MemoryCorrespondenceReviewOperation::Approved,
        occurrence.path().clone(),
        occurrence.artifact(),
        occurrence.fact_ordinal(),
        memory_actor(),
        memory_recorded_at(),
    )
}

fn lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to a string cannot fail");
    }
    output
}
