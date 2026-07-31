# ADR-0037: Scope each SCIP overlay to one source slot in a pinned workspace view

- Status: Accepted
- Date: 2026-07-31
- Owners: Project maintainers
- Scope: Phase 2 SCIP import requests, immutable overlay identity, SQLite
  persistence/activation, package-aware reads, CLI, and local MCP

## Context

ADR-0035 requires an imported precision overlay to be pinned to one completed
connected-workspace view and its exact source state. A SCIP `Index` has one
`Metadata.project_root`, and every `Document.relative_path` is relative to that
single producer root. A connected RepoWitness workspace can contain multiple
independent source slots/repositories, each with a distinct source root,
manifest, snapshot, generation, and path namespace.

Treating a SCIP path as globally unique in a connected workspace would allow
the same relative path in two source slots to be associated with the wrong
bytes. Inferring a slot from producer metadata or the host filesystem would
expand the trust boundary and bypass the explicit workspace-view contract.

## Decision

One Phase 2 SCIP import targets exactly one `SourceSlotGeneration` member of
one immutable published `PinnedWorkspaceView`. Its source manifest and exact
source-byte lookup are obtained only from that member. The imported overlay
identity and receipt include the connected workspace, workspace view, source
slot, source epoch, source generation, source snapshot, source manifest,
resolved configuration, producer provenance, reviewed schema/importer, and
exact SCIP input digest.

The resulting document paths are unique only within that source slot. An
overlay never resolves a producer path into another slot, joins equal relative
paths across slots, or makes a cross-repository package claim. A later command
may import a separate explicit artifact for another member of the same view;
the reader selects only overlays whose complete scope matches the requested
view and source slot.

An import stages all document batches and facts transactionally. It may publish
an active-overlay pointer only after the exact member remains in the same
published view at the final fence. A failed, cancelled, stale, or ambiguous
import leaves the previous pointer readable. The local stdio MCP server remains
read-only and can expose only an already selected immutable overlay.

## Alternatives considered

### One overlay spanning every connected-workspace source slot

This would need a trusted producer-to-slot mapping even though SCIP paths have
only one project root. It makes same-path collisions and mixed-generation
admission harder to prevent.

### Infer a source slot from producer project-root metadata

Producer metadata is hostile and may contain arbitrary host paths. It is not
an authorization to read or select a RepoWitness source root.

### Treat each import as a repository-global mutable cache

This loses the workspace-view/source-epoch fence and can apply an old index to
current bytes.

## Consequences

### Positive

- The existing immutable source-manifest and generation validation boundaries
  can be reused without cross-slot path ambiguity.
- Source-slot and workspace-view mismatches become categorical availability or
  coverage outcomes instead of heuristic relinking.
- One explicit SCIP artifact has a small, reviewable persistence and read
  scope.

### Negative and risks

- Multi-repository navigation requires multiple explicit imports and cannot
  claim package completeness across source slots.
- Overlay activation/retention must retain the pinned workspace-view member
  while it is active, and must collect an expired overlay atomically with its
  no-longer-retained source generation.
- CLI and storage requests gain additional exact scope fields.

## Validation

- Fixtures with identical relative paths in two slots prove that an import can
  affect only its selected member.
- Workspace-view/source-epoch changes between staging and final fence preserve
  the prior active overlay.
- Reopen, crash, retention, and read tests prove a selected overlay cannot
  reference a removed, incomplete, or mismatched member. Retention must mark
  and delete an expired overlay before deleting its source generation, while
  a new overlay invalidates any previously computed retention plan.
- Package-aware query fixtures prove that unsupported cross-slot claims remain
  explicit unresolved coverage.

## Follow-up

- Define the versioned SQLite overlay receipt, documents, occurrences,
  relationships, coverage, activation, and retention roots.
- Add a contained CLI local-file capability and application publication port.
- Define package-aware resolution and MCP/CLI result schemas on top of this
  source-slot scope.

## Supersession

None. This narrows the implementation representation left open by ADR-0035
and composes with ADR-0026 and ADR-0031.
