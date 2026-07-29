use std::collections::BTreeSet;

use repowitness_domain::ConfigurationDigest;

use super::{ConfigurationLayerKind, ConfigurationProfile, McpToolProfile};
use crate::SourceLanguage;

/// Canonical semantic configuration encoding version.
pub const CONFIGURATION_DIGEST_VERSION: u16 = 1;

/// One resolved ordinary preference with supplier and optional policy cap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedPreference<T> {
    requested: T,
    effective: T,
    supplied_by: ConfigurationLayerKind,
    constrained_by: Vec<ConfigurationLayerKind>,
}

impl<T> ResolvedPreference<T> {
    pub(super) fn new(
        requested: T,
        effective: T,
        supplied_by: ConfigurationLayerKind,
        constrained_by: Vec<ConfigurationLayerKind>,
    ) -> Self {
        Self {
            requested,
            effective,
            supplied_by,
            constrained_by,
        }
    }

    /// Returns the winning ordinary-precedence request.
    #[must_use]
    pub const fn requested(&self) -> &T {
        &self.requested
    }

    /// Returns the value after monotonic policy constraints.
    #[must_use]
    pub const fn effective(&self) -> &T {
        &self.effective
    }

    /// Returns the layer that supplied the winning ordinary request.
    #[must_use]
    pub const fn supplied_by(&self) -> ConfigurationLayerKind {
        self.supplied_by
    }

    /// Returns every policy layer that prevented the request from becoming effective.
    #[must_use]
    pub fn constrained_by(&self) -> &[ConfigurationLayerKind] {
        &self.constrained_by
    }
}

/// One effective monotonic policy value and all binding provenance layers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyValue<T> {
    effective: T,
    constraining_layers: Vec<ConfigurationLayerKind>,
}

/// Ordinary MCP profile request plus its monotonic startup authorization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedToolProfilePreference {
    requested: McpToolProfile,
    supplied_by: ConfigurationLayerKind,
    authorized: Option<McpToolProfile>,
    constrained_by: Vec<ConfigurationLayerKind>,
}

impl ResolvedToolProfilePreference {
    pub(super) fn new(
        requested: McpToolProfile,
        supplied_by: ConfigurationLayerKind,
        authorized: Option<McpToolProfile>,
        constrained_by: Vec<ConfigurationLayerKind>,
    ) -> Self {
        Self {
            requested,
            supplied_by,
            authorized,
            constrained_by,
        }
    }

    /// Returns the winning ordinary-precedence request.
    #[must_use]
    pub const fn requested(&self) -> McpToolProfile {
        self.requested
    }

    /// Returns the requested profile only when monotonic policy authorizes it.
    #[must_use]
    pub const fn authorized(&self) -> Option<McpToolProfile> {
        self.authorized
    }

    /// Returns the ordinary layer that requested the profile.
    #[must_use]
    pub const fn supplied_by(&self) -> ConfigurationLayerKind {
        self.supplied_by
    }

    /// Returns policy layers that prevent startup with the requested profile.
    #[must_use]
    pub fn constrained_by(&self) -> &[ConfigurationLayerKind] {
        &self.constrained_by
    }
}

impl<T> PolicyValue<T> {
    pub(super) fn new(effective: T, constraining_layers: Vec<ConfigurationLayerKind>) -> Self {
        Self {
            effective,
            constraining_layers,
        }
    }

    /// Returns the effective policy value.
    #[must_use]
    pub const fn effective(&self) -> &T {
        &self.effective
    }

    /// Returns the ordered path-free categories that bind the effective value.
    #[must_use]
    pub fn constraining_layers(&self) -> &[ConfigurationLayerKind] {
        &self.constraining_layers
    }
}

/// Complete effective ordinary-preference set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveConfigurationPreferences {
    pub(super) query_results: ResolvedPreference<u64>,
    pub(super) context_bytes: ResolvedPreference<u64>,
    pub(super) graph_depth: ResolvedPreference<u64>,
    pub(super) graph_results: ResolvedPreference<u64>,
    pub(super) watcher_poll_interval_ms: ResolvedPreference<u64>,
    pub(super) mcp_tool_profile: ResolvedToolProfilePreference,
}

impl EffectiveConfigurationPreferences {
    /// Returns the default bounded query result count.
    #[must_use]
    pub const fn query_results(&self) -> &ResolvedPreference<u64> {
        &self.query_results
    }

    /// Returns the default bounded context-content budget in bytes.
    #[must_use]
    pub const fn context_bytes(&self) -> &ResolvedPreference<u64> {
        &self.context_bytes
    }

    /// Returns the default bounded graph traversal depth.
    #[must_use]
    pub const fn graph_depth(&self) -> &ResolvedPreference<u64> {
        &self.graph_depth
    }

    /// Returns the default bounded graph result count.
    #[must_use]
    pub const fn graph_results(&self) -> &ResolvedPreference<u64> {
        &self.graph_results
    }

    /// Returns the watcher reconciliation polling interval in milliseconds.
    #[must_use]
    pub const fn watcher_poll_interval_ms(&self) -> &ResolvedPreference<u64> {
        &self.watcher_poll_interval_ms
    }

    /// Returns the fixed MCP tool profile.
    #[must_use]
    pub const fn mcp_tool_profile(&self) -> &ResolvedToolProfilePreference {
        &self.mcp_tool_profile
    }
}

/// Complete effective monotonic generation-retention policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveRetentionConfiguration {
    pub(super) retained_generations_per_source_slot: PolicyValue<u64>,
    pub(super) max_generation_candidates: PolicyValue<u64>,
    pub(super) max_rows: PolicyValue<u64>,
    pub(super) max_bytes: PolicyValue<u64>,
}

impl EffectiveRetentionConfiguration {
    /// Returns the minimum newest generations retained for every source slot.
    #[must_use]
    pub const fn retained_generations_per_source_slot(&self) -> &PolicyValue<u64> {
        &self.retained_generations_per_source_slot
    }

    /// Returns the maximum generation candidates admitted by one retention pass.
    #[must_use]
    pub const fn max_generation_candidates(&self) -> &PolicyValue<u64> {
        &self.max_generation_candidates
    }

    /// Returns the maximum estimated rows admitted by one retention pass.
    #[must_use]
    pub const fn max_rows(&self) -> &PolicyValue<u64> {
        &self.max_rows
    }

    /// Returns the maximum estimated bytes admitted by one retention pass.
    #[must_use]
    pub const fn max_bytes(&self) -> &PolicyValue<u64> {
        &self.max_bytes
    }
}

/// Complete effective monotonic policy set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveConfigurationPolicy {
    pub(super) allowed_languages: PolicyValue<BTreeSet<SourceLanguage>>,
    pub(super) allowed_mcp_tool_profiles: PolicyValue<BTreeSet<McpToolProfile>>,
    pub(super) max_source_file_bytes: PolicyValue<u64>,
    pub(super) max_source_files: PolicyValue<u64>,
    pub(super) max_query_results: PolicyValue<u64>,
    pub(super) max_context_bytes: PolicyValue<u64>,
    pub(super) max_graph_depth: PolicyValue<u64>,
    pub(super) max_graph_results: PolicyValue<u64>,
    pub(super) deny_memory_writes: PolicyValue<bool>,
    pub(super) follow_symlinks: PolicyValue<bool>,
    pub(super) retention: EffectiveRetentionConfiguration,
}

impl EffectiveConfigurationPolicy {
    /// Returns the intersected allowed source-language set.
    #[must_use]
    pub const fn allowed_languages(&self) -> &PolicyValue<BTreeSet<SourceLanguage>> {
        &self.allowed_languages
    }

    /// Returns the intersected set of startup-authorized MCP tool profiles.
    #[must_use]
    pub const fn allowed_mcp_tool_profiles(&self) -> &PolicyValue<BTreeSet<McpToolProfile>> {
        &self.allowed_mcp_tool_profiles
    }

    /// Returns the maximum admitted bytes for one source file.
    #[must_use]
    pub const fn max_source_file_bytes(&self) -> &PolicyValue<u64> {
        &self.max_source_file_bytes
    }

    /// Returns the maximum admitted source-file count.
    #[must_use]
    pub const fn max_source_files(&self) -> &PolicyValue<u64> {
        &self.max_source_files
    }

    /// Returns the maximum query result count.
    #[must_use]
    pub const fn max_query_results(&self) -> &PolicyValue<u64> {
        &self.max_query_results
    }

    /// Returns the maximum context-content budget in bytes.
    #[must_use]
    pub const fn max_context_bytes(&self) -> &PolicyValue<u64> {
        &self.max_context_bytes
    }

    /// Returns the maximum graph traversal depth.
    #[must_use]
    pub const fn max_graph_depth(&self) -> &PolicyValue<u64> {
        &self.max_graph_depth
    }

    /// Returns the maximum graph result count.
    #[must_use]
    pub const fn max_graph_results(&self) -> &PolicyValue<u64> {
        &self.max_graph_results
    }

    /// Returns whether all memory writes are denied by configuration policy.
    #[must_use]
    pub const fn deny_memory_writes(&self) -> &PolicyValue<bool> {
        &self.deny_memory_writes
    }

    /// Returns whether source traversal may follow symlinks.
    #[must_use]
    pub const fn follow_symlinks(&self) -> &PolicyValue<bool> {
        &self.follow_symlinks
    }

    /// Returns the complete effective generation-retention policy.
    #[must_use]
    pub const fn retention(&self) -> &EffectiveRetentionConfiguration {
        &self.retention
    }
}

/// Fully resolved path-free semantic configuration and provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedConfiguration {
    pub(super) schema_version: u16,
    pub(super) resolver_version: u16,
    pub(super) profile: ConfigurationProfile,
    pub(super) profile_supplied_by: ConfigurationLayerKind,
    pub(super) preferences: EffectiveConfigurationPreferences,
    pub(super) policy: EffectiveConfigurationPolicy,
    pub(super) digest: ConfigurationDigest,
}

impl ResolvedConfiguration {
    /// Returns the admitted file schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the deterministic resolver version.
    #[must_use]
    pub const fn resolver_version(&self) -> u16 {
        self.resolver_version
    }

    /// Returns the selected built-in profile.
    #[must_use]
    pub const fn profile(&self) -> ConfigurationProfile {
        self.profile
    }

    /// Returns the layer that selected the profile.
    #[must_use]
    pub const fn profile_supplied_by(&self) -> ConfigurationLayerKind {
        self.profile_supplied_by
    }

    /// Returns the complete effective ordinary preferences.
    #[must_use]
    pub const fn preferences(&self) -> &EffectiveConfigurationPreferences {
        &self.preferences
    }

    /// Returns the complete effective monotonic policy.
    #[must_use]
    pub const fn policy(&self) -> &EffectiveConfigurationPolicy {
        &self.policy
    }

    /// Returns the canonical SHA-256 identity of effective semantics.
    #[must_use]
    pub const fn digest(&self) -> ConfigurationDigest {
        self.digest
    }
}
