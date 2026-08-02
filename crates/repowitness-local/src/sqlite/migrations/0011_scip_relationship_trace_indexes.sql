-- Directional immutable relationship indexes for bounded SCIP precision traversal.
--
-- The trace reader orders siblings by the persisted document and relationship
-- ordinals. These indexes add no relation or source assertion and do not alter
-- the completed immutable overlay rows.

-- Migration 4's target index is a strict prefix of the directional inbound
-- index below, so retaining both would duplicate every target-symbol key.
DROP INDEX scip_overlay_relationships_by_target;

CREATE INDEX scip_overlay_relationships_trace_outbound
ON scip_overlay_relationships(
    overlay_digest, source_symbol, document_ordinal, relationship_ordinal
);

CREATE INDEX scip_overlay_relationships_trace_inbound
ON scip_overlay_relationships(
    overlay_digest, target_symbol, document_ordinal, relationship_ordinal
);
