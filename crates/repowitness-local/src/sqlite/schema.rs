pub(super) const APPLICATION_ID: i64 = 0x5257_5031;
pub(super) const SCHEMA_VERSION: i64 = 1;
pub(super) const MIGRATION_1_NAME: &str = "phase0_design_partner_baseline";
pub(super) const MIGRATION_1: &str = concat!(
    include_str!("schema/baseline_1_core.sql"),
    include_str!("schema/baseline_1_memory_journal.sql"),
    include_str!("schema/baseline_1_memory_projection.sql"),
);

pub(super) const RECREATE_GENERATION_SEARCH: &str = r#"
DROP TABLE IF EXISTS generation_search;
CREATE VIRTUAL TABLE generation_search USING fts5(
    generation_id UNINDEXED,
    repository_path UNINDEXED,
    fact_ordinal UNINDEXED,
    content_digest UNINDEXED,
    artifact_digest UNINDEXED,
    name_start UNINDEXED,
    name_end UNINDEXED,
    declaration_start UNINDEXED,
    declaration_end UNINDEXED,
    kind,
    name,
    qualified_name,
    tokenize = "unicode61 remove_diacritics 0 tokenchars '_'"
);
"#;

pub(super) const RECREATE_GENERATION_SEARCH_REBUILD: &str = r#"
DROP TABLE IF EXISTS generation_search_rebuild;
CREATE VIRTUAL TABLE generation_search_rebuild USING fts5(
    generation_id UNINDEXED,
    repository_path UNINDEXED,
    fact_ordinal UNINDEXED,
    content_digest UNINDEXED,
    artifact_digest UNINDEXED,
    name_start UNINDEXED,
    name_end UNINDEXED,
    declaration_start UNINDEXED,
    declaration_end UNINDEXED,
    kind,
    name,
    qualified_name,
    tokenize = "unicode61 remove_diacritics 0 tokenchars '_'"
);
"#;
