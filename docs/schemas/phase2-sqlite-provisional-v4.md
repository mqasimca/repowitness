# Phase 2 SQLite schema version 4

- Status: Accepted persistence boundary
- Governing decisions: [ADR-0012](../adr/0012-phase0-sqlite-schema-and-ownership.md),
  [ADR-0026](../adr/0026-connected-workspace-source-slots-and-views.md),
  [ADR-0035](../adr/0035-phase2-scip-precision-overlay.md), and
  [ADR-0037](../adr/0037-phase2-scip-overlay-source-slot-scope.md)
- Compatibility: fresh databases and exact supported versions 1 through 4
- Accepted predecessor: [Phase 1 SQLite schema version 3](phase1-sqlite-provisional-v3.md)

Migration 4, `phase2_scip_precision_overlay`, has SHA-256 checksum
`20cb9211ca11c4041b48b6c287b72c714396eb853d11de41aff36cbd52ad23d8`.
It is an additive, immediate migration; migration 1 through migration 3 remain
byte-identical accepted history.

## Immutable SCIP overlay receipt

`scip_overlay_receipts` is a content-addressed, provider-provenance receipt
with the one-way lifecycle:

```text
staging -> complete
```

It binds the exact connected workspace, immutable workspace view, source slot,
source epoch, generation workspace/generation, source snapshot and manifest,
resolved configuration, producer, reviewed SCIP schema/importer, and raw input
digest. The schema admits it only when all of that source scope is an exact
member of a published workspace view. It stores only fixed-width digests and
opaque identities; no producer root, host path, source text, or user query is
persisted in the receipt.

`scip_overlay_documents`, `scip_overlay_occurrences`, and
`scip_overlay_relationships` contain the bounded immutable payload. A document
must match an exact `generation_files` path/content pair for the receipt's
generation. Completion checks declared global and per-document counts plus
contiguous document, occurrence, and relationship ordinals. An occurrence may
have no producer symbol and retains its explicit zero role bits; rows are accepted
only while their receipt is staging and cannot be edited or removed after it
is complete.

`retention_scip_overlay_garbage` is the only destructive authority for a
complete receipt. During the existing immediate retention transaction, the
writer first marks every complete overlay owned by a candidate source
generation, then deletes its pointer, facts, documents, and receipt before
deleting that generation. The mark is constrained to the exact retention-plan
digest and cascades away with the receipt; rollback leaves the complete overlay
and its pointer unchanged. Both receipts and pointers participate in the
retention-plan root digest, so an import or pointer switch invalidates an
already computed plan rather than allowing it to collect newly attached
evidence.

## Activation

`active_scip_overlays` permits one pointer for each connected-workspace/source
slot pair. Insertion and replacement require a complete receipt whose view,
slot, epoch, generation, and published member all match exactly. Pointer
identity cannot change and a pointer cannot be deleted. A failed or cancelled
writer transaction therefore leaves the prior pointer readable. The sole
exception is the matching `retention_scip_overlay_garbage` mark during an
atomic collection of an expired source generation; that deletion cannot occur
for a member of an active workspace view.

The pointer stores its workspace view explicitly. Readers must select the
pointer only when its view is the requested immutable view; an overlay for a
retained view never silently supplies evidence for a newer active view.

## Package-scoped evidence reads

The local reader resolves an opaque SCIP symbol only against the exact active
overlay selected by a caller's pinned view and source slot. It returns source
validated occurrence spans and explicitly attributed incoming/outgoing
relationships in deterministic document and source order. A caller supplies a
versioned `PackageScope`; explicit roots are compared as repository-byte
component boundaries inside that one source slot. This is scope filtering, not
package-manager inference: no producer root, build file, dependency graph, or
second source slot is consulted.

Occurrence and relationship limits are independent. The result reports each
truncation independently; a missing overlay and a scope-local no-match are
separate categorical outcomes. The reader rejects a result that would exceed
its declared encoded-output budget rather than silently omitting evidence.

## Validation

Schema tests cover migration checksums/catalogues and verify that a wrong-path
document, incomplete overlay, post-completion mutation, and mismatched
activation are rejected. The owned-worker test proves atomic activation, exact
idempotent replay, nullable producer-symbol preservation, cancellation without
pointer replacement, read-only status through the matching pinned view, and
atomic retention of an expired overlay with its source generation (including
stale-plan rejection after an overlay import), as well as a two-document
package-scoped source/target relationship read. Recovery, CLI/MCP, and
end-to-end provider tests remain required Phase 2 gates.
