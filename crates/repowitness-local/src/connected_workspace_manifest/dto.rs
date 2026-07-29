use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ManifestDto {
    pub(super) schema_version: u64,
    pub(super) connected_workspace_id: String,
    #[serde(rename = "source")]
    pub(super) sources: Vec<SourceDto>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceDto {
    pub(super) source_slot_id: String,
    pub(super) repository_identity: String,
    pub(super) worktree_root: String,
    pub(super) selector: SelectorDto,
    pub(super) scope: ScopeDto,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SelectorDto {
    pub(super) kind: String,
    pub(super) value: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ScopeDto {
    pub(super) kind: String,
    pub(super) roots: Option<Vec<String>>,
}
