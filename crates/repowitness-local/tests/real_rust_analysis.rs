//! Real-workspace integration for the complete local Rust preparation slice.

#![cfg(unix)]

use std::{
    error::Error,
    io,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

use repowitness_analysis::RustSymbolKind;
use repowitness_application::RustArtifactIdentity;
use repowitness_domain::{AnalysisSchemaDigest, ConfigurationDigest, ProducerManifestDigest};
use repowitness_local::{LocalRustIndexLimits, prepare_local_rust_index};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn workspace_root() -> TestResult<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("workspace root is unavailable").into())
}

fn artifact_identity() -> RustArtifactIdentity {
    RustArtifactIdentity::new(
        ProducerManifestDigest::new([1; 32]),
        ConfigurationDigest::new([2; 32]),
        AnalysisSchemaDigest::new([3; 32]),
        1,
    )
}

#[test]
fn workspace_vertical_slice_prepares_and_revalidates_every_rust_source() -> TestResult {
    let root = workspace_root()?;
    let cancelled = AtomicBool::new(false);
    let local = prepare_local_rust_index(
        &root,
        artifact_identity(),
        LocalRustIndexLimits::default(),
        &cancelled,
    )?;
    let prepared = local.prepared();
    let mut saw_analyzer = false;
    let mut saw_git_discovery = false;

    for (ordinal, file) in prepared.files().iter().enumerate() {
        let analysis = file.analysis();
        assert!(
            !analysis.has_syntax_errors(),
            "workspace Rust source ordinal {ordinal} contains syntax errors"
        );
        saw_analyzer |= analysis.facts().iter().any(|fact| {
            fact.kind() == RustSymbolKind::Struct && fact.qualified_name() == "RustSourceAnalyzer"
        });
        saw_git_discovery |= analysis.facts().iter().any(|fact| {
            fact.kind() == RustSymbolKind::Function
                && fact.qualified_name() == "discover_repository_paths"
        });
    }

    assert!(local.selected_rust_files() >= 20);
    assert!(prepared.total_facts() >= 200);
    assert!(saw_analyzer);
    assert!(saw_git_discovery);
    Ok(())
}

#[test]
#[ignore = "requires REPOWITNESS_REAL_REPOSITORY to identify a Rust Git worktree"]
fn configured_repository_prepares_and_revalidates_every_rust_source() -> TestResult {
    let configured = std::env::var_os("REPOWITNESS_REAL_REPOSITORY")
        .ok_or_else(|| io::Error::other("REPOWITNESS_REAL_REPOSITORY is required"))?;
    let configured = Path::new(&configured);
    let root = if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        workspace_root()?.join(configured)
    };
    let cancelled = AtomicBool::new(false);
    let local = prepare_local_rust_index(
        &root,
        artifact_identity(),
        LocalRustIndexLimits::default(),
        &cancelled,
    )?;
    let prepared = local.prepared();

    assert!(
        local.selected_rust_files() > 0,
        "the configured repository must contain Rust source"
    );
    assert!(
        prepared.total_facts() > 0,
        "the configured repository must produce Rust facts"
    );
    for (ordinal, file) in prepared.files().iter().enumerate() {
        assert!(
            !file.analysis().has_syntax_errors(),
            "configured Rust source ordinal {ordinal} contains syntax errors"
        );
    }

    println!(
        "prepared {} Rust files, {} source bytes, and {} facts with {} syntax errors",
        local.selected_rust_files(),
        prepared.total_source_bytes(),
        prepared.total_facts(),
        prepared.total_syntax_error_nodes()
    );
    Ok(())
}
