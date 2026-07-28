-- RepoWitness Phase 0 baseline: source indexing and retrieval.

CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY CHECK (version > 0),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 128),
    checksum BLOB NOT NULL CHECK (length(checksum) = 32),
    applied_at_unix_ms INTEGER NOT NULL CHECK (applied_at_unix_ms >= 0)
) STRICT;
CREATE TABLE workspaces (
    workspace_id INTEGER PRIMARY KEY CHECK (workspace_id > 0),
    repository_identity BLOB NOT NULL UNIQUE CHECK (length(repository_identity) = 32),
    source_epoch INTEGER NOT NULL CHECK (source_epoch >= 0),
    active_generation_id INTEGER,
    FOREIGN KEY (active_generation_id) REFERENCES index_generations(generation_id)
) STRICT;
CREATE TABLE source_snapshots (
    snapshot_digest BLOB PRIMARY KEY CHECK (length(snapshot_digest) = 32),
    lifecycle_state TEXT NOT NULL CHECK (lifecycle_state IN ('staging', 'complete')),
    repository_identity BLOB NOT NULL CHECK (length(repository_identity) = 32),
    git_state_digest BLOB NOT NULL CHECK (length(git_state_digest) = 32),
    worktree_state_digest BLOB NOT NULL CHECK (length(worktree_state_digest) = 32),
    configuration_digest BLOB NOT NULL CHECK (length(configuration_digest) = 32),
    producer_manifest_digest BLOB NOT NULL CHECK (length(producer_manifest_digest) = 32),
    analysis_schema_digest BLOB NOT NULL CHECK (length(analysis_schema_digest) = 32),
    canonicalization_version INTEGER NOT NULL
        CHECK (canonicalization_version BETWEEN 0 AND 4294967295),
    manifest_digest BLOB NOT NULL CHECK (length(manifest_digest) = 32),
    file_count INTEGER NOT NULL CHECK (file_count >= 0),
    total_source_bytes INTEGER NOT NULL CHECK (total_source_bytes >= 0),
    total_syntax_error_nodes INTEGER NOT NULL CHECK (total_syntax_error_nodes >= 0)
) STRICT, WITHOUT ROWID;
CREATE TABLE source_manifest_entries (
    snapshot_digest BLOB NOT NULL REFERENCES source_snapshots(snapshot_digest),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    repository_path BLOB NOT NULL CHECK (length(repository_path) BETWEEN 1 AND 1048576),
    file_kind TEXT NOT NULL CHECK (
        file_kind IN ('regular', 'symbolic_link', 'gitlink', 'other')
    ),
    content_digest BLOB NOT NULL CHECK (length(content_digest) = 32),
    PRIMARY KEY (snapshot_digest, ordinal),
    UNIQUE (snapshot_digest, repository_path)
) STRICT, WITHOUT ROWID;
CREATE TABLE "analysis_artifacts" (
    artifact_digest BLOB PRIMARY KEY CHECK (length(artifact_digest) = 32),
    lifecycle_state TEXT NOT NULL CHECK (lifecycle_state IN ('staging', 'complete')),
    source_content_digest BLOB NOT NULL CHECK (length(source_content_digest) = 32),
    producer_manifest_digest BLOB NOT NULL CHECK (length(producer_manifest_digest) = 32),
    configuration_digest BLOB NOT NULL CHECK (length(configuration_digest) = 32),
    analysis_schema_digest BLOB NOT NULL CHECK (length(analysis_schema_digest) = 32),
    canonicalization_version INTEGER NOT NULL
        CHECK (canonicalization_version BETWEEN 0 AND 4294967295),
    fact_count INTEGER NOT NULL CHECK (fact_count >= 0),
    visited_nodes INTEGER NOT NULL CHECK (visited_nodes >= 0),
    syntax_error_nodes INTEGER NOT NULL CHECK (syntax_error_nodes >= 0),
    payload_digest BLOB CHECK (payload_digest IS NULL OR length(payload_digest) = 32),
    language TEXT NOT NULL DEFAULT 'rust'
        CHECK (language IN ('rust', 'go', 'typescript', 'tsx', 'python'))
) STRICT, WITHOUT ROWID;
CREATE TABLE artifact_facts (
    artifact_digest BLOB NOT NULL REFERENCES analysis_artifacts(artifact_digest),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    kind TEXT NOT NULL CHECK (
        kind IN (
            'function', 'method', 'struct', 'enum', 'union', 'trait', 'module',
            'type_alias', 'constant', 'static', 'macro', 'interface',
            'defined_type', 'variable', 'class'
        )
    ),
    name TEXT NOT NULL CHECK (length(CAST(name AS BLOB)) BETWEEN 1 AND 1024),
    qualified_name TEXT NOT NULL
        CHECK (length(CAST(qualified_name AS BLOB)) BETWEEN 1 AND 4096),
    name_start INTEGER NOT NULL CHECK (name_start >= 0),
    name_end INTEGER NOT NULL CHECK (name_end >= name_start),
    declaration_start INTEGER NOT NULL CHECK (declaration_start >= 0),
    declaration_end INTEGER NOT NULL CHECK (declaration_end >= declaration_start),
    PRIMARY KEY (artifact_digest, ordinal)
) STRICT, WITHOUT ROWID;
CREATE TABLE artifact_fact_correspondence (
    artifact_digest BLOB NOT NULL,
    fact_ordinal INTEGER NOT NULL CHECK (fact_ordinal >= 0),
    profile_id TEXT NOT NULL CHECK (
        length(CAST(profile_id AS BLOB)) BETWEEN 1 AND 128
        AND profile_id NOT GLOB '*[^ -~]*'
    ),
    profile_version INTEGER NOT NULL
        CHECK (profile_version BETWEEN 1 AND 4294967295),
    declaration_digest BLOB NOT NULL CHECK (length(declaration_digest) = 32),
    name_elided_digest BLOB NOT NULL CHECK (length(name_elided_digest) = 32),
    PRIMARY KEY (
        artifact_digest, fact_ordinal, profile_id, profile_version
    ),
    UNIQUE (artifact_digest, fact_ordinal),
    FOREIGN KEY (artifact_digest, fact_ordinal)
        REFERENCES artifact_facts(artifact_digest, ordinal)
) STRICT, WITHOUT ROWID;
CREATE TABLE index_generations (
    generation_id INTEGER PRIMARY KEY CHECK (generation_id > 0),
    workspace_id INTEGER NOT NULL REFERENCES workspaces(workspace_id),
    source_epoch INTEGER NOT NULL CHECK (source_epoch >= 0),
    snapshot_digest BLOB NOT NULL REFERENCES source_snapshots(snapshot_digest),
    lifecycle_state TEXT NOT NULL CHECK (
        lifecycle_state IN (
            'discovered', 'extracting', 'resolving', 'validating', 'ready',
            'active', 'retained', 'failed', 'cancelled'
        )
    ),
    searched_count INTEGER NOT NULL DEFAULT 0 CHECK (searched_count >= 0),
    skipped_count INTEGER NOT NULL DEFAULT 0 CHECK (skipped_count >= 0),
    unresolved_count INTEGER NOT NULL DEFAULT 0 CHECK (unresolved_count >= 0),
    truncated_count INTEGER NOT NULL DEFAULT 0 CHECK (truncated_count >= 0),
    UNIQUE (workspace_id, generation_id)
) STRICT;
CREATE TABLE generation_files (
    generation_id INTEGER NOT NULL REFERENCES index_generations(generation_id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    repository_path BLOB NOT NULL CHECK (length(repository_path) BETWEEN 1 AND 1048576),
    content_digest BLOB NOT NULL CHECK (length(content_digest) = 32),
    artifact_digest BLOB NOT NULL REFERENCES analysis_artifacts(artifact_digest),
    PRIMARY KEY (generation_id, ordinal),
    UNIQUE (generation_id, repository_path)
) STRICT, WITHOUT ROWID;
CREATE TABLE generation_facts (
    generation_id INTEGER NOT NULL REFERENCES index_generations(generation_id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    fact_kind TEXT NOT NULL CHECK (length(fact_kind) BETWEEN 1 AND 64),
    payload BLOB NOT NULL CHECK (length(payload) BETWEEN 1 AND 4096),
    PRIMARY KEY (generation_id, ordinal)
) STRICT, WITHOUT ROWID;
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
CREATE TABLE search_projection_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    active_slot INTEGER NOT NULL CHECK (active_slot IN (0, 1))
) STRICT;

INSERT INTO search_projection_state(singleton, active_slot) VALUES (1, 0);

CREATE INDEX generations_by_workspace_and_epoch
ON index_generations(workspace_id, source_epoch);
CREATE UNIQUE INDEX one_active_generation_per_workspace
ON index_generations(workspace_id)
WHERE lifecycle_state = 'active';

CREATE TRIGGER analysis_artifact_completion
BEFORE UPDATE OF lifecycle_state ON analysis_artifacts
WHEN NOT (OLD.lifecycle_state = 'staging' AND NEW.lifecycle_state = 'complete')
BEGIN
    SELECT RAISE(ABORT, 'invalid analysis artifact transition');
END;
CREATE TRIGGER analysis_artifact_payload_digest_set_once
BEFORE UPDATE OF payload_digest ON analysis_artifacts
WHEN NOT (
    OLD.payload_digest IS NULL
    AND NEW.payload_digest IS NOT NULL
    AND length(NEW.payload_digest) = 32
)
BEGIN
    SELECT RAISE(ABORT, 'immutable analysis artifact payload identity');
END;
CREATE TRIGGER analysis_artifacts_no_semantic_update
BEFORE UPDATE OF
    artifact_digest, source_content_digest, producer_manifest_digest,
    configuration_digest, analysis_schema_digest, canonicalization_version,
    fact_count, visited_nodes, syntax_error_nodes, language
ON analysis_artifacts BEGIN
    SELECT RAISE(ABORT, 'immutable analysis artifact semantics');
END;
CREATE TRIGGER artifact_fact_correspondence_insert_only_while_staging
BEFORE INSERT ON artifact_fact_correspondence
WHEN
    (SELECT lifecycle_state FROM analysis_artifacts
     WHERE artifact_digest = NEW.artifact_digest) != 'staging'
    OR
    (SELECT language FROM analysis_artifacts
     WHERE artifact_digest = NEW.artifact_digest) != 'rust'
    OR NEW.profile_id != 'rust-name-elided'
    OR NEW.profile_version != 1
BEGIN
    SELECT RAISE(ABORT, 'analysis artifact is not accepting correspondence');
END;
CREATE TRIGGER artifact_fact_correspondence_no_update
BEFORE UPDATE ON artifact_fact_correspondence BEGIN
    SELECT RAISE(ABORT, 'immutable artifact correspondence');
END;
CREATE TRIGGER artifact_facts_insert_only_while_staging
BEFORE INSERT ON artifact_facts
WHEN (SELECT lifecycle_state FROM analysis_artifacts
      WHERE artifact_digest = NEW.artifact_digest) != 'staging'
BEGIN
    SELECT RAISE(ABORT, 'analysis artifact is not accepting facts');
END;
CREATE TRIGGER artifact_facts_no_update
BEFORE UPDATE ON artifact_facts BEGIN
    SELECT RAISE(ABORT, 'immutable artifact facts');
END;
CREATE TRIGGER complete_analysis_artifacts_no_delete
BEFORE DELETE ON analysis_artifacts
WHEN OLD.lifecycle_state = 'complete' BEGIN
    SELECT RAISE(ABORT, 'immutable complete analysis artifact');
END;
CREATE TRIGGER complete_artifact_fact_correspondence_no_delete
BEFORE DELETE ON artifact_fact_correspondence
WHEN (SELECT lifecycle_state FROM analysis_artifacts
      WHERE artifact_digest = OLD.artifact_digest) = 'complete'
BEGIN
    SELECT RAISE(ABORT, 'immutable complete artifact correspondence');
END;
CREATE TRIGGER complete_artifact_facts_no_delete
BEFORE DELETE ON artifact_facts
WHEN (SELECT lifecycle_state FROM analysis_artifacts
      WHERE artifact_digest = OLD.artifact_digest) = 'complete'
BEGIN
    SELECT RAISE(ABORT, 'immutable complete artifact facts');
END;
CREATE TRIGGER complete_source_manifest_entries_no_delete
BEFORE DELETE ON source_manifest_entries
WHEN (SELECT lifecycle_state FROM source_snapshots
      WHERE snapshot_digest = OLD.snapshot_digest) = 'complete'
BEGIN
    SELECT RAISE(ABORT, 'immutable complete source manifest');
END;
CREATE TRIGGER complete_source_snapshots_no_delete
BEFORE DELETE ON source_snapshots
WHEN OLD.lifecycle_state = 'complete' BEGIN
    SELECT RAISE(ABORT, 'immutable complete source snapshot');
END;
CREATE TRIGGER generation_facts_insert_only_while_staging
BEFORE INSERT ON generation_facts
WHEN (SELECT lifecycle_state FROM index_generations
      WHERE generation_id = NEW.generation_id) NOT IN ('resolving', 'validating')
BEGIN
    SELECT RAISE(ABORT, 'generation is not accepting facts');
END;
CREATE TRIGGER generation_facts_no_update
BEFORE UPDATE ON generation_facts BEGIN
    SELECT RAISE(ABORT, 'immutable generation facts');
END;
CREATE TRIGGER generation_files_insert_only_while_staging
BEFORE INSERT ON generation_files
WHEN (SELECT lifecycle_state FROM index_generations
      WHERE generation_id = NEW.generation_id) NOT IN ('extracting', 'resolving')
BEGIN
    SELECT RAISE(ABORT, 'generation is not accepting files');
END;
CREATE TRIGGER generation_files_no_update
BEFORE UPDATE ON generation_files BEGIN
    SELECT RAISE(ABORT, 'immutable generation files');
END;
CREATE TRIGGER generation_files_require_complete_artifacts
BEFORE INSERT ON generation_files
WHEN (SELECT lifecycle_state FROM analysis_artifacts
      WHERE artifact_digest = NEW.artifact_digest) != 'complete'
BEGIN
    SELECT RAISE(ABORT, 'generation artifact is incomplete');
END;
CREATE TRIGGER generation_lifecycle_transition
BEFORE UPDATE OF lifecycle_state ON index_generations
WHEN NOT (
    (OLD.lifecycle_state = 'discovered' AND NEW.lifecycle_state IN ('extracting', 'failed', 'cancelled')) OR
    (OLD.lifecycle_state = 'extracting' AND NEW.lifecycle_state IN ('resolving', 'failed', 'cancelled')) OR
    (OLD.lifecycle_state = 'resolving' AND NEW.lifecycle_state IN ('validating', 'failed', 'cancelled')) OR
    (OLD.lifecycle_state = 'validating' AND NEW.lifecycle_state IN ('ready', 'failed', 'cancelled')) OR
    (OLD.lifecycle_state = 'ready' AND NEW.lifecycle_state IN ('active', 'failed', 'cancelled')) OR
    (OLD.lifecycle_state = 'active' AND NEW.lifecycle_state = 'retained')
) BEGIN
    SELECT RAISE(ABORT, 'invalid generation lifecycle transition');
END;
CREATE TRIGGER generation_requires_complete_snapshot
BEFORE INSERT ON index_generations
WHEN (SELECT lifecycle_state FROM source_snapshots
      WHERE snapshot_digest = NEW.snapshot_digest) != 'complete'
BEGIN
    SELECT RAISE(ABORT, 'generation snapshot is incomplete');
END;
CREATE TRIGGER rust_artifact_correspondence_required_before_completion
BEFORE UPDATE OF lifecycle_state ON analysis_artifacts
WHEN
    OLD.lifecycle_state = 'staging'
    AND NEW.lifecycle_state = 'complete'
    AND NEW.language = 'rust'
    AND NEW.fact_count != (
        SELECT count(*) FROM artifact_fact_correspondence
        WHERE artifact_digest = NEW.artifact_digest
          AND profile_id = 'rust-name-elided'
          AND profile_version = 1
    )
BEGIN
    SELECT RAISE(ABORT, 'incomplete Rust artifact correspondence');
END;
CREATE TRIGGER source_manifest_entries_insert_only_while_staging
BEFORE INSERT ON source_manifest_entries
WHEN (SELECT lifecycle_state FROM source_snapshots
      WHERE snapshot_digest = NEW.snapshot_digest) != 'staging'
BEGIN
    SELECT RAISE(ABORT, 'source snapshot is not accepting entries');
END;
CREATE TRIGGER source_manifest_entries_no_update
BEFORE UPDATE ON source_manifest_entries BEGIN
    SELECT RAISE(ABORT, 'immutable source manifest');
END;
CREATE TRIGGER source_snapshot_completion
BEFORE UPDATE OF lifecycle_state ON source_snapshots
WHEN NOT (OLD.lifecycle_state = 'staging' AND NEW.lifecycle_state = 'complete')
BEGIN
    SELECT RAISE(ABORT, 'invalid source snapshot transition');
END;
CREATE TRIGGER source_snapshots_no_semantic_update
BEFORE UPDATE OF
    snapshot_digest, repository_identity, git_state_digest, worktree_state_digest,
    configuration_digest, producer_manifest_digest, analysis_schema_digest,
    canonicalization_version, manifest_digest, file_count, total_source_bytes,
    total_syntax_error_nodes
ON source_snapshots BEGIN
    SELECT RAISE(ABORT, 'immutable source snapshot semantics');
END;
