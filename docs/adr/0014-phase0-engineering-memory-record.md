# ADR-0014: Define a strict Phase 0 engineering-memory record

- Status: Accepted
- Date: 2026-07-26
- Last reviewed: 2026-07-28
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

Adopt the following Phase 0 version-1 semantic record. The exact field and
presentation profile plus byte-exact vectors are recorded in the
[Phase 0 memory schema](../schemas/phase0-memory-v1.md).

### File and record identity

- Store one current record at
  `.code-memory/records/<record-id>.yaml`.
- `record_id` is `mem_` followed by exactly 26 characters from the uppercase
  Crockford Base32 alphabet `0123456789ABCDEFGHJKMNPQRSTVWXYZ`. The first
  encoded character is `0` through `7`. Encode by prepending two zero bits to
  the 128-bit value, splitting the resulting 130 bits into five-bit groups from
  most significant to least significant, and mapping those groups through the
  alphabet. Decoding validates and removes those two zero bits. The ID is an
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

The Phase 0 writer samples all 128 ID bits uniformly from an operating-system
cryptographic random source behind an injected byte-source boundary. It does
not use a timestamp, time-seeded generator, path, repository contents, or
record contents. Tests inject exact bytes. A collision with any current or
previously observed record ID fails closed; a bounded create operation may
sample a new ID before publication. Import never generates or rewrites an ID.

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
- `symbol_kind`: `function`, `method`, `struct`, `enum`, `union`, `trait`,
  `module`, `type_alias`, `constant`, `static`, or `macro`;
- relationship kinds `contradicts` and `supersedes`.

The importer treats `actor_id` as a locally asserted label, never as an
authenticated organization principal. It cannot upgrade assurance based only
on file contents.

`scope.repository_id` uses the canonical ADR-0013 `rwi1:h:` encoding.
`scope.subject_evidence` must select an existing evidence entry and is `0` in
the Phase 0 writer. A record has one through sixteen evidence entries.

Every SHA-256 text field other than the separately tagged repository identity
is exactly 64 lowercase ASCII hexadecimal characters. This includes parent and
relationship revision digests, source-snapshot, content, analysis-artifact,
declaration, and worktree-snapshot digests. Repository identity and path text
retain the uppercase tagged encodings accepted by ADR-0013 and ADR-0011.

Rust symbol evidence preserves the exact observed source snapshot, canonical
ADR-0011 repository path, content and analysis-artifact digests, deterministic
fact ordinal, symbol kind and names, byte spans, SHA-256 digest of the exact
declaration bytes, and producer identity. Import reconstructs validated domain
types and checks that:

- the name and declaration spans are within the declared Phase 0 source limit;
- the name span is contained by the declaration span;
- `name_length` equals the UTF-8 byte length of `name`;
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
- `kind: worktree` has exactly `kind` and one `source_snapshot_digest`
  containing the canonical source-snapshot SHA-256 digest.

A commit ID stores `object_format` as `sha1` or `sha256` and a lowercase object
ID of exactly 40 or 64 hex characters respectively. Commit lists are sorted by
object format and decoded object bytes. Introduction and invalidation sets are
disjoint. A dirty-worktree record cannot also claim descendant commit
semantics; a later reviewed version rebinds it to commit validity.

Each relationship contains `kind`, `record_id`, and `revision_digest`.
There are zero through sixteen relationships. They are sorted by that tuple and
are unique. When referenced versions are available, a parent must belong to
the same `record_id` and a relationship target must belong to the same
repository. Missing versions produce explicit version-history or relationship
coverage; they never fabricate a match. A tombstone requires `lifecycle:
tombstoned`, `tombstone: true`, and at least one parent digest. Every other
lifecycle requires `tombstone: false`. Missing files never create a tombstone.
A tombstone remains a complete semantic version and is never eligible as an
active claim. It does not redact an earlier Git or SQLite version.

### Authored state, effective state, and recorded time

The YAML `provenance`, `assurance`, and `lifecycle` fields are authored claims.
They cannot grant authority or force retrieval eligibility. The effective
local state is a derived projection that may preserve or reduce those claims
but never upgrade them based on repository bytes.

An exact record version is eligible as active only when:

- its authored lifecycle is `active` and it is not a tombstone;
- a trusted local audit event binds the repository ID, record ID, canonical
  revision digest, approval operation, and configured local actor;
- scope, secret, and local policy accept the exact version;
- its version-DAG and Git-ancestry coverage are sufficient to establish that
  it is an unconflicted current candidate and project-valid at the query state;
  and
- exact evidence revalidation or reviewed correspondence supports the claim.

A new canonical revision digest requires a new approval. Missing parents,
history, source, or correspondence produce `indeterminate`, `unresolved`,
`needs_review`, `stale`, or `quarantined` effective state as appropriate, never
implicit activation.

ADR-0005 `recorded_at`/`recorded_until` and the audit actor, origin, and
operation are trusted append-only observation metadata, not repository-authored
YAML and not canonical record semantics. The application supplies them through
injected clock, identity, and operation boundaries when importing or approving
a version. Git author and committer timestamps remain evidence about Git, not
the system-recorded clock.

### Bounds and text rules

- Input is UTF-8, LF-only, one YAML document, and at most 64 KiB.
- Raw preflight permits at most 4,096 parser events, 2,048 data nodes, nesting
  depth 8, and one document.
- Decoding permits at most 48 KiB of aggregate data-scalar bytes and 4 KiB of
  aggregate comment bytes.
- Reject directives, every explicit tag, anchors, aliases, merge keys,
  duplicate keys, composite keys, floats, non-strict booleans, unknown fields,
  and implicit scalar-to-string conversions.
- `title` is 1 through 256 UTF-8 bytes, contains no NUL or line break, and is
  not trimmed or Unicode-normalized.
- `body` is 1 through 16 KiB of UTF-8 and contains no NUL or carriage return.
- `actor_id`, `producer_id`, and `producer_version` are each 1 through 128
  printable ASCII bytes.
- `name` is 1 through 256 bytes and `qualified_name` is 1 through 1,024 bytes.
  Both are UTF-8 and contain no NUL, carriage return, or line feed.
- `display_revision` is a nonzero `u32`. Evidence indexes, fact ordinals, byte
  offsets, and byte lengths are nonnegative integers no greater than
  9,007,199,254,740,991, the RFC 8785 interoperable integer maximum.
- Name and declaration lengths are nonzero.
- Name and declaration span addition is checked for overflow and both spans
  end at or before the Phase 0 per-file hard limit of 8 MiB.
- Canonical and decoded repository path limits are the existing ADR-0011
  limits. Arrays have the independent item bounds above. Canonical JSON is at
  most 256 KiB, and the deterministic writer must not emit a YAML file over the
  64 KiB input limit.
- Parser, validation, canonicalization, import, Git traversal, and projection
  each have explicit deadlines and cancellation.

Diagnostics identify only the failing field category and bound. They do not
include title, body, actor, source names, repository paths, YAML snippets, or
secret-like values.

### Canonical semantic digest

Construct a serialization-only DTO from validated values. Its object and field
shape is exactly the YAML semantic shape above; the worktree variant is
`{"kind":"worktree","source_snapshot_digest":"..."}` and the commits variant
contains exactly `kind`, `introduced_by`, and `invalidated_by`. Exclude only
`display_revision`; all other fields above are semantic. Sort every set-like
array as specified, preserve evidence order because
`scope.subject_evidence` indexes it, and serialize with the RFC 8785 JSON
Canonicalization Scheme.

Reject canonical output over 256 KiB. Hash the canonical UTF-8 bytes with
SHA-256 over this exact byte concatenation:

```text
UTF8("RepoWitness\0memory-record\0")
|| U32_BE(1)
|| U64_BE(canonical_json_byte_length)
|| canonical_json
```

The domain result is `CanonicalMemoryDigest`. YAML bytes, comments, key order,
scalar style, and `display_revision` do not participate. A changed semantic
field must change the digest.

### Parser and canonicalizer

Use the exact reviewed stable versions promoted from the independent spike:

- `serde-saphyr` 0.0.29 with only `deserialize`;
- `granit-parser` 0.0.7 with default features disabled;
- `serde_json_canonicalizer` 0.3.2.

Keep `serde-saphyr` and `granit-parser` inside the local hostile-file format
adapter, and keep Serde/YAML DTOs out of domain APIs. Keep the independent
raw-event preflight. Typed deserialization alone is not a security boundary
because the spike proved that an explicit custom tag could be consumed before
DTO validation. The deterministic YAML writer consumes validated domain values
only and has a separately versioned, golden-tested presentation profile.

As of 2026-07-27, the YAML stack has a 1.0.0 release candidate but no stable
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
- Every new semantic revision requires local approval before it can become
  active in a fresh installation.
- Tombstones suppress current retrieval but cannot remove secrets from retained
  Git or SQLite history.
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

### Acceptance evidence

On 2026-07-27, the format gate passed with:

- pure domain values and a local-only hostile-format adapter, with no
  Serde/YAML types in the domain API;
- byte-exact commit and worktree golden YAML, canonical JSON, record-ID, and
  canonical-digest vectors shared with the independent spike oracle;
- strict hostile-input, semantic mutation, presentation-invariance, scalar,
  collection, cancellation, deadline, redaction, and inclusive/one-over output
  tests;
- a standalone [coverage-guided parser/writer fuzz target](../../fuzz/README.md)
  that completed 1,226,220 executions in 61 seconds with no crash, timeout, or
  invariant failure and a 47 MiB peak RSS;
- release probes covering 10,000 ordinary iterations and 1,000 maximum-size
  inputs with no failure; and
- clean Clippy, workspace dependency policy, and `cargo-deny` advisory,
  license, ban, and source results for the locked production graph.

The local Apple stable toolchain did not include the ASan runtime, so the
recorded campaign used libFuzzer coverage instrumentation with
`--sanitizer none`. The durable target defaults to the normal cargo-fuzz
sanitizer when run with a supported nightly toolchain.

## Follow-up

- Completed 2026-07-27: add capability-contained worktree admission, the
  scope-checked application import use case, and the SQLite v5 append-only
  journal under ADR-0017.
- Completed 2026-07-27: add SQLite v6 revalidation/current projection,
  current-memory recall, bounded context compilation, diagnostics, and thin
  CLI/MCP adapters under ADR-0018 and ADR-0019.
- Completed 2026-07-28: add bounded observation-only Git-tree history
  admission, bounded manual correspondence review, and deterministic
  conflict-preserving aggregation under proposed ADR-0021.
- Completed 2026-07-28: add the fixed high-confidence secret/promotion policy,
  contained canonical writes, explicit local approval, and default-deny
  write-capable MCP under proposed ADR-0021.
- Completed 2026-07-28: pass the rewritten/missing-history, obsolete-review,
  competing-target, split/merge, and canonical-file/SQLite publication-fault
  release matrix.
- Supersede this ADR for new record kinds or incompatible schema semantics;
  never edit version 1 in place after release.

## Supersession

None.
