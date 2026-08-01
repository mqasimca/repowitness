# Phase 1 SQLite schema version 3

- Status: Accepted persistence boundary
- Governing decisions:
  [ADR-0012](../adr/0012-phase0-sqlite-schema-and-ownership.md),
  [ADR-0024](../adr/0024-persist-parser-diagnostics-migration.md),
  [ADR-0026](../adr/0026-connected-workspace-source-slots-and-views.md),
  [ADR-0027](../adr/0027-phase1-rust-syntax-graph.md),
  [ADR-0028](../adr/0028-reconciliation-watching-and-source-epochs.md), and
  [ADR-0029](../adr/0029-bounded-generation-retention-and-garbage-collection.md)
- Compatibility: fresh databases and exact supported ledgers through current
  version 6
- Accepted predecessor:
  [Phase 0 SQLite schema version 2](phase0-sqlite-current-v2.md)

> Migration 3 contains the accepted workspace and Rust graph fragments plus
> the accepted bounded-retention lifecycle. Its exact checksum is part of the
> supported persistence boundary.

## Identity

The database uses:

- `PRAGMA application_id = 0x52575031` (`RWP1`);
- `PRAGMA user_version = 6`;
- the byte-identical accepted migration 1 and migration 2 names, text, and
  checksums documented by the version-2 predecessor; and
- accepted migration 3
  `phase1_workspace_graph_and_retention_foundation`, assembled in fixed order from
  workspace, graph tables and row guards, graph completion guards, and retention
  fragments; and
- byte-identical compatible migrations 4 through 6, including migration 6
  `linear_graph_site_completion_validation`.

At the current graph-and-retention-enabled state, migration 3 has SHA-256
checksum
`b2cc733ce8ebd2d23e33126257ec4092b7adf4dcddb864d9201251aa2717fcd8`.
This vector is an accepted corruption/reopen guard. A checksum change requires
a new versioned migration and an explicit decision.

Fresh creation applies all six migrations. An exact version-1 or version-2
database upgrades through the fixed forward migration chain. Existing
version-2 repository workspaces are backfilled with a byte-identical default
connected-workspace ID and source-slot ID plus the repository's existing
source epoch. A workspace with an active generation also receives an immutable
slot-epoch completion receipt, one published default view carrying that epoch,
and an active-view pointer. Source, generation, memory, approval, review, and
audit rows are not rewritten.

## Compatible current-schema migrations

The current writer applies the exact six-entry migration ledger. Migration 6
has SHA-256 checksum
`74bea1fde365ed169934bdcfe3033c313f833913fc729e643bbecce19090c7d2` and
replaces only the graph-artifact completion trigger. It preserves the required
zero-based contiguous site ordinals by comparing the site count with the
maximum ordinal plus one. The `rust_graph_sites` primary key makes ordinals
unique and its `CHECK` constraint makes them nonnegative, so this is equivalent
to the former correlated check while avoiding quadratic work for large graph
artifacts. Historical migration text and checksums remain unchanged.

## Connected workspaces and source slots

`connected_workspaces` stores only one opaque 32-byte identity.
`workspace_source_slots` maps a globally unique opaque 32-byte source-slot ID
to:

- exactly one connected workspace;
- exactly one logical 32-byte repository identity; and
- exactly one existing generation workspace; and
- one nonnegative fixed-width monotonic `source_epoch`.

The schema contains no source root, filesystem path, branch, ref, or selector
text. Those values remain bounded configuration or pre-persistence selection
inputs. The same logical repository may be mapped by multiple distinct source
slots. Membership is bounded to 256 slots and freezes after the first view;
changing it requires a new connected-workspace identity.

The owned writer exposes idempotent exact-membership registration. Existing
single-repository registration creates the default mapping automatically, so
its source-slot bytes remain equal to its repository identity while the Rust
types remain distinct.

`source_slot_generation_receipts` immutably binds one source-slot epoch to one
eligible generation in the slot's mapped generation workspace. Receipt
insertion succeeds only while that epoch remains current. A generation may be
bound independently to multiple source slots, including slots for the same
logical repository; the primary key remains the slot and epoch, so their
histories do not merge. Source-slot epochs may advance only by exactly one,
and receipts cannot be updated or deleted.

## Immutable workspace views

`workspace_views` has the one-way persisted lifecycle:

```text
staging -> published
```

`workspace_view_members` contains exactly one ordered member for every source
slot. Each member names the slot's current source epoch and one generation
belonging to that slot's mapped generation workspace. Publication requires an
exact immutable completion receipt for the member's slot, epoch, and
generation; accepts only `ready`, `active`, or `retained` generations; and
validates canonical ascending source-slot order. Published view rows and
members cannot be updated or deleted.

`active_workspace_views` is the only representation of active-versus-retained
view status. Its primary key permits one pointer per connected workspace, and
schema triggers permit the pointer to reference only a published view. This
avoids a redundant lifecycle value that could disagree with the pointer.

The owned writer creates the view and members, publishes the complete view,
and switches the pointer in one immediate transaction. Every
connected-workspace membership or view command accepts an explicit cancellation
signal and absolute deadline and rechecks both during bounded traversal and
before commit. Failure rolls back the entire attempt and preserves the previous
pointer. A read command captures the pointer and all bounded, canonically
ordered members in one SQLite snapshot.

Ready generations selected by an active workspace view or by a completion
receipt for a source slot's current epoch are pinned across startup recovery.
The latter closes the crash window between durable completion and atomic view
publication so a restarted supervisor can finish publishing the exact
candidate. Older receipts do not pin superseded ready generations. A
generation selected by the active pointer cannot transition to `failed` or
`cancelled`.

## Immutable Rust graph publication

`rust_graph_artifacts` and `rust_graph_sites` contain reusable, content-bound
Rust site extraction output. Graph artifacts have a one-way
`staging -> complete` lifecycle through their owning `analysis_artifacts` row.
Completion requires the exact declared, contiguous site count, and complete
artifact metadata and sites cannot be updated or deleted.

Generation-owned graph projection uses:

- `generation_graph_requirements` to distinguish a graph-required generation
  from a legacy generation where graph output was not produced;
- `generation_graph_publications` for the immutable complete receipt, including
  resolver profile, input/output digests, source/artifact/definition/site and
  categorical outcome counts, candidate and edge counts, bounded byte
  accounting, and syntax/macro/test/heuristic coverage;
- `generation_graph_sources` and `generation_graph_artifacts` to bind the exact
  connected-workspace source slots, generations, paths, content, and reusable
  graph artifacts; distinct paths with identical content may reference the
  same content-local graph artifact;
- `generation_graph_definitions` for exact declaration identities backed by
  generation files and syntax facts;
- `generation_graph_resolutions` for one explicit unresolved, unique, or
  ambiguous outcome per exact site;
- `generation_graph_candidates` for deterministic retained candidates with
  attributed evidence; and
- `generation_graph_edges` for the unique import, reference, and call subset.

All projection rows are accepted only while the publication is staging.
Completion validates exact counts, zero/one/many candidate cardinality, and
unique-edge coverage. A generation carrying a graph requirement cannot become
active unless its graph publication is complete. Failure, cancellation, or a
stale source epoch therefore leaves the previous active generation and view
readable.

Native local reads pin one immutable published workspace view and graph-owning
generation. They distinguish `not produced` from corruption, validate the
receipt and exact source set, apply caller and resolved-configuration bounds,
and expose exact symbol identities, categorical site evidence, count-only
architecture summaries, deterministic graph traces, and conservative inbound
impact. Trace input includes every bounded retained unique or ambiguous
candidate with its exact originating site and evidence. Independent depth,
node, edge, frontier, and result truncation plus generation-level coverage
remain inspectable. Source bytes and persisted raw-target text remain behind
the contained filesystem capability rather than the SQLite graph API.

## Bounded generation retention

Migration 3 represents explicit garbage lifecycle transitions in typed
`retention_*_garbage` mark relations. This avoids rewriting accepted
migration-1 and migration-2 lifecycle `CHECK` constraints while preserving the
same guarded transition: only a retained generation, complete unreferenced
snapshot or artifact, inactive published view, or superseded source-slot
receipt may receive a mark. Existing immutability triggers permit deletion only
while the exact parent is marked. The append-only
`retention_collection_audit` relation stores only policy and plan digests,
aggregate counts, estimated bytes, remaining-work state, and the categorical
outcome.

The owned writer exposes separate read-only plan and explicit apply operations.
The default policy preserves the active generation and at least the two newest
retained generations per source slot. Bounded explicit generation pins,
supervised-task pins, and immutable-view pins are resolved before planning.
Active workspace pointers and views, current source-slot receipts, memory
projections, memory evidence, append-only memory and correspondence audits,
external graph-source references, and every snapshot or artifact they reach
fail closed as roots.

Planning uses one consistent deferred snapshot, canonical source-slot,
source-epoch, and generation ordering, hard candidate/row/estimated-byte
limits, cooperative cancellation, and an absolute deadline. The row ceiling is
one shared logical-work budget: root and candidate rows are consumed during
planning, worst-case mark/delete work and the audit row are reserved in the
digest-bound plan, and apply consumes those reservations rather than starting
a second budget. Plans expose aggregate root count, unresolved candidate count,
conservative unresolved truncation, and total logical work. Applying opens one
immediate transaction, recomputes the complete bounded plan, rejects a stale
digest, marks the exact candidate set, deletes dependents in foreign-key order,
and records one aggregate receipt before commit. An exact apply replay returns
the prior receipt. Before a new mark phase begins, every transient mark
relation must be empty; a foreign-plan mark blocks apply without deleting or
auditing anything. Cancellation, timeout, constraint failure, or process
termination rolls back the batch. Startup revokes any externally persisted
stale marks without deleting their targets; marks are never treated as
standalone deletion authority.

Collection removes both FTS slots for deleted generations but does not run
`VACUUM` or promise immediate file shrinkage. Shared immutable snapshots and
artifacts remain until no generation, graph, memory, evidence, or audit root
reaches them.

## Validation

Automated validation covers:

- unchanged migration-1 and migration-2 checksum vectors;
- the accepted migration-3 checksum and complete catalog vector;
- fresh version-3 creation, populated version-2 upgrade, exact reopen, ledger,
  integrity, and foreign-key checks;
- two logical repositories in one connected workspace;
- two source slots for one logical repository across restart;
- the same generation completed independently for multiple source slots;
- exact-successor reservation, stale compare-and-set, epoch exhaustion, and
  out-of-order completion/publication rejection;
- deterministic member ordering and the 256-slot bound;
- globally unique source slots and frozen membership;
- incomplete, duplicate, wrong-repository, and ineligible-generation
  rejection;
- failed publication rollback, successful atomic pointer switching, and no
  committed staging residue;
- graph publication reopen, immutability, cancellation, exact receipt counts,
  categorical evidence, view-pinned symbol search, architecture summary,
  unique and ambiguous trace/impact, candidate and traversal truncation,
  old-generation reads, corruption rejection, configuration/input/output
  bounds, cancellation/deadline behavior, and redacted request diagnostics;
- deterministic retention planning and bounded multi-workspace apply,
  active/current/floor/generation/task/view/memory/evidence/audit roots, shared
  artifacts, exact and one-over shared-row boundaries, byte/candidate limits,
  cancellation, deadline, stale-plan rejection, explicit root/unresolved/work
  metrics, full multi-source-view row estimates, exact replay idempotency,
  no-op audit, foreign-key integrity, foreign-plan mark rejection, stale-mark
  restart recovery, and redacted policy/request diagnostics;
- direct attempts to point at staging views, publish incomplete views, mutate
  published views, or create redundant retained lifecycle state; and
- path-free schema, redacted debug, stable errors, recovery, backup, and the
  complete local test suite.
