-- RepoWitness Phase 2 SCIP precision overlays.
--
-- An overlay is an immutable, source-slot-scoped receipt.  Its lifetime is
-- staging -> complete.  The writer creates all staging rows, verifies exact
-- cardinalities, completes the receipt, and (optionally) switches the active
-- pointer in one transaction.  Thus an interrupted import leaves neither a
-- partial receipt nor a changed pointer.

CREATE TABLE scip_overlay_receipts (
    overlay_digest BLOB PRIMARY KEY CHECK (length(overlay_digest) = 32),
    connected_workspace_id BLOB NOT NULL
        CHECK (length(connected_workspace_id) = 32),
    workspace_view_id INTEGER NOT NULL CHECK (workspace_view_id > 0),
    source_slot_id BLOB NOT NULL CHECK (length(source_slot_id) = 32),
    source_epoch INTEGER NOT NULL CHECK (source_epoch >= 0),
    generation_workspace_id INTEGER NOT NULL CHECK (generation_workspace_id > 0),
    generation_id INTEGER NOT NULL CHECK (generation_id > 0),
    source_snapshot_digest BLOB NOT NULL CHECK (length(source_snapshot_digest) = 32),
    source_manifest_digest BLOB NOT NULL CHECK (length(source_manifest_digest) = 32),
    configuration_digest BLOB NOT NULL CHECK (length(configuration_digest) = 32),
    producer_digest BLOB NOT NULL CHECK (length(producer_digest) = 32),
    schema_digest BLOB NOT NULL CHECK (length(schema_digest) = 32),
    importer_digest BLOB NOT NULL CHECK (length(importer_digest) = 32),
    input_digest BLOB NOT NULL CHECK (length(input_digest) = 32),
    lifecycle_state TEXT NOT NULL
        CHECK (lifecycle_state IN ('staging', 'complete')),
    document_count INTEGER NOT NULL CHECK (document_count >= 0),
    occurrence_count INTEGER NOT NULL CHECK (occurrence_count >= 0),
    relationship_count INTEGER NOT NULL CHECK (relationship_count >= 0),
    FOREIGN KEY (connected_workspace_id, workspace_view_id)
        REFERENCES workspace_views(connected_workspace_id, workspace_view_id),
    FOREIGN KEY (workspace_view_id, source_slot_id)
        REFERENCES workspace_view_members(workspace_view_id, source_slot_id),
    FOREIGN KEY (generation_workspace_id, generation_id)
        REFERENCES index_generations(workspace_id, generation_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE scip_overlay_documents (
    overlay_digest BLOB NOT NULL CHECK (length(overlay_digest) = 32)
        REFERENCES scip_overlay_receipts(overlay_digest),
    document_ordinal INTEGER NOT NULL CHECK (document_ordinal >= 0),
    repository_path BLOB NOT NULL
        CHECK (length(repository_path) BETWEEN 1 AND 4096),
    content_digest BLOB NOT NULL CHECK (length(content_digest) = 32),
    occurrence_count INTEGER NOT NULL CHECK (occurrence_count >= 0),
    relationship_count INTEGER NOT NULL CHECK (relationship_count >= 0),
    PRIMARY KEY (overlay_digest, document_ordinal),
    UNIQUE (overlay_digest, repository_path)
) STRICT, WITHOUT ROWID;

CREATE TABLE scip_overlay_occurrences (
    overlay_digest BLOB NOT NULL CHECK (length(overlay_digest) = 32),
    document_ordinal INTEGER NOT NULL CHECK (document_ordinal >= 0),
    occurrence_ordinal INTEGER NOT NULL CHECK (occurrence_ordinal >= 0),
    symbol BLOB CHECK (symbol IS NULL OR length(symbol) BETWEEN 1 AND 16384),
    roles INTEGER NOT NULL CHECK (roles BETWEEN 0 AND 4294967295),
    start_byte INTEGER NOT NULL CHECK (start_byte >= 0),
    end_byte INTEGER NOT NULL CHECK (end_byte >= start_byte),
    PRIMARY KEY (overlay_digest, document_ordinal, occurrence_ordinal),
    FOREIGN KEY (overlay_digest, document_ordinal)
        REFERENCES scip_overlay_documents(overlay_digest, document_ordinal)
) STRICT, WITHOUT ROWID;

CREATE TABLE scip_overlay_relationships (
    overlay_digest BLOB NOT NULL CHECK (length(overlay_digest) = 32),
    document_ordinal INTEGER NOT NULL CHECK (document_ordinal >= 0),
    relationship_ordinal INTEGER NOT NULL CHECK (relationship_ordinal >= 0),
    source_symbol BLOB NOT NULL CHECK (length(source_symbol) BETWEEN 1 AND 16384),
    target_symbol BLOB NOT NULL CHECK (length(target_symbol) BETWEEN 1 AND 16384),
    kinds INTEGER NOT NULL CHECK (kinds BETWEEN 1 AND 255),
    PRIMARY KEY (overlay_digest, document_ordinal, relationship_ordinal),
    FOREIGN KEY (overlay_digest, document_ordinal)
        REFERENCES scip_overlay_documents(overlay_digest, document_ordinal)
) STRICT, WITHOUT ROWID;

CREATE TABLE active_scip_overlays (
    connected_workspace_id BLOB NOT NULL
        CHECK (length(connected_workspace_id) = 32),
    source_slot_id BLOB NOT NULL CHECK (length(source_slot_id) = 32),
    workspace_view_id INTEGER NOT NULL CHECK (workspace_view_id > 0),
    overlay_digest BLOB NOT NULL CHECK (length(overlay_digest) = 32),
    PRIMARY KEY (connected_workspace_id, source_slot_id),
    FOREIGN KEY (connected_workspace_id, source_slot_id)
        REFERENCES workspace_source_slots(connected_workspace_id, source_slot_id),
    FOREIGN KEY (connected_workspace_id, workspace_view_id)
        REFERENCES workspace_views(connected_workspace_id, workspace_view_id),
    FOREIGN KEY (overlay_digest) REFERENCES scip_overlay_receipts(overlay_digest)
) STRICT, WITHOUT ROWID;

-- A complete overlay is immutable until the existing bounded-retention
-- transaction marks its owning source generation. The mark is deliberately a
-- separate relation rather than another receipt lifecycle state: that keeps a
-- receipt's immutable identity stable while making the destructive authority
-- explicit, auditable through the retention collection, and rollback-safe.
CREATE TABLE retention_scip_overlay_garbage (
    overlay_digest BLOB PRIMARY KEY
        REFERENCES scip_overlay_receipts(overlay_digest) ON DELETE CASCADE,
    plan_digest BLOB NOT NULL CHECK (length(plan_digest) = 32),
    lifecycle_state TEXT NOT NULL CHECK (lifecycle_state = 'garbage')
) STRICT, WITHOUT ROWID;

CREATE INDEX scip_overlay_documents_by_path
ON scip_overlay_documents(overlay_digest, repository_path);

CREATE INDEX scip_overlay_occurrences_by_symbol
ON scip_overlay_occurrences(overlay_digest, symbol);

CREATE INDEX scip_overlay_relationships_by_target
ON scip_overlay_relationships(overlay_digest, target_symbol);

CREATE TRIGGER scip_overlay_receipts_insert_staging_only
BEFORE INSERT ON scip_overlay_receipts
WHEN NEW.lifecycle_state != 'staging'
BEGIN
    SELECT RAISE(ABORT, 'SCIP overlay receipt must begin staging');
END;

CREATE TRIGGER scip_overlay_receipts_require_exact_view_member
BEFORE INSERT ON scip_overlay_receipts
WHEN NOT EXISTS (
    SELECT 1
    FROM workspace_views AS view
    JOIN workspace_view_members AS member
      ON member.workspace_view_id = view.workspace_view_id
    JOIN index_generations AS generation
      ON generation.workspace_id = member.generation_workspace_id
     AND generation.generation_id = member.generation_id
    JOIN source_snapshots AS snapshot
      ON snapshot.snapshot_digest = generation.snapshot_digest
    WHERE view.connected_workspace_id = NEW.connected_workspace_id
      AND view.workspace_view_id = NEW.workspace_view_id
      AND view.lifecycle_state = 'published'
      AND member.source_slot_id = NEW.source_slot_id
      AND member.source_epoch = NEW.source_epoch
      AND member.generation_workspace_id = NEW.generation_workspace_id
      AND member.generation_id = NEW.generation_id
      AND generation.snapshot_digest = NEW.source_snapshot_digest
      AND snapshot.manifest_digest = NEW.source_manifest_digest
)
BEGIN
    SELECT RAISE(ABORT, 'SCIP overlay scope is not an exact published view member');
END;

CREATE TRIGGER scip_overlay_receipts_no_semantic_update
BEFORE UPDATE OF
    overlay_digest, connected_workspace_id, workspace_view_id, source_slot_id,
    source_epoch, generation_workspace_id, generation_id, source_snapshot_digest,
    source_manifest_digest, configuration_digest, producer_digest, schema_digest,
    importer_digest, input_digest, document_count, occurrence_count,
    relationship_count
ON scip_overlay_receipts BEGIN
    SELECT RAISE(ABORT, 'immutable SCIP overlay receipt');
END;

CREATE TRIGGER scip_overlay_receipt_lifecycle_transition
BEFORE UPDATE OF lifecycle_state ON scip_overlay_receipts
WHEN NOT (OLD.lifecycle_state = 'staging' AND NEW.lifecycle_state = 'complete')
BEGIN
    SELECT RAISE(ABORT, 'invalid SCIP overlay lifecycle transition');
END;

CREATE TRIGGER scip_overlay_receipt_completion_requires_exact_rows
BEFORE UPDATE OF lifecycle_state ON scip_overlay_receipts
WHEN NEW.lifecycle_state = 'complete'
AND (
    (SELECT count(*) FROM scip_overlay_documents
     WHERE overlay_digest = NEW.overlay_digest) != NEW.document_count
    OR (SELECT coalesce(sum(occurrence_count), 0) FROM scip_overlay_documents
        WHERE overlay_digest = NEW.overlay_digest) != NEW.occurrence_count
    OR (SELECT coalesce(sum(relationship_count), 0) FROM scip_overlay_documents
        WHERE overlay_digest = NEW.overlay_digest) != NEW.relationship_count
    OR (SELECT count(*) FROM scip_overlay_occurrences
        WHERE overlay_digest = NEW.overlay_digest) != NEW.occurrence_count
    OR (SELECT count(*) FROM scip_overlay_relationships
        WHERE overlay_digest = NEW.overlay_digest) != NEW.relationship_count
    OR EXISTS (
        SELECT 1
        FROM scip_overlay_documents AS document
        WHERE document.overlay_digest = NEW.overlay_digest
          AND document.occurrence_count != (
              SELECT count(*)
              FROM scip_overlay_occurrences AS occurrence
              WHERE occurrence.overlay_digest = document.overlay_digest
                AND occurrence.document_ordinal = document.document_ordinal
          )
    )
    OR EXISTS (
        SELECT 1
        FROM scip_overlay_documents AS document
        WHERE document.overlay_digest = NEW.overlay_digest
          AND document.relationship_count != (
              SELECT count(*)
              FROM scip_overlay_relationships AS relationship
              WHERE relationship.overlay_digest = document.overlay_digest
                AND relationship.document_ordinal = document.document_ordinal
          )
    )
    OR EXISTS (
        SELECT 1
        FROM scip_overlay_documents AS document
        WHERE document.overlay_digest = NEW.overlay_digest
          AND document.document_ordinal != (
              SELECT count(*)
              FROM scip_overlay_documents AS prior
              WHERE prior.overlay_digest = document.overlay_digest
                AND prior.document_ordinal < document.document_ordinal
          )
    )
    OR EXISTS (
        SELECT 1
        FROM scip_overlay_occurrences AS occurrence
        WHERE occurrence.overlay_digest = NEW.overlay_digest
          AND occurrence.occurrence_ordinal != (
              SELECT count(*)
              FROM scip_overlay_occurrences AS prior
              WHERE prior.overlay_digest = occurrence.overlay_digest
                AND prior.document_ordinal = occurrence.document_ordinal
                AND prior.occurrence_ordinal < occurrence.occurrence_ordinal
          )
    )
    OR EXISTS (
        SELECT 1
        FROM scip_overlay_relationships AS relationship
        WHERE relationship.overlay_digest = NEW.overlay_digest
          AND relationship.relationship_ordinal != (
              SELECT count(*)
              FROM scip_overlay_relationships AS prior
              WHERE prior.overlay_digest = relationship.overlay_digest
                AND prior.document_ordinal = relationship.document_ordinal
                AND prior.relationship_ordinal < relationship.relationship_ordinal
          )
    )
)
BEGIN
    SELECT RAISE(ABORT, 'SCIP overlay receipt is incomplete');
END;

CREATE TRIGGER scip_overlay_receipts_delete_staging_only
BEFORE DELETE ON scip_overlay_receipts
WHEN OLD.lifecycle_state != 'staging'
AND NOT EXISTS (
    SELECT 1 FROM retention_scip_overlay_garbage
    WHERE overlay_digest = OLD.overlay_digest
)
BEGIN
    SELECT RAISE(ABORT, 'complete SCIP overlay receipt is immutable');
END;

CREATE TRIGGER scip_overlay_documents_insert_staging_only
BEFORE INSERT ON scip_overlay_documents
WHEN (SELECT lifecycle_state FROM scip_overlay_receipts
      WHERE overlay_digest = NEW.overlay_digest) != 'staging'
BEGIN
    SELECT RAISE(ABORT, 'SCIP overlay is not accepting documents');
END;

CREATE TRIGGER scip_overlay_documents_require_exact_generation_file
BEFORE INSERT ON scip_overlay_documents
WHEN NOT EXISTS (
    SELECT 1
    FROM scip_overlay_receipts AS receipt
    JOIN generation_files AS file
      ON file.generation_id = receipt.generation_id
    WHERE receipt.overlay_digest = NEW.overlay_digest
      AND file.repository_path = NEW.repository_path
      AND file.content_digest = NEW.content_digest
)
BEGIN
    SELECT RAISE(ABORT, 'SCIP overlay document is not an exact generation file');
END;

CREATE TRIGGER scip_overlay_documents_no_update
BEFORE UPDATE ON scip_overlay_documents BEGIN
    SELECT RAISE(ABORT, 'immutable SCIP overlay document');
END;

CREATE TRIGGER scip_overlay_documents_delete_staging_only
BEFORE DELETE ON scip_overlay_documents
WHEN (SELECT lifecycle_state FROM scip_overlay_receipts
      WHERE overlay_digest = OLD.overlay_digest) != 'staging'
AND NOT EXISTS (
    SELECT 1 FROM retention_scip_overlay_garbage
    WHERE overlay_digest = OLD.overlay_digest
)
BEGIN
    SELECT RAISE(ABORT, 'complete SCIP overlay document is immutable');
END;

CREATE TRIGGER scip_overlay_occurrences_insert_staging_only
BEFORE INSERT ON scip_overlay_occurrences
WHEN (SELECT lifecycle_state FROM scip_overlay_receipts
      WHERE overlay_digest = NEW.overlay_digest) != 'staging'
BEGIN
    SELECT RAISE(ABORT, 'SCIP overlay is not accepting occurrences');
END;

CREATE TRIGGER scip_overlay_occurrences_no_update
BEFORE UPDATE ON scip_overlay_occurrences BEGIN
    SELECT RAISE(ABORT, 'immutable SCIP overlay occurrence');
END;

CREATE TRIGGER scip_overlay_occurrences_delete_staging_only
BEFORE DELETE ON scip_overlay_occurrences
WHEN (SELECT lifecycle_state FROM scip_overlay_receipts
      WHERE overlay_digest = OLD.overlay_digest) != 'staging'
AND NOT EXISTS (
    SELECT 1 FROM retention_scip_overlay_garbage
    WHERE overlay_digest = OLD.overlay_digest
)
BEGIN
    SELECT RAISE(ABORT, 'complete SCIP overlay occurrence is immutable');
END;

CREATE TRIGGER scip_overlay_relationships_insert_staging_only
BEFORE INSERT ON scip_overlay_relationships
WHEN (SELECT lifecycle_state FROM scip_overlay_receipts
      WHERE overlay_digest = NEW.overlay_digest) != 'staging'
BEGIN
    SELECT RAISE(ABORT, 'SCIP overlay is not accepting relationships');
END;

CREATE TRIGGER scip_overlay_relationships_no_update
BEFORE UPDATE ON scip_overlay_relationships BEGIN
    SELECT RAISE(ABORT, 'immutable SCIP overlay relationship');
END;

CREATE TRIGGER scip_overlay_relationships_delete_staging_only
BEFORE DELETE ON scip_overlay_relationships
WHEN (SELECT lifecycle_state FROM scip_overlay_receipts
      WHERE overlay_digest = OLD.overlay_digest) != 'staging'
AND NOT EXISTS (
    SELECT 1 FROM retention_scip_overlay_garbage
    WHERE overlay_digest = OLD.overlay_digest
)
BEGIN
    SELECT RAISE(ABORT, 'complete SCIP overlay relationship is immutable');
END;

CREATE TRIGGER retention_scip_overlay_garbage_validate_insert
BEFORE INSERT ON retention_scip_overlay_garbage
WHEN
    (SELECT lifecycle_state FROM scip_overlay_receipts
     WHERE overlay_digest = NEW.overlay_digest) != 'complete'
    OR NOT EXISTS (
        SELECT 1
        FROM scip_overlay_receipts AS receipt
        JOIN retention_generation_garbage AS garbage
          ON garbage.generation_id = receipt.generation_id
         AND garbage.plan_digest = NEW.plan_digest
        WHERE receipt.overlay_digest = NEW.overlay_digest
    )
BEGIN
    SELECT RAISE(ABORT, 'SCIP overlay is a retention root');
END;

CREATE TRIGGER retention_scip_overlay_garbage_no_update
BEFORE UPDATE ON retention_scip_overlay_garbage BEGIN
    SELECT RAISE(ABORT, 'immutable SCIP overlay garbage mark');
END;

CREATE TRIGGER active_scip_overlays_require_complete_exact_scope_insert
BEFORE INSERT ON active_scip_overlays
WHEN NOT EXISTS (
    SELECT 1
    FROM scip_overlay_receipts AS receipt
    JOIN workspace_views AS view
      ON view.connected_workspace_id = receipt.connected_workspace_id
     AND view.workspace_view_id = receipt.workspace_view_id
    JOIN workspace_view_members AS member
      ON member.workspace_view_id = receipt.workspace_view_id
     AND member.source_slot_id = receipt.source_slot_id
    WHERE receipt.overlay_digest = NEW.overlay_digest
      AND receipt.lifecycle_state = 'complete'
      AND receipt.connected_workspace_id = NEW.connected_workspace_id
      AND receipt.workspace_view_id = NEW.workspace_view_id
      AND receipt.source_slot_id = NEW.source_slot_id
      AND view.lifecycle_state = 'published'
      AND member.source_epoch = receipt.source_epoch
      AND member.generation_workspace_id = receipt.generation_workspace_id
      AND member.generation_id = receipt.generation_id
)
BEGIN
    SELECT RAISE(ABORT, 'active SCIP overlay must match a complete published scope');
END;

CREATE TRIGGER active_scip_overlays_require_complete_exact_scope_update
BEFORE UPDATE OF workspace_view_id, overlay_digest ON active_scip_overlays
WHEN NOT EXISTS (
    SELECT 1
    FROM scip_overlay_receipts AS receipt
    JOIN workspace_views AS view
      ON view.connected_workspace_id = receipt.connected_workspace_id
     AND view.workspace_view_id = receipt.workspace_view_id
    JOIN workspace_view_members AS member
      ON member.workspace_view_id = receipt.workspace_view_id
     AND member.source_slot_id = receipt.source_slot_id
    WHERE receipt.overlay_digest = NEW.overlay_digest
      AND receipt.lifecycle_state = 'complete'
      AND receipt.connected_workspace_id = NEW.connected_workspace_id
      AND receipt.workspace_view_id = NEW.workspace_view_id
      AND receipt.source_slot_id = NEW.source_slot_id
      AND view.lifecycle_state = 'published'
      AND member.source_epoch = receipt.source_epoch
      AND member.generation_workspace_id = receipt.generation_workspace_id
      AND member.generation_id = receipt.generation_id
)
BEGIN
    SELECT RAISE(ABORT, 'active SCIP overlay must match a complete published scope');
END;

CREATE TRIGGER active_scip_overlays_no_identity_update
BEFORE UPDATE OF connected_workspace_id, source_slot_id ON active_scip_overlays BEGIN
    SELECT RAISE(ABORT, 'immutable active SCIP overlay pointer identity');
END;

CREATE TRIGGER active_scip_overlays_no_delete
BEFORE DELETE ON active_scip_overlays
WHEN NOT EXISTS (
    SELECT 1 FROM retention_scip_overlay_garbage
    WHERE overlay_digest = OLD.overlay_digest
)
BEGIN
    SELECT RAISE(ABORT, 'active SCIP overlay pointer is required');
END;
