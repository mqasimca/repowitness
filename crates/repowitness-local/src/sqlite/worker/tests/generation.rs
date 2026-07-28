fn artifact_identity() -> RustArtifactIdentity {
    RustArtifactIdentity::new(
        ProducerManifestDigest::new([5; 32]),
        ConfigurationDigest::new([4; 32]),
        AnalysisSchemaDigest::new([6; 32]),
        7,
    )
}

fn snapshot_identity() -> RustSourceSnapshotIdentity {
    RustSourceSnapshotIdentity::new(
        RepositoryIdentityDigest::new([1; 32]),
        GitStateDigest::new([2; 32]),
        WorktreeStateDigest::new([3; 32]),
        ConfigurationDigest::new([4; 32]),
        ProducerManifestDigest::new([5; 32]),
        AnalysisSchemaDigest::new([6; 32]),
        7,
    )
}

fn prepared(suffix: &str) -> repowitness_application::PreparedRustIndex {
    let cancelled = AtomicBool::new(false);
    prepare_rust_index(
        vec![
            ImmutableRustSource::new(
                RepositoryPath::try_from_bytes(b"src/lib.rs", PATH_LIMITS)
                    .expect("fixture path should be valid"),
                format!("pub fn stable_{suffix}() {{}}\n")
                    .into_bytes()
                    .into_boxed_slice(),
            ),
            ImmutableRustSource::new(
                RepositoryPath::try_from_bytes(b"src/model.rs", PATH_LIMITS)
                    .expect("fixture path should be valid"),
                b"pub struct Model;\n".to_vec().into_boxed_slice(),
            ),
        ],
        artifact_identity(),
        RustIndexLimits::default(),
        &cancelled,
        deadline(),
    )
    .expect("fixture index should prepare")
}

fn prepared_many(count: u16) -> repowitness_application::PreparedRustIndex {
    let mut source = String::new();
    for ordinal in 0..count {
        use std::fmt::Write as _;
        writeln!(source, "pub fn symbol_{ordinal:04}() {{}}")
            .expect("fixture source should be writable");
    }
    let cancelled = AtomicBool::new(false);
    prepare_rust_index(
        vec![ImmutableRustSource::new(
            RepositoryPath::try_from_bytes(b"src/many.rs", PATH_LIMITS)
                .expect("fixture path should be valid"),
            source.into_bytes().into_boxed_slice(),
        )],
        artifact_identity(),
        RustIndexLimits::default(),
        &cancelled,
        deadline(),
    )
    .expect("large fixture index should prepare")
}

fn verify_backup_publication(directory: &TempDirectory) -> PathBuf {
    let backup_path = directory.0.join("backup.sqlite3");
    let backup = create_online_backup(
        &directory.database(),
        &backup_path,
        BackupLimits::default(),
        Arc::new(AtomicBool::new(false)),
        deadline(),
    )
    .expect("online backup should publish");
    assert!(backup.steps() > 0);
    assert!(backup.source_pages() > 0);
    assert_eq!(
        create_online_backup(
            &directory.database(),
            &backup_path,
            BackupLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect_err("backup publication must not overwrite an existing file"),
        SqliteStoreError::BackupDestinationUnavailable
    );
    let cancelled_backup = directory.0.join("cancelled.sqlite3");
    assert_eq!(
        create_online_backup(
            &directory.database(),
            &cancelled_backup,
            BackupLimits::default(),
            Arc::new(AtomicBool::new(true)),
            deadline(),
        )
        .expect_err("pre-cancelled backup should fail"),
        SqliteStoreError::Cancelled
    );
    assert!(!cancelled_backup.exists());
    let bounded_backup = directory.0.join("bounded.sqlite3");
    let one_page_limit =
        BackupLimits::try_new(1, 1, Duration::ZERO).expect("fixture limit should be valid");
    assert_eq!(
        create_online_backup(
            &directory.database(),
            &bounded_backup,
            one_page_limit,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect_err("one page cannot contain the Phase 0 schema"),
        SqliteStoreError::BackupStepLimitExceeded
    );
    assert!(!bounded_backup.exists());
    assert!(
        fs::read_dir(&directory.0)
            .expect("fixture directory should remain readable")
            .all(|entry| !entry
                .expect("fixture entry should be readable")
                .file_name()
                .to_string_lossy()
                .contains("repowitness-partial"))
    );
    backup_path
}

fn verify_persisted_generation(
    directory: &TempDirectory,
    generation: super::GenerationId,
    backup_path: &PathBuf,
) {
    let connection =
        Connection::open(directory.database()).expect("database should reopen for inspection");
    let counts: (i64, i64, i64, i64, i64) = connection
        .query_row(
            "SELECT
                    (SELECT count(*) FROM source_snapshots WHERE lifecycle_state = 'complete'),
                    (SELECT count(*) FROM analysis_artifacts WHERE lifecycle_state = 'complete'),
                    (SELECT count(*) FROM artifact_facts),
                    (SELECT count(*) FROM artifact_fact_correspondence),
                    (SELECT count(*) FROM generation_search WHERE generation_id = ?1)",
            [generation.get()],
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
        .expect("persisted counts should be readable");
    assert_eq!(counts.0, 1);
    assert_eq!(counts.1, 2);
    assert!(counts.2 >= 2);
    assert_eq!(counts.2, counts.3);
    assert_eq!(counts.2, counts.4);
    assert!(
        connection
            .execute(
                "UPDATE source_snapshots SET configuration_digest = zeroblob(32)",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE analysis_artifacts SET source_content_digest = zeroblob(32)",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE artifact_fact_correspondence
                     SET declaration_digest = zeroblob(32)",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM artifact_fact_correspondence", [])
            .is_err()
    );
    let backup_connection =
        Connection::open(backup_path).expect("published backup should be readable");
    let backup_state: (i64, i64) = backup_connection
        .query_row(
            "SELECT active_generation_id,
                        (SELECT count(*) FROM artifact_fact_correspondence)
                 FROM workspaces",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("backup should preserve active generation");
    assert_eq!(backup_state, (generation.get(), counts.3));
}

#[test]
fn owned_writer_stages_and_atomically_activates_real_prepared_facts() {
    let directory = TempDirectory::new();
    let (store, startup) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let repository = snapshot_identity().repository();
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let generation = store
        .stage(
            0,
            snapshot_identity(),
            prepared("v1"),
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("generation should stage");

    assert_eq!(startup.recovered_generations(), 0);
    assert_eq!(
        store
            .active_generation(repository, deadline())
            .expect("active generation should be readable"),
        None
    );
    store
        .activate(generation, 0, deadline())
        .expect("ready generation should activate");
    assert_eq!(
        store
            .active_generation(repository, deadline())
            .expect("active generation should be readable"),
        Some(generation)
    );
    let checkpoint = store
        .checkpoint(deadline())
        .expect("explicit checkpoint should complete");
    assert_eq!(checkpoint.busy(), 0);
    assert!(checkpoint.checkpointed_frames() <= checkpoint.log_frames());
    let backup_path = verify_backup_publication(&directory);
    store.shutdown(deadline()).expect("worker should stop");
    verify_persisted_generation(&directory, generation, &backup_path);
}

#[test]
fn memory_revalidation_loads_one_pinned_journal_and_complete_candidate_set() {
    let directory = TempDirectory::new();
    let database = directory.database();
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
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("store should start");
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
    assert_eq!(journal.source().generation(), generation);
    assert_eq!(journal.source().source_epoch(), 0);
    assert!(journal.source().has_complete_index_coverage());
    assert_eq!(journal.versions().len(), 1);
    assert_eq!(journal.versions()[0].revision(), revision);
    assert!(journal.versions()[0].locally_approved());
    assert_eq!(
        journal.versions()[0].approval_git_source(),
        Some(MemoryCommitId::Sha1([0x11; 20]))
    );

    let MemoryEvidence::RustSymbol(evidence) = &record.evidence()[0];
    let candidates = store
        .load_rust_memory_candidates(
            journal.source(),
            evidence.clone(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("candidate set should load");
    assert_eq!(candidates.subject_name_elided(), None);
    assert_eq!(candidates.candidate_count_before_limit(), 1);
    assert_eq!(candidates.into_candidates().len(), 1);

    assert!(matches!(
        store.load_memory_journal(
            repository,
            MemoryProjectionLoadLimits::try_new(1, 1).expect("valid tiny bound"),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
        Err(SqliteStoreError::MemoryProjectionLimitExceeded)
    ));
    store.shutdown(deadline()).expect("writer should stop");
}
