-- RepoWitness Phase 0 baseline: immutable memory revalidation projection.

CREATE TABLE memory_projection_generations (
    projection_id INTEGER PRIMARY KEY CHECK (projection_id > 0),
    workspace_id INTEGER NOT NULL REFERENCES workspaces(workspace_id),
    index_generation_id INTEGER NOT NULL CHECK (index_generation_id > 0),
    source_epoch INTEGER NOT NULL CHECK (source_epoch >= 0),
    snapshot_digest BLOB NOT NULL CHECK (length(snapshot_digest) = 32),
    target_kind TEXT NOT NULL CHECK (target_kind IN ('git', 'worktree')),
    target_format TEXT NOT NULL CHECK (
        target_format IN ('sha1', 'sha256', 'source_snapshot')
    ),
    target_revision BLOB NOT NULL,
    head_format TEXT CHECK (head_format IN ('sha1', 'sha256')),
    head_revision BLOB,
    correspondence_profile_id TEXT NOT NULL CHECK (
        length(CAST(correspondence_profile_id AS BLOB)) BETWEEN 1 AND 128
        AND correspondence_profile_id NOT GLOB '*[^ -~]*'
    ),
    correspondence_profile_version INTEGER NOT NULL
        CHECK (correspondence_profile_version BETWEEN 1 AND 4294967295),
    correspondence_profile_digest BLOB NOT NULL
        CHECK (length(correspondence_profile_digest) = 32),
    lifecycle_state TEXT NOT NULL CHECK (lifecycle_state IN ('staging', 'complete')),
    searched_count INTEGER NOT NULL CHECK (searched_count BETWEEN 0 AND 4096),
    skipped_count INTEGER NOT NULL CHECK (skipped_count BETWEEN 0 AND 4096),
    unresolved_count INTEGER NOT NULL CHECK (unresolved_count BETWEEN 0 AND 4096),
    truncated_count INTEGER NOT NULL CHECK (truncated_count BETWEEN 0 AND 4096),
    total_count INTEGER NOT NULL CHECK (total_count BETWEEN 0 AND 4096),
    current_count INTEGER NOT NULL CHECK (current_count BETWEEN 0 AND 4096),
    not_applicable_count INTEGER NOT NULL
        CHECK (not_applicable_count BETWEEN 0 AND 4096),
    stale_count INTEGER NOT NULL CHECK (stale_count BETWEEN 0 AND 4096),
    needs_review_count INTEGER NOT NULL
        CHECK (needs_review_count BETWEEN 0 AND 4096),
    indeterminate_count INTEGER NOT NULL
        CHECK (indeterminate_count BETWEEN 0 AND 4096),
    conflicted_count INTEGER NOT NULL CHECK (conflicted_count BETWEEN 0 AND 4096),
    contradicted_count INTEGER NOT NULL
        CHECK (contradicted_count BETWEEN 0 AND 4096),
    superseded_count INTEGER NOT NULL CHECK (superseded_count BETWEEN 0 AND 4096),
    quarantined_count INTEGER NOT NULL CHECK (quarantined_count BETWEEN 0 AND 4096),
    tombstoned_count INTEGER NOT NULL CHECK (tombstoned_count BETWEEN 0 AND 4096),
    CHECK (
        (
            target_kind = 'git'
            AND (
                (target_format = 'sha1' AND length(target_revision) = 20)
                OR (target_format = 'sha256' AND length(target_revision) = 32)
            )
            AND head_format IS NULL
            AND head_revision IS NULL
        )
        OR
        (
            target_kind = 'worktree'
            AND target_format = 'source_snapshot'
            AND length(target_revision) = 32
            AND target_revision = snapshot_digest
            AND (
                (head_format IS NULL AND head_revision IS NULL)
                OR (head_format = 'sha1' AND length(head_revision) = 20)
                OR (head_format = 'sha256' AND length(head_revision) = 32)
            )
        )
    ),
    UNIQUE (projection_id, workspace_id),
    FOREIGN KEY (workspace_id, index_generation_id)
        REFERENCES index_generations(workspace_id, generation_id),
    FOREIGN KEY (snapshot_digest) REFERENCES source_snapshots(snapshot_digest)
) STRICT;
CREATE TABLE memory_projection_records (
    projection_id INTEGER NOT NULL,
    workspace_id INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 4095),
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB CHECK (
        revision_digest IS NULL OR length(revision_digest) = 32
    ),
    effective_state TEXT NOT NULL CHECK (
        effective_state IN (
            'current', 'not_applicable', 'stale', 'needs_review',
            'indeterminate', 'conflicted', 'contradicted', 'superseded',
            'quarantined', 'tombstoned'
        )
    ),
    validity_state TEXT NOT NULL CHECK (
        validity_state IN ('valid', 'invalid', 'indeterminate', 'not_evaluated')
    ),
    evidence_state TEXT NOT NULL CHECK (
        evidence_state IN (
            'exact', 'corresponded', 'changed', 'ambiguous', 'missing',
            'indeterminate', 'conflicted', 'not_evaluated'
        )
    ),
    reason TEXT NOT NULL CHECK (
        reason IN (
            'evidence_exact', 'evidence_corresponded', 'evidence_changed',
            'evidence_ambiguous', 'evidence_missing', 'evidence_indeterminate',
            'project_not_applicable', 'project_indeterminate',
            'authored_needs_review', 'authored_stale',
            'authored_contradicted', 'authored_superseded',
            'authored_quarantined', 'authored_tombstoned',
            'approved_head_conflict', 'missing_parent', 'invalid_head_graph'
        )
    ),
    evidence_count INTEGER NOT NULL CHECK (evidence_count BETWEEN 0 AND 16),
    resolved_count INTEGER NOT NULL CHECK (resolved_count BETWEEN 0 AND 16),
    review_count INTEGER NOT NULL CHECK (review_count BETWEEN 0 AND 16),
    indeterminate_count INTEGER NOT NULL
        CHECK (indeterminate_count BETWEEN 0 AND 16),
    head_count INTEGER NOT NULL CHECK (head_count BETWEEN 0 AND 4096),
    missing_parent_count INTEGER NOT NULL
        CHECK (missing_parent_count BETWEEN 0 AND 32768),
    has_trusted_approval INTEGER NOT NULL CHECK (has_trusted_approval = 1),
    CHECK (resolved_count + review_count + indeterminate_count <= evidence_count),
    CHECK (
        effective_state != 'conflicted'
        OR (
            revision_digest IS NULL
            AND evidence_state = 'conflicted'
            AND validity_state = 'not_evaluated'
            AND reason = 'approved_head_conflict'
            AND head_count >= 2
        )
    ),
    CHECK (
        reason != 'missing_parent'
        OR (
            effective_state = 'indeterminate'
            AND validity_state = 'not_evaluated'
            AND evidence_state = 'not_evaluated'
            AND missing_parent_count > 0
        )
    ),
    PRIMARY KEY (projection_id, ordinal),
    UNIQUE (projection_id, record_id),
    UNIQUE (
        projection_id, ordinal, workspace_id, record_id, revision_digest
    ),
    FOREIGN KEY (projection_id, workspace_id)
        REFERENCES memory_projection_generations(projection_id, workspace_id),
    FOREIGN KEY (workspace_id, record_id, revision_digest)
        REFERENCES memory_versions(workspace_id, record_id, revision_digest)
) STRICT, WITHOUT ROWID;
CREATE TABLE memory_projection_evidence (
    projection_id INTEGER NOT NULL,
    workspace_id INTEGER NOT NULL,
    record_ordinal INTEGER NOT NULL CHECK (record_ordinal BETWEEN 0 AND 4095),
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB NOT NULL CHECK (length(revision_digest) = 32),
    evidence_ordinal INTEGER NOT NULL CHECK (evidence_ordinal BETWEEN 0 AND 15),
    outcome TEXT NOT NULL CHECK (
        outcome IN (
            'exact', 'same_path_rename', 'git_exact_move', 'reviewed_link',
            'changed', 'ambiguous', 'missing', 'indeterminate'
        )
    ),
    method_id TEXT NOT NULL CHECK (
        length(CAST(method_id AS BLOB)) BETWEEN 1 AND 128
        AND method_id NOT GLOB '*[^ -~]*'
    ),
    method_version INTEGER NOT NULL
        CHECK (method_version BETWEEN 1 AND 4294967295),
    assurance TEXT NOT NULL CHECK (assurance IN ('automatic', 'reviewed', 'none')),
    target_snapshot_digest BLOB,
    target_repository_path BLOB,
    target_artifact_digest BLOB,
    target_fact_ordinal INTEGER,
    target_declaration_digest BLOB,
    target_name_elided_digest BLOB,
    candidate_coverage TEXT NOT NULL CHECK (
        candidate_coverage IN ('complete', 'partial')
    ),
    candidate_count_before_limit INTEGER NOT NULL
        CHECK (candidate_count_before_limit BETWEEN 0 AND 9007199254740991),
    CHECK (
        (
            outcome IN (
                'exact', 'same_path_rename', 'git_exact_move',
                'reviewed_link', 'changed'
            )
            AND length(target_snapshot_digest) = 32
            AND length(target_repository_path) BETWEEN 1 AND 32764
            AND instr(target_repository_path, X'00') = 0
            AND length(target_artifact_digest) = 32
            AND target_fact_ordinal BETWEEN 0 AND 9007199254740991
            AND length(target_declaration_digest) = 32
            AND length(target_name_elided_digest) = 32
        )
        OR
        (
            outcome IN ('ambiguous', 'missing', 'indeterminate')
            AND target_snapshot_digest IS NULL
            AND target_repository_path IS NULL
            AND target_artifact_digest IS NULL
            AND target_fact_ordinal IS NULL
            AND target_declaration_digest IS NULL
            AND target_name_elided_digest IS NULL
        )
    ),
    CHECK (
        (outcome IN ('exact', 'same_path_rename', 'git_exact_move')
         AND assurance IN ('automatic', 'reviewed'))
        OR (outcome = 'reviewed_link' AND assurance = 'reviewed')
        OR (outcome = 'changed' AND assurance = 'none')
        OR (outcome IN ('ambiguous', 'missing', 'indeterminate') AND assurance = 'none')
    ),
    CHECK (
        (
            outcome IN ('indeterminate', 'reviewed_link')
            AND candidate_coverage IN ('complete', 'partial')
        )
        OR (
            outcome NOT IN ('indeterminate', 'reviewed_link')
            AND candidate_coverage = 'complete'
        )
    ),
    CHECK (
        (outcome = 'ambiguous' AND candidate_count_before_limit BETWEEN 1 AND 16)
        OR outcome != 'ambiguous'
    ),
    PRIMARY KEY (projection_id, record_ordinal, evidence_ordinal),
    UNIQUE (
        projection_id, record_ordinal, evidence_ordinal,
        workspace_id, record_id, revision_digest
    ),
    FOREIGN KEY (
        projection_id, record_ordinal, workspace_id, record_id, revision_digest
    ) REFERENCES memory_projection_records(
        projection_id, ordinal, workspace_id, record_id, revision_digest
    ),
    FOREIGN KEY (
        workspace_id, record_id, revision_digest, evidence_ordinal
    ) REFERENCES memory_evidence(
        workspace_id, record_id, revision_digest, ordinal
    ),
    FOREIGN KEY (target_artifact_digest, target_fact_ordinal)
        REFERENCES artifact_facts(artifact_digest, ordinal)
) STRICT, WITHOUT ROWID;
CREATE TABLE memory_projection_candidates (
    projection_id INTEGER NOT NULL,
    workspace_id INTEGER NOT NULL,
    record_ordinal INTEGER NOT NULL CHECK (record_ordinal BETWEEN 0 AND 4095),
    evidence_ordinal INTEGER NOT NULL CHECK (evidence_ordinal BETWEEN 0 AND 15),
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 15),
    record_id BLOB NOT NULL CHECK (length(record_id) = 16),
    revision_digest BLOB NOT NULL CHECK (length(revision_digest) = 32),
    target_snapshot_digest BLOB NOT NULL CHECK (length(target_snapshot_digest) = 32),
    target_repository_path BLOB NOT NULL CHECK (
        length(target_repository_path) BETWEEN 1 AND 32764
        AND instr(target_repository_path, X'00') = 0
    ),
    target_artifact_digest BLOB NOT NULL CHECK (length(target_artifact_digest) = 32),
    target_fact_ordinal INTEGER NOT NULL
        CHECK (target_fact_ordinal BETWEEN 0 AND 9007199254740991),
    target_declaration_digest BLOB NOT NULL
        CHECK (length(target_declaration_digest) = 32),
    target_name_elided_digest BLOB NOT NULL
        CHECK (length(target_name_elided_digest) = 32),
    proposed_relation TEXT NOT NULL CHECK (
        proposed_relation IN (
            'same', 'moved', 'renamed', 'moved_renamed', 'split', 'merged'
        )
    ),
    method_id TEXT NOT NULL CHECK (
        length(CAST(method_id AS BLOB)) BETWEEN 1 AND 128
        AND method_id NOT GLOB '*[^ -~]*'
    ),
    method_version INTEGER NOT NULL
        CHECK (method_version BETWEEN 1 AND 4294967295),
    assurance TEXT NOT NULL CHECK (assurance = 'review_required'),
    PRIMARY KEY (projection_id, record_ordinal, evidence_ordinal, ordinal),
    UNIQUE (
        projection_id, record_ordinal, evidence_ordinal,
        target_repository_path, target_artifact_digest, target_fact_ordinal
    ),
    FOREIGN KEY (
        projection_id, record_ordinal, evidence_ordinal,
        workspace_id, record_id, revision_digest
    ) REFERENCES memory_projection_evidence(
        projection_id, record_ordinal, evidence_ordinal,
        workspace_id, record_id, revision_digest
    ),
    FOREIGN KEY (target_artifact_digest, target_fact_ordinal)
        REFERENCES artifact_facts(artifact_digest, ordinal)
) STRICT, WITHOUT ROWID;
CREATE TABLE active_memory_projections (
    workspace_id INTEGER PRIMARY KEY REFERENCES workspaces(workspace_id),
    projection_id INTEGER NOT NULL UNIQUE,
    FOREIGN KEY (projection_id, workspace_id)
        REFERENCES memory_projection_generations(projection_id, workspace_id)
) STRICT, WITHOUT ROWID;

CREATE TRIGGER active_memory_projection_no_delete
BEFORE DELETE ON active_memory_projections BEGIN
    SELECT RAISE(ABORT, 'active memory projection cannot be deleted');
END;
CREATE TRIGGER active_memory_projection_validate_insert
BEFORE INSERT ON active_memory_projections
WHEN NOT EXISTS (
    SELECT 1
    FROM memory_projection_generations AS projection
    JOIN workspaces AS workspace
      ON workspace.workspace_id = projection.workspace_id
    JOIN index_generations AS generation
      ON generation.generation_id = projection.index_generation_id
     AND generation.workspace_id = projection.workspace_id
    WHERE projection.projection_id = NEW.projection_id
      AND projection.workspace_id = NEW.workspace_id
      AND projection.lifecycle_state = 'complete'
      AND workspace.active_generation_id = projection.index_generation_id
      AND workspace.source_epoch = projection.source_epoch
      AND generation.lifecycle_state = 'active'
      AND generation.source_epoch = projection.source_epoch
      AND generation.snapshot_digest = projection.snapshot_digest
)
BEGIN
    SELECT RAISE(ABORT, 'memory projection cannot be activated');
END;
CREATE TRIGGER active_memory_projection_validate_update
BEFORE UPDATE OF projection_id ON active_memory_projections
WHEN NOT EXISTS (
    SELECT 1
    FROM memory_projection_generations AS projection
    JOIN workspaces AS workspace
      ON workspace.workspace_id = projection.workspace_id
    JOIN index_generations AS generation
      ON generation.generation_id = projection.index_generation_id
     AND generation.workspace_id = projection.workspace_id
    WHERE projection.projection_id = NEW.projection_id
      AND projection.workspace_id = OLD.workspace_id
      AND projection.lifecycle_state = 'complete'
      AND workspace.active_generation_id = projection.index_generation_id
      AND workspace.source_epoch = projection.source_epoch
      AND generation.lifecycle_state = 'active'
      AND generation.source_epoch = projection.source_epoch
      AND generation.snapshot_digest = projection.snapshot_digest
)
BEGIN
    SELECT RAISE(ABORT, 'memory projection cannot be activated');
END;
CREATE TRIGGER active_memory_projection_workspace_no_update
BEFORE UPDATE OF workspace_id ON active_memory_projections BEGIN
    SELECT RAISE(ABORT, 'immutable active projection workspace');
END;
CREATE TRIGGER complete_memory_projection_candidates_no_delete
BEFORE DELETE ON memory_projection_candidates
WHEN (
    SELECT lifecycle_state FROM memory_projection_generations
    WHERE projection_id = OLD.projection_id
) = 'complete'
BEGIN
    SELECT RAISE(ABORT, 'immutable complete memory projection candidates');
END;
CREATE TRIGGER complete_memory_projection_evidence_no_delete
BEFORE DELETE ON memory_projection_evidence
WHEN (
    SELECT lifecycle_state FROM memory_projection_generations
    WHERE projection_id = OLD.projection_id
) = 'complete'
BEGIN
    SELECT RAISE(ABORT, 'immutable complete memory projection evidence');
END;
CREATE TRIGGER complete_memory_projection_generation_no_delete
BEFORE DELETE ON memory_projection_generations
WHEN OLD.lifecycle_state = 'complete'
BEGIN
    SELECT RAISE(ABORT, 'immutable complete memory projection');
END;
CREATE TRIGGER complete_memory_projection_records_no_delete
BEFORE DELETE ON memory_projection_records
WHEN (
    SELECT lifecycle_state FROM memory_projection_generations
    WHERE projection_id = OLD.projection_id
) = 'complete'
BEGIN
    SELECT RAISE(ABORT, 'immutable complete memory projection records');
END;
CREATE TRIGGER memory_projection_candidates_insert_only_while_staging
BEFORE INSERT ON memory_projection_candidates
WHEN NOT EXISTS (
    SELECT 1
    FROM memory_projection_evidence AS evidence
    JOIN memory_projection_generations AS generation
      ON generation.projection_id = evidence.projection_id
    WHERE evidence.projection_id = NEW.projection_id
      AND evidence.record_ordinal = NEW.record_ordinal
      AND evidence.evidence_ordinal = NEW.evidence_ordinal
      AND evidence.workspace_id = NEW.workspace_id
      AND evidence.record_id = NEW.record_id
      AND evidence.revision_digest = NEW.revision_digest
      AND evidence.outcome = 'ambiguous'
      AND generation.lifecycle_state = 'staging'
)
BEGIN
    SELECT RAISE(ABORT, 'memory projection is not accepting candidates');
END;
CREATE TRIGGER memory_projection_candidates_no_update
BEFORE UPDATE ON memory_projection_candidates BEGIN
    SELECT RAISE(ABORT, 'immutable memory projection candidates');
END;
CREATE TRIGGER memory_projection_evidence_insert_only_while_staging
BEFORE INSERT ON memory_projection_evidence
WHEN NOT EXISTS (
    SELECT 1
    FROM memory_projection_records AS record
    JOIN memory_projection_generations AS generation
      ON generation.projection_id = record.projection_id
    WHERE record.projection_id = NEW.projection_id
      AND record.ordinal = NEW.record_ordinal
      AND record.workspace_id = NEW.workspace_id
      AND record.record_id = NEW.record_id
      AND record.revision_digest = NEW.revision_digest
      AND generation.lifecycle_state = 'staging'
)
BEGIN
    SELECT RAISE(ABORT, 'memory projection is not accepting evidence');
END;
CREATE TRIGGER memory_projection_evidence_no_update
BEFORE UPDATE ON memory_projection_evidence BEGIN
    SELECT RAISE(ABORT, 'immutable memory projection evidence');
END;
CREATE TRIGGER memory_projection_generation_completion
BEFORE UPDATE OF lifecycle_state ON memory_projection_generations
WHEN NOT (OLD.lifecycle_state = 'staging' AND NEW.lifecycle_state = 'complete')
BEGIN
    SELECT RAISE(ABORT, 'invalid memory projection transition');
END;
CREATE TRIGGER memory_projection_generation_no_semantic_update
BEFORE UPDATE OF
    projection_id, workspace_id, index_generation_id, source_epoch,
    snapshot_digest, target_kind, target_format, target_revision,
    head_format, head_revision, correspondence_profile_id,
    correspondence_profile_version, correspondence_profile_digest,
    searched_count, skipped_count, unresolved_count, truncated_count,
    total_count, current_count, not_applicable_count, stale_count,
    needs_review_count, indeterminate_count, conflicted_count,
    contradicted_count, superseded_count, quarantined_count, tombstoned_count
ON memory_projection_generations BEGIN
    SELECT RAISE(ABORT, 'immutable memory projection semantics');
END;
CREATE TRIGGER memory_projection_generation_requires_active_index
BEFORE INSERT ON memory_projection_generations
WHEN NOT EXISTS (
    SELECT 1
    FROM index_generations AS generation
    JOIN workspaces AS workspace
      ON workspace.workspace_id = generation.workspace_id
    WHERE generation.generation_id = NEW.index_generation_id
      AND generation.workspace_id = NEW.workspace_id
      AND generation.source_epoch = NEW.source_epoch
      AND generation.snapshot_digest = NEW.snapshot_digest
      AND generation.lifecycle_state = 'active'
      AND workspace.active_generation_id = generation.generation_id
      AND workspace.source_epoch = generation.source_epoch
)
BEGIN
    SELECT RAISE(ABORT, 'projection source generation is not active');
END;
CREATE TRIGGER memory_projection_generation_validate_completion
BEFORE UPDATE OF lifecycle_state ON memory_projection_generations
WHEN
    OLD.lifecycle_state = 'staging'
    AND NEW.lifecycle_state = 'complete'
    AND (
        NEW.total_count != (
            SELECT count(*) FROM memory_projection_records
            WHERE projection_id = NEW.projection_id
        )
        OR NEW.total_count != coalesce((
            SELECT max(ordinal) + 1 FROM memory_projection_records
            WHERE projection_id = NEW.projection_id
        ), 0)
        OR NEW.total_count != (
            NEW.current_count + NEW.not_applicable_count + NEW.stale_count
            + NEW.needs_review_count + NEW.indeterminate_count
            + NEW.conflicted_count + NEW.contradicted_count
            + NEW.superseded_count + NEW.quarantined_count
            + NEW.tombstoned_count
        )
        OR NEW.current_count != (
            SELECT count(*) FROM memory_projection_records
            WHERE projection_id = NEW.projection_id
              AND effective_state = 'current'
        )
        OR NEW.not_applicable_count != (
            SELECT count(*) FROM memory_projection_records
            WHERE projection_id = NEW.projection_id
              AND effective_state = 'not_applicable'
        )
        OR NEW.stale_count != (
            SELECT count(*) FROM memory_projection_records
            WHERE projection_id = NEW.projection_id
              AND effective_state = 'stale'
        )
        OR NEW.needs_review_count != (
            SELECT count(*) FROM memory_projection_records
            WHERE projection_id = NEW.projection_id
              AND effective_state = 'needs_review'
        )
        OR NEW.indeterminate_count != (
            SELECT count(*) FROM memory_projection_records
            WHERE projection_id = NEW.projection_id
              AND effective_state = 'indeterminate'
        )
        OR NEW.conflicted_count != (
            SELECT count(*) FROM memory_projection_records
            WHERE projection_id = NEW.projection_id
              AND effective_state = 'conflicted'
        )
        OR NEW.contradicted_count != (
            SELECT count(*) FROM memory_projection_records
            WHERE projection_id = NEW.projection_id
              AND effective_state = 'contradicted'
        )
        OR NEW.superseded_count != (
            SELECT count(*) FROM memory_projection_records
            WHERE projection_id = NEW.projection_id
              AND effective_state = 'superseded'
        )
        OR NEW.quarantined_count != (
            SELECT count(*) FROM memory_projection_records
            WHERE projection_id = NEW.projection_id
              AND effective_state = 'quarantined'
        )
        OR NEW.tombstoned_count != (
            SELECT count(*) FROM memory_projection_records
            WHERE projection_id = NEW.projection_id
              AND effective_state = 'tombstoned'
        )
        OR EXISTS (
            SELECT 1
            FROM memory_projection_records AS record
            WHERE record.projection_id = NEW.projection_id
              AND (
                  (
                      record.revision_digest IS NOT NULL
                      AND record.evidence_count != (
                          SELECT count(*) FROM memory_evidence AS evidence
                          WHERE evidence.workspace_id = record.workspace_id
                            AND evidence.record_id = record.record_id
                            AND evidence.revision_digest = record.revision_digest
                      )
                  )
                  OR (
                      record.evidence_state IN (
                          'exact', 'corresponded', 'changed', 'ambiguous',
                          'missing', 'indeterminate'
                      )
                      AND record.evidence_count != (
                          SELECT count(*) FROM memory_projection_evidence AS evidence
                          WHERE evidence.projection_id = record.projection_id
                            AND evidence.record_ordinal = record.ordinal
                      )
                  )
                  OR (
                      record.evidence_state IN ('conflicted', 'not_evaluated')
                      AND EXISTS (
                          SELECT 1 FROM memory_projection_evidence AS evidence
                          WHERE evidence.projection_id = record.projection_id
                            AND evidence.record_ordinal = record.ordinal
                      )
                  )
                  OR (
                      SELECT count(*) FROM memory_projection_evidence AS evidence
                      WHERE evidence.projection_id = record.projection_id
                        AND evidence.record_ordinal = record.ordinal
                  ) != coalesce((
                      SELECT max(evidence_ordinal) + 1
                      FROM memory_projection_evidence AS evidence
                      WHERE evidence.projection_id = record.projection_id
                        AND evidence.record_ordinal = record.ordinal
                  ), 0)
              )
        )
        OR EXISTS (
            SELECT 1
            FROM memory_projection_evidence AS evidence
            WHERE evidence.projection_id = NEW.projection_id
              AND (
                  (
                      evidence.outcome = 'ambiguous'
                      AND (
                          evidence.candidate_count_before_limit != (
                              SELECT count(*) FROM memory_projection_candidates AS candidate
                              WHERE candidate.projection_id = evidence.projection_id
                                AND candidate.record_ordinal = evidence.record_ordinal
                                AND candidate.evidence_ordinal = evidence.evidence_ordinal
                          )
                          OR evidence.candidate_count_before_limit != coalesce((
                              SELECT max(ordinal) + 1
                              FROM memory_projection_candidates AS candidate
                              WHERE candidate.projection_id = evidence.projection_id
                                AND candidate.record_ordinal = evidence.record_ordinal
                                AND candidate.evidence_ordinal = evidence.evidence_ordinal
                          ), 0)
                      )
                  )
                  OR (
                      evidence.outcome != 'ambiguous'
                      AND EXISTS (
                          SELECT 1 FROM memory_projection_candidates AS candidate
                          WHERE candidate.projection_id = evidence.projection_id
                            AND candidate.record_ordinal = evidence.record_ordinal
                            AND candidate.evidence_ordinal = evidence.evidence_ordinal
                      )
                  )
              )
        )
    )
BEGIN
    SELECT RAISE(ABORT, 'incomplete memory projection');
END;
CREATE TRIGGER memory_projection_records_insert_only_while_staging
BEFORE INSERT ON memory_projection_records
WHEN NOT EXISTS (
    SELECT 1 FROM memory_projection_generations
    WHERE projection_id = NEW.projection_id
      AND workspace_id = NEW.workspace_id
      AND lifecycle_state = 'staging'
)
OR NOT EXISTS (
    SELECT 1 FROM memory_audit
    WHERE workspace_id = NEW.workspace_id
      AND record_id = NEW.record_id
      AND operation = 'locally_approved'
      AND (
          NEW.revision_digest IS NULL
          OR revision_digest = NEW.revision_digest
      )
)
BEGIN
    SELECT RAISE(ABORT, 'memory projection is not accepting records');
END;
CREATE TRIGGER memory_projection_records_no_update
BEFORE UPDATE ON memory_projection_records BEGIN
    SELECT RAISE(ABORT, 'immutable memory projection records');
END;
