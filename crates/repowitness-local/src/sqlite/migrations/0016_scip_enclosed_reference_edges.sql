-- Derived, immutable SCIP call/reference edges.
--
-- These rows are built only from an exact SCIP occurrence, an exact indexed
-- function/method fact, and the exact definition occurrence for that fact.
-- They are deliberately separate from producer-declared relationships.

CREATE TABLE scip_enclosed_reference_edges (
    overlay_digest BLOB NOT NULL CHECK (length(overlay_digest) = 32),
    document_ordinal INTEGER NOT NULL CHECK (document_ordinal >= 0),
    relationship_ordinal INTEGER NOT NULL CHECK (relationship_ordinal >= 0),
    source_symbol BLOB NOT NULL CHECK (length(source_symbol) BETWEEN 1 AND 16384),
    target_symbol BLOB NOT NULL CHECK (length(target_symbol) BETWEEN 1 AND 16384),
    kinds INTEGER NOT NULL CHECK (kinds = 1),
    PRIMARY KEY (overlay_digest, document_ordinal, relationship_ordinal),
    FOREIGN KEY (overlay_digest) REFERENCES scip_overlay_receipts(overlay_digest)
) STRICT, WITHOUT ROWID;

CREATE INDEX scip_enclosed_reference_edges_by_source
ON scip_enclosed_reference_edges(overlay_digest, source_symbol);

CREATE INDEX scip_enclosed_reference_edges_by_target
ON scip_enclosed_reference_edges(overlay_digest, target_symbol);
