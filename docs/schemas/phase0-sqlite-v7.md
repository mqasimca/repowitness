# Phase 0 SQLite schema version 7

> Historical pre-baseline development schema. The current runtime does not
> accept this format. It is retained only as design provenance and was
> superseded by [ADR-0022](../adr/0022-squash-pre-release-sqlite-schema.md) and
> the [current schema](phase0-sqlite-current-v2.md).

- Status: Implemented
- Date: 2026-07-27
- Governing decision:
  [ADR-0020](../adr/0020-phase0-python-indexing.md) (accepted)
- Previous version: [Phase 0 SQLite schema version 6](phase0-sqlite-v6.md)
- Implementation:
  [`crates/repowitness-local/src/sqlite/`](../../crates/repowitness-local/src/sqlite/)

Version 7 preserves every version-6 source-generation, occurrence-
correspondence, memory-journal, and current-memory-projection contract. Its
only data-model change is exact admission of `python` as a persisted analysis-
artifact language.

## `analysis_artifacts.language`

The closed language constraint is:

```sql
CHECK (language IN ('rust', 'go', 'typescript', 'tsx', 'python'))
```

The column remains non-null with the historical `'rust'` default needed by
older migrations. Language remains part of immutable artifact semantics:
completed rows cannot be relabeled, and unsupported or differently cased
values fail closed. Existing fact kinds already represent the admitted Python
classes, functions, methods, type aliases, and module variables.

## Forward migration

SQLite cannot alter an existing table-level `CHECK` constraint in place.
Migration 7 therefore runs one immediate transaction that:

1. copies every version-6 artifact row into a transaction-local temporary
   table;
2. creates an empty replacement with the exact version-6 columns and checks
   plus `python`;
3. defers foreign-key enforcement, drops the five artifact-owned triggers,
   replaces the parent table, and inserts the copied rows only after the new
   parent has its final name;
4. recreates semantic immutability, lifecycle, payload-set-once, complete-row
   deletion, and Rust-correspondence-completion triggers; and
5. drops the temporary copy before commit.

The replacement timing is intentional. Inserting parent rows after the old
parent is dropped balances SQLite's deferred foreign-key counters for
`artifact_facts` and `generation_files`. `legacy_alter_table` is enabled only
around the temporary missing-parent rename so dependent trigger SQL can remain
bound to the stable `analysis_artifacts` name, then is reset before row copy
and trigger recreation. Commit fails if any dependent reference is unresolved.

Migrations 1 through 6 and their checksums are unchanged.

## Validation

Automated tests cover:

- fresh version-7 creation and exact seven-row migration ledger;
- upgrades from every historical version;
- a populated version-6 upgrade retaining artifact facts, Rust occurrence
  correspondence, and generation-file references;
- zero rows from `PRAGMA foreign_key_check` after migration;
- exact Python admission plus rejection of unknown language values;
- artifact-language immutability and Rust-only correspondence completion; and
- idempotent reopen, recovery, publication, projection, and backup behavior
  through the existing schema suite.

An opt-in production-shaped external-worktree probe persisted Python artifacts
under version 7, passed `PRAGMA integrity_check` and
`PRAGMA foreign_key_check`, and reused all selected files on an unchanged
second generation. Its repository identity and measurements remain local.
