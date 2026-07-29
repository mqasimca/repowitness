# ADR-0021: Complete Phase 0 memory management through explicit local trust

- Status: Proposed
- Date: 2026-07-27
- Last reviewed: 2026-07-28
- Owners: Project maintainers
- Scope: Git-memory writes, history import, local approval, correspondence review, and MCP authorization

## Context

[ADR-0007](0007-git-memory-synchronization.md) makes reviewed files under
`.code-memory/records/` the canonical shared-memory transport.
[ADR-0017](0017-phase0-memory-journal.md) and proposed
[ADR-0018](0018-phase0-memory-revalidation.md) implement strict record
admission, an append-only SQLite journal, and precision-first revalidation. At
proposal time, the production surface could not write, import, approve, or
review memory.

The existing import port also couples observation and approval. That is safe
for a specifically authorized local import, but unsafe for Git-tree traversal:
repository-authored history must never approve itself. A write surface must
also prevent stale overwrites, path escape, secret promotion, unbounded Git
work, and model-initiated approval without an explicit local capability.

The implementation described below now exists in the CLI, local adapter,
SQLite schema version 8, and explicitly authorized local MCP adapter. Its
release matrix and clean benchmark prerequisite pass. This ADR remains
proposed because it depends on ADR-0018, whose real design-partner
prerequisite is not complete.

## Decision

### One bounded management use case

Add a versioned `memory_manage` use case shared by the CLI and local stdio MCP.
Its operations are explicit and independently authorized:

- `write` validates one complete external version-1 YAML record, scans every
  promotable text field with the fixed high-confidence secret policy, verifies
  repository scope and optimistic parent state, emits canonical YAML, and
  atomically replaces only the matching
  `.code-memory/records/<record-id>.yaml` path;
- `approve` capability-reads the current exact record, appends its worktree
  observation plus a separately supplied locally asserted approval actor, and
  never derives that actor from repository text;
- `import-history` walks a declared, bounded reachable history selector,
  imports exact Git-tree record blobs as observations only, and reports
  inspected commits and records, admitted bytes, Git-process count, and
  explicit completeness;
- `review` appends one exact approve, reject, or manual-link event over a
  specific memory evidence occurrence and current target occurrence.

The existing read-only `diagnostics` use case supplies repository and
projection status. Keeping status out of the mutation tool avoids a second
overlapping read contract.

All mutation operations use one cancellation signal and absolute deadline.
Counts, bytes, process invocations, parser work, and results are bounded.
Partial coverage is explicit and never presented as a complete import.

### Observation is not approval

The application import request carries an explicit `observed_only` or
`locally_approved` policy. Both append an exact observation. Only the latter
may append a trusted approval, and only after the actor is supplied by the
authorized local boundary.

Git-tree import always uses `observed_only`. Replaying or discovering a
repository-authored record can therefore preserve history but cannot activate
the claim. Approval is a separate operation over the exact canonical revision.

### Canonical and conflict-preserving file writes

The write boundary opens the repository as a capability root and rejects
symlinks, reparse points, hard-link aliases, special files, alternate filename
spellings, and path traversal. It creates `.code-memory/records/` without
following links and writes through a same-directory unique temporary file,
flushes file content, then performs one atomic rename and directory sync where
the platform supports it.

A new record requires no existing file and no parent. An ordinary update
requires exactly one parent equal to the current canonical revision and a
display revision exactly one greater than the current presentation. A
tombstone follows the same update rule and remains an explicit version.
Reviewed multi-parent merges are deferred; the command rejects them rather
than choosing a winner.

The writer re-reads the current contained file immediately before publication.
Any missing, replaced, or changed identity produces a conflict and leaves the
existing file untouched. Database import is a separate idempotent operation so
filesystem and SQLite failure cannot masquerade as one atomic commit.

### Fixed secret and promotion policy

Phase 0 rejects promotion when canonical record text contains a
high-confidence credential form: a private-key block, a supported provider
token prefix, or an assignment/JSON/YAML field whose normalized key is
`password`, `passwd`, `secret`, `client_secret`, `access_token`,
`refresh_token`, `api_key`, or `private_key` and whose value is nonempty.

The scanner checks title, body, provenance actor, evidence names, qualified
names, and producer text before writing or approving. It never includes the
matched value in an error or diagnostic. There is no bypass flag in Phase 0;
users must replace sensitive material with a non-secret description.

This policy is deliberately high precision and is not described as a complete
secret detector. Review and repository policy remain required.

### Bounded Git-tree import

The default history selector is the concrete commit resolved from `HEAD`.
Phase 0 accepts no caller-controlled revision syntax through MCP. The CLI may
accept one validated full object ID only.

The adapter invokes sanitized Git without a shell. By default it enumerates at
most 257 commits to admit a maximum of 256 in deterministic oldest-first
order. Across all admitted commits it accepts at most 4,096 record
observations, with a total 64 MiB blob budget and the existing per-record YAML
limit. Each Git command has a 5-second deadline and 16 MiB captured-output
bound. Tree entries must be ordinary blobs at canonical record paths whose
filename matches the parsed ID. Blob reads use object identity, never checkout
filters or worktree paths.

An exceeded commit, entry, byte, process, deadline, cancellation, shallow, or
missing-object bound stops before claiming complete coverage. Successfully
committed observations remain valid append-only history and retries are
idempotent.

### Review semantics

Review events bind the exact record revision, evidence ordinal, historical
source occurrence, target source snapshot, target path, artifact, and fact
ordinal. The writer verifies both occurrences and the current active source
generation before append.

One or more identical approvals are idempotent. One unopposed approved or
manual target establishes reviewed correspondence. Rejections only remove the
exact rejected target; they never cause an automatic “last candidate wins.”
Competing approved targets, or approval and rejection of the same target,
produce `indeterminate`. A review for an older target snapshot remains in the
audit journal but does not affect a newer projection.

Projection rebuild consumes all matching review events before automatic
correspondence. It records `reviewed` assurance and the exact target when one
unopposed link exists. Review cannot make invalid Git-DAG validity current,
override an authored non-active lifecycle, or hide incomplete source coverage.

### Local MCP authorization

The stdio server remains path-confined to its configured repository and
database. `memory_manage` is exposed only when startup receives an explicit
validated local actor and `--enable-memory-writes`; otherwise the server keeps
the existing read-only tool set.

The tool cannot choose a host path, database, repository identity, actor,
timestamp, deadline, history ref, or resource limit. Its write operations are
marked non-idempotent or destructive as appropriate. Tool results contain
receipts and redacted coverage only. Enabling the capability is local
authorization, not proof that repository-authored actor claims are genuine.

## Alternatives considered

### Automatically approve every reachable Git record

This would make setup convenient, but lets repository text manufacture its own
trust and turns a checkout into approval. History import therefore observes
only.

### Let `memory_manage` accept partial claim fields

Generating records from many flags looks friendly but duplicates the versioned
schema, encourages model-authored promotion, and makes evidence selection
ambiguous. Phase 0 admits one complete strict record and provides exact
selectors separately.

### Mutate the Git file and SQLite journal in one command

There is no atomic transaction across the filesystem and SQLite. A crash could
still leave one side committed. Separate idempotent write and approval/import
operations make recovery explicit.

### Select the latest review by timestamp or row ID

That silently discards conflicting trusted decisions. Aggregating all exact
events and failing closed preserves audit history.

### Expose write-capable MCP unconditionally

Local path confinement alone does not authorize approval or repository
mutation. An explicit startup capability and fixed actor make the trust
boundary visible and default deny.

## Consequences

### Positive

- Git history can be reconstructed without self-approval.
- Shared-memory writes are deterministic, conflict preserving, and
  capability-contained.
- Manual review can resolve precision-first abstention without weakening
  automatic correspondence.
- CLI and opt-in MCP use the same bounded application and local paths.
- Secret-bearing inputs fail closed before promotion.

### Negative and risks

- Users author or generate a complete strict record before `write`; Phase 0
  does not provide a conversational record generator.
- The fixed secret scanner can miss novel credentials and reject some
  secret-looking examples.
- History import performs bounded Git subprocess work proportional to admitted
  commits and record versions.
- Review audit grows append-only and needs a later retention/export policy.
- Opt-in MCP mutation remains powerful and depends on the operator protecting
  local process and configuration access.

## Validation

Implemented tests cover:

- observation-only history with no approval, exact retry idempotency, bounded
  commit coverage, shallow-history partial coverage, malformed records, and
  cancellation before persistence;
- create, ordinary update, stale parent, explicit tombstone, concurrent
  replacement, symlink, hard-link, special-file, deadline, and cancellation
  behavior;
- secret-policy positives and negatives with redacted request and error
  diagnostics;
- approved, rejected, manual-link, repeated, conflicting, and invalid-target
  review events with deterministic revalidation;
- CLI and opt-in MCP authorization defaults, schemas, annotations, path
  confinement, bounds, redaction, process transport, and output ceilings; and
- one public end-to-end fixture and one pinned benchmark that write and approve
  a decision, change its source, revalidate, recall, and rebuild context.

Completed release-matrix tests cover rewritten, pruned, and missing-object Git
history; obsolete review snapshots; multiple competing approved targets;
explicit split/merge abstention; and deterministic failure at every
canonical-file and SQLite publication stage, including transaction commit.

## Follow-up

- Completed 2026-07-29: accept ADR-0017 and ADR-0019 after their complete
  Phase 0 validation matrices passed.
- Completed 2026-07-29: rerun the pinned benchmark from a clean exact
  RepoWitness revision on Ubuntu 24.04 and ratify its budgets.
- Decide this proposal after the real design-partner comparison permits a
  decision on ADR-0018. Do not change the accepted baseline migration in
  place.
- Add reviewed multi-parent merge and archival/export policy in a later
  decision.

## Supersession

The pre-release schema-version and migration-compatibility clauses are
superseded by [ADR-0022](0022-squash-pre-release-sqlite-schema.md). The local
trust, memory-management, and correspondence-review proposal remains otherwise
unchanged.
