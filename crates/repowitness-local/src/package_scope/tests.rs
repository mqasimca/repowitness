use std::{
    cell::Cell,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

use repowitness_application::{PackageScope, PackageScopeError};
use repowitness_domain::{CoverageCompleteness, RepositoryPathLimits};

use super::{
    PackageScopeFilterError, filter_discovered_repository_paths, filter_paths_with_control,
};
use crate::{
    DiscoveredRepositoryPaths, GitPathDiscoveryLimits, GitPathDiscoveryStats,
    git_paths::parse_git_paths,
};

const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(1024, 32);

fn explicit<const N: usize>(roots: [&[u8]; N]) -> Result<PackageScope, PackageScopeError> {
    PackageScope::try_explicit_root_bytes(roots, PATH_LIMITS)
}

fn discovered(paths: &[&[u8]]) -> DiscoveredRepositoryPaths {
    let mut output = Vec::new();
    for path in paths {
        output.extend_from_slice(path);
        output.push(0);
    }
    parse_git_paths(output, GitPathDiscoveryLimits::default())
        .expect("fixture discovery paths should be valid")
}

#[test]
fn whole_repository_moves_legacy_paths_through_byte_for_byte() {
    let discovered = discovered(&[b"z.rs", b"a.go", b"pkg/\xff.py"]);
    let original_stats = discovered.stats();
    let original_bytes = discovered
        .paths()
        .iter()
        .map(|path| path.as_bytes().to_vec())
        .collect::<Vec<_>>();
    let first_allocation = discovered.paths()[0].as_bytes().as_ptr();
    let result = filter_discovered_repository_paths(
        discovered,
        &PackageScope::whole_repository(),
        &AtomicBool::new(false),
        Instant::now() + Duration::from_secs(1),
    )
    .expect("whole-repository scope should pass through");

    assert_eq!(result.discovery_stats(), original_stats);
    assert_eq!(result.stats().discovered_paths(), 3);
    assert_eq!(result.stats().selected_paths(), 3);
    assert_eq!(result.stats().policy_omitted_paths(), 0);
    assert_eq!(
        result.paths()[0].as_bytes().as_ptr(),
        first_allocation,
        "whole-repository filtering must not reconstruct path bytes"
    );
    assert_eq!(
        result
            .paths()
            .iter()
            .map(|path| path.as_bytes().to_vec())
            .collect::<Vec<_>>(),
        original_bytes
    );
    assert_eq!(
        result.coverage().completeness(),
        CoverageCompleteness::Complete
    );
}

#[test]
fn explicit_roots_use_exact_component_boundaries_and_report_omissions() {
    let discovered = discovered(&[
        b"a",
        b"a/lib.rs",
        b"a/nested/mod.rs",
        b"ab",
        b"ab/lib.rs",
        b"z.rs",
    ]);
    let scope = explicit([b"a".as_slice()]).expect("fixture scope should be valid");
    let result = filter_discovered_repository_paths(
        discovered,
        &scope,
        &AtomicBool::new(false),
        Instant::now() + Duration::from_secs(1),
    )
    .expect("filtering should succeed");

    assert_eq!(
        result
            .paths()
            .iter()
            .map(|path| path.as_bytes())
            .collect::<Vec<_>>(),
        vec![
            b"a".as_slice(),
            b"a/lib.rs".as_slice(),
            b"a/nested/mod.rs".as_slice()
        ]
    );
    assert_eq!(result.stats().discovered_paths(), 6);
    assert_eq!(result.stats().selected_paths(), 3);
    assert_eq!(result.stats().policy_omitted_paths(), 3);
    assert_eq!(result.coverage().searched().get(), 3);
    assert_eq!(result.coverage().skipped().get(), 3);
    assert_eq!(
        result.coverage().completeness(),
        CoverageCompleteness::Partial
    );
}

#[test]
fn exact_byte_membership_handles_option_like_and_non_utf8_components() {
    let discovered = discovered(&[
        b"--package/main.rs",
        b"--packages/main.rs",
        b"pkg/\xff/main.py",
        b"pkg/\xfe/main.py",
    ]);
    let scope = explicit([b"--package".as_slice(), b"pkg/\xff".as_slice()])
        .expect("fixture scope should be valid");
    let result = filter_discovered_repository_paths(
        discovered,
        &scope,
        &AtomicBool::new(false),
        Instant::now() + Duration::from_secs(1),
    )
    .expect("filtering should succeed");

    assert_eq!(
        result
            .paths()
            .iter()
            .map(|path| path.as_bytes())
            .collect::<Vec<_>>(),
        vec![
            b"--package/main.rs".as_slice(),
            b"pkg/\xff/main.py".as_slice()
        ]
    );
    assert_eq!(result.stats().policy_omitted_paths(), 2);
}

#[test]
fn filtering_observes_cancellation_during_the_scan() {
    let discovered = discovered(&[b"a/1.rs", b"a/2.rs", b"a/3.rs", b"z.rs"]);
    let stats = discovered.stats();
    let checks = Cell::new(0_u64);
    let deadline = Instant::now() + Duration::from_secs(1);

    assert_eq!(
        filter_paths_with_control(
            discovered.into_paths(),
            stats,
            &explicit([b"a".as_slice()]).expect("fixture scope should be valid"),
            deadline,
            || {
                let next = checks.get() + 1;
                checks.set(next);
                next >= 4
            },
            Instant::now,
        ),
        Err(PackageScopeFilterError::Cancelled)
    );
    assert_eq!(checks.get(), 4);
}

#[test]
fn filtering_fails_closed_at_or_after_deadline() {
    let discovered = discovered(&[b"a.rs"]);
    let stats = discovered.stats();
    let deadline = Instant::now();

    assert_eq!(
        filter_paths_with_control(
            discovered.into_paths(),
            stats,
            &PackageScope::whole_repository(),
            deadline,
            || false,
            || deadline,
        ),
        Err(PackageScopeFilterError::DeadlineExceeded)
    );
}

#[test]
fn pre_cancelled_whole_repository_does_not_return_paths() {
    assert_eq!(
        filter_discovered_repository_paths(
            discovered(&[b"private-path-canary.rs"]),
            &PackageScope::whole_repository(),
            &AtomicBool::new(true),
            Instant::now() + Duration::from_secs(1),
        ),
        Err(PackageScopeFilterError::Cancelled)
    );
}

#[test]
fn inconsistent_discovery_counts_fail_closed() {
    let discovered = discovered(&[b"a.rs", b"b.rs"]);
    let paths = discovered.into_paths();
    let deadline = Instant::now() + Duration::from_secs(1);

    assert_eq!(
        filter_paths_with_control(
            paths,
            GitPathDiscoveryStats::new(0, 1, 0, 0, 0),
            &PackageScope::whole_repository(),
            deadline,
            || false,
            Instant::now,
        ),
        Err(PackageScopeFilterError::DiscoveryPathCountMismatch {
            reported: 1,
            observed: 2,
        })
    );
}

#[test]
fn debug_and_errors_do_not_expose_repository_path_bytes() {
    let canary = "private-path-canary";
    let result = filter_discovered_repository_paths(
        discovered(&[canary.as_bytes()]),
        &PackageScope::whole_repository(),
        &AtomicBool::new(false),
        Instant::now() + Duration::from_secs(1),
    )
    .expect("fixture filtering should succeed");
    let error = filter_discovered_repository_paths(
        discovered(&[canary.as_bytes()]),
        &PackageScope::whole_repository(),
        &AtomicBool::new(true),
        Instant::now() + Duration::from_secs(1),
    )
    .expect_err("fixture filtering should be cancelled");

    for rendered in [
        format!("{result:?}"),
        format!("{error:?}"),
        error.to_string(),
    ] {
        assert!(!rendered.contains(canary));
    }
}
