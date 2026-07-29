use std::time::{Duration, Instant};

use repowitness_analysis::{RustGraphAnalysisError, RustGraphSiteKind};
use repowitness_application::{ImmutableRustSource, phase1_rust_graph_artifact_identity};
use repowitness_domain::{RepositoryPath, RepositoryPathLimits};

fn graph_test_path(value: &[u8]) -> RepositoryPath {
    RepositoryPath::try_from_bytes(value, RepositoryPathLimits::new(4_096, 64))
        .expect("test repository path must be valid")
}

fn graph_test_deadline() -> Instant {
    Instant::now()
        .checked_add(Duration::from_secs(10))
        .expect("short test deadline must fit")
}

#[test]
fn local_preparation_builds_raw_graph_sites_only_for_rust_sources() {
    let rust = ImmutableRustSource::new(
        graph_test_path(b"src/lib.rs"),
        b"use crate::model::Item;\npub fn target() {}\npub fn caller() { target(); }\n"
            .to_vec()
            .into_boxed_slice(),
    );
    let go = ImmutableRustSource::new_go(
        graph_test_path(b"cmd/main.go"),
        b"package main\nfunc Execute() {}\n"
            .to_vec()
            .into_boxed_slice(),
    );
    let cancelled = AtomicBool::new(false);

    let artifacts = prepare_local_rust_graph_artifacts(
        &[go, rust],
        phase1_rust_graph_artifact_identity(),
        &BTreeMap::new(),
        &cancelled,
        graph_test_deadline(),
    )
    .expect("bounded graph artifacts must prepare");

    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].path().as_bytes(), b"src/lib.rs");
    assert!(
        artifacts[0]
            .analysis()
            .sites()
            .iter()
            .any(|site| site.kind() == RustGraphSiteKind::Import)
    );
    assert!(
        artifacts[0]
            .analysis()
            .sites()
            .iter()
            .any(|site| site.kind() == RustGraphSiteKind::Call)
    );
    let identity = phase1_rust_graph_artifact_identity();
    assert_eq!(
        artifacts[0].key().analyzer_identity(),
        &identity.producer_manifest()
    );
    assert_eq!(
        artifacts[0].key().configuration_identity(),
        &identity.configuration()
    );
    assert_eq!(artifacts[0].key().schema_identity(), &identity.schema());
    assert!(!format!("{:?}", artifacts[0]).contains("src/lib.rs"));
}

#[test]
fn graph_preparation_honors_controls_even_without_rust_sources() {
    let source = ImmutableRustSource::new_go(
        graph_test_path(b"cmd/main.go"),
        b"package main\n".to_vec().into_boxed_slice(),
    );
    let cancelled = AtomicBool::new(true);
    assert!(matches!(
        prepare_local_rust_graph_artifacts(
            &[source],
            phase1_rust_graph_artifact_identity(),
            &BTreeMap::new(),
            &cancelled,
            graph_test_deadline()
        ),
        Err(RustGraphAnalysisError::Cancelled)
    ));

    let active = AtomicBool::new(false);
    assert!(matches!(
        prepare_local_rust_graph_artifacts(
            &[],
            phase1_rust_graph_artifact_identity(),
            &BTreeMap::new(),
            &active,
            Instant::now()
        ),
        Err(RustGraphAnalysisError::DeadlineExceeded)
    ));
}
