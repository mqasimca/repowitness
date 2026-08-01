# ADR-0038: Separate Phase 3 team, personal, and archival memory

- Status: Accepted
- Date: 2026-07-31
- Owners: Project maintainers
- Scope: Engineering-memory kinds, scope isolation, local persistence, and archival retention

## Context

The accepted Phase 0 record deliberately supports only reviewed shared
`decision` and `failure` claims. It also establishes that team records are
Git-reviewable while personal memory remains local. Phase 3 needs procedures,
episodes, preferences, policies, and durable non-source facts without turning
repository text into a channel for personal data or weakening the existing
version-1 parser and journal invariants.

Git alone cannot provide a durable answer for a locally observed record after a
force-push or pruning event. Conversely, keeping personal memory in
`.code-memory/` would expose it through normal Git, source, MCP, backup, and
review paths. These are different trust and retention domains.

## Decision

Phase 3 retains the version-1 team record exactly as accepted by ADR-0014 and
introduces a separately versioned record profile for additional kinds. The
profile admits `fact`, `procedure`, `episode`, `preference`, and `policy` in
addition to the existing `decision` and `failure` kinds. It preserves bounded
claims, immutable revisions, typed evidence, explicit validity, lifecycle,
relationships, canonical digests, and append-only audit. A procedure is never
eligible as verified guidance solely because it was authored: it requires a
separate successful verification receipt under ADR-0040.

Every durable record has one immutable visibility scope:

- **team** records use the canonical `.code-memory/records/<id>.yaml` Git
  transport, normal shared-memory approval, and the reconstructible reachable
  history projection from ADR-0007;
- **personal** records live only in an owner-controlled local SQLite store,
  keyed by the local profile and repository identity. They are never written
  to the worktree, Git history, default diagnostics, or a team-memory export;
- **archive** is not another active-authoring scope. It is a retained immutable
  observation of an exact team or personal version that is no longer necessarily
  reachable from Git. Archive reads report retention and source-object coverage
  and never claim current applicability merely because an observation exists.

The local profile identity, local store location, and encryption-at-rest choice
are composition inputs rather than repository-controlled configuration.
Personal records use a local actor and local approval policy; they cannot
assert team approval. Team and personal record IDs, audit rows, projections,
queries, and exports remain partitioned even when they cite the same repository
and source occurrence. A query requests personal inclusion explicitly and the
result labels each scope. Context construction and MCP remain team-only unless
the startup composition explicitly enables a fixed local personal profile.

All scopes use explicit lifecycle states. `active`, `needs_review`, `stale`,
`contradicted`, `superseded`, `quarantined`, and `tombstoned` retain their
accepted meanings. TTL is represented as a lifecycle transition recorded by an
audited policy evaluation, never as a wall-clock deletion or an implicit
rewrite of a historical version.

## Alternatives considered

### Put personal notes beside team records

This would make accidental publication and cross-user visibility likely, and
would let a repository control an owner's private memory path.

### Widen the version-1 parser in place

Version 1 is a released hostile-input and canonical-digest contract. Changing
its accepted values would make historical validation ambiguous.

### Treat unreachable Git objects as durable archival proof

An observed row is useful provenance, but it cannot prove that a missing object
or rewritten branch remains reachable or applicable.

## Consequences

### Positive

- Existing shared records retain exact compatibility.
- Personal knowledge has a hard storage and retrieval boundary.
- New kinds can carry explicit lifecycle and verification policy.
- Retained history stays useful without fabricating current Git coverage.

### Negative and risks

- Personal memory is intentionally not portable through a normal Git clone.
- Separate projections and exports add implementation and test surface.
- Local encryption is optional; operators remain responsible for host and
  backup protection unless they enable an encrypted local store.

## Validation

- Version-1 canonical vectors remain byte-for-byte unchanged.
- New-profile parser, canonicalizer, writer, migration, and mutation tests.
- Team/personal query, export, diagnostics, backup, and MCP isolation tests.
- Secret, poisoning, profile-substitution, rewritten-history, and archive
  coverage fixtures.
- Procedure eligibility tests proving a missing or failed verification excludes
  the procedure from verified guidance.

## Follow-up

- Add the compatible local schema and scoped application ports.
- Add explicit scoped recall and context receipts.
- Implement archival historical reads under ADR-0039 and task verification
  under ADR-0040.

## Supersession

This extends ADR-0003 and ADR-0014 without changing their version-1 or
Git-native-team-memory decisions.
