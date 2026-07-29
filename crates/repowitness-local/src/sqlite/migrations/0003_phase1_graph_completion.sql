-- Completion and activation invariants run after all graph tables and row guards exist.

CREATE TRIGGER generation_graph_completion_requires_complete_projection
BEFORE UPDATE OF lifecycle_state ON generation_graph_publications
WHEN NEW.lifecycle_state = 'complete'
AND (
    (SELECT lifecycle_state FROM index_generations
     WHERE generation_id = NEW.generation_id) != 'ready'
    OR NEW.source_count != (
        SELECT count(*) FROM generation_graph_sources
        WHERE generation_id = NEW.generation_id
    )
    OR NEW.artifact_count != (
        SELECT count(*) FROM generation_graph_artifacts
        WHERE generation_id = NEW.generation_id
    )
    OR NEW.definition_count != (
        SELECT count(*) FROM generation_graph_definitions
        WHERE generation_id = NEW.generation_id
    )
    OR NEW.site_count != (
        SELECT count(*) FROM generation_graph_resolutions
        WHERE generation_id = NEW.generation_id
    )
    OR NEW.unresolved_count != (
        SELECT count(*) FROM generation_graph_resolutions
        WHERE generation_id = NEW.generation_id
          AND outcome_kind = 'unresolved'
    )
    OR NEW.unique_count != (
        SELECT count(*) FROM generation_graph_resolutions
        WHERE generation_id = NEW.generation_id
          AND outcome_kind = 'unique'
    )
    OR NEW.ambiguous_count != (
        SELECT count(*) FROM generation_graph_resolutions
        WHERE generation_id = NEW.generation_id
          AND outcome_kind = 'ambiguous'
    )
    OR NEW.retained_candidate_count != (
        SELECT count(*) FROM generation_graph_candidates
        WHERE generation_id = NEW.generation_id
    )
    OR NEW.edge_count != (
        SELECT count(*) FROM generation_graph_edges
        WHERE generation_id = NEW.generation_id
    )
    OR NEW.edge_count != NEW.unique_count
    OR EXISTS (
        SELECT 1
        FROM generation_graph_resolutions AS resolution
        WHERE resolution.generation_id = NEW.generation_id
          AND (
              (resolution.outcome_kind = 'unresolved' AND EXISTS (
                  SELECT 1 FROM generation_graph_candidates AS candidate
                  WHERE candidate.generation_id = resolution.generation_id
                    AND candidate.site_source_slot_id = resolution.source_slot_id
                    AND candidate.site_repository_path = resolution.repository_path
                    AND candidate.site_artifact_digest =
                        resolution.site_artifact_digest
                    AND candidate.site_ordinal = resolution.site_ordinal
              ))
              OR
              (resolution.outcome_kind = 'unique' AND 1 != (
                  SELECT count(*) FROM generation_graph_candidates AS candidate
                  WHERE candidate.generation_id = resolution.generation_id
                    AND candidate.site_source_slot_id = resolution.source_slot_id
                    AND candidate.site_repository_path = resolution.repository_path
                    AND candidate.site_artifact_digest =
                        resolution.site_artifact_digest
                    AND candidate.site_ordinal = resolution.site_ordinal
              ))
              OR
              (resolution.outcome_kind = 'ambiguous' AND (
                  (SELECT count(*) FROM generation_graph_candidates AS candidate
                   WHERE candidate.generation_id = resolution.generation_id
                     AND candidate.site_source_slot_id = resolution.source_slot_id
                     AND candidate.site_repository_path = resolution.repository_path
                     AND candidate.site_artifact_digest =
                         resolution.site_artifact_digest
                     AND candidate.site_ordinal = resolution.site_ordinal) < 2
                  OR
                  (SELECT count(*) FROM generation_graph_candidates AS candidate
                   WHERE candidate.generation_id = resolution.generation_id
                     AND candidate.site_source_slot_id = resolution.source_slot_id
                     AND candidate.site_repository_path = resolution.repository_path
                     AND candidate.site_artifact_digest =
                         resolution.site_artifact_digest
                     AND candidate.site_ordinal = resolution.site_ordinal) >
                      resolution.candidate_count
                  OR resolution.candidates_truncated != (
                      (SELECT count(*) FROM generation_graph_candidates AS candidate
                       WHERE candidate.generation_id = resolution.generation_id
                         AND candidate.site_source_slot_id = resolution.source_slot_id
                         AND candidate.site_repository_path =
                             resolution.repository_path
                         AND candidate.site_artifact_digest =
                             resolution.site_artifact_digest
                         AND candidate.site_ordinal = resolution.site_ordinal)
                      < resolution.candidate_count
                  )
              ))
          )
    )
)
BEGIN
    SELECT RAISE(ABORT, 'incomplete generation graph publication');
END;

CREATE TRIGGER generation_activation_requires_graph_when_required
BEFORE UPDATE OF lifecycle_state ON index_generations
WHEN
    OLD.lifecycle_state = 'ready'
    AND NEW.lifecycle_state = 'active'
    AND EXISTS (
        SELECT 1 FROM generation_graph_requirements
        WHERE generation_id = NEW.generation_id
    )
    AND NOT EXISTS (
        SELECT 1 FROM generation_graph_publications
        WHERE generation_id = NEW.generation_id
          AND lifecycle_state = 'complete'
    )
BEGIN
    SELECT RAISE(ABORT, 'required generation graph is incomplete');
END;
