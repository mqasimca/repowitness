# Current engineering-memory profile

- Status: Implemented
- Governing decision: [ADR-0057](../adr/0057-unified-engineering-memory-profile.md)

RepoWitness exposes one engineering-memory document format and one
`memory-manage` workflow. A document may omit `schema_version`; omission means
the current profile. New documents should therefore use the current profile
without carrying a version-selection concern in their hand-written YAML.

Existing documents with `schema_version: 1` remain readable and writable as a
legacy compatibility format. They are not a second user workflow: the parser,
canonical writer, approval flow, history import, and recall APIs accept both
forms through the same boundary. Explicit version 2 is also accepted and is
the canonical form emitted for a current-profile document.

The current profile admits `decision`, `failure`, `fact`, `procedure`,
`episode`, `preference`, and `policy`. It retains the same bounded evidence,
validity, immutable parentage, canonical digest, and append-only trust rules.
An active `procedure` still requires an independent successful verification
receipt before it can be treated as verified guidance; `policy` remains
descriptive and never grants execution authority.

The physical SQLite compatibility tables are an implementation detail. They
preserve existing version-1 records and their canonical digests while the
application presents one logical memory model.

For migration-sensitive or reproducibility-sensitive tooling, write the
version explicitly. For normal authoring, omit it:

```yaml
record_id: mem_00000000000000000000000000
display_revision: 1
parent_revision_digests: []
kind: fact
title: Example fact
body: The current profile is selected by omission.
scope:
  repository_id: rwi1:h:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
  subject_evidence: 0
provenance:
  origin: human
  actor_kind: local_asserted
  actor_id: author
assurance: locally_approved
lifecycle: active
validity:
  kind: worktree
  source_snapshot_digest: "8888888888888888888888888888888888888888888888888888888888888888"
evidence:
  - kind: rust_symbol
    source_snapshot_digest: "8888888888888888888888888888888888888888888888888888888888888888"
    path: src/lib.rs
    content_digest: "8888888888888888888888888888888888888888888888888888888888888888"
    artifact_digest: "8888888888888888888888888888888888888888888888888888888888888888"
    fact_ordinal: 0
    symbol_kind: function
    name: example
    qualified_name: example
    name_start: 0
    name_length: 7
    declaration_start: 0
    declaration_length: 7
    declaration_digest: "8888888888888888888888888888888888888888888888888888888888888888"
    producer_id: repowitness
    producer_version: "1"
relationships: []
tombstone: false
```
