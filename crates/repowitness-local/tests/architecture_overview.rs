//! End-to-end bounded source-only architecture orientation across all supported languages.

use std::{
    process::Command,
    sync::{Arc, atomic::AtomicBool},
};

use repowitness_analysis::RustSymbolKind;
use repowitness_application::{ArchitectureOverviewSourceRoot, SourceLanguage};
use repowitness_local::{
    LocalArchitectureOverviewRequest, LocalIndexRequest, index_local_repository,
    overview_local_architecture,
};

#[allow(dead_code)]
#[path = "phase0_product_loop/mod.rs"]
mod fixture;

const MIGRATION_TIMESTAMP: u64 = 1_722_000_000_000;
const REPOSITORY_ID: &str = concat!(
    "rwi1:h:",
    "C7C7C7C7C7C7C7C7",
    "C7C7C7C7C7C7C7C7",
    "C7C7C7C7C7C7C7C7",
    "C7C7C7C7C7C7C7C7"
);

#[test]
fn overview_is_generation_pinned_structural_and_explicitly_truncated() {
    let directory = fixture::TempDirectory::new();
    let repository = directory.repository();
    let database = directory.database();
    fixture::initialize_repository(&repository);
    write_supported_language_sources(&repository);
    commit_sources(&repository);

    let indexed = index_local_repository(
        LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, MIGRATION_TIMESTAMP),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("the mixed-language fixture should index");

    let complete = overview_local_architecture(
        LocalArchitectureOverviewRequest::new(&database, REPOSITORY_ID)
            .with_limits(5, 5, 6)
            .expect("fixture limits should be valid"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("the active generation should have an overview");

    assert_eq!(complete.generation(), &indexed.generation());
    assert_eq!(complete.total_files(), 6);
    assert_eq!(complete.total_source_roots(), 5);
    assert_eq!(complete.total_entry_point_candidates(), 5);
    assert!(!complete.source_roots_truncated());
    assert!(!complete.entry_point_candidates_truncated());
    assert!(!complete.files_truncated());
    assert_eq!(
        complete
            .language_summaries()
            .iter()
            .map(|summary| (summary.language(), summary.file_count()))
            .collect::<Vec<_>>(),
        [
            (SourceLanguage::Go, 1),
            (SourceLanguage::Python, 2),
            (SourceLanguage::Rust, 1),
            (SourceLanguage::Tsx, 1),
            (SourceLanguage::TypeScript, 1),
        ]
    );
    assert_eq!(
        complete
            .kind_summaries()
            .iter()
            .filter(|summary| summary.kind() == RustSymbolKind::Function)
            .map(|summary| summary.declaration_count())
            .sum::<u64>(),
        6
    );
    assert_eq!(
        complete
            .source_roots()
            .iter()
            .map(|summary| summary.root())
            .collect::<Vec<_>>(),
        [
            &ArchitectureOverviewSourceRoot::RepositoryRoot,
            &ArchitectureOverviewSourceRoot::TopLevelDirectory(path("go")),
            &ArchitectureOverviewSourceRoot::TopLevelDirectory(path("src")),
            &ArchitectureOverviewSourceRoot::TopLevelDirectory(path("tools")),
            &ArchitectureOverviewSourceRoot::TopLevelDirectory(path("web")),
        ]
    );
    assert_eq!(
        complete
            .entry_point_candidates()
            .iter()
            .map(|candidate| {
                std::str::from_utf8(candidate.path().as_bytes())
                    .expect("fixture paths should be UTF-8")
            })
            .collect::<Vec<_>>(),
        [
            "go/service.go",
            "src/lib.rs",
            "tools/tool.py",
            "web/app.ts",
            "web/view.tsx"
        ]
    );
    assert!(complete.entry_point_candidates().iter().all(
        |candidate| candidate.occurrence().kind() == RustSymbolKind::Function
            && candidate.occurrence().name() == "main"
    ));

    let limited = overview_local_architecture(
        LocalArchitectureOverviewRequest::new(&database, REPOSITORY_ID)
            .with_limits(3, 3, 3)
            .expect("bounded fixture limits should be valid"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("bounded overview should complete");
    assert!(limited.source_roots_truncated());
    assert!(limited.entry_point_candidates_truncated());
    assert!(limited.files_truncated());
    assert_eq!(limited.source_roots().len(), 3);
    assert_eq!(limited.entry_point_candidates().len(), 3);
    assert_eq!(limited.files().len(), 3);
    assert_eq!(limited.kind_summaries(), complete.kind_summaries());
}

fn path(value: &str) -> repowitness_domain::RepositoryPath {
    repowitness_domain::RepositoryPath::try_from_bytes(
        value.as_bytes(),
        repowitness_domain::RepositoryPathLimits::new(128, 8),
    )
    .expect("fixture component should be valid")
}

fn write_supported_language_sources(repository: &std::path::Path) {
    for (relative, source) in [
        ("src/lib.rs", "pub fn main() {}\n"),
        ("go/service.go", "package service\n\nfunc main() {}\n"),
        ("tools/tool.py", "def main():\n    return None\n"),
        ("web/app.ts", "export function main(): void {}\n"),
        (
            "web/view.tsx",
            "export function main(): JSX.Element { return <div />; }\n",
        ),
        ("main.py", "def helper():\n    return None\n"),
    ] {
        let path = repository.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("fixture source parent should be created");
        }
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
            "go/service.go",
            "tools/tool.py",
            "web/app.ts",
            "web/view.tsx",
            "main.py",
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
            "add overview sources",
        ])
        .current_dir(repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("Git fixture commit should start");
    assert!(status.success(), "Git fixture commit should succeed");
}
