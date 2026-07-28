# Phase 0 SQLite schema version 3

> Historical pre-baseline development schema. The current runtime does not
> accept this format. It is retained only as design provenance and was
> superseded by [ADR-0022](../adr/0022-squash-pre-release-sqlite-schema.md) and
> the [current baseline](phase0-sqlite-baseline-v1.md).

- Status: Implemented
- Date: 2026-07-26
- Governing decision:
  [ADR-0012](../adr/0012-phase0-sqlite-schema-and-ownership.md)
- Previous version: [Phase 0 SQLite schema version 2](phase0-sqlite-v2.md)
- Implementation:
  [`crates/repowitness-local/src/sqlite/`](../../crates/repowitness-local/src/sqlite/)

Version 3 preserves the normalized tables, generation lifecycle, ownership,
and double-buffered FTS5 projection from version 2. It adds one independently
canonical integrity identity to `analysis_artifacts`:

```sql
payload_digest BLOB
CHECK (payload_digest IS NULL OR length(payload_digest) = 32)
```

The artifact key digest answers whether source and every semantics-affecting
producer input match. The payload digest separately commits to the complete
ordered output: payload format version, visited-node and syntax-error counts,
fact count, and every fact's ordinal, kind, names, and byte spans. Both use
domain-separated SHA-256 encodings.

## Migration and immutability

The version-1 and version-2 migration text and checksums remain unchanged.
Opening an older supported database validates its exact ledger before applying
the additive version-3 migration in one immediate transaction.

Existing artifacts receive a null payload digest because the migration cannot
derive a trustworthy value from metadata alone. A null artifact is never
reused for analysis. When current analysis later produces the same artifact
key, the writer compares all metadata and every ordered fact row against the
fresh result before setting the digest once. New artifacts store the digest
before completion. A trigger permits only the legacy null-to-valid-digest
transition and rejects every later payload-digest update.

## Bounded production reuse

After bounded source discovery and contained reads, repeat indexing computes
the exact requested artifact keys. If a current database already exists, one
owned read-only connection loads only those sorted unique digests:

1. require a complete artifact and a non-null 32-byte payload digest;
2. validate every typed digest, fixed-width count, producer identity, and
   canonicalization version;
3. recompute the complete artifact-key digest;
4. read facts in exact contiguous ordinal order and reconstruct only validated
   analysis values;
5. enforce the requested file and aggregate fact limits plus a 512 MiB encoded
   artifact-load ceiling;
6. recompute and compare the canonical payload digest; and
7. return no inventory on cancellation, deadline, corruption, or limit
   failure.

The pure application preparation then validates every reused name and
declaration span against the current exact source bytes. A changed content
digest or producer/configuration/schema/canonicalization identity has a
different key and is analyzed normally. Clean and incremental preparation
therefore produce the same manifest and logical facts while reporting separate
fixed-width analyzed and reused file counts.

An absent database remains absent until preparation succeeds. Older supported
schema versions skip the read-only reuse optimization, migrate on the owned
writer, analyze legacy artifacts once, and become reusable after verified
backfill.

## Validation

Production tests cover:

- exact fresh migration, idempotent reopen, and explicit version-1 and
  version-2 upgrade ledger projections;
- stable migration and payload-hash golden vectors;
- unchanged all-file reuse and one-file invalidation;
- producer-identity invalidation;
- clean-versus-incremental logical equivalence;
- malformed kind, count, span, ordinal, source-name, key, and payload
  rejection before reuse;
- cancellation and deadline with no partial inventory;
- complete artifact-fact corruption without generation activation;
- legacy null payload analysis, exact writer verification, one-time backfill,
  and subsequent reuse; and
- installed CLI reports for analyzed-versus-reused work.

Pinned-corpus cold/warm persistence, reuse, query, and MCP resource
measurements remain release-budget gates.
