-- RepoWitness: one logical memory read boundary over the immutable
-- version-1 and current-profile journal tables. The physical tables remain
-- separate until their child-row and projection migrations are complete.

CREATE VIEW memory_versions_all AS
SELECT workspace_id, record_id, revision_digest, schema_version,
       canonical_json, kind, title, body, subject_evidence,
       provenance_origin, authored_actor_kind, authored_actor_id,
       authored_assurance, authored_lifecycle, validity_kind,
       validity_source_snapshot, tombstone
FROM memory_versions
UNION ALL
SELECT workspace_id, record_id, revision_digest, schema_version,
       canonical_json, kind, title, body, subject_evidence,
       provenance_origin, authored_actor_kind, authored_actor_id,
       authored_assurance, authored_lifecycle, validity_kind,
       validity_source_snapshot, tombstone
FROM memory_profile_v2_versions;

CREATE VIEW memory_audit_all AS
SELECT event_id, workspace_id, record_id, revision_digest, operation,
       trusted_actor_kind, trusted_actor_id, recorded_at_unix_ms,
       source_kind, source_format, source_revision, display_revision,
       presentation_digest, 1 AS schema_version
FROM memory_audit
UNION ALL
SELECT event_id, workspace_id, record_id, revision_digest, operation,
       trusted_actor_kind, trusted_actor_id, recorded_at_unix_ms,
       source_kind, source_format, source_revision, display_revision,
       presentation_digest, 2 AS schema_version
FROM memory_profile_v2_audit;

-- Ordinary memory reads need only the current trust decision. Historical
-- observation and approval rows remain internal provenance for recovery and
-- explicit history operations; they are never the default trust surface.
CREATE VIEW memory_current_trust AS
SELECT event_id, workspace_id, record_id, revision_digest,
       trusted_actor_kind, trusted_actor_id, recorded_at_unix_ms,
       source_kind, source_format, source_revision, display_revision,
       presentation_digest, schema_version
FROM memory_audit_all
WHERE operation = 'locally_approved';
