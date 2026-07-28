-- RepoWitness Phase 0 baseline: append-only engineering-memory journal.

CREATE TABLE memory_versions (
    workspace_id INTEGER NOT NULL REFERENCES workspaces(workspace_id),
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB NOT NULL CHECK (length(revision_digest) = 32),
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    canonical_json BLOB NOT NULL CHECK (length(canonical_json) BETWEEN 1 AND 262144),
    kind TEXT NOT NULL CHECK (kind IN ('decision', 'failure')),
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
    subject_evidence INTEGER NOT NULL
        CHECK (subject_evidence BETWEEN 0 AND 9007199254740991),
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
CREATE TABLE memory_version_parents (
    workspace_id INTEGER NOT NULL,
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB NOT NULL CHECK (length(revision_digest) = 32),
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 7),
    parent_revision_digest BLOB NOT NULL CHECK (length(parent_revision_digest) = 32),
    PRIMARY KEY (workspace_id, record_id, revision_digest, ordinal),
    UNIQUE (workspace_id, record_id, revision_digest, parent_revision_digest),
    FOREIGN KEY (workspace_id, record_id, revision_digest)
        REFERENCES memory_versions(workspace_id, record_id, revision_digest)
        DEFERRABLE INITIALLY DEFERRED
) STRICT, WITHOUT ROWID;
CREATE TABLE memory_validity_commits (
    workspace_id INTEGER NOT NULL,
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB NOT NULL CHECK (length(revision_digest) = 32),
    side TEXT NOT NULL CHECK (side IN ('introduced_by', 'invalidated_by')),
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 15),
    object_format TEXT NOT NULL CHECK (object_format IN ('sha1', 'sha256')),
    object_id BLOB NOT NULL CHECK (
        (object_format = 'sha1' AND length(object_id) = 20)
        OR (object_format = 'sha256' AND length(object_id) = 32)
    ),
    PRIMARY KEY (workspace_id, record_id, revision_digest, side, ordinal),
    UNIQUE (
        workspace_id, record_id, revision_digest, side, object_format, object_id
    ),
    FOREIGN KEY (workspace_id, record_id, revision_digest)
        REFERENCES memory_versions(workspace_id, record_id, revision_digest)
        DEFERRABLE INITIALLY DEFERRED
) STRICT, WITHOUT ROWID;
CREATE TABLE memory_evidence (
    workspace_id INTEGER NOT NULL,
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB NOT NULL CHECK (length(revision_digest) = 32),
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 15),
    evidence_kind TEXT NOT NULL CHECK (evidence_kind = 'rust_symbol'),
    source_snapshot_digest BLOB NOT NULL CHECK (length(source_snapshot_digest) = 32),
    repository_path BLOB NOT NULL CHECK (
        length(repository_path) BETWEEN 1 AND 32764
        AND instr(repository_path, X'00') = 0
    ),
    content_digest BLOB NOT NULL CHECK (length(content_digest) = 32),
    artifact_digest BLOB NOT NULL CHECK (length(artifact_digest) = 32),
    fact_ordinal INTEGER NOT NULL CHECK (fact_ordinal BETWEEN 0 AND 9007199254740991),
    symbol_kind TEXT NOT NULL CHECK (
        symbol_kind IN (
            'function', 'method', 'struct', 'enum', 'union', 'trait', 'module',
            'type_alias', 'constant', 'static', 'macro'
        )
    ),
    name TEXT NOT NULL CHECK (
        length(CAST(name AS BLOB)) BETWEEN 1 AND 256
        AND instr(name, char(0)) = 0
        AND instr(name, char(10)) = 0
        AND instr(name, char(13)) = 0
    ),
    qualified_name TEXT NOT NULL CHECK (
        length(CAST(qualified_name AS BLOB)) BETWEEN 1 AND 1024
        AND instr(qualified_name, char(0)) = 0
        AND instr(qualified_name, char(10)) = 0
        AND instr(qualified_name, char(13)) = 0
    ),
    name_start INTEGER NOT NULL CHECK (name_start BETWEEN 0 AND 9007199254740991),
    name_length INTEGER NOT NULL CHECK (
        name_length BETWEEN 1 AND 9007199254740991
        AND name_length = length(CAST(name AS BLOB))
        AND name_start + name_length <= 8388608
    ),
    declaration_start INTEGER NOT NULL
        CHECK (declaration_start BETWEEN 0 AND 9007199254740991),
    declaration_length INTEGER NOT NULL CHECK (
        declaration_length BETWEEN 1 AND 9007199254740991
        AND declaration_start + declaration_length <= 8388608
        AND name_start >= declaration_start
        AND name_start + name_length <= declaration_start + declaration_length
    ),
    declaration_digest BLOB NOT NULL CHECK (length(declaration_digest) = 32),
    producer_id TEXT NOT NULL CHECK (
        length(CAST(producer_id AS BLOB)) BETWEEN 1 AND 128
        AND producer_id NOT GLOB '*[^ -~]*'
    ),
    producer_version TEXT NOT NULL CHECK (
        length(CAST(producer_version AS BLOB)) BETWEEN 1 AND 128
        AND producer_version NOT GLOB '*[^ -~]*'
    ),
    PRIMARY KEY (workspace_id, record_id, revision_digest, ordinal),
    FOREIGN KEY (workspace_id, record_id, revision_digest)
        REFERENCES memory_versions(workspace_id, record_id, revision_digest)
        DEFERRABLE INITIALLY DEFERRED
) STRICT, WITHOUT ROWID;
CREATE TABLE memory_relationships (
    workspace_id INTEGER NOT NULL,
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB NOT NULL CHECK (length(revision_digest) = 32),
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 15),
    relationship_kind TEXT NOT NULL CHECK (
        relationship_kind IN ('contradicts', 'supersedes')
    ),
    target_record_id BLOB NOT NULL CHECK (length(target_record_id) = 16),
    target_revision_digest BLOB NOT NULL CHECK (length(target_revision_digest) = 32),
    PRIMARY KEY (workspace_id, record_id, revision_digest, ordinal),
    UNIQUE (
        workspace_id, record_id, revision_digest,
        relationship_kind, target_record_id, target_revision_digest
    ),
    FOREIGN KEY (workspace_id, record_id, revision_digest)
        REFERENCES memory_versions(workspace_id, record_id, revision_digest)
        DEFERRABLE INITIALLY DEFERRED
) STRICT, WITHOUT ROWID;
CREATE TABLE memory_audit (
    event_id INTEGER PRIMARY KEY CHECK (event_id > 0),
    workspace_id INTEGER NOT NULL,
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB NOT NULL CHECK (length(revision_digest) = 32),
    operation TEXT NOT NULL CHECK (operation IN ('observed', 'locally_approved')),
    trusted_actor_kind TEXT NOT NULL CHECK (trusted_actor_kind = 'local_asserted'),
    trusted_actor_id TEXT NOT NULL CHECK (
        length(CAST(trusted_actor_id AS BLOB)) BETWEEN 1 AND 128
        AND trusted_actor_id NOT GLOB '*[^ -~]*'
    ),
    recorded_at_unix_ms INTEGER NOT NULL CHECK (recorded_at_unix_ms >= 0),
    source_kind TEXT NOT NULL CHECK (source_kind IN ('git', 'worktree')),
    source_format TEXT NOT NULL CHECK (
        source_format IN ('sha1', 'sha256', 'source_snapshot')
    ),
    source_revision BLOB NOT NULL CHECK (
        (source_kind = 'git' AND source_format = 'sha1' AND length(source_revision) = 20)
        OR
        (source_kind = 'git' AND source_format = 'sha256' AND length(source_revision) = 32)
        OR
        (
            source_kind = 'worktree'
            AND source_format = 'source_snapshot'
            AND length(source_revision) = 32
        )
    ),
    display_revision INTEGER NOT NULL CHECK (display_revision BETWEEN 1 AND 4294967295),
    presentation_digest BLOB NOT NULL CHECK (length(presentation_digest) = 32),
    FOREIGN KEY (workspace_id, record_id, revision_digest)
        REFERENCES memory_versions(workspace_id, record_id, revision_digest)
) STRICT;
CREATE TABLE memory_correspondence_audit (
    event_id INTEGER PRIMARY KEY CHECK (event_id > 0),
    workspace_id INTEGER NOT NULL,
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB NOT NULL CHECK (length(revision_digest) = 32),
    evidence_ordinal INTEGER NOT NULL CHECK (evidence_ordinal BETWEEN 0 AND 15),
    operation TEXT NOT NULL CHECK (
        operation IN ('approved', 'rejected', 'manual_link')
    ),
    source_snapshot_digest BLOB NOT NULL CHECK (length(source_snapshot_digest) = 32),
    source_repository_path BLOB NOT NULL CHECK (
        length(source_repository_path) BETWEEN 1 AND 32764
        AND instr(source_repository_path, X'00') = 0
    ),
    source_artifact_digest BLOB NOT NULL CHECK (length(source_artifact_digest) = 32),
    source_fact_ordinal INTEGER NOT NULL
        CHECK (source_fact_ordinal BETWEEN 0 AND 9007199254740991),
    target_snapshot_digest BLOB NOT NULL CHECK (length(target_snapshot_digest) = 32),
    target_repository_path BLOB NOT NULL CHECK (
        length(target_repository_path) BETWEEN 1 AND 32764
        AND instr(target_repository_path, X'00') = 0
    ),
    target_artifact_digest BLOB NOT NULL CHECK (length(target_artifact_digest) = 32),
    target_fact_ordinal INTEGER NOT NULL
        CHECK (target_fact_ordinal BETWEEN 0 AND 9007199254740991),
    method_id TEXT NOT NULL CHECK (
        length(CAST(method_id AS BLOB)) BETWEEN 1 AND 128
        AND method_id NOT GLOB '*[^ -~]*'
    ),
    method_version INTEGER NOT NULL
        CHECK (method_version BETWEEN 1 AND 4294967295),
    trusted_actor_kind TEXT NOT NULL CHECK (trusted_actor_kind = 'local_asserted'),
    trusted_actor_id TEXT NOT NULL CHECK (
        length(CAST(trusted_actor_id AS BLOB)) BETWEEN 1 AND 128
        AND trusted_actor_id NOT GLOB '*[^ -~]*'
    ),
    recorded_at_unix_ms INTEGER NOT NULL CHECK (recorded_at_unix_ms >= 0),
    FOREIGN KEY (
        workspace_id, record_id, revision_digest, evidence_ordinal,
        source_snapshot_digest, source_repository_path,
        source_artifact_digest, source_fact_ordinal
    ) REFERENCES memory_evidence(
        workspace_id, record_id, revision_digest, ordinal,
        source_snapshot_digest, repository_path, artifact_digest, fact_ordinal
    ),
    FOREIGN KEY (source_artifact_digest, source_fact_ordinal)
        REFERENCES artifact_facts(artifact_digest, ordinal),
    FOREIGN KEY (target_artifact_digest, target_fact_ordinal)
        REFERENCES artifact_facts(artifact_digest, ordinal)
) STRICT;

CREATE UNIQUE INDEX memory_evidence_occurrence_identity
ON memory_evidence(
    workspace_id, record_id, revision_digest, ordinal,
    source_snapshot_digest, repository_path, artifact_digest, fact_ordinal
);
CREATE UNIQUE INDEX unique_memory_correspondence_event
ON memory_correspondence_audit(
    workspace_id, record_id, revision_digest, evidence_ordinal, operation,
    source_snapshot_digest, source_repository_path,
    source_artifact_digest, source_fact_ordinal,
    target_snapshot_digest, target_repository_path,
    target_artifact_digest, target_fact_ordinal,
    method_id, method_version, trusted_actor_kind, trusted_actor_id
);
CREATE UNIQUE INDEX unique_memory_local_approval
ON memory_audit(
    workspace_id, record_id, revision_digest, trusted_actor_kind, trusted_actor_id
)
WHERE operation = 'locally_approved';
CREATE UNIQUE INDEX unique_memory_observation
ON memory_audit(
    workspace_id, record_id, revision_digest, source_kind, source_format,
    source_revision, presentation_digest, trusted_actor_kind, trusted_actor_id
)
WHERE operation = 'observed';

CREATE TRIGGER memory_audit_no_delete
BEFORE DELETE ON memory_audit BEGIN
    SELECT RAISE(ABORT, 'append-only memory audit');
END;
CREATE TRIGGER memory_audit_no_update
BEFORE UPDATE ON memory_audit BEGIN
    SELECT RAISE(ABORT, 'append-only memory audit');
END;
CREATE TRIGGER memory_correspondence_audit_no_delete
BEFORE DELETE ON memory_correspondence_audit BEGIN
    SELECT RAISE(ABORT, 'append-only correspondence audit');
END;
CREATE TRIGGER memory_correspondence_audit_no_update
BEFORE UPDATE ON memory_correspondence_audit BEGIN
    SELECT RAISE(ABORT, 'append-only correspondence audit');
END;
CREATE TRIGGER memory_correspondence_audit_validate_occurrences
BEFORE INSERT ON memory_correspondence_audit
WHEN
    NOT EXISTS (
        SELECT 1
        FROM index_generations AS generation
        JOIN generation_files AS file
          ON file.generation_id = generation.generation_id
        WHERE generation.snapshot_digest = NEW.source_snapshot_digest
          AND file.repository_path = NEW.source_repository_path
          AND file.artifact_digest = NEW.source_artifact_digest
    )
    OR NOT EXISTS (
        SELECT 1
        FROM index_generations AS generation
        JOIN generation_files AS file
          ON file.generation_id = generation.generation_id
        WHERE generation.snapshot_digest = NEW.target_snapshot_digest
          AND file.repository_path = NEW.target_repository_path
          AND file.artifact_digest = NEW.target_artifact_digest
    )
BEGIN
    SELECT RAISE(ABORT, 'correspondence occurrence is unavailable');
END;
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
CREATE TRIGGER memory_evidence_no_delete
BEFORE DELETE ON memory_evidence BEGIN
    SELECT RAISE(ABORT, 'immutable memory evidence');
END;
CREATE TRIGGER memory_evidence_no_update
BEFORE UPDATE ON memory_evidence BEGIN
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
CREATE TRIGGER memory_relationships_no_delete
BEFORE DELETE ON memory_relationships BEGIN
    SELECT RAISE(ABORT, 'immutable memory relationships');
END;
CREATE TRIGGER memory_relationships_no_update
BEFORE UPDATE ON memory_relationships BEGIN
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
CREATE TRIGGER memory_validity_commits_no_delete
BEFORE DELETE ON memory_validity_commits BEGIN
    SELECT RAISE(ABORT, 'immutable memory validity');
END;
CREATE TRIGGER memory_validity_commits_no_update
BEFORE UPDATE ON memory_validity_commits BEGIN
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
CREATE TRIGGER memory_version_parents_no_delete
BEFORE DELETE ON memory_version_parents BEGIN
    SELECT RAISE(ABORT, 'immutable memory version parents');
END;
CREATE TRIGGER memory_version_parents_no_update
BEFORE UPDATE ON memory_version_parents BEGIN
    SELECT RAISE(ABORT, 'immutable memory version parents');
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
