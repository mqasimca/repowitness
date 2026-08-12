# ADR-0059: Use a fast source-only onboarding profile during development

- Status: Accepted
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

Private onboarding uses a fast source-only indexing profile by default. It
retains atomic source facts, raw syntax sites, and repository topology, but
skips Rust graph
analysis, resolution, candidate persistence, and graph staging.

Explicit normal `index` operations continue to build the complete graph. This
keeps graph production opt-in at the expensive boundary without adding a new
storage backend, background service, or compatibility migration.

`onboard --full` and normal `index` explicitly request complete graph
production. The graph state remains explicit and generation-pinned; callers
see `not_produced` until an explicit full index creates a graph for a
generation.

Catalog startup applies the same source-first idea to an existing connected
workspace: it performs a bounded source-only revalidation, and keeps the
current immutable graph view when every source still matches. A changed source
falls back to the normal atomic full graph publication path.

The local snapshot producer identity includes the current graph artifact
identity, so graph semantic changes cannot silently keep an older graph view.

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

- Private onboarding is source-first and avoids the largest CPU and write cost.
- Search, symbol retrieval, raw syntax, topology, diagnostics, and context
  remain available with explicit coverage.
- Graph-specific reads can legitimately return `not_produced` after source-only
  onboarding; users who need graph evidence run the normal full index command.
- Existing graph-capable indexes remain readable until a new source-only
  generation is activated; this is acceptable because development indexes are
  disposable.
- Unchanged connected-workspace startup does not create a new view or rebuild
  its graph; changed workspaces retain the existing full refresh behavior.
- No database migration is required; development indexes can be recreated.

## Validation

- Source-only indexing has no graph requirement or publication rows.
- Searchable source facts remain available.
- A later normal full index publishes a complete graph.
- CLI onboarding defaults to source-only and reports `index_profile`; `--full`
  requests graph production.
- Existing graph status and read contracts remain unchanged.
- Repeated startup on an unchanged connected workspace retains the active view
  and skips graph reconstruction; source changes still use the full path.
