fn assert_prior_generation_readable(
    database: &Path,
    expected_generation: crate::GenerationId,
    unpublished_query: &str,
) {
    let repository =
        RepositoryIdentityTextV1::decode(REPOSITORY_ID).expect("fixture identity should decode");
    let reader = OwnedSqliteReader::start(database, deadline())
        .expect("reader should retain the prior active generation");
    let prior = reader
        .search(
            repository,
            "Widget",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("prior active facts should remain searchable");
    assert_eq!(prior.generation(), expected_generation);
    assert!(!prior.hits().is_empty());
    let unpublished = reader
        .search(
            repository,
            unpublished_query,
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("failed candidate must not replace the prior generation");
    assert_eq!(unpublished.generation(), expected_generation);
    assert!(unpublished.hits().is_empty());
    reader
        .shutdown(deadline())
        .expect("reader should shut down");
}

#[test]
fn source_change_after_graph_staging_preserves_the_previous_active_generation() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    let first = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("seed generation should activate");

    let error = index_local_rust_repository_with_hooks(
        request,
        Arc::new(AtomicBool::new(false)),
        || {},
        || {
            fs::write(
                repository.join("src/lib.rs"),
                "pub struct Mutated;\nimpl Mutated { pub fn run() {} }\n",
            )
            .expect("post-staging mutation should be written");
        },
    )
    .expect_err("the final source fence must reject a post-staging mutation");
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains(repository.to_string_lossy().as_ref()));
    assert!(!rendered.contains("Mutated"));
    assert!(matches!(
        &error,
        LocalIndexError::FinalSourceFence {
            source: crate::LocalSourceSnapshotFenceError::SourceChanged
        }
    ));

    assert_prior_generation_readable(&database, first.generation(), "Mutated");

    let converged = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("a fresh reconciliation should publish the changed source");
    assert_ne!(converged.generation(), first.generation());
    assert!(converged.source_epoch() > first.source_epoch());
}

#[test]
fn cancellation_after_graph_staging_preserves_the_previous_active_generation() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    let first = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("seed generation should activate");
    fs::write(
        repository.join("src/lib.rs"),
        "pub struct CancelledCandidate;\nimpl CancelledCandidate { pub fn run() {} }\n",
    )
    .expect("candidate source should be written");
    let cancelled = Arc::new(AtomicBool::new(false));

    let error = index_local_rust_repository_with_hooks(
        request,
        Arc::clone(&cancelled),
        || {},
        || cancelled.store(true, Ordering::Release),
    )
    .expect_err("cancellation before the final fence must abort publication");
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains(repository.to_string_lossy().as_ref()));
    assert!(!rendered.contains("CancelledCandidate"));
    assert!(matches!(
        &error,
        LocalIndexError::FinalSourceFence {
            source: crate::LocalSourceSnapshotFenceError::Cancelled
        }
    ));
    assert_prior_generation_readable(&database, first.generation(), "CancelledCandidate");
}

#[cfg(unix)]
#[test]
fn database_alias_after_graph_staging_preserves_the_previous_active_generation() {
    let directory = TempDirectory::new();
    let repository = fixture_repository(&directory);
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
    let first = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("seed generation should activate");
    fs::write(
        repository.join("src/lib.rs"),
        "pub struct AliasedCandidate;\nimpl AliasedCandidate { pub fn run() {} }\n",
    )
    .expect("candidate source should be written");
    let database_alias = directory.0.join("late-database-alias");

    let error = index_local_rust_repository_with_hooks(
        request,
        Arc::new(AtomicBool::new(false)),
        || {},
        || {
            fs::hard_link(&database, &database_alias)
                .expect("database hard-link alias should be created after graph staging");
        },
    )
    .expect_err("the final fence must reject a late database alias");
    fs::remove_file(&database_alias).expect("fixture database alias should be removed");
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains(repository.to_string_lossy().as_ref()));
    assert!(!rendered.contains(database.to_string_lossy().as_ref()));
    assert!(!rendered.contains("AliasedCandidate"));
    assert!(matches!(
        &error,
        LocalIndexError::FinalSourceFence {
            source: crate::LocalSourceSnapshotFenceError::SourceChanged
        }
    ));
    assert_prior_generation_readable(&database, first.generation(), "AliasedCandidate");
}
