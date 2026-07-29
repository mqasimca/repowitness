use std::{collections::BTreeSet, error::Error, fmt, str};

use repowitness_application::{
    CONFIGURATION_SCHEMA_VERSION, ConfigurationLayer, ConfigurationLayerKind,
    ConfigurationPolicyOverrides, ConfigurationPreferenceOverrides, ConfigurationProfile,
    ConfigurationValidationError, McpToolProfile, RetentionConfigurationOverrides, SourceLanguage,
};

use super::dto::{ConfigurationFileDto, PolicyDto, PreferenceDto};

/// Inclusive byte ceiling for one configuration file.
pub const MAX_CONFIGURATION_FILE_BYTES: usize = 64 * 1024;
/// Inclusive byte ceiling for every scalar text value.
pub const MAX_CONFIGURATION_TEXT_BYTES: usize = 32;
const MAX_CONFIGURATION_LANGUAGES: usize = 5;
const MAX_CONFIGURATION_TOOL_PROFILES: usize = 3;

/// Host-path-free provenance category accepted by the TOML file parser.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationFileLayer {
    /// User-owned local configuration.
    User,
    /// Workspace-owned configuration.
    Workspace,
    /// Repository-owned configuration.
    Repository,
}

impl ConfigurationFileLayer {
    const fn application_kind(self) -> ConfigurationLayerKind {
        match self {
            Self::User => ConfigurationLayerKind::User,
            Self::Workspace => ConfigurationLayerKind::Workspace,
            Self::Repository => ConfigurationLayerKind::Repository,
        }
    }
}

/// Stable content-redacted failure to admit one TOML configuration file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationFileError {
    /// The file exceeded its inclusive byte ceiling before parsing.
    FileTooLarge,
    /// The file was not valid UTF-8.
    InvalidUtf8,
    /// TOML syntax, types, duplicate keys, or unknown fields were invalid.
    InvalidToml,
    /// The required schema version was unsupported or not representable.
    UnsupportedSchemaVersion,
    /// A scalar, collection, or enum value was invalid.
    InvalidValue,
    /// Application validation rejected the otherwise decoded layer.
    Validation(ConfigurationValidationError),
}

impl fmt::Display for ConfigurationFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::FileTooLarge => "configuration file exceeds the byte limit",
            Self::InvalidUtf8 => "configuration file is not valid UTF-8",
            Self::InvalidToml => "configuration file syntax or schema is invalid",
            Self::UnsupportedSchemaVersion => "configuration schema version is unsupported",
            Self::InvalidValue => "configuration file contains an invalid value",
            Self::Validation(_) => "configuration values violate the versioned policy contract",
        })
    }
}

impl Error for ConfigurationFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Validation(source) => Some(source),
            Self::FileTooLarge
            | Self::InvalidUtf8
            | Self::InvalidToml
            | Self::UnsupportedSchemaVersion
            | Self::InvalidValue => None,
        }
    }
}

/// Decodes and validates one bounded strict `repowitness.toml` version-1 layer.
pub fn parse_configuration_file(
    bytes: &[u8],
    layer: ConfigurationFileLayer,
) -> Result<ConfigurationLayer, ConfigurationFileError> {
    if bytes.len() > MAX_CONFIGURATION_FILE_BYTES {
        return Err(ConfigurationFileError::FileTooLarge);
    }
    let text = str::from_utf8(bytes).map_err(|_| ConfigurationFileError::InvalidUtf8)?;
    let dto = toml::from_str::<ConfigurationFileDto>(text)
        .map_err(|_| ConfigurationFileError::InvalidToml)?;
    if dto.schema_version != u64::from(CONFIGURATION_SCHEMA_VERSION) {
        return Err(ConfigurationFileError::UnsupportedSchemaVersion);
    }
    let profile = dto.profile.map(parse_profile).transpose()?;
    let preferences = parse_preferences(dto.preferences.unwrap_or_default())?;
    let policy = parse_policy(dto.policy.unwrap_or_default())?;
    ConfigurationLayer::try_new(layer.application_kind(), profile, preferences, policy)
        .map_err(ConfigurationFileError::Validation)
}

fn parse_profile(value: String) -> Result<ConfigurationProfile, ConfigurationFileError> {
    validate_text(&value)?;
    match value.as_str() {
        "local" => Ok(ConfigurationProfile::Local),
        _ => Err(ConfigurationFileError::InvalidValue),
    }
}

fn parse_preferences(
    dto: PreferenceDto,
) -> Result<ConfigurationPreferenceOverrides, ConfigurationFileError> {
    let tool_profile = dto.mcp_tool_profile.map(parse_tool_profile).transpose()?;
    ConfigurationPreferenceOverrides::try_new(
        dto.query_results,
        dto.context_bytes,
        dto.graph_depth,
        dto.graph_results,
        dto.watcher_poll_interval_ms,
        tool_profile,
    )
    .map_err(ConfigurationFileError::Validation)
}

fn parse_tool_profile(value: String) -> Result<McpToolProfile, ConfigurationFileError> {
    validate_text(&value)?;
    match value.as_str() {
        "canonical" => Ok(McpToolProfile::Canonical),
        "minimal" => Ok(McpToolProfile::Minimal),
        "incumbent-compatible" => Ok(McpToolProfile::IncumbentCompatible),
        _ => Err(ConfigurationFileError::InvalidValue),
    }
}

fn parse_policy(dto: PolicyDto) -> Result<ConfigurationPolicyOverrides, ConfigurationFileError> {
    let allowed_languages = dto.allowed_languages.map(parse_languages).transpose()?;
    let allowed_mcp_tool_profiles = dto
        .allowed_mcp_tool_profiles
        .map(parse_tool_profiles)
        .transpose()?;
    let retention = RetentionConfigurationOverrides::try_new(
        dto.retained_generations_per_source_slot,
        dto.max_retention_generation_candidates,
        dto.max_retention_rows,
        dto.max_retention_bytes,
    )
    .map_err(ConfigurationFileError::Validation)?;
    ConfigurationPolicyOverrides::try_new(
        allowed_languages,
        allowed_mcp_tool_profiles,
        dto.max_source_file_bytes,
        dto.max_source_files,
        dto.max_query_results,
        dto.max_context_bytes,
        dto.max_graph_depth,
        dto.max_graph_results,
        dto.deny_memory_writes,
        dto.follow_symlinks,
    )
    .map(|policy| policy.with_retention(retention))
    .map_err(ConfigurationFileError::Validation)
}

fn parse_languages(
    values: Vec<String>,
) -> Result<BTreeSet<SourceLanguage>, ConfigurationFileError> {
    if values.len() > MAX_CONFIGURATION_LANGUAGES {
        return Err(ConfigurationFileError::InvalidValue);
    }
    let mut languages = BTreeSet::new();
    for value in values {
        validate_text(&value)?;
        let language =
            SourceLanguage::from_stable_str(&value).ok_or(ConfigurationFileError::InvalidValue)?;
        if !languages.insert(language) {
            return Err(ConfigurationFileError::InvalidValue);
        }
    }
    Ok(languages)
}

fn parse_tool_profiles(
    values: Vec<String>,
) -> Result<BTreeSet<McpToolProfile>, ConfigurationFileError> {
    if values.len() > MAX_CONFIGURATION_TOOL_PROFILES {
        return Err(ConfigurationFileError::InvalidValue);
    }
    let mut profiles = BTreeSet::new();
    for value in values {
        let profile = parse_tool_profile(value)?;
        if !profiles.insert(profile) {
            return Err(ConfigurationFileError::InvalidValue);
        }
    }
    Ok(profiles)
}

fn validate_text(value: &str) -> Result<(), ConfigurationFileError> {
    if value.is_empty() || value.len() > MAX_CONFIGURATION_TEXT_BYTES {
        Err(ConfigurationFileError::InvalidValue)
    } else {
        Ok(())
    }
}
