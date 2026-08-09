use std::{collections::BTreeSet, error::Error, fmt};

use super::retention::RetentionConfigurationOverrides;
use crate::{
    MAX_EVIDENCE_CONTEXT_BUDGET_UNITS, SourceLanguage, code_search::MAX_CODE_SEARCH_RESULTS,
    rust_index::MAX_RUST_INDEX_FILES,
};

pub(super) const SUPPORTED_CONFIGURATION_LANGUAGES: [SourceLanguage; 5] = [
    SourceLanguage::Rust,
    SourceLanguage::Go,
    SourceLanguage::TypeScript,
    SourceLanguage::Tsx,
    SourceLanguage::Python,
];

/// Supported local configuration schema version.
pub const CONFIGURATION_SCHEMA_VERSION: u16 = 1;
/// Maximum number of caller-supplied configuration layers.
pub const MAX_CONFIGURATION_FILE_LAYERS: usize = 5;
/// Absolute query-result ceiling accepted by configuration version 1.
pub const MAX_CONFIGURATION_QUERY_RESULTS: u64 = MAX_CODE_SEARCH_RESULTS as u64;
/// Absolute context-content ceiling accepted by configuration version 1.
pub const MAX_CONFIGURATION_CONTEXT_BYTES: u64 = MAX_EVIDENCE_CONTEXT_BUDGET_UNITS;
/// Absolute graph traversal depth accepted by configuration version 1.
pub const MAX_CONFIGURATION_GRAPH_DEPTH: u64 = 64;
/// Absolute graph-result ceiling accepted by configuration version 1.
pub const MAX_CONFIGURATION_GRAPH_RESULTS: u64 = 10_000;
/// Absolute per-source-file byte ceiling accepted by configuration version 1.
pub const MAX_CONFIGURATION_SOURCE_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// Absolute source-file-count ceiling accepted by configuration version 1.
pub const MAX_CONFIGURATION_SOURCE_FILES: u64 = MAX_RUST_INDEX_FILES;
/// Minimum watcher reconciliation polling interval in milliseconds.
pub const MIN_CONFIGURATION_WATCHER_POLL_INTERVAL_MS: u64 = 100;
/// Maximum watcher reconciliation polling interval in milliseconds.
pub const MAX_CONFIGURATION_WATCHER_POLL_INTERVAL_MS: u64 = 24 * 60 * 60 * 1_000;

/// Built-in named configuration profile.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ConfigurationProfile {
    /// Local single-process operation with conservative defaults.
    Local,
}

impl ConfigurationProfile {
    /// Returns the stable configuration spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
        }
    }
}

/// Fixed MCP tool surface selected before server initialization.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum McpToolProfile {
    /// The compact canonical RepoWitness tool surface.
    Canonical,
    /// The smallest read-only discovery surface.
    Minimal,
    /// A separately tested bounded incumbent-compatibility surface.
    IncumbentCompatible,
}

impl McpToolProfile {
    /// Returns the stable configuration spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::Minimal => "minimal",
            Self::IncumbentCompatible => "incumbent-compatible",
        }
    }
}

/// Stable provenance category for one configuration value or constraint.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ConfigurationLayerKind {
    /// Compiled safety bounds and defaults.
    BuiltInDefaults,
    /// Defaults supplied by the selected built-in profile.
    NamedProfile,
    /// User-owned local configuration.
    User,
    /// Workspace-owned configuration.
    Workspace,
    /// Repository-owned configuration.
    Repository,
    /// Explicitly admitted environment-derived values.
    Environment,
    /// Explicit command-line values.
    Cli,
}

impl ConfigurationLayerKind {
    pub(super) const fn precedence(self) -> u8 {
        match self {
            Self::BuiltInDefaults => 0,
            Self::NamedProfile => 1,
            Self::User => 2,
            Self::Workspace => 3,
            Self::Repository => 4,
            Self::Environment => 5,
            Self::Cli => 6,
        }
    }

    const fn is_caller_layer(self) -> bool {
        !matches!(self, Self::BuiltInDefaults | Self::NamedProfile)
    }

    const fn can_select_profile(self) -> bool {
        matches!(self, Self::User | Self::Cli)
    }
}

/// Stable field identity for a configuration validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationField {
    /// Default query result count.
    QueryResults,
    /// Default context-content byte budget.
    ContextBytes,
    /// Default graph traversal depth.
    GraphDepth,
    /// Default graph result count.
    GraphResults,
    /// Watcher reconciliation polling interval in milliseconds.
    WatcherPollIntervalMilliseconds,
    /// Maximum source-file bytes.
    SourceFileBytes,
    /// Maximum source-file count.
    SourceFiles,
    /// Minimum newest generations retained for every source slot.
    RetainedGenerationsPerSourceSlot,
    /// Maximum generation candidates admitted by one retention pass.
    RetentionGenerationCandidates,
    /// Maximum estimated rows admitted by one retention pass.
    RetentionRows,
    /// Maximum estimated bytes admitted by one retention pass.
    RetentionBytes,
}

impl ConfigurationField {
    const fn as_str(self) -> &'static str {
        match self {
            Self::QueryResults => "query results",
            Self::ContextBytes => "context bytes",
            Self::GraphDepth => "graph depth",
            Self::GraphResults => "graph results",
            Self::WatcherPollIntervalMilliseconds => "watcher polling interval milliseconds",
            Self::SourceFileBytes => "source-file bytes",
            Self::SourceFiles => "source-file count",
            Self::RetainedGenerationsPerSourceSlot => "retained generations per source slot",
            Self::RetentionGenerationCandidates => "retention generation candidates",
            Self::RetentionRows => "retention rows",
            Self::RetentionBytes => "retention bytes",
        }
    }
}

/// Stable content-redacted validation failure for one configuration layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationValidationError {
    /// A caller attempted to provide an internally synthesized layer.
    InternalLayer,
    /// A layer outside user or CLI scope attempted to select the profile.
    ProfileSelectionNotAllowed,
    /// A positive numeric field was zero.
    Zero(ConfigurationField),
    /// A numeric field exceeded its absolute schema ceiling.
    AboveMaximum(ConfigurationField),
    /// The watcher interval was below its absolute schema minimum.
    BelowMinimum(ConfigurationField),
    /// The allowed-language set exceeded the built-in language universe.
    TooManyLanguages,
    /// The allowed MCP tool-profile set exceeded the built-in profile universe.
    TooManyToolProfiles,
    /// Version 1 never follows source symlinks.
    FollowSymlinksUnsupported,
}

impl fmt::Display for ConfigurationValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InternalLayer => {
                formatter.write_str("configuration layer is reserved for internal defaults")
            }
            Self::ProfileSelectionNotAllowed => formatter
                .write_str("configuration profile selection is not allowed from this layer"),
            Self::Zero(field) => write!(
                formatter,
                "configuration {} must be positive",
                field.as_str()
            ),
            Self::AboveMaximum(field) => {
                write!(
                    formatter,
                    "configuration {} exceeds its maximum",
                    field.as_str()
                )
            }
            Self::BelowMinimum(field) => {
                write!(
                    formatter,
                    "configuration {} is below its minimum",
                    field.as_str()
                )
            }
            Self::TooManyLanguages => {
                formatter.write_str("configuration allowed-language set is too large")
            }
            Self::TooManyToolProfiles => {
                formatter.write_str("configuration allowed-tool-profile set is too large")
            }
            Self::FollowSymlinksUnsupported => {
                formatter.write_str("configuration version 1 does not follow source symlinks")
            }
        }
    }
}

impl Error for ConfigurationValidationError {}

/// Validated ordinary preference overrides from one provenance layer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConfigurationPreferenceOverrides {
    query_results: Option<u64>,
    context_bytes: Option<u64>,
    graph_depth: Option<u64>,
    graph_results: Option<u64>,
    watcher_poll_interval_ms: Option<u64>,
    mcp_tool_profile: Option<McpToolProfile>,
}

impl ConfigurationPreferenceOverrides {
    /// Validates all supplied ordinary preference values.
    pub fn try_new(
        query_results: Option<u64>,
        context_bytes: Option<u64>,
        graph_depth: Option<u64>,
        graph_results: Option<u64>,
        watcher_poll_interval_ms: Option<u64>,
        mcp_tool_profile: Option<McpToolProfile>,
    ) -> Result<Self, ConfigurationValidationError> {
        validate_positive_maximum(
            query_results,
            MAX_CONFIGURATION_QUERY_RESULTS,
            ConfigurationField::QueryResults,
        )?;
        validate_positive_maximum(
            context_bytes,
            MAX_CONFIGURATION_CONTEXT_BYTES,
            ConfigurationField::ContextBytes,
        )?;
        validate_positive_maximum(
            graph_depth,
            MAX_CONFIGURATION_GRAPH_DEPTH,
            ConfigurationField::GraphDepth,
        )?;
        validate_positive_maximum(
            graph_results,
            MAX_CONFIGURATION_GRAPH_RESULTS,
            ConfigurationField::GraphResults,
        )?;
        if let Some(value) = watcher_poll_interval_ms {
            if value < MIN_CONFIGURATION_WATCHER_POLL_INTERVAL_MS {
                return Err(ConfigurationValidationError::BelowMinimum(
                    ConfigurationField::WatcherPollIntervalMilliseconds,
                ));
            }
            if value > MAX_CONFIGURATION_WATCHER_POLL_INTERVAL_MS {
                return Err(ConfigurationValidationError::AboveMaximum(
                    ConfigurationField::WatcherPollIntervalMilliseconds,
                ));
            }
        }
        Ok(Self {
            query_results,
            context_bytes,
            graph_depth,
            graph_results,
            watcher_poll_interval_ms,
            mcp_tool_profile,
        })
    }

    pub(super) const fn query_results(&self) -> Option<u64> {
        self.query_results
    }

    pub(super) const fn context_bytes(&self) -> Option<u64> {
        self.context_bytes
    }

    pub(super) const fn graph_depth(&self) -> Option<u64> {
        self.graph_depth
    }

    pub(super) const fn graph_results(&self) -> Option<u64> {
        self.graph_results
    }

    pub(super) const fn watcher_poll_interval_ms(&self) -> Option<u64> {
        self.watcher_poll_interval_ms
    }

    pub(super) const fn mcp_tool_profile(&self) -> Option<McpToolProfile> {
        self.mcp_tool_profile
    }
}

/// Validated monotonic policy requests from one provenance layer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConfigurationPolicyOverrides {
    allowed_languages: Option<BTreeSet<SourceLanguage>>,
    allowed_mcp_tool_profiles: Option<BTreeSet<McpToolProfile>>,
    max_source_file_bytes: Option<u64>,
    max_source_files: Option<u64>,
    max_query_results: Option<u64>,
    max_context_bytes: Option<u64>,
    max_graph_depth: Option<u64>,
    max_graph_results: Option<u64>,
    deny_memory_writes: Option<bool>,
    follow_symlinks: Option<bool>,
    retention: RetentionConfigurationOverrides,
}

impl ConfigurationPolicyOverrides {
    /// Validates all supplied monotonic policy requests.
    #[allow(
        clippy::too_many_arguments,
        reason = "the fixed version-1 policy schema is clearer as explicit typed fields"
    )]
    pub fn try_new(
        allowed_languages: Option<BTreeSet<SourceLanguage>>,
        allowed_mcp_tool_profiles: Option<BTreeSet<McpToolProfile>>,
        max_source_file_bytes: Option<u64>,
        max_source_files: Option<u64>,
        max_query_results: Option<u64>,
        max_context_bytes: Option<u64>,
        max_graph_depth: Option<u64>,
        max_graph_results: Option<u64>,
        deny_memory_writes: Option<bool>,
        follow_symlinks: Option<bool>,
    ) -> Result<Self, ConfigurationValidationError> {
        if allowed_languages
            .as_ref()
            .is_some_and(|languages| languages.len() > SUPPORTED_CONFIGURATION_LANGUAGES.len())
        {
            return Err(ConfigurationValidationError::TooManyLanguages);
        }
        if allowed_mcp_tool_profiles
            .as_ref()
            .is_some_and(|profiles| profiles.len() > 3)
        {
            return Err(ConfigurationValidationError::TooManyToolProfiles);
        }
        validate_positive_maximum(
            max_source_file_bytes,
            MAX_CONFIGURATION_SOURCE_FILE_BYTES,
            ConfigurationField::SourceFileBytes,
        )?;
        validate_positive_maximum(
            max_source_files,
            MAX_CONFIGURATION_SOURCE_FILES,
            ConfigurationField::SourceFiles,
        )?;
        validate_positive_maximum(
            max_query_results,
            MAX_CONFIGURATION_QUERY_RESULTS,
            ConfigurationField::QueryResults,
        )?;
        validate_positive_maximum(
            max_context_bytes,
            MAX_CONFIGURATION_CONTEXT_BYTES,
            ConfigurationField::ContextBytes,
        )?;
        validate_positive_maximum(
            max_graph_depth,
            MAX_CONFIGURATION_GRAPH_DEPTH,
            ConfigurationField::GraphDepth,
        )?;
        validate_positive_maximum(
            max_graph_results,
            MAX_CONFIGURATION_GRAPH_RESULTS,
            ConfigurationField::GraphResults,
        )?;
        if follow_symlinks == Some(true) {
            return Err(ConfigurationValidationError::FollowSymlinksUnsupported);
        }
        Ok(Self {
            allowed_languages,
            allowed_mcp_tool_profiles,
            max_source_file_bytes,
            max_source_files,
            max_query_results,
            max_context_bytes,
            max_graph_depth,
            max_graph_results,
            deny_memory_writes,
            follow_symlinks,
            retention: RetentionConfigurationOverrides::default(),
        })
    }

    /// Adds an independently validated monotonic retention-policy request.
    #[must_use]
    pub fn with_retention(mut self, retention: RetentionConfigurationOverrides) -> Self {
        self.retention = retention;
        self
    }

    pub(super) fn allowed_languages(&self) -> Option<&BTreeSet<SourceLanguage>> {
        self.allowed_languages.as_ref()
    }

    pub(super) fn allowed_mcp_tool_profiles(&self) -> Option<&BTreeSet<McpToolProfile>> {
        self.allowed_mcp_tool_profiles.as_ref()
    }

    pub(super) const fn max_source_file_bytes(&self) -> Option<u64> {
        self.max_source_file_bytes
    }

    pub(super) const fn max_source_files(&self) -> Option<u64> {
        self.max_source_files
    }

    pub(super) const fn max_query_results(&self) -> Option<u64> {
        self.max_query_results
    }

    pub(super) const fn max_context_bytes(&self) -> Option<u64> {
        self.max_context_bytes
    }

    pub(super) const fn max_graph_depth(&self) -> Option<u64> {
        self.max_graph_depth
    }

    pub(super) const fn max_graph_results(&self) -> Option<u64> {
        self.max_graph_results
    }

    pub(super) const fn deny_memory_writes(&self) -> Option<bool> {
        self.deny_memory_writes
    }

    pub(super) const fn follow_symlinks(&self) -> Option<bool> {
        self.follow_symlinks
    }

    pub(super) const fn retention(&self) -> RetentionConfigurationOverrides {
        self.retention
    }
}

/// One validated caller-supplied configuration layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigurationLayer {
    kind: ConfigurationLayerKind,
    profile: Option<ConfigurationProfile>,
    preferences: ConfigurationPreferenceOverrides,
    policy: ConfigurationPolicyOverrides,
}

impl ConfigurationLayer {
    /// Constructs a layer after enforcing profile-selection trust.
    pub fn try_new(
        kind: ConfigurationLayerKind,
        profile: Option<ConfigurationProfile>,
        preferences: ConfigurationPreferenceOverrides,
        policy: ConfigurationPolicyOverrides,
    ) -> Result<Self, ConfigurationValidationError> {
        if !kind.is_caller_layer() {
            return Err(ConfigurationValidationError::InternalLayer);
        }
        if profile.is_some() && !kind.can_select_profile() {
            return Err(ConfigurationValidationError::ProfileSelectionNotAllowed);
        }
        Ok(Self {
            kind,
            profile,
            preferences,
            policy,
        })
    }

    /// Returns this layer's path-free provenance category.
    #[must_use]
    pub const fn kind(&self) -> ConfigurationLayerKind {
        self.kind
    }

    /// Returns the optional built-in profile selection.
    #[must_use]
    pub const fn profile(&self) -> Option<ConfigurationProfile> {
        self.profile
    }

    /// Returns the validated ordinary preference overrides.
    #[must_use]
    pub const fn preferences(&self) -> &ConfigurationPreferenceOverrides {
        &self.preferences
    }

    /// Returns the validated monotonic policy requests.
    #[must_use]
    pub const fn policy(&self) -> &ConfigurationPolicyOverrides {
        &self.policy
    }
}

const fn validate_positive_maximum(
    value: Option<u64>,
    maximum: u64,
    field: ConfigurationField,
) -> Result<(), ConfigurationValidationError> {
    match value {
        Some(0) => Err(ConfigurationValidationError::Zero(field)),
        Some(value) if value > maximum => Err(ConfigurationValidationError::AboveMaximum(field)),
        Some(_) | None => Ok(()),
    }
}
