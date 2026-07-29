use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use repowitness_application::PackageScope;
use repowitness_domain::{SourceSlotId, WORKSPACE_ID_BYTES};

use super::{
    index_connected_workspace,
    model::{
        ConnectedSourceSlotRequest, ConnectedWorkspaceIndexError, ConnectedWorkspaceIndexRequest,
        ConnectedWorkspaceRequestError,
    },
};

mod faults;
mod fixtures;
mod process_recovery;
mod scopes;
mod state_contracts;

use fixtures::{
    TempDirectory, active_view, connected, default_configuration, fixture_repository, index,
    repository, request, scope, slot, source_slot,
};

#[test]
fn request_canonicalizes_slots_and_rejects_invalid_cardinality() {
    let configuration = default_configuration();
    let worktree = Path::new("/redacted/not-opened");
    let database = Path::new("/redacted/index.sqlite3");
    let second = slot(
        source_slot(2),
        repository(2),
        worktree,
        "worktree-head",
        PackageScope::whole_repository(),
        &configuration,
    );
    let first = slot(
        source_slot(1),
        repository(1),
        worktree,
        "worktree-head",
        PackageScope::whole_repository(),
        &configuration,
    );
    let canonical = request(connected(9), database, vec![second, first.clone()]);
    assert_eq!(canonical.source_slots()[0].source_slot(), source_slot(1));
    assert_eq!(canonical.source_slots()[1].source_slot(), source_slot(2));

    assert!(matches!(
        ConnectedWorkspaceIndexRequest::try_new(
            connected(9),
            database,
            0,
            Duration::from_secs(1),
            Vec::new()
        ),
        Err(ConnectedWorkspaceRequestError::EmptySourceSlots)
    ));
    assert!(matches!(
        ConnectedWorkspaceIndexRequest::try_new(
            connected(9),
            database,
            0,
            Duration::from_secs(1),
            vec![first.clone(), first]
        ),
        Err(ConnectedWorkspaceRequestError::DuplicateSourceSlot)
    ));
    let reserved = slot(
        SourceSlotId::for_repository(repository(1)),
        repository(1),
        worktree,
        "worktree-head",
        PackageScope::whole_repository(),
        &configuration,
    );
    assert!(matches!(
        ConnectedWorkspaceIndexRequest::try_new(
            connected(9),
            database,
            0,
            Duration::from_secs(1),
            vec![reserved]
        ),
        Err(ConnectedWorkspaceRequestError::ReservedCompatibilitySourceSlot)
    ));
    let compatibility_workspace = slot(
        source_slot(7),
        repository(7),
        worktree,
        "worktree-head",
        PackageScope::whole_repository(),
        &configuration,
    );
    assert!(matches!(
        ConnectedWorkspaceIndexRequest::try_new(
            connected(7),
            database,
            0,
            Duration::from_secs(1),
            vec![compatibility_workspace]
        ),
        Err(ConnectedWorkspaceRequestError::ReservedCompatibilityWorkspace)
    ));
}

#[test]
fn request_rejects_source_slot_overflow_before_other_validation() {
    let configuration = default_configuration();
    let worktree = Path::new("/redacted/not-opened");
    let database = Path::new("/redacted/index.sqlite3");
    let over_limit = (0..=crate::MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS)
        .map(|ordinal| {
            let mut bytes = [0_u8; WORKSPACE_ID_BYTES];
            bytes[..2].copy_from_slice(
                &u16::try_from(ordinal)
                    .expect("bounded test ordinal fits")
                    .to_be_bytes(),
            );
            slot(
                SourceSlotId::new(bytes),
                repository(1),
                worktree,
                "worktree-head",
                PackageScope::whole_repository(),
                &configuration,
            )
        })
        .collect();
    assert!(matches!(
        ConnectedWorkspaceIndexRequest::try_new(
            connected(9),
            database,
            0,
            Duration::from_secs(1),
            over_limit
        ),
        Err(ConnectedWorkspaceRequestError::SourceSlotLimitExceeded { limit: 256 })
    ));
}

#[test]
fn request_and_errors_redact_roots_selectors_and_database_paths() {
    let configuration = default_configuration();
    let root = Path::new("/sensitive/customer/repository");
    let database = Path::new("/sensitive/customer/index.sqlite3");
    let source = slot(
        source_slot(1),
        repository(1),
        root,
        "refs/tags/selected",
        scope(b"pkg_a"),
        &configuration,
    );
    let debug = format!("{:?}", request(connected(21), database, vec![source]));
    assert!(!debug.contains("sensitive"));
    assert!(!debug.contains("selected"));
    assert!(!debug.contains("pkg_a"));

    let selector_error = ConnectedSourceSlotRequest::try_new(
        source_slot(1),
        repository(1),
        root,
        "refs/not-allowed/private",
        PackageScope::whole_repository(),
        &configuration,
        crate::LocalRustIndexLimits::default(),
        crate::source_selector::SourceSelectorLimits::default(),
        Duration::from_secs(1),
    )
    .expect_err("unsupported selector must fail");
    assert!(!format!("{selector_error:?}").contains("private"));
    assert!(!selector_error.to_string().contains("private"));
}

#[test]
fn two_repositories_and_two_slots_for_one_logical_repository_publish_one_view() {
    let directory = TempDirectory::new();
    let first_repository = fixture_repository(&directory, "first");
    let second_repository = fixture_repository(&directory, "second");
    let database = directory.database();
    let configuration = default_configuration();
    let connected_workspace = connected(7);
    let report = index(request(
        connected_workspace,
        &database,
        vec![
            slot(
                source_slot(3),
                repository(2),
                &second_repository,
                "worktree-head",
                PackageScope::whole_repository(),
                &configuration,
            ),
            slot(
                source_slot(2),
                repository(1),
                &first_repository,
                "worktree-head",
                scope(b"pkg_b"),
                &configuration,
            ),
            slot(
                source_slot(1),
                repository(1),
                &first_repository,
                "worktree-head",
                scope(b"pkg_a"),
                &configuration,
            ),
        ],
    ));

    assert_eq!(report.source_slots().len(), 3);
    assert_eq!(report.source_slots()[0].source_slot(), source_slot(1));
    assert_eq!(report.source_slots()[1].source_slot(), source_slot(2));
    assert_eq!(report.source_slots()[2].source_slot(), source_slot(3));
    assert_eq!(report.source_slots()[0].indexed_files(), 1);
    assert_eq!(report.source_slots()[1].indexed_files(), 1);
    assert_eq!(report.source_slots()[2].indexed_files(), 2);
    assert_eq!(report.source_slots()[0].skipped_paths(), 2);
    // The explicit package scope excludes both the other package and the
    // root-level non-source path. They are policy omissions, not unsupported
    // in-scope language paths.
    assert_eq!(report.source_slots()[0].skipped_policy_paths(), 2);
    assert_eq!(report.source_slots()[0].skipped_unsupported_paths(), 0);
    assert_eq!(
        report.source_slots()[0].skipped_policy_paths()
            + report.source_slots()[0].skipped_unsupported_paths(),
        report.source_slots()[0].skipped_paths()
    );
    assert_eq!(report.source_slots()[0].discovered_paths(), 3);
    assert_eq!(report.configuration_digest(), configuration.digest());
    assert_eq!(active_view(&database, connected_workspace), report.view());
}

#[test]
fn pre_cancelled_request_fails_before_database_creation() {
    let directory = TempDirectory::new();
    let repository_path = fixture_repository(&directory, "cancelled");
    let database = directory.database();
    let configuration = default_configuration();
    let cancelled = Arc::new(AtomicBool::new(true));
    let error = index_connected_workspace(
        request(
            connected(3),
            &database,
            vec![slot(
                source_slot(1),
                repository(1),
                &repository_path,
                "worktree-head",
                PackageScope::whole_repository(),
                &configuration,
            )],
        ),
        cancelled,
    )
    .expect_err("pre-cancelled indexing must fail");

    assert!(matches!(error, ConnectedWorkspaceIndexError::Cancelled));
    assert!(!database.exists());
}
