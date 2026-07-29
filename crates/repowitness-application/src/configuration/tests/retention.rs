use super::super::*;

#[test]
fn defaults_match_the_bounded_sqlite_retention_contract() {
    let resolved = resolve_configuration(&[]).expect("built-in configuration");
    let retention = resolved.policy().retention();

    assert_eq!(
        *retention.retained_generations_per_source_slot().effective(),
        DEFAULT_CONFIGURATION_RETAINED_GENERATIONS_PER_SOURCE_SLOT
    );
    assert_eq!(
        *retention.max_generation_candidates().effective(),
        DEFAULT_CONFIGURATION_RETENTION_GENERATION_CANDIDATES
    );
    assert_eq!(
        *retention.max_rows().effective(),
        DEFAULT_CONFIGURATION_RETENTION_ROWS
    );
    assert_eq!(
        *retention.max_bytes().effective(),
        DEFAULT_CONFIGURATION_RETENTION_BYTES
    );
}

#[test]
fn retention_values_reject_zero_and_values_above_absolute_bounds() {
    let fields = [
        (
            RetentionConfigurationOverrides::try_new(Some(0), None, None, None),
            ConfigurationField::RetainedGenerationsPerSourceSlot,
        ),
        (
            RetentionConfigurationOverrides::try_new(None, Some(0), None, None),
            ConfigurationField::RetentionGenerationCandidates,
        ),
        (
            RetentionConfigurationOverrides::try_new(None, None, Some(0), None),
            ConfigurationField::RetentionRows,
        ),
        (
            RetentionConfigurationOverrides::try_new(None, None, None, Some(0)),
            ConfigurationField::RetentionBytes,
        ),
    ];
    for (result, field) in fields {
        assert_eq!(result, Err(ConfigurationValidationError::Zero(field)));
    }

    let above = [
        (
            RetentionConfigurationOverrides::try_new(
                Some(MAX_CONFIGURATION_RETAINED_GENERATIONS_PER_SOURCE_SLOT + 1),
                None,
                None,
                None,
            ),
            ConfigurationField::RetainedGenerationsPerSourceSlot,
        ),
        (
            RetentionConfigurationOverrides::try_new(
                None,
                Some(MAX_CONFIGURATION_RETENTION_GENERATION_CANDIDATES + 1),
                None,
                None,
            ),
            ConfigurationField::RetentionGenerationCandidates,
        ),
        (
            RetentionConfigurationOverrides::try_new(
                None,
                None,
                Some(MAX_CONFIGURATION_RETENTION_ROWS + 1),
                None,
            ),
            ConfigurationField::RetentionRows,
        ),
        (
            RetentionConfigurationOverrides::try_new(
                None,
                None,
                None,
                Some(MAX_CONFIGURATION_RETENTION_BYTES + 1),
            ),
            ConfigurationField::RetentionBytes,
        ),
    ];
    for (result, field) in above {
        assert_eq!(
            result,
            Err(ConfigurationValidationError::AboveMaximum(field))
        );
    }
}

#[test]
fn every_retention_layer_can_only_tighten_the_effective_policy() {
    let user = layer_with_retention(
        ConfigurationLayerKind::User,
        RetentionConfigurationOverrides::try_new(
            Some(4),
            Some(48),
            Some(900_000),
            Some(400 * 1024 * 1024),
        )
        .expect("user retention"),
    );
    let repository = layer_with_retention(
        ConfigurationLayerKind::Repository,
        RetentionConfigurationOverrides::try_new(
            Some(3),
            Some(56),
            Some(950_000),
            Some(450 * 1024 * 1024),
        )
        .expect("broader repository retention"),
    );
    let resolved = resolve_configuration(&[repository, user]).expect("resolved retention");
    let retention = resolved.policy().retention();

    assert_eq!(
        *retention.retained_generations_per_source_slot().effective(),
        4
    );
    assert_eq!(*retention.max_generation_candidates().effective(), 48);
    assert_eq!(*retention.max_rows().effective(), 900_000);
    assert_eq!(*retention.max_bytes().effective(), 400 * 1024 * 1024);
    for value in [
        retention.retained_generations_per_source_slot(),
        retention.max_generation_candidates(),
        retention.max_rows(),
        retention.max_bytes(),
    ] {
        assert_eq!(
            value.constraining_layers(),
            &[
                ConfigurationLayerKind::BuiltInDefaults,
                ConfigurationLayerKind::User
            ]
        );
    }
}

#[test]
fn each_effective_retention_value_participates_in_the_semantic_digest() {
    let default = resolve_configuration(&[]).expect("default").digest();
    for override_value in [
        RetentionConfigurationOverrides::try_new(Some(3), None, None, None),
        RetentionConfigurationOverrides::try_new(None, Some(63), None, None),
        RetentionConfigurationOverrides::try_new(None, None, Some(999_999), None),
        RetentionConfigurationOverrides::try_new(None, None, None, Some(536_870_911)),
    ] {
        let resolved = resolve_configuration(&[layer_with_retention(
            ConfigurationLayerKind::User,
            override_value.expect("valid override"),
        )])
        .expect("resolved override");
        assert_ne!(resolved.digest(), default);
    }

    let request =
        RetentionConfigurationOverrides::try_new(Some(3), Some(32), Some(500_000), Some(1_000_000))
            .expect("valid request");
    let user =
        resolve_configuration(&[layer_with_retention(ConfigurationLayerKind::User, request)])
            .expect("user request");
    let repository = resolve_configuration(&[layer_with_retention(
        ConfigurationLayerKind::Repository,
        request,
    )])
    .expect("repository request");
    assert_eq!(
        user.digest(),
        repository.digest(),
        "presentation-only provenance does not enter semantic identity"
    );
}

fn layer_with_retention(
    kind: ConfigurationLayerKind,
    retention: RetentionConfigurationOverrides,
) -> ConfigurationLayer {
    let policy = ConfigurationPolicyOverrides::try_new(
        None, None, None, None, None, None, None, None, None, None,
    )
    .expect("empty policy")
    .with_retention(retention);
    ConfigurationLayer::try_new(
        kind,
        None,
        ConfigurationPreferenceOverrides::default(),
        policy,
    )
    .expect("retention layer")
}
