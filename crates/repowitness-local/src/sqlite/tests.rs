use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};

use super::{
    APPLICATION_ID, MIGRATION_1, MIGRATION_1_NAME, MIGRATION_2, MIGRATION_2_NAME, MIGRATION_3,
    MIGRATION_3_NAME, MIGRATION_4, MIGRATION_4_NAME, MIGRATION_5, MIGRATION_5_NAME, SCHEMA_VERSION,
    SqliteStoreError, apply_migration, database_file_identity, migration_checksum, migrations,
    open_index_writer, open_index_writer_with_identity_and_hook,
    open_index_writer_with_identity_and_migration_hook,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repowitness-schema-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture directory should be created");
        Self(path)
    }

    fn database(&self) -> PathBuf {
        self.0.join("index.sqlite3")
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn raw_connection(path: &Path) -> Connection {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .expect("fixture database should reopen")
}

fn insert_workspace(connection: &Connection) {
    connection
            .execute(
                "INSERT INTO workspaces(
                    workspace_id, repository_identity, source_epoch
                 ) VALUES (1, X'1010101010101010101010101010101010101010101010101010101010101010', 0)",
                [],
            )
            .expect("workspace fixture should be inserted");
}

fn insert_minimal_worktree_memory(connection: &Connection) {
    connection
        .execute_batch(
            "BEGIN IMMEDIATE;
                 INSERT INTO memory_evidence(
                    workspace_id, record_id, revision_digest, ordinal,
                    evidence_kind, source_snapshot_digest, repository_path,
                    content_digest, artifact_digest, fact_ordinal, symbol_kind,
                    name, qualified_name, name_start, name_length,
                    declaration_start, declaration_length, declaration_digest,
                    producer_id, producer_version
                 ) VALUES (
                    1, X'11111111111111111111111111111111',
                    X'2222222222222222222222222222222222222222222222222222222222222222',
                    0, 'rust_symbol',
                    X'3333333333333333333333333333333333333333333333333333333333333333',
                    X'7372632F6C69622E7273',
                    X'4444444444444444444444444444444444444444444444444444444444444444',
                    X'5555555555555555555555555555555555555555555555555555555555555555',
                    0, 'function', 'publish', 'crate::publish', 3, 7, 0, 20,
                    X'6666666666666666666666666666666666666666666666666666666666666666',
                    'repowitness.rust.syntax', 'phase0-rust-syntax-v1'
                 );
                 INSERT INTO memory_versions(
                    workspace_id, record_id, revision_digest, schema_version,
                    canonical_json, kind, title, body, subject_evidence,
                    provenance_origin, authored_actor_kind, authored_actor_id,
                    authored_assurance, authored_lifecycle, validity_kind,
                    validity_source_snapshot, tombstone
                 ) VALUES (
                    1, X'11111111111111111111111111111111',
                    X'2222222222222222222222222222222222222222222222222222222222222222',
                    1, X'7B7D', 'decision', 'Keep publication atomic',
                    'Readers see complete generations.', 0, 'human',
                    'local_asserted', 'maintainer', 'locally_approved',
                    'active', 'worktree',
                    X'3333333333333333333333333333333333333333333333333333333333333333',
                    0
                 );
                 COMMIT;",
        )
        .expect("minimal memory version should be inserted atomically");
}

fn insert_active_generation_fixture(connection: &Connection) {
    connection
        .execute_batch(
            "INSERT INTO analysis_artifacts(
                    artifact_digest, lifecycle_state, source_content_digest,
                    producer_manifest_digest, configuration_digest,
                    analysis_schema_digest, canonicalization_version,
                    fact_count, visited_nodes, syntax_error_nodes,
                    known_parser_limitation_nodes, payload_digest, language
                 ) VALUES (
                    X'5555555555555555555555555555555555555555555555555555555555555555',
                    'staging',
                    X'4444444444444444444444444444444444444444444444444444444444444444',
                    zeroblob(32), zeroblob(32), zeroblob(32), 2, 1, 1, 0, 0,
                    zeroblob(32), 'rust'
                 );
                 INSERT INTO artifact_facts(
                    artifact_digest, ordinal, kind, name, qualified_name,
                    name_start, name_end, declaration_start, declaration_end
                 ) VALUES (
                    X'5555555555555555555555555555555555555555555555555555555555555555',
                    0, 'function', 'publish', 'crate::publish', 3, 10, 0, 20
                 );
                 INSERT INTO artifact_fact_correspondence(
                    artifact_digest, fact_ordinal, profile_id, profile_version,
                    declaration_digest, name_elided_digest
                 ) VALUES (
                    X'5555555555555555555555555555555555555555555555555555555555555555',
                    0, 'rust-name-elided', 1,
                    X'6666666666666666666666666666666666666666666666666666666666666666',
                    X'7777777777777777777777777777777777777777777777777777777777777777'
                 );
                 UPDATE analysis_artifacts SET lifecycle_state = 'complete'
                 WHERE artifact_digest =
                    X'5555555555555555555555555555555555555555555555555555555555555555';
                 INSERT INTO source_snapshots(
                    snapshot_digest, lifecycle_state, repository_identity,
                    git_state_digest, worktree_state_digest, configuration_digest,
                    producer_manifest_digest, analysis_schema_digest,
                    canonicalization_version, manifest_digest, file_count,
                    total_source_bytes, total_syntax_error_nodes
                 ) VALUES (
                    X'3333333333333333333333333333333333333333333333333333333333333333',
                    'complete',
                    X'1010101010101010101010101010101010101010101010101010101010101010',
                    zeroblob(32), zeroblob(32), zeroblob(32), zeroblob(32),
                    zeroblob(32), 1, zeroblob(32), 1, 20, 0
                 );
                 INSERT INTO index_generations(
                    generation_id, workspace_id, source_epoch, snapshot_digest,
                    lifecycle_state
                 ) VALUES (
                    1, 1, 0,
                    X'3333333333333333333333333333333333333333333333333333333333333333',
                    'resolving'
                 );
                 INSERT INTO generation_files(
                    generation_id, ordinal, repository_path,
                    content_digest, artifact_digest
                 ) VALUES (
                    1, 0, X'7372632F6C69622E7273',
                    X'4444444444444444444444444444444444444444444444444444444444444444',
                    X'5555555555555555555555555555555555555555555555555555555555555555'
                 );
                 UPDATE index_generations SET lifecycle_state = 'validating'
                 WHERE generation_id = 1;
                 UPDATE index_generations SET lifecycle_state = 'ready'
                 WHERE generation_id = 1;
                 UPDATE index_generations SET lifecycle_state = 'active'
                 WHERE generation_id = 1;
                 UPDATE workspaces SET active_generation_id = 1 WHERE workspace_id = 1;",
        )
        .expect("active generation fixture should be inserted");
}

fn insert_minimal_local_approval(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO memory_audit(
                    workspace_id, record_id, revision_digest, operation,
                    trusted_actor_kind, trusted_actor_id, recorded_at_unix_ms,
                    source_kind, source_format, source_revision,
                    display_revision, presentation_digest
                 ) VALUES (
                    1, X'11111111111111111111111111111111',
                    X'2222222222222222222222222222222222222222222222222222222222222222',
                    'locally_approved', 'local_asserted', 'trusted', 1,
                    'worktree', 'source_snapshot',
                    X'3333333333333333333333333333333333333333333333333333333333333333',
                    1, zeroblob(32)
                 )",
            [],
        )
        .expect("local approval fixture should be inserted");
}

include!("tests/schema_projection.rs");
include!("tests/memory_startup.rs");
include!("tests/baseline.rs");
include!("tests/migration_2.rs");
include!("tests/migration_3.rs");
