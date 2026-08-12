# ADR-0059: Use a fast source-only catalog refresh during development

- Status: Proposed
- Date: 2026-08-12
- Owners: Project maintainers
- Scope: local indexing and Codex catalog startup

## Context

RepoWitness is still a development-stage local tool. A catalog MCP process
currently blocks startup while it resolves and persists the complete Rust
syntax graph. On the current repository this takes about 13 seconds on a cold
state directory and produces a large graph projection even when the first
operation only needs source search or context discovery.

The SQLite publication contract already permits a source generation without a
graph publication. Such a generation reports graph availability as
`not_produced`; it does not claim graph evidence or silently use an older graph.

## Decision

Catalog refreshes use a fast source-only indexing profile. They retain atomic
source facts, raw syntax sites, and repository topology, but skip Rust graph
analysis, resolution, candidate persistence, and graph staging.

Explicit normal `index` operations continue to build the complete graph. This
keeps graph production opt-in at the expensive boundary without adding a new
storage backend, background service, or compatibility migration.

The source-only profile is also available to connected-workspace catalog
refreshes. The graph state remains explicit and generation-pinned; callers see
`not_produced` until an explicit full index creates a graph for a generation.

## Alternatives considered

### Keep graph work on the startup path

This preserves immediate graph availability but makes the common catalog path
pay the full graph cost and grows private development databases quickly.

### Add a graph cache or separate graph database

This adds another lifecycle and recovery surface. The existing immutable SQLite
schema already represents absent graph output correctly.

### Build graphs in an unbounded background task

This improves perceived startup but introduces task ownership, shutdown, and
status semantics before demand exists. Explicit full indexing is simpler and
observable.

## Consequences

- Catalog startup is source-first and avoids the largest CPU and write cost.
- Search, symbol retrieval, raw syntax, topology, diagnostics, and context
  remain available with explicit coverage.
- Graph-specific reads can legitimately return `not_produced` after a catalog
  refresh; users who need graph evidence run the normal full index command.
- Existing graph-capable indexes remain readable until a new source-only
  generation is activated.
- No database migration is required; development indexes can be recreated.

## Validation

- Source-only indexing has no graph requirement or publication rows.
- Searchable source facts remain available.
- A later normal full index publishes a complete graph.
- Existing graph status and read contracts remain unchanged.
