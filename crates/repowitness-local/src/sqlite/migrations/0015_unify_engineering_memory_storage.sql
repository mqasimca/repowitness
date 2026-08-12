-- RepoWitness: materialize the current profile in the immutable normalized
-- journal. The profile-v2 tables remain as an archival compatibility source
-- for databases upgraded from the short-lived split implementation.

DROP TRIGGER memory_evidence_before_publication;
DROP TRIGGER memory_relationships_before_publication;
DROP TRIGGER memory_validity_commits_before_publication;
DROP TRIGGER memory_version_parents_before_publication;
DROP TRIGGER retention_snapshot_garbage_validate_insert;
DROP VIEW memory_versions_all;
DROP VIEW memory_current_trust;
DROP VIEW memory_audit_all;

CREATE TABLE memory_versions_rebuilt (
    workspace_id INTEGER NOT NULL REFERENCES workspaces(workspace_id),
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB NOT NULL CHECK (length(revision_digest) = 32),
    schema_version INTEGER NOT NULL CHECK (schema_version IN (1, 2)),
    canonical_json BLOB NOT NULL CHECK (length(canonical_json) BETWEEN 1 AND 262144),
    kind TEXT NOT NULL CHECK (
        kind IN ('decision', 'failure', 'fact', 'procedure', 'episode', 'preference', 'policy')
    ),
    title TEXT NOT NULL CHECK (
        length(CAST(title AS BLOB)) BETWEEN 1 AND 256
        AND instr(title, char(0)) = 0
        AND instr(title, char(10)) = 0
        AND instr(title, char(13)) = 0
        AND instr(title, char(133)) = 0
        AND instr(title, char(8232)) = 0
        AND instr(title, char(8233)) = 0
    ),
    body TEXT NOT NULL CHECK (
        length(CAST(body AS BLOB)) BETWEEN 1 AND 16384
        AND instr(body, char(0)) = 0
        AND instr(body, char(13)) = 0
    ),
    subject_evidence INTEGER NOT NULL CHECK (subject_evidence BETWEEN 0 AND 9007199254740991),
    provenance_origin TEXT NOT NULL CHECK (provenance_origin = 'human'),
    authored_actor_kind TEXT NOT NULL CHECK (authored_actor_kind = 'local_asserted'),
    authored_actor_id TEXT NOT NULL CHECK (
        length(CAST(authored_actor_id AS BLOB)) BETWEEN 1 AND 128
        AND authored_actor_id NOT GLOB '*[^ -~]*'
    ),
    authored_assurance TEXT NOT NULL CHECK (authored_assurance = 'locally_approved'),
    authored_lifecycle TEXT NOT NULL CHECK (
        authored_lifecycle IN (
            'active', 'needs_review', 'stale', 'contradicted', 'superseded',
            'quarantined', 'tombstoned'
        )
    ),
    validity_kind TEXT NOT NULL CHECK (validity_kind IN ('commits', 'worktree')),
    validity_source_snapshot BLOB CHECK (
        (validity_kind = 'commits' AND validity_source_snapshot IS NULL)
        OR
        (validity_kind = 'worktree' AND length(validity_source_snapshot) = 32)
    ),
    tombstone INTEGER NOT NULL CHECK (
        tombstone IN (0, 1)
        AND (
            (tombstone = 1 AND authored_lifecycle = 'tombstoned')
            OR
            (tombstone = 0 AND authored_lifecycle != 'tombstoned')
        )
    ),
    PRIMARY KEY (workspace_id, record_id, revision_digest),
    FOREIGN KEY (
        workspace_id, record_id, revision_digest, subject_evidence
    ) REFERENCES memory_evidence(
        workspace_id, record_id, revision_digest, ordinal
    ) DEFERRABLE INITIALLY DEFERRED
) STRICT, WITHOUT ROWID;

INSERT INTO memory_versions_rebuilt(
    workspace_id, record_id, revision_digest, schema_version, canonical_json,
    kind, title, body, subject_evidence, provenance_origin,
    authored_actor_kind, authored_actor_id, authored_assurance,
    authored_lifecycle, validity_kind, validity_source_snapshot, tombstone
)
SELECT workspace_id, record_id, revision_digest, schema_version, canonical_json,
       kind, title, body, subject_evidence, provenance_origin,
       authored_actor_kind, authored_actor_id, authored_assurance,
       authored_lifecycle, validity_kind, validity_source_snapshot, tombstone
FROM memory_versions;

DROP TABLE memory_versions;
ALTER TABLE memory_versions_rebuilt RENAME TO memory_versions;

CREATE TRIGGER memory_evidence_before_publication
BEFORE INSERT ON memory_evidence
WHEN EXISTS (
    SELECT 1 FROM memory_versions
    WHERE workspace_id = NEW.workspace_id
      AND record_id = NEW.record_id
      AND revision_digest = NEW.revision_digest
)
BEGIN
    SELECT RAISE(ABORT, 'immutable memory evidence');
END;
CREATE TRIGGER memory_relationships_before_publication
BEFORE INSERT ON memory_relationships
WHEN EXISTS (
    SELECT 1 FROM memory_versions
    WHERE workspace_id = NEW.workspace_id
      AND record_id = NEW.record_id
      AND revision_digest = NEW.revision_digest
)
BEGIN
    SELECT RAISE(ABORT, 'immutable memory relationships');
END;
CREATE TRIGGER memory_validity_commits_before_publication
BEFORE INSERT ON memory_validity_commits
WHEN EXISTS (
    SELECT 1 FROM memory_versions
    WHERE workspace_id = NEW.workspace_id
      AND record_id = NEW.record_id
      AND revision_digest = NEW.revision_digest
)
BEGIN
    SELECT RAISE(ABORT, 'immutable memory validity');
END;
CREATE TRIGGER memory_version_parents_before_publication
BEFORE INSERT ON memory_version_parents
WHEN EXISTS (
    SELECT 1 FROM memory_versions
    WHERE workspace_id = NEW.workspace_id
      AND record_id = NEW.record_id
      AND revision_digest = NEW.revision_digest
)
BEGIN
    SELECT RAISE(ABORT, 'immutable memory version parents');
END;

CREATE TRIGGER retention_snapshot_garbage_validate_insert
BEFORE INSERT ON retention_snapshot_garbage
WHEN
    (SELECT lifecycle_state FROM source_snapshots
     WHERE snapshot_digest = NEW.snapshot_digest) != 'complete'
    OR EXISTS (
        SELECT 1
        FROM index_generations AS generation
        WHERE generation.snapshot_digest = NEW.snapshot_digest
          AND NOT EXISTS (
              SELECT 1 FROM retention_generation_garbage AS garbage
              WHERE garbage.generation_id = generation.generation_id
                AND garbage.plan_digest = NEW.plan_digest
          )
    )
    OR EXISTS (
        SELECT 1 FROM memory_projection_generations
        WHERE snapshot_digest = NEW.snapshot_digest
    )
    OR EXISTS (
        SELECT 1 FROM memory_versions
        WHERE validity_source_snapshot = NEW.snapshot_digest
    )
    OR EXISTS (
        SELECT 1 FROM memory_evidence
        WHERE source_snapshot_digest = NEW.snapshot_digest
    )
    OR EXISTS (
        SELECT 1 FROM memory_audit
        WHERE source_format = 'source_snapshot'
          AND source_revision = NEW.snapshot_digest
    )
    OR EXISTS (
        SELECT 1 FROM memory_correspondence_audit
        WHERE source_snapshot_digest = NEW.snapshot_digest
           OR target_snapshot_digest = NEW.snapshot_digest
    )
    OR EXISTS (
        SELECT 1 FROM memory_projection_evidence
        WHERE target_snapshot_digest = NEW.snapshot_digest
    )
    OR EXISTS (
        SELECT 1 FROM memory_projection_candidates
        WHERE target_snapshot_digest = NEW.snapshot_digest
    )
BEGIN
    SELECT RAISE(ABORT, 'source snapshot is a retention root');
END;

CREATE TRIGGER memory_version_validate_children
AFTER INSERT ON memory_versions
WHEN
    (SELECT count(*) FROM memory_evidence
     WHERE workspace_id = NEW.workspace_id
       AND record_id = NEW.record_id
       AND revision_digest = NEW.revision_digest) NOT BETWEEN 1 AND 16
    OR (
        SELECT count(*) FROM memory_evidence
        WHERE workspace_id = NEW.workspace_id
          AND record_id = NEW.record_id
          AND revision_digest = NEW.revision_digest
    ) != (
        SELECT max(ordinal) + 1 FROM memory_evidence
        WHERE workspace_id = NEW.workspace_id
          AND record_id = NEW.record_id
          AND revision_digest = NEW.revision_digest
    )
    OR (
        SELECT count(*) FROM memory_version_parents
        WHERE workspace_id = NEW.workspace_id
          AND record_id = NEW.record_id
          AND revision_digest = NEW.revision_digest
    ) != coalesce((
        SELECT max(ordinal) + 1 FROM memory_version_parents
        WHERE workspace_id = NEW.workspace_id
          AND record_id = NEW.record_id
          AND revision_digest = NEW.revision_digest
    ), 0)
    OR (
        SELECT count(*) FROM memory_relationships
        WHERE workspace_id = NEW.workspace_id
          AND record_id = NEW.record_id
          AND revision_digest = NEW.revision_digest
    ) != coalesce((
        SELECT max(ordinal) + 1 FROM memory_relationships
        WHERE workspace_id = NEW.workspace_id
          AND record_id = NEW.record_id
          AND revision_digest = NEW.revision_digest
    ), 0)
    OR (
        SELECT count(*) FROM memory_validity_commits
        WHERE workspace_id = NEW.workspace_id
          AND record_id = NEW.record_id
          AND revision_digest = NEW.revision_digest
          AND side = 'introduced_by'
    ) != coalesce((
        SELECT max(ordinal) + 1 FROM memory_validity_commits
        WHERE workspace_id = NEW.workspace_id
          AND record_id = NEW.record_id
          AND revision_digest = NEW.revision_digest
          AND side = 'introduced_by'
    ), 0)
    OR (
        SELECT count(*) FROM memory_validity_commits
        WHERE workspace_id = NEW.workspace_id
          AND record_id = NEW.record_id
          AND revision_digest = NEW.revision_digest
          AND side = 'invalidated_by'
    ) != coalesce((
        SELECT max(ordinal) + 1 FROM memory_validity_commits
        WHERE workspace_id = NEW.workspace_id
          AND record_id = NEW.record_id
          AND revision_digest = NEW.revision_digest
          AND side = 'invalidated_by'
    ), 0)
    OR NOT EXISTS (
        SELECT 1 FROM memory_evidence
        WHERE workspace_id = NEW.workspace_id
          AND record_id = NEW.record_id
          AND revision_digest = NEW.revision_digest
          AND ordinal = NEW.subject_evidence
    )
    OR (
        NEW.validity_kind = 'commits'
        AND NOT EXISTS (
            SELECT 1 FROM memory_validity_commits
            WHERE workspace_id = NEW.workspace_id
              AND record_id = NEW.record_id
              AND revision_digest = NEW.revision_digest
              AND side = 'introduced_by'
        )
    )
    OR (
        NEW.validity_kind = 'worktree'
        AND EXISTS (
            SELECT 1 FROM memory_validity_commits
            WHERE workspace_id = NEW.workspace_id
              AND record_id = NEW.record_id
              AND revision_digest = NEW.revision_digest
        )
    )
    OR (
        NEW.tombstone = 1
        AND NOT EXISTS (
            SELECT 1 FROM memory_version_parents
            WHERE workspace_id = NEW.workspace_id
              AND record_id = NEW.record_id
              AND revision_digest = NEW.revision_digest
        )
    )
BEGIN
    SELECT RAISE(ABORT, 'invalid memory version children');
END;

CREATE TRIGGER memory_versions_no_delete
BEFORE DELETE ON memory_versions BEGIN
    SELECT RAISE(ABORT, 'immutable memory versions');
END;
CREATE TRIGGER memory_versions_no_update
BEFORE UPDATE ON memory_versions BEGIN
    SELECT RAISE(ABORT, 'immutable memory versions');
END;

INSERT INTO memory_audit(
    event_id, workspace_id, record_id, revision_digest, operation,
    trusted_actor_kind, trusted_actor_id, recorded_at_unix_ms,
    source_kind, source_format, source_revision,
    display_revision, presentation_digest
)
SELECT
    (SELECT coalesce(max(event_id), 0) FROM memory_audit)
        + row_number() OVER (
            ORDER BY event_id, workspace_id, record_id, revision_digest
        ),
    workspace_id, record_id, revision_digest, operation,
    trusted_actor_kind, trusted_actor_id, recorded_at_unix_ms,
    source_kind, source_format, source_revision,
    display_revision, presentation_digest
FROM memory_profile_v2_audit;

CREATE VIEW memory_versions_all AS
SELECT workspace_id, record_id, revision_digest, schema_version,
       canonical_json, kind, title, body, subject_evidence,
       provenance_origin, authored_actor_kind, authored_actor_id,
       authored_assurance, authored_lifecycle, validity_kind,
       validity_source_snapshot, tombstone
FROM memory_versions;

CREATE VIEW memory_audit_all AS
SELECT audit.event_id, audit.workspace_id, audit.record_id,
       audit.revision_digest, audit.operation,
       audit.trusted_actor_kind, audit.trusted_actor_id,
       audit.recorded_at_unix_ms, audit.source_kind, audit.source_format,
       audit.source_revision, audit.display_revision,
       audit.presentation_digest, version.schema_version
FROM memory_audit AS audit
JOIN memory_versions AS version
  ON version.workspace_id = audit.workspace_id
 AND version.record_id = audit.record_id
 AND version.revision_digest = audit.revision_digest;

CREATE VIEW memory_current_trust AS
SELECT event_id, workspace_id, record_id, revision_digest,
       trusted_actor_kind, trusted_actor_id, recorded_at_unix_ms,
       source_kind, source_format, source_revision, display_revision,
       presentation_digest, schema_version
FROM memory_audit_all
WHERE operation = 'locally_approved';

CREATE TRIGGER memory_profile_v2_versions_archived_insert
BEFORE INSERT ON memory_profile_v2_versions BEGIN
    SELECT RAISE(ABORT, 'profile v2 storage is archival');
END;
CREATE TRIGGER memory_profile_v2_parents_archived_insert
BEFORE INSERT ON memory_profile_v2_parents BEGIN
    SELECT RAISE(ABORT, 'profile v2 storage is archival');
END;
CREATE TRIGGER memory_profile_v2_audit_archived_insert
BEFORE INSERT ON memory_profile_v2_audit BEGIN
    SELECT RAISE(ABORT, 'profile v2 storage is archival');
END;
