use repowitness_application::{PackageScope, resolve_configuration};
use repowitness_domain::RepositoryIdentityDigest;

use crate::source_selector::SourceSelectorCategory;

use super::{
    expected_relative_root, manifest, parse, path_text, repository_text, slot_text, source_table,
    whole_source, workspace_text,
};

#[test]
fn documented_nested_table_spelling_is_accepted() {
    let text = format!(
        "schema_version = 1\n\
         connected_workspace_id = {:?}\n\
         [[source]]\n\
         source_slot_id = {:?}\n\
         repository_identity = {:?}\n\
         worktree_root = \"nested\"\n\
         [source.selector]\n\
         kind = \"worktree-head\"\n\
         [source.scope]\n\
         kind = \"whole-repository\"\n",
        workspace_text(0xA1),
        slot_text(1),
        repository_text(1),
    );

    let parsed = parse(&text).expect("documented nested tables should parse");
    assert_eq!(parsed.sources().len(), 1);
    assert_eq!(
        parsed.sources()[0].worktree_root(),
        expected_relative_root("nested")
    );
}

#[test]
fn golden_manifest_decodes_all_values_and_canonicalizes_slots() {
    let first_scope_root = path_text(b"pkg/\xff");
    let sources = [
        source_table(
            &slot_text(2),
            &repository_text(0x22),
            "../second",
            "kind = \"exact-revision\", value = \"1111111111111111111111111111111111111111\"",
            "kind = \"whole-repository\"",
        ),
        source_table(
            &slot_text(1),
            &repository_text(0x11),
            "--option-shaped-root",
            "kind = \"full-ref\", value = \"refs/heads/main\"",
            &format!("kind = \"explicit-roots\", roots = [{first_scope_root:?}]"),
        ),
    ];
    let parsed = parse(&manifest(&sources)).expect("golden manifest should parse");

    assert_eq!(
        parsed.connected_workspace(),
        repowitness_application::ConnectedWorkspaceIdTextV1::decode(&workspace_text(0xA1))
            .expect("fixture workspace should decode")
    );
    assert_eq!(parsed.sources().len(), 2);
    assert_eq!(
        parsed.sources()[0].source_slot(),
        repowitness_application::SourceSlotIdTextV1::decode(&slot_text(1))
            .expect("fixture slot should decode")
    );
    assert_eq!(
        parsed.sources()[1].source_slot(),
        repowitness_application::SourceSlotIdTextV1::decode(&slot_text(2))
            .expect("fixture slot should decode")
    );
    assert_eq!(
        parsed.sources()[0].repository(),
        RepositoryIdentityDigest::new([0x11; 32])
    );
    assert_eq!(
        parsed.sources()[0].worktree_root(),
        expected_relative_root("--option-shaped-root")
    );
    assert_eq!(
        parsed.sources()[1].worktree_root(),
        expected_relative_root("../second")
    );
    assert_eq!(
        parsed.sources()[0].selector().category(),
        SourceSelectorCategory::FullRef
    );
    assert_eq!(
        parsed.sources()[1].selector().category(),
        SourceSelectorCategory::ExactRevision
    );
    let roots = parsed.sources()[0]
        .package_scope()
        .explicit_roots()
        .expect("first scope should contain explicit roots");
    assert_eq!(roots[0].as_bytes(), b"pkg/\xff");
    assert!(parsed.sources()[1].package_scope().is_whole_repository());
}

#[test]
fn reordered_sources_are_semantically_identical() {
    let first = whole_source(1, 0x44, "one");
    let second = whole_source(2, 0x55, "two");

    let forward =
        parse(&manifest(&[first.clone(), second.clone()])).expect("forward manifest should parse");
    let reverse = parse(&manifest(&[second, first])).expect("reverse manifest should parse");

    assert_eq!(forward, reverse);
}

#[test]
fn repeated_repository_and_worktree_are_preserved_in_distinct_slots() {
    let first = whole_source(1, 0x44, "shared");
    let second = whole_source(2, 0x44, "shared");
    let parsed = parse(&manifest(&[first, second])).expect("repeated repository should be valid");

    assert_eq!(
        parsed.sources()[0].repository(),
        parsed.sources()[1].repository()
    );
    assert_eq!(
        parsed.sources()[0].worktree_root(),
        parsed.sources()[1].worktree_root()
    );
    assert_ne!(
        parsed.sources()[0].source_slot(),
        parsed.sources()[1].source_slot()
    );
}

#[test]
fn all_selector_variants_are_admitted_only_as_their_structured_kind() {
    let sources = [
        source_table(
            &slot_text(1),
            &repository_text(1),
            "head",
            "kind = \"worktree-head\"",
            "kind = \"whole-repository\"",
        ),
        source_table(
            &slot_text(2),
            &repository_text(2),
            "sha1",
            "kind = \"exact-revision\", value = \"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"",
            "kind = \"whole-repository\"",
        ),
        source_table(
            &slot_text(3),
            &repository_text(3),
            "sha256",
            "kind = \"exact-revision\", value = \"BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB\"",
            "kind = \"whole-repository\"",
        ),
        source_table(
            &slot_text(4),
            &repository_text(4),
            "ref",
            "kind = \"full-ref\", value = \"refs/remotes/origin/main\"",
            "kind = \"whole-repository\"",
        ),
    ];
    let parsed = parse(&manifest(&sources)).expect("all selector variants should parse");
    let categories = parsed
        .sources()
        .iter()
        .map(|source| source.selector().category())
        .collect::<Vec<_>>();

    assert_eq!(
        categories,
        vec![
            SourceSelectorCategory::WorktreeHead,
            SourceSelectorCategory::ExactRevision,
            SourceSelectorCategory::ExactRevision,
            SourceSelectorCategory::FullRef,
        ]
    );
}

#[test]
fn configuration_is_attached_once_after_parsing_and_stays_redacted() {
    let parsed =
        parse(&manifest(&[whole_source(1, 1, "private-root-canary")])).expect("valid manifest");
    let configuration = resolve_configuration(&[]).expect("default configuration should resolve");
    let expected_digest = configuration.digest();
    let configured = parsed.with_configuration(configuration);

    assert_eq!(configured.configuration().digest(), expected_digest);
    assert_eq!(configured.manifest().sources().len(), 1);
    let rendered = format!("{configured:?}");
    assert!(!rendered.contains("private-root-canary"));
    let (manifest, configuration) = configured.into_parts();
    assert_eq!(manifest.sources().len(), 1);
    assert_eq!(configuration.digest(), expected_digest);
}

#[test]
fn model_debug_redacts_host_roots_selectors_and_package_roots() {
    let workspace = workspace_text(0xA1);
    let slot = slot_text(1);
    let repository = repository_text(1);
    let root = path_text(b"private-package-canary");
    let source = source_table(
        &slot,
        &repository,
        "private-worktree-canary",
        "kind = \"full-ref\", value = \"refs/heads/private-selector-canary\"",
        &format!("kind = \"explicit-roots\", roots = [{root:?}]"),
    );
    let parsed = parse(&manifest(&[source])).expect("privacy fixture should parse");

    for rendered in [
        format!("{parsed:?}"),
        format!("{:?}", parsed.sources()[0]),
        format!("{:?}", PackageScope::whole_repository()),
    ] {
        assert!(!rendered.contains("private-worktree-canary"));
        assert!(!rendered.contains("private-selector-canary"));
        assert!(!rendered.contains("private-package-canary"));
        assert!(!rendered.contains(&workspace));
        assert!(!rendered.contains(&slot));
        assert!(!rendered.contains(&repository));
    }
}

#[cfg(unix)]
#[test]
fn absolute_roots_remain_absolute_without_parent_joining() {
    let parsed = parse(&manifest(&[whole_source(
        1,
        1,
        "/explicit/authorized/root",
    )]))
    .expect("absolute root should parse");

    assert_eq!(
        parsed.sources()[0].worktree_root(),
        std::path::Path::new("/explicit/authorized/root")
    );
}
