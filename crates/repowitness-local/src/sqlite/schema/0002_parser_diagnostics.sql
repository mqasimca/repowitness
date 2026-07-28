-- Persist the recognized subset of raw parser diagnostics without rewriting
-- the accepted Phase 0 version-1 baseline.

ALTER TABLE analysis_artifacts
ADD COLUMN known_parser_limitation_nodes INTEGER NOT NULL DEFAULT 0 CHECK (
    known_parser_limitation_nodes >= 0
    AND known_parser_limitation_nodes <= syntax_error_nodes
);

DROP TRIGGER analysis_artifacts_no_semantic_update;
CREATE TRIGGER analysis_artifacts_no_semantic_update
BEFORE UPDATE OF
    artifact_digest, source_content_digest, producer_manifest_digest,
    configuration_digest, analysis_schema_digest, canonicalization_version,
    fact_count, visited_nodes, syntax_error_nodes, language,
    known_parser_limitation_nodes
ON analysis_artifacts BEGIN
    SELECT RAISE(ABORT, 'immutable analysis artifact semantics');
END;
