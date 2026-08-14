-- Bounded exact-reference projection indexes. Each index follows one
-- predicate in the derived-edge import query, so large SCIP overlays remain
-- document-local rather than repeatedly scanning every occurrence.

CREATE INDEX scip_overlay_definition_spans
ON scip_overlay_occurrences(
    overlay_digest, document_ordinal, start_byte, end_byte
)
WHERE roles & 1 != 0 AND symbol IS NOT NULL;

CREATE INDEX scip_overlay_reference_spans
ON scip_overlay_occurrences(
    overlay_digest, document_ordinal, start_byte, end_byte
)
WHERE roles & 1 = 0 AND symbol IS NOT NULL;

CREATE INDEX scip_overlay_relationship_pairs
ON scip_overlay_relationships(
    overlay_digest, document_ordinal, source_symbol, target_symbol
);

CREATE INDEX artifact_function_spans
ON artifact_facts(artifact_digest, declaration_start, declaration_end)
WHERE kind IN ('function', 'method');
