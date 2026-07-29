# ADR-0026: Model connected workspaces through source slots and immutable views

- Status: Proposed
- Date: 2026-07-29
- Owners: Project maintainers
- Scope: Workspace identity, repository selection, generation publication, and SQLite migration 3

## Context

The current local store models one logical repository as one SQLite
`workspaces` row with one active generation. That is sufficient for the Phase
0 single-repository command surface, but it cannot represent a connected
workspace containing multiple repositories. It also cannot represent two
simultaneous worktrees, revisions, or selectors for the same logical
repository without making them compete for one active-generation pointer.

Repository roots and Git ref names are unsuitable persisted identities. Roots
are host-specific authorization inputs, while branch and ref names are mutable
selectors. Persisting either as identity would leak local paths, make a
workspace non-portable, or let a moving selector silently change the snapshot
behind an answer.

[ADR-0025](0025-versioned-local-configuration-and-policy.md) defines how local
configuration is bounded, resolved, explained, and redacted. This decision
defines the identity and publication model that consumes those resolved
configuration inputs; it does not add configuration discovery, a watcher, or
new CLI behavior.

## Decision

### Four explicit cardinalities

Model the relationship as:

```text
one connected workspace
    -> one or more source slots
        -> exactly one logical repository per slot
            -> exactly one concrete generation/snapshot per active view
```

A connected-workspace ID and a source-slot ID are separate validated domain
types containing exactly 32 opaque bytes. Their canonical version-1 text
encodings are tagged uppercase Base16:

- connected workspace: `cwi1:h:` followed by 64 uppercase Base16 digits;
- source slot: `ssi1:h:` followed by 64 uppercase Base16 digits.

The values are never inferred from a host path. Their debug and error
representations expose only kind, version, and bounded lengths.

A source-slot ID is globally unique within one database. It belongs to exactly
one connected workspace and maps immutably to exactly one logical
`RepositoryIdentityDigest` plus its existing SQLite generation workspace. One
logical repository may be mapped by multiple distinct source slots. This is
how two linked worktrees, two selected revisions, or two policy profiles for
the same repository coexist without changing logical repository identity.

For backward compatibility, the single-repository path uses the repository
identity bytes as both its default connected-workspace ID and its default
source-slot ID. The Rust types remain distinct even when the bytes are equal.

### Selectors resolve before persistence

Root paths are configuration and authorization inputs only. They are not
stored in connected-workspace, source-slot, or workspace-view rows and are not
included in default logs, diagnostics, debug output, or public errors.

Branch names, tag names, and arbitrary Git refs are selectors. Before a source
slot is indexed, its selector resolves to a concrete Git object identity and
the complete source-state receipt required by ADR-0013. Per ADR-0005, a
selector never substitutes for a commit in temporal-validity evaluation.
Dirty state remains tied to the exact content-digested snapshot.

### Slot epochs and completion receipts

Each source slot owns one durable fixed-width monotonic source epoch. Reserving
the exact successor is a compare-and-set operation; stale reservations and
exhausted counters fail closed. A completed reconciliation creates an
immutable receipt that binds exactly one `(connected workspace, source slot,
source epoch)` tuple to one eligible generation.

A generation may be completed independently for multiple source slots,
including two slots mapped to the same logical repository. Those receipts do
not merge slot identity or epoch history. Reusing one generation across slots
therefore remains explicit and attributed rather than becoming an implicit
repository-wide active pointer.

### Immutable workspace views

An active connected-workspace read pins one immutable workspace view. A view
contains exactly one member for every source slot in that connected workspace.
Each member names the source slot's current epoch and one existing generation
belonging to the slot's mapped logical repository. Members are stored in
ascending source-slot byte order so the representation and returned order are
deterministic.

A view may become active only when:

1. its membership is complete and contains no duplicate or unknown slot;
2. every member epoch equals the source slot's current durable epoch;
3. an immutable completion receipt binds that exact slot and epoch to the
   selected generation;
4. every generation belongs to the generation workspace mapped by that slot;
5. every generation is in `ready`, `active`, or `retained` state; and
6. the configured per-workspace source-slot bound is satisfied.

Publication uses one immediate SQLite transaction. A persisted view has the
one-way lifecycle `staging -> published`; “active” is represented only by the
separate active-view pointer, not by a redundant lifecycle value. The
transaction creates the view and members, validates the complete set,
publishes the new view, and switches the connected workspace's active pointer.
The bounded writer operation checks explicit cancellation and its absolute
deadline before work, throughout member traversal, and before commit. The
previously selected published view becomes retained by virtue of no longer
being selected. Cancellation, timeout, or any other failure rolls the whole
transaction back, so readers observe either the prior complete view or the new
complete view, never a mixed set.

View membership and source-slot mappings are immutable after the first view is
published. Adding or removing slots then requires a new connected-workspace
ID; changing one slot's logical meaning also requires a new source-slot ID.
Explicit retirement and garbage collection are proposed by
[ADR-0029](0029-bounded-generation-retention-and-garbage-collection.md), but
that lifecycle remains a separate explicit maintenance operation. Migration 3
does not silently delete historical views during indexing.

### Compatible schema migration 3

Preserve accepted migrations 1 and 2 byte for byte. Migration 3 is assembled,
in fixed order, from Phase 1 workspace, graph, and retention fragments. The
workspace fragment adds:

- connected-workspace membership;
- source-slot-to-logical-repository and generation-workspace mappings with
  durable source epochs;
- immutable source-slot epoch-to-generation completion receipts;
- immutable workspace views and their ordered members; and
- one atomic active-view pointer per connected workspace.

The graph fragment persists immutable syntax-graph sites, outcomes, and typed
edges. The retention fragment adds typed plan-scoped garbage marks and
aggregate collection audit state. ADR-0026 and the migration-3 checksum remain
provisional until the complete assembled migration and its upgrade/recovery
evidence are reviewed before either decision or migration is accepted.

Migration 3 backfills every version-2 repository workspace with its
byte-identical default connected workspace and source slot. If that repository
has an active generation, it also backfills the source epoch, its exact
completion receipt, and a one-member active view carrying that epoch. The
upgrade is compatible and does not rewrite source, generation, memory,
approval, or review rows.

## Alternatives considered

### Treat each worktree as a logical repository

This avoids a new source-slot concept, but duplicates durable repository
identity and breaks memory and Git-DAG correspondence across worktrees of the
same repository.

### Store one active generation per logical repository

This is the current model. It cannot pin two revisions or worktrees of the same
logical repository in one answer.

### Persist root paths or branch names as source-slot identity

Paths leak host details and are not portable. Branches and refs move. Both
would make historical answers depend on mutable or private presentation
values.

### Read each repository's latest active generation independently

Independent pointer reads can mix publication moments. An immutable workspace
view gives every multi-repository answer one explainable atomic input set.

### Update workspace members in place

In-place mutation loses the exact source set behind earlier evidence and makes
crash recovery ambiguous. Append-only views preserve auditability and allow a
single atomic pointer switch.

## Consequences

### Positive

- Multi-repository reads have one immutable, explainable input set.
- The same logical repository can appear through multiple worktrees or
  revisions without identity collision.
- Single-repository callers remain compatible through the default source slot.
- Moving refs and host paths cannot silently become persisted source identity.
- Failed publication preserves the previous complete view.

### Negative and risks

- Publication adds another immutable object and active pointer.
- Slot membership changes require new identities and the separate, still-
  proposed ADR-0029 retirement and garbage-collection policy. Its development
  implementation remains release-gated pending ratification and evidence.
- Existing per-repository queries are not automatically multi-repository; thin
  adapters must explicitly accept a pinned workspace view.
- The provisional migration-3 checksum must not be treated as accepted until
  the graph and retention fragments are finalized.

## Validation

- Golden, malformed, exhaustive-byte, and redacted-debug tests for both
  canonical ID codecs.
- Fresh version-3 and populated version-2 upgrade fixtures, including exact
  migration ledger/checksum and reopen behavior.
- Two logical repositories in one connected workspace.
- Two source slots mapped to one logical repository.
- Independent completion receipts that bind one generation to two source
  slots without merging their epoch histories.
- Out-of-order completion and view publication reject a stale member epoch
  while preserving the prior active view.
- Rejection of incomplete, duplicate, over-limit, wrong-repository, and
  ineligible-generation views.
- Pre-cancelled and expired-deadline operations append no workspace or view
  state.
- Deterministic member ordering independent of input order.
- Atomic pointer-switch and forced-failure rollback fixtures proving the old
  view remains readable.
- Backward-compatible single-repository registration and activation fixtures.
- Catalog, debug, and error checks proving the new persisted and diagnostic
  surfaces contain no repository root path.

## Follow-up

- Review the complete Phase 1 graph and retention migration fragments before
  accepting migration 3 or this ADR.
- Ratify the implemented resolved-configuration, exact-selector,
  package-scope, watcher-reconciliation, and bounded-retention contracts after
  their Phase 1 release evidence passes.

## Supersession

None. This extends ADR-0005, ADR-0006, ADR-0012, ADR-0013, and ADR-0025 without
changing their accepted identity, temporal-validity, publication, or privacy
requirements.
