use std::{
    fs,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use super::*;

mod fixtures;
use fixtures::*;

#[test]
fn default_connected_workspace_deadlines_share_one_finite_refresh_budget() {
    let directory = TempDirectory::new();
    fixture_repository(directory.path(), "repo");
    let text = manifest(0xA1, &[source_table(1, 1, "repo")]);
    let (contents, parent) = admit_manifest(directory.path(), "workspace.toml", &text);
    let database = directory.join("index.sqlite3");
    let configuration = default_configuration();
    let request = request(&contents, &parent, &database, &configuration);

    assert_eq!(
        request.source_limits().deadline(),
        DEFAULT_LOCAL_CONNECTED_WORKSPACE_SOURCE_DEADLINE
    );
    assert_eq!(
        request.deadline(),
        DEFAULT_LOCAL_CONNECTED_WORKSPACE_DEADLINE
    );
    assert_eq!(
        DEFAULT_LOCAL_CONNECTED_WORKSPACE_SOURCE_DEADLINE,
        DEFAULT_LOCAL_CONNECTED_WORKSPACE_DEADLINE
    );
    assert_eq!(
        DEFAULT_LOCAL_CONNECTED_WORKSPACE_DEADLINE,
        Duration::from_secs(300)
    );
}

#[test]
fn facade_publishes_two_sources_and_returns_only_aggregate_coverage() {
    let directory = TempDirectory::new();
    fixture_repository(directory.path(), "repo-a");
    fixture_repository(directory.path(), "repo-b");
    let text = manifest(
        0xA1,
        &[source_table(1, 1, "repo-a"), source_table(2, 2, "repo-b")],
    );
    let (contents, parent) = admit_manifest(directory.path(), "workspace.toml", &text);
    let database = directory.join("index.sqlite3");
    let configuration = default_configuration();

    let report = index_local_connected_workspace(
        request(&contents, &parent, &database, &configuration)
            .with_source_limits(short_source_limits()),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("connected workspace should publish");

    assert_eq!(report.report_version(), 1);
    assert_eq!(report.manifest_schema_version(), 1);
    assert_eq!(report.view_receipt_version(), 1);
    assert_eq!(report.configuration_digest(), configuration.digest());
    assert_eq!(report.source_count(), 2);
    assert_eq!(report.generation_count(), 2);
    assert_eq!(report.recovered_generations(), 0);
    assert_eq!(report.outcome(), LocalConnectedWorkspaceOutcome::Published);
    assert_eq!(
        report.maintenance(),
        LocalConnectedWorkspaceMaintenance::Complete
    );
    assert_eq!(report.coverage().discovered_paths(), 4);
    assert_eq!(report.coverage().indexed_files(), 2);
    assert_eq!(report.coverage().skipped_policy_paths(), 0);
    assert_eq!(report.coverage().skipped_unsupported_paths(), 2);
    assert_eq!(report.coverage().skipped_paths(), 2);
    assert_eq!(report.coverage().reused_files(), 0);
    assert_eq!(report.coverage().analyzed_files(), 2);
    let view_digest = report.view_digest().to_string();
    assert_eq!(view_digest.len(), 64);
    assert!(
        view_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
    let debug = format!("{report:?}");
    assert!(!debug.contains(&workspace_text(0xA1)));
    assert!(!debug.contains(&slot_text(1)));
}

#[test]
fn malformed_manifest_fails_before_database_creation() {
    let directory = TempDirectory::new();
    let (contents, parent) = admit_manifest(directory.path(), "invalid.toml", "schema_version = [");
    let database = directory.join("must-not-exist.sqlite3");
    let configuration = default_configuration();

    let error = index_local_connected_workspace(
        request(&contents, &parent, &database, &configuration),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("malformed manifest should fail");

    assert_eq!(
        error,
        LocalConnectedWorkspaceIndexError::Manifest {
            kind: LocalConnectedWorkspaceManifestErrorKind::InvalidSyntaxOrSchema,
            source_ordinal: None,
        }
    );
    assert!(!database.exists());
}

#[test]
fn cancellation_and_deadline_fail_before_database_creation() {
    let directory = TempDirectory::new();
    fixture_repository(directory.path(), "repo");
    let text = manifest(0xA1, &[source_table(1, 1, "repo")]);
    let (contents, parent) = admit_manifest(directory.path(), "workspace.toml", &text);
    let configuration = default_configuration();

    let cancelled_database = directory.join("cancelled.sqlite3");
    let error = index_local_connected_workspace(
        request(&contents, &parent, &cancelled_database, &configuration),
        Arc::new(AtomicBool::new(true)),
    )
    .expect_err("pre-cancelled request should fail");
    assert_eq!(error, LocalConnectedWorkspaceIndexError::Cancelled);
    assert!(!cancelled_database.exists());

    let deadline_database = directory.join("deadline.sqlite3");
    let deadline_request = request(&contents, &parent, &deadline_database, &configuration)
        .with_deadline(Duration::MAX)
        .expect("positive duration should validate");
    let error = index_local_connected_workspace(deadline_request, Arc::new(AtomicBool::new(false)))
        .expect_err("unrepresentable deadline should fail");
    assert_eq!(
        error,
        LocalConnectedWorkspaceIndexError::DeadlineNotRepresentable
    );
    assert!(!deadline_database.exists());

    let error = request(&contents, &parent, &deadline_database, &configuration)
        .with_deadline(Duration::ZERO)
        .expect_err("zero deadline should fail request validation");
    assert_eq!(
        error,
        LocalConnectedWorkspaceIndexError::InvalidRequest {
            kind: LocalConnectedWorkspaceRequestErrorKind::Deadline,
        }
    );
}

#[test]
fn unknown_view_publication_preserves_reconciliation_category_and_guidance() {
    let error = LocalConnectedWorkspaceIndexError::from_internal(
        crate::local_index::connected_workspace::model::ConnectedWorkspaceIndexError::ViewPublication {
            source: crate::SqliteStoreError::MutationOutcomeUnknown,
        },
    );

    assert_eq!(
        error,
        LocalConnectedWorkspaceIndexError::MutationOutcomeUnknown {
            phase: LocalConnectedWorkspacePhase::ViewPublication,
            source_ordinal: None,
        }
    );
    assert_eq!(
        error.reconciliation_guidance(),
        Some("reopen the store and read the active immutable workspace view before retrying")
    );
    assert_eq!(
        error.to_string(),
        "connected-workspace mutation outcome could not be determined"
    );
}

#[test]
fn typed_request_and_error_output_redact_manifest_paths_and_selectors() {
    const CANARY: &str = "PRIVATE_CONNECTED_WORKSPACE_CANARY";

    let directory = TempDirectory::new();
    let private_parent = directory.join(CANARY);
    fs::create_dir(&private_parent).expect("private manifest parent should be created");
    let text = format!(
        "schema_version = 1\nconnected_workspace_id = {:?}\n\
         [[source]]\nsource_slot_id = {:?}\nrepository_identity = {:?}\n\
         worktree_root = {:?}\nselector = {{ kind = {:?} }}\n\
         scope = {{ kind = \"whole-repository\" }}\n",
        workspace_text(0xA1),
        slot_text(1),
        repository_text(1),
        format!("../{CANARY}/repository"),
        CANARY,
    );
    let (contents, parent) = admit_manifest(&private_parent, "workspace.toml", &text);
    let database = directory.join(&format!("{CANARY}.sqlite3"));
    let configuration = default_configuration();
    let facade_request = request(&contents, &parent, &database, &configuration);
    let request_debug = format!("{facade_request:?}");

    let error = index_local_connected_workspace(facade_request, Arc::new(AtomicBool::new(false)))
        .expect_err("invalid selector shape should fail");
    assert_eq!(
        error,
        LocalConnectedWorkspaceIndexError::Manifest {
            kind: LocalConnectedWorkspaceManifestErrorKind::Selector,
            source_ordinal: Some(1),
        }
    );
    let rendered = format!("{error} {error:?} {request_debug}");
    assert!(!rendered.contains(CANARY));
    assert!(!rendered.contains("workspace.toml"));
    assert!(!database.exists());
}

#[test]
fn frozen_membership_failure_preserves_the_prior_active_view() {
    let directory = TempDirectory::new();
    fixture_repository(directory.path(), "repo-a");
    fixture_repository(directory.path(), "repo-b");
    let configuration = default_configuration();
    let database = directory.join("index.sqlite3");

    let first_text = manifest(0xA1, &[source_table(1, 1, "repo-a")]);
    let (first_contents, first_parent) =
        admit_manifest(directory.path(), "first.toml", &first_text);
    index_local_connected_workspace(
        request(&first_contents, &first_parent, &database, &configuration),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("first view should publish");
    let prior_view = active_view_database_id(&database, 0xA1);

    let changed_text = manifest(0xA1, &[source_table(1, 2, "repo-b")]);
    let (changed_contents, changed_parent) =
        admit_manifest(directory.path(), "changed.toml", &changed_text);
    let error = index_local_connected_workspace(
        request(
            &changed_contents,
            &changed_parent,
            &database,
            &configuration,
        ),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("published membership must remain frozen");

    assert_eq!(
        error,
        LocalConnectedWorkspaceIndexError::Phase {
            phase: LocalConnectedWorkspacePhase::WorkspaceRegistration,
            source_ordinal: None,
        }
    );
    assert_eq!(active_view_database_id(&database, 0xA1), prior_view);
}

#[test]
fn equivalent_manifest_order_has_the_same_semantic_view_receipt() {
    let directory = TempDirectory::new();
    fixture_repository(directory.path(), "repo-a");
    fixture_repository(directory.path(), "repo-b");
    let configuration = default_configuration();
    let first_text = manifest(
        0xA1,
        &[source_table(1, 1, "repo-a"), source_table(2, 2, "repo-b")],
    );
    let second_text = manifest(
        0xA1,
        &[source_table(2, 2, "repo-b"), source_table(1, 1, "repo-a")],
    );
    let (first_contents, first_parent) =
        admit_manifest(directory.path(), "first.toml", &first_text);
    let (second_contents, second_parent) =
        admit_manifest(directory.path(), "second.toml", &second_text);
    let first_database = directory.join("first.sqlite3");
    let second_database = directory.join("second.sqlite3");

    let first = index_local_connected_workspace(
        request(
            &first_contents,
            &first_parent,
            &first_database,
            &configuration,
        ),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("first order should publish");
    let second = index_local_connected_workspace(
        request(
            &second_contents,
            &second_parent,
            &second_database,
            &configuration,
        ),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("second order should publish");

    assert_eq!(first.view_digest(), second.view_digest());
    assert_eq!(first.configuration_digest(), second.configuration_digest());
    assert_eq!(first.source_count(), second.source_count());
    assert_eq!(first.generation_count(), second.generation_count());
    assert_eq!(first.coverage(), second.coverage());
}

#[test]
fn facade_rejects_bytes_not_bound_to_the_admitted_manifest_before_database_access() {
    let directory = TempDirectory::new();
    let first_text = manifest(0xA1, &[source_table(1, 1, "missing-one")]);
    let second_text = manifest(0xA2, &[source_table(2, 2, "missing-two")]);
    let (first_contents, _first_parent) =
        admit_manifest(directory.path(), "first.toml", &first_text);
    let (_second_contents, second_parent) =
        admit_manifest(directory.path(), "second.toml", &second_text);
    let database = directory.join("must-not-exist.sqlite3");
    let configuration = default_configuration();

    let error = index_local_connected_workspace(
        request(&first_contents, &second_parent, &database, &configuration),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("a request must bind manifest bytes to their admitted file");

    assert_eq!(
        error,
        LocalConnectedWorkspaceIndexError::ManifestParent {
            kind: LocalConnectedWorkspaceParentErrorKind::Changed,
        }
    );
    assert!(!database.exists());
}

#[cfg(unix)]
#[test]
fn admitted_ancestor_replacement_fails_before_source_or_database_access() {
    let directory = TempDirectory::new();
    let admitted_ancestor = directory.join("admitted");
    let manifest_parent_path = admitted_ancestor.join("manifests");
    fs::create_dir_all(&manifest_parent_path).expect("manifest parent should be created");
    fixture_repository(&manifest_parent_path, "repo");
    let text = manifest(0xA1, &[source_table(1, 1, "repo")]);
    let (contents, parent) = admit_manifest(&manifest_parent_path, "workspace.toml", &text);
    let database = directory.join("must-not-exist.sqlite3");
    let configuration = default_configuration();
    let displaced = directory.join("displaced");

    let error = index_local_connected_workspace_with_hook(
        request(&contents, &parent, &database, &configuration),
        Arc::new(AtomicBool::new(false)),
        || {
            fs::rename(&admitted_ancestor, &displaced)
                .expect("admitted ancestor should be displaced");
            fs::create_dir_all(&manifest_parent_path)
                .expect("replacement ancestor chain should be created");
        },
    )
    .expect_err("ancestor replacement must fail closed");

    assert_eq!(
        error,
        LocalConnectedWorkspaceIndexError::ManifestParent {
            kind: LocalConnectedWorkspaceParentErrorKind::Changed,
        }
    );
    assert!(!database.exists());
}
