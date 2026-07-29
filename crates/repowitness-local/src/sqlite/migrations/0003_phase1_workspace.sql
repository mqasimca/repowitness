-- RepoWitness Phase 1 workspace identities and immutable multi-source views.

CREATE UNIQUE INDEX workspaces_by_id_and_repository
ON workspaces(workspace_id, repository_identity);

CREATE TABLE connected_workspaces (
    connected_workspace_id BLOB PRIMARY KEY
        CHECK (length(connected_workspace_id) = 32)
) STRICT, WITHOUT ROWID;

CREATE TABLE workspace_source_slots (
    connected_workspace_id BLOB NOT NULL
        REFERENCES connected_workspaces(connected_workspace_id),
    source_slot_id BLOB NOT NULL CHECK (length(source_slot_id) = 32),
    repository_identity BLOB NOT NULL CHECK (length(repository_identity) = 32),
    generation_workspace_id INTEGER NOT NULL CHECK (generation_workspace_id > 0),
    source_epoch INTEGER NOT NULL DEFAULT 0 CHECK (source_epoch >= 0),
    PRIMARY KEY (connected_workspace_id, source_slot_id),
    UNIQUE (source_slot_id),
    UNIQUE (
        connected_workspace_id, source_slot_id, generation_workspace_id
    ),
    FOREIGN KEY (generation_workspace_id, repository_identity)
        REFERENCES workspaces(workspace_id, repository_identity)
) STRICT, WITHOUT ROWID;

CREATE TABLE source_slot_generation_receipts (
    connected_workspace_id BLOB NOT NULL
        CHECK (length(connected_workspace_id) = 32),
    source_slot_id BLOB NOT NULL CHECK (length(source_slot_id) = 32),
    source_epoch INTEGER NOT NULL CHECK (source_epoch >= 0),
    generation_workspace_id INTEGER NOT NULL CHECK (generation_workspace_id > 0),
    generation_id INTEGER NOT NULL CHECK (generation_id > 0),
    PRIMARY KEY (source_slot_id, source_epoch),
    UNIQUE (
        connected_workspace_id, source_slot_id, source_epoch,
        generation_workspace_id, generation_id
    ),
    FOREIGN KEY (
        connected_workspace_id, source_slot_id, generation_workspace_id
    ) REFERENCES workspace_source_slots(
        connected_workspace_id, source_slot_id, generation_workspace_id
    ),
    FOREIGN KEY (generation_workspace_id, generation_id)
        REFERENCES index_generations(workspace_id, generation_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE workspace_views (
    workspace_view_id INTEGER PRIMARY KEY CHECK (workspace_view_id > 0),
    connected_workspace_id BLOB NOT NULL
        REFERENCES connected_workspaces(connected_workspace_id),
    lifecycle_state TEXT NOT NULL
        CHECK (lifecycle_state IN ('staging', 'published')),
    UNIQUE (connected_workspace_id, workspace_view_id)
) STRICT;

CREATE TABLE workspace_view_members (
    workspace_view_id INTEGER NOT NULL,
    connected_workspace_id BLOB NOT NULL
        CHECK (length(connected_workspace_id) = 32),
    source_slot_id BLOB NOT NULL CHECK (length(source_slot_id) = 32),
    source_epoch INTEGER NOT NULL CHECK (source_epoch >= 0),
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 255),
    generation_workspace_id INTEGER NOT NULL CHECK (generation_workspace_id > 0),
    generation_id INTEGER NOT NULL CHECK (generation_id > 0),
    PRIMARY KEY (workspace_view_id, ordinal),
    UNIQUE (workspace_view_id, source_slot_id),
    FOREIGN KEY (connected_workspace_id, workspace_view_id)
        REFERENCES workspace_views(connected_workspace_id, workspace_view_id),
    FOREIGN KEY (
        connected_workspace_id, source_slot_id, generation_workspace_id
    ) REFERENCES workspace_source_slots(
        connected_workspace_id, source_slot_id, generation_workspace_id
    ),
    FOREIGN KEY (generation_workspace_id, generation_id)
        REFERENCES index_generations(workspace_id, generation_id),
    FOREIGN KEY (
        connected_workspace_id, source_slot_id, source_epoch,
        generation_workspace_id, generation_id
    ) REFERENCES source_slot_generation_receipts(
        connected_workspace_id, source_slot_id, source_epoch,
        generation_workspace_id, generation_id
    )
) STRICT, WITHOUT ROWID;

CREATE TABLE active_workspace_views (
    connected_workspace_id BLOB PRIMARY KEY
        CHECK (length(connected_workspace_id) = 32),
    workspace_view_id INTEGER NOT NULL CHECK (workspace_view_id > 0),
    FOREIGN KEY (connected_workspace_id, workspace_view_id)
        REFERENCES workspace_views(connected_workspace_id, workspace_view_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX workspace_view_members_by_generation
ON workspace_view_members(generation_workspace_id, generation_id);

CREATE INDEX source_slot_receipts_by_generation
ON source_slot_generation_receipts(generation_workspace_id, generation_id);

CREATE TRIGGER connected_workspaces_no_update
BEFORE UPDATE ON connected_workspaces BEGIN
    SELECT RAISE(ABORT, 'immutable connected workspace identity');
END;

CREATE TRIGGER connected_workspaces_no_delete
BEFORE DELETE ON connected_workspaces BEGIN
    SELECT RAISE(ABORT, 'immutable connected workspace identity');
END;

CREATE TRIGGER workspace_source_slots_bounded
BEFORE INSERT ON workspace_source_slots
WHEN (
    SELECT count(*) FROM workspace_source_slots
    WHERE connected_workspace_id = NEW.connected_workspace_id
) >= 256
BEGIN
    SELECT RAISE(ABORT, 'workspace source-slot limit exceeded');
END;

CREATE TRIGGER workspace_source_slots_frozen_after_first_view
BEFORE INSERT ON workspace_source_slots
WHEN EXISTS (
    SELECT 1 FROM workspace_views
    WHERE connected_workspace_id = NEW.connected_workspace_id
      AND lifecycle_state = 'published'
)
BEGIN
    SELECT RAISE(ABORT, 'workspace source-slot membership is frozen');
END;

CREATE TRIGGER workspace_source_slots_mapping_no_update
BEFORE UPDATE OF
    connected_workspace_id, source_slot_id, repository_identity,
    generation_workspace_id
ON workspace_source_slots BEGIN
    SELECT RAISE(ABORT, 'immutable workspace source-slot mapping');
END;

CREATE TRIGGER workspace_source_slots_epoch_monotonic
BEFORE UPDATE OF source_epoch ON workspace_source_slots
WHEN OLD.source_epoch = 9223372036854775807
  OR NEW.source_epoch != OLD.source_epoch + 1
BEGIN
    SELECT RAISE(ABORT, 'invalid workspace source-slot epoch transition');
END;

CREATE TRIGGER workspace_source_slots_no_delete
BEFORE DELETE ON workspace_source_slots
WHEN EXISTS (
    SELECT 1 FROM workspace_views
    WHERE connected_workspace_id = OLD.connected_workspace_id
      AND lifecycle_state = 'published'
)
OR EXISTS (
    SELECT 1 FROM active_workspace_views
    WHERE connected_workspace_id = OLD.connected_workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'immutable workspace source-slot mapping');
END;

CREATE TRIGGER source_slot_generation_receipts_current_epoch
BEFORE INSERT ON source_slot_generation_receipts
WHEN NOT EXISTS (
    SELECT 1
    FROM workspace_source_slots AS slot
    JOIN index_generations AS generation
      ON generation.workspace_id = NEW.generation_workspace_id
     AND generation.generation_id = NEW.generation_id
    WHERE slot.connected_workspace_id = NEW.connected_workspace_id
      AND slot.source_slot_id = NEW.source_slot_id
      AND slot.generation_workspace_id = NEW.generation_workspace_id
      AND slot.source_epoch = NEW.source_epoch
      AND generation.lifecycle_state IN ('ready', 'active', 'retained')
)
BEGIN
    SELECT RAISE(ABORT, 'source-slot completion is stale or ineligible');
END;

CREATE TRIGGER source_slot_generation_receipts_no_update
BEFORE UPDATE ON source_slot_generation_receipts BEGIN
    SELECT RAISE(ABORT, 'immutable source-slot generation receipt');
END;

CREATE TRIGGER source_slot_generation_receipts_no_delete
BEFORE DELETE ON source_slot_generation_receipts
WHEN EXISTS (
    SELECT 1 FROM workspace_views
    WHERE connected_workspace_id = OLD.connected_workspace_id
      AND lifecycle_state = 'published'
)
OR EXISTS (
    SELECT 1 FROM active_workspace_views
    WHERE connected_workspace_id = OLD.connected_workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'immutable source-slot generation receipt');
END;

CREATE TRIGGER workspace_views_insert_staging_only
BEFORE INSERT ON workspace_views
WHEN NEW.lifecycle_state != 'staging'
BEGIN
    SELECT RAISE(ABORT, 'workspace view must begin staging');
END;

CREATE TRIGGER workspace_views_no_semantic_update
BEFORE UPDATE OF workspace_view_id, connected_workspace_id ON workspace_views BEGIN
    SELECT RAISE(ABORT, 'immutable workspace view identity');
END;

CREATE TRIGGER workspace_view_lifecycle_transition
BEFORE UPDATE OF lifecycle_state ON workspace_views
WHEN NOT (
    OLD.lifecycle_state = 'staging' AND NEW.lifecycle_state = 'published'
)
BEGIN
    SELECT RAISE(ABORT, 'invalid workspace view lifecycle transition');
END;

CREATE TRIGGER workspace_view_publication_requires_complete_membership
BEFORE UPDATE OF lifecycle_state ON workspace_views
WHEN NEW.lifecycle_state = 'published'
AND (
    (SELECT count(*) FROM workspace_source_slots
     WHERE connected_workspace_id = NEW.connected_workspace_id) = 0
    OR
    (SELECT count(*) FROM workspace_source_slots
     WHERE connected_workspace_id = NEW.connected_workspace_id) !=
    (SELECT count(*) FROM workspace_view_members
     WHERE workspace_view_id = NEW.workspace_view_id)
    OR EXISTS (
        SELECT 1
        FROM workspace_source_slots AS slot
        WHERE slot.connected_workspace_id = NEW.connected_workspace_id
          AND NOT EXISTS (
              SELECT 1
              FROM workspace_view_members AS member
              WHERE member.workspace_view_id = NEW.workspace_view_id
                AND member.source_slot_id = slot.source_slot_id
          )
    )
    OR EXISTS (
        SELECT 1
        FROM workspace_view_members AS member
        JOIN workspace_source_slots AS slot
          ON slot.connected_workspace_id = member.connected_workspace_id
         AND slot.source_slot_id = member.source_slot_id
        WHERE member.workspace_view_id = NEW.workspace_view_id
          AND member.source_epoch != slot.source_epoch
    )
    OR EXISTS (
        SELECT 1
        FROM workspace_view_members AS member
        JOIN index_generations AS generation
          ON generation.workspace_id = member.generation_workspace_id
         AND generation.generation_id = member.generation_id
        WHERE member.workspace_view_id = NEW.workspace_view_id
          AND generation.lifecycle_state NOT IN ('ready', 'active', 'retained')
    )
    OR EXISTS (
        SELECT 1
        FROM workspace_view_members AS member
        WHERE member.workspace_view_id = NEW.workspace_view_id
          AND member.ordinal != (
              SELECT count(*)
              FROM workspace_view_members AS prior
              WHERE prior.workspace_view_id = member.workspace_view_id
                AND prior.source_slot_id < member.source_slot_id
          )
    )
)
BEGIN
    SELECT RAISE(ABORT, 'workspace view is incomplete or ineligible');
END;

CREATE TRIGGER workspace_views_delete_staging_only
BEFORE DELETE ON workspace_views
WHEN OLD.lifecycle_state != 'staging'
BEGIN
    SELECT RAISE(ABORT, 'published workspace view is immutable');
END;

CREATE TRIGGER workspace_view_members_insert_staging_only
BEFORE INSERT ON workspace_view_members
WHEN (
    SELECT lifecycle_state FROM workspace_views
    WHERE connected_workspace_id = NEW.connected_workspace_id
      AND workspace_view_id = NEW.workspace_view_id
) != 'staging'
BEGIN
    SELECT RAISE(ABORT, 'workspace view is not accepting members');
END;

CREATE TRIGGER workspace_view_members_no_update
BEFORE UPDATE ON workspace_view_members BEGIN
    SELECT RAISE(ABORT, 'immutable workspace view member');
END;

CREATE TRIGGER workspace_view_members_delete_staging_only
BEFORE DELETE ON workspace_view_members
WHEN (
    SELECT lifecycle_state FROM workspace_views
    WHERE connected_workspace_id = OLD.connected_workspace_id
      AND workspace_view_id = OLD.workspace_view_id
) != 'staging'
BEGIN
    SELECT RAISE(ABORT, 'published workspace view member is immutable');
END;

CREATE TRIGGER active_workspace_views_require_published_insert
BEFORE INSERT ON active_workspace_views
WHEN (
    SELECT lifecycle_state FROM workspace_views
    WHERE connected_workspace_id = NEW.connected_workspace_id
      AND workspace_view_id = NEW.workspace_view_id
) != 'published'
BEGIN
    SELECT RAISE(ABORT, 'workspace view is not published');
END;

CREATE TRIGGER active_workspace_views_require_published_update
BEFORE UPDATE ON active_workspace_views
WHEN (
    SELECT lifecycle_state FROM workspace_views
    WHERE connected_workspace_id = NEW.connected_workspace_id
      AND workspace_view_id = NEW.workspace_view_id
) != 'published'
BEGIN
    SELECT RAISE(ABORT, 'workspace view is not published');
END;

CREATE TRIGGER active_workspace_views_no_identity_update
BEFORE UPDATE OF connected_workspace_id ON active_workspace_views BEGIN
    SELECT RAISE(ABORT, 'immutable active workspace pointer identity');
END;

CREATE TRIGGER active_workspace_views_no_delete
BEFORE DELETE ON active_workspace_views BEGIN
    SELECT RAISE(ABORT, 'active workspace view pointer is required');
END;

CREATE TRIGGER active_workspace_view_generations_cannot_fail
BEFORE UPDATE OF lifecycle_state ON index_generations
WHEN NEW.lifecycle_state IN ('failed', 'cancelled')
AND EXISTS (
    SELECT 1
    FROM workspace_view_members AS member
    JOIN active_workspace_views AS active
      ON active.connected_workspace_id = member.connected_workspace_id
     AND active.workspace_view_id = member.workspace_view_id
    WHERE member.generation_workspace_id = OLD.workspace_id
      AND member.generation_id = OLD.generation_id
)
BEGIN
    SELECT RAISE(ABORT, 'active workspace view pins generation');
END;

INSERT INTO connected_workspaces(connected_workspace_id)
SELECT repository_identity
FROM workspaces
ORDER BY workspace_id;

INSERT INTO workspace_source_slots(
    connected_workspace_id, source_slot_id, repository_identity,
    generation_workspace_id, source_epoch
)
SELECT
    repository_identity, repository_identity, repository_identity, workspace_id,
    source_epoch
FROM workspaces
ORDER BY workspace_id;

INSERT INTO source_slot_generation_receipts(
    connected_workspace_id, source_slot_id, source_epoch,
    generation_workspace_id, generation_id
)
SELECT
    repository_identity, repository_identity, source_epoch,
    workspace_id, active_generation_id
FROM workspaces
WHERE active_generation_id IS NOT NULL
ORDER BY workspace_id;

INSERT INTO workspace_views(
    workspace_view_id, connected_workspace_id, lifecycle_state
)
SELECT workspace_id, repository_identity, 'staging'
FROM workspaces
WHERE active_generation_id IS NOT NULL
ORDER BY workspace_id;

INSERT INTO workspace_view_members(
    workspace_view_id, connected_workspace_id, source_slot_id, source_epoch, ordinal,
    generation_workspace_id, generation_id
)
SELECT
    workspace_id, repository_identity, repository_identity, source_epoch, 0,
    workspace_id, active_generation_id
FROM workspaces
WHERE active_generation_id IS NOT NULL
ORDER BY workspace_id;

UPDATE workspace_views SET lifecycle_state = 'published';

INSERT INTO active_workspace_views(
    connected_workspace_id, workspace_view_id
)
SELECT connected_workspace_id, workspace_view_id
FROM workspace_views
WHERE lifecycle_state = 'published'
ORDER BY workspace_view_id;
