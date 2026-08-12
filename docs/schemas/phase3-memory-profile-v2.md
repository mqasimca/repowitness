# Current engineering-memory profile (schema version 2)

This is the current profile behind the single user-facing engineering-memory
format. It widens the accepted legacy profile without changing version-1
canonical fixtures. Existing version-1 documents remain accepted as legacy
compatibility input; users do not need a separate command or workflow.

## Kinds

Version 2 admits `decision`, `failure`, `fact`, `procedure`, `episode`,
`preference`, and `policy`. All records retain bounded titles and bodies,
typed source evidence, explicit validity, immutable revision parentage,
canonical semantic digests, and append-only observation/approval records.

`procedure` is not current verified guidance merely because it is active or
approved. A separate successful task-verification receipt is required before a
caller may treat it as verified guidance. `policy` is descriptive memory only;
it is not command or authorization authority.

## Lifecycle

The lifecycle states are `active`, `needs_review`, `stale`, `contradicted`,
`superseded`, `quarantined`, and `tombstoned`. Lifecycle policy evaluation is
deterministic and produces a disposition of retain, review, archive, or
tombstone. Retention evaluation never deletes a revision or silently rewrites
its state.

The domain policy profile is versioned independently as
`MEMORY_LIFECYCLE_POLICY_VERSION = 1`. State transitions require an explicit
reason. Tombstoned records cannot be resurrected; supersession requires an
explicit successor relationship.

## Storage boundary

Current-profile team records are stored in the same normalized immutable
`memory_versions`, `memory_version_parents`, `memory_validity_commits`,
`memory_evidence`, `memory_relationships`, and `memory_audit` tables as legacy
records. SQLite migration 13 introduced temporary v2 compatibility tables;
migration 15 backfills them into the unified tables and keeps those physical
tables archival. `memory_versions_all` and `memory_audit_all` remain bounded
logical views, plus `memory_current_trust` for the single logical trust
decision used by ordinary journal loading, recall, projection, retention, and
review checks. Historical rows are not a user-facing feature.
Canonical JSON is retained as the integrity-bearing representation so the
typed domain parser can revalidate every v2 record on read.

The domain policy prevents active procedures from being treated as verified
guidance without a separate successful verification receipt. Policy and
retention evaluation are deliberately pure and non-destructive until an
application-owned lifecycle mutation supplies an explicit audit event.
