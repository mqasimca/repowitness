# ADR-0012: Use a versioned immutable-generation SQLite schema and owned connections

- Status: Accepted
- Date: 2026-07-25
- Owners: Project maintainers
- Scope: Phase 0 SQLite schema, migrations, generation recovery, connection ownership, and backups

## Context

[ADR-0002](0002-sqlite-first.md) selects SQLite for the local profile and
[ADR-0006](0006-immutable-index-generations.md) requires reusable immutable
artifacts, staging generations, atomic activation, pinned readers, and
old-generation survival after failure. The
[durability spike](../research/sqlite-generation-spike-2026-07-23.md) now
exercises WAL-reset version checks, crash recovery, checkpoint starvation,
owned connections, online backup and cancellation, and candidate transaction
sizes.

Moving `rusqlite` from a test-only dependency requires a concrete schema and
ownership contract. The design must not make unresolved repository,
snapshot, artifact, or producer encodings accidental database APIs.

SQLite reserves `PRAGMA user_version` for applications and recommends an
`application_id` for application file formats. `STRICT` tables add rigid type
checking, while explicit transactions are required to control the single
writer and atomic activation boundaries. See SQLite's
[PRAGMA](https://sqlite.org/pragma.html),
[STRICT-table](https://sqlite.org/stricttables.html), and
[transaction](https://sqlite.org/lang_transaction.html) documentation.

## Decision

### Database identity and migration ledger

- Use application ID `0x52575031` (`RWP1`) and schema `user_version = 1`.
- Maintain a `schema_migrations` ledger containing the positive migration
  version, stable name, and a 32-byte checksum of the exact migration.
- Startup accepts only the expected application ID, a supported
  `user_version`, and a ledger that exactly matches compiled migrations.
  Mismatch fails closed without serving requests.
- Apply migrations before transport startup on the owned writer connection.
  Each migration uses one explicit immediate transaction. A new database is
  created transactionally; a future destructive migration requires a verified
  online backup first.
- Migration timestamps are diagnostic metadata, not migration identity or
  ordering. Tests inject them.
- Never modify SQLite's internal `schema_version`.

### Schema shape

Schema version 1 uses `STRICT` tables, explicit `CHECK` limits, fixed-width
integer fields, lossless path BLOBs, and digest BLOBs whose lengths are checked
at the boundary and in SQL.

The initial ownership groups are:

- `workspaces`: stable workspace identity, monotonic source epoch, and the
  nullable active-generation pointer;
- `source_snapshots` and `source_manifest_entries`: exact snapshot metadata
  plus canonical path-ordered file type and content digest entries;
- `analysis_artifacts` and `artifact_facts`: immutable artifact keys,
  integrity digest, producer identity, and bounded ordered local facts;
- `index_generations` and `generation_files`: lifecycle, snapshot, coverage,
  and the generation's path-to-source-to-artifact mapping;
- `generation_facts`: bounded ordered cross-file facts that are not yet safe
  to reuse independently;
- a generation-scoped FTS5 projection rebuilt from validated searchable facts.

Opaque identities are stored only after their focused boundary schema defines
their canonical bytes. Version 1 does not persist target-local `PathBuf`,
`usize`, enum discriminants, YAML bytes, randomized hashes, or lossy display
text. Database rows do not become domain values until their type, version,
length, relationship, and digest checks pass.

Artifacts and snapshots are immutable after insertion. Generation-local rows
may be appended only while their generation is in a staging state. Triggers
and writer checks reject mutation of active, retained, failed, or cancelled
generations.

### Lifecycle and activation

The stored lifecycle is:

```text
discovered -> extracting -> resolving -> validating -> ready -> active
       |            |            |             |          |
       +------------+------------+-------------+----------+-> failed/cancelled

active -> retained
```

- Every transition compares the expected prior state and source epoch.
- Writes use `BEGIN IMMEDIATE` transactions containing at most 256 fact rows
  or a stricter configured byte bound.
- Validation records exact searched, skipped, unresolved, and truncated
  coverage before a generation becomes `ready`.
- Activation is one short transaction: confirm `ready`, confirm the expected
  source epoch is still current, retain the previous active generation,
  activate the candidate, and change the workspace pointer.
- Readers begin one read transaction, resolve the active pointer once, and
  use only that generation until the request ends.
- A newer source epoch, cancellation, validation failure, database error, or
  process restart never advances the active pointer.

Startup recovery marks every incomplete generation failed and deletes only its
generation-scoped mutable facts and FTS rows. Immutable source snapshots and
artifacts remain eligible for verified reuse. Recovery is idempotent and
records a stable diagnostic outcome.

### Connection ownership and resource policy

- One OS thread owns the only long-lived write connection and receives
  commands through a capacity-one queue.
- A fixed configured number of read workers each owns one read connection.
  A read transaction has a deadline and cooperative cancellation flag and
  never crosses `.await`.
- Replies are bounded one-shot channels. Queue-full, reply-timeout,
  cancellation, deadline, stale-epoch, and contention outcomes have stable
  redacted diagnostics.
- Connections enable foreign keys, disable trusted schema, set a fixed busy
  timeout, use WAL with automatic checkpoints disabled, and verify the
  bundled SQLite is at least 3.51.3.
- Phase 0 uses `synchronous=FULL`, explicit checkpoint scheduling, and a
  maximum 256-row write transaction. These defaults may change only after
  corpus measurements preserve the same correctness gates.
- Checkpoint policy observes latency, busy results, frame progress, and WAL
  bytes. It never blocks activation indefinitely and diagnoses long reader
  pins.

### Backup, restore, and migration safety

- Live backups use SQLite's online backup API through a separately owned
  connection; the rusqlite backup handle is neither `Send` nor `Sync` and
  remains on its owner thread.
- Backup work has page-step, duration, retry, and cancellation bounds. A
  partial destination is never renamed or reported as a completed backup.
- Restore first checks application ID, supported migration ledger,
  `integrity_check`, foreign keys, active-pointer/lifecycle agreement, and
  artifact/snapshot referential integrity.
- Checkpointing is not a substitute for backup. Copying the main database file
  while WAL mode is active is unsupported.

## Alternatives considered

### One mutable set of active rows

This minimizes retained data but permits mixed-generation reads and makes
cancellation, incremental equivalence, and evidence provenance harder to
prove.

### One transaction for the complete index

This is simple but makes transaction duration and cancellation latency grow
with repository size and delays crash-visible progress.

### Private in-memory assembly followed by backup

The spike produced equivalent rows, but the initial sample was slower, used
more peak memory, and did not improve the accepted recovery model.

### A generic storage-backend schema or trait

Phase 0 has no second backend. A generic contract would prematurely hide
SQLite-specific search, locking, backup, and migration behavior.

## Consequences

### Positive

- The production boundary follows the already-tested generation semantics.
- Database identity and migrations fail closed instead of guessing.
- Reader pinning, bounded writes, cancellation, and backup ownership are
  explicit.
- Snapshot and artifact reuse avoid copying local facts into every generation.

### Negative and risks

- Retained immutable data requires later reachability-based garbage collection.
- FTS5 is generation-scoped and requires explicit cleanup/rebuild behavior.
- A capacity-one writer favors predictability over maximum write throughput.
- Exact schema columns remain blocked on the focused canonical encodings named
  below; this ADR cannot turn generic placeholder types into persisted blobs.

## Validation

- Fresh creation and exact schema/ledger introspection.
- Upgrade from every supported `user_version`, repeated migration,
  interruption, unknown future version, wrong application ID, and checksum
  mismatch.
- Atomic activation with concurrent pinned readers under `FULL` and `NORMAL`.
- Cancellation, stale epoch, database errors, and process termination in every
  staging state.
- Clean-versus-incremental logical equivalence and artifact corruption.
- Queue saturation, reply timeout, reader deadline, shutdown, and restart.
- Checkpoint starvation, bounded WAL growth, online-backup interleaving,
  cancellation, restore, and post-restore recovery.
- Invalid types, lengths, states, paths, digests, foreign keys, and hostile
  database files fail before domain construction.

## Follow-up

- Completed 2026-07-26: construct production repository, Git/worktree,
  resolved-configuration, schema, grammar, and producer identities at the
  local boundary.
- Completed 2026-07-26: connect shared application publication to the CLI and
  shared retrieval use cases to both CLI and local stdio MCP DTO boundaries.
- Remaining: add pinned-corpus persistence, reuse, retrieval, MCP, and rebuild
  measurements before
  ratifying resource budgets.

## Implementation status

Accepted and first implemented on 2026-07-25. The exact schema and canonical
snapshot encoding are recorded in the
[Phase 0 SQLite v3 schema](../schemas/phase0-sqlite-v3.md). Production code now
enforces migration identity, semantic immutability, bounded staging, recovery,
atomic activation, owned capacity-one writer and reader queues, cancellable
generation-scoped FTS5 retrieval, explicit checkpointing, and validated
no-clobber online backup. Schema version 2 adds bounded double-buffered FTS5
projection rebuild, integrity checking, atomic slot publication, and
version-1 upgrade coverage without changing migration 1. A shared application
use case stages and activates through the narrow port implemented by the
SQLite owner. Schema version 3 adds an independent canonical artifact-payload
digest, one-time verified legacy backfill, bounded exact inventory loading,
and production clean-versus-incremental reuse without changing earlier
migrations. CLI and local stdio MCP composition are implemented. Remaining
Phase 0 integration work includes pinned-corpus full-index, reuse, query, MCP,
and rebuild budgets. Startup now also bounds recovery to 4,096 incomplete
generations with rollback on cancellation, deadline, or overflow. The
production database boundary rejects hard-link aliases and replacement races,
keeps an identity-checked file guard through writer startup, opens SQLite with
no-follow semantics, revalidates after SQLite opens, and cleans up only an
identity-matched newly reserved database after failed startup.

The later [version-4 schema](../schemas/phase0-sqlite-v4.md) is a forward
migration that preserves these accepted ownership and generation rules while
adding Go-and-Rust artifact language under proposed
[ADR-0015](0015-phase0-go-and-rust-indexing.md). It does not rewrite the
accepted version-1 decision or historical migrations.

## Supersession

The exact migration identity and supported pre-release upgrade chain are
superseded by
[ADR-0022](0022-squash-pre-release-sqlite-schema.md). The ownership,
immutable-generation, recovery, backup, and fail-closed ledger decisions remain
accepted.
