use std::fs;

use repowitness_application::{PackageScope, SourceLanguage, phase1_rust_graph_artifact_identity};

use super::fixtures::{
    TempDirectory, connected, default_configuration, fixture_repository, index, repository,
    request, scope, slot, source_slot,
};
use crate::local_index::{
    connected_scope_artifact_identity, connected_scope_configuration,
    connected_scope_source_artifact_identities, phase0_local_source_artifact_identities,
};

#[test]
fn connected_whole_scope_has_distinct_semantic_identity_from_legacy() {
    let configuration = default_configuration();
    let whole = PackageScope::whole_repository();
    let scoped = connected_scope_configuration(configuration.digest(), &whole);
    let legacy_artifacts = phase0_local_source_artifact_identities();
    let scoped_artifacts = connected_scope_source_artifact_identities(scoped);
    let scoped_graph =
        connected_scope_artifact_identity(phase1_rust_graph_artifact_identity(), scoped);

    assert_ne!(scoped, configuration.digest());
    for language in [
        SourceLanguage::Rust,
        SourceLanguage::Go,
        SourceLanguage::TypeScript,
        SourceLanguage::Tsx,
        SourceLanguage::Python,
    ] {
        assert_ne!(
            scoped_artifacts.for_language(language).configuration(),
            legacy_artifacts.for_language(language).configuration()
        );
    }
    assert_ne!(
        scoped_graph.configuration(),
        phase1_rust_graph_artifact_identity().configuration()
    );
}

#[test]
fn unchanged_scope_reuses_but_scope_change_never_reuses_mismatched_artifacts() {
    let directory = TempDirectory::new();
    let repository_path = fixture_repository(&directory, "scope-reuse");
    let database = directory.database();
    let configuration = default_configuration();
    let connected_workspace = connected(4);
    let first = index(request(
        connected_workspace,
        &database,
        vec![slot(
            source_slot(1),
            repository(1),
            &repository_path,
            "worktree-head",
            scope(b"pkg_a"),
            &configuration,
        )],
    ));
    assert_eq!(first.source_slots()[0].analyzed_files(), 1);
    assert_eq!(first.source_slots()[0].reused_files(), 0);

    let changed_scope = index(request(
        connected_workspace,
        &database,
        vec![slot(
            source_slot(1),
            repository(1),
            &repository_path,
            "worktree-head",
            scope(b"pkg_b"),
            &configuration,
        )],
    ));
    assert_ne!(changed_scope.view(), first.view());
    assert_eq!(changed_scope.source_slots()[0].analyzed_files(), 1);
    assert_eq!(changed_scope.source_slots()[0].reused_files(), 0);

    let unchanged_scope = index(request(
        connected_workspace,
        &database,
        vec![slot(
            source_slot(1),
            repository(1),
            &repository_path,
            "worktree-head",
            scope(b"pkg_b"),
            &configuration,
        )],
    ));
    assert_eq!(unchanged_scope.view(), changed_scope.view());
    assert_eq!(unchanged_scope.source_slots()[0].analyzed_files(), 0);
    assert_eq!(unchanged_scope.source_slots()[0].reused_files(), 1);

    fs::write(
        repository_path.join("pkg_b/src/lib.rs"),
        b"pub struct PackageB;\nimpl PackageB { pub fn changed() {} }\n",
    )
    .expect("source change should be written");
    let changed_source = index(request(
        connected_workspace,
        &database,
        vec![slot(
            source_slot(1),
            repository(1),
            &repository_path,
            "worktree-head",
            scope(b"pkg_b"),
            &configuration,
        )],
    ));
    assert_ne!(changed_source.view(), unchanged_scope.view());
    assert_eq!(changed_source.source_slots()[0].analyzed_files(), 1);
}

#[test]
fn explicit_deleted_root_publishes_truthful_empty_scope_coverage() {
    let directory = TempDirectory::new();
    let repository_path = fixture_repository(&directory, "empty-scope");
    let database = directory.database();
    let configuration = default_configuration();
    let report = index(request(
        connected(13),
        &database,
        vec![slot(
            source_slot(1),
            repository(1),
            &repository_path,
            "worktree-head",
            scope(b"deleted-package"),
            &configuration,
        )],
    ));
    let source = report.source_slots()[0];

    assert_eq!(source.discovered_paths(), 3);
    assert_eq!(source.indexed_files(), 0);
    assert_eq!(source.skipped_paths(), 3);
    assert_eq!(source.analyzed_files(), 0);
    assert_eq!(source.reused_files(), 0);
}
