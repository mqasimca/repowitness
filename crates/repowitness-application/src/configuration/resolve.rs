use std::{collections::BTreeSet, error::Error, fmt};

use super::{
    CONFIGURATION_SCHEMA_VERSION, ConfigurationLayer, ConfigurationLayerKind,
    ConfigurationPolicyOverrides, ConfigurationPreferenceOverrides, ConfigurationProfile,
    DEFAULT_CONFIGURATION_RETAINED_GENERATIONS_PER_SOURCE_SLOT,
    DEFAULT_CONFIGURATION_RETENTION_BYTES, DEFAULT_CONFIGURATION_RETENTION_GENERATION_CANDIDATES,
    DEFAULT_CONFIGURATION_RETENTION_ROWS, EffectiveConfigurationPolicy,
    EffectiveConfigurationPreferences, EffectiveRetentionConfiguration,
    MAX_CONFIGURATION_CONTEXT_BYTES, MAX_CONFIGURATION_FILE_LAYERS, MAX_CONFIGURATION_GRAPH_DEPTH,
    MAX_CONFIGURATION_GRAPH_RESULTS, MAX_CONFIGURATION_QUERY_RESULTS,
    MAX_CONFIGURATION_SOURCE_FILE_BYTES, MAX_CONFIGURATION_SOURCE_FILES, McpToolProfile,
    PolicyValue, ResolvedConfiguration, ResolvedPreference, ResolvedToolProfilePreference,
    digest::canonical_configuration_digest, model::SUPPORTED_CONFIGURATION_LANGUAGES,
};
use crate::{DEFAULT_CODE_SEARCH_RESULTS, DEFAULT_CONTEXT_BUILD_BUDGET_UNITS, SourceLanguage};

/// Deterministic configuration resolver version.
pub const CONFIGURATION_RESOLVER_VERSION: u16 = 1;

const DEFAULT_GRAPH_DEPTH: u64 = 8;
const DEFAULT_GRAPH_RESULTS: u64 = 1_000;
const DEFAULT_WATCHER_POLL_INTERVAL_MS: u64 = 2_000;

/// Stable content-redacted failure to resolve configuration layers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationResolutionError {
    /// More caller layers were supplied than the fixed schema permits.
    TooManyLayers,
    /// The same provenance category was supplied more than once.
    DuplicateLayer(ConfigurationLayerKind),
}

impl fmt::Display for ConfigurationResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TooManyLayers => "configuration contains too many layers",
            Self::DuplicateLayer(_) => "configuration contains a duplicate layer category",
        })
    }
}

impl Error for ConfigurationResolutionError {}

/// Resolves ordinary preferences and monotonic policy into one canonical result.
pub fn resolve_configuration(
    layers: &[ConfigurationLayer],
) -> Result<ResolvedConfiguration, ConfigurationResolutionError> {
    if layers.len() > MAX_CONFIGURATION_FILE_LAYERS {
        return Err(ConfigurationResolutionError::TooManyLayers);
    }
    let mut ordered = layers.to_vec();
    ordered.sort_by_key(|layer| layer.kind().precedence());
    for pair in ordered.windows(2) {
        if pair[0].kind() == pair[1].kind() {
            return Err(ConfigurationResolutionError::DuplicateLayer(pair[0].kind()));
        }
    }

    let (profile, profile_supplied_by) = resolve_profile(&ordered);
    let mut preferences = PreferenceAccumulator::profile_defaults(profile);
    let mut policy = PolicyAccumulator::built_in();
    for layer in &ordered {
        preferences.apply(layer.kind(), layer.preferences());
        policy.apply(layer.kind(), layer.policy());
    }
    let preferences = preferences.finish(&policy);
    let policy = policy.finish();
    let digest = canonical_configuration_digest(
        CONFIGURATION_SCHEMA_VERSION,
        CONFIGURATION_RESOLVER_VERSION,
        profile,
        &preferences,
        &policy,
    );
    Ok(ResolvedConfiguration {
        schema_version: CONFIGURATION_SCHEMA_VERSION,
        resolver_version: CONFIGURATION_RESOLVER_VERSION,
        profile,
        profile_supplied_by,
        preferences,
        policy,
        digest,
    })
}

fn resolve_profile(
    layers: &[ConfigurationLayer],
) -> (ConfigurationProfile, ConfigurationLayerKind) {
    let mut selected = ConfigurationProfile::Local;
    let mut supplied_by = ConfigurationLayerKind::BuiltInDefaults;
    for layer in layers {
        if let Some(profile) = layer.profile() {
            selected = profile;
            supplied_by = layer.kind();
        }
    }
    (selected, supplied_by)
}

#[derive(Clone, Copy)]
struct PreferenceCandidate<T> {
    value: T,
    supplied_by: ConfigurationLayerKind,
}

struct PreferenceAccumulator {
    query_results: PreferenceCandidate<u64>,
    context_bytes: PreferenceCandidate<u64>,
    graph_depth: PreferenceCandidate<u64>,
    graph_results: PreferenceCandidate<u64>,
    watcher_poll_interval_ms: PreferenceCandidate<u64>,
    mcp_tool_profile: PreferenceCandidate<McpToolProfile>,
}

impl PreferenceAccumulator {
    fn profile_defaults(_profile: ConfigurationProfile) -> Self {
        let layer = ConfigurationLayerKind::NamedProfile;
        Self {
            query_results: PreferenceCandidate {
                value: u64::from(DEFAULT_CODE_SEARCH_RESULTS),
                supplied_by: layer,
            },
            context_bytes: PreferenceCandidate {
                value: DEFAULT_CONTEXT_BUILD_BUDGET_UNITS,
                supplied_by: layer,
            },
            graph_depth: PreferenceCandidate {
                value: DEFAULT_GRAPH_DEPTH,
                supplied_by: layer,
            },
            graph_results: PreferenceCandidate {
                value: DEFAULT_GRAPH_RESULTS,
                supplied_by: layer,
            },
            watcher_poll_interval_ms: PreferenceCandidate {
                value: DEFAULT_WATCHER_POLL_INTERVAL_MS,
                supplied_by: layer,
            },
            mcp_tool_profile: PreferenceCandidate {
                value: McpToolProfile::Canonical,
                supplied_by: layer,
            },
        }
    }

    fn apply(
        &mut self,
        layer: ConfigurationLayerKind,
        preferences: &ConfigurationPreferenceOverrides,
    ) {
        replace(&mut self.query_results, preferences.query_results(), layer);
        replace(&mut self.context_bytes, preferences.context_bytes(), layer);
        replace(&mut self.graph_depth, preferences.graph_depth(), layer);
        replace(&mut self.graph_results, preferences.graph_results(), layer);
        replace(
            &mut self.watcher_poll_interval_ms,
            preferences.watcher_poll_interval_ms(),
            layer,
        );
        replace(
            &mut self.mcp_tool_profile,
            preferences.mcp_tool_profile(),
            layer,
        );
    }

    fn finish(&self, policy: &PolicyAccumulator) -> EffectiveConfigurationPreferences {
        EffectiveConfigurationPreferences {
            query_results: cap(self.query_results, &policy.max_query_results),
            context_bytes: cap(self.context_bytes, &policy.max_context_bytes),
            graph_depth: cap(self.graph_depth, &policy.max_graph_depth),
            graph_results: cap(self.graph_results, &policy.max_graph_results),
            watcher_poll_interval_ms: uncapped(self.watcher_poll_interval_ms),
            mcp_tool_profile: authorize_tool_profile(
                self.mcp_tool_profile,
                &policy.allowed_mcp_tool_profiles,
            ),
        }
    }
}

fn replace<T: Copy>(
    candidate: &mut PreferenceCandidate<T>,
    value: Option<T>,
    supplied_by: ConfigurationLayerKind,
) {
    if let Some(value) = value {
        *candidate = PreferenceCandidate { value, supplied_by };
    }
}

fn uncapped<T: Copy>(candidate: PreferenceCandidate<T>) -> ResolvedPreference<T> {
    ResolvedPreference::new(
        candidate.value,
        candidate.value,
        candidate.supplied_by,
        Vec::new(),
    )
}

fn cap(
    candidate: PreferenceCandidate<u64>,
    ceiling: &PolicyAccumulatorValue<u64>,
) -> ResolvedPreference<u64> {
    if candidate.value > ceiling.effective {
        ResolvedPreference::new(
            candidate.value,
            ceiling.effective,
            candidate.supplied_by,
            ceiling.constraining_layers.clone(),
        )
    } else {
        uncapped(candidate)
    }
}

fn authorize_tool_profile(
    candidate: PreferenceCandidate<McpToolProfile>,
    allowed: &PolicyAccumulatorValue<BTreeSet<McpToolProfile>>,
) -> ResolvedToolProfilePreference {
    let source_is_authorized = candidate.value == McpToolProfile::Canonical
        || matches!(
            candidate.supplied_by,
            ConfigurationLayerKind::User | ConfigurationLayerKind::Cli
        );
    if source_is_authorized && allowed.effective.contains(&candidate.value) {
        ResolvedToolProfilePreference::new(
            candidate.value,
            candidate.supplied_by,
            Some(candidate.value),
            Vec::new(),
        )
    } else {
        ResolvedToolProfilePreference::new(
            candidate.value,
            candidate.supplied_by,
            None,
            allowed.constraining_layers.clone(),
        )
    }
}

struct PolicyAccumulatorValue<T> {
    effective: T,
    constraining_layers: Vec<ConfigurationLayerKind>,
}

impl<T> PolicyAccumulatorValue<T> {
    fn built_in(effective: T) -> Self {
        Self {
            effective,
            constraining_layers: vec![ConfigurationLayerKind::BuiltInDefaults],
        }
    }

    fn finish(self) -> PolicyValue<T> {
        PolicyValue::new(self.effective, self.constraining_layers)
    }
}

struct PolicyAccumulator {
    allowed_languages: PolicyAccumulatorValue<BTreeSet<SourceLanguage>>,
    allowed_mcp_tool_profiles: PolicyAccumulatorValue<BTreeSet<McpToolProfile>>,
    max_source_file_bytes: PolicyAccumulatorValue<u64>,
    max_source_files: PolicyAccumulatorValue<u64>,
    max_query_results: PolicyAccumulatorValue<u64>,
    max_context_bytes: PolicyAccumulatorValue<u64>,
    max_graph_depth: PolicyAccumulatorValue<u64>,
    max_graph_results: PolicyAccumulatorValue<u64>,
    deny_memory_writes: PolicyAccumulatorValue<bool>,
    follow_symlinks: PolicyAccumulatorValue<bool>,
    retention: RetentionPolicyAccumulator,
}

struct RetentionPolicyAccumulator {
    retained_generations_per_source_slot: PolicyAccumulatorValue<u64>,
    max_generation_candidates: PolicyAccumulatorValue<u64>,
    max_rows: PolicyAccumulatorValue<u64>,
    max_bytes: PolicyAccumulatorValue<u64>,
}

impl PolicyAccumulator {
    fn built_in() -> Self {
        Self {
            allowed_languages: PolicyAccumulatorValue::built_in(
                SUPPORTED_CONFIGURATION_LANGUAGES.into_iter().collect(),
            ),
            allowed_mcp_tool_profiles: PolicyAccumulatorValue::built_in(
                [McpToolProfile::Canonical].into_iter().collect(),
            ),
            max_source_file_bytes: PolicyAccumulatorValue::built_in(
                MAX_CONFIGURATION_SOURCE_FILE_BYTES,
            ),
            max_source_files: PolicyAccumulatorValue::built_in(MAX_CONFIGURATION_SOURCE_FILES),
            max_query_results: PolicyAccumulatorValue::built_in(MAX_CONFIGURATION_QUERY_RESULTS),
            max_context_bytes: PolicyAccumulatorValue::built_in(MAX_CONFIGURATION_CONTEXT_BYTES),
            max_graph_depth: PolicyAccumulatorValue::built_in(MAX_CONFIGURATION_GRAPH_DEPTH),
            max_graph_results: PolicyAccumulatorValue::built_in(MAX_CONFIGURATION_GRAPH_RESULTS),
            deny_memory_writes: PolicyAccumulatorValue {
                effective: false,
                constraining_layers: Vec::new(),
            },
            follow_symlinks: PolicyAccumulatorValue::built_in(false),
            retention: RetentionPolicyAccumulator {
                retained_generations_per_source_slot: PolicyAccumulatorValue::built_in(
                    DEFAULT_CONFIGURATION_RETAINED_GENERATIONS_PER_SOURCE_SLOT,
                ),
                max_generation_candidates: PolicyAccumulatorValue::built_in(
                    DEFAULT_CONFIGURATION_RETENTION_GENERATION_CANDIDATES,
                ),
                max_rows: PolicyAccumulatorValue::built_in(DEFAULT_CONFIGURATION_RETENTION_ROWS),
                max_bytes: PolicyAccumulatorValue::built_in(DEFAULT_CONFIGURATION_RETENTION_BYTES),
            },
        }
    }

    fn apply(&mut self, layer: ConfigurationLayerKind, policy: &ConfigurationPolicyOverrides) {
        if let Some(requested) = policy.allowed_languages() {
            intersect(&mut self.allowed_languages, requested, layer);
        }
        if let Some(requested) = policy.allowed_mcp_tool_profiles() {
            intersect(&mut self.allowed_mcp_tool_profiles, requested, layer);
        }
        minimize(
            &mut self.max_source_file_bytes,
            policy.max_source_file_bytes(),
            layer,
        );
        minimize(&mut self.max_source_files, policy.max_source_files(), layer);
        minimize(
            &mut self.max_query_results,
            policy.max_query_results(),
            layer,
        );
        minimize(
            &mut self.max_context_bytes,
            policy.max_context_bytes(),
            layer,
        );
        minimize(&mut self.max_graph_depth, policy.max_graph_depth(), layer);
        minimize(
            &mut self.max_graph_results,
            policy.max_graph_results(),
            layer,
        );
        if policy.deny_memory_writes() == Some(true) {
            self.deny_memory_writes.effective = true;
            push_unique(&mut self.deny_memory_writes.constraining_layers, layer);
        }
        if policy.follow_symlinks() == Some(false) {
            push_unique(&mut self.follow_symlinks.constraining_layers, layer);
        }
        let retention = policy.retention();
        maximize(
            &mut self.retention.retained_generations_per_source_slot,
            retention.retained_generations_per_source_slot(),
            layer,
        );
        minimize(
            &mut self.retention.max_generation_candidates,
            retention.max_generation_candidates(),
            layer,
        );
        minimize(&mut self.retention.max_rows, retention.max_rows(), layer);
        minimize(&mut self.retention.max_bytes, retention.max_bytes(), layer);
    }

    fn finish(self) -> EffectiveConfigurationPolicy {
        EffectiveConfigurationPolicy {
            allowed_languages: self.allowed_languages.finish(),
            allowed_mcp_tool_profiles: self.allowed_mcp_tool_profiles.finish(),
            max_source_file_bytes: self.max_source_file_bytes.finish(),
            max_source_files: self.max_source_files.finish(),
            max_query_results: self.max_query_results.finish(),
            max_context_bytes: self.max_context_bytes.finish(),
            max_graph_depth: self.max_graph_depth.finish(),
            max_graph_results: self.max_graph_results.finish(),
            deny_memory_writes: self.deny_memory_writes.finish(),
            follow_symlinks: self.follow_symlinks.finish(),
            retention: EffectiveRetentionConfiguration {
                retained_generations_per_source_slot: self
                    .retention
                    .retained_generations_per_source_slot
                    .finish(),
                max_generation_candidates: self.retention.max_generation_candidates.finish(),
                max_rows: self.retention.max_rows.finish(),
                max_bytes: self.retention.max_bytes.finish(),
            },
        }
    }
}

fn maximize(
    value: &mut PolicyAccumulatorValue<u64>,
    requested: Option<u64>,
    layer: ConfigurationLayerKind,
) {
    let Some(requested) = requested else {
        return;
    };
    if requested > value.effective {
        value.effective = requested;
        push_unique(&mut value.constraining_layers, layer);
    } else if requested == value.effective {
        push_unique(&mut value.constraining_layers, layer);
    }
}

fn minimize(
    value: &mut PolicyAccumulatorValue<u64>,
    requested: Option<u64>,
    layer: ConfigurationLayerKind,
) {
    let Some(requested) = requested else {
        return;
    };
    if requested < value.effective {
        value.effective = requested;
        push_unique(&mut value.constraining_layers, layer);
    } else if requested == value.effective {
        push_unique(&mut value.constraining_layers, layer);
    }
}

fn intersect<T: Copy + Ord>(
    value: &mut PolicyAccumulatorValue<BTreeSet<T>>,
    requested: &BTreeSet<T>,
    layer: ConfigurationLayerKind,
) {
    let intersection = value
        .effective
        .intersection(requested)
        .copied()
        .collect::<BTreeSet<_>>();
    if intersection != value.effective || requested == &value.effective {
        value.effective = intersection;
        push_unique(&mut value.constraining_layers, layer);
    }
}

fn push_unique(layers: &mut Vec<ConfigurationLayerKind>, layer: ConfigurationLayerKind) {
    if !layers.contains(&layer) {
        layers.push(layer);
    }
}
