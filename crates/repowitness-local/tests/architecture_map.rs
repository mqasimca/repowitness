//! End-to-end source-inventory coverage for every supported Phase 0 language.

use std::{
    process::Command,
    sync::{Arc, atomic::AtomicBool},
};

use repowitness_application::SourceLanguage;
use repowitness_local::{
    LocalArchitectureMapRequest, LocalIndexRequest, index_local_repository, map_local_architecture,
};

#[allow(dead_code)]
#[path = "phase0_product_loop/mod.rs"]
mod fixture;

const MIGRATION_TIMESTAMP: u64 = 1_722_000_000_000;
const REPOSITORY_ID: &str = concat!(
    "rwi1:h:",
    "A5A5A5A5A5A5A5A5",
    "A5A5A5A5A5A5A5A5",
    "A5A5A5A5A5A5A5A5",
    "A5A5A5A5A5A5A5A5"
);

#[test]
fn map_is_generation_pinned_complete_by_language_and_explicitly_truncated() {
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

    let complete = map_local_architecture(
        LocalArchitectureMapRequest::new(&database, REPOSITORY_ID)
            .with_max_files(5)
            .expect("five is valid"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("the complete active generation should map");

    assert_eq!(complete.generation(), &indexed.generation());
    assert_eq!(complete.total_files(), 5);
    assert!(!complete.truncated());
    assert_eq!(complete.files().len(), 5);
    assert_eq!(
        complete
            .files()
            .iter()
            .map(|file| std::str::from_utf8(file.path().as_bytes())
                .expect("fixture paths are UTF-8"))
            .collect::<Vec<_>>(),
        [
            "go/service.go",
            "src/lib.rs",
            "tools/tool.py",
            "web/app.ts",
            "web/view.tsx"
        ]
    );
    assert_eq!(
        complete
            .language_summaries()
            .iter()
            .map(|summary| (summary.language(), summary.file_count()))
            .collect::<Vec<_>>(),
        [
            (SourceLanguage::Go, 1),
            (SourceLanguage::Python, 1),
            (SourceLanguage::Rust, 1),
            (SourceLanguage::Tsx, 1),
            (SourceLanguage::TypeScript, 1),
        ]
    );
    assert_eq!(
        complete.total_declarations(),
        complete
            .language_summaries()
            .iter()
            .map(|summary| summary.declaration_count())
            .sum::<u64>()
    );
    assert!(
        complete
            .files()
            .iter()
            .all(|file| file.declaration_count() > 0)
    );

    let limited = map_local_architecture(
        LocalArchitectureMapRequest::new(&database, REPOSITORY_ID)
            .with_max_files(3)
            .expect("three is valid"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("the bounded active generation should map");
    assert_eq!(limited.total_files(), 5);
    assert_eq!(limited.files().len(), 3);
    assert!(limited.truncated());
    assert_eq!(limited.language_summaries(), complete.language_summaries());
}

fn write_supported_language_sources(repository: &std::path::Path) {
    for (relative, source) in [
        ("go/service.go", "package service\n\nfunc Run() {}\n"),
        ("tools/tool.py", "def run():\n    return None\n"),
        ("web/app.ts", "export function run(): void {}\n"),
        ("web/view.tsx", "export const View = () => <div />;\n"),
    ] {
        let path = repository.join(relative);
        std::fs::create_dir_all(path.parent().expect("fixture source has a parent"))
            .expect("fixture source parent should be created");
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
            "go/service.go",
            "tools/tool.py",
            "web/app.ts",
            "web/view.tsx",
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
            "add supported source languages",
        ])
        .current_dir(repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("Git fixture command should start");
    assert!(status.success(), "Git fixture command should succeed");
}
