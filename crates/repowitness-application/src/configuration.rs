//! Versioned local configuration values and deterministic policy resolution.

mod digest;
mod model;
mod resolve;
mod resolved;
mod retention;

pub use model::{
    CONFIGURATION_SCHEMA_VERSION, ConfigurationField, ConfigurationLayer, ConfigurationLayerKind,
    ConfigurationPolicyOverrides, ConfigurationPreferenceOverrides, ConfigurationProfile,
    ConfigurationValidationError, MAX_CONFIGURATION_CONTEXT_BYTES, MAX_CONFIGURATION_FILE_LAYERS,
    MAX_CONFIGURATION_GRAPH_DEPTH, MAX_CONFIGURATION_GRAPH_RESULTS,
    MAX_CONFIGURATION_QUERY_RESULTS, MAX_CONFIGURATION_SOURCE_FILE_BYTES,
    MAX_CONFIGURATION_SOURCE_FILES, MAX_CONFIGURATION_WATCHER_POLL_INTERVAL_MS,
    MIN_CONFIGURATION_WATCHER_POLL_INTERVAL_MS, McpToolProfile,
};
pub use resolve::{
    CONFIGURATION_RESOLVER_VERSION, ConfigurationResolutionError, resolve_configuration,
};
pub use resolved::{
    CONFIGURATION_DIGEST_VERSION, EffectiveConfigurationPolicy, EffectiveConfigurationPreferences,
    EffectiveRetentionConfiguration, PolicyValue, ResolvedConfiguration, ResolvedPreference,
    ResolvedToolProfilePreference,
};
pub use retention::{
    DEFAULT_CONFIGURATION_RETAINED_GENERATIONS_PER_SOURCE_SLOT,
    DEFAULT_CONFIGURATION_RETENTION_BYTES, DEFAULT_CONFIGURATION_RETENTION_GENERATION_CANDIDATES,
    DEFAULT_CONFIGURATION_RETENTION_ROWS, MAX_CONFIGURATION_RETAINED_GENERATIONS_PER_SOURCE_SLOT,
    MAX_CONFIGURATION_RETENTION_BYTES, MAX_CONFIGURATION_RETENTION_GENERATION_CANDIDATES,
    MAX_CONFIGURATION_RETENTION_ROWS, RetentionConfigurationOverrides,
};

#[cfg(test)]
mod tests;
