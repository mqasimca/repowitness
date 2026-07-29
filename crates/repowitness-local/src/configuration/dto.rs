use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConfigurationFileDto {
    pub(super) schema_version: u64,
    pub(super) profile: Option<String>,
    pub(super) preferences: Option<PreferenceDto>,
    pub(super) policy: Option<PolicyDto>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreferenceDto {
    pub(super) query_results: Option<u64>,
    pub(super) context_bytes: Option<u64>,
    pub(super) graph_depth: Option<u64>,
    pub(super) graph_results: Option<u64>,
    pub(super) watcher_poll_interval_ms: Option<u64>,
    pub(super) mcp_tool_profile: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PolicyDto {
    pub(super) allowed_languages: Option<Vec<String>>,
    pub(super) allowed_mcp_tool_profiles: Option<Vec<String>>,
    pub(super) max_source_file_bytes: Option<u64>,
    pub(super) max_source_files: Option<u64>,
    pub(super) max_query_results: Option<u64>,
    pub(super) max_context_bytes: Option<u64>,
    pub(super) max_graph_depth: Option<u64>,
    pub(super) max_graph_results: Option<u64>,
    pub(super) deny_memory_writes: Option<bool>,
    pub(super) follow_symlinks: Option<bool>,
    pub(super) retained_generations_per_source_slot: Option<u64>,
    pub(super) max_retention_generation_candidates: Option<u64>,
    pub(super) max_retention_rows: Option<u64>,
    pub(super) max_retention_bytes: Option<u64>,
}
