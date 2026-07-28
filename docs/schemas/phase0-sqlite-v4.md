# Phase 0 SQLite schema version 4

> Historical pre-baseline development schema. The current runtime does not
> accept this format. It is retained only as design provenance and was
> superseded by [ADR-0022](../adr/0022-squash-pre-release-sqlite-schema.md) and
> the [current schema](phase0-sqlite-current-v2.md).

- Status: Implemented
- Date: 2026-07-27
- Governing direction:
  [ADR-0015](../adr/0015-phase0-go-and-rust-indexing.md) and
  [ADR-0016](../adr/0016-phase0-typescript-and-tsx-indexing.md) (accepted)
- Previous version: [Phase 0 SQLite schema version 3](phase0-sqlite-v3.md)
- Implementation:
  [`crates/repowitness-local/src/sqlite/`](../../crates/repowitness-local/src/sqlite/)

Version 4 preserves the immutable generation lifecycle, independent artifact
payload identity, owned connections, and double-buffered FTS5 projection from
version 3. It adds an immutable language identity to every analysis artifact:

```sql
language TEXT NOT NULL DEFAULT 'rust'
CHECK (language IN ('rust', 'go', 'typescript', 'tsx'))
```

The migration defaults existing version-1 through version-3 artifacts to
`rust`, which is the only language those schema versions could produce. New
artifacts persist their validated `rust`, `go`, `typescript`, or `tsx`
language. The artifact semantic-immutability trigger includes the language
column, and exact reuse requires both the requested language-specific artifact
digest and matching persisted language.

## Fact kinds

Version 4 rebuilds `artifact_facts` transactionally to retain every historical
row while extending the closed declaration-kind set with:

- `interface`;
- `defined_type`;
- `variable`; and
- `class`.

Existing Rust kinds keep their original encoding. Go structs use `struct`,
true aliases use `type_alias`, constants use `constant`, and receiver
declarations use `method`. The new kinds avoid misrepresenting Go interfaces,
defined types, or package variables as Rust concepts. TypeScript and TSX use
`class`, `interface`, `enum`, `type_alias`, `module`, `function`, `method`, and
`variable` according to the syntax-only extraction profile.

The FTS5 tables remain disposable projections and do not duplicate language.
Search and exact retrieval join through the authoritative generation-to-
artifact mapping and read `analysis_artifacts.language`. Boundary results
therefore expose persisted language instead of inferring it from a filename,
while also rejecting a language that disagrees with the exact case-sensitive
repository extension.

## Migration and validation

Migration 4 runs in one immediate transaction after validating the exact
historical ledger. It drops and recreates only the affected immutability
triggers and `artifact_facts` table, copies facts in deterministic
artifact/ordinal order, and leaves prior migration text and checksums
unchanged.

Production tests cover:

- fresh version-4 creation and exact migration-ledger identity;
- upgrades from versions 1, 2, and 3;
- preservation of non-empty version-3 artifacts and facts;
- the default `rust` language for historical artifacts;
- exact admission of `rust`, `go`, `typescript`, and `tsx` plus rejection of
  every other language;
- fail-closed search and exact retrieval when persisted language disagrees with
  the repository extension;
- admission of the new fact kinds and rejection of unknown kinds;
- language-specific artifact reuse with no cross-language reuse for identical
  source bytes; and
- per-occurrence search and exact-retrieval producer attribution from the
  authoritative persisted artifact rather than the combined snapshot; and
- inclusive search-output budgeting that counts persisted language and the
  per-occurrence producer digest; and
- mixed four-language publication, search, exact retrieval, and repeat indexing
  in one active generation.
