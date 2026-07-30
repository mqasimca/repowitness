# Phase 0 SQLite schema version 6

> Historical pre-baseline development schema. The current runtime does not
> accept this format. It is retained only as design provenance and was
> superseded by [ADR-0022](../adr/0022-squash-pre-release-sqlite-schema.md) and
> the [current schema](phase0-sqlite-current-v2.md).

- Status: Implemented; governing decision proposed
- Date: 2026-07-27
- Governing decision:
  [ADR-0018](../adr/0018-phase0-memory-revalidation.md)
- Previous version: [Phase 0 SQLite schema version 5](phase0-sqlite-v5.md)
- Implementation:
  [`crates/repowitness-local/src/sqlite/`](../../crates/repowitness-local/src/sqlite/)

Version 6 preserves every version-5 source-generation and memory-journal
contract. It adds immutable derived occurrence fingerprints, trusted
correspondence review, and double-buffered current-memory projections. It does
not change canonical memory records or append derived state to
`memory_versions`.

## Global bounds

- at most 4,096 projected records per workspace rebuild;
- at most 16 evidence results per projected memory version;
- at most 16 review candidates per evidence result;
- one source/index generation and one correspondence profile per projection;
- one owned writer transaction at a time; and
- the existing request deadline and cooperative cancellation token checked
  before every bounded loop and commit.

Exceeding a bound fails the rebuild without switching the active pointer.

## `artifact_fact_correspondence`

One immutable companion row may exist for each `artifact_facts` row:

| Column | Storage and check |
|---|---|
| `artifact_digest` | 32-byte foreign key to `analysis_artifacts` |
| `fact_ordinal` | nonnegative integer; composite foreign key to the fact |
| `profile_id` | printable ASCII, 1–128 bytes |
| `profile_version` | positive integer no greater than `4294967295` |
| `declaration_digest` | 32-byte standard SHA-256 digest |
| `name_elided_digest` | 32-byte domain-separated SHA-256 digest |

The primary key is `(artifact_digest, fact_ordinal, profile_id,
profile_version)`. Rows may be inserted only while their artifact is staging
and can never be updated or deleted after artifact completion. Existing
version-5 artifacts have explicit missing-fingerprint coverage until a
semantics-key change causes reanalysis.

## `memory_correspondence_audit`

This rowid-backed append-only journal records trusted review:

| Column | Storage and check |
|---|---|
| `event_id` | positive integer primary key |
| memory version key | workspace, 16-byte record ID, 32-byte revision digest |
| `evidence_ordinal` | integer `0`–`15`, foreign key to exact memory evidence |
| `operation` | `approved`, `rejected`, or `manual_link` |
| source occurrence | exact cited snapshot, path, artifact, and fact ordinal |
| target occurrence | exact target snapshot, path, artifact, and fact ordinal |
| `method_id` | printable ASCII, 1–128 bytes |
| `method_version` | positive integer no greater than `4294967295` |
| trusted actor | `local_asserted` plus 1–128 printable ASCII bytes |
| `recorded_at_unix_ms` | nonnegative integer |

The writer verifies both exact occurrence identities before insert. Events are
never updated or deleted. Later review does not rewrite historical projection
generations.

## `memory_projection_generations`

One row describes a staged or complete derivation:

| Column | Storage and check |
|---|---|
| `projection_id` | positive integer primary key |
| `workspace_id` | foreign key to `workspaces` |
| `index_generation_id` | foreign key to the exact index generation |
| `source_epoch` | nonnegative source epoch copied from that generation |
| `snapshot_digest` | exact 32-byte active source snapshot |
| `target_kind` | `git` or `worktree` |
| `target_format` | `sha1`, `sha256`, or `source_snapshot` consistent with kind |
| `target_revision` | 20- or 32-byte identity consistent with format |
| optional HEAD | object format plus 20- or 32-byte commit for worktree targets |
| correspondence profile | printable-ASCII ID/version plus 32-byte digest |
| `lifecycle_state` | `staging` or `complete` |
| coverage counts | searched, skipped, unresolved, and truncated, all nonnegative |
| result counts | total plus one nonnegative count for every effective state |

The source generation, epoch, and snapshot must agree. Semantic columns are
immutable. The only allowed row transition is `staging` to `complete`, after
trigger validation of child density and counts.

## `memory_projection_records`

The primary key is `(projection_id, ordinal)`. Record IDs are also unique within
one projection.

Each row stores the exact memory-version key and:

- `effective_state`: `current`, `not_applicable`, `stale`, `needs_review`,
  `indeterminate`, `conflicted`, `contradicted`, `superseded`, `quarantined`,
  or `tombstoned`;
- `validity_state`: `valid`, `invalid`, or `indeterminate`;
- `evidence_state`: `exact`, `corresponded`, `changed`, `ambiguous`, `missing`,
  `indeterminate`, `conflicted`, or `not_evaluated`;
- a stable categorical reason from ADR-0018;
- evidence, resolved, review, and indeterminate counts;
- parent-head and missing-parent counts; and
- a Boolean indicating whether a trusted local approval exists.

Rows are immutable, dense by ordinal, and accepted only while the parent
projection is staging.

## `memory_projection_evidence`

The primary key is `(projection_id, record_ordinal, evidence_ordinal)` and
references one projected record plus its exact immutable `memory_evidence` row.

Each row stores:

- outcome `exact`, `same_path_rename`, `git_exact_move`, `changed`,
  `ambiguous`, `missing`, or `indeterminate`;
- method ID/version and categorical assurance `automatic`, `reviewed`, or
  `none`;
- optional exact target path, artifact, fact ordinal, declaration digest, and
  name-elided digest; and
- complete/partial candidate coverage plus the pre-limit candidate count.

Resolved outcomes require every target field. Missing and indeterminate
outcomes require target fields to be NULL. Ambiguous outcomes use child
candidate rows and cannot claim automatic assurance.

## `memory_projection_candidates`

The primary key extends one evidence result with a dense ordinal `0`–`15`.
Each immutable candidate stores the exact target occurrence, proposed relation
(`same`, `moved`, `renamed`, `moved_renamed`, `split`, or `merged`), attributed
method, categorical assurance `review_required`, and deterministic ordering
inputs. The table never stores raw source or a floating-point score.

## `active_memory_projections`

There is at most one row per workspace:

| Column | Storage and check |
|---|---|
| `workspace_id` | primary key and foreign key to `workspaces` |
| `projection_id` | unique foreign key to a complete projection |

The owned writer verifies that the target index generation is still active,
the source epoch and snapshot still match, and every projection count and
foreign key is valid before one pointer update. Readers first pin this
projection ID and never combine rows from another projection.

## Immutability and cleanup

Triggers reject:

- update/delete of completed projection semantics or any projection child;
- child insertion after projection completion;
- update/delete of occurrence-fingerprint or correspondence-audit rows;
- activation of a staging, cross-workspace, stale-epoch, or non-active-index
  projection; and
- mutation of an active projection's children.

Cancelled and failed staging projections may be deleted in bounded batches.
Completed inactive projections are retained until an explicit bounded retention
policy exists. Online backup includes the active pointer, completed
projections, review audit, and all version-5 journal rows.

## Migration and validation

Migration 6 runs in one immediate transaction after exact validation of
migrations 1 through 5. It creates only new tables, indexes, and triggers; no
historical table or row is rewritten.

The implementation fixes a stable migration name/checksum and tests fresh
creation, upgrades from versions 1 through 5, exact schema introspection,
idempotent reopen, immutable companion rows, bounded atomic projection
publication, stale-source rejection, recovery, and online backup. The complete
ADR-0018 adversarial correspondence and design-partner evaluation matrix now
supports the accepted Phase 0 decision.
