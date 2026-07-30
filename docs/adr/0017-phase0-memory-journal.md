# ADR-0017: Persist an append-only Phase 0 memory journal in SQLite

- Status: Accepted
- Date: 2026-07-27
- Last reviewed: 2026-07-29
- Owners: Project maintainers
- Scope: Schema-v5 memory versions, observations, local approval, and writer ownership

## Context

[ADR-0005](0005-git-dag-temporal-memory.md) separates project-valid time from
system-recorded time. [ADR-0007](0007-git-memory-synchronization.md) requires
idempotent import, immutable version history, append-only audit evidence, and a
rebuildable current projection. [ADR-0014](0014-phase0-engineering-memory-record.md)
now fixes the accepted version-1 record, canonical JSON, and digest contract.

The production database is already at released schema version 4. Editing an
earlier migration would invalidate existing databases and their migration
ledgers. Adding all correspondence, ancestry, conflict, and retrieval policy to
the first persistence change would also stabilize unresolved behavior before
its evidence exists.

The immediate boundary must preserve every validated observation without
treating authored `locally_approved` text as trusted approval. It must remain
idempotent, bounded, redacted, and owned by the existing single SQLite writer.

## Decision

### Forward-only schema

Add schema version 5 as a new transactional migration. Do not change migrations
1 through 4 or their checksums.

Version 5 adds:

- `memory_versions`, keyed by workspace, 128-bit record ID, and canonical
  revision digest;
- ordered parent, validity-commit, Rust-evidence, and relationship child
  tables;
- `memory_audit`, an append-only journal of `observed` and
  `locally_approved` events; and
- immutable-update/delete triggers for every version and audit table.

The exact columns and checks are fixed in the
[schema-v5 document](../schemas/phase0-sqlite-v5.md).

### Stored representation

Persist accepted canonical JSON plus normalized validated fields. Do not store
raw YAML, comments, source snippets, host paths, enum discriminants, `usize`,
or unvalidated parser DTOs.

The canonical JSON and normalized rows must agree with the supplied domain
record before the writer commits. Re-importing an existing revision verifies
the stored canonical bytes and normalized identity. Any disagreement is
corruption or a digest collision and fails closed.

Presentation metadata is not semantic version identity. Each observation
therefore records the display revision and a standard SHA-256 digest of the
exact admitted YAML bytes without retaining those bytes.

### Trusted audit boundary

An imported file can claim authored assurance but cannot approve itself.
Trusted local import receives a separately validated configured actor and an
injected nonnegative Unix-millisecond timestamp. In one immediate transaction,
the writer:

1. inserts or verifies the immutable version and child rows;
2. inserts an idempotent `observed` event for the exact source and presentation
   receipt; and
3. inserts at most one `locally_approved` event for that version and configured
   actor.

An observation source is either a raw SHA-1/SHA-256 Git commit identity or an
exact source-snapshot digest. Source and presentation receipts are evidence,
not secrets, and debug/error output still redacts their bytes.

Repeated import of the same semantic version is successful and reports which
rows were already present. A different revision of the same record is appended;
it never overwrites an earlier revision.

### Ownership and limits

Extend the existing capacity-one owned SQLite writer rather than opening a
second write connection. The application layer owns the import request, scope
check, and narrow port. The local adapter owns hostile file admission,
canonicalization, presentation hashing, SQLite encoding, and transactions.

One record has the bounds already accepted by ADR-0014. One import transaction
contains at most one version, 8 parents, 32 validity commits, 16 evidence rows,
16 relationships, and two audit rows. Cancellation and the absolute deadline
are checked before canonicalization, queueing, transaction start, each bounded
child loop, and commit.

### Explicitly deferred

Schema version 5 does not decide which observed versions are current,
conflicted, stale, or retrievable. It does not traverse Git history, infer
correspondence, delete missing files, or activate authored assurance without a
trusted audit event. The next decision adds the immutable current projection
only after correspondence and Git-DAG validity fixtures establish its states
and coverage.

## Alternatives considered

### Store raw YAML only

This preserves presentation but makes identity and query behavior parser
dependent, retains comments unnecessarily, and conflicts with ADR-0007.

### Store only canonical JSON

It is compact, but every query and validation would need to deserialize the
whole record. Normalized immutable child rows make exact evidence and temporal
work bounded while canonical JSON remains the integrity source.

### Update one current row per record

This loses divergent versions and system-recorded history and would silently
embed unresolved conflict policy.

### Add the current projection in version 5

This saves a migration but would force unresolved correspondence, ancestry,
partial-history, and conflict states into a durable schema prematurely.

### Use a second memory writer

That complicates SQLite contention, migration, checkpoint, backup, and
shutdown ownership without creating an independent security boundary.

## Consequences

### Positive

- Existing version-4 databases migrate forward without historical changes.
- Every validated version and trusted audit event is retained and idempotent.
- Authored assurance stays separate from trusted local approval.
- Raw potentially sensitive YAML is not duplicated into SQLite.
- The later current projection can be rebuilt from exact immutable inputs.

### Negative and risks

- Canonical JSON and normalized rows duplicate some semantic data.
- No memory is retrievable as current until the next projection is implemented.
- Append-only history requires a later retention/export policy.
- A future projection requires another forward migration.

## Validation

- Exact migration-5 name/checksum and upgrades from versions 1 through 4.
- Fresh schema introspection and rejection of unknown future versions.
- Golden commit and worktree records persist with exact canonical identity.
- Repeated import is idempotent; a second semantic revision appends.
- Same semantic version with a new display revision adds an observation without
  creating a new version or duplicate approval.
- Canonical/normalized disagreement and database corruption fail closed.
- Parent, validity, evidence, relationship, text, integer, and digest SQL
  checks reject invalid rows.
- Direct update/delete of versions, child rows, or audit rows is rejected.
- Authored assurance without a trusted approval event is never considered
  approved.
- Cancellation, deadline, queue saturation, transaction rollback, shutdown,
  backup/restore, and reopen preserve append-only history.
- Worktree admission rejects ID/filename mismatch, traversal, alternate case,
  symlinks, hard links, special files, malformed YAML, and over-limit bytes.

## Follow-up

- Accepted 2026-07-29 after the append-only import, trust-separation,
  idempotency, corruption, rollback, reopen, backup, hostile-path, and complete
  release-matrix tests passed. The clean release-platform product benchmark
  also passed without changing the journal contract.
- Implemented 2026-07-27: schema version 5, the application import port, the
  local owned-writer adapter, and capability-contained worktree admission,
  including migration, idempotency, rollback, corruption, reopen, and online
  backup fixtures.
- Implemented 2026-07-27: precision-first correspondence, Git-DAG validity,
  conflict states, projection coverage, atomic projection activation, and
  current-memory retrieval under ADR-0018.
- Implemented 2026-07-28: bounded Git-tree history import observes exact
  record blobs without self-approval under ADR-0021.
- Completed 2026-07-28: rewritten and pruned history retain prior observations
  without creating approval, while missing-object validity stays
  indeterminate.
- Define archival/export policy in a later decision.

## Supersession

The pre-release schema-version and migration-compatibility clauses are
superseded by [ADR-0022](0022-squash-pre-release-sqlite-schema.md). The
append-only journal and trust-boundary proposal remains otherwise unchanged.
