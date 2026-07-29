use repowitness_domain::{RepositoryPathError, RepositoryPathLimits};

use super::{
    MAX_PACKAGE_SCOPE_ROOTS, PACKAGE_SCOPE_VERSION, PackageRootCount, PackageRootOrdinal,
    PackageScope, PackageScopeError,
};

const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(1024, 32);

fn explicit<const N: usize>(roots: [&[u8]; N]) -> Result<PackageScope, PackageScopeError> {
    PackageScope::try_explicit_root_bytes(roots, PATH_LIMITS)
}

#[test]
fn whole_repository_is_distinct_from_empty_or_empty_root() {
    let whole = PackageScope::whole_repository();

    assert!(whole.is_whole_repository());
    assert_eq!(whole.root_count(), PackageRootCount::new(0));
    assert_eq!(explicit([]), Err(PackageScopeError::EmptyExplicitRoots));
    assert_eq!(
        explicit([b"".as_slice()]),
        Err(PackageScopeError::InvalidRoot {
            ordinal: PackageRootOrdinal::new(1),
            source: RepositoryPathError::Empty,
        })
    );
}

#[test]
fn raw_roots_apply_repository_path_validation_without_host_parsing() {
    let cases = [
        (b"/absolute".as_slice(), RepositoryPathError::LeadingSlash),
        (
            b"pkg/../escape".as_slice(),
            RepositoryPathError::ParentDirectoryComponent,
        ),
        (
            b"pkg/./source".as_slice(),
            RepositoryPathError::CurrentDirectoryComponent,
        ),
        (
            b"pkg/.git/source".as_slice(),
            RepositoryPathError::DotGitComponent,
        ),
        (
            b"pkg//source".as_slice(),
            RepositoryPathError::EmptyComponent,
        ),
        (
            b"pkg/source/".as_slice(),
            RepositoryPathError::TrailingSlash,
        ),
    ];

    for (bytes, expected) in cases {
        assert_eq!(
            explicit([bytes]),
            Err(PackageScopeError::InvalidRoot {
                ordinal: PackageRootOrdinal::new(1),
                source: expected,
            })
        );
    }
}

#[test]
fn option_like_and_non_utf8_roots_are_preserved() {
    let scope = explicit([b"--all/src".as_slice(), b"pkg/\xff".as_slice()])
        .expect("repository bytes need no UTF-8 or option interpretation");
    let roots = scope.explicit_roots().expect("scope should be explicit");

    assert_eq!(roots[0].as_bytes(), b"--all/src");
    assert_eq!(roots[1].as_bytes(), b"pkg/\xff");
}

#[test]
fn exact_byte_order_is_canonical_and_case_sensitive() {
    let scope = explicit([b"zeta".as_slice(), b"Alpha".as_slice(), b"alpha".as_slice()])
        .expect("case-distinct roots should be accepted");
    let actual = scope
        .explicit_roots()
        .expect("scope should be explicit")
        .iter()
        .map(|root| root.as_bytes())
        .collect::<Vec<_>>();

    assert_eq!(
        actual,
        vec![b"Alpha".as_slice(), b"alpha".as_slice(), b"zeta".as_slice()]
    );
}

#[test]
fn duplicates_and_component_boundary_overlaps_are_rejected() {
    assert_eq!(
        explicit([b"a".as_slice(), b"a".as_slice()]),
        Err(PackageScopeError::DuplicateRoot)
    );
    for roots in [
        [b"a".as_slice(), b"a/b".as_slice()],
        [b"a/b".as_slice(), b"a".as_slice()],
    ] {
        assert_eq!(explicit(roots), Err(PackageScopeError::OverlappingRoots));
    }

    assert_eq!(
        explicit([b"a".as_slice(), b"a-b".as_slice(), b"a/b".as_slice()]),
        Err(PackageScopeError::OverlappingRoots),
        "a lexicographic interloper must not hide an overlap"
    );
    assert!(
        explicit([b"a".as_slice(), b"ab".as_slice()]).is_ok(),
        "raw prefix collisions are not component ancestry"
    );
}

#[test]
fn root_limit_is_inclusive_and_stops_at_the_first_excess_item() {
    let accepted = (0..MAX_PACKAGE_SCOPE_ROOTS.get())
        .map(|index| format!("root-{index:02}").into_bytes())
        .collect::<Vec<_>>();
    assert_eq!(
        PackageScope::try_explicit_root_bytes(&accepted, PATH_LIMITS)
            .expect("the inclusive root limit should be accepted")
            .root_count(),
        MAX_PACKAGE_SCOPE_ROOTS
    );

    let rejected = (0..=MAX_PACKAGE_SCOPE_ROOTS.get())
        .map(|index| format!("root-{index:02}").into_bytes())
        .collect::<Vec<_>>();
    assert_eq!(
        PackageScope::try_explicit_root_bytes(&rejected, PATH_LIMITS),
        Err(PackageScopeError::RootLimitExceeded {
            limit: MAX_PACKAGE_SCOPE_ROOTS,
        })
    );
}

#[test]
fn input_order_never_changes_scope_or_semantic_identity() {
    let permutations = [
        [b"a".as_slice(), b"m".as_slice(), b"z".as_slice()],
        [b"a".as_slice(), b"z".as_slice(), b"m".as_slice()],
        [b"m".as_slice(), b"a".as_slice(), b"z".as_slice()],
        [b"m".as_slice(), b"z".as_slice(), b"a".as_slice()],
        [b"z".as_slice(), b"a".as_slice(), b"m".as_slice()],
        [b"z".as_slice(), b"m".as_slice(), b"a".as_slice()],
    ];
    let expected = explicit(permutations[0]).expect("fixture should be valid");

    for permutation in permutations {
        let actual = explicit(permutation).expect("fixture should be valid");
        assert_eq!(actual, expected);
        assert_eq!(actual.semantic_digest(), expected.semantic_digest());
    }
}

#[test]
fn semantic_identity_is_versioned_and_scope_sensitive() {
    let whole = PackageScope::whole_repository();
    let first = explicit([b"pkg".as_slice()]).expect("fixture should be valid");
    let second = explicit([b"pkg-two".as_slice()]).expect("fixture should be valid");

    assert_eq!(PACKAGE_SCOPE_VERSION, 1);
    assert_ne!(whole.semantic_digest(), first.semantic_digest());
    assert_ne!(first.semantic_digest(), second.semantic_digest());
    assert_eq!(
        whole.semantic_digest().into_bytes(),
        [
            0x88, 0x6c, 0x5c, 0x5f, 0x5d, 0x68, 0xc5, 0xad, 0x1f, 0x7e, 0xcd, 0xe5, 0x83, 0x0b,
            0x1d, 0xcc, 0x38, 0xf8, 0x75, 0x0d, 0x1c, 0x73, 0x0d, 0xd4, 0x9d, 0x9c, 0x6e, 0x69,
            0x1e, 0x96, 0xa4, 0xe6,
        ]
    );
}

#[test]
fn debug_and_errors_do_not_expose_root_bytes() {
    let canary = b"private-root-canary";
    let scope = explicit([canary.as_slice()]).expect("fixture should be valid");
    let invalid = PackageScope::try_explicit_root_bytes(
        [b"private-root-canary/../escape".as_slice()],
        PATH_LIMITS,
    )
    .expect_err("fixture should be invalid");

    for rendered in [
        format!("{scope:?}"),
        format!("{:?}", scope.semantic_digest()),
        format!("{invalid:?}"),
        invalid.to_string(),
    ] {
        assert!(!rendered.contains("private-root-canary"));
    }
}
