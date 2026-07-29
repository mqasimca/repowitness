use repowitness_application::{
    MAX_PACKAGE_SCOPE_ROOTS, PackageRootCount, PackageScopeError, RepositoryIdentityTextError,
    RepositoryPathTextError, WorkspaceIdentityTextError,
};

use super::{
    ConnectedWorkspaceManifestError, ConnectedWorkspaceManifestSourceError,
    MAX_CONNECTED_WORKSPACE_MANIFEST_BYTES, MAX_CONNECTED_WORKSPACE_MANIFEST_SOURCES,
    MAX_CONNECTED_WORKSPACE_WORKTREE_ROOT_BYTES, assert_invalid_source, manifest, parse, path_text,
    repository_text, slot_text, source_table, whole_source, workspace_text,
};

#[test]
fn source_count_bounds_accept_one_and_256_but_reject_zero_and_257() {
    assert!(parse(&manifest(&[whole_source(1, 1, "one")])).is_ok());

    let maximum = (1..=MAX_CONNECTED_WORKSPACE_MANIFEST_SOURCES)
        .map(|index| whole_source(u16::try_from(index).expect("fixture index fits"), 1, "root"))
        .collect::<Vec<_>>();
    let parsed = parse(&manifest(&maximum)).expect("256 sources should be accepted");
    assert_eq!(parsed.sources().len(), 256);

    let zero = format!(
        "schema_version = 1\nconnected_workspace_id = {:?}\nsource = []\n",
        workspace_text(1)
    );
    assert_eq!(
        parse(&zero),
        Err(ConnectedWorkspaceManifestError::SourceCountOutOfRange {
            minimum: 1,
            maximum: 256,
        })
    );

    let over = (1..=MAX_CONNECTED_WORKSPACE_MANIFEST_SOURCES + 1)
        .map(|index| whole_source(u16::try_from(index).expect("fixture index fits"), 1, "root"))
        .collect::<Vec<_>>();
    assert_eq!(
        parse(&manifest(&over)),
        Err(ConnectedWorkspaceManifestError::SourceCountOutOfRange {
            minimum: 1,
            maximum: 256,
        })
    );
}

#[test]
fn manifest_byte_limit_is_inclusive_and_checked_before_parsing() {
    let mut exact = manifest(&[whole_source(1, 1, "root")]).into_bytes();
    exact.extend_from_slice(b"\n#");
    exact.resize(MAX_CONNECTED_WORKSPACE_MANIFEST_BYTES, b'x');
    assert_eq!(exact.len(), MAX_CONNECTED_WORKSPACE_MANIFEST_BYTES);
    super::parse_connected_workspace_manifest(&exact, std::path::Path::new(super::TEST_PARENT))
        .expect("exact byte limit should parse");

    exact.push(b'x');
    assert_eq!(
        super::parse_connected_workspace_manifest(&exact, std::path::Path::new(super::TEST_PARENT)),
        Err(ConnectedWorkspaceManifestError::InputTooLarge {
            limit: u64::try_from(MAX_CONNECTED_WORKSPACE_MANIFEST_BYTES)
                .expect("fixture limit fits"),
        })
    );
}

#[test]
fn invalid_utf8_and_malformed_toml_are_rejected_without_fallback() {
    assert_eq!(
        super::parse_connected_workspace_manifest(
            &[0xff],
            std::path::Path::new(super::TEST_PARENT)
        ),
        Err(ConnectedWorkspaceManifestError::InvalidUtf8)
    );
    assert_eq!(
        parse("schema_version = ["),
        Err(ConnectedWorkspaceManifestError::InvalidToml)
    );
}

#[test]
fn worktree_root_byte_limit_is_inclusive_and_empty_is_rejected() {
    let exact = "r".repeat(MAX_CONNECTED_WORKSPACE_WORKTREE_ROOT_BYTES);
    let parsed =
        parse(&manifest(&[whole_source(1, 1, &exact)])).expect("4096-byte root should parse");
    assert!(
        parsed.sources()[0]
            .worktree_root()
            .ends_with(exact.as_str())
    );

    assert_invalid_source(
        parse(&manifest(&[whole_source(1, 1, "")])),
        ConnectedWorkspaceManifestSourceError::WorktreeRoot,
    );
    let over = "r".repeat(MAX_CONNECTED_WORKSPACE_WORKTREE_ROOT_BYTES + 1);
    assert_invalid_source(
        parse(&manifest(&[whole_source(1, 1, &over)])),
        ConnectedWorkspaceManifestSourceError::WorktreeRoot,
    );
}

#[test]
fn workspace_slot_and_repository_text_must_be_canonical() {
    let bad_workspace = format!(
        "schema_version = 1\nconnected_workspace_id = {:?}\n{}",
        workspace_text(0xAB).to_ascii_lowercase(),
        whole_source(1, 1, "root")
    );
    assert!(matches!(
        parse(&bad_workspace),
        Err(
            ConnectedWorkspaceManifestError::InvalidConnectedWorkspaceId {
                source: WorkspaceIdentityTextError::InvalidPrefix
                    | WorkspaceIdentityTextError::InvalidBase16,
            }
        )
    ));

    let bad_slot = source_table(
        &slot_text(0xABCD).to_ascii_lowercase(),
        &repository_text(1),
        "root",
        "kind = \"worktree-head\"",
        "kind = \"whole-repository\"",
    );
    assert!(matches!(
        parse(&manifest(&[bad_slot])),
        Err(ConnectedWorkspaceManifestError::InvalidSource {
            ordinal: 1,
            source: ConnectedWorkspaceManifestSourceError::SourceSlotId {
                source: WorkspaceIdentityTextError::InvalidPrefix
                    | WorkspaceIdentityTextError::InvalidBase16,
            },
        })
    ));

    let bad_repository = source_table(
        &slot_text(1),
        &repository_text(0xAB).to_ascii_lowercase(),
        "root",
        "kind = \"worktree-head\"",
        "kind = \"whole-repository\"",
    );
    assert!(matches!(
        parse(&manifest(&[bad_repository])),
        Err(ConnectedWorkspaceManifestError::InvalidSource {
            ordinal: 1,
            source: ConnectedWorkspaceManifestSourceError::RepositoryIdentity {
                source: RepositoryIdentityTextError::InvalidPrefix
                    | RepositoryIdentityTextError::InvalidBase16,
            },
        })
    ));
}

#[test]
fn package_root_codec_preserves_non_utf8_and_rejects_alternate_text() {
    let encoded = path_text(b"pkg/\xff");
    let source = source_table(
        &slot_text(1),
        &repository_text(1),
        "root",
        "kind = \"worktree-head\"",
        &format!("kind = \"explicit-roots\", roots = [{encoded:?}]"),
    );
    let parsed = parse(&manifest(&[source])).expect("non-UTF8 root should decode");
    assert_eq!(
        parsed.sources()[0]
            .package_scope()
            .explicit_roots()
            .expect("scope should be explicit")[0]
            .as_bytes(),
        b"pkg/\xff"
    );

    for invalid in ["rwp1:h:A", "rwp1:h:aa", "rp1:h:AA"] {
        let source = source_table(
            &slot_text(1),
            &repository_text(1),
            "root",
            "kind = \"worktree-head\"",
            &format!("kind = \"explicit-roots\", roots = [{invalid:?}]"),
        );
        assert!(matches!(
            parse(&manifest(&[source])),
            Err(ConnectedWorkspaceManifestError::InvalidSource {
                ordinal: 1,
                source: ConnectedWorkspaceManifestSourceError::PackageRoot {
                    ordinal: 1,
                    source: RepositoryPathTextError::OddPayloadLength
                        | RepositoryPathTextError::NonCanonicalBase16
                        | RepositoryPathTextError::InvalidTag,
                },
            })
        ));
    }
}

#[test]
fn package_scope_enforces_root_count_and_overlap() {
    let roots = (0..MAX_PACKAGE_SCOPE_ROOTS.get())
        .map(|index| path_text(format!("root-{index}").as_bytes()))
        .collect::<Vec<_>>();
    let rendered = roots
        .iter()
        .map(|root| format!("{root:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let accepted = source_table(
        &slot_text(1),
        &repository_text(1),
        "root",
        "kind = \"worktree-head\"",
        &format!("kind = \"explicit-roots\", roots = [{rendered}]"),
    );
    assert!(parse(&manifest(&[accepted])).is_ok());

    let over = (0..=MAX_PACKAGE_SCOPE_ROOTS.get())
        .map(|index| format!("{:?}", path_text(format!("over-{index}").as_bytes())))
        .collect::<Vec<_>>()
        .join(", ");
    let source = source_table(
        &slot_text(1),
        &repository_text(1),
        "root",
        "kind = \"worktree-head\"",
        &format!("kind = \"explicit-roots\", roots = [{over}]"),
    );
    assert_invalid_source(
        parse(&manifest(&[source])),
        ConnectedWorkspaceManifestSourceError::PackageScope {
            source: PackageScopeError::RootLimitExceeded {
                limit: PackageRootCount::new(64),
            },
        },
    );

    let overlap = [path_text(b"a"), path_text(b"a/b")]
        .iter()
        .map(|root| format!("{root:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = source_table(
        &slot_text(1),
        &repository_text(1),
        "root",
        "kind = \"worktree-head\"",
        &format!("kind = \"explicit-roots\", roots = [{overlap}]"),
    );
    assert_invalid_source(
        parse(&manifest(&[source])),
        ConnectedWorkspaceManifestSourceError::PackageScope {
            source: PackageScopeError::OverlappingRoots,
        },
    );
}
