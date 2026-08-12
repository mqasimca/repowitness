-- RepoWitness Phase 3: append-only storage for the separately versioned
-- additional team-memory profile. Version-1 tables remain unchanged.

CREATE TABLE memory_profile_v2_versions (
    workspace_id INTEGER NOT NULL REFERENCES workspaces(workspace_id),
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB NOT NULL CHECK (length(revision_digest) = 32),
    schema_version INTEGER NOT NULL CHECK (schema_version = 2),
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
    PRIMARY KEY (workspace_id, record_id, revision_digest)
) STRICT, WITHOUT ROWID;

CREATE TABLE memory_profile_v2_audit (
    event_id INTEGER PRIMARY KEY CHECK (event_id > 0),
    workspace_id INTEGER NOT NULL,
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB NOT NULL CHECK (length(revision_digest) = 32),
    operation TEXT NOT NULL CHECK (operation IN ('observed', 'locally_approved')),
    trusted_actor_kind TEXT NOT NULL CHECK (trusted_actor_kind = 'local_asserted'),
    trusted_actor_id TEXT NOT NULL CHECK (length(CAST(trusted_actor_id AS BLOB)) BETWEEN 1 AND 128),
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
        REFERENCES memory_profile_v2_versions(workspace_id, record_id, revision_digest)
) STRICT;

CREATE TABLE memory_profile_v2_parents (
    workspace_id INTEGER NOT NULL,
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB NOT NULL CHECK (length(revision_digest) = 32),
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 7),
    parent_revision_digest BLOB NOT NULL CHECK (length(parent_revision_digest) = 32),
    PRIMARY KEY (workspace_id, record_id, revision_digest, ordinal),
    UNIQUE (workspace_id, record_id, revision_digest, parent_revision_digest),
    FOREIGN KEY (workspace_id, record_id, revision_digest)
        REFERENCES memory_profile_v2_versions(workspace_id, record_id, revision_digest)
) STRICT, WITHOUT ROWID;

CREATE UNIQUE INDEX unique_memory_profile_v2_observation
ON memory_profile_v2_audit(
    workspace_id, record_id, revision_digest, source_kind, source_format,
    source_revision, presentation_digest, trusted_actor_kind, trusted_actor_id
)
WHERE operation = 'observed';

CREATE UNIQUE INDEX unique_memory_profile_v2_local_approval
ON memory_profile_v2_audit(
    workspace_id, record_id, revision_digest, trusted_actor_kind, trusted_actor_id
)
WHERE operation = 'locally_approved';

CREATE TRIGGER memory_profile_v2_versions_no_delete
BEFORE DELETE ON memory_profile_v2_versions BEGIN
    SELECT RAISE(ABORT, 'immutable profile v2 memory versions');
END;
CREATE TRIGGER memory_profile_v2_versions_no_update
BEFORE UPDATE ON memory_profile_v2_versions BEGIN
    SELECT RAISE(ABORT, 'immutable profile v2 memory versions');
END;
CREATE TRIGGER memory_profile_v2_audit_no_delete
BEFORE DELETE ON memory_profile_v2_audit BEGIN
    SELECT RAISE(ABORT, 'append-only profile v2 memory audit');
END;
CREATE TRIGGER memory_profile_v2_audit_no_update
BEFORE UPDATE ON memory_profile_v2_audit BEGIN
    SELECT RAISE(ABORT, 'append-only profile v2 memory audit');
END;
CREATE TRIGGER memory_profile_v2_parents_no_delete
BEFORE DELETE ON memory_profile_v2_parents BEGIN
    SELECT RAISE(ABORT, 'immutable profile v2 memory parents');
END;
CREATE TRIGGER memory_profile_v2_parents_no_update
BEFORE UPDATE ON memory_profile_v2_parents BEGIN
    SELECT RAISE(ABORT, 'immutable profile v2 memory parents');
END;
