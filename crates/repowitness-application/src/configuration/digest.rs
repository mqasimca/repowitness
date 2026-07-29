use sha2::{Digest, Sha256};

use repowitness_domain::ConfigurationDigest;

use super::{
    ConfigurationProfile, EffectiveConfigurationPolicy, EffectiveConfigurationPreferences,
    McpToolProfile, resolved::CONFIGURATION_DIGEST_VERSION,
};
use crate::SourceLanguage;

const CONFIGURATION_DIGEST_DOMAIN: &[u8] = b"RepoWitness\0resolved-semantic-configuration\0";

pub(super) fn canonical_configuration_digest(
    schema_version: u16,
    resolver_version: u16,
    profile: ConfigurationProfile,
    preferences: &EffectiveConfigurationPreferences,
    policy: &EffectiveConfigurationPolicy,
) -> ConfigurationDigest {
    let mut hasher = Sha256::new();
    hasher.update(CONFIGURATION_DIGEST_DOMAIN);
    hasher.update(CONFIGURATION_DIGEST_VERSION.to_be_bytes());
    hasher.update(schema_version.to_be_bytes());
    hasher.update(resolver_version.to_be_bytes());
    hasher.update([profile_tag(profile)]);
    hasher.update(preferences.query_results.effective().to_be_bytes());
    hasher.update(preferences.context_bytes.effective().to_be_bytes());
    hasher.update(preferences.graph_depth.effective().to_be_bytes());
    hasher.update(preferences.graph_results.effective().to_be_bytes());
    hasher.update(
        preferences
            .watcher_poll_interval_ms
            .effective()
            .to_be_bytes(),
    );
    hasher.update([tool_profile_tag(preferences.mcp_tool_profile.requested())]);
    hasher.update([preferences
        .mcp_tool_profile
        .authorized()
        .map_or(0, tool_profile_tag)]);
    hasher.update([language_mask(policy.allowed_languages.effective())]);
    hasher.update([tool_profile_mask(
        policy.allowed_mcp_tool_profiles.effective(),
    )]);
    hasher.update(policy.max_source_file_bytes.effective().to_be_bytes());
    hasher.update(policy.max_source_files.effective().to_be_bytes());
    hasher.update(policy.max_query_results.effective().to_be_bytes());
    hasher.update(policy.max_context_bytes.effective().to_be_bytes());
    hasher.update(policy.max_graph_depth.effective().to_be_bytes());
    hasher.update(policy.max_graph_results.effective().to_be_bytes());
    hasher.update([u8::from(*policy.deny_memory_writes.effective())]);
    hasher.update([u8::from(*policy.follow_symlinks.effective())]);
    let retention = policy.retention();
    hasher.update(
        retention
            .retained_generations_per_source_slot()
            .effective()
            .to_be_bytes(),
    );
    hasher.update(
        retention
            .max_generation_candidates()
            .effective()
            .to_be_bytes(),
    );
    hasher.update(retention.max_rows().effective().to_be_bytes());
    hasher.update(retention.max_bytes().effective().to_be_bytes());
    ConfigurationDigest::new(hasher.finalize().into())
}

const fn profile_tag(profile: ConfigurationProfile) -> u8 {
    match profile {
        ConfigurationProfile::Local => 1,
    }
}

const fn tool_profile_tag(profile: McpToolProfile) -> u8 {
    match profile {
        McpToolProfile::Canonical => 1,
        McpToolProfile::Minimal => 2,
        McpToolProfile::IncumbentCompatible => 3,
    }
}

fn language_mask(languages: &std::collections::BTreeSet<SourceLanguage>) -> u8 {
    languages.iter().fold(0_u8, |mask, language| {
        mask | match language {
            SourceLanguage::Rust => 1 << 0,
            SourceLanguage::Go => 1 << 1,
            SourceLanguage::TypeScript => 1 << 2,
            SourceLanguage::Tsx => 1 << 3,
            SourceLanguage::Python => 1 << 4,
        }
    })
}

fn tool_profile_mask(profiles: &std::collections::BTreeSet<McpToolProfile>) -> u8 {
    profiles.iter().fold(0_u8, |mask, profile| {
        mask | match profile {
            McpToolProfile::Canonical => 1 << 0,
            McpToolProfile::Minimal => 1 << 1,
            McpToolProfile::IncumbentCompatible => 1 << 2,
        }
    })
}
