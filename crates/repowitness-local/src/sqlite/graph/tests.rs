use repowitness_application::{
    ConfigurationLayer, ConfigurationLayerKind, ConfigurationPolicyOverrides,
    ConfigurationPreferenceOverrides, resolve_configuration,
};

use super::{RustGraphReadError, RustGraphReadLimits};

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one boundary matrix keeps every independent graph-read ceiling adjacent"
)]
fn graph_read_limits_accept_inclusive_ceilings_and_reject_each_overflow() {
    let maximum = RustGraphReadLimits::try_new(
        256,
        100_000,
        1_000_000,
        4_000_000,
        1_000_000,
        256 * 1024 * 1024,
    )
    .expect("inclusive graph ceilings should be valid");
    assert_eq!(maximum.max_depth(), 256);
    assert_eq!(maximum.max_results(), 100_000);
    let explicit_input = RustGraphReadLimits::try_new_with_input(
        4_000_000,
        512 * 1024 * 1024,
        256,
        100_000,
        1_000_000,
        4_000_000,
        1_000_000,
        256 * 1024 * 1024,
    )
    .expect("inclusive graph input ceilings should be valid");
    assert_eq!(explicit_input.max_input_edges(), 4_000_000);
    assert_eq!(explicit_input.max_input_bytes(), 512 * 1024 * 1024);
    assert_eq!(
        RustGraphReadLimits::try_new_with_input(
            4_000_001,
            512 * 1024 * 1024,
            256,
            100_000,
            1_000_000,
            4_000_000,
            1_000_000,
            256 * 1024 * 1024,
        ),
        Err(RustGraphReadError::InvalidLimits)
    );
    assert_eq!(
        RustGraphReadLimits::try_new_with_input(
            4_000_000,
            512 * 1024 * 1024 + 1,
            256,
            100_000,
            1_000_000,
            4_000_000,
            1_000_000,
            256 * 1024 * 1024,
        ),
        Err(RustGraphReadError::InvalidLimits)
    );
    assert_eq!(
        RustGraphReadLimits::try_new(
            257,
            100_000,
            1_000_000,
            4_000_000,
            1_000_000,
            256 * 1024 * 1024,
        ),
        Err(RustGraphReadError::InvalidLimits)
    );
    assert_eq!(
        RustGraphReadLimits::try_new(
            256,
            100_001,
            1_000_000,
            4_000_000,
            1_000_000,
            256 * 1024 * 1024,
        ),
        Err(RustGraphReadError::InvalidLimits)
    );
    assert_eq!(
        RustGraphReadLimits::try_new(
            256,
            100_000,
            1_000_001,
            4_000_000,
            1_000_000,
            256 * 1024 * 1024,
        ),
        Err(RustGraphReadError::InvalidLimits)
    );
    assert_eq!(
        RustGraphReadLimits::try_new(
            256,
            100_000,
            1_000_000,
            4_000_001,
            1_000_000,
            256 * 1024 * 1024,
        ),
        Err(RustGraphReadError::InvalidLimits)
    );
    assert_eq!(
        RustGraphReadLimits::try_new(
            256,
            100_000,
            1_000_000,
            4_000_000,
            1_000_001,
            256 * 1024 * 1024,
        ),
        Err(RustGraphReadError::InvalidLimits)
    );
    assert_eq!(
        RustGraphReadLimits::try_new(
            256,
            100_000,
            1_000_000,
            4_000_000,
            1_000_000,
            256 * 1024 * 1024 + 1,
        ),
        Err(RustGraphReadError::InvalidLimits)
    );
    assert_eq!(
        RustGraphReadLimits::try_new(1, 1, 1, 1, 1, u64::MAX),
        Err(RustGraphReadError::InvalidLimits)
    );
}

#[test]
fn resolved_graph_preferences_only_tighten_caller_limits() {
    let preferences =
        ConfigurationPreferenceOverrides::try_new(None, None, Some(3), Some(7), None, None)
            .expect("graph preferences should validate");
    let layer = ConfigurationLayer::try_new(
        ConfigurationLayerKind::Cli,
        None,
        preferences,
        ConfigurationPolicyOverrides::default(),
    )
    .expect("CLI configuration layer should validate");
    let configuration = resolve_configuration(&[layer]).expect("graph preferences should resolve");
    let broad = RustGraphReadLimits::try_new(8, 9, 100, 100, 100, 4096)
        .expect("caller limits should validate")
        .constrained_by(Some(&configuration));
    assert_eq!(broad.max_depth(), 3);
    assert_eq!(broad.max_results(), 7);

    let tighter = RustGraphReadLimits::try_new(2, 5, 100, 100, 100, 4096)
        .expect("tighter caller limits should validate")
        .constrained_by(Some(&configuration));
    assert_eq!(tighter.max_depth(), 2);
    assert_eq!(tighter.max_results(), 5);
    assert_eq!(
        tighter.constrained_by(None),
        tighter,
        "absence of a resolved configuration must preserve caller limits"
    );
}
