-- RepoWitness Phase 3: isolated local personal memory and durable work receipts.
-- Team-memory version-1 tables remain immutable; these tables never share their
-- record IDs, audit events, or default retrieval paths.

CREATE TABLE personal_memory_records (
    profile_id BLOB NOT NULL CHECK (length(profile_id) = 16),
    repository_identity BLOB NOT NULL CHECK (length(repository_identity) = 32),
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB NOT NULL CHECK (length(revision_digest) = 32),
    kind TEXT NOT NULL CHECK (
        kind IN (
            'fact', 'decision', 'procedure', 'episode', 'failure',
            'preference', 'policy'
        )
    ),
    title TEXT NOT NULL CHECK (
        length(CAST(title AS BLOB)) BETWEEN 1 AND 4096
        AND instr(title, char(0)) = 0
        AND instr(title, char(13)) = 0
    ),
    body TEXT NOT NULL CHECK (
        length(CAST(body AS BLOB)) BETWEEN 1 AND 4096
        AND instr(body, char(0)) = 0
        AND instr(body, char(13)) = 0
    ),
    lifecycle TEXT NOT NULL CHECK (
        lifecycle IN (
            'active', 'needs_review', 'stale', 'contradicted', 'superseded',
            'quarantined', 'tombstoned'
        )
    ),
    recorded_at_unix_ms INTEGER NOT NULL CHECK (recorded_at_unix_ms >= 0),
    PRIMARY KEY (profile_id, repository_identity, record_id, revision_digest)
) STRICT, WITHOUT ROWID;

CREATE TABLE personal_memory_audit (
    event_id INTEGER PRIMARY KEY CHECK (event_id > 0),
    profile_id BLOB NOT NULL CHECK (length(profile_id) = 16),
    repository_identity BLOB NOT NULL CHECK (length(repository_identity) = 32),
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB NOT NULL CHECK (length(revision_digest) = 32),
    operation TEXT NOT NULL CHECK (operation = 'recorded'),
    recorded_at_unix_ms INTEGER NOT NULL CHECK (recorded_at_unix_ms >= 0),
    FOREIGN KEY (profile_id, repository_identity, record_id, revision_digest)
        REFERENCES personal_memory_records(profile_id, repository_identity, record_id, revision_digest)
) STRICT;

CREATE TRIGGER personal_memory_records_no_delete
BEFORE DELETE ON personal_memory_records BEGIN
    SELECT RAISE(ABORT, 'immutable personal memory revisions');
END;
CREATE TRIGGER personal_memory_records_no_update
BEFORE UPDATE ON personal_memory_records BEGIN
    SELECT RAISE(ABORT, 'immutable personal memory revisions');
END;

CREATE TRIGGER personal_memory_audit_no_delete
BEFORE DELETE ON personal_memory_audit BEGIN
    SELECT RAISE(ABORT, 'append-only personal memory audit');
END;
CREATE TRIGGER personal_memory_audit_no_update
BEFORE UPDATE ON personal_memory_audit BEGIN
    SELECT RAISE(ABORT, 'append-only personal memory audit');
END;

CREATE TABLE engineering_tasks (
    task_id BLOB PRIMARY KEY CHECK (length(task_id) = 16),
    repository_identity BLOB NOT NULL CHECK (length(repository_identity) = 32),
    created_at_unix_ms INTEGER NOT NULL CHECK (created_at_unix_ms >= 0)
) STRICT;

CREATE TABLE engineering_task_checkpoints (
    task_id BLOB NOT NULL CHECK (length(task_id) = 16),
    sequence INTEGER NOT NULL CHECK (sequence BETWEEN 1 AND 4096),
    state TEXT NOT NULL CHECK (state IN ('open', 'blocked', 'completed', 'cancelled')),
    objective TEXT NOT NULL CHECK (
        length(CAST(objective AS BLOB)) BETWEEN 1 AND 4096
        AND instr(objective, char(0)) = 0
        AND instr(objective, char(13)) = 0
    ),
    hypothesis TEXT CHECK (
        length(CAST(hypothesis AS BLOB)) BETWEEN 1 AND 4096
        AND instr(hypothesis, char(0)) = 0
        AND instr(hypothesis, char(13)) = 0
    ),
    next_safe_action TEXT CHECK (
        length(CAST(next_safe_action AS BLOB)) BETWEEN 1 AND 4096
        AND instr(next_safe_action, char(0)) = 0
        AND instr(next_safe_action, char(13)) = 0
    ),
    recorded_at_unix_ms INTEGER NOT NULL CHECK (recorded_at_unix_ms >= 0),
    PRIMARY KEY (task_id, sequence),
    FOREIGN KEY (task_id) REFERENCES engineering_tasks(task_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE engineering_task_verifications (
    verification_id INTEGER PRIMARY KEY CHECK (verification_id > 0),
    task_id BLOB NOT NULL CHECK (length(task_id) = 16),
    checkpoint_sequence INTEGER NOT NULL CHECK (checkpoint_sequence BETWEEN 1 AND 4096),
    source_snapshot_digest BLOB NOT NULL CHECK (length(source_snapshot_digest) = 32),
    check_identity TEXT NOT NULL CHECK (
        length(CAST(check_identity AS BLOB)) BETWEEN 1 AND 4096
        AND instr(check_identity, char(0)) = 0
        AND instr(check_identity, char(13)) = 0
    ),
    producer TEXT NOT NULL CHECK (
        length(CAST(producer AS BLOB)) BETWEEN 1 AND 4096
        AND instr(producer, char(0)) = 0
        AND instr(producer, char(13)) = 0
    ),
    configuration_digest BLOB NOT NULL CHECK (length(configuration_digest) = 32),
    outcome TEXT NOT NULL CHECK (outcome IN ('passed', 'failed', 'cancelled', 'incomplete')),
    captured_output_digest BLOB NOT NULL CHECK (length(captured_output_digest) = 32),
    captured_output_bytes INTEGER NOT NULL CHECK (
        captured_output_bytes BETWEEN 0 AND 16777216
    ),
    recorded_at_unix_ms INTEGER NOT NULL CHECK (recorded_at_unix_ms >= 0),
    FOREIGN KEY (task_id, checkpoint_sequence)
        REFERENCES engineering_task_checkpoints(task_id, sequence)
) STRICT;

CREATE TRIGGER engineering_task_checkpoints_no_delete
BEFORE DELETE ON engineering_task_checkpoints BEGIN
    SELECT RAISE(ABORT, 'append-only task checkpoints');
END;
CREATE TRIGGER engineering_task_checkpoints_no_update
BEFORE UPDATE ON engineering_task_checkpoints BEGIN
    SELECT RAISE(ABORT, 'append-only task checkpoints');
END;
CREATE TRIGGER engineering_task_verifications_no_delete
BEFORE DELETE ON engineering_task_verifications BEGIN
    SELECT RAISE(ABORT, 'append-only task verifications');
END;
CREATE TRIGGER engineering_task_verifications_no_update
BEFORE UPDATE ON engineering_task_verifications BEGIN
    SELECT RAISE(ABORT, 'append-only task verifications');
END;
