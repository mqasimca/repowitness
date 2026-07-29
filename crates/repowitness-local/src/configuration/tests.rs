use repowitness_application::{
    ConfigurationLayerKind, ConfigurationProfile, McpToolProfile, SourceLanguage,
    resolve_configuration,
};

use super::*;

#[test]
fn complete_version_one_file_decodes_then_resolves_without_wire_types() {
    let input = br#"
schema_version = 1
profile = "local"

[preferences]
query_results = 25
context_bytes = 70000
graph_depth = 7
graph_results = 900
watcher_poll_interval_ms = 1500
mcp_tool_profile = "minimal"

[policy]
allowed_languages = ["rust", "go", "typescript", "tsx", "python"]
allowed_mcp_tool_profiles = ["canonical", "minimal", "incumbent-compatible"]
max_source_file_bytes = 8000000
max_source_files = 100000
max_query_results = 24
max_context_bytes = 60000
max_graph_depth = 6
max_graph_results = 800
retained_generations_per_source_slot = 4
max_retention_generation_candidates = 48
max_retention_rows = 900000
max_retention_bytes = 419430400
deny_memory_writes = true
follow_symlinks = false
"#;
    let layer =
        parse_configuration_file(input, ConfigurationFileLayer::User).expect("valid config");
    let resolved = resolve_configuration(&[layer]).expect("resolved config");

    assert_eq!(resolved.profile(), ConfigurationProfile::Local);
    assert_eq!(resolved.profile_supplied_by(), ConfigurationLayerKind::User);
    assert_eq!(*resolved.preferences().query_results().requested(), 25);
    assert_eq!(*resolved.preferences().query_results().effective(), 24);
    assert_eq!(*resolved.preferences().context_bytes().effective(), 60_000);
    assert_eq!(*resolved.preferences().graph_depth().effective(), 6);
    assert_eq!(*resolved.preferences().graph_results().effective(), 800);
    assert_eq!(
        resolved.preferences().mcp_tool_profile().requested(),
        McpToolProfile::Minimal
    );
    assert_eq!(
        resolved.preferences().mcp_tool_profile().authorized(),
        None,
        "parsing a profile request does not authorize an unimplemented startup surface"
    );
    assert_eq!(
        resolved.policy().allowed_mcp_tool_profiles().effective(),
        &[
            McpToolProfile::Canonical,
            McpToolProfile::IncumbentCompatible,
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(
        resolved.policy().allowed_languages().effective(),
        &[
            SourceLanguage::Rust,
            SourceLanguage::Go,
            SourceLanguage::TypeScript,
            SourceLanguage::Tsx,
            SourceLanguage::Python,
        ]
        .into_iter()
        .collect()
    );
    assert!(*resolved.policy().deny_memory_writes().effective());
    assert!(!*resolved.policy().follow_symlinks().effective());
    let retention = resolved.policy().retention();
    assert_eq!(
        *retention.retained_generations_per_source_slot().effective(),
        4
    );
    assert_eq!(*retention.max_generation_candidates().effective(), 48);
    assert_eq!(*retention.max_rows().effective(), 900_000);
    assert_eq!(*retention.max_bytes().effective(), 419_430_400);
}

#[test]
fn exact_file_limit_is_inclusive_and_one_extra_byte_fails_before_parsing() {
    let prefix = b"schema_version = 1\n#";
    let mut exact = Vec::with_capacity(MAX_CONFIGURATION_FILE_BYTES);
    exact.extend_from_slice(prefix);
    exact.resize(MAX_CONFIGURATION_FILE_BYTES, b'a');
    parse_configuration_file(&exact, ConfigurationFileLayer::User)
        .expect("exact inclusive byte limit");

    exact.push(b'a');
    assert_eq!(
        parse_configuration_file(&exact, ConfigurationFileLayer::User),
        Err(ConfigurationFileError::FileTooLarge)
    );
}

#[test]
fn invalid_utf8_unknown_fields_and_duplicates_fail_closed() {
    assert_eq!(
        parse_configuration_file(
            b"schema_version = 1\nprofile = \"local\"\n\xff",
            ConfigurationFileLayer::User,
        ),
        Err(ConfigurationFileError::InvalidUtf8)
    );
    for input in [
        "schema_version = 1\nunknown = true\n",
        "schema_version = 1\nschema_version = 1\n",
        "schema_version = 1\n[preferences]\nquery_results = 2\nquery_results = 3\n",
        "schema_version = 1\n[preferences]\n[preferences.extra]\nvalue = true\n",
        "schema_version = 1\npreferences.extra = true\n",
        "schema_version = 1\n[policy]\nunknown = true\n",
        "schema_version = 1\n[[policy]]\n",
        "schema_version = 1\n[unknown]\nvalue = true\n",
    ] {
        assert_eq!(
            parse_configuration_file(input.as_bytes(), ConfigurationFileLayer::User),
            Err(ConfigurationFileError::InvalidToml),
            "{input}"
        );
    }
}

#[test]
fn unsupported_missing_and_wrong_type_versions_are_rejected() {
    for version in [0, 2, u64::MAX] {
        let input = format!("schema_version = {version}\n");
        assert_eq!(
            parse_configuration_file(input.as_bytes(), ConfigurationFileLayer::User),
            Err(ConfigurationFileError::UnsupportedSchemaVersion)
        );
    }
    for input in [
        "",
        "profile = \"local\"\n",
        "schema_version = \"1\"\n",
        "schema_version = -1\n",
        "schema_version = 1.0\n",
    ] {
        assert_eq!(
            parse_configuration_file(input.as_bytes(), ConfigurationFileLayer::User),
            Err(ConfigurationFileError::InvalidToml),
            "{input}"
        );
    }
}

#[test]
fn every_numeric_boundary_rejects_zero_wrong_types_and_excess() {
    let invalid_documents = [
        "schema_version=1\n[preferences]\nquery_results=0\n",
        "schema_version=1\n[preferences]\nquery_results=101\n",
        "schema_version=1\n[preferences]\ncontext_bytes=0\n",
        "schema_version=1\n[preferences]\ncontext_bytes=1048577\n",
        "schema_version=1\n[preferences]\ngraph_depth=0\n",
        "schema_version=1\n[preferences]\ngraph_depth=65\n",
        "schema_version=1\n[preferences]\ngraph_results=0\n",
        "schema_version=1\n[preferences]\ngraph_results=10001\n",
        "schema_version=1\n[preferences]\nwatcher_poll_interval_ms=99\n",
        "schema_version=1\n[preferences]\nwatcher_poll_interval_ms=86400001\n",
        "schema_version=1\n[policy]\nmax_source_file_bytes=0\n",
        "schema_version=1\n[policy]\nmax_source_file_bytes=268435457\n",
        "schema_version=1\n[policy]\nmax_source_files=0\n",
        "schema_version=1\n[policy]\nmax_source_files=1000001\n",
        "schema_version=1\n[policy]\nmax_query_results=0\n",
        "schema_version=1\n[policy]\nmax_query_results=101\n",
        "schema_version=1\n[policy]\nmax_context_bytes=0\n",
        "schema_version=1\n[policy]\nmax_context_bytes=1048577\n",
        "schema_version=1\n[policy]\nmax_graph_depth=0\n",
        "schema_version=1\n[policy]\nmax_graph_depth=65\n",
        "schema_version=1\n[policy]\nmax_graph_results=0\n",
        "schema_version=1\n[policy]\nmax_graph_results=10001\n",
        "schema_version=1\n[policy]\nretained_generations_per_source_slot=0\n",
        "schema_version=1\n[policy]\nretained_generations_per_source_slot=4097\n",
        "schema_version=1\n[policy]\nmax_retention_generation_candidates=0\n",
        "schema_version=1\n[policy]\nmax_retention_generation_candidates=4097\n",
        "schema_version=1\n[policy]\nmax_retention_rows=0\n",
        "schema_version=1\n[policy]\nmax_retention_rows=100000001\n",
        "schema_version=1\n[policy]\nmax_retention_bytes=0\n",
        "schema_version=1\n[policy]\nmax_retention_bytes=17179869185\n",
        "schema_version=1\n[preferences]\nquery_results=-1\n",
        "schema_version=1\n[preferences]\nquery_results=1.0\n",
        "schema_version=1\n[preferences]\nquery_results=\"1\"\n",
    ];
    for input in invalid_documents {
        assert!(
            parse_configuration_file(input.as_bytes(), ConfigurationFileLayer::User).is_err(),
            "{input}"
        );
    }
}

#[test]
fn enum_text_and_language_collections_are_independently_bounded() {
    for input in [
        "schema_version=1\nprofile=\"\"\n",
        "schema_version=1\nprofile=\"unknown\"\n",
        "schema_version=1\nprofile=\"abcdefghijklmnopqrstuvwxyz1234567\"\n",
        "schema_version=1\n[preferences]\nmcp_tool_profile=\"unknown\"\n",
        "schema_version=1\n[policy]\nallowed_languages=[\"unknown\"]\n",
        "schema_version=1\n[policy]\nallowed_languages=[\"rust\",\"rust\"]\n",
        "schema_version=1\n[policy]\nallowed_languages=[\"rust\",\"go\",\"typescript\",\"tsx\",\"python\",\"rust\"]\n",
        "schema_version=1\n[policy]\nallowed_mcp_tool_profiles=[\"canonical\",\"canonical\"]\n",
        "schema_version=1\n[policy]\nallowed_mcp_tool_profiles=[\"canonical\",\"minimal\",\"incumbent-compatible\",\"canonical\"]\n",
        "schema_version=1\n[policy]\nallowed_mcp_tool_profiles=[\"unknown\"]\n",
        "schema_version=1\n[policy]\ndeny_memory_writes=\"true\"\n",
        "schema_version=1\n[policy]\nfollow_symlinks=true\n",
    ] {
        assert!(
            parse_configuration_file(input.as_bytes(), ConfigurationFileLayer::User).is_err(),
            "{input}"
        );
    }

    for spelling in ["canonical", "minimal", "incumbent-compatible"] {
        let input = format!("schema_version=1\n[preferences]\nmcp_tool_profile=\"{spelling}\"\n");
        parse_configuration_file(input.as_bytes(), ConfigurationFileLayer::User)
            .expect("supported tool profile");
    }

    let empty_policy = parse_configuration_file(
        b"schema_version=1\n[policy]\nallowed_languages=[]\nallowed_mcp_tool_profiles=[]\n",
        ConfigurationFileLayer::Repository,
    )
    .expect("empty allow sets are valid tightening requests");
    let resolved = resolve_configuration(&[empty_policy]).expect("empty policy");
    assert!(
        resolved.policy().allowed_languages().effective().is_empty()
            && resolved
                .policy()
                .allowed_mcp_tool_profiles()
                .effective()
                .is_empty()
    );
}

#[test]
fn profile_selection_from_workspace_or_repository_is_rejected_after_decode() {
    for layer in [
        ConfigurationFileLayer::Workspace,
        ConfigurationFileLayer::Repository,
    ] {
        assert_eq!(
            parse_configuration_file(b"schema_version=1\nprofile=\"local\"\n", layer),
            Err(ConfigurationFileError::Validation(
                repowitness_application::ConfigurationValidationError::ProfileSelectionNotAllowed
            ))
        );
    }
}

#[test]
fn secret_path_command_and_endpoint_fields_are_never_admitted_or_rendered() {
    let marker = "SUPER_SECRET_MARKER";
    for field in ["token", "credential", "path", "command", "endpoint"] {
        let input = format!("schema_version=1\n{field}=\"{marker}/Users/private/repository\"\n");
        let error = parse_configuration_file(input.as_bytes(), ConfigurationFileLayer::Repository)
            .expect_err("unsupported sensitive field");
        let rendered = format!("{error:?} {error}");
        assert!(!rendered.contains(marker));
        assert!(!rendered.contains("/Users/private"));
        assert!(!rendered.contains("repository"));
        assert_eq!(error, ConfigurationFileError::InvalidToml);
    }
}

#[test]
fn parsed_debug_and_validation_errors_contain_no_host_path_or_source_text() {
    let parsed = parse_configuration_file(
        b"schema_version=1\n[policy]\ndeny_memory_writes=true\n",
        ConfigurationFileLayer::Repository,
    )
    .expect("valid layer");
    let rendered = format!("{parsed:?}");
    assert!(!rendered.contains("/private"));
    assert!(!rendered.contains("secret"));

    let input = b"schema_version=1\nprofile=\"private-path-marker\"\n";
    let error =
        parse_configuration_file(input, ConfigurationFileLayer::User).expect_err("invalid profile");
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains("private-path-marker"));
}
