//! End-to-end repository-scoped raw test-marker navigation.

use std::{
    process::Command,
    sync::{Arc, atomic::AtomicBool},
};

use repowitness_analysis::RawSyntaxSiteKind;
use repowitness_application::SourceLanguage;
use repowitness_local::{
    LocalIndexRequest, LocalTestMarkersRequest, index_local_repository, read_local_test_markers,
};

#[allow(dead_code)]
#[path = "phase0_product_loop/mod.rs"]
mod fixture;

const MIGRATION_TIMESTAMP: u64 = 1_722_000_000_000;
const REPOSITORY_ID: &str = concat!(
    "rwi1:h:",
    "D8D8D8D8D8D8D8D8",
    "D8D8D8D8D8D8D8D8",
    "D8D8D8D8D8D8D8D8",
    "D8D8D8D8D8D8D8D8"
);

#[test]
fn raw_markers_are_generation_pinned_filtered_and_explicitly_truncated() {
    let directory = fixture::TempDirectory::new();
    let repository = directory.repository();
    let database = directory.database();
    fixture::initialize_repository(&repository);
    write_sources(&repository);
    commit_sources(&repository);

    let indexed = index_local_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, MIGRATION_TIMESTAMP),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("fixture should index");

    let complete = read_local_test_markers(
        LocalTestMarkersRequest::new(&database, REPOSITORY_ID)
            .with_max_results(10)
            .expect("ten is valid"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("marker read should complete");
    assert_eq!(complete.generation(), &indexed.generation());
    assert_eq!(complete.total_markers(), 2);
    assert_eq!(complete.markers().len(), 2);
    assert!(!complete.truncated());
    assert_eq!(
        marker_language_coverage(&complete),
        [
            (SourceLanguage::Rust, 2, 2, 0, 2),
            (SourceLanguage::TypeScript, 1, 0, 1, 0),
        ]
    );
    assert!(complete.markers().iter().all(|marker| {
        marker.site().kind() == RawSyntaxSiteKind::TestMarker
            && marker.language() == SourceLanguage::Rust
    }));
    assert_eq!(
        complete
            .markers()
            .iter()
            .map(|marker| std::str::from_utf8(marker.path().as_bytes()).expect("fixture paths"))
            .collect::<Vec<_>>(),
        ["src/lib.rs", "tests/integration.rs"]
    );

    let limited = read_local_test_markers(
        LocalTestMarkersRequest::new(&database, REPOSITORY_ID)
            .with_max_results(1)
            .expect("one is valid"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("bounded marker read should complete");
    assert_eq!(limited.total_markers(), 2);
    assert_eq!(limited.markers().len(), 1);
    assert!(limited.truncated());
}

#[test]
fn marker_language_coverage_respects_filters_and_explicitly_reports_unsupported_extraction() {
    let directory = fixture::TempDirectory::new();
    let repository = directory.repository();
    let database = directory.database();
    fixture::initialize_repository(&repository);
    write_sources(&repository);
    commit_sources(&repository);
    index_local_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, MIGRATION_TIMESTAMP),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("fixture should index");

    let filtered = read_local_test_markers(
        LocalTestMarkersRequest::new(&database, REPOSITORY_ID)
            .with_filters(Some(SourceLanguage::Rust), Some("tests/"))
            .with_max_results(10)
            .expect("ten is valid"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("filtered marker read should complete");
    assert_eq!(filtered.total_markers(), 1);
    assert_eq!(filtered.markers().len(), 1);
    assert_eq!(
        filtered.markers()[0].path().as_bytes(),
        b"tests/integration.rs"
    );
    assert_eq!(
        marker_language_coverage(&filtered),
        [(SourceLanguage::Rust, 1, 1, 0, 1)]
    );

    let unsupported = read_local_test_markers(
        LocalTestMarkersRequest::new(&database, REPOSITORY_ID)
            .with_filters(Some(SourceLanguage::TypeScript), None)
            .with_max_results(10)
            .expect("ten is valid"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("unsupported marker feature remains an explicit successful read");
    assert_eq!(unsupported.total_markers(), 0);
    assert!(unsupported.markers().is_empty());
    assert_eq!(
        marker_language_coverage(&unsupported),
        [(SourceLanguage::TypeScript, 1, 0, 1, 0)]
    );
}

#[test]
fn test_marker_reads_reject_a_mismatched_raw_projection_profile() {
    let directory = fixture::TempDirectory::new();
    let repository = directory.repository();
    let database = directory.database();
    fixture::initialize_repository(&repository);
    write_sources(&repository);
    commit_sources(&repository);
    let indexed = index_local_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, MIGRATION_TIMESTAMP),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("fixture should index");

    let connection = rusqlite::Connection::open(&database).expect("fixture database opens");
    connection
        .execute_batch("DROP TRIGGER generation_syntax_site_requirements_no_update;")
        .expect("test-only immutability guard should be removable");
    connection
        .execute(
            "UPDATE generation_syntax_site_requirements
             SET site_profile_version = site_profile_version + 1
             WHERE generation_id = ?1",
            [indexed.generation().get()],
        )
        .expect("test-only corruption should apply");

    assert!(
        read_local_test_markers(
            LocalTestMarkersRequest::new(&database, REPOSITORY_ID),
            Arc::new(AtomicBool::new(false)),
        )
        .is_err(),
        "a completed raw projection with a mismatched profile must fail closed"
    );
}

fn marker_language_coverage(
    result: &repowitness_local::LocalTestMarkersResult,
) -> Vec<(SourceLanguage, u64, u64, u64, u64)> {
    result
        .language_coverage()
        .iter()
        .map(|coverage| {
            (
                coverage.language(),
                coverage.indexed_files(),
                coverage.supported_files(),
                coverage.unsupported_files(),
                coverage.emitted_markers(),
            )
        })
        .collect()
}

fn write_sources(repository: &std::path::Path) {
    for (relative, source) in [
        ("src/lib.rs", "#[test]\nfn unit_case() {}\n"),
        (
            "tests/integration.rs",
            "#[test]\nfn integration_case() {}\n",
        ),
        ("web/app.ts", "export function ordinary(): void {}\n"),
    ] {
        let path = repository.join(relative);
        std::fs::create_dir_all(path.parent().expect("fixture source parent"))
            .expect("fixture source directory should exist");
        std::fs::write(path, source).expect("fixture source should be written");
    }
}

fn commit_sources(repository: &std::path::Path) {
    let status = Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "add",
            "--",
            "src/lib.rs",
            "tests/integration.rs",
            "web/app.ts",
        ])
        .current_dir(repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("Git fixture command should start");
    assert!(status.success(), "Git fixture command should succeed");
    let status = Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--quiet",
            "-m",
            "add marker sources",
        ])
        .current_dir(repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("Git fixture command should start");
    assert!(status.success(), "Git fixture command should succeed");
}
