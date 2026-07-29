WITH ranked AS (
    SELECT generation.generation_id, slot.source_slot_id,
           generation.source_epoch,
           row_number() OVER (
               PARTITION BY slot.source_slot_id
               ORDER BY generation.source_epoch DESC,
                        generation.generation_id DESC
           ) AS retained_rank
    FROM workspace_source_slots AS slot
    JOIN index_generations AS generation
      ON generation.workspace_id = slot.generation_workspace_id
     AND generation.lifecycle_state = 'retained'
),
eligible AS (
    SELECT generation_id, min(source_slot_id) AS sort_source_slot,
           min(source_epoch) AS sort_source_epoch
    FROM ranked
    GROUP BY generation_id
    HAVING min(retained_rank) > ?1
)
SELECT eligible.generation_id, eligible.sort_source_slot,
       eligible.sort_source_epoch
FROM eligible
JOIN index_generations AS generation
  ON generation.generation_id = eligible.generation_id
WHERE NOT EXISTS (
          SELECT 1 FROM workspaces
          WHERE active_generation_id = eligible.generation_id
      )
  AND NOT EXISTS (
          SELECT 1
          FROM workspace_view_members AS member
          JOIN active_workspace_views AS active
            ON active.connected_workspace_id = member.connected_workspace_id
           AND active.workspace_view_id = member.workspace_view_id
          WHERE member.generation_id = eligible.generation_id
      )
  AND NOT EXISTS (
          SELECT 1
          FROM source_slot_generation_receipts AS receipt
          JOIN workspace_source_slots AS slot
            ON slot.connected_workspace_id = receipt.connected_workspace_id
           AND slot.source_slot_id = receipt.source_slot_id
           AND slot.source_epoch = receipt.source_epoch
          WHERE receipt.generation_id = eligible.generation_id
      )
  AND NOT EXISTS (
          SELECT 1 FROM memory_projection_generations
          WHERE index_generation_id = eligible.generation_id
      )
  AND NOT EXISTS (
          SELECT 1 FROM generation_graph_sources AS source
          WHERE source.source_generation_id = eligible.generation_id
            AND source.generation_id != eligible.generation_id
      )
  AND NOT EXISTS (
          SELECT 1 FROM memory_versions
          WHERE validity_source_snapshot = generation.snapshot_digest
      )
  AND NOT EXISTS (
          SELECT 1 FROM memory_evidence
          WHERE source_snapshot_digest = generation.snapshot_digest
      )
  AND NOT EXISTS (
          SELECT 1 FROM memory_audit
          WHERE source_format = 'source_snapshot'
            AND source_revision = generation.snapshot_digest
      )
  AND NOT EXISTS (
          SELECT 1 FROM memory_correspondence_audit
          WHERE source_snapshot_digest = generation.snapshot_digest
             OR target_snapshot_digest = generation.snapshot_digest
      )
  AND NOT EXISTS (
          SELECT 1 FROM memory_projection_evidence
          WHERE target_snapshot_digest = generation.snapshot_digest
      )
  AND NOT EXISTS (
          SELECT 1 FROM memory_projection_candidates
          WHERE target_snapshot_digest = generation.snapshot_digest
      )
  AND NOT EXISTS (
          SELECT 1
          FROM generation_files AS file
          WHERE file.generation_id = eligible.generation_id
            AND (
                EXISTS (
                    SELECT 1 FROM memory_evidence
                    WHERE artifact_digest = file.artifact_digest
                )
                OR EXISTS (
                    SELECT 1 FROM memory_correspondence_audit
                    WHERE source_artifact_digest = file.artifact_digest
                       OR target_artifact_digest = file.artifact_digest
                )
                OR EXISTS (
                    SELECT 1 FROM memory_projection_evidence
                    WHERE target_artifact_digest = file.artifact_digest
                )
                OR EXISTS (
                    SELECT 1 FROM memory_projection_candidates
                    WHERE target_artifact_digest = file.artifact_digest
                )
            )
      )
