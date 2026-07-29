use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_application::PackageScope;

use super::super::{
    CoordinatorPhase, index_connected_workspace_with_control_hooks,
    index_connected_workspace_with_parent_control_hooks, model::ConnectedWorkspaceIndexError,
};
use super::fixtures::{
    TempDirectory, active_view, connected, default_configuration, fixture_repository, git, index,
    repository, request, slot, source_slot, write_source,
};
use crate::{
    MAX_LOCAL_CONNECTED_WORKSPACE_MANIFEST_BYTES,
    local_index::post_commit::{PostCommitMaintenancePhase, PostCommitMaintenanceStatus},
    read_bounded_regular_file_with_parent,
};

#[cfg(any(unix, windows))]
#[test]
fn valid_database_replacement_after_writer_open_fails_before_view_publication() {
    let directory = TempDirectory::new();
    let repository_path = fixture_repository(&directory, "replacement");
    let database = directory.database();
    let configuration = default_configuration();
    let connected_workspace = connected(31);
    let make_request = || {
        request(
            connected_workspace,
            &database,
            vec![slot(
                source_slot(1),
                repository(1),
                &repository_path,
                "worktree-head",
                PackageScope::whole_repository(),
                &configuration,
            )],
        )
    };
    let initial = index(make_request());
    let replacement = directory.repository("replacement.sqlite3");
    fs::copy(&database, &replacement).expect("valid replacement database should be copied");
    let displaced = directory.repository("displaced.sqlite3");

    let error = index_connected_workspace_with_control_hooks(
        make_request(),
        Arc::new(AtomicBool::new(false)),
        |phase, _| {
            if phase == CoordinatorPhase::WorkspaceRegistered {
                fs::rename(&database, &displaced).expect("opened database should be displaced");
                fs::rename(&replacement, &database)
                    .expect("valid replacement should occupy the database path");
            }
        },
        |_, deadline| deadline,
    )
    .expect_err("writer-opened identity must reject a valid path replacement");

    assert!(matches!(
        error,
        ConnectedWorkspaceIndexError::DatabaseIsolation { .. }
    ));
    assert_eq!(active_view(&database, connected_workspace), initial.view());
}

#[cfg(any(unix, windows))]
#[test]
fn newly_created_database_replacement_is_not_adopted_after_registration() {
    let seed_directory = TempDirectory::new();
    let seed_repository = fixture_repository(&seed_directory, "seed");
    let seed_database = seed_directory.database();
    let seed_configuration = default_configuration();
    let _seed = index(request(
        connected(32),
        &seed_database,
        vec![slot(
            source_slot(1),
            repository(1),
            &seed_repository,
            "worktree-head",
            PackageScope::whole_repository(),
            &seed_configuration,
        )],
    ));

    let directory = TempDirectory::new();
    let repository_path = fixture_repository(&directory, "new-database-race");
    let database = directory.database();
    let replacement = directory.repository("replacement.sqlite3");
    fs::copy(&seed_database, &replacement).expect("valid replacement database should be copied");
    let displaced = directory.repository("displaced.sqlite3");
    let configuration = default_configuration();

    let error = index_connected_workspace_with_control_hooks(
        request(
            connected(33),
            &database,
            vec![slot(
                source_slot(1),
                repository(2),
                &repository_path,
                "worktree-head",
                PackageScope::whole_repository(),
                &configuration,
            )],
        ),
        Arc::new(AtomicBool::new(false)),
        |phase, _| {
            if phase == CoordinatorPhase::WorkspaceRegistered {
                fs::rename(&database, &displaced)
                    .expect("newly opened database should be displaced");
                fs::rename(&replacement, &database)
                    .expect("valid replacement should occupy the database path");
            }
        },
        |_, deadline| deadline,
    )
    .expect_err("new-file replacement must not become the writer identity");

    assert!(matches!(
        error,
        ConnectedWorkspaceIndexError::DatabaseIsolation { .. }
    ));
}

#[test]
fn failed_first_run_can_replace_membership_before_the_first_view() {
    let directory = TempDirectory::new();
    let first_repository = fixture_repository(&directory, "failed-membership");
    let corrected_repository = fixture_repository(&directory, "corrected-membership");
    let database = directory.database();
    let configuration = default_configuration();
    let connected_workspace = connected(34);
    let cancelled = Arc::new(AtomicBool::new(false));
    let hook_cancelled = Arc::clone(&cancelled);

    let error = index_connected_workspace_with_control_hooks(
        request(
            connected_workspace,
            &database,
            vec![slot(
                source_slot(1),
                repository(1),
                &first_repository,
                "worktree-head",
                PackageScope::whole_repository(),
                &configuration,
            )],
        ),
        cancelled,
        move |phase, _| {
            if phase == CoordinatorPhase::SourceCompleted {
                hook_cancelled.store(true, Ordering::Release);
            }
        },
        |_, deadline| deadline,
    )
    .expect_err("first run should stop after producing an unpublished receipt");
    assert!(matches!(error, ConnectedWorkspaceIndexError::Cancelled));

    let corrected = index(request(
        connected_workspace,
        &database,
        vec![slot(
            source_slot(2),
            repository(2),
            &corrected_repository,
            "worktree-head",
            PackageScope::whole_repository(),
            &configuration,
        )],
    ));
    assert_eq!(corrected.source_slots().len(), 1);
    assert_eq!(corrected.source_slots()[0].source_slot(), source_slot(2));
    assert_eq!(
        active_view(&database, connected_workspace),
        corrected.view()
    );
}

#[test]
fn post_commit_maintenance_failures_return_committed_reports() {
    let directory = TempDirectory::new();
    let repository_path = fixture_repository(&directory, "post-commit");
    let database = directory.database();
    let configuration = default_configuration();
    let connected_workspace = connected(35);
    let make_request = || {
        request(
            connected_workspace,
            &database,
            vec![slot(
                source_slot(1),
                repository(1),
                &repository_path,
                "worktree-head",
                PackageScope::whole_repository(),
                &configuration,
            )],
        )
    };
    let initial = index(make_request());

    for (fault, expected) in [
        (
            Some(PostCommitMaintenancePhase::Checkpoint),
            PostCommitMaintenanceStatus::CheckpointDeferred,
        ),
        (
            Some(PostCommitMaintenancePhase::Shutdown),
            PostCommitMaintenanceStatus::ShutdownDeferred,
        ),
        (None, PostCommitMaintenanceStatus::Complete),
    ] {
        let report = index_connected_workspace_with_control_hooks(
            make_request(),
            Arc::new(AtomicBool::new(false)),
            |_, _| {},
            |phase, deadline| {
                if Some(phase) == fault {
                    Instant::now()
                } else {
                    deadline
                }
            },
        )
        .expect("post-commit maintenance cannot turn publication into failure");
        assert_eq!(report.maintenance(), expected);
        assert_ne!(report.view(), initial.view());
        assert_eq!(active_view(&database, connected_workspace), report.view());
    }
}

#[test]
fn equivalent_source_order_and_database_local_ids_have_one_view_receipt() {
    let directory = TempDirectory::new();
    let first_repository = fixture_repository(&directory, "receipt-first");
    let second_repository = fixture_repository(&directory, "receipt-second");
    let first_database = directory.repository("first.sqlite3");
    let second_database = directory.repository("second.sqlite3");
    let configuration = default_configuration();
    let connected_workspace = connected(36);
    let first_slot = slot(
        source_slot(1),
        repository(1),
        &first_repository,
        "worktree-head",
        PackageScope::whole_repository(),
        &configuration,
    );
    let second_slot = slot(
        source_slot(2),
        repository(2),
        &second_repository,
        "worktree-head",
        PackageScope::whole_repository(),
        &configuration,
    );
    let canonical = index(request(
        connected_workspace,
        &first_database,
        vec![first_slot.clone(), second_slot.clone()],
    ));
    let reversed = index(request(
        connected_workspace,
        &second_database,
        vec![second_slot, first_slot],
    ));

    assert_eq!(
        canonical.view_receipt_digest(),
        reversed.view_receipt_digest()
    );
    assert_eq!(
        canonical.configuration_digest(),
        reversed.configuration_digest()
    );
}

#[cfg(unix)]
#[test]
fn admitted_parent_replacement_at_final_fence_preserves_the_prior_view() {
    let directory = TempDirectory::new();
    let manifest_parent = directory.repository("authority-parent");
    let repository_path = fixture_repository(&directory, "authority-parent/repo");
    let manifest_path = manifest_parent.join("workspace.toml");
    fs::write(&manifest_path, "admitted manifest bytes").expect("manifest should be written");
    let (_contents, parent_authority) = read_bounded_regular_file_with_parent(
        &manifest_path,
        MAX_LOCAL_CONNECTED_WORKSPACE_MANIFEST_BYTES,
    )
    .expect("manifest parent should be admitted");
    let database = directory.database();
    let configuration = default_configuration();
    let connected_workspace = connected(37);
    let make_request = || {
        request(
            connected_workspace,
            &database,
            vec![slot(
                source_slot(1),
                repository(1),
                &repository_path,
                "worktree-head",
                PackageScope::whole_repository(),
                &configuration,
            )],
        )
    };
    let initial = index_connected_workspace_with_parent_control_hooks(
        make_request(),
        Arc::new(AtomicBool::new(false)),
        Some(&parent_authority),
        |_, _| {},
        |_, deadline| deadline,
    )
    .expect("initial authority-bound view should publish");

    write_source(
        &repository_path,
        "pkg_a/src/lib.rs",
        b"pub struct PackageAChanged;\n",
    );
    git(
        &repository_path,
        &["commit", "--quiet", "-m", "change source"],
    );
    let displaced = directory.repository("authority-displaced");
    let error = index_connected_workspace_with_parent_control_hooks(
        make_request(),
        Arc::new(AtomicBool::new(false)),
        Some(&parent_authority),
        |phase, _| {
            if phase == CoordinatorPhase::BeforeViewPublication {
                fs::rename(&manifest_parent, &displaced)
                    .expect("manifest parent should be displaced");
            }
        },
        |_, deadline| deadline,
    )
    .expect_err("final authority fence must reject parent replacement");

    assert!(matches!(
        error,
        ConnectedWorkspaceIndexError::ManifestParentAuthority { .. }
    ));
    assert_eq!(active_view(&database, connected_workspace), initial.view());
}
