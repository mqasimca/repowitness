-- RepoWitness Phase 2 immutable all-language raw syntax-site projection.
--
-- These rows are observations only.  No table stores a resolved target,
-- correspondence, graph edge, or same-name association.

CREATE TABLE syntax_site_artifacts (
    artifact_digest BLOB PRIMARY KEY
        REFERENCES analysis_artifacts(artifact_digest),
    site_profile_version INTEGER NOT NULL
        CHECK (site_profile_version BETWEEN 1 AND 4294967295),
    site_count INTEGER NOT NULL CHECK (site_count >= 0),
    max_observed_depth INTEGER NOT NULL
        CHECK (max_observed_depth BETWEEN 0 AND 65535),
    owned_text_bytes INTEGER NOT NULL CHECK (owned_text_bytes >= 0),
    import_support TEXT NOT NULL CHECK (import_support IN ('available', 'unsupported')),
    reference_support TEXT NOT NULL CHECK (reference_support IN ('available', 'unsupported')),
    call_support TEXT NOT NULL CHECK (call_support IN ('available', 'unsupported')),
    test_marker_support TEXT NOT NULL
        CHECK (test_marker_support IN ('available', 'unsupported')),
    import_emitted INTEGER NOT NULL CHECK (import_emitted >= 0),
    reference_emitted INTEGER NOT NULL CHECK (reference_emitted >= 0),
    call_emitted INTEGER NOT NULL CHECK (call_emitted >= 0),
    test_marker_emitted INTEGER NOT NULL CHECK (test_marker_emitted >= 0),
    CHECK (site_count = import_emitted + reference_emitted + call_emitted + test_marker_emitted)
) STRICT, WITHOUT ROWID;

CREATE TABLE syntax_sites (
    artifact_digest BLOB NOT NULL
        REFERENCES syntax_site_artifacts(artifact_digest),
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 4294967295),
    site_kind TEXT NOT NULL CHECK (site_kind IN ('import', 'reference', 'call', 'test_marker')),
    extraction_evidence TEXT NOT NULL
        CHECK (extraction_evidence IN ('direct_syntax', 'syntax_heuristic')),
    occurrence_start INTEGER NOT NULL CHECK (occurrence_start >= 0),
    occurrence_end INTEGER NOT NULL CHECK (occurrence_end >= occurrence_start),
    target_start INTEGER NOT NULL CHECK (target_start >= occurrence_start),
    target_end INTEGER NOT NULL CHECK (target_end >= target_start AND target_end <= occurrence_end),
    raw_target TEXT NOT NULL CHECK (length(CAST(raw_target AS BLOB)) BETWEEN 1 AND 16384),
    PRIMARY KEY (artifact_digest, ordinal),
    UNIQUE (artifact_digest, ordinal, site_kind, occurrence_start, occurrence_end, target_start, target_end)
) STRICT, WITHOUT ROWID;

CREATE TABLE generation_syntax_site_requirements (
    generation_id INTEGER PRIMARY KEY
        REFERENCES index_generations(generation_id) ON DELETE CASCADE,
    site_profile_version INTEGER NOT NULL
        CHECK (site_profile_version BETWEEN 1 AND 4294967295)
) STRICT, WITHOUT ROWID;

CREATE TABLE generation_syntax_site_publications (
    generation_id INTEGER PRIMARY KEY
        REFERENCES generation_syntax_site_requirements(generation_id) ON DELETE CASCADE,
    lifecycle_state TEXT NOT NULL CHECK (lifecycle_state IN ('staging', 'complete')),
    site_profile_version INTEGER NOT NULL
        CHECK (site_profile_version BETWEEN 1 AND 4294967295),
    artifact_count INTEGER NOT NULL CHECK (artifact_count >= 0),
    site_count INTEGER NOT NULL CHECK (site_count >= 0),
    visited_node_count INTEGER NOT NULL CHECK (visited_node_count >= 0),
    syntax_error_node_count INTEGER NOT NULL CHECK (syntax_error_node_count >= 0),
    owned_text_bytes INTEGER NOT NULL CHECK (owned_text_bytes >= 0),
    import_site_count INTEGER NOT NULL CHECK (import_site_count >= 0),
    reference_site_count INTEGER NOT NULL CHECK (reference_site_count >= 0),
    call_site_count INTEGER NOT NULL CHECK (call_site_count >= 0),
    test_marker_site_count INTEGER NOT NULL CHECK (test_marker_site_count >= 0),
    CHECK (site_count = import_site_count + reference_site_count + call_site_count + test_marker_site_count)
) STRICT, WITHOUT ROWID;

CREATE TABLE generation_syntax_site_artifacts (
    generation_id INTEGER NOT NULL
        REFERENCES generation_syntax_site_publications(generation_id) ON DELETE CASCADE,
    repository_path BLOB NOT NULL CHECK (length(repository_path) BETWEEN 1 AND 1048576),
    syntax_site_artifact_digest BLOB NOT NULL
        REFERENCES syntax_site_artifacts(artifact_digest),
    PRIMARY KEY (generation_id, repository_path)
) STRICT, WITHOUT ROWID;

CREATE INDEX generation_syntax_site_artifacts_by_artifact
ON generation_syntax_site_artifacts(syntax_site_artifact_digest);

CREATE TRIGGER syntax_site_artifact_completion_requires_sites
BEFORE UPDATE OF lifecycle_state ON analysis_artifacts
WHEN OLD.lifecycle_state = 'staging'
AND NEW.lifecycle_state = 'complete'
AND EXISTS (SELECT 1 FROM syntax_site_artifacts WHERE artifact_digest = NEW.artifact_digest)
AND (
    NEW.fact_count != 0
    OR NEW.payload_digest IS NULL
    OR (SELECT site_count FROM syntax_site_artifacts WHERE artifact_digest = NEW.artifact_digest)
       != (SELECT count(*) FROM syntax_sites WHERE artifact_digest = NEW.artifact_digest)
    OR (SELECT count(*) FROM syntax_sites WHERE artifact_digest = NEW.artifact_digest)
       != COALESCE((SELECT max(ordinal) + 1 FROM syntax_sites WHERE artifact_digest = NEW.artifact_digest), 0)
)
BEGIN
    SELECT RAISE(ABORT, 'incomplete syntax site artifact');
END;

CREATE TRIGGER syntax_site_artifacts_no_update
BEFORE UPDATE ON syntax_site_artifacts BEGIN
    SELECT RAISE(ABORT, 'immutable syntax site artifact');
END;

CREATE TRIGGER syntax_sites_no_update
BEFORE UPDATE ON syntax_sites BEGIN
    SELECT RAISE(ABORT, 'immutable syntax site');
END;

CREATE TRIGGER syntax_site_artifacts_complete_no_delete
BEFORE DELETE ON syntax_site_artifacts
WHEN (SELECT lifecycle_state FROM analysis_artifacts
      WHERE artifact_digest = OLD.artifact_digest) = 'complete'
AND NOT EXISTS (
    SELECT 1 FROM retention_artifact_garbage
    WHERE artifact_digest = OLD.artifact_digest
)
BEGIN
    SELECT RAISE(ABORT, 'immutable complete syntax site artifact');
END;

CREATE TRIGGER syntax_sites_complete_no_delete
BEFORE DELETE ON syntax_sites
WHEN (SELECT lifecycle_state FROM analysis_artifacts
      WHERE artifact_digest = OLD.artifact_digest) = 'complete'
AND NOT EXISTS (
    SELECT 1 FROM retention_artifact_garbage
    WHERE artifact_digest = OLD.artifact_digest
)
BEGIN
    SELECT RAISE(ABORT, 'immutable complete syntax site');
END;

CREATE TRIGGER generation_syntax_site_requirements_insert_eligible
BEFORE INSERT ON generation_syntax_site_requirements
WHEN (SELECT lifecycle_state FROM index_generations WHERE generation_id = NEW.generation_id)
     NOT IN ('resolving', 'validating', 'ready')
BEGIN
    SELECT RAISE(ABORT, 'generation is not accepting syntax site requirement');
END;

CREATE TRIGGER generation_syntax_site_requirements_no_update
BEFORE UPDATE ON generation_syntax_site_requirements BEGIN
    SELECT RAISE(ABORT, 'immutable syntax site requirement');
END;

CREATE TRIGGER generation_syntax_site_publications_insert_staging_only
BEFORE INSERT ON generation_syntax_site_publications
WHEN NEW.lifecycle_state != 'staging'
OR NEW.site_profile_version != (
    SELECT site_profile_version FROM generation_syntax_site_requirements
    WHERE generation_id = NEW.generation_id
)
OR (SELECT lifecycle_state FROM index_generations WHERE generation_id = NEW.generation_id) != 'ready'
BEGIN
    SELECT RAISE(ABORT, 'generation is not accepting syntax site publication');
END;

CREATE TRIGGER generation_syntax_site_publications_no_semantic_update
BEFORE UPDATE OF generation_id, site_profile_version, artifact_count, site_count,
    visited_node_count, syntax_error_node_count, owned_text_bytes, import_site_count,
    reference_site_count, call_site_count, test_marker_site_count
ON generation_syntax_site_publications BEGIN
    SELECT RAISE(ABORT, 'immutable syntax site publication');
END;

CREATE TRIGGER generation_syntax_site_publication_lifecycle_transition
BEFORE UPDATE OF lifecycle_state ON generation_syntax_site_publications
WHEN NOT (OLD.lifecycle_state = 'staging' AND NEW.lifecycle_state = 'complete')
BEGIN
    SELECT RAISE(ABORT, 'invalid syntax site publication lifecycle transition');
END;

CREATE TRIGGER generation_syntax_site_artifacts_insert_staging_only
BEFORE INSERT ON generation_syntax_site_artifacts
WHEN (SELECT lifecycle_state FROM generation_syntax_site_publications
      WHERE generation_id = NEW.generation_id) != 'staging'
OR NOT EXISTS (
    SELECT 1
    FROM generation_files AS file
    JOIN analysis_artifacts AS source_artifact
      ON source_artifact.artifact_digest = file.artifact_digest
    JOIN analysis_artifacts AS artifact
      ON artifact.artifact_digest = NEW.syntax_site_artifact_digest
     AND artifact.lifecycle_state = 'complete'
     AND artifact.source_content_digest = file.content_digest
     AND artifact.language = source_artifact.language
    WHERE file.generation_id = NEW.generation_id
      AND file.repository_path = NEW.repository_path
)
BEGIN
    SELECT RAISE(ABORT, 'invalid generation syntax site artifact');
END;

CREATE TRIGGER generation_syntax_site_artifacts_no_update
BEFORE UPDATE ON generation_syntax_site_artifacts BEGIN
    SELECT RAISE(ABORT, 'immutable generation syntax site artifact');
END;

CREATE TRIGGER generation_syntax_site_completion_requires_complete_projection
BEFORE UPDATE OF lifecycle_state ON generation_syntax_site_publications
WHEN NEW.lifecycle_state = 'complete'
AND (
    (SELECT lifecycle_state FROM index_generations WHERE generation_id = NEW.generation_id) != 'ready'
    OR NEW.artifact_count != (
        SELECT count(*) FROM generation_syntax_site_artifacts WHERE generation_id = NEW.generation_id
    )
    OR NEW.artifact_count != (
        SELECT count(*) FROM generation_files WHERE generation_id = NEW.generation_id
    )
    OR NEW.site_count != (
        SELECT count(*)
        FROM generation_syntax_site_artifacts AS occurrence
        JOIN syntax_sites AS site ON site.artifact_digest = occurrence.syntax_site_artifact_digest
        WHERE occurrence.generation_id = NEW.generation_id
    )
    OR NEW.visited_node_count != (
        SELECT COALESCE(sum(artifact.visited_nodes), 0)
        FROM generation_syntax_site_artifacts AS occurrence
        JOIN analysis_artifacts AS artifact
          ON artifact.artifact_digest = occurrence.syntax_site_artifact_digest
        WHERE occurrence.generation_id = NEW.generation_id
    )
    OR NEW.syntax_error_node_count != (
        SELECT COALESCE(sum(artifact.syntax_error_nodes), 0)
        FROM generation_syntax_site_artifacts AS occurrence
        JOIN analysis_artifacts AS artifact
          ON artifact.artifact_digest = occurrence.syntax_site_artifact_digest
        WHERE occurrence.generation_id = NEW.generation_id
    )
    OR NEW.owned_text_bytes != (
        SELECT COALESCE(sum(artifact.owned_text_bytes), 0)
        FROM generation_syntax_site_artifacts AS occurrence
        JOIN syntax_site_artifacts AS artifact
          ON artifact.artifact_digest = occurrence.syntax_site_artifact_digest
        WHERE occurrence.generation_id = NEW.generation_id
    )
    OR NEW.import_site_count != (
        SELECT count(*)
        FROM generation_syntax_site_artifacts AS occurrence
        JOIN syntax_sites AS site ON site.artifact_digest = occurrence.syntax_site_artifact_digest
        WHERE occurrence.generation_id = NEW.generation_id AND site.site_kind = 'import'
    )
    OR NEW.reference_site_count != (
        SELECT count(*)
        FROM generation_syntax_site_artifacts AS occurrence
        JOIN syntax_sites AS site ON site.artifact_digest = occurrence.syntax_site_artifact_digest
        WHERE occurrence.generation_id = NEW.generation_id AND site.site_kind = 'reference'
    )
    OR NEW.call_site_count != (
        SELECT count(*)
        FROM generation_syntax_site_artifacts AS occurrence
        JOIN syntax_sites AS site ON site.artifact_digest = occurrence.syntax_site_artifact_digest
        WHERE occurrence.generation_id = NEW.generation_id AND site.site_kind = 'call'
    )
    OR NEW.test_marker_site_count != (
        SELECT count(*)
        FROM generation_syntax_site_artifacts AS occurrence
        JOIN syntax_sites AS site ON site.artifact_digest = occurrence.syntax_site_artifact_digest
        WHERE occurrence.generation_id = NEW.generation_id AND site.site_kind = 'test_marker'
    )
)
BEGIN
    SELECT RAISE(ABORT, 'incomplete generation syntax site publication');
END;

CREATE TRIGGER generation_activation_requires_syntax_sites_when_required
BEFORE UPDATE OF lifecycle_state ON index_generations
WHEN OLD.lifecycle_state = 'ready'
AND NEW.lifecycle_state = 'active'
AND EXISTS (SELECT 1 FROM generation_syntax_site_requirements WHERE generation_id = NEW.generation_id)
AND NOT EXISTS (
    SELECT 1 FROM generation_syntax_site_publications
    WHERE generation_id = NEW.generation_id AND lifecycle_state = 'complete'
)
BEGIN
    SELECT RAISE(ABORT, 'required generation syntax site projection is incomplete');
END;

DROP TRIGGER retention_artifact_garbage_validate_insert;
CREATE TRIGGER retention_artifact_garbage_validate_insert
BEFORE INSERT ON retention_artifact_garbage
WHEN
    (SELECT lifecycle_state FROM analysis_artifacts
     WHERE artifact_digest = NEW.artifact_digest) != 'complete'
    OR EXISTS (
        SELECT 1
        FROM generation_files AS file
        WHERE file.artifact_digest = NEW.artifact_digest
          AND NOT EXISTS (
              SELECT 1 FROM retention_generation_garbage AS garbage
              WHERE garbage.generation_id = file.generation_id
                AND garbage.plan_digest = NEW.plan_digest
          )
    )
    OR EXISTS (
        SELECT 1
        FROM generation_syntax_site_artifacts AS syntax_site_artifact
        WHERE syntax_site_artifact.syntax_site_artifact_digest = NEW.artifact_digest
          AND NOT EXISTS (
              SELECT 1 FROM retention_generation_garbage AS garbage
              WHERE garbage.generation_id = syntax_site_artifact.generation_id
                AND garbage.plan_digest = NEW.plan_digest
          )
    )
    OR EXISTS (
        SELECT 1 FROM memory_evidence
        WHERE artifact_digest = NEW.artifact_digest
    )
    OR EXISTS (
        SELECT 1 FROM memory_correspondence_audit
        WHERE source_artifact_digest = NEW.artifact_digest
           OR target_artifact_digest = NEW.artifact_digest
    )
    OR EXISTS (
        SELECT 1 FROM memory_projection_evidence
        WHERE target_artifact_digest = NEW.artifact_digest
    )
    OR EXISTS (
        SELECT 1 FROM memory_projection_candidates
        WHERE target_artifact_digest = NEW.artifact_digest
    )
    OR EXISTS (
        SELECT 1
        FROM generation_graph_artifacts AS artifact
        WHERE artifact.graph_artifact_digest = NEW.artifact_digest
          AND NOT EXISTS (
              SELECT 1 FROM retention_generation_garbage AS garbage
              WHERE garbage.generation_id = artifact.generation_id
                AND garbage.plan_digest = NEW.plan_digest
          )
    )
    OR EXISTS (
        SELECT 1
        FROM generation_graph_definitions AS definition
        WHERE definition.artifact_digest = NEW.artifact_digest
          AND NOT EXISTS (
              SELECT 1 FROM retention_generation_garbage AS garbage
              WHERE garbage.generation_id = definition.generation_id
                AND garbage.plan_digest = NEW.plan_digest
          )
    )
    OR EXISTS (
        SELECT 1
        FROM generation_graph_resolutions AS resolution
        WHERE resolution.site_artifact_digest = NEW.artifact_digest
          AND NOT EXISTS (
              SELECT 1 FROM retention_generation_garbage AS garbage
              WHERE garbage.generation_id = resolution.generation_id
                AND garbage.plan_digest = NEW.plan_digest
          )
    )
BEGIN
    SELECT RAISE(ABORT, 'analysis artifact is a retention root');
END;
