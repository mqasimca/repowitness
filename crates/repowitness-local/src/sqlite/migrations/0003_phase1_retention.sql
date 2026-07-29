-- RepoWitness Phase 1 bounded retention and garbage-collection lifecycle.
--
-- Accepted migrations 1 and 2 remain byte-identical. Their core lifecycle
-- CHECK constraints cannot gain an inline `garbage` value without rewriting
-- accepted history, so migration 3 represents each authorized garbage
-- transition in a typed mark relation. Marks and deletion are owned by one
-- immediate writer transaction.

CREATE TABLE retention_generation_garbage (
    generation_id INTEGER PRIMARY KEY
        REFERENCES index_generations(generation_id) ON DELETE CASCADE,
    plan_digest BLOB NOT NULL CHECK (length(plan_digest) = 32),
    lifecycle_state TEXT NOT NULL CHECK (lifecycle_state = 'garbage')
) STRICT, WITHOUT ROWID;

CREATE TABLE retention_snapshot_garbage (
    snapshot_digest BLOB PRIMARY KEY
        REFERENCES source_snapshots(snapshot_digest) ON DELETE CASCADE,
    plan_digest BLOB NOT NULL CHECK (length(plan_digest) = 32),
    lifecycle_state TEXT NOT NULL CHECK (lifecycle_state = 'garbage')
) STRICT, WITHOUT ROWID;

CREATE TABLE retention_artifact_garbage (
    artifact_digest BLOB PRIMARY KEY
        REFERENCES analysis_artifacts(artifact_digest) ON DELETE CASCADE,
    plan_digest BLOB NOT NULL CHECK (length(plan_digest) = 32),
    lifecycle_state TEXT NOT NULL CHECK (lifecycle_state = 'garbage')
) STRICT, WITHOUT ROWID;

CREATE TABLE retention_workspace_view_garbage (
    workspace_view_id INTEGER PRIMARY KEY
        REFERENCES workspace_views(workspace_view_id) ON DELETE CASCADE,
    plan_digest BLOB NOT NULL CHECK (length(plan_digest) = 32),
    lifecycle_state TEXT NOT NULL CHECK (lifecycle_state = 'garbage')
) STRICT, WITHOUT ROWID;

CREATE TABLE retention_source_slot_receipt_garbage (
    source_slot_id BLOB NOT NULL CHECK (length(source_slot_id) = 32),
    source_epoch INTEGER NOT NULL CHECK (source_epoch >= 0),
    plan_digest BLOB NOT NULL CHECK (length(plan_digest) = 32),
    lifecycle_state TEXT NOT NULL CHECK (lifecycle_state = 'garbage'),
    PRIMARY KEY (source_slot_id, source_epoch),
    FOREIGN KEY (source_slot_id, source_epoch)
        REFERENCES source_slot_generation_receipts(source_slot_id, source_epoch)
        ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

CREATE TABLE retention_collection_audit (
    collection_id INTEGER PRIMARY KEY CHECK (collection_id > 0),
    policy_digest BLOB NOT NULL CHECK (length(policy_digest) = 32),
    plan_digest BLOB NOT NULL CHECK (length(plan_digest) = 32),
    generation_count INTEGER NOT NULL CHECK (generation_count >= 0),
    workspace_view_count INTEGER NOT NULL CHECK (workspace_view_count >= 0),
    source_slot_receipt_count INTEGER NOT NULL
        CHECK (source_slot_receipt_count >= 0),
    snapshot_count INTEGER NOT NULL CHECK (snapshot_count >= 0),
    artifact_count INTEGER NOT NULL CHECK (artifact_count >= 0),
    deleted_row_count INTEGER NOT NULL CHECK (deleted_row_count >= 0),
    estimated_deleted_bytes INTEGER NOT NULL
        CHECK (estimated_deleted_bytes >= 0),
    more_work INTEGER NOT NULL CHECK (more_work IN (0, 1)),
    outcome TEXT NOT NULL CHECK (outcome IN ('applied', 'no_op')),
    UNIQUE (policy_digest, plan_digest)
) STRICT;

CREATE TRIGGER retention_generation_garbage_validate_insert
BEFORE INSERT ON retention_generation_garbage
WHEN
    (SELECT lifecycle_state FROM index_generations
     WHERE generation_id = NEW.generation_id) != 'retained'
    OR EXISTS (
        SELECT 1 FROM workspaces
        WHERE active_generation_id = NEW.generation_id
    )
    OR EXISTS (
        SELECT 1
        FROM workspace_view_members AS member
        JOIN active_workspace_views AS active
          ON active.connected_workspace_id = member.connected_workspace_id
         AND active.workspace_view_id = member.workspace_view_id
        WHERE member.generation_id = NEW.generation_id
    )
    OR EXISTS (
        SELECT 1
        FROM source_slot_generation_receipts AS receipt
        JOIN workspace_source_slots AS slot
          ON slot.connected_workspace_id = receipt.connected_workspace_id
         AND slot.source_slot_id = receipt.source_slot_id
         AND slot.source_epoch = receipt.source_epoch
        WHERE receipt.generation_id = NEW.generation_id
    )
    OR EXISTS (
        SELECT 1 FROM memory_projection_generations
        WHERE index_generation_id = NEW.generation_id
    )
BEGIN
    SELECT RAISE(ABORT, 'generation is a retention root');
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

CREATE TRIGGER retention_workspace_view_garbage_validate_insert
BEFORE INSERT ON retention_workspace_view_garbage
WHEN
    (SELECT lifecycle_state FROM workspace_views
     WHERE workspace_view_id = NEW.workspace_view_id) != 'published'
    OR EXISTS (
        SELECT 1 FROM active_workspace_views
        WHERE workspace_view_id = NEW.workspace_view_id
    )
BEGIN
    SELECT RAISE(ABORT, 'workspace view is a retention root');
END;

CREATE TRIGGER retention_source_slot_receipt_garbage_validate_insert
BEFORE INSERT ON retention_source_slot_receipt_garbage
WHEN EXISTS (
    SELECT 1
    FROM source_slot_generation_receipts AS receipt
    JOIN workspace_source_slots AS slot
      ON slot.connected_workspace_id = receipt.connected_workspace_id
     AND slot.source_slot_id = receipt.source_slot_id
    WHERE receipt.source_slot_id = NEW.source_slot_id
      AND receipt.source_epoch = NEW.source_epoch
      AND receipt.source_epoch = slot.source_epoch
)
BEGIN
    SELECT RAISE(ABORT, 'source-slot receipt is current');
END;

CREATE TRIGGER retention_generation_garbage_no_update
BEFORE UPDATE ON retention_generation_garbage BEGIN
    SELECT RAISE(ABORT, 'immutable generation garbage mark');
END;
CREATE TRIGGER retention_snapshot_garbage_no_update
BEFORE UPDATE ON retention_snapshot_garbage BEGIN
    SELECT RAISE(ABORT, 'immutable snapshot garbage mark');
END;
CREATE TRIGGER retention_artifact_garbage_no_update
BEFORE UPDATE ON retention_artifact_garbage BEGIN
    SELECT RAISE(ABORT, 'immutable artifact garbage mark');
END;
CREATE TRIGGER retention_workspace_view_garbage_no_update
BEFORE UPDATE ON retention_workspace_view_garbage BEGIN
    SELECT RAISE(ABORT, 'immutable workspace-view garbage mark');
END;
CREATE TRIGGER retention_source_slot_receipt_garbage_no_update
BEFORE UPDATE ON retention_source_slot_receipt_garbage BEGIN
    SELECT RAISE(ABORT, 'immutable source-slot receipt garbage mark');
END;
CREATE TRIGGER retention_collection_audit_no_update
BEFORE UPDATE ON retention_collection_audit BEGIN
    SELECT RAISE(ABORT, 'immutable retention collection audit');
END;
CREATE TRIGGER retention_collection_audit_no_delete
BEFORE DELETE ON retention_collection_audit BEGIN
    SELECT RAISE(ABORT, 'append-only retention collection audit');
END;

DROP TRIGGER complete_analysis_artifacts_no_delete;
CREATE TRIGGER complete_analysis_artifacts_no_delete
BEFORE DELETE ON analysis_artifacts
WHEN OLD.lifecycle_state = 'complete'
AND NOT EXISTS (
    SELECT 1 FROM retention_artifact_garbage
    WHERE artifact_digest = OLD.artifact_digest
)
BEGIN
    SELECT RAISE(ABORT, 'immutable complete analysis artifact');
END;

DROP TRIGGER complete_artifact_fact_correspondence_no_delete;
CREATE TRIGGER complete_artifact_fact_correspondence_no_delete
BEFORE DELETE ON artifact_fact_correspondence
WHEN (SELECT lifecycle_state FROM analysis_artifacts
      WHERE artifact_digest = OLD.artifact_digest) = 'complete'
AND NOT EXISTS (
    SELECT 1 FROM retention_artifact_garbage
    WHERE artifact_digest = OLD.artifact_digest
)
BEGIN
    SELECT RAISE(ABORT, 'immutable complete artifact correspondence');
END;

DROP TRIGGER complete_artifact_facts_no_delete;
CREATE TRIGGER complete_artifact_facts_no_delete
BEFORE DELETE ON artifact_facts
WHEN (SELECT lifecycle_state FROM analysis_artifacts
      WHERE artifact_digest = OLD.artifact_digest) = 'complete'
AND NOT EXISTS (
    SELECT 1 FROM retention_artifact_garbage
    WHERE artifact_digest = OLD.artifact_digest
)
BEGIN
    SELECT RAISE(ABORT, 'immutable complete artifact facts');
END;

DROP TRIGGER complete_source_manifest_entries_no_delete;
CREATE TRIGGER complete_source_manifest_entries_no_delete
BEFORE DELETE ON source_manifest_entries
WHEN (SELECT lifecycle_state FROM source_snapshots
      WHERE snapshot_digest = OLD.snapshot_digest) = 'complete'
AND NOT EXISTS (
    SELECT 1 FROM retention_snapshot_garbage
    WHERE snapshot_digest = OLD.snapshot_digest
)
BEGIN
    SELECT RAISE(ABORT, 'immutable complete source manifest');
END;

DROP TRIGGER complete_source_snapshots_no_delete;
CREATE TRIGGER complete_source_snapshots_no_delete
BEFORE DELETE ON source_snapshots
WHEN OLD.lifecycle_state = 'complete'
AND NOT EXISTS (
    SELECT 1 FROM retention_snapshot_garbage
    WHERE snapshot_digest = OLD.snapshot_digest
)
BEGIN
    SELECT RAISE(ABORT, 'immutable complete source snapshot');
END;

DROP TRIGGER rust_graph_artifacts_complete_no_delete;
CREATE TRIGGER rust_graph_artifacts_complete_no_delete
BEFORE DELETE ON rust_graph_artifacts
WHEN (SELECT lifecycle_state FROM analysis_artifacts
      WHERE artifact_digest = OLD.artifact_digest) = 'complete'
AND NOT EXISTS (
    SELECT 1 FROM retention_artifact_garbage
    WHERE artifact_digest = OLD.artifact_digest
)
BEGIN
    SELECT RAISE(ABORT, 'immutable complete graph artifact');
END;

DROP TRIGGER rust_graph_sites_complete_no_delete;
CREATE TRIGGER rust_graph_sites_complete_no_delete
BEFORE DELETE ON rust_graph_sites
WHEN (SELECT lifecycle_state FROM analysis_artifacts
      WHERE artifact_digest = OLD.artifact_digest) = 'complete'
AND NOT EXISTS (
    SELECT 1 FROM retention_artifact_garbage
    WHERE artifact_digest = OLD.artifact_digest
)
BEGIN
    SELECT RAISE(ABORT, 'immutable complete graph sites');
END;

DROP TRIGGER source_slot_generation_receipts_no_delete;
CREATE TRIGGER source_slot_generation_receipts_no_delete
BEFORE DELETE ON source_slot_generation_receipts
WHEN NOT EXISTS (
    SELECT 1 FROM retention_source_slot_receipt_garbage
    WHERE source_slot_id = OLD.source_slot_id
      AND source_epoch = OLD.source_epoch
)
AND (
    EXISTS (
        SELECT 1 FROM workspace_views
        WHERE connected_workspace_id = OLD.connected_workspace_id
          AND lifecycle_state = 'published'
    )
    OR EXISTS (
        SELECT 1 FROM active_workspace_views
        WHERE connected_workspace_id = OLD.connected_workspace_id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'immutable source-slot generation receipt');
END;

DROP TRIGGER workspace_views_delete_staging_only;
CREATE TRIGGER workspace_views_delete_staging_only
BEFORE DELETE ON workspace_views
WHEN OLD.lifecycle_state != 'staging'
AND NOT EXISTS (
    SELECT 1 FROM retention_workspace_view_garbage
    WHERE workspace_view_id = OLD.workspace_view_id
)
BEGIN
    SELECT RAISE(ABORT, 'published workspace view is immutable');
END;

DROP TRIGGER workspace_view_members_delete_staging_only;
CREATE TRIGGER workspace_view_members_delete_staging_only
BEFORE DELETE ON workspace_view_members
WHEN (
    SELECT lifecycle_state FROM workspace_views
    WHERE connected_workspace_id = OLD.connected_workspace_id
      AND workspace_view_id = OLD.workspace_view_id
) != 'staging'
AND NOT EXISTS (
    SELECT 1 FROM retention_workspace_view_garbage
    WHERE workspace_view_id = OLD.workspace_view_id
)
BEGIN
    SELECT RAISE(ABORT, 'published workspace view member is immutable');
END;

CREATE TRIGGER retained_generation_delete_requires_garbage
BEFORE DELETE ON index_generations
WHEN OLD.lifecycle_state IN ('ready', 'active', 'retained')
AND NOT EXISTS (
    SELECT 1 FROM retention_generation_garbage
    WHERE generation_id = OLD.generation_id
)
BEGIN
    SELECT RAISE(ABORT, 'generation is not marked as garbage');
END;
