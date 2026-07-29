use std::collections::BTreeSet;

use super::*;
use crate::SourceLanguage;

mod retention;

const CALLER_LAYERS: [ConfigurationLayerKind; 5] = [
    ConfigurationLayerKind::User,
    ConfigurationLayerKind::Workspace,
    ConfigurationLayerKind::Repository,
    ConfigurationLayerKind::Environment,
    ConfigurationLayerKind::Cli,
];

#[test]
fn defaults_are_versioned_bounded_and_have_a_stable_golden_digest() {
    let resolved = resolve_configuration(&[]).expect("built-in configuration");

    assert_eq!(resolved.schema_version(), CONFIGURATION_SCHEMA_VERSION);
    assert_eq!(resolved.resolver_version(), CONFIGURATION_RESOLVER_VERSION);
    assert_eq!(resolved.profile(), ConfigurationProfile::Local);
    assert_eq!(
        resolved.profile_supplied_by(),
        ConfigurationLayerKind::BuiltInDefaults
    );
    assert_eq!(
        *resolved.preferences().query_results().effective(),
        u64::from(crate::DEFAULT_CODE_SEARCH_RESULTS)
    );
    assert_eq!(
        *resolved.preferences().context_bytes().effective(),
        crate::DEFAULT_CONTEXT_BUILD_BUDGET_UNITS
    );
    assert!(!*resolved.policy().deny_memory_writes().effective());
    assert!(!*resolved.policy().follow_symlinks().effective());
    assert_eq!(
        resolved.preferences().mcp_tool_profile().requested(),
        McpToolProfile::Canonical
    );
    assert_eq!(
        resolved.preferences().mcp_tool_profile().authorized(),
        Some(McpToolProfile::Canonical)
    );
    assert_eq!(
        resolved.policy().allowed_mcp_tool_profiles().effective(),
        &[
            McpToolProfile::Canonical,
            McpToolProfile::IncumbentCompatible
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(
        resolved.policy().allowed_languages().effective(),
        &all_languages()
    );
    assert!(
        resolved
            .policy()
            .deny_memory_writes()
            .constraining_layers()
            .is_empty(),
        "an inactive deny has no constraining provenance"
    );
    assert_eq!(
        hex(resolved.digest().as_bytes()),
        "e26f3f0c86f3dca85b2af3189445b0d60adbcf470b7de377b1b147e3fd65cec2"
    );
}

#[test]
fn ordinary_preferences_follow_precedence_and_preserve_the_winner() {
    let values = [30, 40, 50, 60, 70];
    let layers = CALLER_LAYERS
        .into_iter()
        .zip(values)
        .map(|(kind, value)| {
            layer(
                kind,
                None,
                preferences(Some(value), None),
                ConfigurationPolicyOverrides::default(),
            )
        })
        .collect::<Vec<_>>();

    let resolved = resolve_configuration(&layers).expect("layered preferences");
    let query = resolved.preferences().query_results();
    assert_eq!(*query.requested(), 70);
    assert_eq!(*query.effective(), 70);
    assert_eq!(query.supplied_by(), ConfigurationLayerKind::Cli);
    assert!(query.constrained_by().is_empty());
}

#[test]
fn profile_selection_is_limited_to_user_and_cli_layers() {
    for kind in [
        ConfigurationLayerKind::Workspace,
        ConfigurationLayerKind::Repository,
        ConfigurationLayerKind::Environment,
    ] {
        assert_eq!(
            ConfigurationLayer::try_new(
                kind,
                Some(ConfigurationProfile::Local),
                ConfigurationPreferenceOverrides::default(),
                ConfigurationPolicyOverrides::default(),
            ),
            Err(ConfigurationValidationError::ProfileSelectionNotAllowed)
        );
    }
    let layers = [
        layer(
            ConfigurationLayerKind::Cli,
            Some(ConfigurationProfile::Local),
            ConfigurationPreferenceOverrides::default(),
            ConfigurationPolicyOverrides::default(),
        ),
        layer(
            ConfigurationLayerKind::User,
            Some(ConfigurationProfile::Local),
            ConfigurationPreferenceOverrides::default(),
            ConfigurationPolicyOverrides::default(),
        ),
    ];
    let resolved = resolve_configuration(&layers).expect("trusted profile selection");
    assert_eq!(resolved.profile_supplied_by(), ConfigurationLayerKind::Cli);

    let same_semantics_without_explicit_selection = resolve_configuration(&[]).expect("defaults");
    assert_eq!(
        resolved.digest(),
        same_semantics_without_explicit_selection.digest(),
        "profile provenance must not enter semantic identity"
    );
}

#[test]
fn every_allowed_language_pair_resolves_to_exact_intersection() {
    for left_mask in 0_u8..32 {
        for right_mask in 0_u8..32 {
            let left = languages(left_mask);
            let right = languages(right_mask);
            let layers = [
                layer(
                    ConfigurationLayerKind::User,
                    None,
                    ConfigurationPreferenceOverrides::default(),
                    policy(Some(left.clone()), None, None),
                ),
                layer(
                    ConfigurationLayerKind::Repository,
                    None,
                    ConfigurationPreferenceOverrides::default(),
                    policy(Some(right.clone()), None, None),
                ),
            ];
            let resolved = resolve_configuration(&layers).expect("language intersection");
            let expected = left.intersection(&right).copied().collect::<BTreeSet<_>>();
            assert_eq!(
                resolved.policy().allowed_languages().effective(),
                &expected,
                "left={left_mask:#07b} right={right_mask:#07b}"
            );
        }
    }
}

#[test]
fn every_tool_profile_pair_can_only_shrink_compiled_authority() {
    let compiled = tool_profiles(0b101);
    for left_mask in 0_u8..8 {
        for right_mask in 0_u8..8 {
            let left = tool_profiles(left_mask);
            let right = tool_profiles(right_mask);
            let layers = [
                layer(
                    ConfigurationLayerKind::User,
                    None,
                    ConfigurationPreferenceOverrides::default(),
                    tool_policy(Some(left.clone())),
                ),
                layer(
                    ConfigurationLayerKind::Repository,
                    None,
                    ConfigurationPreferenceOverrides::default(),
                    tool_policy(Some(right.clone())),
                ),
            ];
            let resolved = resolve_configuration(&layers).expect("tool-profile intersection");
            let expected = compiled
                .intersection(&left)
                .copied()
                .collect::<BTreeSet<_>>()
                .intersection(&right)
                .copied()
                .collect::<BTreeSet<_>>();
            assert_eq!(
                resolved.policy().allowed_mcp_tool_profiles().effective(),
                &expected,
                "left={left_mask:#05b} right={right_mask:#05b}"
            );
        }
    }
}

#[test]
fn repository_and_workspace_tool_profile_requests_never_grant_startup_authority() {
    let all_profiles: BTreeSet<McpToolProfile> = [
        McpToolProfile::Canonical,
        McpToolProfile::Minimal,
        McpToolProfile::IncumbentCompatible,
    ]
    .into_iter()
    .collect();
    for layer_kind in [
        ConfigurationLayerKind::Workspace,
        ConfigurationLayerKind::Repository,
    ] {
        let untrusted = ConfigurationLayer::try_new(
            layer_kind,
            None,
            preferences(None, Some(McpToolProfile::IncumbentCompatible)),
            ConfigurationPolicyOverrides::try_new(
                None,
                Some(all_profiles.clone()),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .expect("untrusted policy request"),
        )
        .expect("untrusted layer");
        let resolved = resolve_configuration(&[untrusted]).expect("tool profile policy");
        let requested = resolved.preferences().mcp_tool_profile();
        assert_eq!(requested.requested(), McpToolProfile::IncumbentCompatible);
        assert_eq!(requested.supplied_by(), layer_kind);
        assert_eq!(requested.authorized(), None);
        assert_eq!(
            requested.constrained_by(),
            [ConfigurationLayerKind::BuiltInDefaults]
        );
        assert_eq!(
            resolved.policy().allowed_mcp_tool_profiles().effective(),
            &[
                McpToolProfile::Canonical,
                McpToolProfile::IncumbentCompatible
            ]
            .into_iter()
            .collect(),
            "untrusted allow-lists cannot change compiled authority"
        );
    }
}

#[test]
fn only_user_and_cli_layers_can_select_the_compiled_compatibility_profile() {
    for layer_kind in [ConfigurationLayerKind::User, ConfigurationLayerKind::Cli] {
        let trusted = layer(
            layer_kind,
            None,
            preferences(None, Some(McpToolProfile::IncumbentCompatible)),
            ConfigurationPolicyOverrides::default(),
        );
        let resolved = resolve_configuration(&[trusted]).expect("trusted tool profile");
        let preference = resolved.preferences().mcp_tool_profile();
        assert_eq!(preference.requested(), McpToolProfile::IncumbentCompatible);
        assert_eq!(
            preference.authorized(),
            Some(McpToolProfile::IncumbentCompatible)
        );
    }

    let user = layer(
        ConfigurationLayerKind::User,
        None,
        preferences(None, Some(McpToolProfile::Minimal)),
        ConfigurationPolicyOverrides::default(),
    );
    assert_eq!(
        resolve_configuration(&[user])
            .expect("minimal request resolves as denied")
            .preferences()
            .mcp_tool_profile()
            .authorized(),
        None,
        "the unimplemented minimal surface remains outside compiled authority"
    );
}

#[test]
fn deny_policy_is_monotonic_for_every_layer_combination() {
    for mask in 0_u8..32 {
        let layers = CALLER_LAYERS
            .into_iter()
            .enumerate()
            .map(|(index, kind)| {
                layer(
                    kind,
                    None,
                    ConfigurationPreferenceOverrides::default(),
                    policy(None, None, Some(mask & (1 << index) != 0)),
                )
            })
            .collect::<Vec<_>>();
        let resolved = resolve_configuration(&layers).expect("deny union");
        assert_eq!(
            *resolved.policy().deny_memory_writes().effective(),
            mask != 0
        );
        for (index, kind) in CALLER_LAYERS.into_iter().enumerate() {
            assert_eq!(
                resolved
                    .policy()
                    .deny_memory_writes()
                    .constraining_layers()
                    .contains(&kind),
                mask & (1 << index) != 0
            );
        }
    }
}

#[test]
fn numeric_policy_uses_minimum_and_ignores_broader_later_requests() {
    let base = [
        layer(
            ConfigurationLayerKind::User,
            None,
            ConfigurationPreferenceOverrides::default(),
            policy(None, Some(80), None),
        ),
        layer(
            ConfigurationLayerKind::Workspace,
            None,
            ConfigurationPreferenceOverrides::default(),
            policy(None, Some(20), None),
        ),
        layer(
            ConfigurationLayerKind::Repository,
            None,
            ConfigurationPreferenceOverrides::default(),
            policy(None, Some(60), None),
        ),
    ];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let layers = order.map(|index| base[index].clone());
        let resolved = resolve_configuration(&layers).expect("numeric minimum");
        let maximum = resolved.policy().max_query_results();
        assert_eq!(*maximum.effective(), 20);
        assert_eq!(
            maximum.constraining_layers(),
            [
                ConfigurationLayerKind::BuiltInDefaults,
                ConfigurationLayerKind::User,
                ConfigurationLayerKind::Workspace,
            ]
        );
    }
}

#[test]
fn effective_preference_reports_the_policy_that_capped_its_winner() {
    let layers = [
        layer(
            ConfigurationLayerKind::User,
            None,
            ConfigurationPreferenceOverrides::default(),
            policy(None, Some(30), None),
        ),
        layer(
            ConfigurationLayerKind::Cli,
            None,
            preferences(Some(80), None),
            ConfigurationPolicyOverrides::default(),
        ),
    ];
    let resolved = resolve_configuration(&layers).expect("capped preference");
    let query = resolved.preferences().query_results();
    assert_eq!(*query.requested(), 80);
    assert_eq!(*query.effective(), 30);
    assert_eq!(query.supplied_by(), ConfigurationLayerKind::Cli);
    assert_eq!(
        query.constrained_by(),
        [
            ConfigurationLayerKind::BuiltInDefaults,
            ConfigurationLayerKind::User,
        ]
    );
}

#[test]
fn digest_is_deterministic_and_excludes_provenance() {
    let user = layer(
        ConfigurationLayerKind::User,
        None,
        preferences(Some(30), None),
        ConfigurationPolicyOverrides::default(),
    );
    let repository = layer(
        ConfigurationLayerKind::Repository,
        None,
        preferences(Some(30), None),
        ConfigurationPolicyOverrides::default(),
    );
    let user_resolved = resolve_configuration(&[user]).expect("user value");
    let repository_resolved = resolve_configuration(&[repository]).expect("repository value");
    assert_ne!(
        user_resolved.preferences().query_results().supplied_by(),
        repository_resolved
            .preferences()
            .query_results()
            .supplied_by()
    );
    assert_eq!(user_resolved.digest(), repository_resolved.digest());

    let user_policy = resolve_configuration(&[layer(
        ConfigurationLayerKind::User,
        None,
        ConfigurationPreferenceOverrides::default(),
        policy(None, Some(30), None),
    )])
    .expect("user policy");
    let repository_policy = resolve_configuration(&[layer(
        ConfigurationLayerKind::Repository,
        None,
        ConfigurationPreferenceOverrides::default(),
        policy(None, Some(30), None),
    )])
    .expect("repository policy");
    assert_ne!(
        user_policy
            .policy()
            .max_query_results()
            .constraining_layers(),
        repository_policy
            .policy()
            .max_query_results()
            .constraining_layers()
    );
    assert_eq!(user_policy.digest(), repository_policy.digest());

    let changed = resolve_configuration(&[layer(
        ConfigurationLayerKind::Repository,
        None,
        preferences(Some(31), None),
        ConfigurationPolicyOverrides::default(),
    )])
    .expect("changed semantics");
    assert_ne!(changed.digest(), user_resolved.digest());
}

#[test]
fn resolver_rejects_duplicate_and_excessive_layer_categories() {
    let duplicate = layer(
        ConfigurationLayerKind::User,
        None,
        ConfigurationPreferenceOverrides::default(),
        ConfigurationPolicyOverrides::default(),
    );
    assert_eq!(
        resolve_configuration(&[duplicate.clone(), duplicate]),
        Err(ConfigurationResolutionError::DuplicateLayer(
            ConfigurationLayerKind::User
        ))
    );

    let repeated = layer(
        ConfigurationLayerKind::User,
        None,
        ConfigurationPreferenceOverrides::default(),
        ConfigurationPolicyOverrides::default(),
    );
    assert_eq!(
        resolve_configuration(&[
            repeated.clone(),
            repeated.clone(),
            repeated.clone(),
            repeated.clone(),
            repeated.clone(),
            repeated,
        ]),
        Err(ConfigurationResolutionError::TooManyLayers)
    );
}

#[test]
fn validation_errors_and_debug_output_are_value_and_path_free() {
    let error = ConfigurationPreferenceOverrides::try_new(
        Some(MAX_CONFIGURATION_QUERY_RESULTS + 1),
        None,
        None,
        None,
        None,
        None,
    )
    .expect_err("above hard limit");
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains(&(MAX_CONFIGURATION_QUERY_RESULTS + 1).to_string()));
    assert!(!rendered.contains("/private"));
    assert!(!rendered.contains("secret"));

    assert_eq!(
        ConfigurationPolicyOverrides::try_new(
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(true),
        ),
        Err(ConfigurationValidationError::FollowSymlinksUnsupported)
    );
}

fn preferences(
    query_results: Option<u64>,
    tool_profile: Option<McpToolProfile>,
) -> ConfigurationPreferenceOverrides {
    ConfigurationPreferenceOverrides::try_new(query_results, None, None, None, None, tool_profile)
        .expect("test preferences")
}

fn policy(
    allowed_languages: Option<BTreeSet<SourceLanguage>>,
    max_query_results: Option<u64>,
    deny_memory_writes: Option<bool>,
) -> ConfigurationPolicyOverrides {
    ConfigurationPolicyOverrides::try_new(
        allowed_languages,
        None,
        None,
        None,
        max_query_results,
        None,
        None,
        None,
        deny_memory_writes,
        None,
    )
    .expect("test policy")
}

fn tool_policy(
    allowed_mcp_tool_profiles: Option<BTreeSet<McpToolProfile>>,
) -> ConfigurationPolicyOverrides {
    ConfigurationPolicyOverrides::try_new(
        None,
        allowed_mcp_tool_profiles,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("test tool-profile policy")
}

fn layer(
    kind: ConfigurationLayerKind,
    profile: Option<ConfigurationProfile>,
    preferences: ConfigurationPreferenceOverrides,
    policy: ConfigurationPolicyOverrides,
) -> ConfigurationLayer {
    ConfigurationLayer::try_new(kind, profile, preferences, policy).expect("test layer")
}

fn all_languages() -> BTreeSet<SourceLanguage> {
    languages(0b1_1111)
}

fn languages(mask: u8) -> BTreeSet<SourceLanguage> {
    [
        SourceLanguage::Rust,
        SourceLanguage::Go,
        SourceLanguage::TypeScript,
        SourceLanguage::Tsx,
        SourceLanguage::Python,
    ]
    .into_iter()
    .enumerate()
    .filter_map(|(index, language)| (mask & (1 << index) != 0).then_some(language))
    .collect()
}

fn tool_profiles(mask: u8) -> BTreeSet<McpToolProfile> {
    [
        McpToolProfile::Canonical,
        McpToolProfile::Minimal,
        McpToolProfile::IncumbentCompatible,
    ]
    .into_iter()
    .enumerate()
    .filter_map(|(index, profile)| (mask & (1 << index) != 0).then_some(profile))
    .collect()
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    bytes.iter().fold(
        String::with_capacity(bytes.len() * 2),
        |mut output, byte| {
            write!(&mut output, "{byte:02x}").expect("string writes cannot fail");
            output
        },
    )
}
