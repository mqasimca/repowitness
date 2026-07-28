# Phase 0 SQLite schema version 5

> Historical pre-baseline development schema. The current runtime does not
> accept this format. It is retained only as design provenance and was
> superseded by [ADR-0022](../adr/0022-squash-pre-release-sqlite-schema.md) and
> the [current baseline](phase0-sqlite-baseline-v1.md).

- Status: Implemented against proposed ADR-0017
- Date: 2026-07-27
- Governing decision:
  [ADR-0017](../adr/0017-phase0-memory-journal.md)
- Previous version: [Phase 0 SQLite schema version 4](phase0-sqlite-v4.md)
- Implementation:
  [`crates/repowitness-local/src/sqlite/`](../../crates/repowitness-local/src/sqlite/)

Version 5 preserves every version-4 indexing, generation, ownership, FTS5,
checkpoint, recovery, and backup contract. It adds an immutable memory-version
DAG and append-only system-recorded journal. It does not add a current-memory
projection.

## `memory_versions`

The primary key is `(workspace_id, record_id, revision_digest)`.

| Column | Storage and check |
|---|---|
| `workspace_id` | positive foreign key to `workspaces` |
| `record_id` | 16-byte BLOB |
| `revision_digest` | 32-byte canonical-memory digest BLOB |
| `schema_version` | integer exactly `1` |
| `canonical_json` | BLOB, 1–262,144 bytes |
| `kind` | `decision` or `failure` |
| `title` | TEXT, 1–256 UTF-8 bytes |
| `body` | TEXT, 1–16,384 UTF-8 bytes |
| `subject_evidence` | integer `0`–`9,007,199,254,740,991` |
| `provenance_origin` | `human` |
| `authored_actor_kind` | `local_asserted` |
| `authored_actor_id` | TEXT, 1–128 printable ASCII bytes |
| `authored_assurance` | `locally_approved` |
| `authored_lifecycle` | accepted ADR-0014 lifecycle text |
| `validity_kind` | `commits` or `worktree` |
| `validity_source_snapshot` | 32-byte BLOB only for `worktree`; otherwise NULL |
| `tombstone` | strict integer Boolean |

The workspace repository identity must equal the validated record scope before
insert. Canonical JSON is the accepted semantic identity material and excludes
`display_revision`.

## Ordered child tables

Every child primary key starts with the complete `memory_versions` key and an
integer ordinal.

The writer inserts all child rows first under deferred foreign keys and
publishes the parent `memory_versions` row last. A parent-insert trigger rejects
publication unless ordinals are dense, the record has 1–16 evidence rows,
`subject_evidence` resolves, validity children match the declared validity
kind, and tombstones have a parent. Separate triggers reject child inserts after
publication, so a visible version cannot gain children later.

### `memory_version_parents`

- ordinal `0`–`7`;
- 32-byte parent digest;
- unique parent digest per version.

### `memory_validity_commits`

- side `introduced_by` or `invalidated_by`;
- ordinal `0`–`15` within each side;
- object format `sha1` or `sha256`;
- 20- or 32-byte object ID matching its format;
- unique `(side, object_format, object_id)` per version.

Commit-valid versions have at least one `introduced_by` row. Worktree-valid
versions have none.

### `memory_evidence`

Version 1 admits only `rust_symbol` evidence. Each row stores:

- 32-byte source snapshot, content, artifact, and declaration digests;
- the exact repository path as a 1–32,764-byte BLOB;
- fact ordinal no greater than `9,007,199,254,740,991`;
- the accepted Rust symbol kind;
- name and qualified name with their accepted byte bounds;
- nonempty name and declaration spans ending at or before 8 MiB, with the name
  span contained by the declaration span and its length equal to the UTF-8
  name byte length; and
- printable-ASCII producer ID and version with 1–128-byte bounds.

There are 1–16 rows, and `subject_evidence` selects one existing ordinal.

### `memory_relationships`

- ordinal `0`–`15`;
- kind `contradicts` or `supersedes`;
- 16-byte target record ID;
- 32-byte target revision digest;
- unique target tuple per source version.

## `memory_audit`

`memory_audit` is rowid-backed with a positive integer `event_id`. Every row
references one exact memory version and stores:

- operation `observed` or `locally_approved`;
- trusted actor kind `local_asserted` and a 1–128-byte printable-ASCII actor ID;
- nonnegative `recorded_at_unix_ms`;
- source kind `git` or `worktree`;
- source format `sha1`, `sha256`, or `source_snapshot`, consistent with source
  kind;
- a 20- or 32-byte source revision matching the source format;
- nonzero display revision;
- a 32-byte SHA-256 digest of the exact admitted YAML presentation.

A unique observation index covers the exact version, source, presentation, and
actor. A partial unique approval index permits at most one local approval per
exact version and trusted actor. All events are append-only.

## Immutability

Triggers reject every update or delete on:

- `memory_versions`;
- all four child tables; and
- `memory_audit`.

Publication triggers, deferred foreign keys, and writer verification reject
partial, orphan, or cross-workspace rows. The writer also re-reads and compares
canonical bytes plus every normalized child row when an existing revision is
re-imported. Raw YAML, comments, source snippets, and host paths are never
persisted.

## Migration and validation

Migration 5 runs in one immediate transaction after exact validation of
migrations 1 through 4. It creates new tables, indexes, and triggers only. No
version-4 table is rebuilt and no historical row changes.

Focused automated validation covers the ADR-0017 matrix, the stable migration
name/checksum, upgrades from versions 1 through 4, fresh/reopen behavior,
idempotent observations and approvals, semantic revisions, trigger-enforced
rollback and immutability, canonical/normalized corruption detection, and
online backup. The repository-wide dependency and recovery suites remain part
of every release gate.
