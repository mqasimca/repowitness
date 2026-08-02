//! End-to-end immutable path-only repository-topology coverage.

use std::{
    process::Command,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use repowitness_application::{
    RepositoryTopologyCategory, RepositoryTopologyEntry, resolve_configuration,
};
use repowitness_local::{
    LocalIndexRequest, LocalRepositoryTopologyRequest, LocalRetentionPins,
    LocalRetentionPlanRequest, index_local_repository, plan_local_retention,
    read_local_repository_topology,
};

#[allow(dead_code)]
#[path = "phase0_product_loop/mod.rs"]
mod fixture;

const MIGRATION_TIMESTAMP: u64 = 1_722_000_000_000;
const REPOSITORY_ID: &str = concat!(
    "rwi1:h:",
    "B6B6B6B6B6B6B6B6",
    "B6B6B6B6B6B6B6B6",
    "B6B6B6B6B6B6B6B6",
    "B6B6B6B6B6B6B6B6"
);

#[test]
fn topology_is_generation_pinned_path_only_and_explicitly_truncated() {
    let directory = fixture::TempDirectory::new();
    let repository = directory.repository();
    let database = directory.database();
    fixture::initialize_repository(&repository);
    write_topology_paths(&repository);
    commit_paths(&repository);
    std::fs::write(
        repository.join("private-local-note.txt"),
        "untracked fixture path must not enter topology\n",
    )
    .expect("untracked fixture path should be written");

    let indexed = index_local_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, MIGRATION_TIMESTAMP),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("fixture should index");
    let complete = read_local_repository_topology(
        LocalRepositoryTopologyRequest::new(&database, REPOSITORY_ID)
            .with_max_paths(1000)
            .expect("maximum path limit should be valid"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("active generation should have a topology receipt");

    assert_eq!(complete.generation(), &indexed.generation());
    assert_eq!(complete.coverage().omitted_paths(), 0);
    assert_eq!(
        complete.coverage().discovered_paths(),
        complete.total_paths()
    );
    assert!(!complete.truncated());
    assert_eq!(
        complete
            .category_summaries()
            .iter()
            .map(|summary| summary.category())
            .collect::<Vec<_>>(),
        RepositoryTopologyCategory::all(),
    );
    assert_complete_topology_entries(complete.entries());

    let limited = read_local_repository_topology(
        LocalRepositoryTopologyRequest::new(&database, REPOSITORY_ID)
            .with_max_paths(3)
            .expect("three should be valid"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("bounded active generation should be readable");
    assert_eq!(limited.total_paths(), complete.total_paths());
    assert_eq!(limited.entries().len(), 3);
    assert!(limited.truncated());
    assert_eq!(limited.category_summaries(), complete.category_summaries());
}

fn assert_complete_topology_entries(entries: &[RepositoryTopologyEntry]) {
    for category in RepositoryTopologyCategory::all() {
        assert!(entries.iter().any(|entry| entry.category() == category));
    }
    assert!(entries.iter().any(|entry| {
        entry.path().as_bytes() == b"crates/tool/Cargo.toml"
            && entry.category() == RepositoryTopologyCategory::PackageDescriptor
    }));
    assert!(entries.iter().any(|entry| {
        entry.path().as_bytes() == b"services/AGENTS.md"
            && entry.category() == RepositoryTopologyCategory::AgentInstruction
    }));
    assert!(
        !entries
            .iter()
            .any(|entry| entry.path().as_bytes() == b"private-local-note.txt")
    );
}

#[test]
fn topology_rejects_persisted_path_or_category_corruption() {
    let directory = fixture::TempDirectory::new();
    let repository = directory.repository();
    let database = directory.database();
    fixture::initialize_repository(&repository);
    write_topology_paths(&repository);
    commit_paths(&repository);

    let indexed = index_local_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, MIGRATION_TIMESTAMP),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("fixture should index");
    let connection = rusqlite::Connection::open(&database).expect("fixture database opens");
    connection
        .execute_batch("DROP TRIGGER generation_repository_topology_entries_no_update;")
        .expect("test-only immutable trigger removal should succeed");
    connection
        .execute(
            "UPDATE generation_repository_topology_entries
             SET category = 'other_tracked_file'
             WHERE generation_id = ?1 AND category = 'documentation'",
            [indexed.generation().get()],
        )
        .expect("test-only topology corruption should succeed");
    drop(connection);

    assert!(
        read_local_repository_topology(
            LocalRepositoryTopologyRequest::new(&database, REPOSITORY_ID),
            Arc::new(AtomicBool::new(false)),
        )
        .is_err(),
        "digest verification must fail closed after category corruption"
    );
}

#[test]
fn retention_estimates_every_generation_owned_topology_row() {
    let directory = fixture::TempDirectory::new();
    let repository = directory.repository();
    let database = directory.database();
    fixture::initialize_repository(&repository);
    write_topology_paths(&repository);
    commit_paths(&repository);
    for timestamp in [
        MIGRATION_TIMESTAMP,
        MIGRATION_TIMESTAMP + 1,
        MIGRATION_TIMESTAMP + 2,
        MIGRATION_TIMESTAMP + 3,
    ] {
        index_local_repository(
            LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, timestamp),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("fixture generation should index");
    }
    let configuration = resolve_configuration(&[]).expect("configuration");
    let before = plan_topology_retention(&database, &configuration);
    assert_eq!(before.candidate_count(), 1);

    let connection = rusqlite::Connection::open(&database).expect("fixture database opens");
    let candidate: i64 = connection
        .query_row(
            "SELECT generation_id FROM index_generations
             WHERE lifecycle_state != 'active' ORDER BY generation_id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("retention candidate generation should exist");
    let topology_rows: i64 = connection
        .query_row(
            "SELECT
                (SELECT count(*) FROM generation_repository_topology_entries
                 WHERE generation_id = ?1)
              + (SELECT count(*) FROM generation_repository_topology_publications
                 WHERE generation_id = ?1)
              + (SELECT count(*) FROM generation_repository_topology_requirements
                 WHERE generation_id = ?1)",
            [candidate],
            |row| row.get(0),
        )
        .expect("candidate topology rows should be countable");
    assert!(
        topology_rows > 3,
        "fixture should persist a topology inventory"
    );
    let topology_rows = u64::try_from(topology_rows).expect("topology row count is nonnegative");
    for statement in [
        "DELETE FROM generation_repository_topology_entries WHERE generation_id = ?1",
        "DELETE FROM generation_repository_topology_publications WHERE generation_id = ?1",
        "DELETE FROM generation_repository_topology_requirements WHERE generation_id = ?1",
    ] {
        connection
            .execute(statement, [candidate])
            .expect("test-only topology row deletion");
    }
    drop(connection);

    let after = plan_topology_retention(&database, &configuration);
    assert_eq!(after.candidate_count(), before.candidate_count());
    assert_eq!(
        before.estimated_rows(),
        after.estimated_rows() + topology_rows
    );
}

fn plan_topology_retention(
    database: &std::path::Path,
    configuration: &repowitness_application::ResolvedConfiguration,
) -> repowitness_local::LocalRetentionPlanReport {
    let request = LocalRetentionPlanRequest::try_new(
        database,
        MIGRATION_TIMESTAMP + 3,
        configuration,
        LocalRetentionPins::default(),
        Arc::new(AtomicBool::new(false)),
        Duration::from_secs(30),
    )
    .expect("retention plan request should be valid");
    plan_local_retention(request).expect("retention plan should complete")
}

fn write_topology_paths(repository: &std::path::Path) {
    for (relative, contents) in [
        ("AGENTS.md", "agent directions\n"),
        ("services/AGENTS.md", "nested agent directions\n"),
        ("README.md", "documentation\n"),
        ("Makefile", "all:\n\t@true\n"),
        (
            "Cargo.toml",
            "[package]\nname = 'fixture'\nversion = '0.0.0'\n",
        ),
        (
            "crates/tool/Cargo.toml",
            "[package]\nname = 'nested-fixture'\nversion = '0.0.0'\n",
        ),
        (".github/workflows/ci.yml", "name: ci\n"),
        (
            "config/local.toml",
            "value = 'sensitive-but-never-returned'\n",
        ),
        ("assets/blob.bin", "opaque bytes\n"),
    ] {
        let path = repository.join(relative);
        std::fs::create_dir_all(path.parent().expect("fixture path has parent"))
            .expect("fixture parent should be created");
        std::fs::write(path, contents).expect("fixture path should be written");
    }
}

fn commit_paths(repository: &std::path::Path) {
    for arguments in [
        ["add", "--all"].as_slice(),
        ["commit", "--quiet", "-m", "topology fixture"].as_slice(),
    ] {
        let status = Command::new("git")
            .args(arguments)
            .current_dir(repository)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .status()
            .expect("Git fixture command should start");
        assert!(status.success(), "Git fixture command should succeed");
    }
}
