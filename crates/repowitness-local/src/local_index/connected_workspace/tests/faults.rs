use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use repowitness_application::PackageScope;

use super::super::{
    CoordinatorPhase, index_connected_workspace, index_connected_workspace_with_hook,
    model::{
        ConnectedSourceSlotRequest, ConnectedWorkspaceIndexError, ConnectedWorkspaceIndexRequest,
    },
};
use super::fixtures::{
    TempDirectory, active_view, active_view_database_id_unchecked, connected,
    default_configuration, fixture_repository, git, index, repository, request, scope, slot,
    source_slot, write_source,
};

#[test]
fn cancellation_at_every_coordinator_phase_preserves_the_prior_active_view() {
    let directory = TempDirectory::new();
    let repository_path = fixture_repository(&directory, "fault-phases");
    let database = directory.database();
    let configuration = default_configuration();
    let connected_workspace = connected(5);
    let make_request = || {
        request(
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
        )
    };
    let initial = index(make_request());
    let initial_view = initial.view();

    for target in [
        CoordinatorPhase::MutationLeaseAcquired,
        CoordinatorPhase::WorkspaceRegistered,
        CoordinatorPhase::SelectorResolved,
        CoordinatorPhase::SourcePrepared,
        CoordinatorPhase::SourceStaged,
        CoordinatorPhase::GraphStaged,
        CoordinatorPhase::SourceCompleted,
        CoordinatorPhase::BeforeFinalFence,
        CoordinatorPhase::BeforeViewPublication,
    ] {
        let cancelled = Arc::new(AtomicBool::new(false));
        let hook_cancelled = Arc::clone(&cancelled);
        let error =
            index_connected_workspace_with_hook(make_request(), cancelled, move |phase, _| {
                if phase == target {
                    hook_cancelled.store(true, Ordering::Release);
                }
            })
            .expect_err("injected cancellation must fail before view publication");
        assert!(
            matches!(error, ConnectedWorkspaceIndexError::Cancelled),
            "unexpected failure at {target:?}: {error:?}"
        );
        assert_eq!(
            active_view(&database, connected_workspace),
            initial_view,
            "phase {target:?} must preserve the prior view"
        );
    }
}

#[test]
fn moving_ref_after_preparation_fails_the_final_fence_and_preserves_view() {
    let directory = TempDirectory::new();
    let repository_path = fixture_repository(&directory, "moving-ref");
    let database = directory.database();
    let configuration = default_configuration();
    let connected_workspace = connected(6);
    let make_request = || {
        request(
            connected_workspace,
            &database,
            vec![slot(
                source_slot(1),
                repository(1),
                &repository_path,
                "refs/tags/selected",
                scope(b"pkg_a"),
                &configuration,
            )],
        )
    };
    let initial = index(make_request());
    let error = index_connected_workspace_with_hook(
        make_request(),
        Arc::new(AtomicBool::new(false)),
        |phase, _| {
            if phase == CoordinatorPhase::GraphStaged {
                git(&repository_path, &["tag", "--force", "selected", "HEAD^"]);
            }
        },
    )
    .expect_err("a moving selector must fail closed");

    assert!(matches!(
        error,
        ConnectedWorkspaceIndexError::FinalSourceFence { .. }
    ));
    assert_eq!(active_view(&database, connected_workspace), initial.view());
}

#[test]
fn scoped_source_change_after_staging_fails_closed() {
    let directory = TempDirectory::new();
    let repository_path = fixture_repository(&directory, "source-change");
    let database = directory.database();
    let configuration = default_configuration();
    let connected_workspace = connected(8);
    let make_request = || {
        request(
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
        )
    };
    let initial = index(make_request());
    let error = index_connected_workspace_with_hook(
        make_request(),
        Arc::new(AtomicBool::new(false)),
        |phase, _| {
            if phase == CoordinatorPhase::GraphStaged {
                write_source(
                    &repository_path,
                    "pkg_a/src/lib.rs",
                    b"pub struct ChangedAfterStaging;\n",
                );
            }
        },
    )
    .expect_err("a post-staging source change must fail closed");

    assert!(matches!(
        error,
        ConnectedWorkspaceIndexError::FinalSourceFence { .. }
    ));
    assert_eq!(active_view(&database, connected_workspace), initial.view());
}

#[test]
fn immutable_slot_mapping_mismatch_preserves_the_prior_view() {
    let directory = TempDirectory::new();
    let first_repository = fixture_repository(&directory, "mapping-first");
    let second_repository = fixture_repository(&directory, "mapping-second");
    let database = directory.database();
    let configuration = default_configuration();
    let connected_workspace = connected(10);
    let initial = index(request(
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
    ));
    let error = index_connected_workspace(
        request(
            connected_workspace,
            &database,
            vec![slot(
                source_slot(1),
                repository(2),
                &second_repository,
                "worktree-head",
                PackageScope::whole_repository(),
                &configuration,
            )],
        ),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("immutable mapping mismatch must fail");

    assert!(matches!(
        error,
        ConnectedWorkspaceIndexError::WorkspaceRegistration { .. }
    ));
    assert_eq!(active_view(&database, connected_workspace), initial.view());
}

#[cfg(unix)]
#[test]
fn database_hard_link_alias_is_rejected_without_changing_the_view() {
    let directory = TempDirectory::new();
    let repository_path = fixture_repository(&directory, "database-alias");
    let database = directory.database();
    let configuration = default_configuration();
    let connected_workspace = connected(11);
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
    let alias = repository_path.join("database-alias.sqlite3");
    fs::hard_link(&database, &alias).expect("fixture hard link should be created");
    let error = index_connected_workspace(make_request(), Arc::new(AtomicBool::new(false)))
        .expect_err("database alias must fail closed");

    assert!(matches!(
        error,
        ConnectedWorkspaceIndexError::DatabaseIsolation { .. }
    ));
    assert_eq!(
        active_view_database_id_unchecked(&database, connected_workspace),
        initial.view().get()
    );
}

#[test]
fn positive_but_elapsed_whole_deadline_fails_without_publication() {
    let directory = TempDirectory::new();
    let repository_path = fixture_repository(&directory, "deadline");
    let database = directory.database();
    let configuration = default_configuration();
    let source = slot(
        source_slot(1),
        repository(1),
        &repository_path,
        "worktree-head",
        PackageScope::whole_repository(),
        &configuration,
    );
    let request = ConnectedWorkspaceIndexRequest::try_new(
        connected(12),
        &database,
        0,
        Duration::from_nanos(1),
        vec![source],
    )
    .expect("positive deadline should validate");
    let error = index_connected_workspace(request, Arc::new(AtomicBool::new(false)))
        .expect_err("elapsed deadline must fail");

    assert!(matches!(
        error,
        ConnectedWorkspaceIndexError::DeadlineExceeded
            | ConnectedWorkspaceIndexError::StoreStartup { .. }
    ));
    assert!(!database.exists());
}

#[test]
fn positive_per_slot_deadline_expires_without_expiring_whole_operation() {
    let directory = TempDirectory::new();
    let repository_path = fixture_repository(&directory, "slot-deadline");
    let database = directory.database();
    let configuration = default_configuration();
    let connected_workspace = connected(14);
    let initial = index(request(
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
    ));
    let source = ConnectedSourceSlotRequest::try_new(
        source_slot(1),
        repository(1),
        &repository_path,
        "worktree-head",
        PackageScope::whole_repository(),
        &configuration,
        crate::LocalRustIndexLimits::default(),
        crate::source_selector::SourceSelectorLimits::default(),
        Duration::from_millis(100),
    )
    .expect("positive per-slot deadline should validate");
    let request = ConnectedWorkspaceIndexRequest::try_new(
        connected_workspace,
        &database,
        0,
        Duration::from_secs(10),
        vec![source],
    )
    .expect("whole operation deadline should validate");
    let error = index_connected_workspace_with_hook(
        request,
        Arc::new(AtomicBool::new(false)),
        |phase, _| {
            if phase == CoordinatorPhase::WorkspaceRegistered {
                std::thread::sleep(Duration::from_millis(150));
            }
        },
    )
    .expect_err("expired per-slot deadline must fail");

    assert!(matches!(
        error,
        ConnectedWorkspaceIndexError::DeadlineExceeded
            | ConnectedWorkspaceIndexError::SelectorResolution { .. }
    ));
    assert_eq!(active_view(&database, connected_workspace), initial.view());
}
