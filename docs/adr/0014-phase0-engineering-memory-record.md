# ADR-0014: Define a strict Phase 0 engineering-memory record

- Status: Proposed
- Date: 2026-07-26
- Owners: Project maintainers
- Scope: Phase 0 team-memory YAML, canonical identity, symbol attachment, and parser stack

## Context

[ADR-0005](0005-git-dag-temporal-memory.md) and
[ADR-0007](0007-git-memory-synchronization.md) require immutable,
conflict-preserving, bitemporal engineering memory, but intentionally leave the
first complete record schema and maintained strict-YAML stack unresolved.
Production import, SQLite projection, correspondence, and context compilation
must not stabilize around the incomplete DTO from the
[strict YAML spike](../research/strict-memory-yaml-spike-2026-07-25.md).

Phase 0 needs only enough shared memory to attach one manually approved
`decision` or `failure` to exact Rust evidence, preserve concurrent versions,
and make later revalidation explicit. It does not need the broader Phase 3
record kinds, personal memory, authenticated remote actors, arbitrary evidence
URIs, automatic memory extraction, or a generic extension map.

The boundary is hostile-input facing. YAML features can hide or duplicate
meaning, presentation bytes are not semantic identity, and a record can contain
source text, personal data, or secrets. Missing or ambiguous information must
not become active knowledge.

## Decision

Adopt the following Phase 0 version-1 semantic record after this ADR is
accepted. Until then, the existing implementation remains a test-only spike.

### File and record identity

- Store one current record at
  `.code-memory/records/<record-id>.yaml`.
- `record_id` is `mem_` followed by exactly 26 uppercase Crockford Base32
  characters. The first encoded character is `0` through `7`. The ID is an
  opaque 128-bit logical identity; consumers do not infer time or ordering from
  it.
- The filename stem must equal `record_id` byte for byte. Directory traversal,
  nested paths, alternate case, symlinks, hard-link aliases, and special files
  fail closed.
- `schema_version` is the integer `1`.
- `display_revision` is a nonzero `u32`. It is presentation metadata, not a
  concurrency or content-identity primitive.
- `parent_revision_digests` contains zero through eight distinct lowercase
  SHA-256 hex strings. Parents are sorted by decoded digest bytes before
  canonicalization. An ordinary edit has exactly one parent; a reviewed merge
  may have several.

ID generation uses injected entropy and time sources in tests. Import never
generates or rewrites an ID.

### Complete semantic shape

Every key below is required. Version 1 has no extension map and rejects unknown
fields.

```yaml
schema_version: 1
record_id: mem_01J00000000000000000000000
display_revision: 1
parent_revision_digests: []
kind: decision
title: Keep generation publication atomic
body: Readers must never observe a partially staged generation.
scope:
  repository_id: rwi1:h:0000000000000000000000000000000000000000000000000000000000000001
  subject_evidence: 0
provenance:
  origin: human
  actor_kind: local_asserted
  actor_id: maintainer
assurance: locally_approved
lifecycle: active
validity:
  kind: commits
  introduced_by:
    - object_format: sha1
      object_id: 0000000000000000000000000000000000000000
  invalidated_by: []
evidence:
  - kind: rust_symbol
    source_snapshot_digest: 0000000000000000000000000000000000000000000000000000000000000000
    path: rwp1:h:7372632F6C69622E7273
    content_digest: 0000000000000000000000000000000000000000000000000000000000000000
    artifact_digest: 0000000000000000000000000000000000000000000000000000000000000000
    fact_ordinal: 0
    symbol_kind: function
    name: publish
    qualified_name: crate::publish
    name_start: 3
    name_length: 7
    declaration_start: 0
    declaration_length: 20
    declaration_digest: 0000000000000000000000000000000000000000000000000000000000000000
    producer_id: repowitness.rust.syntax
    producer_version: phase0-rust-syntax-v1
relationships: []
tombstone: false
```

Phase 0 supports only:

- `kind`: `decision` or `failure`;
- `provenance.origin`: `human`;
- `provenance.actor_kind`: `local_asserted`;
- `assurance`: `locally_approved`;
- `lifecycle`: `active`, `needs_review`, `stale`, `contradicted`,
  `superseded`, `quarantined`, or `tombstoned`;
- evidence kind `rust_symbol`;
- relationship kinds `contradicts` and `supersedes`.

The importer treats `actor_id` as a locally asserted label, never as an
authenticated organization principal. It cannot upgrade assurance based only
on file contents.

`scope.repository_id` uses the canonical ADR-0013 `rwi1:h:` encoding.
`scope.subject_evidence` must select an existing evidence entry and is `0` in
the Phase 0 writer. A record has one through sixteen evidence entries.

Rust symbol evidence preserves the exact observed source snapshot, canonical
ADR-0011 repository path, content and analysis-artifact digests, deterministic
fact ordinal, symbol kind and names, byte spans, SHA-256 digest of the exact
declaration bytes, and producer identity. Import reconstructs validated domain
types and checks that:

- the name and declaration spans are within the declared Phase 0 source limit;
- the name span is contained by the declaration span;
- `name` equals the bytes at the name span when the source is available;
- `declaration_digest` equals the exact declaration bytes when the source is
  available;
- the repository, snapshot, path, content, artifact, ordinal, and producer
  agree with the cited indexed occurrence when that generation is available.

Unavailable source or history produces explicit unresolved or indeterminate
coverage. It does not make the record active by assumption.

`validity` is a tagged union:

- `kind: commits` has one through sixteen distinct introduction commit IDs and
  zero through sixteen distinct invalidation commit IDs;
- `kind: worktree` has exactly one canonical source-snapshot SHA-256 digest.

A commit ID stores `object_format` as `sha1` or `sha256` and a lowercase object
ID of exactly 40 or 64 hex characters respectively. Commit lists are sorted by
object format and decoded object bytes. A dirty-worktree record cannot also
claim descendant commit semantics; a later reviewed version rebinds it to
commit validity.

Each relationship contains `kind`, `record_id`, and `revision_digest`.
Relationships are sorted by that tuple and are unique. A tombstone requires
`lifecycle: tombstoned`, `tombstone: true`, and at least one parent digest.
Every other lifecycle requires `tombstone: false`. Missing files never create a
tombstone.

### Bounds and text rules

- Input is UTF-8, LF-only, one YAML document, and at most 64 KiB.
- Raw preflight permits at most 4,096 parser events, 2,048 data nodes, nesting
  depth 8, and one document.
- Reject directives, every explicit tag, anchors, aliases, merge keys,
  duplicate keys, composite keys, floats, non-strict booleans, unknown fields,
  and implicit scalar-to-string conversions.
- `title` is 1 through 256 UTF-8 bytes, contains no NUL or line break, and is
  not trimmed or Unicode-normalized.
- `body` is 1 through 16 KiB of UTF-8 and contains no NUL or carriage return.
- `actor_id`, `producer_id`, and `producer_version` are each 1 through 128
  printable ASCII bytes.
- `name` is 1 through 256 bytes and `qualified_name` is 1 through 1,024 bytes.
- Byte offsets and lengths are nonnegative integers within the Phase 0 source
  limit. Every integer field is within the RFC 8785 interoperable integer
  range; version 1 contains no float.
- Canonical and decoded repository path limits are the existing ADR-0011
  limits. Arrays and canonical output have independent aggregate bounds.
- Parser, validation, canonicalization, import, Git traversal, and projection
  each have explicit deadlines and cancellation.

Diagnostics identify only the failing field category and bound. They do not
include title, body, actor, source names, repository paths, YAML snippets, or
secret-like values.

### Canonical semantic digest

Construct a serialization-only DTO from validated values. Exclude only
`display_revision`; all other fields above are semantic. Sort every set-like
array as specified, preserve evidence order because
`scope.subject_evidence` indexes it, and serialize with the RFC 8785 JSON
Canonicalization Scheme.

Hash the canonical UTF-8 bytes with SHA-256 using a versioned,
length-prefixed frame under:

```text
RepoWitness\0memory-record\0
```

The domain result is `CanonicalMemoryDigest`. YAML bytes, comments, key order,
scalar style, and `display_revision` do not participate. A changed semantic
field must change the digest.

### Parser and canonicalizer

Promote the exact reviewed stable spike versions to production only after this
ADR is accepted:

- `serde-saphyr` 0.0.29 with only `deserialize`;
- `granit-parser` 0.0.7 with default features disabled;
- `serde_json_canonicalizer` 0.3.2.

Keep the independent raw-event preflight. Typed deserialization alone is not a
security boundary because the spike proved that an explicit custom tag could
be consumed before DTO validation.

As of 2026-07-26, the YAML stack has a 1.0.0 release candidate but no stable
1.0 release. Do not adopt a release candidate for the durable format merely to
obtain a larger version number. Re-evaluate the stable 1.0 line with the full
hostile-input, golden, fuzz, MSRV, dependency, and resource suite before
upgrading.

## Alternatives considered

### Keep the spike DTO

It is small and tested, but omits scope, provenance, assurance, temporal
validity, exact source evidence, relationships, and lifecycle states required
by accepted ADRs.

### Store free-form Markdown or YAML

This is pleasant to author but cannot provide strict identity, bounded parsing,
typed scope, or reliable conflict and validity behavior.

### Hash the YAML bytes

This avoids canonical JSON but makes comments, key order, quoting, and
formatting semantic, creating unstable identities and noisy conflicts.

### Adopt the 1.0 release candidates immediately

They document useful hardening and migration work, but a durable boundary
should not depend on pre-release APIs without a measured need. The exact stable
versions already pass the focused spike.

### Use JSON as the only human-facing file

JSON narrows syntax but is less review-friendly for multiline engineering
records. Strict YAML plus canonical JSON keeps human presentation separate from
semantic identity.

### Include branch names or local generation IDs

Branch names move and local generation IDs are not portable. Commit object IDs,
exact worktree snapshots, and source evidence provide durable identity.

## Consequences

### Positive

- Production code receives one complete, bounded, versioned contract.
- Exact source evidence can be revalidated without treating names as identity.
- Concurrent edits, merges, tombstones, and semantic conflicts remain explicit.
- Canonical identity is independent of human YAML formatting.
- Phase 0 does not prematurely stabilize broad memory kinds or extension maps.

### Negative and risks

- The schema is deliberately verbose for one manual record.
- The strict YAML stack remains pre-1.0 and requires active dependency review.
- Source snapshots or Git objects may be unavailable, yielding honest
  indeterminate results.
- Exact evidence alone does not prove rename correspondence; the later
  precision-first correspondence step must abstain on ambiguity.
- Shared files cannot authenticate their own actor or assurance claims.
- Secret detection can produce false positives and requires a separately
  versioned policy before tool-written promotion.

## Validation

- Golden canonical JSON, digest, and generated-YAML fixtures.
- Property tests changing every semantic field and permuting every
  presentation-only feature.
- Duplicate, directive, tag, anchor, alias, merge, float, composite-key,
  malformed UTF-8, CRLF, deep nesting, scalar bomb, and unknown-field fixtures.
- Exact inclusive and one-over tests for every scalar, collection, canonical
  output, time, and memory bound.
- Filename/record mismatch, traversal, symlink, hard-link, and special-file
  tests.
- SHA-1, SHA-256, dirty worktree, shallow history, missing object, branch,
  merge, rebase, and force-push validity fixtures.
- Exact evidence revalidation, renamed symbol, ambiguous duplicate,
  meaning-changing edit, stale source, changed producer, and missing-source
  fixtures.
- Idempotent repeated import, divergent parents, reviewed merge, explicit
  tombstone, missing file, projection rebuild, crash recovery, and backup
  restore.
- Parser/canonicalizer fuzz targets and realistic-history resource
  measurements before the format is called production-ready.

## Follow-up

- Review and explicitly accept or revise this ADR before production code
  depends on version 1.
- After acceptance, add the focused schema document and golden vectors.
- Promote the parser dependencies from test-only to the narrow local boundary.
- Add pure domain values, an application import/revalidation use case, SQLite
  append-only projection, and thin CLI/MCP adapters in that order.
- Define the secret/promotion policy before `memory_manage` writes active shared
  records.
- Supersede this ADR for new record kinds or incompatible schema semantics;
  never edit version 1 in place after release.

## Supersession

None.
