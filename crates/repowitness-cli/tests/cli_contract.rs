//! Black-box regression coverage for the installed command contract.

use std::{
    ffi::OsStr,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{ChildStdin, ChildStdout, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

const REPOSITORY_ID: &str = concat!(
    "rwi1:h:",
    "B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2"
);
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repowitness-cli-contract-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture directory should be created");
        Self(path)
    }

    fn repository(&self) -> PathBuf {
        self.0.join("repository")
    }

    fn database(&self) -> PathBuf {
        self.0.join("index.sqlite3")
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn repowitness(arguments: &[&str]) -> Output {
    repowitness_os(arguments.iter().map(OsStr::new))
}

fn repowitness_os<'a>(arguments: impl IntoIterator<Item = &'a OsStr>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_repowitness"))
        .args(arguments)
        .output()
        .expect("the RepoWitness binary must start")
}

fn fixture_repository(directory: &TempDirectory) -> PathBuf {
    let repository = directory.repository();
    fs::create_dir_all(repository.join("src")).expect("fixture source directory should be created");
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .arg(&repository)
        .status()
        .expect("Git should start");
    assert!(status.success());
    fs::write(
        repository.join("src/lib.rs"),
        "pub struct Widget;\nimpl Widget { pub fn run() {} }\n",
    )
    .expect("Rust fixture should be written");
    fs::write(
        repository.join("src/widget.go"),
        "package fixture\n\ntype Gadget struct{}\n\nfunc (Gadget) Launch() {}\n",
    )
    .expect("Go fixture should be written");
    fs::write(repository.join("README.md"), "fixture\n")
        .expect("unsupported fixture should be written");
    let status = Command::new("git")
        .current_dir(&repository)
        .args(["add", "--", "src/lib.rs", "src/widget.go", "README.md"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    repository
}

fn index(repository: &Path, database: &Path, identity: &str) -> Output {
    repowitness_os([
        OsStr::new("index"),
        OsStr::new("--repository-id"),
        OsStr::new(identity),
        OsStr::new("--database"),
        database.as_os_str(),
        repository.as_os_str(),
    ])
}

fn search(database: &Path, identity: &str, query: &str, limit: &str) -> Output {
    repowitness_os([
        OsStr::new("search"),
        OsStr::new("--repository-id"),
        OsStr::new(identity),
        OsStr::new("--database"),
        database.as_os_str(),
        OsStr::new("--query"),
        OsStr::new(query),
        OsStr::new("--limit"),
        OsStr::new(limit),
    ])
}

fn report_value<'a>(report: &'a str, key: &str) -> &'a str {
    let prefix = format!("{key}=");
    report
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .expect("report field must be present")
}

fn symbol_get_from_search(
    repository: &Path,
    database: &Path,
    identity: &str,
    search_report: &str,
) -> Output {
    repowitness_os([
        OsStr::new("symbol-get"),
        OsStr::new("--repository-id"),
        OsStr::new(identity),
        OsStr::new("--database"),
        database.as_os_str(),
        OsStr::new("--root"),
        repository.as_os_str(),
        OsStr::new("--snapshot"),
        OsStr::new(report_value(search_report, "snapshot_sha256")),
        OsStr::new("--generation"),
        OsStr::new(report_value(search_report, "generation")),
        OsStr::new("--path"),
        OsStr::new(report_value(search_report, "match_0_path")),
        OsStr::new("--content"),
        OsStr::new(report_value(search_report, "match_0_content_sha256")),
        OsStr::new("--artifact"),
        OsStr::new(report_value(search_report, "match_0_artifact_sha256")),
        OsStr::new("--fact"),
        OsStr::new(report_value(search_report, "match_0_fact_ordinal")),
    ])
}

fn assert_symbol_get_success(
    output: Output,
    expected_language: &str,
    expected_name: &str,
    expected_source: &str,
) {
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let report = String::from_utf8(output.stdout).expect("symbol report must be UTF-8");
    assert!(report.contains("status=ok\noperation=symbol-get\n"));
    assert!(report.contains("schema_version=2\n"));
    assert!(report.contains("symbol_profile=3\n"));
    assert!(report.contains("resolution=confirmed\n"));
    assert!(report.contains("symbol_found=true\n"));
    assert!(report.contains("evidence_tier=syntax\n"));
    assert_eq!(report_value(&report, "language"), expected_language);
    assert_eq!(report_value(&report, "name"), expected_name);
    assert_eq!(report_value(&report, "declaration_encoding"), "utf8");
    assert_eq!(
        serde_json::from_str::<String>(report_value(&report, "declaration_data_json"))
            .expect("declaration must be one JSON string"),
        expected_source
    );
}

fn assert_stale_symbol_rejected(output: Output, repository: &Path, forbidden_digest: Option<&str>) {
    assert_eq!(output.status.code(), Some(70));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("diagnostic must be UTF-8");
    assert!(!diagnostic.contains(repository.to_string_lossy().as_ref()));
    if let Some(digest) = forbidden_digest {
        assert!(!diagnostic.contains(digest));
    }
}

fn modify_source_and_assert_stale_rejection(repository: &Path, database: &Path) {
    let searched = search(database, REPOSITORY_ID, "Widget", "1");
    assert!(searched.status.success());
    let searched = String::from_utf8(searched.stdout).expect("search report must be UTF-8");
    fs::write(
        repository.join("src/lib.rs"),
        "pub struct Widget;\nimpl Widget { pub fn run() {} }\npub fn changed() {}\n",
    )
    .expect("changed Rust fixture should be written");
    let output = symbol_get_from_search(repository, database, REPOSITORY_ID, &searched);
    assert_stale_symbol_rejected(
        output,
        repository,
        Some(report_value(&searched, "match_0_content_sha256")),
    );
}

fn assert_changed_symbol_contract(repository: &Path, database: &Path) {
    assert_changed_search_contract(database);
    let searched = search(database, REPOSITORY_ID, "changed", "1");
    assert!(searched.status.success());
    let searched = String::from_utf8(searched.stdout).expect("search report must be UTF-8");
    assert_symbol_get_success(
        symbol_get_from_search(repository, database, REPOSITORY_ID, &searched),
        "rust",
        "changed",
        "pub fn changed() {}",
    );
}

fn assert_widget_search_contract(database: &Path) -> String {
    let searched = search(database, REPOSITORY_ID, "Widget", "1");
    assert!(searched.status.success());
    assert!(searched.stderr.is_empty());
    let searched = String::from_utf8(searched.stdout).expect("search report must be UTF-8");
    assert!(searched.contains("status=ok\noperation=search\n"));
    assert!(searched.contains("query_profile=3\n"));
    assert!(searched.contains("generation=1\n"));
    assert!(searched.contains("resolution=confirmed\n"));
    assert!(searched.contains("matches_returned=1\n"));
    assert!(searched.contains("matches_total=2\n"));
    assert!(searched.contains("coverage_searched=2\n"));
    assert!(searched.contains("coverage_skipped=1\n"));
    assert!(searched.contains("coverage_truncated=1\n"));
    assert!(searched.contains("match_0_path=rwp1:h:7372632F6C69622E7273\n"));
    assert!(searched.contains("match_0_fact_ordinal=0\n"));
    assert!(searched.contains("match_0_evidence_tier=syntax\n"));
    assert!(searched.contains("match_0_language=rust\n"));
    assert!(searched.contains("match_0_content_sha256="));
    assert!(searched.contains("match_0_artifact_sha256="));
    assert!(searched.contains("match_0_producer_manifest_sha256="));
    assert!(!searched.contains(REPOSITORY_ID));
    assert!(!searched.contains(database.to_string_lossy().as_ref()));
    searched
}

fn assert_go_search_contract(repository: &Path, database: &Path) {
    let searched = search(database, REPOSITORY_ID, "Launch", "1");
    assert!(searched.status.success());
    assert!(searched.stderr.is_empty());
    let searched = String::from_utf8(searched.stdout).expect("search report must be UTF-8");
    assert!(searched.contains("matches_returned=1\nmatches_total=1\n"));
    assert!(searched.contains("match_0_language=go\n"));
    assert!(searched.contains("match_0_kind=method\n"));
    assert!(searched.contains("match_0_name=Launch\n"));
    assert_symbol_get_success(
        symbol_get_from_search(repository, database, REPOSITORY_ID, &searched),
        "go",
        "Launch",
        "func (Gadget) Launch() {}",
    );
}

fn assert_absent_search_contract(database: &Path) {
    let absent = search(database, REPOSITORY_ID, "definitely_absent_symbol", "20");
    assert!(absent.status.success());
    assert!(absent.stderr.is_empty());
    let absent = String::from_utf8(absent.stdout).expect("search report must be UTF-8");
    assert!(absent.contains("resolution=unresolved\n"));
    assert!(absent.contains("matches_returned=0\nmatches_total=0\n"));
    assert!(absent.contains("coverage_unresolved=1\n"));
    assert!(!absent.contains("definitely_absent_symbol"));
}

fn assert_changed_search_contract(database: &Path) {
    let changed = search(database, REPOSITORY_ID, "changed", "20");
    assert!(changed.status.success());
    assert!(changed.stderr.is_empty());
    let changed = String::from_utf8(changed.stdout).expect("search report must be UTF-8");
    assert!(changed.contains("generation=3\n"));
    assert!(changed.contains("matches_returned=1\nmatches_total=1\n"));
    assert!(changed.contains("match_0_name=changed\n"));
}

include!("cli_contract/cli_behavior.rs");
include!("cli_contract/mcp_contract.rs");
