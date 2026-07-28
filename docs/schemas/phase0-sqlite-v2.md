# Phase 0 SQLite schema version 2

> Historical pre-baseline development schema. The current runtime does not
> accept this format. It is retained only as design provenance and was
> superseded by [ADR-0022](../adr/0022-squash-pre-release-sqlite-schema.md) and
> the [current schema](phase0-sqlite-current-v2.md).

- Status: Implemented
- Date: 2026-07-26
- Governing decision:
  [ADR-0012](../adr/0012-phase0-sqlite-schema-and-ownership.md)
- Previous version: [Phase 0 SQLite schema version 1](phase0-sqlite-v1.md)
- Current successor: [Phase 0 SQLite schema version 3](phase0-sqlite-v3.md)
- Implementation:
  [`crates/repowitness-local/src/sqlite/`](../../crates/repowitness-local/src/sqlite/)

Version 2 preserves every normalized table, identity, lifecycle, trigger, and
resource contract from version 1. It adds a second disposable FTS5 projection
and one singleton pointer:

```text
generation_search          projection slot 0
generation_search_rebuild  projection slot 1
search_projection_state    atomically selected active slot
```

The version-1 migration text and checksum remain unchanged. Opening a valid
version-1 database validates its exact ledger before applying the additive
version-2 migration in one immediate transaction. Existing
`generation_search` rows remain active in slot 0; the new inactive slot starts
empty. Fresh databases record both exact migrations.

## Rebuild and publication

A production rebuild derives the complete searchable projection from
`index_generations`, `generation_files`, and immutable `artifact_facts`.
Only `ready`, `active`, and `retained` generations participate.

1. Resolve the active slot on the single writer-owner thread.
2. Count authoritative rows and reject zero, oversized, cancelled, or expired
   rebuild limits before publication.
3. Drop and recreate only the inactive FTS5 table. This also recovers a
   missing or internally damaged inactive projection.
4. Populate it in deterministic
   `(generation_id, file_ordinal, fact_ordinal)` keyset order. Each immediate
   transaction inserts at most 256 rows.
5. Check the exact row count and run the FTS5 `integrity-check` command.
6. Recheck cancellation and the absolute deadline.
7. Change the singleton active-slot pointer in one transaction.

The old slot remains readable during every build batch. A reader resolves the
workspace's active generation, concrete snapshot, producer manifest, stored
index coverage, and projection slot in one read transaction. It obtains an
exact generation-scoped match count before applying the row limit, then reads
candidates from the same snapshot. A query therefore observes either the
complete old projection or the complete new projection and can report exact
truncation. Cancellation, deadline, row-limit, database, count, or integrity
failure leaves the pointer unchanged. The old slot is retained as disposable
inactive state and is recreated on the next rebuild.

The default rebuild ceiling is 5,000,000 authoritative rows and the hard
Phase 0 ceiling is 100,000,000 rows. SQLite progress callbacks observe
cancellation and the absolute deadline during scans and FTS work. Outcomes
report the previous and published slots, exact rebuilt row count, and number
of bounded write batches.

## Exact occurrence lookup

`symbol_get` does not depend on either disposable FTS table. On the owned
reader thread it first resolves the workspace's active snapshot and generation
in a read transaction, requires both to match the caller's expected context,
then joins `generation_files` to immutable `artifact_facts` by the complete
generation, path, content digest, artifact digest, and fact ordinal selector.
The lookup returns at most one occurrence because those columns are already
covered by authoritative primary and uniqueness constraints. A missing exact
row remains an explicit unresolved result; it is never replaced by a
same-named occurrence.

SQLite supplies identity, producer, coverage, and source-span metadata only.
The local source adapter separately performs a capability-contained no-follow
read, verifies the complete file digest, validates the persisted declaration
span against those bytes, and returns only the selected bounded declaration.
An FTS rebuild cannot alter this result.

## Validation

Production tests cover:

- exact fresh migration and idempotent reopen;
- upgrade from version 1 without rewriting its ledger or active projection;
- a rebuild larger than one 256-row transaction;
- inclusive limits and failure without a slot switch;
- cancellation without a slot switch;
- logical search equivalence after rebuilding a damaged active projection;
- exact pre-limit counts and application material-result mapping;
- exact active-context occurrence lookup, explicit missing rows, and
  declaration retrieval from real worktrees before and after projection
  rebuild;
- a read transaction pinned across publication;
- repeated slot alternation; and
- recovery when the inactive FTS5 table is missing.

Pinned-corpus rebuild latency, database-size amplification, and cold/warm
resource measurements remain release-budget gates.
