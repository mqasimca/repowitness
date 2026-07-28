# ADR-0018: Revalidate Phase 0 Rust memory through precision-first correspondence

- Status: Proposed
- Date: 2026-07-27
- Last reviewed: 2026-07-28
- Owners: Project maintainers
- Scope: Rust occurrence correspondence, Git-DAG validity, memory effective
  state, and SQLite projection publication

## Context

The implemented version-1 memory record cites one or more exact Rust syntax
occurrences. SQLite version 5 preserves those immutable records and trusted
audit events, but it intentionally does not decide which approved version is
current or whether its evidence still applies to the active source generation.

[ADR-0004](0004-logical-symbol-identity.md) requires precision-first
correspondence with explicit ambiguity, and
[ADR-0005](0005-git-dag-temporal-memory.md) requires Git-DAG validity to become
indeterminate when ancestry is unavailable. A path, qualified name, declaration
digest, or similarity score alone is not a durable symbol identity. False
automatic relinks are more dangerous than abstention.

Memory version 1 stores an exact declaration digest but no rename-stable
fingerprint. Rewriting that accepted schema would invalidate canonical memory
identities. The source index can instead derive a versioned, attributed
fingerprint from immutable source bytes and persist it as analysis output.

## Decision

### Phase 0 boundary

Implement correspondence and current-memory projection only for version-1
`rust_symbol` evidence and one active local source generation. Go, TypeScript,
and TSX remain searchable source evidence but cannot be memory subjects until a
separate record-schema decision defines their evidence contract.

The revalidation operation receives:

- one exact workspace and active index generation;
- its complete source-snapshot identity;
- a concrete Git commit target, or an exact worktree snapshot plus its concrete
  HEAD commit when one exists;
- the immutable approved memory-version journal;
- a versioned correspondence profile; and
- explicit cancellation, deadline, record, candidate, subprocess-output, and
  result bounds.

Readers pin one immutable memory-projection generation. A rebuild stages every
row, validates counts and coverage, and switches one workspace pointer only
after the complete projection is ready. Failure, cancellation, a changed source
epoch, or a replaced active index generation leaves the previous projection
readable.

### Derived occurrence fingerprints

Rust syntax analysis derives two standard SHA-256 values from validated source
bytes:

1. the exact declaration digest; and
2. `rust-name-elided-v1`, a domain-separated encoding of the symbol kind,
   exact syntactic container, declaration bytes before the declared name, one
   fixed marker, and declaration bytes after the name.

The name-elided fingerprint removes only the declaration's validated name span.
It does not normalize whitespace, comments, types, literals, nested names, or
the body. The exact container is the qualified name without its terminal symbol
component. A producer/profile change invalidates calibration and artifact reuse.

The existing artifact fact remains the source of path, kind, name, qualified
name, and spans. SQLite version 6 adds an immutable companion fact row rather
than editing migrations 1 through 5.

### Precision-first correspondence

Candidate enumeration is deterministic, complete within its declared bound, and
ordered by exact repository path, artifact digest, and fact ordinal. Exceeding
the bound produces `indeterminate`; it never truncates into a unique match.

The automatic Phase 0 rules are:

1. **Exact occurrence:** the current occurrence has the cited path, kind, name,
   qualified name, and exact declaration digest.
2. **Same-path rename:** exactly one current occurrence has the cited path and
   kind, the same syntactic container, and the same `rust-name-elided-v1`
   fingerprint, while its name changed.
3. **Exact move:** exactly one current occurrence has the cited kind, name,
   qualified name, and exact declaration digest at a different path, and
   sanitized Git reports exact path continuity between the evidence commit and
   target commit.

Only these rules have automatic assurance. A move plus rename, more than one
matching fingerprint, split/merge, copy, container change, unavailable old
source, incomplete Git history, or candidate overflow does not auto-link.
Plausible bounded candidates become `needs_review`; unavailable evidence or
coverage becomes `indeterminate`.

If the exact cited path, kind, and qualified name still identify one occurrence
but its declaration and name-elided digests changed, the evidence is
meaning-changed and the memory becomes `stale`. A missing cited occurrence with
complete source and history coverage is also stale unless review candidates
exist.

Manual approval, rejection, or establishment of a correspondence is an
append-only trusted audit event over exact source and target occurrence
identities. A repository-authored record cannot approve its own
correspondence.

### Git-DAG validity

An ancestry adapter accepts raw SHA-1 or SHA-256 commit IDs without invoking a
shell. It disables ambient configuration, prompts, pagers, hooks, external
diffs, credential helpers, and optional locks; bounds stdout/stderr, runtime,
and process count; and returns only `ancestor`, `not_ancestor`, or
`indeterminate`.

For commit validity:

- any invalidation ancestor makes the version `not_applicable`;
- otherwise, at least one introduction ancestor and no indeterminate
  invalidation makes it valid;
- all introductions proven non-ancestors makes it `not_applicable`; and
- any missing object, incompatible object format, shallow boundary, cancelled
  query, or unresolved required ancestry makes validity `indeterminate`.

Worktree validity is valid only for the exact source-snapshot digest.
Commit-valid memory may be evaluated against a dirty worktree's concrete HEAD,
but source correspondence still uses the exact active worktree snapshot.

### Version heads and effective state

Only versions with a trusted `locally_approved` audit event are eligible.
Within one record, an approved version referenced as a parent by another
approved version is not a head. One complete head is evaluated. Multiple heads
are `conflicted`; a missing referenced parent makes head coverage
`indeterminate`. The system does not choose a winner by display revision,
timestamp, row ID, or lexical digest order.

Before source correspondence, authored lifecycle is honored:

- `tombstoned`, `contradicted`, `superseded`, `quarantined`, `stale`, and
  `needs_review` remain explicit effective states;
- only authored `active` versions can become source-current; and
- authored assurance becomes effective only because the trusted approval event
  exists.

The projection records the exact memory version, source/index generation,
correspondence profile, project-valid result, evidence result, categorical
effective state, bounded candidates, and complete/partial coverage. It stores
no source snippets.

### Forward-only SQLite version 6

Add a transactional schema version 6 containing:

- immutable occurrence-fingerprint companion rows;
- append-only correspondence-review audit events;
- immutable memory-projection generations, record results, evidence results,
  and bounded review candidates; and
- one atomically replaceable active-projection pointer per workspace.

The concrete columns, checks, triggers, and bounds are fixed in the
[schema-v6 document](../schemas/phase0-sqlite-v6.md). Migrations 1 through 5
and their checksums do not change.

## Alternatives considered

### Treat path and qualified name as durable identity

This makes ordinary renames and moves fail and can silently attach memory after
delete/reintroduce. They remain matching signals only.

### Use one fuzzy similarity threshold

It is easy to demonstrate on a small fixture but has no universal probability
meaning and can confidently mislink repetitive or generated code.

### Auto-link every unique name-elided fingerprint

A deleted symbol and an unrelated identical addition can look unique. Phase 0
therefore also requires same-path/container continuity or exact Git-backed move
continuity.

### Rewrite memory version 1 to add a fingerprint

That would change accepted canonical identities. Derived, versioned analysis
facts preserve the record contract and can be recalculated.

### Update one mutable current-memory table

Partial rebuilds and crashes could mix source generations. Immutable projection
generations plus one pointer preserve the existing publication invariant.

### Make missing Git objects mean not-an-ancestor

That converts incomplete history into false certainty and violates ADR-0005.

## Consequences

### Positive

- Name-only Rust renames and exact moves can retain reviewed memory without a
  broad heuristic.
- Meaning-changing edits cannot remain silently current.
- Ambiguity, conflicts, partial history, and bounded-work exhaustion remain
  visible.
- Projection rebuild and publication follow the already-tested immutable
  generation model.
- Accepted memory-record identities and prior SQLite migrations remain stable.

### Negative and risks

- The first profile intentionally misses container moves, move-plus-rename,
  formatting edits, and semantically equivalent rewrites.
- Old evidence needs verified historical source bytes to derive its fingerprint;
  unavailable bytes force abstention.
- Git ancestry and path-continuity subprocesses add bounded latency.
- Projection tables duplicate exact occurrence identity for auditability.
- The profile needs per-language replacement before non-Rust memory evidence is
  admitted.

## Validation

- Golden fingerprint vectors prove every retained byte and field participates,
  only the declared name may change, and malformed spans fail closed.
- Same-path rename, exact file move, unchanged declaration, body edit,
  signature edit, formatting edit, copy, delete/reintroduce, duplicate bodies,
  container move, move-plus-rename, split, and merge fixtures.
- No automatic relink in any ambiguous, copy, split/merge, incomplete-history,
  or candidate-overflow fixture.
- Linear, branch-specific invalidation, merge, cherry-pick, SHA-1, SHA-256,
  shallow, missing-object, rebase/force-push, and dirty-worktree validity
  fixtures.
- Approved linear heads, unapproved children, divergent approved heads, missing
  parents, tombstones, and every authored lifecycle.
- Migration-6 checksum and upgrades from versions 1 through 5.
- Projection cancellation, failure, stale source epoch, competing publication,
  startup recovery, online backup/restore, exact rebuild equivalence, and
  normalized-row corruption.
- Redaction tests prove paths, source, claim text, actors, queries, object IDs,
  and digests do not enter default diagnostics.
- The Phase 0 release fixture permits no known false automatic relink.

## Follow-up

- Completed 2026-07-27: implement the pure validity and Rust correspondence
  profiles, sanitized Git ancestry/path-continuity adapter, shared application
  and local revalidation path, SQLite version 6, and atomic projection
  publication.
- Completed 2026-07-27: expose bounded current-memory recall through the CLI and
  local stdio MCP.
- Completed 2026-07-28: add bounded approve, reject, and manual-link review
  commands, immutable SQLite audit, deterministic conflict aggregation, and
  reviewed projection evidence under proposed ADR-0021.
- Completed 2026-07-28: pass obsolete-snapshot, competing-target,
  split/merge-abstention, rewritten/missing-history, and projection-publication
  fault cases.
- Complete the comparative design-partner evaluation before ratification.

## Supersession

The pre-release schema-version and migration-compatibility clauses are
superseded by [ADR-0022](0022-squash-pre-release-sqlite-schema.md). The
correspondence, revalidation, and immutable-projection proposal remains
otherwise unchanged.
