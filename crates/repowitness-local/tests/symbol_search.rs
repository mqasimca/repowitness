//! End-to-end typed declaration discovery across persisted Phase 0 languages.

use std::{
    process::Command,
    sync::{Arc, atomic::AtomicBool},
};

use repowitness_analysis::RustSymbolKind;
use repowitness_application::{SourceLanguage, SymbolSearchNameMatch};
use repowitness_domain::EvidenceLocation;
use repowitness_local::{
    LocalIndexRequest, LocalSymbolSearchRequest, index_local_repository, search_local_symbols,
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
fn exact_prefix_and_typed_filters_remain_generation_pinned_and_non_relational() {
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
    .expect("mixed-language fixture should index");

    let exact = search_local_symbols(
        LocalSymbolSearchRequest::new(
            &database,
            REPOSITORY_ID,
            "run",
            SymbolSearchNameMatch::Exact,
        ),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("exact declaration search should complete");
    assert_eq!(exact.generation(), &indexed.generation());
    assert_eq!(exact.claim().returned_matches(), 5);
    assert_eq!(exact.claim().total_matches(), 5);
    let exact_paths = exact
        .evidence()
        .as_slice()
        .iter()
        .map(|evidence| {
            let EvidenceLocation::SymbolOccurrence(occurrence) = evidence.identity().location()
            else {
                panic!("typed search must return syntax occurrences");
            };
            (
                occurrence.language(),
                occurrence.kind(),
                std::str::from_utf8(evidence.identity().path().as_bytes())
                    .expect("fixture paths are UTF-8"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        exact_paths,
        [
            (
                SourceLanguage::Go,
                RustSymbolKind::Function,
                "go/service.go"
            ),
            (SourceLanguage::Rust, RustSymbolKind::Function, "src/lib.rs"),
            (
                SourceLanguage::Python,
                RustSymbolKind::Function,
                "tools/tool.py"
            ),
            (
                SourceLanguage::TypeScript,
                RustSymbolKind::Function,
                "web/app.ts"
            ),
            (
                SourceLanguage::Tsx,
                RustSymbolKind::Function,
                "web/view.tsx"
            ),
        ]
    );

    let prefix = search_local_symbols(
        LocalSymbolSearchRequest::new(&database, REPOSITORY_ID, "r", SymbolSearchNameMatch::Prefix)
            .with_filters(
                Some(SourceLanguage::TypeScript),
                Some(RustSymbolKind::Function),
                Some("web/"),
            ),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("filtered prefix declaration search should complete");
    assert_eq!(prefix.claim().returned_matches(), 1);
    assert_eq!(prefix.claim().total_matches(), 1);
    assert_eq!(prefix.evidence().as_slice().len(), 1);
    assert_eq!(prefix.notices().as_slice().len(), 2);
    assert!(prefix.notices().as_slice().iter().all(|notice| matches!(
        notice.kind(),
        repowitness_domain::ResultNoticeKind::Limitation
    )));
}

fn write_supported_language_sources(repository: &std::path::Path) {
    for (relative, source) in [
        ("go/service.go", "package service\n\nfunc run() {}\n"),
        ("src/lib.rs", "pub fn run() {}\n"),
        ("tools/tool.py", "def run():\n    return None\n"),
        ("web/app.ts", "export function run(): void {}\n"),
        ("web/view.tsx", "export function run(): void {}\n"),
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
            "src/lib.rs",
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
