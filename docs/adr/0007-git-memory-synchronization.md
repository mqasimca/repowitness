# ADR-0007: Use canonical, conflict-preserving Git-memory synchronization

- Status: Accepted
- Date: 2026-07-22
- Last reviewed: 2026-07-28
- Owners: Project maintainers
- Scope: `.code-memory/` serialization, version import, edits, conflicts, tombstones, and projection rebuild

## Context

[ADR-0003](0003-git-native-team-memory.md) selects Git as the transport and canonical store for initial team memory. A reliable implementation still needs rules for canonical serialization, concurrent edits, audit reconstruction, history rewrites, deletion, untrusted files, and idempotent SQLite projection.

Naive last-write-wins would silently replace team claims. Treating missing files as deletion could erase knowledge after a sparse checkout or rewritten branch. YAML without canonicalization can also produce noisy diffs and unstable digests.

## Decision

### Files and canonical form

- Store the current representation at `.code-memory/records/<record-id>.yaml`.
- Record IDs are immutable and filenames must match validated IDs.
- Each record includes schema version, display revision, zero or more parent revision digests, scope, content, provenance, assurance, lifecycle, temporal events, evidence, relationships, and tombstone state.
- YAML is the human-facing representation, not the digest format. Parse into a strict versioned DTO, validate it, and then construct domain values.
- Tool-written YAML uses UTF-8, LF endings, deterministic key ordering, normalized paths, and stable formatting. Semantically equivalent human formatting must still produce the same digest.
- Reject duplicate keys, custom tags, aliases/anchors, merge keys, floats, traversal, over-budget nesting/counts/scalars, and unknown fields except where a schema version explicitly permits them.
- Calculate a cryptographic content digest over a domain-separated, versioned canonical JSON representation of the validated semantic object, excluding fields explicitly defined as transport metadata. Do not hash YAML bytes.
- Reject symlinks inside `.code-memory/` by default.

### Import and projection

- Identify an imported version by record ID, canonical content digest, and source commit/worktree snapshot.
- Import is idempotent.
- Materialize every observed version into append-only `memory_versions` and `memory_audit` rows.
- The active state for currently reachable Git history and current worktree files is a derived, reproducible projection. Every rebuild records the refs, history depth, missing objects, worktree snapshot, and coverage it actually inspected; it never labels partial history as a complete rebuild.
- Previously imported versions remain in the local append-only journal according to retention policy when their Git commits become unreachable. They cannot be reconstructed after database loss unless the objects become reachable again or a verified SQLite backup/export retained them.
- SQLite is therefore disposable for current-state indexing, but not for local continuity of previously observed, unreachable audit history. Long-term or organizational audit guarantees require an explicit backup/export or later archival profile.
- Schema, scope, path, secret, actor, provenance, and approval policy are validated before a version becomes active.

### Updates and conflicts

- An ordinary update supplies the expected current digest as its one parent and increments the display revision. Parent digests, not the display number, enforce optimistic concurrency and version identity.
- If the expected digest is not current, return a conflict rather than overwrite.
- Divergent Git versions remain separate conflicted candidates until a reviewed merge creates a new version referencing every chosen parent digest.
- Semantic conflicts are preserved even if Git can merge the YAML text automatically.
- Deletion uses an explicit tombstone version. A missing file is diagnosed and never treated by itself as authorization to erase audit history.
- Rewritten or pruned history does not delete previously imported local versions. Queries and rebuilds report `indeterminate` historical coverage when required Git objects or retained observations are unavailable rather than fabricating continuity.

### Actor semantics

Local stdio approval records a configured local identity as locally asserted. Remote approval, when implemented, binds the actor to the authenticated principal and authorization decision. A record cannot claim stronger authentication than the operation supplied.

## Alternatives considered

### Last-write-wins file import

Simple but can discard reviewed team knowledge and makes conflicting decisions invisible.

### Append one file per event forever

Produces a clear immutable log but may create excessive file counts and poor ordinary review ergonomics. Git history plus append-only SQLite versions provides the initial compromise.

### Store only current YAML state

Easy to query but cannot answer recorded-time questions reliably and weakens audit reconstruction.

### Treat Git merge success as semantic merge

Textual merge says nothing about whether two decisions, scopes, or validity events are compatible.

## Consequences

### Positive

- Deterministic diffs, hashing, import, and rebuild behavior.
- No silent semantic last-write-wins.
- Explicit tombstones and retained audit versions preserve observed history without treating absence as deletion.
- Shared memory remains portable and reviewable without a server.

### Negative and risks

- Importing Git history can be expensive for large record counts.
- Divergent history and semantic conflicts need user-facing review tools.
- Canonical YAML rules must remain compatible across versions.
- Git history alone does not guarantee long-term organizational audit retention after force pushes, pruning, repository deletion, or loss of the local database and its backups.

## Validation

- Canonical round-trip and cross-platform newline/path fixtures.
- Semantically identical YAML formatting produces the same canonical semantic digest.
- Duplicate keys, aliases, tags, merge keys, floats, unknown fields, and resource-exhaustion inputs are rejected as specified.
- Repeated import idempotency.
- Concurrent edit with matching and stale previous digests.
- Textually mergeable but semantically conflicting edits.
- Explicit tombstone versus missing/sparse file.
- Projection deletion and reproducible rebuild from a declared set of reachable refs, current files, and history depth.
- Shallow clone, force push, rebase, and pruned-history fixtures proving retained observations are not silently deleted and missing coverage is explicit.
- Backup/export and restore fixtures proving which unreachable observed versions survive database loss.
- Secret, symlink, traversal, forged-actor, and malformed-record cases.
- Scale tests with realistic record counts and Git histories.

## Open questions

- Maintained strict YAML parser and exact RFC 8785 compatibility/profile.
- Whether unknown fields are rejected for all record schemas or preserved only in explicitly versioned extension maps.
- How much Git history is required before import and how users request missing objects.
- The portable archival/export format and default retention period for previously observed versions whose Git objects become unreachable.
- Whether signed commits or record signatures become an optional organization assurance signal.

## Implementation status

The accepted ADR-0014 domain model, hostile byte parser, canonical semantic
digest, and deterministic writer are implemented, with the earlier spike kept
as an independent regression oracle. Capability-contained worktree admission,
trusted local import, immutable versions including tombstones, and append-only
audit history are implemented under proposed ADR-0017. Proposed ADR-0018 and
ADR-0021 now implement conflict-preserving current projection, deterministic
projection rebuild, observation-only bounded Git-tree history import,
separately trusted local approval, canonical writes, and manual correspondence
review. The force-push/pruned-object release matrix and portable archival/export
policy remain open.

## Supersession

None.
