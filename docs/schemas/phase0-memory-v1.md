# Phase 0 engineering-memory record version 1

- Status: Implemented; accepted
- Date: 2026-07-27
- Governing decision:
  [ADR-0014](../adr/0014-phase0-engineering-memory-record.md)
- Independent regression oracle:
  [`phase0_memory_record_v1_spike.rs`](../../crates/repowitness-local/tests/phase0_memory_record_v1_spike.rs)
- Production domain:
  [`memory.rs`](../../crates/repowitness-domain/src/memory.rs)
- Production parser and writer:
  [`memory_format.rs`](../../crates/repowitness-local/src/memory_format.rs)

This document fixes the accepted version-1 YAML, validated semantic object,
canonical JSON, digest frame, and generated-YAML profile governed by ADR-0014.
The pure domain values and hostile byte parser, canonicalizer, and writer are
production code. Capability-contained worktree admission, trusted import, and
the append-only SQLite journal are implemented as separate boundaries. The
derived current-memory projection is implemented in forward-only SQLite schema
version 6 without changing this record. Schema version 8 and the bounded
Git-tree history importer add reviewed correspondence and observation history
without changing the canonical record.

## File boundary

One current record is stored at:

```text
.code-memory/records/<record-id>.yaml
```

The file name is ASCII and its stem equals the decoded record's `record_id`
byte for byte. The local adapter admits only one directly contained regular
file opened without following links. It rejects nested paths, alternate case,
symlinks in any `.code-memory/` component, hard-link aliases, and special
files. A future Git-tree importer must apply equivalent mode and path checks to
blob entries. Absence is diagnostic, never a tombstone.

## Top-level semantic object

Every key is required and unknown keys fail. The generated-YAML order is the
order in this table. All fields except `display_revision` participate in the
canonical semantic digest.

| Key | Type and bound | Semantic rule |
|---|---|---|
| `schema_version` | integer literal `1` | Selects this schema |
| `record_id` | canonical record ID | Immutable logical identity |
| `display_revision` | nonzero `u32` | Presentation only; excluded from the digest |
| `parent_revision_digests` | 0–8 SHA-256 texts | Sorted by decoded bytes; unique |
| `kind` | `decision` or `failure` | Claim kind |
| `title` | 1–256 UTF-8 bytes | No NUL or Unicode line-break character |
| `body` | 1–16 KiB UTF-8 bytes | No NUL or carriage return; LF is allowed |
| `scope` | scope object | Repository and subject evidence |
| `provenance` | provenance object | Repository-authored origin claim |
| `assurance` | `locally_approved` | Authored claim; cannot approve itself |
| `lifecycle` | lifecycle enum | Authored lifecycle; effective state may only reduce it |
| `validity` | tagged validity object | Commit-DAG or exact-worktree applicability |
| `evidence` | 1–16 evidence objects | Order is semantic because scope indexes it |
| `relationships` | 0–16 relationship objects | Sorted by kind, record ID, then digest; unique |
| `tombstone` | strict Boolean | Must agree with lifecycle and parent rules |

Lifecycle values are `active`, `needs_review`, `stale`, `contradicted`,
`superseded`, `quarantined`, and `tombstoned`.

## Record ID

The ID is `mem_` followed by 26 characters from:

```text
0123456789ABCDEFGHJKMNPQRSTVWXYZ
```

Treat the source value as 16 opaque bytes in big-endian bit order. Prepend two
zero bits, split the 130 bits into five-bit groups from most significant to
least significant, and map each group through the alphabet. The first
character is therefore `0` through `7`. Decoding rejects aliases, lowercase,
the omitted Crockford letters `I`, `L`, `O`, and `U`, and every noncanonical
length or prefix.

The writer fills all 128 source bits from an operating-system cryptographic
random source. Time, paths, repository data, and record content are not inputs.

Golden vectors:

| 128-bit input | Record ID |
|---|---|
| `00000000000000000000000000000000` | `mem_00000000000000000000000000` |
| `ffffffffffffffffffffffffffffffff` | `mem_7ZZZZZZZZZZZZZZZZZZZZZZZZZ` |
| `000102030405060708090a0b0c0d0e0f` | `mem_00041061050R3GG28A1C60T3GF` |

## Nested objects

### Scope

Generated key order is `repository_id`, `subject_evidence`.

- `repository_id` is exactly the ADR-0013 `rwi1:h:` tag followed by 64
  uppercase Base16 characters.
- `subject_evidence` is an RFC 8785 interoperable nonnegative integer and
  selects an existing zero-based evidence entry. The Phase 0 writer emits `0`.

### Provenance

Generated key order is `origin`, `actor_kind`, `actor_id`.

- `origin` is `human`.
- `actor_kind` is `local_asserted`.
- `actor_id` is 1–128 printable ASCII bytes.

These are authored labels. The trusted approval actor is separate append-only
audit metadata.

### Validity

`validity` is one of two exact tagged objects:

```yaml
kind: commits
introduced_by:
  - object_format: sha1
    object_id: "1111111111111111111111111111111111111111"
invalidated_by: []
```

```yaml
kind: worktree
source_snapshot_digest: "8888888888888888888888888888888888888888888888888888888888888888"
```

The commits variant has one through sixteen unique introduction commits and
zero through sixteen unique invalidation commits. The two sets are disjoint.
Each commit object contains exactly `object_format` and `object_id`.
`object_format` is `sha1` or `sha256`; its object ID is respectively 40 or 64
lowercase Base16 characters. Each list is sorted first by object format
(`sha1` before `sha256`) and then by decoded object bytes.

The worktree variant contains only `kind` and one exact source-snapshot digest.
It never implies descendant-commit validity.

### Rust symbol evidence

Generated key order is:

```text
kind
source_snapshot_digest
path
content_digest
artifact_digest
fact_ordinal
symbol_kind
name
qualified_name
name_start
name_length
declaration_start
declaration_length
declaration_digest
producer_id
producer_version
```

`kind` is `rust_symbol`. `symbol_kind` is one of `function`, `method`,
`struct`, `enum`, `union`, `trait`, `module`, `type_alias`, `constant`,
`static`, or `macro`.

The snapshot, content, artifact, and declaration digests are canonical
lowercase SHA-256 texts. `path` is the validated ADR-0011 `rwp1:h:` uppercase
Base16 encoding. `fact_ordinal`, offsets, and lengths are nonnegative integers
no greater than 9,007,199,254,740,991. Lengths are nonzero, checked addition
cannot overflow, both span ends are at or before 8 MiB, and the name span is
contained by the declaration span. `name_length` equals the UTF-8 byte length
of `name`.

`name` is 1–256 UTF-8 bytes and `qualified_name` is 1–1,024 UTF-8 bytes.
Neither contains NUL, CR, or LF. `producer_id` and `producer_version` are each
1–128 printable ASCII bytes.

When source and generation data are available, import verifies the exact name
bytes, declaration digest, repository, snapshot, path, content, artifact,
ordinal, and producer. Unavailable data produces explicit coverage rather than
activation.

### Relationships

Each relationship contains exactly:

```yaml
kind: contradicts
record_id: mem_00000000000000000000000000
revision_digest: "9999999999999999999999999999999999999999999999999999999999999999"
```

`kind` is `contradicts` or `supersedes`. The record ID and revision digest use
their canonical encodings. Relationship targets are same-repository in Phase
0. Missing target history remains explicit unresolved coverage.

### Tombstones

`tombstone: true` requires `lifecycle: tombstoned` and at least one parent.
Every other lifecycle requires `tombstone: false`. A tombstone remains a
complete semantic version but is ineligible for active retrieval. It preserves
rather than redacts prior Git and SQLite history.

## SHA-256 text and integer profile

Parent, relationship-revision, source-snapshot, content, artifact,
declaration, and worktree-snapshot digests are exactly 64 lowercase characters
from `0-9a-f`. Repository identity and path are the only uppercase,
separately-tagged Base16 values.

Version 1 contains no floating-point value. Every integer entering canonical
JSON is nonnegative and no greater than 9,007,199,254,740,991, except
`display_revision`, which is a nonzero `u32` and is excluded from canonical
JSON.

## Strict YAML admission

Admission runs in this order and fails closed:

1. reject inputs over 64 KiB, malformed UTF-8, or any carriage return;
2. scan raw tokens and reject YAML version, tag, and reserved directives,
   explicit tags, anchors, and aliases;
3. stream raw events with limits of 4,096 events, 2,048 data nodes, depth 8,
   and exactly one document;
4. decode with at most 48 KiB aggregate data-scalar bytes and 4 KiB aggregate
   comment bytes, zero aliases/anchors/merge keys/includes, strict booleans,
   no schema coercion, duplicate-key errors, and unknown-field errors; and
5. validate every scalar, collection, tagged union, ordering, cross-field
   invariant, and canonical output bound before constructing domain values.

Composite keys, merge keys, floats, implicit scalar-to-string conversions, and
multiple documents are invalid. Parser and validation diagnostics name only a
field category or bound and never include YAML snippets, titles, bodies,
actors, symbols, paths, or digest bytes.

Filesystem/Git admission, parsing, validation, canonicalization, import,
history traversal, and projection each retain independent deadlines,
cancellation, and resource limits.

## Canonical JSON and digest

Build a serialization-only object with exactly the semantic shape above and
without `display_revision`. Sort parents, commit sets, and relationships as
specified. Preserve evidence order. Serialize with RFC 8785 JCS and reject
output over 256 KiB.

The revision digest is SHA-256 over:

```text
UTF8("RepoWitness\0memory-record\0")
|| U32_BE(1)
|| U64_BE(canonical_json_byte_length)
|| canonical_json
```

YAML comments, mapping order, scalar style, parent input order, and
`display_revision` do not affect this digest. No other field is excluded.

## Generated-YAML presentation profile

The Phase 0 writer consumes validated values only and emits:

- UTF-8 without BOM, LF line endings, and one final LF;
- two-space mapping indentation and two additional spaces for sequence item
  contents;
- the top-level and nested key orders defined above;
- `[]` for empty sequences and block style for nonempty sequences;
- lowercase plain scalars for enums and strict Booleans;
- decimal integers without signs or leading zeros;
- plain canonical record, repository, and path IDs;
- double-quoted lowercase digest and Git object-ID strings; and
- JSON-compatible double-quoted free text. Escape quote, reverse solidus,
  backspace, form feed, LF, CR, and tab with their JSON short escapes; escape
  U+0000–U+001F and U+007F–U+009F otherwise as lowercase `\u00xx`; escape
  U+2028 and U+2029 as lowercase `\u2028` and `\u2029`; and emit every other
  Unicode scalar value directly.

The writer rejects output over 64 KiB. This presentation profile is
golden-tested but is not part of the semantic digest.

## Effective local state

Repository-authored provenance, assurance, and lifecycle cannot authorize
themselves. An exact version becomes an active candidate only after a trusted
local audit event binds repository ID, record ID, canonical digest, approval
operation, and configured local actor, and after policy, version-DAG,
Git-ancestry, and evidence checks succeed. A semantic edit creates a new digest
and requires new approval.

ADR-0005 system-recorded timestamps, audit actor, audit origin, and operation
are append-only observation metadata outside the YAML and canonical record.
Missing history, source, evidence, or correspondence reduces effective state
to an explicit non-active outcome.

## Golden vectors

The test-only harness verifies these byte-exact files:

- commit-scoped generated
  [YAML](../../crates/repowitness-local/tests/fixtures/memory-v1/commit.yaml),
  [canonical JSON](../../crates/repowitness-local/tests/fixtures/memory-v1/commit.canonical.json),
  and [digest](../../crates/repowitness-local/tests/fixtures/memory-v1/commit.digest);
- worktree/relationship generated
  [YAML](../../crates/repowitness-local/tests/fixtures/memory-v1/worktree-relationship.yaml),
  [canonical JSON](../../crates/repowitness-local/tests/fixtures/memory-v1/worktree-relationship.canonical.json),
  and
  [digest](../../crates/repowitness-local/tests/fixtures/memory-v1/worktree-relationship.digest).

The commit vector digest is
`f58daa9524b67d2d488b272024294eb1050fea5551891b153940266a9e7aacf6`.
The worktree/relationship vector digest is
`07a1b1a1aa99df6a6d2fa032a8a123df775a16b2610479b9feac3f804ddae7df`.
The exact generated-YAML byte vectors have ordinary file SHA-256 checksums
`916d2366754e37a20ac49416172a88815d5bd47aa5477c5eaac41062e7c90c1f`
and
`762a1220300cc182a129c20864dd15c3bdbc4a59b997ecb3c963f970a7b8e083`
respectively; those checksums are fixture-integrity checks, not record
identities.

The harness also proves record-ID vectors, presentation/display-revision
invariance, nested unknown-field rejection, directive/tag/anchor/alias/merge
rejection, strict cross-field invariants, and redacted diagnostics.

## Acceptance and compatibility

ADR acceptance fixed this schema after the vectors, parser dependency evidence,
hostile-input suite, fuzz target, and resource gates passed. An incompatible
semantic change requires a new schema version and superseding ADR. Version 1
must never be silently reinterpreted or edited in place.
