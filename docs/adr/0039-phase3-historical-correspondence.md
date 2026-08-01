# ADR-0039: Preserve reviewed multi-parent and historical correspondence

- Status: Accepted
- Date: 2026-07-31
- Owners: Project maintainers
- Scope: Team-memory merges, correspondence review, archival reads, and bitemporal queries

## Context

ADR-0007 defines parent-digest concurrency and conflict preservation. ADR-0021
implements ordinary single-parent writes and intentionally rejects reviewed
multi-parent merges. It also binds review to an active source generation, which
is correct for a current revalidation but insufficient for reviewing preserved
historical evidence or asking what the local system knew at an earlier time.

Choosing one divergent head, treating a textual Git merge as a semantic merge,
or reconstructing historical knowledge from the current projection would hide
conflict or introduce hindsight.

## Decision

A Phase 3 team merge is an explicit canonical record revision with two through
eight distinct parent digests. The writer accepts it only when every selected
parent is an observed version of the same record and every selected parent is a
current unresolved head at the writer's final containment and identity fence.
The new display revision is greater than every parent display revision. It does
not infer semantic compatibility from Git's text merge. The local approval
operation remains separate from writing the merge.

Correspondence review gains an archival form. It binds the exact memory
revision and source occurrence to an exact target occurrence in an immutable
indexed source snapshot. An archival review never changes the current
projection unless that snapshot is the query's pinned source. Missing retained
generations, source objects, or history are reported as unavailable or partial
coverage; no historical target is guessed from a name or current file.

`as-known-at` reads select immutable audit and correspondence events whose
recorded timestamp is at or before the supplied timestamp, then evaluate the
requested concrete Git revision or worktree snapshot independently. Results
therefore expose both recorded-time coverage and project-validity coverage.
They do not use current approvals, current head selection, or later review
events to rewrite a past answer. Equal timestamps are ordered by immutable
event ID. A timestamp is not a Git selector; branch names resolve to a concrete
revision before evaluation.

Historical queries are bounded by records, events, source snapshots, object
lookups, bytes, deadline, and cancellation. They return a retained-coverage
category (`complete`, `partial`, or `unavailable`) independently from the
memory's validity and correspondence states. Archive retention may omit a
result, but cannot silently convert that omission into "no matching memory."

## Alternatives considered

### Automatically merge all current heads

This creates a false resolution for incompatible decisions and bypasses the
human review required by ADR-0007.

### Answer historical reads from the latest projection

This leaks later knowledge into an earlier view and makes rewritten history
look continuous.

### Store correspondence only against current files

That discards useful reviewed historical relationships and prevents auditable
as-known-at answers.

## Consequences

### Positive

- Git and semantic conflicts remain visible until an explicit reviewed merge.
- Historical answers can distinguish what was known from what applies now.
- Archived source evidence remains attributable without overclaiming coverage.

### Negative and risks

- Historical source retention consumes local storage and increases query work.
- Multi-parent review is a powerful mutation and needs focused authorization,
  secret scanning, recovery, and concurrency tests.

## Validation

- Two-parent successful merge, stale parent, missing parent, non-head parent,
  and concurrent-replacement fixtures.
- Historical review, retained/missing generation, rewritten/pruned Git, and
  equal-timestamp deterministic-order fixtures.
- Tests proving a future approval or review never appears in an earlier
  `as-known-at` result.
- Rebuild and backup/restore tests preserving conflicts and archival coverage.

## Follow-up

- Extend contained canonical writes and the immutable local journal.
- Add CLI/MCP historical query receipts and scoped archive reporting. The local
  reader now evaluates retained worktree snapshots against pre-cutoff direct
  observations and non-conflicted archival correspondence reviews. A Git
  commit still requires a bounded adapter-owned object/reachability check
  before it can be reported applicable; a journal-only Git receipt remains
  explicitly unavailable rather than inferring current Git validity.

## Supersession

This supersedes ADR-0021's deferred multi-parent-write limitation. Other
ADR-0021 trust and review rules remain accepted.
