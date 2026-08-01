-- A unique nonnegative ordinal has no gaps from zero exactly when its count
-- equals its maximum ordinal plus one. This replaces migration 3's correlated
-- per-site test while preserving the immutable completion invariant in linear
-- index work.
DROP TRIGGER rust_graph_artifact_completion_requires_sites;

CREATE TRIGGER rust_graph_artifact_completion_requires_sites
BEFORE UPDATE OF lifecycle_state ON analysis_artifacts
WHEN
    OLD.lifecycle_state = 'staging'
    AND NEW.lifecycle_state = 'complete'
    AND EXISTS (
        SELECT 1 FROM rust_graph_artifacts
        WHERE artifact_digest = NEW.artifact_digest
    )
    AND (
        NEW.fact_count != 0
        OR NEW.payload_digest IS NULL
        OR (SELECT site_count FROM rust_graph_artifacts
            WHERE artifact_digest = NEW.artifact_digest) !=
           (SELECT count(*) FROM rust_graph_sites
            WHERE artifact_digest = NEW.artifact_digest)
        OR (SELECT count(*) FROM rust_graph_sites
            WHERE artifact_digest = NEW.artifact_digest) !=
           COALESCE((
               SELECT max(ordinal) + 1
               FROM rust_graph_sites
               WHERE artifact_digest = NEW.artifact_digest
           ), 0)
    )
BEGIN
    SELECT RAISE(ABORT, 'incomplete graph artifact');
END;
