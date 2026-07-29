use repowitness_application::PackageScopeError;

use crate::source_selector::SourceSelectorAdmissionError;

use super::{
    ConnectedWorkspaceManifestError, ConnectedWorkspaceManifestSourceError, assert_invalid_source,
    manifest, parse, path_text, repository_text, slot_text, source_table, whole_source,
    workspace_text,
};

#[test]
fn schema_version_is_required_exactly_and_type_confusion_fails_closed() {
    let valid_source = whole_source(1, 1, "root");
    for version in ["0", "2", "18446744073709551615"] {
        let text = format!(
            "schema_version = {version}\nconnected_workspace_id = {:?}\n{valid_source}",
            workspace_text(1)
        );
        assert_eq!(
            parse(&text),
            Err(ConnectedWorkspaceManifestError::UnsupportedSchemaVersion)
        );
    }
    for schema in [
        "schema_version = \"1\"",
        "schema_version = 1.0",
        "schema_version = []",
    ] {
        let text = format!(
            "{schema}\nconnected_workspace_id = {:?}\n{valid_source}",
            workspace_text(1)
        );
        assert_eq!(
            parse(&text),
            Err(ConnectedWorkspaceManifestError::InvalidToml)
        );
    }
}

#[test]
fn duplicate_and_unknown_keys_fail_at_every_structural_level() {
    let workspace = workspace_text(1);
    let slot = slot_text(1);
    let repository = repository_text(1);
    let cases = [
        format!(
            "schema_version = 1\nschema_version = 1\nconnected_workspace_id = {workspace:?}\n{}",
            whole_source(1, 1, "root")
        ),
        format!(
            "schema_version = 1\nconnected_workspace_id = {workspace:?}\nunknown = 1\n{}",
            whole_source(1, 1, "root")
        ),
        format!(
            "schema_version = 1\nconnected_workspace_id = {workspace:?}\n[[source]]\nsource_slot_id = {slot:?}\nsource_slot_id = {slot:?}\nrepository_identity = {repository:?}\nworktree_root = \"root\"\nselector = {{ kind = \"worktree-head\" }}\nscope = {{ kind = \"whole-repository\" }}\n"
        ),
        format!(
            "schema_version = 1\nconnected_workspace_id = {workspace:?}\n[[source]]\nsource_slot_id = {slot:?}\nrepository_identity = {repository:?}\nworktree_root = \"root\"\nunknown = 1\nselector = {{ kind = \"worktree-head\" }}\nscope = {{ kind = \"whole-repository\" }}\n"
        ),
        format!(
            "schema_version = 1\nconnected_workspace_id = {workspace:?}\n[[source]]\nsource_slot_id = {slot:?}\nrepository_identity = {repository:?}\nworktree_root = \"root\"\nselector = {{ kind = \"worktree-head\", unknown = true }}\nscope = {{ kind = \"whole-repository\" }}\n"
        ),
        format!(
            "schema_version = 1\nconnected_workspace_id = {workspace:?}\n[[source]]\nsource_slot_id = {slot:?}\nrepository_identity = {repository:?}\nworktree_root = \"root\"\nselector = {{ kind = \"worktree-head\" }}\nscope = {{ kind = \"whole-repository\", unknown = true }}\n"
        ),
    ];

    for text in cases {
        assert_eq!(
            parse(&text),
            Err(ConnectedWorkspaceManifestError::InvalidToml)
        );
    }
}

#[test]
fn source_selector_and_scope_must_be_structured_tables() {
    let workspace = workspace_text(1);
    let slot = slot_text(1);
    let repository = repository_text(1);
    for fields in [
        "selector = \"worktree-head\"\nscope = { kind = \"whole-repository\" }",
        "selector = { kind = \"worktree-head\" }\nscope = \"whole-repository\"",
        "selector = []\nscope = { kind = \"whole-repository\" }",
        "selector = { kind = \"worktree-head\" }\nscope = []",
    ] {
        let text = format!(
            "schema_version = 1\nconnected_workspace_id = {workspace:?}\n[[source]]\nsource_slot_id = {slot:?}\nrepository_identity = {repository:?}\nworktree_root = \"root\"\n{fields}\n"
        );
        assert_eq!(
            parse(&text),
            Err(ConnectedWorkspaceManifestError::InvalidToml)
        );
    }
}

#[test]
fn missing_required_top_level_and_source_fields_fail_closed() {
    let workspace = workspace_text(1);
    let slot = slot_text(1);
    let repository = repository_text(1);
    let cases = [
        format!("schema_version = 1\n{}", whole_source(1, 1, "root")),
        format!(
            "schema_version = 1\nconnected_workspace_id = {workspace:?}\n\
             [[source]]\nrepository_identity = {repository:?}\nworktree_root = \"root\"\n\
             selector = {{ kind = \"worktree-head\" }}\n\
             scope = {{ kind = \"whole-repository\" }}\n"
        ),
        format!(
            "schema_version = 1\nconnected_workspace_id = {workspace:?}\n\
             [[source]]\nsource_slot_id = {slot:?}\nrepository_identity = {repository:?}\n\
             selector = {{ kind = \"worktree-head\" }}\n\
             scope = {{ kind = \"whole-repository\" }}\n"
        ),
        format!(
            "schema_version = 1\nconnected_workspace_id = {workspace:?}\n\
             [[source]]\nsource_slot_id = {slot:?}\nrepository_identity = {repository:?}\n\
             worktree_root = \"root\"\nscope = {{ kind = \"whole-repository\" }}\n"
        ),
    ];

    for text in cases {
        assert_eq!(
            parse(&text),
            Err(ConnectedWorkspaceManifestError::InvalidToml)
        );
    }
}

#[test]
fn selector_kind_enforces_required_and_forbidden_value() {
    for selector in [
        "kind = \"worktree-head\", value = \"refs/heads/main\"",
        "kind = \"exact-revision\"",
        "kind = \"full-ref\"",
        "kind = \"unknown\"",
        "kind = \"exact-revision\", value = \"refs/heads/main\"",
        "kind = \"full-ref\", value = \"1111111111111111111111111111111111111111\"",
    ] {
        let source = source_table(
            &slot_text(1),
            &repository_text(1),
            "root",
            selector,
            "kind = \"whole-repository\"",
        );
        assert_invalid_source(
            parse(&manifest(&[source])),
            ConnectedWorkspaceManifestSourceError::SelectorShape,
        );
    }

    let invalid_value = source_table(
        &slot_text(1),
        &repository_text(1),
        "root",
        "kind = \"exact-revision\", value = \"private-selector-canary\"",
        "kind = \"whole-repository\"",
    );
    assert_invalid_source(
        parse(&manifest(&[invalid_value])),
        ConnectedWorkspaceManifestSourceError::Selector {
            source: SourceSelectorAdmissionError::UnsupportedCategory,
        },
    );
}

#[test]
fn scope_kind_enforces_required_and_forbidden_roots() {
    for scope in [
        "kind = \"whole-repository\", roots = []",
        "kind = \"explicit-roots\"",
        "kind = \"unknown\"",
    ] {
        let source = source_table(
            &slot_text(1),
            &repository_text(1),
            "root",
            "kind = \"worktree-head\"",
            scope,
        );
        assert_invalid_source(
            parse(&manifest(&[source])),
            ConnectedWorkspaceManifestSourceError::ScopeShape,
        );
    }

    let empty = source_table(
        &slot_text(1),
        &repository_text(1),
        "root",
        "kind = \"worktree-head\"",
        "kind = \"explicit-roots\", roots = []",
    );
    assert_invalid_source(
        parse(&manifest(&[empty])),
        ConnectedWorkspaceManifestSourceError::PackageScope {
            source: PackageScopeError::EmptyExplicitRoots,
        },
    );
}

#[test]
fn duplicate_slots_are_rejected_after_order_canonicalization() {
    let first = whole_source(1, 1, "one");
    let second = whole_source(1, 2, "two");

    assert_eq!(
        parse(&manifest(&[first, second])),
        Err(ConnectedWorkspaceManifestError::DuplicateSourceSlot)
    );
}

#[test]
fn deeply_nested_or_unknown_toml_fails_without_panicking() {
    let depth = 512;
    let nested = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
    let text = format!(
        "schema_version = 1\nconnected_workspace_id = {:?}\ndeep = {nested}\n{}",
        workspace_text(1),
        whole_source(1, 1, "root")
    );

    assert_eq!(
        parse(&text),
        Err(ConnectedWorkspaceManifestError::InvalidToml)
    );
}

#[test]
fn errors_and_debug_never_echo_private_manifest_values() {
    let package = path_text(b"private-package-canary");
    let source = source_table(
        &slot_text(1),
        &repository_text(1),
        "private-worktree-canary",
        "kind = \"full-ref\", value = \"private-selector-canary\"",
        &format!("kind = \"explicit-roots\", roots = [{package:?}]"),
    );
    let error = parse(&manifest(&[source])).expect_err("selector fixture should fail");

    for rendered in [format!("{error:?}"), error.to_string()] {
        assert!(!rendered.contains("private-worktree-canary"));
        assert!(!rendered.contains("private-selector-canary"));
        assert!(!rendered.contains("private-package-canary"));
        assert!(!rendered.contains(super::TEST_PARENT));
    }
}
