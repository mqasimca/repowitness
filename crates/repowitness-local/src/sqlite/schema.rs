pub(super) const APPLICATION_ID: i64 = 0x5257_5031;
pub(super) const SCHEMA_VERSION: i64 = 4;
pub(super) const MIGRATION_1_NAME: &str = "phase0_design_partner_baseline";
pub(super) const MIGRATION_1: &str = concat!(
    include_str!("schema/baseline_1_core.sql"),
    include_str!("schema/baseline_1_memory_journal.sql"),
    include_str!("schema/baseline_1_memory_projection.sql"),
);
pub(super) const MIGRATION_2_NAME: &str = "persist_known_parser_limitations";
pub(super) const MIGRATION_2: &str = include_str!("schema/0002_parser_diagnostics.sql");
pub(super) const MIGRATION_3_NAME: &str = "phase1_workspace_graph_and_retention_foundation";
pub(super) const MIGRATION_3: &str = concat!(
    include_str!("migrations/0003_phase1_workspace.sql"),
    include_str!("migrations/0003_phase1_graph.sql"),
    include_str!("migrations/0003_phase1_graph_completion.sql"),
    include_str!("migrations/0003_phase1_retention.sql"),
);
pub(super) const MIGRATION_4_NAME: &str = "phase2_scip_precision_overlay";
pub(super) const MIGRATION_4: &str = include_str!("migrations/0004_phase2_scip_overlay.sql");

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
