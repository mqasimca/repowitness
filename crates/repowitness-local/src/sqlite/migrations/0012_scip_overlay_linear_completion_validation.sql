-- A complete overlay retains ordinal sequences for documents, occurrences,
-- and relationships. Migration 4 proved those sequences with correlated
-- counts, which becomes quadratic for large otherwise-valid SCIP overlays.
-- The primary keys already make each ordinal unique, so minimum, maximum, and
-- count prove exactly the same contiguous-zero-based invariant in linear work.
DROP TRIGGER scip_overlay_receipt_completion_requires_exact_rows;

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
        FROM scip_overlay_documents
        WHERE overlay_digest = NEW.overlay_digest
        GROUP BY overlay_digest
        HAVING min(document_ordinal) != 0
            OR max(document_ordinal) + 1 != count(*)
    )
    OR EXISTS (
        SELECT 1
        FROM scip_overlay_occurrences
        WHERE overlay_digest = NEW.overlay_digest
        GROUP BY document_ordinal
        HAVING min(occurrence_ordinal) != 0
            OR max(occurrence_ordinal) + 1 != count(*)
    )
    OR EXISTS (
        SELECT 1
        FROM scip_overlay_relationships
        WHERE overlay_digest = NEW.overlay_digest
        GROUP BY document_ordinal
        HAVING min(relationship_ordinal) != 0
            OR max(relationship_ordinal) + 1 != count(*)
    )
)
BEGIN
    SELECT RAISE(ABORT, 'SCIP overlay receipt is incomplete');
END;
